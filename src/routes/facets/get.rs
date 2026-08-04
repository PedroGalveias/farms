use crate::{
    domain::{farm::Canton, language::Language},
    routes::farms::FarmError,
    taxonomy::TaxonomySnapshot,
};
use actix_web::{HttpResponse, http::header, web};
use anyhow::Context;
use sqlx::PgPool;

/// Query parameters for `GET /facets`.
#[derive(Debug, serde::Deserialize)]
pub struct FacetsQuery {
    /// Display language for category labels — see `FarmListQuery::lang`.
    pub lang: Option<String>,
}

/// How many farms are in one canton.
#[derive(serde::Serialize)]
pub struct CantonFacet {
    /// Two-letter canton code, e.g. `BE`.
    pub code: Canton,
    pub count: i64,
}

/// How many farms are in one category group.
#[derive(serde::Serialize)]
pub struct CategoryFacet {
    /// Stable, language-independent identifier — what `?category=` takes.
    pub slug: String,
    /// Display label in the requested language, falling back where a
    /// translation is missing.
    pub name: String,
    /// Whether `name` is a real translation or a fallback.
    pub translated: bool,
    pub count: i64,
}

/// Counts across the whole directory.
#[derive(serde::Serialize)]
pub struct FacetsResponse {
    /// The language the labels were resolved to.
    pub lang: Language,
    /// Every farm in the directory, so a client can show "N farms" without
    /// paging through the list to count them.
    pub total: i64,
    /// Only cantons that actually have farms. A canton with none would render
    /// as a dead option in a picker.
    pub cantons: Vec<CantonFacet>,
    /// Every category in the taxonomy, including those with a count of zero —
    /// a picker needs the full vocabulary, and the count tells it what to grey
    /// out. This mirrors `GET /taxonomy`, which is also exhaustive.
    pub categories: Vec<CategoryFacet>,
}

/// How long a client may reuse this response.
///
/// Matched to the 5 minutes the frontend already caches `GET /farms` for: these
/// counts describe that same data, and letting them drift apart would show a
/// visitor "412 farms" next to a list of a different length. `stale-while-
/// revalidate` lets a picker render instantly from a slightly old copy while
/// the refresh happens behind it, which matters because these counts are on the
/// critical path for the directory's filter UI.
///
/// `public`: the response depends only on `?lang=`, which is in the URL. No
/// session, no `Vary` needed.
const CACHE_CONTROL: &str = "public, max-age=300, stale-while-revalidate=3600";

/// `GET /facets` — how many farms sit behind each filter option.
///
/// This exists because a client cannot derive these counts without holding the
/// entire directory in memory. The frontend did exactly that: it fetched all
/// ~3,155 farms on every route that showed a filter, purely so it could count
/// them per canton and per category. That is the one thing keeping it from
/// asking the API for a filtered subset — filter server-side to one canton and
/// a locally-derived picker can only offer that canton, so the visitor cannot
/// get back out.
///
/// Counts are **unfiltered on purpose**. They describe the whole directory, so
/// a filter option's count does not change as other filters are applied. That
/// is what makes them a stable ordering for a picker — the frontend already
/// sorts its category chips by overall availability "so chips keep their place
/// as the contextual counts below change". Contextual counts, if wanted later,
/// are a different question and would take the same filter parameters as
/// `GET /farms`.
#[tracing::instrument(name = "Get facets", skip(pool, taxonomy))]
pub async fn get_facets(
    query: web::Query<FacetsQuery>,
    pool: web::Data<PgPool>,
    taxonomy: web::Data<TaxonomySnapshot>,
) -> Result<HttpResponse, FarmError> {
    let language = Language::from_query(query.lang.as_deref())
        .map_err(|e| FarmError::ValidationError(e.to_string()))?;

    // One round trip per facet rather than one query with several CTEs: each is
    // an index-friendly aggregate, and keeping them apart means a change to one
    // cannot silently alter another's grouping.
    let canton_rows = sqlx::query!(
        r#"
        SELECT f.canton AS "canton: Canton", count(*) AS "count!"
        FROM farms f
        GROUP BY f.canton
        ORDER BY f.canton
        "#,
    )
    .fetch_all(pool.get_ref())
    .await
    .context("Failed to count farms per canton.")?;

    // A farm belongs to a category either directly (`farm_categories`) or
    // through a product in that category — the same "any of" rule
    // `GET /farms?category=` applies. UNION rather than UNION ALL, so a farm
    // tagged both ways is counted once.
    let category_rows = sqlx::query!(
        r#"
        SELECT c.slug, count(DISTINCT m.farm_id) AS "count!"
        FROM product_categories c
        LEFT JOIN (
            SELECT fc.category_id, fc.farm_id
            FROM farm_categories fc
            UNION
            SELECT p.category_id, fp.farm_id
            FROM farm_products fp
            JOIN products p ON p.id = fp.product_id
        ) m ON m.category_id = c.id
        GROUP BY c.id, c.slug
        "#,
    )
    .fetch_all(pool.get_ref())
    .await
    .context("Failed to count farms per category.")?;

    let total = sqlx::query_scalar!(r#"SELECT count(*) AS "count!" FROM farms"#)
        .fetch_one(pool.get_ref())
        .await
        .context("Failed to count farms.")?;

    let counts: std::collections::HashMap<&str, i64> = category_rows
        .iter()
        .map(|row| (row.slug.as_str(), row.count))
        .collect();

    // Driven by the taxonomy snapshot, not by the query, so the list and its
    // order match `GET /taxonomy` exactly and a category with no farms yet
    // still appears.
    let categories = taxonomy
        .categories()
        .iter()
        .map(|category| CategoryFacet {
            slug: category.slug.clone(),
            name: category.labels.resolve(language).to_string(),
            translated: category.labels.has(language),
            count: counts.get(category.slug.as_str()).copied().unwrap_or(0),
        })
        .collect();

    let cantons = canton_rows
        .into_iter()
        .map(|row| CantonFacet {
            code: row.canton,
            count: row.count,
        })
        .collect();

    Ok(HttpResponse::Ok()
        .insert_header((header::CACHE_CONTROL, CACHE_CONTROL))
        .json(FacetsResponse {
            lang: language,
            total,
            cantons,
            categories,
        }))
}
