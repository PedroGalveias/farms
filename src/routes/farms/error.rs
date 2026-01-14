use crate::{errors::error_chain_fmt, idempotency::IdempotencyError};
use actix_web::{ResponseError, http::StatusCode};
use std::fmt::Formatter;

#[derive(thiserror::Error)]
pub enum FarmError {
    // `error` Implements the Display for this enum variant
    #[error("{0}")]
    ValidationError(String),
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
    // `from` derives an implementation of From for the type
    // this field is also used as error `source`. this denotes what should be returned as root cause
    #[error(transparent)]
    DuplicateRequestConflict(#[from] IdempotencyError),
}
impl ResponseError for FarmError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::ValidationError(_) => StatusCode::BAD_REQUEST,
            Self::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::DuplicateRequestConflict(_) => StatusCode::CONFLICT,
        }
    }
}
impl std::fmt::Debug for FarmError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
