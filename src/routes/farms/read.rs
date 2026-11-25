use super::{FarmError, FarmResponse, ProductDto};
use crate::domain::farm::{Address, Canton, Name, Point, StockStatus};
use anyhow::Context;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SortOrder {
    Newest,
    Name,
    Canton,
    Nearest,
}

impl SortOrder {
    pub(super) fn from_query(value: &str) -> Self {
        match value {
            "name" => Self::Name,
            "canton" => Self::Canton,
            "nearest" => Self::Nearest,
            _ => Self::Newest,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Newest => "newest",
            Self::Name => "name",
            Self::Canton => "canton",
            Self::Nearest => "nearest",
        }
    }
}

pub(super) struct ListParams {
    pub(super) category_ids: Vec<i16>,
    pub(super) product_ids: Vec<i32>,
    pub(super) match_all: bool,
    pub(super) canton_codes: Vec<String>,
    pub(super) q_pattern: Option<String>,
    pub(super) lat: Option<f64>,
    pub(super) lng: Option<f64>,
    pub(super) radius_km: Option<f64>,
    pub(super) sort: SortOrder,
    pub(super) requested_sort: String,
    pub(super) limit: i64,
    pub(super) offset: i64,
}

/// The raw farm row loaded from the database, before products are attached.
struct FarmRow {
    id: Uuid,
    name: Name,
    address: Address,
    canton: Canton,
    coordinates: Point,
    distance: Option<f64>,
    created_at: DateTime<Utc>,
    updated_at: Option<DateTime<Utc>>,
}

#[tracing::instrument(name = "Query farms page", skip(pool, params), fields(sort = params.requested_sort.as_str()))]
pub(super) async fn list_farms(
    pool: &PgPool,
    params: &ListParams,
) -> Result<Vec<FarmResponse>, FarmError> {
    // A page of farms. Filters: category (group directly OR via a product in
    // it), product (granular, any/all), canton, and free-text q over name /
    // address / product names. `distance` (great-circle) is computed once in
    // the CTE and reused for the radius filter and `sort=nearest`. Offset
    // pagination keeps every sort (newest/name/canton/nearest) uniform.
    let farm_rows = sqlx::query_as!(
        FarmRow,
        r#"
        WITH base AS (
            SELECT
                f.id, f.name, f.address, f.canton, f.coordinates,
                f.created_at, f.updated_at,
                CASE
                    WHEN $6::float8 IS NULL OR $7::float8 IS NULL THEN NULL
                    ELSE 6371.0 * acos(least(1, greatest(-1,
                        sin(radians($6)) * sin(radians(f.coordinates[1]))
                      + cos(radians($6)) * cos(radians(f.coordinates[1]))
                        * cos(radians(f.coordinates[0] - $7))
                    )))
                END AS distance
            FROM farms f
        )
        SELECT
            f.id,
            f.name        AS "name: Name",
            f.address     AS "address: Address",
            f.canton      AS "canton: Canton",
            f.coordinates AS "coordinates: Point",
            f.created_at,
            f.updated_at,
            f.distance AS "distance?"
        FROM base f
        WHERE
            (
                cardinality($1::int2[]) = 0
                OR f.id IN (
                    SELECT fc.farm_id FROM farm_categories fc WHERE fc.category_id = ANY($1)
                    UNION
                    SELECT fp.farm_id FROM farm_products fp
                        JOIN products p ON p.id = fp.product_id
                        WHERE p.category_id = ANY($1)
                )
            )
            AND (
                cardinality($2::int[]) = 0
                OR f.id IN (
                    SELECT fp.farm_id FROM farm_products fp
                    WHERE fp.product_id = ANY($2)
                    GROUP BY fp.farm_id
                    HAVING $3 = false OR count(DISTINCT fp.product_id) = cardinality($2)
                )
            )
            AND (cardinality($4::text[]) = 0 OR f.canton = ANY($4))
            AND (
                $5::text IS NULL
                OR f.name ILIKE $5
                OR f.address ILIKE $5
                OR EXISTS (
                    SELECT 1 FROM farm_products fpq
                    JOIN products pq ON pq.id = fpq.product_id
                    WHERE fpq.farm_id = f.id
                      AND (pq.key_de ILIKE $5 OR pq.name_en ILIKE $5)
                )
            )
            AND ($8::float8 IS NULL OR (f.distance IS NOT NULL AND f.distance <= $8))
        ORDER BY
            CASE WHEN $9 = 'nearest' THEN f.distance END ASC NULLS LAST,
            CASE WHEN $9 = 'name' THEN f.name END ASC,
            CASE WHEN $9 = 'canton' THEN f.canton END ASC,
            f.created_at DESC, f.id DESC
        LIMIT $10 OFFSET $11
        "#,
        params.category_ids.as_slice(),
        params.product_ids.as_slice(),
        params.match_all,
        params.canton_codes.as_slice(),
        params.q_pattern.as_deref(),
        params.lat,
        params.lng,
        params.radius_km,
        params.sort.as_str(),
        params.limit,
        params.offset,
    )
    .fetch_all(pool)
    .await
    .context("Failed to page farms.")?;

    let farm_ids: Vec<Uuid> = farm_rows.iter().map(|f| f.id).collect();
    let direct_categories_by_farm = load_direct_categories(pool, &farm_ids).await?;
    let mut products_by_farm = load_products(pool, &farm_ids).await?;

    let mut responses = Vec::with_capacity(farm_rows.len());
    for farm in farm_rows {
        let products = products_by_farm.remove(&farm.id).unwrap_or_default();
        let direct_categories = direct_categories_by_farm
            .get(&farm.id)
            .map(Vec::as_slice)
            .unwrap_or_default();

        responses.push(assemble_farm_response(farm, direct_categories, products));
    }

    Ok(responses)
}

