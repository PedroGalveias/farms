use crate::{domain::language::Language, routes::farms::FarmError, taxonomy::TaxonomySnapshot};
use actix_web::{HttpResponse, web};

/// Query parameters for `GET /taxonomy`.
#[derive(Debug, serde::Deserialize)]
pub struct TaxonomyQuery {
    /// Display language for the labels — see `FarmListQuery::lang`.
    pub lang: Option<String>,
}

/// A category group as returned to API clients.
#[derive(serde::Serialize)]
pub struct CategoryDto {
    /// Stable, language-independent identifier. This is what `?category=`
    /// takes, and what a client should store.
    pub slug: String,
    /// Display label in the requested language, falling back where a
    /// translation is missing.
    pub name: String,
    /// Whether `name` is a real translation or a fallback. Lets a client decide
    /// whether to show the label as authoritative, and lets us measure coverage
    /// without guessing.
    pub translated: bool,
}

/// The vocabulary the API filters on, with display labels.
#[derive(serde::Serialize)]
pub struct TaxonomyResponse {
    /// The language the labels were resolved to.
    pub lang: Language,
    pub categories: Vec<CategoryDto>,
}

/// `GET /taxonomy` — the whole filtering vocabulary in one request.
///
/// This exists so a client does not have to carry its own copy of the taxonomy.
/// Anything with a picker, a type-ahead, or an offline mode needs the *full*
/// list of categories, not just the ones attached to the farms in the current
/// response — so putting labels on `/farms` alone would leave every client
/// maintaining a duplicate list and keeping it in sync by hand.
///
/// Labels are deliberately absent from `/farms`: repeating thirteen category
/// strings across three thousand farms would be a lot of bytes to say something
/// a client can look up once and cache.
///
/// The response is small (tens of rows) and changes only on deploy, so it is a
/// good candidate for a long client-side cache.
#[tracing::instrument(name = "Get taxonomy", skip(taxonomy))]
pub async fn get_taxonomy(
    query: web::Query<TaxonomyQuery>,
    taxonomy: web::Data<TaxonomySnapshot>,
) -> Result<HttpResponse, FarmError> {
    let language = Language::from_query(query.lang.as_deref())
        .map_err(|e| FarmError::ValidationError(e.to_string()))?;

    let categories = taxonomy
        .categories()
        .iter()
        .map(|category| CategoryDto {
            slug: category.slug.clone(),
            name: category.labels.resolve(language).to_string(),
            translated: category.labels.has(language),
        })
        .collect();

    Ok(HttpResponse::Ok().json(TaxonomyResponse {
        lang: language,
        categories,
    }))
}
