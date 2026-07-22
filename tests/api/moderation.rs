use crate::helpers::{insert_test_farm, seed_test_taxonomy, spawn_app};
use actix_web::http::StatusCode;
use farms::configuration::IdempotencyEngine;
use uuid::Uuid;

#[tokio::test]
async fn non_admin_cannot_list_the_queue() {
    let app = spawn_app(IdempotencyEngine::None).await;
    app.log_in_active_user().await; // a plain USER

    let response = app
        .api_client
        .get(format!("{}/admin/product-suggestions", app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::FORBIDDEN.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn admin_sees_pending_suggestions_in_the_queue() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;
    let farm_id = insert_test_farm(&app.db_pool, "Farm").await;
    let submitter = app.log_in_admin_user().await;

    let suggestion_id = Uuid::new_v4();
    sqlx::query!(
        r#"
        INSERT INTO farm_product_suggestions
            (id, farm_id, product_id, action, note, submitted_by, status, created_at)
        VALUES ($1, $2, $3, 'ADD', NULL, $4, 'PENDING', now())
        "#,
        suggestion_id,
        farm_id,
        taxonomy.strawberries_id,
        submitter,
    )
    .execute(&app.db_pool)
    .await
    .unwrap();

    let response = app
        .api_client
        .get(format!("{}/admin/product-suggestions", app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());

    let body: serde_json::Value = response.json().await.unwrap();
    let rows = body.as_array().unwrap();
    assert_eq!(1, rows.len());
    assert_eq!(suggestion_id.to_string(), rows[0]["id"].as_str().unwrap());
    assert_eq!("strawberries", rows[0]["product_slug"].as_str().unwrap());
}

#[tokio::test]
async fn approving_add_creates_the_link_and_is_idempotent_on_second_approve() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;
    let farm_id = insert_test_farm(&app.db_pool, "Farm").await;

    let submitter = app.log_in_admin_user().await;
    let suggestion_id = Uuid::new_v4();
    sqlx::query!(
        r#"
        INSERT INTO farm_product_suggestions
            (id, farm_id, product_id, action, note, submitted_by, status, created_at)
        VALUES ($1, $2, $3, 'ADD', NULL, $4, 'PENDING', now())
        "#,
        suggestion_id,
        farm_id,
        taxonomy.strawberries_id,
        submitter,
    )
    .execute(&app.db_pool)
    .await
    .unwrap();

    let first = app
        .api_client
        .post(format!(
            "{}/admin/product-suggestions/{}/approve",
            app.address, suggestion_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::OK.as_u16(), first.status().as_u16());

    // The link now exists, marked AVAILABLE and confirmed (PR6 freshness bump).
    let row = sqlx::query!(
        r#"
        SELECT status::text AS "status!", last_confirmed_at
        FROM farm_products WHERE farm_id = $1 AND product_id = $2
        "#,
        farm_id,
        taxonomy.strawberries_id,
    )
    .fetch_one(&app.db_pool)
    .await
    .unwrap();
    assert_eq!("AVAILABLE", row.status);
    assert!(row.last_confirmed_at.is_some());

    // A second approve is a conflict (no longer pending).
    let second = app
        .api_client
        .post(format!(
            "{}/admin/product-suggestions/{}/approve",
            app.address, suggestion_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::CONFLICT.as_u16(), second.status().as_u16());
}

#[tokio::test]
async fn rejecting_leaves_no_link_and_conflicts_on_second_review() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;
    let farm_id = insert_test_farm(&app.db_pool, "Farm").await;

    let submitter = app.log_in_admin_user().await;
    let suggestion_id = Uuid::new_v4();
    sqlx::query!(
        r#"
        INSERT INTO farm_product_suggestions
            (id, farm_id, product_id, action, note, submitted_by, status, created_at)
        VALUES ($1, $2, $3, 'ADD', NULL, $4, 'PENDING', now())
        "#,
        suggestion_id,
        farm_id,
        taxonomy.strawberries_id,
        submitter,
    )
    .execute(&app.db_pool)
    .await
    .unwrap();

    let response = app
        .api_client
        .post(format!(
            "{}/admin/product-suggestions/{}/reject",
            app.address, suggestion_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());

    let count = sqlx::query!(
        r#"SELECT count(*) AS "count!" FROM farm_products WHERE farm_id = $1"#,
        farm_id,
    )
    .fetch_one(&app.db_pool)
    .await
    .unwrap()
    .count;
    assert_eq!(0, count);

    let second = app
        .api_client
        .post(format!(
            "{}/admin/product-suggestions/{}/reject",
            app.address, suggestion_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::CONFLICT.as_u16(), second.status().as_u16());
}
