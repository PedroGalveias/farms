use crate::authentication::password::{compute_password_hash, verify_password_hash};
use crate::domain::user::Role;
use crate::errors::error_chain_fmt;
use crate::telemetry::spawn_blocking_with_tracing;
use anyhow::Context;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use std::fmt::{Debug, Formatter};
use std::sync::LazyLock;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedUser {
    pub id: Uuid,
    pub role: Role,
}

struct StoredCredentials {
    id: Uuid,
    password_hash: SecretString,
    role: Role,
}

#[derive(thiserror::Error)]
pub enum ValidateCredentialsError {
    #[error("Invalid email or password")]
    InvalidCredentials(#[source] anyhow::Error),
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl Debug for ValidateCredentialsError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

// This static value is used when the email does not exist. This is used to reduce timing side channels.
static DUMMY_PASSWORD_HASH: LazyLock<SecretString> = LazyLock::new(|| {
    compute_password_hash(SecretString::from("dummy-pw".to_string()))
        .expect("Failed to compute dummy password hash.")
});

#[tracing::instrument(name = "Validate credentials", skip(email, password, pool))]
pub async fn validate_credentials(
    email: &str,
    password: SecretString,
    pool: &PgPool,
) -> Result<AuthenticatedUser, ValidateCredentialsError> {
    let stored_credentials = get_credentials(email, pool)
        .await
        .context("Failed to retrieve stored credentials.")
        .map_err(ValidateCredentialsError::UnexpectedError)?;

    let (authenticated_user, expected_password_hash) = match stored_credentials {
        Some(credentials) => (
            Some(AuthenticatedUser {
                id: credentials.id,
                role: credentials.role,
            }),
            credentials.password_hash,
        ),
        None => (None, DUMMY_PASSWORD_HASH.clone()),
    };

    let verification_result =
        spawn_blocking_with_tracing(move || verify_password_hash(expected_password_hash, password))
            .await
            .context("Failed to spawn blocking task.")
            .map_err(ValidateCredentialsError::UnexpectedError)?;

    verification_result.map_err(ValidateCredentialsError::InvalidCredentials)?;

    authenticated_user.ok_or_else(|| {
        ValidateCredentialsError::InvalidCredentials(anyhow::anyhow!("Invalid email or password."))
    })
}

#[tracing::instrument(name = "Change password", skip(password, pool))]
pub async fn change_password(
    id: Uuid,
    password: SecretString,
    pool: &PgPool,
) -> Result<(), anyhow::Error> {
    let password_hash = spawn_blocking_with_tracing(move || compute_password_hash(password))
        .await?
        .context("Failed to hash password.")?;

    sqlx::query!(
        r#"
        UPDATE users
        SET password_hash = $1
        WHERE id = $2
        "#,
        password_hash.expose_secret(),
        id,
    )
    .execute(pool)
    .await
    .context("Failed to update user's password")?;

    Ok(())
}

#[tracing::instrument(name = "Get user credentials", skip(email, pool))]
async fn get_credentials(
    email: &str,
    pool: &PgPool,
) -> Result<Option<StoredCredentials>, anyhow::Error> {
    let user = sqlx::query!(
        r#"
        SELECT id, password_hash, role as "role: Role"
        FROM users
        WHERE email = $1
        "#,
        email
    )
    .fetch_optional(pool)
    .await
    .context("Failed to retrieve user credentials.")?
    .map(|user| StoredCredentials {
        id: user.id,
        password_hash: SecretString::from(user.password_hash),
        role: user.role,
    });

    Ok(user)
}

#[tracing::instrument(name = "Get user by id", skip(pool))]
pub async fn get_user_by_id(
    id: Uuid,
    pool: &PgPool,
) -> Result<Option<AuthenticatedUser>, anyhow::Error> {
    let user = sqlx::query!(
        r#"
        SELECT id, role as "role: Role"
        FROM users
        WHERE id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await
    .context("Failed to retrieve user by id.")?
    .map(|user| AuthenticatedUser {
        id: user.id,
        role: user.role,
    });

    Ok(user)
}
