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
