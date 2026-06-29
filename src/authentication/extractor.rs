use std::{future::Future, pin::Pin};

use actix_session::SessionExt;
use actix_web::{
    Error, FromRequest, HttpRequest, ResponseError, dev::Payload, http::StatusCode, web,
};
use anyhow::Context;
use sqlx::PgPool;
use std::fmt::Formatter;

use crate::{
    authentication::{AuthenticatedUser, TypedSession, get_user_by_id},
    errors::error_chain_fmt,
};

#[derive(thiserror::Error)]
pub enum AuthenticationError {
    #[error("Authentication required")]
    Unauthorized,
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl std::fmt::Debug for AuthenticationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl ResponseError for AuthenticationError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: uuid::Uuid,
    pub role: crate::domain::user::Role,
}

impl From<AuthenticatedUser> for CurrentUser {
    fn from(value: AuthenticatedUser) -> Self {
        Self {
            id: value.id,
            role: value.role,
        }
    }
}

impl FromRequest for CurrentUser {
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let session = TypedSession::from(req.get_session());
        let pool = req.app_data::<web::Data<PgPool>>().cloned();

        Box::pin(async move {
            let pool = pool.ok_or_else(|| {
                AuthenticationError::UnexpectedError(anyhow::anyhow!(
                    "Postgres connection pool is not configured."
                ))
            })?;

            let user_id = session
                .get_user_id()
                .map_err(AuthenticationError::UnexpectedError)?
                .ok_or(AuthenticationError::Unauthorized)?;

            let user = get_user_by_id(user_id, pool.get_ref())
                .await
                .context("Failed to retrieve current user from the database.")
                .map_err(AuthenticationError::UnexpectedError)?;

            match user {
                Some(user) => Ok(user.into()),
                None => {
                    session.log_out();
                    Err(AuthenticationError::Unauthorized.into())
                }
            }
        })
    }
}
