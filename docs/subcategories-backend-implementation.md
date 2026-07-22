# Sub-Categories (Products) — Backend Implementation Guide

Complete, copy-ready code for the product-level farm data feature and the
community moderation workflow. **Dataset seeding is intentionally excluded** —
populating `product_categories`, `products`, and `farm_products` from the source
dataset is handled in a separate PR. This document covers only the *application
logic*: schema, domain types, read/write API, product search, and moderation.

Nothing here loads the real dataset. The taxonomy tables are assumed to be
populated out-of-band; the integration tests below seed their own tiny fixtures.

Conventions followed (from the existing codebase):

- Newtype domain types with `parse()` + `thiserror`.
- Thin Actix handlers; logic in service/query functions.
- Explicit SQLx queries; `error_chain_fmt` for `Debug`; `ResponseError` mapping.
- The idempotency module (`try_processing` / `save_response`) for writes.
- `CurrentUser` / `Role` for auth.

> After any migration: `sqlx migrate run` then
> `cargo sqlx prepare --workspace -- --all-targets`, commit `.sqlx/`.
> After any change: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.

---

## PR 1 — Schema

**Goal:** create the taxonomy + relationship tables. No code change; nothing reads
them yet. The `farms.categories` column stays for now (dropped in PR 3, once no
code references it).

### File: `migrations/<timestamp>_create_product_taxonomy.sql`

```sql
-- Category groups: the small, stable, admin-owned vocabulary.
CREATE TABLE product_categories (
    id            smallint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key_de        text     NOT NULL UNIQUE,   -- canonical identity, e.g. 'Früchte'
    slug          text     NOT NULL UNIQUE,   -- URL/API-safe, e.g. 'fruits'
    display_order smallint NOT NULL DEFAULT 0
);

-- Products (subcategories). Each belongs to exactly one group.
CREATE TABLE products (
    id          integer  GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    category_id smallint NOT NULL REFERENCES product_categories (id),
    key_de      text     NOT NULL UNIQUE,     -- 'Erdbeeren'
    slug        text     NOT NULL UNIQUE,     -- 'strawberries'
    name_en     text     NOT NULL             -- 'Strawberries'
);
CREATE INDEX products_category_id_idx ON products (category_id);

-- Which products a farm offers. The source of truth for product-level data.
CREATE TABLE farm_products (
    farm_id    uuid    NOT NULL REFERENCES farms (id)    ON DELETE CASCADE,
    product_id integer NOT NULL REFERENCES products (id) ON DELETE RESTRICT,
    PRIMARY KEY (farm_id, product_id)
);
-- The PK covers farm -> products; this covers product -> farms (product search).
CREATE INDEX farm_products_product_id_idx ON farm_products (product_id);
```

There is no Rust change in PR 1. Run `sqlx migrate run` and `cargo sqlx prepare`.

---

## PR 2 — Read API + product search

**Goal:** return `products` on each farm, derive `categories`, and support
filtering by product with keyset pagination.

### 2.1 The in-memory taxonomy snapshot

New module. Loaded once at startup; resolves a product `slug` to its `id` without
a DB round trip and gives an early `400` for unknown slugs.

> The snapshot is loaded at boot. Because the seeding PR populates the tables,
> start (or restart) the app *after* seeding so the snapshot is non-empty. A
> live-refresh path is out of scope here.

#### File: `src/taxonomy/mod.rs`

```rust
use sqlx::PgPool;
use std::collections::HashMap;

/// A single product plus the group it belongs to.
#[derive(Clone)]
pub struct ProductInfo {
    pub id: i32,
    pub slug: String,
    pub name_en: String,
    pub group_slug: String,
}

/// Read-mostly, in-process index of the product taxonomy.
#[derive(Clone, Default)]
pub struct TaxonomySnapshot {
    by_slug: HashMap<String, ProductInfo>,
    by_id: HashMap<i32, ProductInfo>,
}

impl TaxonomySnapshot {
    /// Load the whole taxonomy from Postgres. Cheap: at most a few hundred rows.
    #[tracing::instrument(name = "Load taxonomy snapshot", skip(pool))]
    pub async fn load(pool: &PgPool) -> Result<Self, anyhow::Error> {
        let rows = sqlx::query!(
            r#"
            SELECT
                p.id,
                p.slug,
                p.name_en,
                c.slug AS group_slug
            FROM products p
            JOIN product_categories c ON c.id = p.category_id
            "#
        )
        .fetch_all(pool)
        .await?;

        let mut by_slug = HashMap::with_capacity(rows.len());
        let mut by_id = HashMap::with_capacity(rows.len());
        for row in rows {
            let info = ProductInfo {
                id: row.id,
                slug: row.slug.clone(),
                name_en: row.name_en,
                group_slug: row.group_slug,
            };
            by_slug.insert(row.slug, info.clone());
            by_id.insert(row.id, info);
        }

        Ok(Self { by_slug, by_id })
    }

    /// Resolve a slug to a product id, or `None` if the slug is unknown.
    pub fn id_for_slug(&self, slug: &str) -> Option<i32> {
        self.by_slug.get(slug).map(|p| p.id)
    }

    /// Look up a product by id.
    pub fn by_id(&self, id: i32) -> Option<&ProductInfo> {
        self.by_id.get(&id)
    }

    /// Number of products in the snapshot (useful for a startup log).
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}
```

#### Edit: `src/lib.rs`

Add the module declaration alongside the others:

```rust
pub mod taxonomy;
```

### 2.2 Wire the snapshot into startup

In `src/startup.rs`, inside `run(...)` (where `db_pool` and the other `Data`
values are built), construct and register the snapshot. Full context shown:

```rust
// After `let connection_pool = ...;` is available and before building the
// HttpServer closure:
let taxonomy = crate::taxonomy::TaxonomySnapshot::load(&db_pool)
    .await
    .context("Failed to load the product taxonomy snapshot.")?;
tracing::info!(products = taxonomy.len(), "Loaded product taxonomy snapshot.");
let taxonomy = Data::new(taxonomy);
```

Then attach it in the `App` factory next to the other `.app_data(...)` calls:

```rust
.app_data(taxonomy.clone())
```

`db_pool` here is the `PgPool` before it is wrapped in `Data::new(...)`. If your
`run` wraps it earlier, load the snapshot from the un-wrapped pool (or call
`taxonomy = TaxonomySnapshot::load(db_pool.get_ref())` after wrapping).

### 2.3 Response DTOs

Replace the `Farm` struct (which carried `categories`) with response DTOs. The
read row is loaded into a private `FarmRow`; the API returns `FarmResponse`.

#### Rewrite: `src/routes/farms/mod.rs`

```rust
use crate::domain::farm::{Address, Canton, Name, Point};
use chrono::{DateTime, Utc};
use uuid::Uuid;

mod error;
mod get;
mod post;

pub use error::FarmError;
pub use get::{get_all, get_by_id};
pub use post::create;

/// A product as returned to API clients.
#[derive(serde::Serialize, Clone)]
pub struct ProductDto {
    pub slug: String,
    pub name_en: String,
    /// The slug of the category group this product belongs to.
    pub group: String,
}

/// A farm plus its products. `categories` is derived from `products`
/// (distinct group slugs) and kept for backward compatibility.
#[derive(serde::Serialize)]
pub struct FarmResponse {
    pub id: Uuid,
    pub name: Name,
    pub address: Address,
    pub canton: Canton,
    pub coordinates: Point,
    pub categories: Vec<String>,
    pub products: Vec<ProductDto>,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// A page of farms plus the cursor for the next page (if any).
#[derive(serde::Serialize)]
pub struct FarmListResponse {
    pub farms: Vec<FarmResponse>,
    pub next_cursor: Option<String>,
}

/// The raw farm row loaded from the database, before products are attached.
pub(crate) struct FarmRow {
    pub id: Uuid,
    pub name: Name,
    pub address: Address,
    pub canton: Canton,
    pub coordinates: Point,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}
```

