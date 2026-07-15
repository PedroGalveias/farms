use crate::errors::error_chain_fmt;
use actix_web::{ResponseError, http::StatusCode};
use std::fmt::Formatter;

#[derive(thiserror::Error)]
pub enum SuggestionError {
    #[error("{0}")]
    ValidationError(String),
    #[error("Farm not found.")]
    FarmNotFound,
    #[error("Too many suggestions. Try again later.")]
    RateLimited,
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl ResponseError for SuggestionError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::ValidationError(_) => StatusCode::BAD_REQUEST,
            Self::FarmNotFound => StatusCode::NOT_FOUND,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl std::fmt::Debug for SuggestionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
