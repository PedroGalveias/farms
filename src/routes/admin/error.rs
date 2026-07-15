use crate::errors::error_chain_fmt;
use actix_web::{ResponseError, http::StatusCode};
use std::fmt::Formatter;

#[derive(thiserror::Error)]
pub enum AdminError {
    #[error("{0}")]
    ValidationError(String),
    #[error("Suggestion is no longer pending.")]
    Conflict,
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl ResponseError for AdminError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::ValidationError(_) => StatusCode::BAD_REQUEST,
            Self::Conflict => StatusCode::CONFLICT,
            Self::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl std::fmt::Debug for AdminError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
