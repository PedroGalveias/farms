use crate::authentication::ValidateCredentialsError;
use crate::errors::error_chain_fmt;
use actix_web::ResponseError;
use actix_web::http::StatusCode;
use std::fmt::Formatter;

#[derive(thiserror::Error)]
pub enum LoginError {
    #[error("Invalid email or password")]
    InvalidCredentials(#[source] anyhow::Error),
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

#[derive(thiserror::Error)]
pub enum VerifyEmailError {
    #[error("Invalid verification token.")]
    InvalidToken, // → 400, deliberately vague
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error), // → 500
}

#[derive(thiserror::Error)]
pub enum RegisterError {
    #[error("{0}")]
    ValidationError(String),
    #[error("Too many registration attempts. Try again later.")]
    RateLimited,
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl From<ValidateCredentialsError> for LoginError {
    fn from(value: ValidateCredentialsError) -> Self {
        match value {
            ValidateCredentialsError::InvalidCredentials(err) => Self::InvalidCredentials(err),
            ValidateCredentialsError::UnexpectedError(err) => Self::UnexpectedError(err),
        }
    }
}

impl ResponseError for LoginError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidCredentials(_) => StatusCode::UNAUTHORIZED,
            Self::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl std::fmt::Debug for LoginError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl ResponseError for RegisterError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::ValidationError(_) => StatusCode::BAD_REQUEST,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl std::fmt::Debug for RegisterError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl ResponseError for VerifyEmailError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::InvalidToken => StatusCode::BAD_REQUEST,
            Self::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl std::fmt::Debug for VerifyEmailError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
