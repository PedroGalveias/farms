use crate::helpers::{seed_test_taxonomy, spawn_app};
use actix_web::http::StatusCode;
use farms::configuration::IdempotencyEngine;

async fn taxonomy(app: &crate::helpers::TestApp, query: &str) -> serde_json::Value {
    app.api_client
        .get(format!("{}/taxonomy{query}", app.address))
        .send()
        .await
        .expect("Failed to execute request.")
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn returns_the_category_vocabulary() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    let body = taxonomy(&app, "").await;
    let categories = body["categories"].as_array().unwrap();

    assert!(!categories.is_empty(), "expected some categories");
    // Every entry carries the stable key AND something displayable — a client
    // should never have to invent a label.
    for category in categories {
        assert!(category["slug"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(category["name"].as_str().is_some_and(|s| !s.is_empty()));
        assert!(category["translated"].is_boolean());
    }
}

#[tokio::test]
async fn labels_change_with_the_language_but_slugs_do_not() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    let slugs_for = |body: &serde_json::Value| -> Vec<String> {
        body["categories"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["slug"].as_str().unwrap().to_string())
            .collect()
    };

    let en = taxonomy(&app, "?lang=en").await;
    let de = taxonomy(&app, "?lang=de").await;

    // The identity is stable across languages — that is the whole contract.
    assert_eq!(slugs_for(&en), slugs_for(&de));
    assert_eq!(en["lang"], "en");
    assert_eq!(de["lang"], "de");
}

#[tokio::test]
async fn defaults_to_english_without_a_language() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    let body = taxonomy(&app, "").await;
    assert_eq!(body["lang"], "en");
}

#[tokio::test]
async fn rejects_an_unsupported_language() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let response = app
        .api_client
        .get(format!("{}/taxonomy?lang=es", app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    // Same contract as /farms — a client learns it once.
    assert_eq!(response.status().as_u16(), StatusCode::BAD_REQUEST.as_u16());
}

#[tokio::test]
async fn serves_the_real_translation_for_each_language() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    // The seeded labels are authored translations, so this pins actual output
    // rather than "some non-empty string" — which would pass even if every
    // language silently fell back to German.
    let expected = [
        ("en", "Vegetables"),
        ("de", "Gemüse"),
        ("fr", "Légumes"),
        ("it", "Verdura"),
        ("rm", "Verduras"),
    ];

    for (code, label) in expected {
        let body = taxonomy(&app, &format!("?lang={code}")).await;
        let vegetables = body["categories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["slug"] == "vegetables")
            .expect("vegetables category");

        assert_eq!(vegetables["name"], label, "lang={code}");
        // All five are authored for categories, so none should be a fallback.
        assert_eq!(
            vegetables["translated"], true,
            "lang={code} should be a real translation, not a fallback"
        );
    }
}

#[tokio::test]
async fn is_reachable_without_authentication() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    // Pickers and type-aheads need this before a visitor has any session.
    let response = app
        .api_client
        .get(format!("{}/taxonomy", app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    assert_eq!(response.status().as_u16(), StatusCode::OK.as_u16());
}
