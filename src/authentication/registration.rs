use crate::authentication::password::compute_password_hash;
use crate::configuration::RegistrationSettings;
use crate::domain::user::{Email, Role, UserPassword, UserStatus, Username};
use crate::email_client::EmailClient;
use crate::errors::error_chain_fmt;
use crate::telemetry::spawn_blocking_with_tracing;
use anyhow::Context;
use chrono::Utc;
use rand::RngExt;
use rand::distr::Alphanumeric;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use sqlx::{Executor, PgPool, Postgres, Transaction};
use std::fmt::{Debug, Formatter};
use uuid::Uuid;

#[derive(thiserror::Error)]
pub enum RegisterUserError {
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
    #[error("Username is already taken.")]
    UsernameTaken,
    #[error("Failed to deliver the verification email.")]
    EmailDeliveryError(#[source] anyhow::Error),
}

impl Debug for RegisterUserError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(
    name = "Register a new user",
    skip(username, email, password, pool, email_client, settings)
)]
pub async fn register_user(
    username: Username,
    email: Email,
    password: UserPassword,
    pool: &PgPool,
    email_client: &EmailClient,
    settings: &RegistrationSettings,
) -> Result<(), RegisterUserError> {
    // Hash BEFORE checking existence: the expensive Argon2 work happens on
    // every request, so response timing does not reveal whether the email
    // is already registered.
    let password_hash =
        spawn_blocking_with_tracing(move || compute_password_hash(password.into_secret()))
            .await
            .context("Failed to spawn blocking task.")?
            .context("Failed to hash password.")?;

    let mut transaction = pool
        .begin()
        .await
        .context("Failed to acquire a Postgres connection from the pool.")?;

    if email_is_registered(&mut transaction, &email).await? {
        // Generic success: never reveal that an account exists.
        return Ok(());
    }

    let user_id =
        match insert_pending_user(&mut transaction, &username, &email, password_hash).await {
            Ok(user_id) => user_id,
            Err(e) => match unique_violation_constraint(&e).as_deref() {
                // A username is a public identifier, so it is fine - and better UX -
                // to tell the caller it is already taken.
                Some("users_username_key") => return Err(RegisterUserError::UsernameTaken),
                // Any other unique violation is the email constraint racing the check
                // above. Stay generic so email existence is never revealed.
                Some(_) => return Ok(()),
                None => {
                    return Err(anyhow::Error::from(e)
                        .context("Failed to insert new user.")
                        .into());
                }
            },
        };

    let token = generate_verification_token();
    store_verification_token(&mut transaction, user_id, &token, settings).await?;

    transaction
        .commit()
        .await
        .context("Failed to commit user registration transaction.")?;

    // Send AFTER commit: a provider outage must not roll back the account.
    // The user stays PENDING_VERIFICATION and can request a resend later.
    send_verification_email(email_client, &email, &token, settings)
        .await
        .map_err(RegisterUserError::EmailDeliveryError)?;

    Ok(())
}

fn generate_verification_token() -> String {
    let mut rng = rand::rng();
    std::iter::repeat_with(|| rng.sample(Alphanumeric))
        .map(char::from)
        .take(32)
        .collect()
}

/// Only the hash ever touches the database; the raw token goes
/// to the user's inbox and nowhere else.
pub fn hash_verification_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// If `e` is a UNIQUE violation, returns the name of the violated constraint
/// (Postgres names inline constraints `<table>_<column>_key`). This lets
/// registration distinguish a username clash (public, surfaced as 409) from an
/// email clash (private, hidden behind a generic 202).
fn unique_violation_constraint(e: &sqlx::Error) -> Option<String> {
    let db_err = e.as_database_error()?;
    if !db_err.is_unique_violation() {
        return None;
    }
    Some(db_err.constraint().unwrap_or_default().to_string())
}

#[tracing::instrument(name = "Check whether email is registered", skip_all)]
async fn email_is_registered(
    transaction: &mut Transaction<'_, Postgres>,
    email: &Email,
) -> Result<bool, anyhow::Error> {
    let existing = sqlx::query!(r#"SELECT id FROM users WHERE email = $1"#, email.as_str(),)
        .fetch_optional(&mut **transaction)
        .await
        .context("Failed to check for an existing user.")?;

    Ok(existing.is_some())
}

#[tracing::instrument(name = "Insert pending user", skip_all)]
async fn insert_pending_user(
    transaction: &mut Transaction<'_, Postgres>,
    username: &Username,
    email: &Email,
    password_hash: SecretString,
) -> Result<Uuid, sqlx::Error> {
    let user_id = Uuid::new_v4();
    let query = sqlx::query!(
        r#"
        INSERT INTO users
            (id, username, email, password_hash, role, status, created_at)
        VALUES ($1, $2, $3, $4, $5::user_role, $6::user_status, $7)
        "#,
        user_id,
        username.as_str(),
        email.as_str(),
        password_hash.expose_secret(),
        Role::User as Role,
        UserStatus::PendingVerification as UserStatus,
        Utc::now(),
    );
    transaction.execute(query).await?;

    Ok(user_id)
}

#[tracing::instrument(name = "Store verification token", skip_all)]
async fn store_verification_token(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    token: &str,
    settings: &RegistrationSettings,
) -> Result<(), anyhow::Error> {
    let now = Utc::now();
    let query = sqlx::query!(
        r#"
        INSERT INTO email_verification_tokens (token_hash, user_id, expires_at, created_at)
        VALUES ($1, $2, $3, $4)
        "#,
        hash_verification_token(token),
        user_id,
        now + chrono::Duration::seconds(settings.verification_token_ttl_seconds),
        now,
    );
    transaction
        .execute(query)
        .await
        .context("Failed to store the verification token.")?;

    Ok(())
}

#[tracing::instrument(name = "Send verification email", skip_all)]
async fn send_verification_email(
    email_client: &EmailClient,
    email: &Email,
    token: &str,
    settings: &RegistrationSettings,
) -> Result<(), anyhow::Error> {
    let verification_link = format!(
        "{}/verify-email?token={}",
        settings.verification_base_url, token
    );

    email_client
        .send_email(
            email,
            "Please verify your email address",
            &format!(
                "Welcome to Farms!<br />Click <a href=\"{verification_link}\">here</a> \
                 to verify your email address."
            ),
            &format!("Welcome to Farms!\nVisit {verification_link} to verify your email address."),
        )
        .await
        .map_err(|e| e.context("Failed to send the verification email."))
}
