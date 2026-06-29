use crate::authentication::registration::hash_verification_token;
use crate::domain::user::UserStatus;
use crate::errors::error_chain_fmt;
use anyhow::Context;
use chrono::Utc;
use sqlx::PgPool;
use std::fmt::{Debug, Formatter};

#[derive(thiserror::Error)]
pub enum VerifyEmailError {
    // One variant for unknown, expired, AND already-used tokens:
    // the caller must not be able to distinguish them.
    #[error("Invalid verification token.")]
    InvalidToken,
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl Debug for VerifyEmailError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(name = "Verify email with token", skip(token, pool))]
pub async fn consume_verification_token(
    token: &str,
    pool: &PgPool,
) -> Result<(), VerifyEmailError> {
    let token_hash = hash_verification_token(token);
    let now = Utc::now();

    let mut transaction = pool
        .begin()
        .await
        .context("Failed to acquire a Postgres connection from the pool.")?;

    // Marking the token used and fetching its owner is one atomic statement:
    // a concurrent request for the same token sees used_at already set.
    let record = sqlx::query!(
        r#"
        UPDATE email_verification_tokens
        SET used_at = $1
        WHERE token_hash = $2
          AND used_at IS NULL
          AND expires_at > $1
        RETURNING user_id
        "#,
        now,
        token_hash,
    )
    .fetch_optional(&mut *transaction)
    .await
    .context("Failed to consume the verification token.")?;

    let Some(record) = record else {
        return Err(VerifyEmailError::InvalidToken);
    };

    sqlx::query!(
        r#"
        UPDATE users
        SET status = $1::user_status, email_verified_at = $2, updated_at = $2
        WHERE id = $3
        "#,
        UserStatus::Active as UserStatus,
        now,
        record.user_id,
    )
    .execute(&mut *transaction)
    .await
    .context("Failed to activate the user.")?;

    transaction
        .commit()
        .await
        .context("Failed to commit email verification transaction.")?;

    Ok(())
}
