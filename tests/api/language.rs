use crate::helpers::{insert_test_farm, spawn_app};
use actix_web::http::StatusCode;
use farms::configuration::IdempotencyEngine;

/// The `lang` the server resolved for a list request.
async fn resolved_lang(response: reqwest::Response) -> String {
    let body: serde_json::Value = response.json().await.unwrap();
    body["lang"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn defaults_to_english_when_no_language_is_requested() {
    let app = spawn_app(IdempotencyEngine::None).await;
    insert_test_farm(&app.db_pool, "Default Language Farm").await;

    let response = app
        .api_client
        .get(format!("{}/farms", app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    assert_eq!(response.status().as_u16(), StatusCode::OK.as_u16());
    // Echoing the resolution back is what lets a client tell "defaulted" apart
    // from "honoured my request".
    assert_eq!(resolved_lang(response).await, "en");
}

#[tokio::test]
async fn honours_every_supported_language() {
    let app = spawn_app(IdempotencyEngine::None).await;
    insert_test_farm(&app.db_pool, "Multilingual Farm").await;

    for code in ["en", "de", "fr", "it", "rm"] {
        let response = app
            .api_client
            .get(format!("{}/farms?lang={code}", app.address))
            .send()
            .await
            .expect("Failed to execute request.");

        assert_eq!(
            response.status().as_u16(),
            StatusCode::OK.as_u16(),
            "lang={code}"
        );
        assert_eq!(resolved_lang(response).await, code);
    }
}

#[tokio::test]
async fn accepts_a_regional_tag_and_reports_the_base_language() {
    let app = spawn_app(IdempotencyEngine::None).await;
    insert_test_farm(&app.db_pool, "Swiss German Farm").await;

    // Swiss clients commonly send de-CH; it must not be a client error.
    let response = app
        .api_client
        .get(format!("{}/farms?lang=de-CH", app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    assert_eq!(response.status().as_u16(), StatusCode::OK.as_u16());
    assert_eq!(resolved_lang(response).await, "de");
}

#[tokio::test]
async fn is_case_insensitive() {
    let app = spawn_app(IdempotencyEngine::None).await;
    insert_test_farm(&app.db_pool, "Shouty Farm").await;

    let response = app
        .api_client
        .get(format!("{}/farms?lang=DE", app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    assert_eq!(response.status().as_u16(), StatusCode::OK.as_u16());
    assert_eq!(resolved_lang(response).await, "de");
}

#[tokio::test]
async fn rejects_an_unsupported_language_with_400() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let response = app
        .api_client
        .get(format!("{}/farms?lang=es", app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    // Consistent with unknown category/product slugs: a caller who asked for
    // something specific is told they did not get it.
    assert_eq!(response.status().as_u16(), StatusCode::BAD_REQUEST.as_u16());
    let body = response.text().await.unwrap();
    assert!(body.contains("es"), "message should name the input: {body}");
    assert!(
        body.contains("en, de, fr, it, rm"),
        "message should list the supported codes: {body}"
    );
}

#[tokio::test]
async fn an_empty_language_parameter_falls_back_to_the_default() {
    let app = spawn_app(IdempotencyEngine::None).await;
    insert_test_farm(&app.db_pool, "Empty Lang Farm").await;

    // `?lang=` reads as "unset", not as a typo.
    let response = app
        .api_client
        .get(format!("{}/farms?lang=", app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    assert_eq!(response.status().as_u16(), StatusCode::OK.as_u16());
    assert_eq!(resolved_lang(response).await, "en");
}

#[tokio::test]
async fn the_detail_endpoint_validates_the_language_too() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let farm = insert_test_farm(&app.db_pool, "Detail Farm").await;

    let ok = app
        .api_client
        .get(format!("{}/farms/{farm}?lang=fr", app.address))
        .send()
        .await
        .expect("Failed to execute request.");
    assert_eq!(ok.status().as_u16(), StatusCode::OK.as_u16());

    // Both endpoints reject the same way, so the contract is learnable from
    // either one.
    let rejected = app
        .api_client
        .get(format!("{}/farms/{farm}?lang=es", app.address))
        .send()
        .await
        .expect("Failed to execute request.");
    assert_eq!(rejected.status().as_u16(), StatusCode::BAD_REQUEST.as_u16());
}

#[tokio::test]
async fn language_does_not_change_which_farms_match() {
    let app = spawn_app(IdempotencyEngine::None).await;
    insert_test_farm(&app.db_pool, "Stable Keys Farm").await;
    insert_test_farm(&app.db_pool, "Another Farm").await;

    // The whole point of separating keys from labels: switching language must
    // never change the result set, only how it reads.
    let mut counts = Vec::new();
    for code in ["en", "de", "fr", "it", "rm"] {
        let body: serde_json::Value = app
            .api_client
            .get(format!("{}/farms?lang={code}", app.address))
            .send()
            .await
            .expect("Failed to execute request.")
            .json()
            .await
            .unwrap();
        counts.push(body["farms"].as_array().unwrap().len());
    }
    assert!(
        counts.windows(2).all(|w| w[0] == w[1]),
        "result count varied by language: {counts:?}"
    );
}

#[tokio::test]
async fn the_detail_endpoint_echoes_the_language_too() {
    // The list endpoint reports which language it resolved to; the detail one
    // must as well, or a client has to special-case which response it is
    // reading to answer the same question.
    let app = spawn_app(IdempotencyEngine::None).await;
    let farm_id = insert_test_farm(&app.db_pool, "Echo Farm").await;

    for (query, expected) in [
        ("", "en"),
        ("?lang=de", "de"),
        ("?lang=fr-CH", "fr"),
        ("?lang=", "en"),
    ] {
        let body: serde_json::Value = app
            .api_client
            .get(format!("{}/farms/{farm_id}{query}", app.address))
            .send()
            .await
            .expect("Failed to execute request.")
            .json()
            .await
            .unwrap();

        assert_eq!(body["lang"], expected, "query={query:?}");
        // Flattened, so the farm's own fields stay exactly where they were.
        assert_eq!(body["id"], farm_id.to_string(), "query={query:?}");
        assert_eq!(body["name"], "Echo Farm", "query={query:?}");
        assert!(body["products"].is_array(), "query={query:?}");
    }
}
