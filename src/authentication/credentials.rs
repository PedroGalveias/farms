use crate::authentication::password::verify_password_hash;
use anyhow::Context;
use secrecy::SecretString;
use sqlx::PgPool;
use uuid::Uuid;

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
