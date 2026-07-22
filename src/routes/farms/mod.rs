use crate::domain::farm::{Address, Canton, Name, Point, StockStatus};
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
