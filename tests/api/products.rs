use crate::helpers::{insert_test_farm, link_farm_product, seed_test_taxonomy, spawn_app};
use actix_web::http::StatusCode;
use farms::configuration::IdempotencyEngine;

/// The `farms` array from a list response.
async fn farms_array(response: reqwest::Response) -> Vec<serde_json::Value> {
    let body: serde_json::Value = response.json().await.unwrap();
    body["farms"].as_array().unwrap().clone()
}

#[tokio::test]
async fn list_filters_by_product_slug() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;

    let has_strawberries = insert_test_farm(&app.db_pool, "Berry Farm").await;
    link_farm_product(&app.db_pool, has_strawberries, taxonomy.strawberries_id).await;

    let no_strawberries = insert_test_farm(&app.db_pool, "Cherry Only").await;
    link_farm_product(&app.db_pool, no_strawberries, taxonomy.cherries_id).await;

    let response = app
        .api_client
        .get(format!("{}/farms?product=strawberries", app.address))
        .send()
        .await
        .expect("Failed to execute request.");
    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());

    let farms = farms_array(response).await;
    assert_eq!(1, farms.len());
    assert_eq!(
        has_strawberries.to_string(),
        farms[0]["id"].as_str().unwrap()
    );

    // The response lists the farm's full product set with its derived category.
    let slugs: Vec<&str> = farms[0]["products"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["slug"].as_str().unwrap())
        .collect();
    assert_eq!(vec!["strawberries"], slugs);

    let categories: Vec<&str> = farms[0]["categories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    assert_eq!(vec!["fruits"], categories);
}

#[tokio::test]
async fn match_all_requires_every_product() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;

    let both = insert_test_farm(&app.db_pool, "Both").await;
    link_farm_product(&app.db_pool, both, taxonomy.strawberries_id).await;
    link_farm_product(&app.db_pool, both, taxonomy.cherries_id).await;

    let only_one = insert_test_farm(&app.db_pool, "One").await;
    link_farm_product(&app.db_pool, only_one, taxonomy.strawberries_id).await;

    let response = app
        .api_client
        .get(format!(
            "{}/farms?product=strawberries,cherries&match=all",
            app.address
        ))
        .send()
        .await
        .unwrap();
    let farms = farms_array(response).await;
    assert_eq!(1, farms.len());
    assert_eq!(both.to_string(), farms[0]["id"].as_str().unwrap());
}

#[tokio::test]
async fn unknown_product_slug_is_400() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    let response = app
        .api_client
        .get(format!("{}/farms?product=dragonfruit", app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn unknown_category_slug_is_400() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    let response = app
        .api_client
        .get(format!("{}/farms?category=made-up", app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}