### 2.4 The read handlers

Product filtering uses a **comma-separated** `product` query parameter
(`?product=strawberries,cherries`) because Actix's `web::Query` (serde_urlencoded)
does not decode repeated keys into a `Vec`. Pagination is keyset-based on the
existing `(created_at DESC, id DESC)` index.

#### Rewrite: `src/routes/farms/get.rs`

```rust
use crate::{
    domain::farm::{Address, Canton, Name, Point},
    routes::farms::{FarmError, FarmListResponse, FarmResponse, FarmRow, ProductDto},
    taxonomy::TaxonomySnapshot,
};
use actix_web::{HttpResponse, web};
use anyhow::Context;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct FarmPath {
    id: String,
}

#[derive(serde::Deserialize)]
pub struct FarmListQuery {
    /// Comma-separated product slugs, e.g. `?product=strawberries,cherries`.
    pub product: Option<String>,
    /// `"all"` requires every product; anything else (or absent) means "any of".
    pub r#match: Option<String>,
    /// Keyset cursor: `"<rfc3339>_<uuid>"` of the last row from the previous page.
    pub after: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    20
}

#[tracing::instrument(name = "List farms", skip(pool, taxonomy))]
pub async fn get_all(
    query: web::Query<FarmListQuery>,
    pool: web::Data<PgPool>,
    taxonomy: web::Data<TaxonomySnapshot>,
) -> Result<HttpResponse, FarmError> {
    let limit = query.limit.clamp(1, 100);

    // Resolve product slugs to ids (early 400 on any unknown slug).
    let product_ids = match &query.product {
        None => Vec::new(),
        Some(csv) => {
            let mut ids = Vec::new();
            for slug in csv.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let id = taxonomy.id_for_slug(slug).ok_or_else(|| {
                    FarmError::ValidationError(format!("Unknown product '{slug}'."))
                })?;
                ids.push(id);
            }
            ids
        }
    };

    let match_all = query.r#match.as_deref() == Some("all");

    let cursor = match &query.after {
        None => None,
        Some(raw) => Some(
            parse_cursor(raw)
                .map_err(|_| FarmError::ValidationError("Invalid cursor.".to_string()))?,
        ),
    };

    let farms = list_farms(&pool, &product_ids, match_all, cursor, limit).await?;

    // If the page is full, hand back a cursor to fetch the next one.
    let next_cursor = if farms.len() as i64 == limit {
        farms
            .last()
            .map(|f| make_cursor(f.created_at, f.id))
    } else {
        None
    };

    Ok(HttpResponse::Ok().json(FarmListResponse { farms, next_cursor }))
}

#[tracing::instrument(name = "Query farms page", skip(pool))]
async fn list_farms(
    pool: &PgPool,
    product_ids: &[i32],
    match_all: bool,
    cursor: Option<(DateTime<Utc>, Uuid)>,
    limit: i64,
) -> Result<Vec<FarmResponse>, FarmError> {
    let (cursor_ts, cursor_id) = match cursor {
        Some((ts, id)) => (Some(ts), Some(id)),
        None => (None, None),
    };

    // Page of farms: product filter via a grouped subquery, keyset cursor via
    // the (created_at DESC, id DESC) index.
    let farm_rows = sqlx::query_as!(
        FarmRow,
        r#"
        SELECT
            f.id,
            f.name        AS "name: Name",
            f.address     AS "address: Address",
            f.canton      AS "canton: Canton",
            f.coordinates AS "coordinates: Point",
            f.created_at,
            f.updated_at
        FROM farms f
        WHERE
            (
                cardinality($1::int[]) = 0
                OR f.id IN (
                    SELECT fp.farm_id
                    FROM farm_products fp
                    WHERE fp.product_id = ANY($1)
                    GROUP BY fp.farm_id
                    HAVING $2 = false
                        OR count(DISTINCT fp.product_id) = cardinality($1)
                )
            )
            AND (
                $3::timestamptz IS NULL
                OR (f.created_at, f.id) < ($3, $4)
            )
        ORDER BY f.created_at DESC, f.id DESC
        LIMIT $5
        "#,
        product_ids,
        match_all,
        cursor_ts,
        cursor_id,
        limit,
    )
    .fetch_all(pool)
    .await
    .context("Failed to page farms.")?;

    let farm_ids: Vec<Uuid> = farm_rows.iter().map(|f| f.id).collect();

    // One query for the products of every farm on this page (no N+1).
    let product_rows = sqlx::query!(
        r#"
        SELECT
            fp.farm_id,
            p.slug    AS "slug!",
            p.name_en AS "name_en!",
            c.slug    AS "group_slug!"
        FROM farm_products fp
        JOIN products p ON p.id = fp.product_id
        JOIN product_categories c ON c.id = p.category_id
        WHERE fp.farm_id = ANY($1)
        ORDER BY p.slug
        "#,
        &farm_ids,
    )
    .fetch_all(pool)
    .await
    .context("Failed to load farm products.")?;

    let mut products_by_farm: HashMap<Uuid, Vec<ProductDto>> = HashMap::new();
    for row in product_rows {
        products_by_farm
            .entry(row.farm_id)
            .or_default()
            .push(ProductDto {
                slug: row.slug,
                name_en: row.name_en,
                group: row.group_slug,
            });
    }

    let mut responses = Vec::with_capacity(farm_rows.len());
    for farm in farm_rows {
        let products = products_by_farm.remove(&farm.id).unwrap_or_default();
        let categories = derive_categories(&products);
        responses.push(FarmResponse {
            id: farm.id,
            name: farm.name,
            address: farm.address,
            canton: farm.canton,
            coordinates: farm.coordinates,
            categories,
            products,
            created_at: farm.created_at,
            updated_at: farm.updated_at,
        });
    }

    Ok(responses)
}

#[tracing::instrument(name = "Get farm by id", skip(pool))]
pub async fn get_by_id(
    path: web::Path<FarmPath>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, FarmError> {
    let farm_id = Uuid::parse_str(&path.id)
        .map_err(|_| FarmError::ValidationError("Invalid farm id.".to_string()))?;

    match get_farm_by_id(farm_id, &pool).await? {
        Some(farm) => Ok(HttpResponse::Ok().json(farm)),
        None => Err(FarmError::NotFound),
    }
}

#[tracing::instrument(name = "Query single farm", skip(pool))]
async fn get_farm_by_id(farm_id: Uuid, pool: &PgPool) -> Result<Option<FarmResponse>, FarmError> {
    let farm = sqlx::query_as!(
        FarmRow,
        r#"
        SELECT
            f.id,
            f.name        AS "name: Name",
            f.address     AS "address: Address",
            f.canton      AS "canton: Canton",
            f.coordinates AS "coordinates: Point",
            f.created_at,
            f.updated_at
        FROM farms f
        WHERE f.id = $1
        "#,
        farm_id,
    )
    .fetch_optional(pool)
    .await
    .context("Failed to fetch farm.")?;

    let Some(farm) = farm else {
        return Ok(None);
    };

    let product_rows = sqlx::query!(
        r#"
        SELECT
            p.slug    AS "slug!",
            p.name_en AS "name_en!",
            c.slug    AS "group_slug!"
        FROM farm_products fp
        JOIN products p ON p.id = fp.product_id
        JOIN product_categories c ON c.id = p.category_id
        WHERE fp.farm_id = $1
        ORDER BY p.slug
        "#,
        farm_id,
    )
    .fetch_all(pool)
    .await
    .context("Failed to load farm products.")?;

    let products: Vec<ProductDto> = product_rows
        .into_iter()
        .map(|row| ProductDto {
            slug: row.slug,
            name_en: row.name_en,
            group: row.group_slug,
        })
        .collect();
    let categories = derive_categories(&products);

    Ok(Some(FarmResponse {
        id: farm.id,
        name: farm.name,
        address: farm.address,
        canton: farm.canton,
        coordinates: farm.coordinates,
        categories,
        products,
        created_at: farm.created_at,
        updated_at: farm.updated_at,
    }))
}

/// Distinct, sorted group slugs derived from a farm's products.
fn derive_categories(products: &[ProductDto]) -> Vec<String> {
    let mut categories: Vec<String> = products.iter().map(|p| p.group.clone()).collect();
    categories.sort();
    categories.dedup();
    categories
}

/// Decode a keyset cursor of the form `"<rfc3339>_<uuid>"`.
fn parse_cursor(s: &str) -> Result<(DateTime<Utc>, Uuid), anyhow::Error> {
    let (ts, id) = s
        .split_once('_')
        .context("Cursor is missing its separator.")?;
    let created_at = DateTime::parse_from_rfc3339(ts)
        .context("Cursor timestamp is not valid RFC 3339.")?
        .with_timezone(&Utc);
    let id = Uuid::parse_str(id).context("Cursor id is not a valid UUID.")?;
    Ok((created_at, id))
}

/// Encode the cursor for the next page.
fn make_cursor(created_at: DateTime<Utc>, id: Uuid) -> String {
    format!("{}_{}", created_at.to_rfc3339(), id)
}
```

