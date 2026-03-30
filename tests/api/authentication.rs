use crate::helpers::{TestUser, spawn_app};
use actix_web::http::StatusCode;
use farms::domain::user::Role;
use uuid::Uuid;

#[derive(serde::Deserialize)]
struct LoginResponseBody {
    user_id: Uuid,
    role: Role,
}

#[tokio::test]
async fn login_returns_200_and_user_data_for_valid_credentials() {
    let app = spawn_app().await;
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
async fn login_returns_admin_role_for_admin_user() {
    let app = spawn_app().await;
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
async fn login_returns_401_for_wrong_password() {
    let app = spawn_app().await;
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
    let app = spawn_app().await;

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