/// Direct group-level memberships for a page of farms (no N+1).
async fn load_direct_categories(
    pool: &PgPool,
    farm_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<String>>, FarmError> {
    let rows = sqlx::query!(
        r#"
        SELECT fc.farm_id, c.slug AS "slug!"
        FROM farm_categories fc
        JOIN product_categories c ON c.id = fc.category_id
        WHERE fc.farm_id = ANY($1)
        "#,
        farm_ids,
    )
    .fetch_all(pool)
    .await
    .context("Failed to load farm categories.")?;

    let mut by_farm: HashMap<Uuid, Vec<String>> = HashMap::new();
    for row in rows {
        by_farm.entry(row.farm_id).or_default().push(row.slug);
    }
    Ok(by_farm)
}

/// Products for a page of farms (no N+1).
async fn load_products(
    pool: &PgPool,
    farm_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<ProductDto>>, FarmError> {
    let rows = sqlx::query!(
        r#"
        SELECT
            fp.farm_id,
            p.slug              AS "slug!",
            p.key_de            AS "name_de!",
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
        farm_ids,
    )
    .fetch_all(pool)
    .await
    .context("Failed to load farm products.")?;

    let mut by_farm: HashMap<Uuid, Vec<ProductDto>> = HashMap::new();
    for row in rows {
        by_farm.entry(row.farm_id).or_default().push(ProductDto {
            slug: row.slug,
            name_de: row.name_de,
            name_en: row.name_en,
            group: row.group_slug,
            status: row.status,
            last_confirmed_at: row.last_confirmed_at,
        });
    }
    Ok(by_farm)
}

#[tracing::instrument(name = "Query single farm", skip(pool))]
pub(super) async fn get_farm_by_id(
    farm_id: Uuid,
    pool: &PgPool,
) -> Result<Option<FarmResponse>, FarmError> {
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
            f.updated_at,
            NULL::float8 AS "distance?"
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

    let mut products_by_farm = load_products(pool, &[farm.id]).await?;
    let products = products_by_farm.remove(&farm.id).unwrap_or_default();

    let direct_categories_by_farm = load_direct_categories(pool, &[farm.id]).await?;
    let direct_categories = direct_categories_by_farm
        .get(&farm.id)
        .map(Vec::as_slice)
        .unwrap_or_default();

    Ok(Some(assemble_farm_response(
        farm,
        direct_categories,
        products,
    )))
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

fn assemble_farm_response(
    farm: FarmRow,
    direct_categories: &[String],
    products: Vec<ProductDto>,
) -> FarmResponse {
    let categories = derive_categories(direct_categories, &products);

    FarmResponse {
        id: farm.id,
        name: farm.name,
        address: farm.address,
        canton: farm.canton,
        coordinates: farm.coordinates,
        distance_km: farm.distance,
        categories,
        products,
        created_at: farm.created_at,
        updated_at: farm.updated_at,
    }
}