> After this PR, `get.rs` no longer selects `categories`, but `post.rs` still
> inserts it, so the column stays until PR 3. Everything compiles.

### 2.5 Test fixtures + read tests

Because there is no dataset seeder here, tests insert their own tiny taxonomy.

#### Add to `tests/common/mod.rs`

```rust
/// Insert a minimal product taxonomy for a test and return the ids the test
/// needs. One group ("Früchte"/"fruits") with two products.
#[allow(dead_code)]
pub struct TestTaxonomy {
    pub category_id: i16,
    pub strawberries_id: i32,
    pub cherries_id: i32,
}

#[allow(dead_code)]
pub async fn seed_test_taxonomy(pool: &PgPool) -> TestTaxonomy {
    let category_id = sqlx::query!(
        r#"
        INSERT INTO product_categories (key_de, slug, display_order)
        VALUES ('Früchte', 'fruits', 0)
        RETURNING id
        "#,
    )
    .fetch_one(pool)
    .await
    .expect("Failed to insert test category.")
    .id;

    let strawberries_id = insert_test_product(pool, category_id, "Erdbeeren", "strawberries", "Strawberries").await;
    let cherries_id = insert_test_product(pool, category_id, "Kirschen", "cherries", "Cherries").await;

    TestTaxonomy { category_id, strawberries_id, cherries_id }
}

#[allow(dead_code)]
async fn insert_test_product(
    pool: &PgPool,
    category_id: i16,
    key_de: &str,
    slug: &str,
    name_en: &str,
) -> i32 {
    sqlx::query!(
        r#"
        INSERT INTO products (category_id, key_de, slug, name_en)
        VALUES ($1, $2, $3, $4)
        RETURNING id
        "#,
        category_id,
        key_de,
        slug,
        name_en,
    )
    .fetch_one(pool)
    .await
    .expect("Failed to insert test product.")
    .id
}

/// Insert a bare farm (no products) and return its id.
#[allow(dead_code)]
pub async fn insert_test_farm(pool: &PgPool, name: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query!(
        r#"
        INSERT INTO farms (id, name, address, canton, coordinates, categories, created_at, updated_at)
        VALUES ($1, $2, 'Somewhere 1', 'ZH', POINT(8.5, 47.4), ARRAY[]::text[], now(), NULL)
        "#,
        id,
        name,
    )
    .execute(pool)
    .await
    .expect("Failed to insert test farm.");
    id
}

/// Link a farm to a product.
#[allow(dead_code)]
pub async fn link_farm_product(pool: &PgPool, farm_id: Uuid, product_id: i32) {
    sqlx::query!(
        r#"INSERT INTO farm_products (farm_id, product_id) VALUES ($1, $2)"#,
        farm_id,
        product_id,
    )
    .execute(pool)
    .await
    .expect("Failed to link farm product.");
}
```

> `insert_test_farm` still writes `categories` because PR 2 has not dropped the
> column yet. In PR 3, drop the `categories` argument from this helper.

#### New file: `tests/api/products.rs` (+ `mod products;` in `tests/api/main.rs`)

```rust
use crate::helpers::{insert_test_farm, link_farm_product, seed_test_taxonomy, spawn_app};
use actix_web::http::StatusCode;
use farms::configuration::IdempotencyEngine;

#[tokio::test]
async fn list_filters_by_product_slug() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;

    let has_strawberries = insert_test_farm(&app.db_pool, "Berry Farm").await;
    link_farm_product(&app.db_pool, has_strawberries, taxonomy.strawberries_id).await;

    let no_strawberries = insert_test_farm(&app.db_pool, "Cherry Only").await;
    link_farm_product(&app.db_pool, no_strawberries, taxonomy.cherries_id).await;

    let response = app
        .api_client
        .get(format!("{}/farms?product=strawberries", app.address))
        .send()
        .await
        .expect("Failed to execute request.");
    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());

    let body: serde_json::Value = response.json().await.unwrap();
    let farms = body["farms"].as_array().unwrap();
    assert_eq!(1, farms.len());
    assert_eq!(has_strawberries.to_string(), farms[0]["id"].as_str().unwrap());

    // The response lists the farm's full product set with the derived category.
    let slugs: Vec<&str> = farms[0]["products"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["slug"].as_str().unwrap())
        .collect();
    assert_eq!(vec!["strawberries"], slugs);
    let categories: Vec<&str> = farms[0]["categories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    assert_eq!(vec!["fruits"], categories);
}

#[tokio::test]
async fn match_all_requires_every_product() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;

    let both = insert_test_farm(&app.db_pool, "Both").await;
    link_farm_product(&app.db_pool, both, taxonomy.strawberries_id).await;
    link_farm_product(&app.db_pool, both, taxonomy.cherries_id).await;

    let only_one = insert_test_farm(&app.db_pool, "One").await;
    link_farm_product(&app.db_pool, only_one, taxonomy.strawberries_id).await;

    let response = app
        .api_client
        .get(format!(
            "{}/farms?product=strawberries,cherries&match=all",
            app.address
        ))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = response.json().await.unwrap();
    let farms = body["farms"].as_array().unwrap();
    assert_eq!(1, farms.len());
    assert_eq!(both.to_string(), farms[0]["id"].as_str().unwrap());
}

#[tokio::test]
async fn unknown_product_slug_is_400() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    let response = app
        .api_client
        .get(format!("{}/farms?product=dragonfruit", app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}
```

---

## PR 3 — Write API (create a farm with products)

**Goal:** `POST /farms` accepts `products: [slug]`, validated and written
transactionally (reusing idempotency), and the `farms.categories` column is
dropped.

### 3.1 The `ProductSlug` domain type

#### New file: `src/domain/farm/product_slug.rs`

