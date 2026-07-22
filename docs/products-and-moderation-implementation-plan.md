# Per-Product Farm Data & Community Moderation — Implementation Plan

This document contains the concrete code for shipping **product-level farm data**
(so users can search for "strawberries", not just "fruit") and a **community
moderation** workflow (users suggest product updates; admins approve them).

It is split into **6 PRs**, each independently shippable and green. Code follows
the existing repo conventions: Zero-to-Production-style newtypes (`parse()` +
`thiserror`), thin Actix handlers, explicit SQLx queries, the idempotency/Valkey
modules, and `CurrentUser`/`Role` auth.

> After **any** migration: run `sqlx migrate run` then
> `cargo sqlx prepare --workspace -- --all-targets` and commit `.sqlx/`.
> After **any** code change: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`,
> `cargo test`.

## Locked design decisions

1. **Normalised schema.** `product_categories` (groups) → `products` (subcategories)
   → `farm_products` (a farm offers a product). The relationship is a *table*, not
   an array, because Phase 2 attaches per-(farm, product) state to it.
2. **Drop `farms.categories`** (the `TEXT[]`); derive group slugs from
   `farm_products`. The API keeps returning `categories` for backward compatibility,
   computed at read time.
3. **Backend owns existence + grouping** (`key_de`, `slug`, `name_en`); the
   **frontend keeps owning localized labels** keyed by `slug`.
4. **Search is an indexed equality on `slug`.** The frontend resolves the user's
   localized input to a `slug` and calls `?product=<slug>`.
5. **Filter default is OR** ("sells any of"); `match=all` switches to AND.
6. **Suggestions require an authenticated user**; moderation requires `Role::Admin`.

---

# PR 1 — Schema & seed

**Goal:** introduce the taxonomy + relationship tables and load them from the
dataset. No API change yet.

## 1.1 Migration — `migrations/<ts>_create_product_taxonomy.sql`

```sql
-- Category groups: the small, stable, admin-owned vocabulary (13 rows).
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
    name_en     text     NOT NULL            -- 'Strawberries'
);
CREATE INDEX products_category_id_idx ON products (category_id);

-- The relationship: which products a farm offers. This is the source of truth.
CREATE TABLE farm_products (
    farm_id    uuid    NOT NULL REFERENCES farms (id)    ON DELETE CASCADE,
    product_id integer NOT NULL REFERENCES products (id) ON DELETE RESTRICT,
    PRIMARY KEY (farm_id, product_id)
);
-- The PK covers farm -> products; this covers the reverse (product -> farms),
-- which product search needs.
CREATE INDEX farm_products_product_id_idx ON farm_products (product_id);
```

**Why:** surrogate integer keys keep the large `farm_products` join compact and
stable under renames; `ON DELETE CASCADE` cleans up links when a farm is deleted;
`ON DELETE RESTRICT` stops a product disappearing while farms reference it; the
composite PK gives free per-farm uniqueness.

> The `farms.categories` column is **kept for now** (the existing seeder still
> fills it). PR 2 removes it once reads move to `farm_products`.

## 1.2 Dataset models — `src/bin/seed/dataset.rs` (new seeder binary)

We add a dedicated seeding binary so loading is in Rust (typed, testable,
transactional) rather than ad-hoc SQL. Register it in `Cargo.toml`:

```toml
[[bin]]
name = "seed"
path = "src/bin/seed/main.rs"
```

`src/bin/seed/dataset.rs` — typed view of `data/farms_with_categorized_products.patched.json`:

```rust
use serde::Deserialize;
use std::collections::BTreeMap;

/// Top-level dataset shape: { "<anything>": Location, ... } OR a list.
/// The real file is an object keyed by url_title; we only need the values.
#[derive(Debug, Deserialize)]
pub struct Dataset(pub BTreeMap<String, Location>);

#[derive(Debug, Deserialize)]
pub struct Location {
    pub url_title: Option<String>,
    pub title: Option<String>,
    pub city: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub address: Option<Address>,
    pub display_name: Option<String>,
    /// group(de) -> [ { de, en } ]
    #[serde(default)]
    pub categorized_products: BTreeMap<String, Vec<Product>>,
}

