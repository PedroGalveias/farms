use crate::helpers::{
    insert_test_farm, link_farm_category, link_farm_product, seed_test_taxonomy, spawn_app,
};
use actix_web::http::StatusCode;
use farms::configuration::IdempotencyEngine;

async fn farms_array(response: reqwest::Response) -> Vec<serde_json::Value> {
    let body: serde_json::Value = response.json().await.unwrap();
    body["farms"].as_array().unwrap().clone()
}

/// Move a seeded farm to known coordinates (the fixtures insert at 8.5, 47.4).
async fn set_coords(app: &crate::helpers::TestApp, farm: uuid::Uuid, lng: f64, lat: f64) {
    sqlx::query!(
        "UPDATE farms SET coordinates = POINT($1, $2) WHERE id = $3",
        lng,
        lat,
        farm,
    )
    .execute(&app.db_pool)
    .await
    .unwrap();
}

/// Set a farm's canton (fixtures default to ZH).
async fn set_canton(app: &crate::helpers::TestApp, farm: uuid::Uuid, canton: &str) {
    sqlx::query!("UPDATE farms SET canton = $1 WHERE id = $2", canton, farm)
        .execute(&app.db_pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn filters_by_canton() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let zh = insert_test_farm(&app.db_pool, "Zurich Farm").await;
    let be = insert_test_farm(&app.db_pool, "Bern Farm").await;
    set_canton(&app, be, "BE").await;

    let response = app
        .api_client
        .get(format!("{}/farms?canton=BE", app.address))
        .send()
        .await
        .unwrap();
    let farms = farms_array(response).await;
    assert_eq!(1, farms.len());
    assert_eq!(be.to_string(), farms[0]["id"].as_str().unwrap());
    assert_ne!(zh.to_string(), farms[0]["id"].as_str().unwrap());
}

#[tokio::test]
async fn free_text_q_matches_name_and_product() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;

    let by_name = insert_test_farm(&app.db_pool, "Zaugg Beerenhof").await;
    let by_product = insert_test_farm(&app.db_pool, "Nondescript Farm").await;
    link_farm_product(&app.db_pool, by_product, taxonomy.strawberries_id).await;
    let _unrelated = insert_test_farm(&app.db_pool, "Unrelated").await;

    // Matches the farm name.
    let named = farms_array(
        app.api_client
            .get(format!("{}/farms?q=beerenhof", app.address))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(1, named.len());
    assert_eq!(by_name.to_string(), named[0]["id"].as_str().unwrap());

    // Matches a product name (Strawberries) on a farm whose name doesn't.
    let by_prod = farms_array(
        app.api_client
            .get(format!("{}/farms?q=strawber", app.address))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(1, by_prod.len());
    assert_eq!(by_product.to_string(), by_prod[0]["id"].as_str().unwrap());
}

#[tokio::test]
async fn nearest_sort_orders_by_distance_and_reports_km() {
    let app = spawn_app(IdempotencyEngine::None).await;

    // Bern ~ (7.44, 46.95); Zurich ~ (8.54, 47.37). Query from near Bern.
    let bern = insert_test_farm(&app.db_pool, "Bern Farm").await;
    set_coords(&app, bern, 7.44, 46.95).await;
    let zurich = insert_test_farm(&app.db_pool, "Zurich Farm").await;
    set_coords(&app, zurich, 8.54, 47.37).await;

    let response = app
        .api_client
        .get(format!(
            "{}/farms?lat=46.95&lng=7.45&sort=nearest",
            app.address
        ))
        .send()
        .await
        .unwrap();
    let farms = farms_array(response).await;
    assert_eq!(2, farms.len());
    assert_eq!(
        bern.to_string(),
        farms[0]["id"].as_str().unwrap(),
        "nearest farm should come first"
    );
    // Distance is reported and Bern is essentially at the query point.
    let d0 = farms[0]["distance_km"].as_f64().unwrap();
    let d1 = farms[1]["distance_km"].as_f64().unwrap();
    assert!(d0 < 5.0, "Bern within 5km, got {d0}");
    assert!(d1 > d0, "Zurich should be farther than Bern");
}

#[tokio::test]
async fn radius_filters_out_distant_farms() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let bern = insert_test_farm(&app.db_pool, "Bern Farm").await;
    set_coords(&app, bern, 7.44, 46.95).await;
    let zurich = insert_test_farm(&app.db_pool, "Zurich Farm").await;
    set_coords(&app, zurich, 8.54, 47.37).await;

    // Bern↔Zurich is ~95 km; a 30 km radius around Bern keeps only Bern.
    let response = app
        .api_client
        .get(format!(
            "{}/farms?lat=46.95&lng=7.45&radius_km=30",
            app.address
        ))
        .send()
        .await
        .unwrap();
    let farms = farms_array(response).await;
    assert_eq!(1, farms.len());
    assert_eq!(bern.to_string(), farms[0]["id"].as_str().unwrap());
    assert_ne!(zurich.to_string(), farms[0]["id"].as_str().unwrap());
}

#[tokio::test]
async fn nearest_sort_requires_a_location() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let response = app
        .api_client
        .get(format!("{}/farms?sort=nearest", app.address))
        .send()
        .await
        .unwrap();
    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn offset_pagination_pages_and_stops() {
    let app = spawn_app(IdempotencyEngine::None).await;
    for i in 0..3 {
        insert_test_farm(&app.db_pool, &format!("Farm {i}")).await;
    }

    let page1: serde_json::Value = app
        .api_client
        .get(format!("{}/farms?limit=2&offset=0", app.address))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(2, page1["farms"].as_array().unwrap().len());
    assert_eq!("2", page1["next_cursor"].as_str().unwrap());

    let page2: serde_json::Value = app
        .api_client
        .get(format!("{}/farms?limit=2&offset=2", app.address))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(1, page2["farms"].as_array().unwrap().len());
    assert!(page2["next_cursor"].is_null(), "last page has no cursor");
}

#[tokio::test]
async fn category_and_group_only_still_work_after_geo_changes() {
    // Regression guard: the sub-categories behavior survives the rewrite.
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;

    let group_only = insert_test_farm(&app.db_pool, "Group Only").await;
    link_farm_category(&app.db_pool, group_only, taxonomy.vegetables_category_id).await;

    let farms = farms_array(
        app.api_client
            .get(format!("{}/farms?category=vegetables", app.address))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(1, farms.len());
    assert_eq!(group_only.to_string(), farms[0]["id"].as_str().unwrap());
}