```rust
//! A validated product slug supplied by API clients (filters, create requests).

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProductSlug(String);

#[derive(Debug, Error)]
pub enum ProductSlugError {
    #[error("Product slug cannot be empty.")]
    Empty,
    #[error("Product slug is too long (max 64 characters).")]
    TooLong,
    #[error("Product slug may only contain lowercase letters, digits and hyphens.")]
    InvalidCharacters,
}

impl ProductSlug {
    /// Parse a slug. Shape-only validation; whether the slug *exists* is checked
    /// against the taxonomy snapshot by the caller.
    pub fn parse(s: String) -> Result<Self, ProductSlugError> {
        let trimmed = s.trim().to_lowercase();

        if trimmed.is_empty() {
            return Err(ProductSlugError::Empty);
        }
        if trimmed.len() > 64 {
            return Err(ProductSlugError::TooLong);
        }
        if !trimmed
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(ProductSlugError::InvalidCharacters);
        }

        Ok(Self(trimmed))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ProductSlug {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};

    #[test]
    fn a_valid_slug_is_accepted() {
        assert_ok!(ProductSlug::parse("strawberries".to_string()));
        assert_ok!(ProductSlug::parse("stone-fruits".to_string()));
    }

    #[test]
    fn it_is_lowercased_and_trimmed() {
        let slug = ProductSlug::parse("  Strawberries  ".to_string()).unwrap();
        assert_eq!("strawberries", slug.as_str());
    }

    #[test]
    fn empty_is_rejected() {
        assert_err!(ProductSlug::parse("   ".to_string()));
    }

    #[test]
    fn invalid_characters_are_rejected() {
        assert_err!(ProductSlug::parse("straw berries".to_string()));
        assert_err!(ProductSlug::parse("straw_berries".to_string()));
        assert_err!(ProductSlug::parse("straw!".to_string()));
    }

    #[test]
    fn overly_long_is_rejected() {
        assert_err!(ProductSlug::parse("a".repeat(65)));
    }
}
```

#### Edit: `src/domain/farm/mod.rs`

```rust
mod address;
mod canton;
mod categories;
mod name;
mod point;
mod product_slug;

pub use address::Address;
pub use canton::Canton;
pub use categories::Categories;
pub use name::Name;
pub use point::{Point, PointError};
pub use product_slug::{ProductSlug, ProductSlugError};
```

(Keep `Categories` exported if anything else still uses it; it is no longer used
by the farms routes after this PR and can be removed in a follow-up.)

### 3.2 Rewrite the create handler

#### Rewrite: `src/routes/farms/post.rs`

```rust
use crate::{
    authentication::CurrentUser,
    configuration::Settings,
    domain::farm::{Address, Canton, Name, Point, ProductSlug},
    idempotency::{IdempotencyError, IdempotencyNextAction, save_response, try_processing},
    routes::farms::FarmError,
    taxonomy::TaxonomySnapshot,
};
use actix_web::{HttpResponse, web};
use anyhow::Context;
use chrono::{DateTime, Utc};
use deadpool_redis::Pool;
use sqlx::{Executor, PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct FormData {
    name: String,
    address: String,
    canton: String,
    coordinates: String,
    /// Product slugs the farm offers, e.g. ["strawberries", "cherries"].
    products: Vec<String>,
    idempotency_key: String,
}

#[allow(clippy::async_yields_async)]
#[tracing::instrument(
    name = "Adding a new farm",
    skip(body, pool, redis_pool, taxonomy, configuration)
)]
pub async fn create(
    current_user: CurrentUser,
    body: web::Json<FormData>,
    pool: web::Data<PgPool>,
    redis_pool: web::Data<Pool>,
    taxonomy: web::Data<TaxonomySnapshot>,
    configuration: web::Data<Settings>,
) -> Result<HttpResponse, FarmError> {
    let body = body.into_inner();

    // Validate the farm's own fields.
    let name = Name::parse(body.name).map_err(|e| FarmError::ValidationError(e.to_string()))?;
    let address =
        Address::parse(body.address).map_err(|e| FarmError::ValidationError(e.to_string()))?;
    let canton =
        Canton::parse(body.canton).map_err(|e| FarmError::ValidationError(e.to_string()))?;
    let coordinates =
        Point::parse(&body.coordinates).map_err(|e| FarmError::ValidationError(e.to_string()))?;

    // Validate the product slugs (shape) then resolve them to ids (existence).
    if body.products.is_empty() {
        return Err(FarmError::ValidationError(
            "At least one product is required.".to_string(),
        ));
    }
    let mut product_ids = Vec::with_capacity(body.products.len());
    for raw in body.products {
        let slug = ProductSlug::parse(raw).map_err(|e| FarmError::ValidationError(e.to_string()))?;
        let id = taxonomy.id_for_slug(slug.as_str()).ok_or_else(|| {
            FarmError::ValidationError(format!("Unknown product '{}'.", slug.as_str()))
        })?;
        product_ids.push(id);
    }
    // De-duplicate so ON CONFLICT is not doing our work at the DB layer.
    product_ids.sort_unstable();
    product_ids.dedup();

    // Idempotency: open (or short-circuit) the request.
    let mut transaction = match try_processing(
        &redis_pool,
        &pool,
        body.idempotency_key.as_str(),
        current_user.id,
        &configuration.idempotency,
    )
    .await
    .map_err(|e| match e {
        IdempotencyError::ExpectedResponseNotFoundError => FarmError::DuplicateRequestConflict(e),
        _ => FarmError::UnexpectedError(e.into()),
    })? {
        IdempotencyNextAction::ReturnSavedResponse(saved_response) => {
            return Ok(saved_response);
        }
        IdempotencyNextAction::StartProcessing(transaction) => transaction,
    };

    let farm_id = insert_farm(&mut transaction, &name, &address, &canton, &coordinates).await?;
    insert_farm_products(&mut transaction, farm_id, &product_ids).await?;

    let response = HttpResponse::Created().finish();
    let (response, transaction) = save_response(
        &redis_pool,
        transaction,
        body.idempotency_key.as_str(),
        current_user.id,
        &configuration.idempotency,
        response,
    )
    .await
    .map_err(|e| FarmError::UnexpectedError(e.into()))?;

    transaction
        .commit()
        .await
        .map_err(|e| FarmError::UnexpectedError(e.into()))?;

    Ok(response)
}

#[tracing::instrument(name = "Saving new farm details in the database", skip(transaction))]
async fn insert_farm(
    transaction: &mut Transaction<'_, Postgres>,
    name: &Name,
    address: &Address,
    canton: &Canton,
    coordinates: &Point,
) -> Result<Uuid, FarmError> {
    let farm_id = Uuid::new_v4();
    let query = sqlx::query!(
        r#"
        INSERT INTO farms (id, name, address, canton, coordinates, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        farm_id,
        name as &Name,
        address as &Address,
        canton as &Canton,
        coordinates as &Point,
        Utc::now(),
        Option::<DateTime<Utc>>::None,
    );
    transaction
        .execute(query)
        .await
        .context("Failed to insert new farm in the database.")?;

    Ok(farm_id)
}

#[tracing::instrument(name = "Linking farm to products", skip(transaction))]
async fn insert_farm_products(
    transaction: &mut Transaction<'_, Postgres>,
    farm_id: Uuid,
    product_ids: &[i32],
) -> Result<(), FarmError> {
    // Single round-trip bulk insert via UNNEST.
    let query = sqlx::query!(
        r#"
        INSERT INTO farm_products (farm_id, product_id)
        SELECT $1, * FROM UNNEST($2::int[])
        ON CONFLICT DO NOTHING
        "#,
        farm_id,
        product_ids,
    );
    transaction
        .execute(query)
        .await
        .context("Failed to link farm to products.")?;

    Ok(())
}
```

