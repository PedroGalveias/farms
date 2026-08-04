use crate::helpers::{
    insert_test_farm_in_canton, link_farm_category, link_farm_product, seed_test_taxonomy,
    spawn_app,
};
use actix_web::http::StatusCode;
use farms::configuration::IdempotencyEngine;

async fn facets(app: &crate::helpers::TestApp, query: &str) -> serde_json::Value {
    app.api_client
        .get(format!("{}/facets{query}", app.address))
        .send()
        .await
        .expect("Failed to execute request.")
        .json()
        .await
        .unwrap()
}

fn count_for(body: &serde_json::Value, list: &str, key: &str, value: &str) -> i64 {
    body[list]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry[key].as_str() == Some(value))
        .unwrap_or_else(|| panic!("no {list} entry for {value}"))["count"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn counts_farms_per_canton() {
    let app = spawn_app(IdempotencyEngine::None).await;

    insert_test_farm_in_canton(&app.db_pool, "Berner Hof A", "BE").await;
    insert_test_farm_in_canton(&app.db_pool, "Berner Hof B", "BE").await;
    insert_test_farm_in_canton(&app.db_pool, "Zürcher Hof", "ZH").await;

    let body = facets(&app, "").await;

    assert_eq!(count_for(&body, "cantons", "code", "BE"), 2);
    assert_eq!(count_for(&body, "cantons", "code", "ZH"), 1);
    assert_eq!(body["total"].as_i64().unwrap(), 3);
}

#[tokio::test]
async fn omits_cantons_with_no_farms() {
    let app = spawn_app(IdempotencyEngine::None).await;
    insert_test_farm_in_canton(&app.db_pool, "Only farm", "BE").await;

    let body = facets(&app, "").await;
    let codes: Vec<&str> = body["cantons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["code"].as_str().unwrap())
        .collect();

    // A canton nobody farms in would be a dead entry in a picker.
    assert_eq!(codes, vec!["BE"]);
}

#[tokio::test]
async fn counts_a_farm_once_when_it_is_tagged_both_ways() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;

    // Belongs to `fruits` twice over: directly, and through a product in it.
    // The union that resolves the two paths must not double-count it.
    let farm = insert_test_farm_in_canton(&app.db_pool, "Doubly tagged", "BE").await;
    link_farm_category(&app.db_pool, farm, taxonomy.fruits_category_id).await;
    link_farm_product(&app.db_pool, farm, taxonomy.strawberries_id).await;

    let body = facets(&app, "").await;

    assert_eq!(count_for(&body, "categories", "slug", "fruits"), 1);
}

#[tokio::test]
async fn counts_a_farm_reached_only_through_a_product() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;

    // No direct category link at all — the only route to `fruits` is the
    // product. This is the same "any of" rule `GET /farms?category=` applies,
    // so the count has to agree with what filtering would return.
    let farm = insert_test_farm_in_canton(&app.db_pool, "Product only", "BE").await;
    link_farm_product(&app.db_pool, farm, taxonomy.cherries_id).await;

    let body = facets(&app, "").await;

    assert_eq!(count_for(&body, "categories", "slug", "fruits"), 1);
}

#[tokio::test]
async fn lists_every_category_including_empty_ones() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;

    let farm = insert_test_farm_in_canton(&app.db_pool, "Fruit farm", "BE").await;
    link_farm_category(&app.db_pool, farm, taxonomy.fruits_category_id).await;

    let body = facets(&app, "").await;

    // A picker needs the whole vocabulary; the count is what tells it which
    // options to grey out.
    assert_eq!(count_for(&body, "categories", "slug", "fruits"), 1);
    assert_eq!(count_for(&body, "categories", "slug", "vegetables"), 0);
}

#[tokio::test]
async fn category_labels_follow_the_language_but_slugs_do_not() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    let english = facets(&app, "?lang=en").await;
    let french = facets(&app, "?lang=fr").await;

    let slugs = |body: &serde_json::Value| -> Vec<String> {
        body["categories"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["slug"].as_str().unwrap().to_string())
            .collect()
    };
    let name_of = |body: &serde_json::Value, slug: &str| -> String {
        body["categories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["slug"].as_str() == Some(slug))
            .unwrap()["name"]
            .as_str()
            .unwrap()
            .to_string()
    };

    assert_eq!(slugs(&english), slugs(&french));
    assert_eq!(english["lang"].as_str().unwrap(), "en");
    assert_eq!(french["lang"].as_str().unwrap(), "fr");
    assert_eq!(name_of(&english, "vegetables"), "Vegetables");
    assert_eq!(name_of(&french, "vegetables"), "Légumes");
}

#[tokio::test]
async fn rejects_an_unsupported_language() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let response = app
        .api_client
        .get(format!("{}/facets?lang=xx", app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    assert_eq!(response.status().as_u16(), StatusCode::BAD_REQUEST.as_u16());
}

#[tokio::test]
async fn is_publicly_cacheable() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let response = app
        .api_client
        .get(format!("{}/facets", app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    // These counts describe the same data the frontend caches `GET /farms` for
    // five minutes; if the two lifetimes drift a visitor sees a count that
    // disagrees with the list beside it.
    let cache_control = response
        .headers()
        .get("cache-control")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        cache_control.contains("public") && cache_control.contains("max-age=300"),
        "unexpected cache-control: {cache_control}"
    );
}

#[tokio::test]
async fn reports_zero_for_an_empty_directory() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    let body = facets(&app, "").await;

    assert_eq!(body["total"].as_i64().unwrap(), 0);
    assert!(body["cantons"].as_array().unwrap().is_empty());
    // The vocabulary is still there — an empty directory is not an empty picker.
    assert!(!body["categories"].as_array().unwrap().is_empty());
    assert_eq!(count_for(&body, "categories", "slug", "fruits"), 0);
}
