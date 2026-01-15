use crate::domain::farm::{Address, Canton, Categories, Name, Point};
use chrono::{DateTime, Utc};
use uuid::Uuid;

mod error;
mod get;
mod post;

pub use error::FarmError;
pub use get::get_all;
pub use post::create;

#[derive(serde::Deserialize, serde::Serialize, sqlx::FromRow)]
pub struct Farm {
    pub id: Uuid,
    pub name: Name,
    pub address: Address,
    pub canton: Canton,
    pub coordinates: Point,
    pub categories: Categories,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}