> The `name as &Name` bindings rely on the domain types implementing SQLx
> `Encode`/`Type` (via `impl_sqlx_for_string_domain_type!` / the `Point` impl),
> exactly as the current `insert_farm` does.

### 3.3 Drop the `categories` column

Now that neither read nor write references it:

#### File: `migrations/<timestamp>_drop_farms_categories.sql`

```sql
ALTER TABLE farms DROP COLUMN categories;
```

Update the test helper `insert_test_farm` (from PR 2) to stop writing it:

```rust
#[allow(dead_code)]
pub async fn insert_test_farm(pool: &PgPool, name: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query!(
        r#"
        INSERT INTO farms (id, name, address, canton, coordinates, created_at, updated_at)
        VALUES ($1, $2, 'Somewhere 1', 'ZH', POINT(8.5, 47.4), now(), NULL)
        "#,
        id,
        name,
    )
    .execute(pool)
    .await
    .expect("Failed to insert test farm.");
    id
}
```

Register the taxonomy on the create route (it is already an `app_data` from
PR 2, so no route wiring change is needed — the extractor picks it up).

### 3.4 Write tests

#### Add to `tests/api/products.rs`

```rust
use crate::helpers::TestApp;
use serde_json::json;
use uuid::Uuid;

/// Helper: authenticate as an active user (create + verify), returning a client
/// with a live session cookie. Assumes a `create_active_user` helper exists in
/// tests/common (mirroring how the auth tests obtain a session). If your suite
/// uses `TestUser::store` + login, use that instead.
async fn create_farm(app: &TestApp, products: serde_json::Value) -> reqwest::Response {
    app.post_farm(&json!({
        "name": "Test Farm",
        "address": "Road 1, 8000 Zürich",
        "canton": "ZH",
        "coordinates": "47.37, 8.54",
        "products": products,
        "idempotency_key": Uuid::new_v4().to_string(),
    }))
    .await
}

#[tokio::test]
async fn create_farm_persists_product_links() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;
    app.log_in_active_user().await; // whatever your suite uses to get a session

    let response = create_farm(&app, json!(["strawberries"])).await;
    assert_eq!(StatusCode::CREATED.as_u16(), response.status().as_u16());

    let linked = sqlx::query!(
        r#"
        SELECT p.slug AS "slug!"
        FROM farm_products fp
        JOIN products p ON p.id = fp.product_id
        "#,
    )
    .fetch_all(&app.db_pool)
    .await
    .unwrap();
    let slugs: Vec<String> = linked.into_iter().map(|r| r.slug).collect();
    assert_eq!(vec!["strawberries".to_string()], slugs);
}

#[tokio::test]
async fn create_farm_rejects_unknown_product() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;
    app.log_in_active_user().await;

    let response = create_farm(&app, json!(["dragonfruit"])).await;
    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn create_farm_requires_at_least_one_product() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;
    app.log_in_active_user().await;

    let response = create_farm(&app, json!([])).await;
    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}
```

> Use whatever session helper your suite already has for authenticated `POST
> /farms` (the create route is behind `CurrentUser`). The exact helper name
> (`log_in_active_user`) is illustrative.

---

## PR 4 — User-submitted product suggestions

**Goal:** an authenticated user proposes adding/removing a product on a farm;
stored as `PENDING`.

### 4.1 Migration

#### File: `migrations/<timestamp>_create_farm_product_suggestions.sql`

```sql
CREATE TYPE suggestion_status AS ENUM ('PENDING', 'APPROVED', 'REJECTED');
CREATE TYPE suggestion_action AS ENUM ('ADD', 'REMOVE');

CREATE TABLE farm_product_suggestions (
    id           uuid              PRIMARY KEY,
    farm_id      uuid              NOT NULL REFERENCES farms (id)    ON DELETE CASCADE,
    product_id   integer           NOT NULL REFERENCES products (id) ON DELETE RESTRICT,
    action       suggestion_action NOT NULL,
    note         text,
    submitted_by uuid              NOT NULL REFERENCES users (id),
    status       suggestion_status NOT NULL DEFAULT 'PENDING',
    reviewed_by  uuid              REFERENCES users (id),
    reviewed_at  timestamptz,
    created_at   timestamptz       NOT NULL
);

-- Partial index: the moderation queue only ever reads PENDING rows.
CREATE INDEX farm_product_suggestions_pending_idx
    ON farm_product_suggestions (created_at DESC, id DESC)
    WHERE status = 'PENDING';
```

### 4.2 Domain enums

#### New file: `src/domain/suggestion.rs` (+ `pub mod suggestion;` in `src/domain/mod.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, serde::Serialize, serde::Deserialize)]
#[sqlx(type_name = "suggestion_status", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "snake_case")]
pub enum SuggestionStatus {
    Pending,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, serde::Serialize, serde::Deserialize)]
#[sqlx(type_name = "suggestion_action", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "snake_case")]
pub enum SuggestionAction {
    Add,
    Remove,
}
```

### 4.3 The submit route + error

#### New file: `src/routes/suggestions/error.rs`

```rust
use crate::errors::error_chain_fmt;
use actix_web::{ResponseError, http::StatusCode};
use std::fmt::Formatter;

