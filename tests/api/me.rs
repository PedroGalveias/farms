use crate::helpers::{TestUser, spawn_app};
use actix_web::http::StatusCode;
use farms::configuration::IdempotencyEngine;

#[tokio::test]
async fn me_returns_username_and_role() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let user = TestUser::generate_user();
    user.store(&app.db_pool).await;
    app.post_login(&serde_json::json!({
        "email": user.email, "password": user.password
    }))
    .await;

    let response = app.get_me().await;
    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());

    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(user.username, body["username"].as_str().unwrap());
    assert_eq!(user.id.to_string(), body["user_id"].as_str().unwrap());
    assert_eq!("user", body["role"].as_str().unwrap());
}

#[tokio::test]
async fn me_requires_authentication() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let response = app.get_me().await;
    assert_eq!(
        StatusCode::UNAUTHORIZED.as_u16(),
        response.status().as_u16()
    );
}
