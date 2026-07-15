use crate::helpers::{insert_test_farm, seed_test_taxonomy, spawn_app};
use actix_web::http::StatusCode;
use farms::configuration::IdempotencyEngine;
use serde_json::json;

#[tokio::test]
async fn submit_requires_authentication() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;
    let farm_id = insert_test_farm(&app.db_pool, "Farm").await;

    let response = app
        .api_client
        .post(format!(
            "{}/farms/{}/product-suggestions",
            app.address, farm_id
        ))
        .json(&json!({ "product": "strawberries", "action": "add" }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        StatusCode::UNAUTHORIZED.as_u16(),
        response.status().as_u16()
    );
}

#[tokio::test]
async fn authenticated_user_can_submit_pending_suggestion() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;
    let farm_id = insert_test_farm(&app.db_pool, "Farm").await;
    app.log_in_active_user().await;

    let response = app
        .api_client
        .post(format!(
            "{}/farms/{}/product-suggestions",
            app.address, farm_id
        ))
        .json(&json!({ "product": "strawberries", "action": "add" }))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::ACCEPTED.as_u16(), response.status().as_u16());

    let row = sqlx::query!(
        r#"SELECT status::text AS "status!" FROM farm_product_suggestions WHERE farm_id = $1"#,
        farm_id,
    )
    .fetch_one(&app.db_pool)
    .await
    .unwrap();
    assert_eq!("PENDING", row.status);
}

#[tokio::test]
async fn submit_rejects_unknown_product() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;
    let farm_id = insert_test_farm(&app.db_pool, "Farm").await;
    app.log_in_active_user().await;

    let response = app
        .api_client
        .post(format!(
            "{}/farms/{}/product-suggestions",
            app.address, farm_id
        ))
        .json(&json!({ "product": "dragonfruit", "action": "add" }))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn submit_on_missing_farm_is_404() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;
    app.log_in_active_user().await;

    let response = app
        .api_client
        .post(format!(
            "{}/farms/{}/product-suggestions",
            app.address,
            uuid::Uuid::new_v4()
        ))
        .json(&json!({ "product": "strawberries", "action": "add" }))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::NOT_FOUND.as_u16(), response.status().as_u16());
}