#[derive(thiserror::Error)]
pub enum SuggestionError {
    #[error("{0}")]
    ValidationError(String),
    #[error("Farm not found.")]
    FarmNotFound,
    #[error("Too many suggestions. Try again later.")]
    RateLimited,
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl ResponseError for SuggestionError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::ValidationError(_) => StatusCode::BAD_REQUEST,
            Self::FarmNotFound => StatusCode::NOT_FOUND,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl std::fmt::Debug for SuggestionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
```

#### New file: `src/routes/suggestions/post.rs`

```rust
use crate::{
    authentication::CurrentUser,
    configuration::Settings,
    domain::suggestion::SuggestionAction,
    rate_limit::{RateLimitDecision, check_rate_limit},
    routes::suggestions::error::SuggestionError,
    taxonomy::TaxonomySnapshot,
};
use actix_web::{HttpRequest, HttpResponse, web};
use anyhow::Context;
use chrono::Utc;
use deadpool_redis::Pool;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct SuggestionRequest {
    /// Product slug being suggested.
    product: String,
    action: SuggestionAction,
    note: Option<String>,
}

#[tracing::instrument(
    name = "Submit product suggestion",
    skip(body, pool, redis_pool, taxonomy, configuration, request)
)]
pub async fn submit_suggestion(
    current_user: CurrentUser,
    path: web::Path<Uuid>,
    body: web::Json<SuggestionRequest>,
    pool: web::Data<PgPool>,
    redis_pool: web::Data<Pool>,
    taxonomy: web::Data<TaxonomySnapshot>,
    configuration: web::Data<Settings>,
    request: HttpRequest,
) -> Result<HttpResponse, SuggestionError> {
    let farm_id = path.into_inner();
    let body = body.into_inner();

    // Resolve the product.
    let product_id = taxonomy
        .id_for_slug(body.product.trim())
        .ok_or_else(|| SuggestionError::ValidationError("Unknown product.".to_string()))?;

    // Optional note length guard.
    if let Some(note) = &body.note {
        if note.chars().count() > 500 {
            return Err(SuggestionError::ValidationError(
                "Note is too long (max 500 characters).".to_string(),
            ));
        }
    }

    // Rate limit per user (suggestion spam is the abuse vector).
    enforce_rate_limit(&redis_pool, current_user.id, &request, &configuration).await?;

    // The farm must exist (a fabricated id must 404, not FK-error).
    let farm_exists = sqlx::query!(r#"SELECT id FROM farms WHERE id = $1"#, farm_id)
        .fetch_optional(pool.get_ref())
        .await
        .context("Failed to check farm existence.")?
        .is_some();
    if !farm_exists {
        return Err(SuggestionError::FarmNotFound);
    }

    sqlx::query!(
        r#"
        INSERT INTO farm_product_suggestions
            (id, farm_id, product_id, action, note, submitted_by, status, created_at)
        VALUES ($1, $2, $3, $4::suggestion_action, $5, $6, 'PENDING', $7)
        "#,
        Uuid::new_v4(),
        farm_id,
        product_id,
        body.action as SuggestionAction,
        body.note,
        current_user.id,
        Utc::now(),
    )
    .execute(pool.get_ref())
    .await
    .context("Failed to store the suggestion.")?;

    Ok(HttpResponse::Accepted().finish())
}

/// Fixed-window limit keyed by user and by client IP. Fails open.
#[tracing::instrument(name = "Enforce suggestion rate limit", skip(redis_pool, configuration, request))]
async fn enforce_rate_limit(
    redis_pool: &Pool,
    user_id: Uuid,
    request: &HttpRequest,
    configuration: &Settings,
) -> Result<(), SuggestionError> {
    let limits = &configuration.registration.rate_limit; // reuse the same tunables
    let client_ip = request
        .connection_info()
        .realip_remote_addr()
        .unwrap_or("unknown")
        .to_string();

    let prefix = &limits.key_prefix;
    let keys = [
        format!("{prefix}:suggestion:user:{user_id}"),
        format!("{prefix}:suggestion:ip:{client_ip}"),
    ];

    for key in keys {
        match check_rate_limit(redis_pool, &key, limits.max_requests, limits.window_seconds).await {
            Ok(RateLimitDecision::Allowed) => {}
            Ok(RateLimitDecision::Limited) => return Err(SuggestionError::RateLimited),
            Err(e) => {
                tracing::warn!(error = ?e, "Suggestion rate limit check failed; allowing (fail-open).");
            }
        }
    }

    Ok(())
}
```

#### New file: `src/routes/suggestions/mod.rs`

```rust
mod error;
mod post;

pub use error::SuggestionError;
pub use post::submit_suggestion;
```

#### Edit: `src/routes/mod.rs`

```rust
pub mod authentication;
pub mod farms;
mod health_check;
pub mod suggestions;

pub use health_check::*;
```

### 4.4 Route wiring

#### Edit: `src/startup.rs` (inside the `App` factory)

```rust
.route(
    "/farms/{id}/product-suggestions",
    web::post().to(crate::routes::suggestions::submit_suggestion),
)
```

### 4.5 Tests

#### New file: `tests/api/suggestions.rs` (+ `mod suggestions;` in `tests/api/main.rs`)

```rust
use crate::helpers::{insert_test_farm, seed_test_taxonomy, spawn_app};
use actix_web::http::StatusCode;
use farms::configuration::IdempotencyEngine;
use serde_json::json;

#[tokio::test]
async fn submit_requires_authentication() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;
    let _ = taxonomy;
    let farm_id = insert_test_farm(&app.db_pool, "Farm").await;

    let response = app
        .api_client
        .post(format!("{}/farms/{}/product-suggestions", app.address, farm_id))
        .json(&json!({ "product": "strawberries", "action": "add" }))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::UNAUTHORIZED.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn authenticated_user_can_submit_pending_suggestion() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;
    let farm_id = insert_test_farm(&app.db_pool, "Farm").await;
    app.log_in_active_user().await;

    let response = app
        .api_client
        .post(format!("{}/farms/{}/product-suggestions", app.address, farm_id))
        .json(&json!({ "product": "strawberries", "action": "add" }))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::ACCEPTED.as_u16(), response.status().as_u16());

    let row = sqlx::query!(
        r#"SELECT status::text AS "status!" FROM farm_product_suggestions WHERE farm_id = $1"#,
        farm_id,
    )
    .fetch_one(&app.db_pool)
    .await
    .unwrap();
    assert_eq!("PENDING", row.status);
}

#[tokio::test]
async fn submit_rejects_unknown_product() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;
    let farm_id = insert_test_farm(&app.db_pool, "Farm").await;
    app.log_in_active_user().await;

    let response = app
        .api_client
        .post(format!("{}/farms/{}/product-suggestions", app.address, farm_id))
        .json(&json!({ "product": "dragonfruit", "action": "add" }))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}
```

---

## PR 5 — Admin moderation

**Goal:** admins list the pending queue and approve/reject; approval applies the
change atomically.

### 5.1 The admin extractor

#### New file: `src/authentication/admin.rs` (+ `mod admin; pub use admin::AdminUser;` in `src/authentication/mod.rs`)

```rust
use crate::authentication::CurrentUser;
use crate::domain::user::Role;
use actix_web::{FromRequest, HttpRequest, dev::Payload};
use std::future::Future;
use std::pin::Pin;

/// Extractor that only succeeds for ADMIN users; otherwise 403.
pub struct AdminUser(pub CurrentUser);

impl FromRequest for AdminUser {
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let current_user = CurrentUser::from_request(req, payload);
        Box::pin(async move {
            let user = current_user.await?;
            if user.role == Role::Admin {
                Ok(AdminUser(user))
            } else {
                Err(actix_web::error::ErrorForbidden("Admin access required."))
            }
        })
    }
}
```

### 5.2 Moderation error

#### New file: `src/routes/admin/error.rs`

```rust
use crate::errors::error_chain_fmt;
use actix_web::{ResponseError, http::StatusCode};
use std::fmt::Formatter;

