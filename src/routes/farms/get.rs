use super::{
    FarmDetailResponse, FarmError, FarmListResponse, lookup_classification_id,
    read::{ListParams, SortOrder, get_farm_by_id, list_farms},
};
use crate::{domain::language::Language, taxonomy::TaxonomySnapshot};
use actix_web::{HttpResponse, web};
use sqlx::PgPool;
use uuid::Uuid;

/// Query parameters accepted by `GET /farms/{id}`.
#[derive(Debug, serde::Deserialize)]
pub struct FarmDetailQuery {
    /// Display language for labels — see `FarmListQuery::lang`.
    pub lang: Option<String>,
}

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
    /// Display language for labels: `en` (default) | `de` | `fr` | `it` | `rm`.
    /// Filtering is unaffected — slugs stay language-independent.
    pub lang: Option<String>,
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
    let (params, language) = interpret_list_query(query.into_inner(), taxonomy.get_ref())?;
    let farms = list_farms(pool.get_ref(), &params).await?;

    // A full page implies there may be more; hand back the next offset.
    let next_cursor = if farms.len() as i64 == params.limit {
        Some((params.offset + params.limit).to_string())
    } else {
        None
    };

    Ok(HttpResponse::Ok().json(FarmListResponse {
        farms,
        next_cursor,
        lang: language,
    }))
}

fn interpret_list_query(
    query: FarmListQuery,
    taxonomy: &TaxonomySnapshot,
) -> Result<(ListParams, Language), FarmError> {
    let limit = query.limit.clamp(1, 100);
    let offset = query.offset.max(0);

    // Resolve the display language up front so an unsupported code fails the
    // request before any database work, the same way unknown slugs do.
    let language = Language::from_query(query.lang.as_deref())?;

    // Keep the requested value for tracing and fall back to newest for unknown sorts.
    let requested_sort = query.sort.unwrap_or_else(|| "newest".to_owned());
    let sort = SortOrder::from_query(&requested_sort);

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
    if (sort == SortOrder::Nearest || query.radius_km.is_some())
        && (query.lat.is_none() || query.lng.is_none())
    {
        return Err(FarmError::ValidationError(
            "lat and lng are required for nearest sort or radius filtering.".to_string(),
        ));
    }

    let params = ListParams {
        category_ids,
        product_ids,
        match_all,
        canton_codes,
        q_pattern,
        lat: query.lat,
        lng: query.lng,
        radius_km: query.radius_km,
        sort,
        requested_sort,
        limit,
        offset,
    };

    Ok((params, language))
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
        let id = lookup_classification_id(slug, &resolver, kind)?;
        ids.push(id);
    }

    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

#[tracing::instrument(name = "Get farm by id", skip(pool))]
pub async fn get_by_id(
    path: web::Path<FarmPath>,
    query: web::Query<FarmDetailQuery>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, FarmError> {
    let language = Language::from_query(query.lang.as_deref())?;

    let farm_id = Uuid::parse_str(&path.id)
        .map_err(|_| FarmError::ValidationError("Invalid farm id.".to_string()))?;

    match get_farm_by_id(farm_id, &pool).await? {
        Some(farm) => Ok(HttpResponse::Ok().json(FarmDetailResponse {
            farm,
            lang: language,
        })),
        None => Err(FarmError::NotFound),
    }
}
