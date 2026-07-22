use crate::{
    domain::farm::{Address, Canton, Name, Point, StockStatus},
    routes::farms::{FarmError, FarmListResponse, FarmResponse, FarmRow, ProductDto},
    taxonomy::TaxonomySnapshot,
};
use actix_web::{HttpResponse, web};
use anyhow::Context;
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
    /// Comma-separated canton codes, e.g. `?canton=ZH,BE`.
    pub canton: Option<String>,
    /// Free-text query matched against farm name, address and product names.
    pub q: Option<String>,
    /// The requester's location. When both are given, each farm carries a
    /// `distance_km`, `radius_km` can filter, and `sort=nearest` is allowed.
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    /// Keep only farms within this many km of `lat`/`lng`.
    pub radius_km: Option<f64>,
    /// `newest` (default) | `name` | `canton` | `nearest` (needs lat/lng).
    pub sort: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    20
}

/// Escape LIKE/ILIKE wildcards in user input so `%` and `_` are literal.
fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    for ch in input.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[tracing::instrument(name = "List farms", skip(pool, taxonomy))]
pub async fn get_all(
    query: web::Query<FarmListQuery>,
    pool: web::Data<PgPool>,
    taxonomy: web::Data<TaxonomySnapshot>,
) -> Result<HttpResponse, FarmError> {
    let limit = query.limit.clamp(1, 100);
    let offset = query.offset.max(0);
    let sort = query.sort.as_deref().unwrap_or("newest");

    // Resolve product slugs to ids (early 400 on any unknown slug).
    let product_ids = resolve_slugs(&query.product, |slug| taxonomy.id_for_slug(slug), "product")?;
    // Resolve category slugs to ids.
    let category_ids = resolve_slugs(
        &query.category,
        |slug| taxonomy.category_id_for_slug(slug),
        "category",
    )?;

    let match_all = query.r#match.as_deref() == Some("all");

    let canton_codes: Vec<String> = query
        .canton
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(|c| c.trim().to_uppercase())
        .filter(|c| !c.is_empty())
        .collect();

    let q_pattern = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", escape_like(s)));

    // Distance-dependent features need a location.
    if (sort == "nearest" || query.radius_km.is_some())
        && (query.lat.is_none() || query.lng.is_none())
    {
        return Err(FarmError::ValidationError(
            "lat and lng are required for nearest sort or radius filtering.".to_string(),
        ));
    }

    let farms = list_farms(
        &pool,
        ListParams {
            category_ids: &category_ids,
            product_ids: &product_ids,
            match_all,
            canton_codes: &canton_codes,
            q_pattern: q_pattern.as_deref(),
            lat: query.lat,
            lng: query.lng,
            radius_km: query.radius_km,
            sort,
            limit,
            offset,
        },
    )
    .await?;

    // A full page implies there may be more; hand back the next offset.
    let next_cursor = if farms.len() as i64 == limit {
        Some((offset + limit).to_string())
    } else {
        None
    };

    Ok(HttpResponse::Ok().json(FarmListResponse { farms, next_cursor }))
}

/// Resolve a comma-separated slug list to ids via `resolver`, 400 on unknown.
/// Deduplicated: `match=all` compares COUNT(DISTINCT id) to cardinality(), so a
/// repeated slug (`?product=x,x`) would inflate the target and match nothing.
fn resolve_slugs<T: Ord>(
    raw: &Option<String>,
    resolver: impl Fn(&str) -> Option<T>,
    kind: &str,
) -> Result<Vec<T>, FarmError> {
    let Some(csv) = raw else {
        return Ok(Vec::new());
    };
    let mut ids = Vec::new();
    for slug in csv.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let id = resolver(slug)
            .ok_or_else(|| FarmError::ValidationError(format!("Unknown {kind} '{slug}'.")))?;
        ids.push(id);
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

struct ListParams<'a> {
    category_ids: &'a [i16],
    product_ids: &'a [i32],
    match_all: bool,
    canton_codes: &'a [String],
    q_pattern: Option<&'a str>,
    lat: Option<f64>,
    lng: Option<f64>,
    radius_km: Option<f64>,
    sort: &'a str,
    limit: i64,
    offset: i64,
}

#[tracing::instrument(name = "Query farms page", skip(pool, params), fields(sort = params.sort))]
async fn list_farms(pool: &PgPool, params: ListParams<'_>) -> Result<Vec<FarmResponse>, FarmError> {
    // A page of farms. Filters: category (group directly OR via a product in
    // it), product (granular, any/all), canton, and free-text q over name /
    // address / product names. `distance_km` (great-circle) is computed once in
    // the CTE and reused for the radius filter and `sort=nearest`. Offset
    // pagination keeps every sort (newest/name/canton/nearest) uniform.
    let farm_rows = sqlx::query!(
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
                END AS distance_km
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
            f.distance_km AS "distance_km?"
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
                    WHERE fpq.farm_id = f.id AND pq.name_en ILIKE $5
                )
            )
            AND ($8::float8 IS NULL OR (f.distance_km IS NOT NULL AND f.distance_km <= $8))
        ORDER BY
            CASE WHEN $9 = 'nearest' THEN f.distance_km END ASC NULLS LAST,
            CASE WHEN $9 = 'name' THEN f.name END ASC,
            CASE WHEN $9 = 'canton' THEN f.canton END ASC,
            f.created_at DESC, f.id DESC
        LIMIT $10 OFFSET $11
        "#,
        params.category_ids,
        params.product_ids,
        params.match_all,
        params.canton_codes,
        params.q_pattern,
        params.lat,
        params.lng,
        params.radius_km,
        params.sort,
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
        let direct = direct_categories_by_farm
            .get(&farm.id)
            .cloned()
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
            distance_km: farm.distance_km,
            created_at: farm.created_at,
            updated_at: farm.updated_at,
        });
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

    let mut products_by_farm = load_products(pool, &[farm.id]).await?;
    let products = products_by_farm.remove(&farm.id).unwrap_or_default();
    let direct_categories_by_farm = load_direct_categories(pool, &[farm.id]).await?;
    let direct = direct_categories_by_farm
        .get(&farm.id)
        .cloned()
        .unwrap_or_default();
    let categories = derive_categories(&direct, &products);

    Ok(Some(FarmResponse {
        id: farm.id,
        name: farm.name,
        address: farm.address,
        canton: farm.canton,
        coordinates: farm.coordinates,
        categories,
        products,
        distance_km: None,
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