#[derive(thiserror::Error)]
pub enum AdminError {
    #[error("{0}")]
    ValidationError(String),
    #[error("Suggestion is no longer pending.")]
    Conflict,
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl ResponseError for AdminError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::ValidationError(_) => StatusCode::BAD_REQUEST,
            Self::Conflict => StatusCode::CONFLICT,
            Self::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl std::fmt::Debug for AdminError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
```

### 5.3 Moderation handlers

#### New file: `src/routes/admin/suggestions.rs`

```rust
use crate::{
    authentication::AdminUser,
    domain::suggestion::{SuggestionAction, SuggestionStatus},
    routes::admin::error::AdminError,
};
use actix_web::{HttpResponse, web};
use anyhow::Context;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct ListQuery {
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    50
}

#[derive(serde::Serialize)]
pub struct SuggestionView {
    id: Uuid,
    farm_id: Uuid,
    product_slug: String,
    action: SuggestionAction,
    note: Option<String>,
    submitted_by: Uuid,
    created_at: DateTime<Utc>,
}

/// GET /admin/product-suggestions — the pending queue.
#[tracing::instrument(name = "List pending suggestions", skip(pool))]
pub async fn list_pending(
    _admin: AdminUser,
    query: web::Query<ListQuery>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, AdminError> {
    let limit = query.limit.clamp(1, 200);

    let rows = sqlx::query!(
        r#"
        SELECT
            s.id,
            s.farm_id,
            p.slug        AS "product_slug!",
            s.action      AS "action: SuggestionAction",
            s.note,
            s.submitted_by,
            s.created_at
        FROM farm_product_suggestions s
        JOIN products p ON p.id = s.product_id
        WHERE s.status = 'PENDING'
        ORDER BY s.created_at DESC, s.id DESC
        LIMIT $1
        "#,
        limit,
    )
    .fetch_all(pool.get_ref())
    .await
    .context("Failed to list pending suggestions.")?;

    let views: Vec<SuggestionView> = rows
        .into_iter()
        .map(|r| SuggestionView {
            id: r.id,
            farm_id: r.farm_id,
            product_slug: r.product_slug,
            action: r.action,
            note: r.note,
            submitted_by: r.submitted_by,
            created_at: r.created_at,
        })
        .collect();

    Ok(HttpResponse::Ok().json(views))
}

/// POST /admin/product-suggestions/{id}/approve
#[tracing::instrument(name = "Approve suggestion", skip(pool))]
pub async fn approve(
    admin: AdminUser,
    path: web::Path<Uuid>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, AdminError> {
    let id = path.into_inner();
    let mut transaction = pool
        .begin()
        .await
        .context("Failed to acquire a Postgres connection from the pool.")?;

    // Atomic state transition: only a PENDING row can be claimed. Concurrent
    // approvers race here and exactly one wins.
    let claimed = sqlx::query!(
        r#"
        UPDATE farm_product_suggestions
        SET status = 'APPROVED', reviewed_by = $1, reviewed_at = $2
        WHERE id = $3 AND status = 'PENDING'
        RETURNING farm_id, product_id, action AS "action: SuggestionAction"
        "#,
        admin.0.id,
        Utc::now(),
        id,
    )
    .fetch_optional(&mut *transaction)
    .await
    .context("Failed to claim the suggestion.")?;

    let Some(claimed) = claimed else {
        // Already reviewed, or does not exist.
        return Err(AdminError::Conflict);
    };

    match claimed.action {
        SuggestionAction::Add => {
            sqlx::query!(
                r#"
                INSERT INTO farm_products (farm_id, product_id)
                VALUES ($1, $2)
                ON CONFLICT DO NOTHING
                "#,
                claimed.farm_id,
                claimed.product_id,
            )
            .execute(&mut *transaction)
            .await
            .context("Failed to apply ADD.")?;
        }
        SuggestionAction::Remove => {
            sqlx::query!(
                r#"DELETE FROM farm_products WHERE farm_id = $1 AND product_id = $2"#,
                claimed.farm_id,
                claimed.product_id,
            )
            .execute(&mut *transaction)
            .await
            .context("Failed to apply REMOVE.")?;
        }
    }

    transaction
        .commit()
        .await
        .context("Failed to commit approval.")?;

    Ok(HttpResponse::Ok().finish())
}

/// POST /admin/product-suggestions/{id}/reject — same claim, no apply step.
#[tracing::instrument(name = "Reject suggestion", skip(pool))]
pub async fn reject(
    admin: AdminUser,
    path: web::Path<Uuid>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, AdminError> {
    let id = path.into_inner();

    let claimed = sqlx::query!(
        r#"
        UPDATE farm_product_suggestions
        SET status = 'REJECTED', reviewed_by = $1, reviewed_at = $2
        WHERE id = $3 AND status = 'PENDING'
        RETURNING id
        "#,
        admin.0.id,
        Utc::now(),
        id,
    )
    .fetch_optional(pool.get_ref())
    .await
    .context("Failed to reject the suggestion.")?;

    if claimed.is_none() {
        return Err(AdminError::Conflict);
    }

    Ok(HttpResponse::Ok().finish())
}

// Suppress the unused-status-import lint if SuggestionStatus is only used in SQL.
const _: Option<SuggestionStatus> = None;
```

> The trailing `const _` line is only to keep `SuggestionStatus` imported if you
> reference it in a typed query elsewhere; delete it if unused.

#### New file: `src/routes/admin/mod.rs`

```rust
mod error;
mod suggestions;

pub use error::AdminError;
pub use suggestions::{approve, list_pending, reject};
```

#### Edit: `src/routes/mod.rs`

```rust
pub mod admin;
pub mod authentication;
pub mod farms;
mod health_check;
pub mod suggestions;

pub use health_check::*;
```

### 5.4 Route wiring

#### Edit: `src/startup.rs`

```rust
.route(
    "/admin/product-suggestions",
    web::get().to(crate::routes::admin::list_pending),
)
.route(
    "/admin/product-suggestions/{id}/approve",
    web::post().to(crate::routes::admin::approve),
)
.route(
    "/admin/product-suggestions/{id}/reject",
    web::post().to(crate::routes::admin::reject),
)
```

### 5.5 Tests

#### New file: `tests/api/moderation.rs` (+ `mod moderation;` in `tests/api/main.rs`)

```rust
use crate::helpers::{insert_test_farm, seed_test_taxonomy, spawn_app};
use actix_web::http::StatusCode;
use farms::configuration::IdempotencyEngine;
use uuid::Uuid;

#[tokio::test]
async fn non_admin_cannot_list_the_queue() {
    let app = spawn_app(IdempotencyEngine::None).await;
    app.log_in_active_user().await; // a plain USER

    let response = app
        .api_client
        .get(format!("{}/admin/product-suggestions", app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::FORBIDDEN.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn approving_add_creates_the_link_and_is_idempotent_on_second_approve() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;
    let farm_id = insert_test_farm(&app.db_pool, "Farm").await;

    // Insert a pending ADD suggestion directly (bypassing the submit endpoint).
    let suggestion_id = Uuid::new_v4();
    let submitter = app.log_in_admin_user().await; // returns the admin's user id
    sqlx::query!(
        r#"
        INSERT INTO farm_product_suggestions
            (id, farm_id, product_id, action, note, submitted_by, status, created_at)
        VALUES ($1, $2, $3, 'ADD', NULL, $4, 'PENDING', now())
        "#,
        suggestion_id,
        farm_id,
        taxonomy.strawberries_id,
        submitter,
    )
    .execute(&app.db_pool)
    .await
    .unwrap();

    let first = app
        .api_client
        .post(format!(
            "{}/admin/product-suggestions/{}/approve",
            app.address, suggestion_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::OK.as_u16(), first.status().as_u16());

    let count = sqlx::query!(
        r#"SELECT count(*) AS "count!" FROM farm_products WHERE farm_id = $1 AND product_id = $2"#,
        farm_id,
        taxonomy.strawberries_id,
    )
    .fetch_one(&app.db_pool)
    .await
    .unwrap()
    .count;
    assert_eq!(1, count);

    // A second approve is a conflict (no longer pending).
    let second = app
        .api_client
        .post(format!(
            "{}/admin/product-suggestions/{}/approve",
            app.address, suggestion_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::CONFLICT.as_u16(), second.status().as_u16());
}
```

> The tests assume `log_in_active_user` (USER) and `log_in_admin_user` (ADMIN,
> returning its id) session helpers. If your suite obtains sessions differently,
> adapt these calls; the moderation logic under test is unchanged.

---

## PR 6 — Stocking status & seasonality

**Goal:** per-(farm, product) availability state.

### 6.1 Migration

#### File: `migrations/<timestamp>_add_farm_product_status.sql`

```sql
CREATE TYPE stock_status AS ENUM ('AVAILABLE', 'SEASONAL', 'UNAVAILABLE');

ALTER TABLE farm_products
    ADD COLUMN status            stock_status NOT NULL DEFAULT 'AVAILABLE',
    ADD COLUMN last_confirmed_at timestamptz;

CREATE INDEX farm_products_available_idx
    ON farm_products (product_id)
    WHERE status = 'AVAILABLE';
```

### 6.2 Domain enum

#### New file: `src/domain/farm/stock_status.rs` (+ export from `src/domain/farm/mod.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, serde::Serialize, serde::Deserialize)]
#[sqlx(type_name = "stock_status", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "snake_case")]
pub enum StockStatus {
    Available,
    Seasonal,
    Unavailable,
}
```

### 6.3 Surface it in the read DTO

#### Edit: `src/routes/farms/mod.rs` — extend `ProductDto`

```rust
use crate::domain::farm::StockStatus;
use chrono::{DateTime, Utc};

#[derive(serde::Serialize, Clone)]
pub struct ProductDto {
    pub slug: String,
    pub name_en: String,
    pub group: String,
    pub status: StockStatus,
    pub last_confirmed_at: Option<DateTime<Utc>>,
}
```

#### Edit: the product-load queries in `src/routes/farms/get.rs`

Both product queries (list and single) gain the two columns; the `ProductDto`
construction gains the two fields. The list query becomes:

```rust
let product_rows = sqlx::query!(
    r#"
    SELECT
        fp.farm_id,
        p.slug              AS "slug!",
        p.name_en           AS "name_en!",
        c.slug              AS "group_slug!",
        fp.status           AS "status!: crate::domain::farm::StockStatus",
        fp.last_confirmed_at
    FROM farm_products fp
    JOIN products p ON p.id = fp.product_id
    JOIN product_categories c ON c.id = p.category_id
    WHERE fp.farm_id = ANY($1)
    ORDER BY p.slug
    "#,
    &farm_ids,
)
.fetch_all(pool)
.await
.context("Failed to load farm products.")?;

// ...
products_by_farm
    .entry(row.farm_id)
    .or_default()
    .push(ProductDto {
        slug: row.slug,
        name_en: row.name_en,
        group: row.group_slug,
        status: row.status,
        last_confirmed_at: row.last_confirmed_at,
    });
```

Apply the same two-column addition to `get_farm_by_id`'s product query and its
`ProductDto` construction.

### 6.4 Bump freshness on approved ADD

#### Edit: `src/routes/admin/suggestions.rs` — the `SuggestionAction::Add` arm

```rust
SuggestionAction::Add => {
    sqlx::query!(
        r#"
        INSERT INTO farm_products (farm_id, product_id, last_confirmed_at)
        VALUES ($1, $2, $3)
        ON CONFLICT (farm_id, product_id)
        DO UPDATE SET last_confirmed_at = EXCLUDED.last_confirmed_at,
                      status = 'AVAILABLE'
        "#,
        claimed.farm_id,
        claimed.product_id,
        Utc::now(),
    )
    .execute(&mut *transaction)
    .await
    .context("Failed to apply ADD.")?;
}
```

### 6.5 Test

#### Add to `tests/api/moderation.rs`

```rust
#[tokio::test]
async fn approved_add_marks_product_available_and_confirmed() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;
    let farm_id = insert_test_farm(&app.db_pool, "Farm").await;
    let submitter = app.log_in_admin_user().await;

    let suggestion_id = Uuid::new_v4();
    sqlx::query!(
        r#"
        INSERT INTO farm_product_suggestions
            (id, farm_id, product_id, action, note, submitted_by, status, created_at)
        VALUES ($1, $2, $3, 'ADD', NULL, $4, 'PENDING', now())
        "#,
        suggestion_id,
        farm_id,
        taxonomy.strawberries_id,
        submitter,
    )
    .execute(&app.db_pool)
    .await
    .unwrap();

    app.api_client
        .post(format!(
            "{}/admin/product-suggestions/{}/approve",
            app.address, suggestion_id
        ))
        .send()
        .await
        .unwrap();

    let row = sqlx::query!(
        r#"
        SELECT status::text AS "status!", last_confirmed_at
        FROM farm_products WHERE farm_id = $1 AND product_id = $2
        "#,
        farm_id,
        taxonomy.strawberries_id,
    )
    .fetch_one(&app.db_pool)
    .await
    .unwrap();
    assert_eq!("AVAILABLE", row.status);
    assert!(row.last_confirmed_at.is_some());
}
```

---

## Cross-cutting checklist (every PR)

- [ ] `sqlx migrate run` + `cargo sqlx prepare --workspace -- --all-targets`, commit `.sqlx/`.
- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- [ ] New routes registered in `src/startup.rs`; new Bruno requests in `api_docs/`.
- [ ] `#[tracing::instrument]` on new service/query fns; never log secrets.
- [ ] `EXPLAIN (ANALYZE, BUFFERS)` the list/filter queries; confirm index scans.

## Dependency on the seeding PR

Nothing here populates `product_categories` / `products` / `farm_products` from
the dataset — that is the separate seeding PR. Until it runs:

- The `TaxonomySnapshot` is empty, so `POST /farms` rejects every product and
  `GET /farms?product=…` returns 400/empty.
- Integration tests are unaffected: they seed their own fixtures via
  `seed_test_taxonomy` / `insert_test_farm` / `link_farm_product`.

Restart the app after the seeding PR runs so the boot-time snapshot is populated.

## Open decisions

1. Keep `categories` in the API response (derived, as here) or drop it entirely
   and expose only `products` + groups?
2. Comma-separated `?product=a,b` (as here) or add `serde_qs` for repeated keys?
3. Default filter semantics OR-with-`match=all` (as here) or AND by default?
4. Suggestions authenticated-only (as here) or allow anonymous with stricter limits?
5. Email the submitter on approve/reject (uses the existing `EmailClient`)?

---

## Amendment (2026-07-15) — mixed-granularity + what shipped

The original spec derived a farm's `categories` **only** from its granular
`products`. Real data doesn't cooperate: a large share of farms are classified
at the **group level** ("sells vegetables") with no specific product. Under the
original model those farms would have empty `categories` and disappear from
category search. Implemented change:

- **New table `farm_categories (farm_id, category_id)`** — authoritative
  group-level membership, independent of granular products
  (`migrations/*_create_farm_categories.sql`).
- **`TaxonomySnapshot`** also resolves category slugs (`category_id_for_slug`)
  for validating `?category=` and category creates at the edge.
- **`GET /farms`** gained `?category=slug,slug` (matches the group directly via
  `farm_categories` **OR** via a product in the group), and the response
  `categories` is now the **union** of direct group links and product groups —
  so a farm surfaces under a category whether its data is coarse or granular.
  `?product=slug` still matches only the granular link (true granularity).
- **`POST /farms`** accepts `categories` and/or `products` (≥1 required), so a
  farm can be added at whatever granularity the source data has.

### Status of the guide's PRs

PR1 (schema), PR2 (read + taxonomy snapshot + startup wiring), PR3 (write),
PR4 (suggestions), PR5 (moderation), PR6 (stock status), plus this amendment
are all implemented. Full verification against a live Postgres: `cargo test`
(138 unit + 63 api + 6 auth), `cargo clippy --all-targets -- -D warnings`,
`cargo fmt --check`, and `cargo sqlx prepare --workspace -- --all-targets` all
green. The dataset **seeding** of `product_categories` / `products` /
`farm_categories` / `farm_products` from the source data remains a separate PR
(the app's taxonomy snapshot is empty until seeding runs + the app restarts).

### Still recommended (frontend-facing decisions, not yet done)

1. **Nearest-first is not server-side.** Pagination is keyset on
   `(created_at DESC, id DESC)`; the app's signature "nearest farms" flow needs
   the user's coordinates + distance sort (PostGIS `<->`) or the frontend keeps
   pulling everything to sort client-side. Biggest open gap.
2. **Product i18n.** `ProductDto` exposes only `slug` + `name_en`; the app is
   5-locale. Cleanest: expose the stable `slug` and let the frontend own
   translations (as it already does for categories), with `name_en` as a
   fallback.
3. **`categories` is now group slugs** (not the old German group names).
   Deliberately — clients key on slugs going forward. Not backward compatible
   with any client still keying on the old values.
4. **Duplicate pending suggestions** aren't prevented; consider a unique partial
   index `(farm_id, product_id, submitted_by) WHERE status = 'PENDING'`.
5. **`SEASONAL`** is a flag with no month range; a backend-driven seasonal
   calendar would need per-(farm, product) season months.