#[derive(Debug, Deserialize)]
pub struct Address {
    #[serde(rename = "ISO3166-2-lvl4")]
    pub iso_lvl4: Option<String>,
    pub road: Option<String>,
    pub postcode: Option<String>,
    pub village: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Product {
    pub de: String,
    pub en: String,
}
```

## 1.3 Canonical groups + slugging — `src/bin/seed/taxonomy.rs`

```rust
use std::collections::BTreeMap;

/// The 13 canonical groups, in display order. Mirrors lib/categories.ts.
/// (key_de, slug)
pub const GROUPS: &[(&str, &str)] = &[
    ("Früchte", "fruits"),
    ("Gemüse", "vegetables"),
    ("Milchprodukte", "dairy"),
    ("Fleisch und Geflügel", "meat-poultry"),
    ("Verarbeitete und haltbare Produkte", "preserves-processed"),
    ("Honig und Süßstoffe", "honey-sweeteners"),
    ("Getränke", "drinks"),
    ("Backwaren und Gebäck", "bakery"),
    ("Blumen und Pflanzen", "flowers-plants"),
    ("Nüsse, Samen und Öle", "nuts-seeds-oils"),
    // ... complete from lib/categories.ts (all 13) ...
];

/// Deterministic slug from an English product name: lowercase ASCII, words
/// joined by '-'. Falls back to a transliteration of the German key for the
/// few products with empty/odd English.
pub fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = false;
    for ch in s.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Build the distinct set of products across all farms: key_de -> (group_de, name_en).
pub fn collect_products(
    dataset: &crate::dataset::Dataset,
) -> BTreeMap<String, (String, String)> {
    let mut products = BTreeMap::new();
    for loc in dataset.0.values() {
        for (group_de, items) in &loc.categorized_products {
            for p in items {
                products
                    .entry(p.de.clone())
                    .or_insert_with(|| (group_de.clone(), p.en.clone()));
            }
        }
    }
    products
}
```

## 1.4 The seeder — `src/bin/seed/main.rs`

```rust
mod dataset;
mod taxonomy;

use anyhow::Context;
use dataset::Dataset;
use farms::configuration::get_configuration;
use farms::startup::get_connection_pool;
use sqlx::PgPool;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let configuration = get_configuration().context("Failed to read configuration.")?;
    let pool = get_connection_pool(&configuration.database);

    let raw = std::fs::read_to_string("data/farms_with_categorized_products.patched.json")
        .context("Failed to read dataset file.")?;
    let dataset: Dataset = serde_json::from_str(&raw).context("Failed to parse dataset.")?;

    seed_taxonomy(&pool, &dataset).await?;
    seed_farms_and_products(&pool, &dataset).await?;

    tracing::info!("Seeding complete.");
    Ok(())
}

/// Upsert groups and products. Idempotent: safe to re-run.
async fn seed_taxonomy(pool: &PgPool, dataset: &Dataset) -> anyhow::Result<()> {
    // Groups.
    for (order, (key_de, slug)) in taxonomy::GROUPS.iter().enumerate() {
        sqlx::query!(
            r#"
            INSERT INTO product_categories (key_de, slug, display_order)
            VALUES ($1, $2, $3)
            ON CONFLICT (key_de) DO UPDATE
              SET slug = EXCLUDED.slug, display_order = EXCLUDED.display_order
            "#,
            key_de,
            slug,
            order as i16,
        )
        .execute(pool)
        .await
        .with_context(|| format!("Failed to upsert group {key_de}"))?;
    }

    // Map group key_de -> category_id, so products can reference it.
    let group_ids = load_group_ids(pool).await?;

    // Products derived from the dataset.
    for (key_de, (group_de, name_en)) in taxonomy::collect_products(dataset) {
        let category_id = *group_ids
            .get(&group_de)
            .with_context(|| format!("Product '{key_de}' references unknown group '{group_de}'"))?;
        let slug = taxonomy::slugify(if name_en.is_empty() { &key_de } else { &name_en });

        sqlx::query!(
            r#"
            INSERT INTO products (category_id, key_de, slug, name_en)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (key_de) DO UPDATE
              SET category_id = EXCLUDED.category_id,
                  slug        = EXCLUDED.slug,
                  name_en     = EXCLUDED.name_en
            "#,
            category_id,
            key_de,
            slug,
            name_en,
        )
        .execute(pool)
        .await
        .with_context(|| format!("Failed to upsert product {key_de}"))?;
    }
    Ok(())
}

async fn load_group_ids(pool: &PgPool) -> anyhow::Result<HashMap<String, i16>> {
    let rows = sqlx::query!("SELECT id, key_de FROM product_categories")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|r| (r.key_de, r.id)).collect())
}

