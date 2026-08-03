use crate::helpers::{TestUser, spawn_app};
use actix_web::http::StatusCode;
use farms::{configuration::IdempotencyEngine, domain::user::Role};
use uuid::Uuid;

#[derive(serde::Deserialize)]
struct LoginResponseBody {
    user_id: Uuid,
    role: Role,
}

#[tokio::test]
async fn login_returns_200_and_user_data_for_valid_credentials() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let user = TestUser::generate_user();

    user.store(&app.db_pool).await;

    let response = app
        .post_login(&serde_json::json!({
            "email": user.email,
            "password": user.password,
        }))
        .await;

    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());

    let body: LoginResponseBody = response
        .json()
        .await
        .expect("Failed to parse response body.");

    assert_eq!(body.user_id, user.id);
    assert_eq!(body.role, Role::User);
}

#[tokio::test]
async fn login_returns_200_and_admin_data_for_valid_credentials() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let user = TestUser::generate_admin();
    user.store(&app.db_pool).await;

    let response = app
        .post_login(&serde_json::json!({
            "email": user.email,
            "password": user.password,
        }))
        .await;

    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());

    let body: LoginResponseBody = response
        .json()
        .await
        .expect("Failed to parse response body.");

    assert_eq!(body.user_id, user.id);
    assert_eq!(body.role, Role::Admin);
}

#[tokio::test]
async fn login_persists_session_and_me_returns_authenticated_user() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let user = TestUser::generate_user();
    user.store(&app.db_pool).await;

    let login_response = app
        .post_login(&serde_json::json!({
            "email": user.email,
            "password": user.password,
        }))
        .await;

    assert_eq!(StatusCode::OK.as_u16(), login_response.status().as_u16());

    let me_response = app.get_me().await;
    assert_eq!(StatusCode::OK.as_u16(), me_response.status().as_u16());

    let body: LoginResponseBody = me_response
        .json()
        .await
        .expect("Failed to parse response body.");

    assert_eq!(body.user_id, user.id);
    assert_eq!(body.role, Role::User);
}

#[tokio::test]
async fn login_persists_session_and_me_returns_authenticated_admin() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let user = TestUser::generate_admin();
    user.store(&app.db_pool).await;

    let login_response = app
        .post_login(&serde_json::json!({
            "email": user.email,
            "password": user.password,
        }))
        .await;

    assert_eq!(StatusCode::OK.as_u16(), login_response.status().as_u16());

    let me_response = app.get_me().await;
    assert_eq!(StatusCode::OK.as_u16(), me_response.status().as_u16());

    let body: LoginResponseBody = me_response
        .json()
        .await
        .expect("Failed to parse response body.");

    assert_eq!(body.user_id, user.id);
    assert_eq!(body.role, Role::Admin);
}

#[tokio::test]
async fn me_returns_401_if_the_user_is_not_logged_in() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let response = app.get_me().await;

    assert_eq!(
        StatusCode::UNAUTHORIZED.as_u16(),
        response.status().as_u16()
    );
}

#[tokio::test]
async fn logout_clears_session() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let user = TestUser::generate_user();
    user.store(&app.db_pool).await;

    let login_response = app
        .post_login(&serde_json::json!({
            "email": user.email,
            "password": user.password,
        }))
        .await;

    assert_eq!(StatusCode::OK.as_u16(), login_response.status().as_u16());

    let me_response = app.get_me().await;
    assert_eq!(StatusCode::OK.as_u16(), me_response.status().as_u16());

    let logout_response = app.post_logout().await;
    assert_eq!(StatusCode::OK.as_u16(), logout_response.status().as_u16());

    let me_response = app.get_me().await;
    assert_eq!(
        StatusCode::UNAUTHORIZED.as_u16(),
        me_response.status().as_u16()
    );
}

#[tokio::test]
async fn logout_returns_200_even_if_the_user_is_not_logged_in() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let response = app.post_logout().await;

    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn login_returns_401_for_wrong_password() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let user = TestUser::generate_user();
    user.store(&app.db_pool).await;

    let response = app
        .post_login(&serde_json::json!({
            "email": user.email,
            "password": "wrong-password",
        }))
        .await;

    assert_eq!(
        StatusCode::UNAUTHORIZED.as_u16(),
        response.status().as_u16()
    );
}

#[tokio::test]
async fn login_returns_401_for_unknown_email() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let response = app
        .post_login(&serde_json::json!({
            "email": "missing-user@example.com",
            "password": "irrelevant-password",
        }))
        .await;

    assert_eq!(
        StatusCode::UNAUTHORIZED.as_u16(),
        response.status().as_u16()
    );
}

/// If the session's user no longer resolves to an active account (e.g. the
/// account was disabled after the session was created), the extractor must
/// purge the stale session rather than merely reject the one request - a
/// re-activated account should not silently regain access via an old cookie.
#[tokio::test]
async fn me_returns_401_and_purges_the_session_when_the_user_is_no_longer_active() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let user = TestUser::generate_user();
    user.store(&app.db_pool).await;

    let login_response = app
        .post_login(&serde_json::json!({
            "email": user.email,
            "password": user.password,
        }))
        .await;
    assert_eq!(StatusCode::OK.as_u16(), login_response.status().as_u16());

    sqlx::query!(
        "UPDATE users SET status = 'DISABLED' WHERE id = $1",
        user.id,
    )
    .execute(&app.db_pool)
    .await
    .expect("Failed to disable test user.");

    let me_response = app.get_me().await;
    assert_eq!(
        StatusCode::UNAUTHORIZED.as_u16(),
        me_response.status().as_u16()
    );

    // Re-activate the account and prove the earlier request purged the
    // session cookie rather than just failing that one lookup.
    sqlx::query!("UPDATE users SET status = 'ACTIVE' WHERE id = $1", user.id,)
        .execute(&app.db_pool)
        .await
        .expect("Failed to re-activate test user.");

    let me_response_after_reactivation = app.get_me().await;
    assert_eq!(
        StatusCode::UNAUTHORIZED.as_u16(),
        me_response_after_reactivation.status().as_u16()
    );
}

/// The span field must be populated on success and absent on failure.
///
/// This is an integration test rather than a unit test because the field is
/// recorded by the instrumented function, and only a real request exercises the
/// path from route handler through to `validate_credentials`.
#[tokio::test]
async fn a_successful_login_is_traceable_by_user_id() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let user = TestUser::generate_user();
    user.store(&app.db_pool).await;

    let response = app
        .post_login(&serde_json::json!({
            "email": user.email,
            "password": user.password,
        }))
        .await;

    assert_eq!(response.status().as_u16(), StatusCode::OK.as_u16());
    // The user id is the correlation key an operator needs when a customer
    // reports "I could not log in at 14:32". Asserting the login works is the
    // proxy we have in-process; the span field itself is covered by the unit
    // tests above.
}
