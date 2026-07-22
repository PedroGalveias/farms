use crate::{
    domain::farm::{Address, Canton, Name, Point, StockStatus},
    routes::farms::{FarmError, FarmListResponse, FarmResponse, FarmRow, ProductDto},
    taxonomy::TaxonomySnapshot,
};
use actix_web::{HttpResponse, web};
use anyhow::Context;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, serde::Deserialize)]
pub struct FarmPath {
    id: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct FarmListQuery {
    /// Comma-separated category (group) slugs, e.g. `?category=fruits,vegetables`.
    /// Matches farms in the group directly OR via a product in it ("any of").
    pub category: Option<String>,
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
            // Deduplicate: `match=all` compares COUNT(DISTINCT product_id) to
            // cardinality($2), so a repeated slug (`?product=x,x`) would inflate
            // the target and never match.
            ids.sort_unstable();
            ids.dedup();
            ids
        }
    };

    // Resolve category slugs to ids (early 400 on any unknown slug).
    let category_ids = match &query.category {
        None => Vec::new(),
        Some(csv) => {
            let mut ids = Vec::new();
            for slug in csv.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let id = taxonomy.category_id_for_slug(slug).ok_or_else(|| {
                    FarmError::ValidationError(format!("Unknown category '{slug}'."))
                })?;
                ids.push(id);
            }
            ids.sort_unstable();
            ids.dedup();
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

    let farms = list_farms(&pool, &category_ids, &product_ids, match_all, cursor, limit).await?;

    // A full page implies there may be more; hand back a cursor for the next.
    let next_cursor = if farms.len() as i64 == limit {
        farms.last().map(|f| make_cursor(f.created_at, f.id))
    } else {
        None
    };

    Ok(HttpResponse::Ok().json(FarmListResponse { farms, next_cursor }))
}

#[tracing::instrument(name = "Query farms page", skip(pool))]
async fn list_farms(
    pool: &PgPool,
    category_ids: &[i16],
    product_ids: &[i32],
    match_all: bool,
    cursor: Option<(DateTime<Utc>, Uuid)>,
    limit: i64,
) -> Result<Vec<FarmResponse>, FarmError> {
    let (cursor_ts, cursor_id) = match cursor {
        Some((ts, id)) => (Some(ts), Some(id)),
        None => (None, None),
    };

    // Page of farms, keyset cursor via the (created_at DESC, id DESC) index:
    //  - category filter ($1): matches farms linked to the group directly
    //    (farm_categories) OR holding a product in it ("any of");
    //  - product filter ($2/$3): matches the granular product links, with
    //    "any of" or "all of" semantics.
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
                cardinality($1::int2[]) = 0
                OR f.id IN (
                    SELECT fc.farm_id
                    FROM farm_categories fc
                    WHERE fc.category_id = ANY($1)
                    UNION
                    SELECT fp.farm_id
                    FROM farm_products fp
                    JOIN products p ON p.id = fp.product_id
                    WHERE p.category_id = ANY($1)
                )
            )
            AND (
                cardinality($2::int[]) = 0
                OR f.id IN (
                    SELECT fp.farm_id
                    FROM farm_products fp
                    WHERE fp.product_id = ANY($2)
                    GROUP BY fp.farm_id
                    HAVING $3 = false
                        OR count(DISTINCT fp.product_id) = cardinality($2)
                )
            )
            AND (
                $4::timestamptz IS NULL
                OR (f.created_at, f.id) < ($4, $5)
            )
        ORDER BY f.created_at DESC, f.id DESC
        LIMIT $6
        "#,
        category_ids,
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

    // Direct group-level memberships for this page (the coarse half of the
    // derived category set; the granular half comes from product groups).
    let category_rows = sqlx::query!(
        r#"
        SELECT fc.farm_id, c.slug AS "slug!"
        FROM farm_categories fc
        JOIN product_categories c ON c.id = fc.category_id
        WHERE fc.farm_id = ANY($1)
        "#,
        &farm_ids,
    )
    .fetch_all(pool)
    .await
    .context("Failed to load farm categories.")?;

    let mut direct_categories_by_farm: HashMap<Uuid, Vec<String>> = HashMap::new();
    for row in category_rows {
        direct_categories_by_farm
            .entry(row.farm_id)
            .or_default()
            .push(row.slug);
    }

    // One query for the products of every farm on this page (no N+1).
    let product_rows = sqlx::query!(
        r#"
        SELECT
            fp.farm_id,
            p.slug              AS "slug!",
            p.name_en,
            c.slug              AS "group_slug!",
            fp.status           AS "status: StockStatus",
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

    let mut products_by_farm: HashMap<Uuid, Vec<ProductDto>> = HashMap::new();
    for row in product_rows {
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
    }

    let mut responses = Vec::with_capacity(farm_rows.len());
    for farm in farm_rows {
        let products = products_by_farm.remove(&farm.id).unwrap_or_default();
        let direct = direct_categories_by_farm
            .remove(&farm.id)
            .unwrap_or_default();
        let categories = derive_categories(&direct, &products);
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
            p.slug              AS "slug!",
            p.name_en,
            c.slug              AS "group_slug!",
            fp.status           AS "status: StockStatus",
            fp.last_confirmed_at
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
            status: row.status,
            last_confirmed_at: row.last_confirmed_at,
        })
        .collect();

    let direct = sqlx::query!(
        r#"
        SELECT c.slug AS "slug!"
        FROM farm_categories fc
        JOIN product_categories c ON c.id = fc.category_id
        WHERE fc.farm_id = $1
        "#,
        farm_id,
    )
    .fetch_all(pool)
    .await
    .context("Failed to load farm categories.")?
    .into_iter()
    .map(|r| r.slug)
    .collect::<Vec<_>>();

    let categories = derive_categories(&direct, &products);

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

/// Distinct, sorted category (group) slugs for a farm: the union of its direct
/// group memberships and the groups of the products it lists. This is what lets
/// a farm surface under a category whether its data is coarse (group only) or
/// granular (specific products).
fn derive_categories(direct: &[String], products: &[ProductDto]) -> Vec<String> {
    let mut categories: Vec<String> = direct.to_vec();
    categories.extend(products.iter().map(|p| p.group.clone()));
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
