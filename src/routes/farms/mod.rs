use crate::domain::farm::{Address, Canton, Name, Point, StockStatus};
use crate::domain::language::Language;
use chrono::{DateTime, Utc};
use uuid::Uuid;

mod error;
mod get;
mod post;

pub use error::FarmError;
pub use get::{get_all, get_by_id};
pub use post::create;

/// A product as returned to API clients.
///
/// i18n ownership: the `slug` is the stable identity and the frontend owns
/// display localization keyed by it. The backend ships the two canonical names
/// it holds — `name_de` (always present) and `name_en` (when known) — as the
/// fallback; the frontend layers fr/it/rm on top, keyed by slug.
#[derive(serde::Serialize, Clone)]
pub struct ProductDto {
    pub slug: String,
    pub name_de: String,
    pub name_en: Option<String>,
    /// The slug of the category group this product belongs to.
    pub group: String,
    pub status: StockStatus,
    pub last_confirmed_at: Option<DateTime<Utc>>,
}

/// A farm plus its products. `categories` is derived from `products` (the
/// distinct group slugs) and kept for backward compatibility.
#[derive(serde::Serialize)]
pub struct FarmResponse {
    pub id: Uuid,
    pub name: Name,
    pub address: Address,
    pub canton: Canton,
    pub coordinates: Point,
    pub categories: Vec<String>,
    pub products: Vec<ProductDto>,
    /// Straight-line distance in km from the request's `lat`/`lng`, when given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_km: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// A single farm plus the language it was resolved for.
///
/// The list endpoint echoes `lang` at the top level, and the detail endpoint
/// has to as well — a client should be able to read the same field from either
/// without a special case. Flattened, so every existing field stays exactly
/// where it was and adding this moves nothing a caller already reads.
///
/// It lives here rather than on `FarmResponse` because that type is also the
/// element type of the list, where repeating the language on all hundred farms
/// would say the same thing a hundred times.
#[derive(serde::Serialize)]
pub struct FarmDetailResponse {
    #[serde(flatten)]
    pub farm: FarmResponse,
    pub lang: Language,
}

/// A page of farms plus the offset to fetch the next page (if any).
#[derive(serde::Serialize)]
pub struct FarmListResponse {
    pub farms: Vec<FarmResponse>,
    /// Offset for the next page as a string, or null when this is the last page.
    pub next_cursor: Option<String>,
    /// The language the labels in this response were resolved to.
    ///
    /// Echoing it back means a caller can tell the difference between "you got
    /// German because you asked" and "you got the default", without having to
    /// infer it from the payload. It is also what makes an omitted `lang`
    /// self-documenting.
    pub lang: Language,
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
