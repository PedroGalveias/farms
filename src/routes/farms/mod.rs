use crate::{
    domain::farm::{Address, Canton, Categories, Name, Point},
    errors::error_chain_fmt,
};
use actix_web::{http::StatusCode, ResponseError};
use chrono::{DateTime, Utc};
use std::fmt::Formatter;
use uuid::Uuid;

mod get;
mod post;

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

#[derive(thiserror::Error)]
pub enum FarmError {
    // `error` Implements the Display for this enum variant
    #[error("{0}")]
    ValidationError(String),
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
    // `from` derives an implementation of From for the type
    // this field is also used as error `source`. this denotes what should be returned as root cause
}
impl ResponseError for FarmError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::ValidationError(_) => StatusCode::BAD_REQUEST,
            Self::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
impl std::fmt::Debug for FarmError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
