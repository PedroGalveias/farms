use crate::helpers::{TestUser, spawn_app};
use farms::authentication::{ValidateCredentialsError, change_password, validate_credentials};
use farms::domain::user::Role;
use secrecy::SecretString;
use sqlx::PgPool;
use uuid::Uuid;

async fn assert_invalid_credentials(email: &str, password: SecretString, pool: &PgPool) {
    let error = validate_credentials(email, password, pool)
        .await
        .expect_err("Credentials validation should fail.");

    assert!(matches!(
        error,
        ValidateCredentialsError::InvalidCredentials(_)
    ));
}

#[tokio::test]
async fn validate_credentials_returns_authenticated_user_for_valid_credentials() {
    let app = spawn_app().await;
    let user = TestUser::generate_user();
    user.store(&app.db_pool).await;

    let authenticated_user =
        validate_credentials(&user.email, user.password_secret(), &app.db_pool)
            .await
            .expect("Credentials should be valid.");

    assert_eq!(authenticated_user.id, user.id);
    assert_eq!(authenticated_user.role, Role::User);
}

#[tokio::test]
async fn validate_credentials_returns_admin_role_for_admin_user() {
    let app = spawn_app().await;
    let user = TestUser::generate_admin();
    user.store(&app.db_pool).await;

    let authenticated_user =
        validate_credentials(&user.email, user.password_secret(), &app.db_pool)
            .await
            .expect("Credentials should be valid.");

    assert_eq!(authenticated_user.id, user.id);
    assert_eq!(authenticated_user.role, Role::Admin);
}

#[tokio::test]
async fn validate_credentials_rejects_wrong_password() {
    let app = spawn_app().await;
    let user = TestUser::generate_user();
    user.store(&app.db_pool).await;

    assert_invalid_credentials(
        &user.email,
        SecretString::from("wrong-password".to_string()),
        &app.db_pool,
    )
    .await;
}

#[tokio::test]
async fn validate_credentials_rejects_unknown_email() {
    let app = spawn_app().await;

    assert_invalid_credentials(
        "missing-user@example.com",
        SecretString::from("unknown-password".to_string()),
        &app.db_pool,
    )
    .await;
}

#[tokio::test]
async fn validate_credentials_returns_unexpected_error_when_the_query_fails() {
    let app = spawn_app().await;

    sqlx::query("ALTER TABLE users DROP COLUMN password_hash;")
        .execute(&app.db_pool)
        .await
        .expect("Failed to panic/break users table.");

    let error = validate_credentials(
        "test-user@example.com",
        SecretString::from("irrelevant-password".to_string()),
        &app.db_pool,
    )
    .await
    .expect_err("Credentials validation should fail.");

    assert!(matches!(
        error,
        ValidateCredentialsError::UnexpectedError(_)
    ));
}

#[tokio::test]
async fn change_password_allows_login_with_the_new_password() {
    let app = spawn_app().await;
    let user = TestUser::generate_user();
    user.store(&app.db_pool).await;

    let new_password = format!("new-password-{}", Uuid::new_v4());

    change_password(
        user.id,
        SecretString::from(new_password.clone()),
        &app.db_pool,
    )
    .await
    .expect("Password change should succeed.");

    assert_invalid_credentials(&user.email, user.password_secret(), &app.db_pool).await;

    let authenticated_user =
        validate_credentials(&user.email, SecretString::from(new_password), &app.db_pool)
            .await
            .expect("The new password should be valid.");

    assert_eq!(authenticated_user.id, user.id);
    assert_eq!(authenticated_user.role, user.role);
}