/// Insert each farm and its product links in ONE transaction per farm so a farm
/// is never half-linked. Preload the key_de -> product_id map to avoid N+1.
async fn seed_farms_and_products(pool: &PgPool, dataset: &Dataset) -> anyhow::Result<()> {
    let product_ids: HashMap<String, i32> = sqlx::query!("SELECT id, key_de FROM products")
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|r| (r.key_de, r.id))
        .collect();

    for loc in dataset.0.values() {
        let mut tx = pool.begin().await?;
        let farm_id = uuid::Uuid::new_v4();

        // ... INSERT INTO farms (...) VALUES (farm_id, ...) using the same column
        //     mapping the existing seeder uses (name/address/canton/coordinates) ...

        for items in loc.categorized_products.values() {
            for p in items {
                if let Some(&product_id) = product_ids.get(&p.de) {
                    sqlx::query!(
                        r#"
                        INSERT INTO farm_products (farm_id, product_id)
                        VALUES ($1, $2)
                        ON CONFLICT DO NOTHING
                        "#,
                        farm_id,
                        product_id,
                    )
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }
        tx.commit().await?;
    }
    Ok(())
}
```

**Run:** `cargo run --bin seed` (locally / as a Render one-off job).

## 1.5 Tests — `tests/api/taxonomy.rs` (+ `mod taxonomy;` in `tests/api/main.rs`)

```rust
// Seed a tiny taxonomy into the per-test DB and assert the FK wiring holds.
// (Add a `seed_minimal_taxonomy(&pool)` helper to tests/common/mod.rs that
//  inserts one group + two products + returns their ids.)

#[tokio::test]
async fn farm_products_enforces_referential_integrity() {
    let app = spawn_app().await;
    let (_group, product_ids) = seed_minimal_taxonomy(&app.db_pool).await;
    let farm_id = insert_bare_farm(&app.db_pool).await;

    // Valid link succeeds.
    sqlx::query!("INSERT INTO farm_products (farm_id, product_id) VALUES ($1, $2)",
        farm_id, product_ids[0]).execute(&app.db_pool).await.unwrap();

    // Unknown product is rejected by the FK.
    let bad = sqlx::query!("INSERT INTO farm_products (farm_id, product_id) VALUES ($1, $2)",
        farm_id, 999_999).execute(&app.db_pool).await;
    assert!(bad.is_err());
}
```

---

# PR 2 — Read API + product search

**Goal:** return `products` on farms, derive `categories`, and filter by product
with keyset pagination. Drop the stored `farms.categories`.

## 2.1 Migration — `migrations/<ts>_drop_farms_categories.sql`

```sql
ALTER TABLE farms DROP COLUMN categories;
```

## 2.2 The taxonomy snapshot — `src/taxonomy/mod.rs` (+ `pub mod taxonomy;` in `lib.rs`)

An in-memory, read-mostly index of the taxonomy: fast slug→id resolution and
early 400s without a DB round trip. Loaded at startup, shared via `web::Data`.

```rust
use sqlx::PgPool;
use std::collections::HashMap;

#[derive(Clone)]
pub struct ProductInfo {
    pub id: i32,
    pub slug: String,
    pub name_en: String,
    pub group_slug: String,
}

#[derive(Clone, Default)]
pub struct TaxonomySnapshot {
    by_slug: HashMap<String, ProductInfo>,
    by_id: HashMap<i32, ProductInfo>,
}

impl TaxonomySnapshot {
    #[tracing::instrument(name = "Load taxonomy snapshot", skip(pool))]
    pub async fn load(pool: &PgPool) -> Result<Self, anyhow::Error> {
        let rows = sqlx::query!(
            r#"
            SELECT p.id, p.slug, p.name_en, c.slug AS group_slug
            FROM products p
            JOIN product_categories c ON c.id = p.category_id
            "#
        )
        .fetch_all(pool)
        .await?;

        let mut by_slug = HashMap::new();
        let mut by_id = HashMap::new();
        for r in rows {
            let info = ProductInfo { id: r.id, slug: r.slug.clone(),
                name_en: r.name_en, group_slug: r.group_slug };
            by_slug.insert(r.slug, info.clone());
            by_id.insert(r.id, info);
        }
        Ok(Self { by_slug, by_id })
    }

    /// Resolve a slug to a product id, or None if unknown.
    pub fn id_for_slug(&self, slug: &str) -> Option<i32> {
        self.by_slug.get(slug).map(|p| p.id)
    }
    pub fn by_id(&self, id: i32) -> Option<&ProductInfo> {
        self.by_id.get(&id)
    }
}
```

Wire it into `startup.rs`:

```rust
let taxonomy = TaxonomySnapshot::load(&connection_pool).await?;
let taxonomy = Data::new(taxonomy);
// ... in the App factory: .app_data(taxonomy.clone())
```

> Multi-instance note: this snapshot is per-process and only refreshed at boot.
> That's fine until the taxonomy becomes admin-editable; then back it with a
> Valkey version key and reload when it bumps. Out of scope for PR 2.

## 2.3 DTOs + query — rework `src/routes/farms/`

`Farm` (the DB row) no longer carries `categories`. Introduce a response DTO that
adds `products` and a derived `categories`.

`src/routes/farms/mod.rs`:

```rust
#[derive(serde::Serialize)]
pub struct FarmResponse {
    pub id: Uuid,
    pub name: Name,
    pub address: Address,
    pub canton: Canton,
    pub coordinates: Point,
    pub categories: Vec<String>, // derived group slugs (backward compatible)
    pub products: Vec<ProductDto>,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(serde::Serialize, Clone)]
pub struct ProductDto {
    pub slug: String,
    pub name_en: String,
    pub group: String,
}
```

`src/routes/farms/get.rs` — keyset pagination + batch product load (no N+1):

```rust
#[derive(serde::Deserialize)]
pub struct FarmListQuery {
    /// Repeatable: ?product=strawberries&product=cherries
    #[serde(default)]
    pub product: Vec<String>,
    /// "any" (default) or "all"
    pub r#match: Option<String>,
    /// Keyset cursor: opaque "<rfc3339>_<uuid>" of the last row seen.
    pub after: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}
fn default_limit() -> i64 { 20 }

#[tracing::instrument(name = "List farms", skip(pool, taxonomy))]
pub async fn get_all(
    query: web::Query<FarmListQuery>,
    pool: web::Data<PgPool>,
    taxonomy: web::Data<TaxonomySnapshot>,
) -> Result<HttpResponse, FarmError> {
    let limit = query.limit.clamp(1, 100);

    // 1. Resolve product slugs -> ids (early 400 on unknown).
    let product_ids = query.product.iter()
        .map(|s| taxonomy.id_for_slug(s)
            .ok_or_else(|| FarmError::ValidationError(format!("Unknown product '{s}'."))))
        .collect::<Result<Vec<_>, _>>()?;
    let match_all = query.r#match.as_deref() == Some("all");

    // 2. Decode the keyset cursor, if any.
    let cursor = query.after.as_deref().map(parse_cursor).transpose()
        .map_err(|_| FarmError::ValidationError("Invalid cursor.".into()))?;

    // 3. Page the farm ids (filtered + keyset), then batch-load products.
    let farms = list_farms(&pool, &product_ids, match_all, cursor, limit).await?;
    Ok(HttpResponse::Ok().json(farms))
}
```

The two-query implementation (page farms, then one product query for the page):

```rust
#[tracing::instrument(skip(pool))]
async fn list_farms(
    pool: &PgPool,
    product_ids: &[i32],
    match_all: bool,
    cursor: Option<(DateTime<Utc>, Uuid)>,
    limit: i64,
) -> Result<Vec<FarmResponse>, FarmError> {
    let (cur_ts, cur_id) = match cursor {
        Some((ts, id)) => (Some(ts), Some(id)),
        None => (None, None),
    };

    // Page of farms. The `product_ids` filter is applied via an EXISTS/HAVING
    // subquery; keyset predicate uses the (created_at DESC, id DESC) index.
    let rows = sqlx::query!(
        r#"
        SELECT f.id, f.name, f.address, f.canton, f.coordinates AS "coordinates: Point",
               f.created_at, f.updated_at
        FROM farms f
        WHERE
          -- product filter (empty array => no filter)
          (cardinality($1::int[]) = 0 OR f.id IN (
              SELECT farm_id FROM farm_products
              WHERE product_id = ANY($1)
              GROUP BY farm_id
              HAVING ($2 = false) OR (count(DISTINCT product_id) = cardinality($1))
          ))
          -- keyset cursor
          AND ($3::timestamptz IS NULL OR (f.created_at, f.id) < ($3, $4))
        ORDER BY f.created_at DESC, f.id DESC
        LIMIT $5
        "#,
        product_ids,
        match_all,
        cur_ts,
        cur_id,
        limit,
    )
    .fetch_all(pool)
    .await
    .context("Failed to page farms.")?;

    let farm_ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();

    // Batch-load ALL products for these farms in one query (no N+1).
    let prod_rows = sqlx::query!(
        r#"
        SELECT fp.farm_id, p.slug, p.name_en, c.slug AS group_slug
        FROM farm_products fp
        JOIN products p ON p.id = fp.product_id
        JOIN product_categories c ON c.id = p.category_id
        WHERE fp.farm_id = ANY($1)
        "#,
        &farm_ids,
    )
    .fetch_all(pool)
    .await
    .context("Failed to load farm products.")?;

    // Group products by farm_id in memory.
    let mut products_by_farm: std::collections::HashMap<Uuid, Vec<ProductDto>> = Default::default();
    for r in prod_rows {
        products_by_farm.entry(r.farm_id).or_default().push(ProductDto {
            slug: r.slug, name_en: r.name_en, group: r.group_slug,
        });
    }

    // Assemble responses, deriving `categories` (distinct group slugs).
    let result = rows.into_iter().map(|r| {
        let products = products_by_farm.remove(&r.id).unwrap_or_default();
        let mut categories: Vec<String> = products.iter().map(|p| p.group.clone()).collect();
        categories.sort();
        categories.dedup();
        FarmResponse {
            id: r.id,
            name: Name::from_db(r.name),           // see note below
            // ... map remaining columns ...
            categories,
            products,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }).collect();

    Ok(result)
}
```

> **Note on the domain newtypes:** `query!` returns raw `String`s for `name`,
> `address`, etc. Either keep using `query_as!` with the `as "name: Name"`
> casts (as today), or add a small `from_db(String)` constructor on each newtype
> for trusted DB values. Pick one and stay consistent.

Cursor helpers:

```rust
fn parse_cursor(s: &str) -> Result<(DateTime<Utc>, Uuid), anyhow::Error> {
    let (ts, id) = s.split_once('_').context("malformed cursor")?;
    Ok((DateTime::parse_from_rfc3339(ts)?.with_timezone(&Utc), Uuid::parse_str(id)?))
}
pub fn make_cursor(ts: DateTime<Utc>, id: Uuid) -> String {
    format!("{}_{}", ts.to_rfc3339(), id)
}
```

`get_by_id` becomes: fetch the farm row, then one product query for its id, assemble.

## 2.4 Tests — `tests/api/farms.rs` additions

```rust
#[tokio::test]
async fn list_filters_by_product_slug() { /* seed 2 farms, 1 with strawberries; ?product=strawberries returns only it, with its full product list */ }

#[tokio::test]
async fn match_all_requires_every_product() { /* ?product=a&product=b&match=all */ }

#[tokio::test]
async fn unknown_product_slug_is_400() { /* ?product=dragonfruit */ }

#[tokio::test]
async fn keyset_pagination_is_stable_and_non_overlapping() { /* page with after= cursor */ }

#[tokio::test]
async fn categories_are_derived_from_products() { /* response.categories == distinct group slugs */ }
```

---

# PR 3 — Write API (create farm with products)

**Goal:** `POST /farms` accepts `products: [slug]`, validated and written
transactionally, reusing idempotency.

## 3.1 Domain newtype — `src/domain/farm/product_slug.rs`

```rust
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProductSlug(String);

#[derive(Debug, Error)]
pub enum ProductSlugError {
    #[error("Product slug cannot be empty.")]
    Empty,
    #[error("Product slug is too long.")]
    TooLong,
    #[error("Product slug may only contain lowercase letters, digits and hyphens.")]
    InvalidCharacters,
}

impl ProductSlug {
    pub fn parse(s: String) -> Result<Self, ProductSlugError> {
        let t = s.trim().to_lowercase();
        if t.is_empty() { return Err(ProductSlugError::Empty); }
        if t.len() > 64 { return Err(ProductSlugError::TooLong); }
        if !t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err(ProductSlugError::InvalidCharacters);
        }
        Ok(Self(t))
    }
    pub fn as_str(&self) -> &str { &self.0 }
}
```

Re-export from `src/domain/farm/mod.rs`.

## 3.2 Handler — `src/routes/farms/post.rs`

Extend `FormData` with `products`, validate them, and insert links inside the
existing idempotency transaction:

```rust
#[derive(serde::Deserialize)]
pub struct FormData {
    name: String,
    address: String,
    canton: String,
    coordinates: String,
    products: Vec<String>,     // <-- replaces free-form `categories`
    idempotency_key: String,
}

// inside `create`, after the other validations:
let product_slugs = body.products.iter()
    .map(|s| ProductSlug::parse(s.clone()).map_err(|e| FarmError::ValidationError(e.to_string())))
    .collect::<Result<Vec<_>, _>>()?;
if product_slugs.is_empty() {
    return Err(FarmError::ValidationError("At least one product is required.".into()));
}
// Resolve slugs -> ids via the snapshot (unknown => 400, no DB hit).
let product_ids = product_slugs.iter()
    .map(|s| taxonomy.id_for_slug(s.as_str())
        .ok_or_else(|| FarmError::ValidationError(format!("Unknown product '{}'.", s.as_str()))))
    .collect::<Result<Vec<_>, _>>()?;

// ... existing try_processing(...) to open the idempotent transaction ...

let farm_id = insert_farm(&mut transaction, name, address, canton, coordinates).await?;
insert_farm_products(&mut transaction, farm_id, &product_ids).await?;

// ... existing save_response(...) + commit ...
```

`insert_farm` now returns the `Uuid`; the link insert:

```rust
#[tracing::instrument(name = "Insert farm products", skip(transaction))]
async fn insert_farm_products(
    transaction: &mut Transaction<'_, Postgres>,
    farm_id: Uuid,
    product_ids: &[i32],
) -> Result<(), FarmError> {
    // Single round-trip bulk insert via UNNEST.
    sqlx::query!(
        r#"
        INSERT INTO farm_products (farm_id, product_id)
        SELECT $1, * FROM UNNEST($2::int[])
        ON CONFLICT DO NOTHING
        "#,
        farm_id,
        product_ids,
    )
    .execute(&mut **transaction)
    .await
    .context("Failed to insert farm products.")?;
    Ok(())
}
```

Add `taxonomy: web::Data<TaxonomySnapshot>` to the `create` handler signature.

## 3.3 Tests

```rust
#[tokio::test]
async fn create_farm_persists_product_links() { /* POST with products, then GET shows them */ }

#[tokio::test]
async fn create_farm_rejects_unknown_product() { /* 400 */ }

#[tokio::test]
async fn create_farm_is_idempotent_for_products() { /* same idempotency_key twice => one set of links */ }
```

---

# PR 4 — User-submitted product suggestions

**Goal:** authenticated users submit "add/remove product X on farm Y"; stored as
`PENDING`. Auth + rate-limit + idempotency.

## 4.1 Migration — `migrations/<ts>_create_farm_product_suggestions.sql`

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
    ON farm_product_suggestions (created_at)
    WHERE status = 'PENDING';
```

`sqlx::Type` enums — `src/domain/suggestion.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, serde::Serialize, serde::Deserialize)]
#[sqlx(type_name = "suggestion_status", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "snake_case")]
pub enum SuggestionStatus { Pending, Approved, Rejected }

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, serde::Serialize, serde::Deserialize)]
#[sqlx(type_name = "suggestion_action", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "snake_case")]
pub enum SuggestionAction { Add, Remove }
```

## 4.2 Route — `src/routes/suggestions/post.rs`

```rust
#[derive(serde::Deserialize)]
pub struct SuggestionRequest {
    product: String,          // slug
    action: SuggestionAction,
    note: Option<String>,
    idempotency_key: String,
}

#[tracing::instrument(name = "Submit product suggestion",
    skip(body, pool, redis_pool, taxonomy, configuration, request))]
pub async fn submit_suggestion(
    current_user: CurrentUser,                  // requires login (extractor)
    path: web::Path<Uuid>,                       // farm_id
    body: web::Json<SuggestionRequest>,
    pool: web::Data<PgPool>,
    redis_pool: web::Data<Pool>,
    taxonomy: web::Data<TaxonomySnapshot>,
    configuration: web::Data<Settings>,
    request: HttpRequest,
) -> Result<HttpResponse, SuggestionError> {
    let farm_id = path.into_inner();
    let body = body.into_inner();

    // Validate product.
    let product_id = taxonomy.id_for_slug(&body.product)
        .ok_or_else(|| SuggestionError::ValidationError("Unknown product.".into()))?;

    // Rate limit per user (suggestion spam is the abuse vector). Reuse PR-26 limiter.
    enforce_suggestion_rate_limit(&redis_pool, current_user.id, &request, &configuration).await?;

    // Idempotency (reuse the module) keyed by user + idempotency_key, so a
    // double-tap doesn't create two suggestions. (Same pattern as farms POST.)

    // Optional: reject ADD when the farm already has the product / REMOVE when it
    // doesn't — or allow and dedupe at apply time. Recommended: allow, keep it simple.

    sqlx::query!(
        r#"
        INSERT INTO farm_product_suggestions
            (id, farm_id, product_id, action, note, submitted_by, status, created_at)
        VALUES ($1, $2, $3, $4::suggestion_action, $5, $6, 'PENDING', $7)
        "#,
        Uuid::new_v4(), farm_id, product_id,
        body.action as SuggestionAction,
        body.note, current_user.id, Utc::now(),
    )
    .execute(pool.get_ref())
    .await
    .context("Failed to store suggestion.")?;

    Ok(HttpResponse::Accepted().finish())
}
```

`src/routes/suggestions/error.rs` — `SuggestionError` mirroring `RegisterError`
(`ValidationError → 400`, `RateLimited → 429`, `NotFound → 404`,
`UnexpectedError → 500`, `Debug` via `error_chain_fmt`).

Register in `startup.rs`:

```rust
.route("/farms/{id}/product-suggestions", web::post().to(suggestions::submit_suggestion))
```

## 4.3 Tests

```rust
#[tokio::test]
async fn submit_requires_authentication() { /* no session => 401 */ }
#[tokio::test]
async fn authenticated_user_can_submit_pending_suggestion() { /* 202; row is PENDING */ }
#[tokio::test]
async fn submit_rejects_unknown_product() { /* 400 */ }
#[tokio::test]
async fn submit_is_rate_limited() { /* N+1 => 429 */ }
```

---

# PR 5 — Admin moderation

**Goal:** admins list the pending queue and approve/reject; approval applies the
change atomically.

## 5.1 Admin guard — `src/authentication/admin.rs`

```rust
use actix_web::{FromRequest, HttpRequest, dev::Payload};
use std::future::Future;
use std::pin::Pin;
use crate::authentication::CurrentUser;
use crate::domain::user::Role;

/// Extractor that succeeds only for ADMIN users. 403 otherwise.
pub struct AdminUser(pub CurrentUser);

impl FromRequest for AdminUser {
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let fut = CurrentUser::from_request(req, payload);
        Box::pin(async move {
            let user = fut.await?;
            if user.role == Role::Admin {
                Ok(AdminUser(user))
            } else {
                Err(actix_web::error::ErrorForbidden("Admin access required."))
            }
        })
    }
}
```

## 5.2 Routes — `src/routes/admin/suggestions.rs`

```rust
// GET /admin/product-suggestions?status=pending&after=<cursor>&limit=20
#[tracing::instrument(name = "List suggestions", skip(pool))]
pub async fn list_suggestions(
    _admin: AdminUser,
    query: web::Query<ListQuery>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, AdminError> {
    // Uses the partial index when status = PENDING; keyset-paginate by created_at,id.
    let rows = sqlx::query_as!(
        SuggestionRow,
        r#"
        SELECT s.id, s.farm_id, p.slug AS product_slug, s.action AS "action: SuggestionAction",
               s.note, s.submitted_by, s.status AS "status: SuggestionStatus", s.created_at
        FROM farm_product_suggestions s
        JOIN products p ON p.id = s.product_id
        WHERE s.status = $1::suggestion_status
        ORDER BY s.created_at DESC, s.id DESC
        LIMIT $2
        "#,
        query.status as SuggestionStatus,
        query.limit.clamp(1, 100),
    ).fetch_all(pool.get_ref()).await.context("list suggestions")?;
    Ok(HttpResponse::Ok().json(rows))
}

// POST /admin/product-suggestions/{id}/approve
#[tracing::instrument(name = "Approve suggestion", skip(pool))]
pub async fn approve_suggestion(
    admin: AdminUser,
    path: web::Path<Uuid>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, AdminError> {
    let id = path.into_inner();
    let mut tx = pool.begin().await.context("begin")?;

    // Atomic state transition: only a PENDING row can be claimed. Concurrent
    // approvers race here; exactly one gets the row back.
    let claimed = sqlx::query!(
        r#"
        UPDATE farm_product_suggestions
        SET status = 'APPROVED', reviewed_by = $1, reviewed_at = now()
        WHERE id = $2 AND status = 'PENDING'
        RETURNING farm_id, product_id, action AS "action: SuggestionAction"
        "#,
        admin.0.id, id,
    )
    .fetch_optional(&mut *tx)
    .await
    .context("claim suggestion")?;

    let Some(c) = claimed else {
        return Err(AdminError::Conflict); // already reviewed (or not found) => 409
    };

    // Apply the structured change idempotently.
    match c.action {
        SuggestionAction::Add => {
            sqlx::query!(
                "INSERT INTO farm_products (farm_id, product_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                c.farm_id, c.product_id,
            ).execute(&mut *tx).await.context("apply add")?;
        }
        SuggestionAction::Remove => {
            sqlx::query!(
                "DELETE FROM farm_products WHERE farm_id = $1 AND product_id = $2",
                c.farm_id, c.product_id,
            ).execute(&mut *tx).await.context("apply remove")?;
        }
    }

    tx.commit().await.context("commit")?;
    Ok(HttpResponse::Ok().finish())
}

// POST /admin/product-suggestions/{id}/reject  — same claim, no apply step.
```

`src/routes/admin/error.rs` — `AdminError` (`Conflict → 409`, `Forbidden` handled
by the extractor, `UnexpectedError → 500`).

Register routes in `startup.rs`.

> Optional: notify the submitter on decision via the `EmailClient` you built in
> the registration feature.

## 5.3 Tests

```rust
#[tokio::test]
async fn non_admin_cannot_list_or_moderate() { /* USER => 403 */ }
#[tokio::test]
async fn approving_add_creates_the_farm_product_link() {}
#[tokio::test]
async fn approving_remove_deletes_the_link() {}
#[tokio::test]
async fn approving_twice_is_a_conflict() { /* second approve => 409, no double apply */ }
#[tokio::test]
async fn rejecting_does_not_change_farm_products() {}
```

---

# PR 6 — Stocking status & seasonality

**Goal:** per-(farm, product) freshness/availability state — enabled by the
relationship being a table.

## 6.1 Migration — `migrations/<ts>_add_farm_product_status.sql`

```sql
CREATE TYPE stock_status AS ENUM ('AVAILABLE', 'SEASONAL', 'UNAVAILABLE');

ALTER TABLE farm_products
    ADD COLUMN status            stock_status NOT NULL DEFAULT 'AVAILABLE',
    ADD COLUMN last_confirmed_at timestamptz;

-- Partial index for "what's actually buyable now" style queries.
CREATE INDEX farm_products_available_idx
    ON farm_products (product_id)
    WHERE status = 'AVAILABLE';
```

## 6.2 Surfacing it

- Add `status` + `last_confirmed_at` to `ProductDto` and the product-load query.
- Approved `ADD` suggestions set `last_confirmed_at = now()` and `status = 'AVAILABLE'`
  (extend the `ON CONFLICT` in PR 5 to bump `last_confirmed_at`):
  ```sql
  INSERT INTO farm_products (farm_id, product_id, last_confirmed_at)
  VALUES ($1, $2, now())
  ON CONFLICT (farm_id, product_id)
  DO UPDATE SET last_confirmed_at = now(), status = 'AVAILABLE';
  ```
- Product search can optionally filter `WHERE status = 'AVAILABLE'`.

## 6.3 Tests

```rust
#[tokio::test]
async fn approved_add_marks_product_available_and_confirmed() {}
#[tokio::test]
async fn search_can_exclude_unavailable_products() {}
```

---

# Cross-cutting checklist (every PR)

- [ ] `sqlx migrate run` + `cargo sqlx prepare --workspace -- --all-targets`, commit `.sqlx/`.
- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- [ ] New routes registered in `src/startup.rs` and added to `api_docs/` (Bruno).
- [ ] `#[tracing::instrument]` on new service/query fns; never log secrets/tokens.
- [ ] `EXPLAIN (ANALYZE, BUFFERS)` the list/filter queries against a seeded DB; confirm index scans.

# Open decisions to confirm before coding

1. Replace free-form `categories` on `POST /farms` with `products` (this plan), or keep both during a transition?
2. Frontend resolves localized name → slug (this plan), or add a backend fuzzy search endpoint (`pg_trgm`)?
3. Default filter semantics OR with `match=all` opt-in (this plan), or AND by default?
4. Suggestions authenticated-only (this plan) vs allow anonymous + stricter rate limits?
5. Notify submitters by email on moderation decision (uses the existing `EmailClient`)?
