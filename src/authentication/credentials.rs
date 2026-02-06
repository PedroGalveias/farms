use crate::authentication::password::{compute_password_hash, verify_password_hash};
use crate::telemetry::spawn_blocking_with_tracing;
use anyhow::Context;
use secrecy::SecretString;
use sqlx::PgPool;
use uuid::Uuid;

#[tracing::instrument(name = "Validate credentials", skip(email, password, pool))]
pub async fn validate_credentials(
    email: &str,
    password: SecretString,
    pool: &PgPool,
) -> Result<Uuid, anyhow::Error> {
    let stored_credentials = get_credentials(email, pool)
        .await
        .context("Failed to retrieve stored credentials.")?;

    let (id, password_hash) =
        stored_credentials.ok_or_else(|| anyhow::anyhow!("Invalid email or password."))?;

    verify_password_hash(password_hash, password).context("Invalid password.")?;

    Ok(id)
}

#[tracing::instrument(name = "Change password", skip(password, pool))]
pub async fn change_password(
    id: Uuid,
    password: SecretString,
    pool: &PgPool,
) -> Result<(), anyhow::Error> {
    let password_hash = spawn_blocking_with_tracing(move || compute_password_hash(password))
        .await
        .context("Failed to hash password.")?;

    sqlx::query!(
        r#"
        UPDATE users
        SET password_hash = $1
        WHERE id = $2
        "#,
        password_hash,
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
) -> Result<Option<(Uuid, SecretString)>, anyhow::Error> {
    let user = sqlx::query!(
        r#"
        SELECT id, password_hash
        FROM users
        WHERE email = $1
        "#,
        email
    )
    .fetch_optional(pool)
    .await
    .context("Failed to retrieve user credentials.")?
    .map(|user| (user.id, SecretString::from(user.password_hash)));

    Ok(user)
}
