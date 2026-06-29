use crate::authentication::{VerifyEmailError as ServiceError, consume_verification_token};
use crate::routes::authentication::error::VerifyEmailError;
use actix_web::{HttpResponse, web};
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;

#[derive(serde::Deserialize)]
pub struct VerifyEmailRequest {
    token: SecretString, // bearer credential: keep it out of Debug/logs
}

#[tracing::instrument(name = "Verify email", skip(body, pool))]
pub async fn verify_email(
    body: web::Json<VerifyEmailRequest>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, VerifyEmailError> {
    consume_verification_token(body.token.expose_secret(), pool.get_ref())
        .await
        .map_err(|e| match e {
            ServiceError::InvalidToken => VerifyEmailError::InvalidToken,
            ServiceError::UnexpectedError(e) => VerifyEmailError::UnexpectedError(e),
        })?;

    Ok(HttpResponse::Ok().finish())
}
