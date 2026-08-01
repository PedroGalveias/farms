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

/// Whether a failure was the label constraint rather than some unrelated error.
///
/// Asserting only `is_err()` would let this test pass for the wrong reason — a
/// typo'd column or a unique violation would look identical.
fn violates_the_label_constraint(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|db| db.constraint())
        .is_some_and(|name| name == "product_categories_labels_not_blank")
}

#[tokio::test]
async fn the_database_rejects_a_blank_label() {
    // Everything /taxonomy promises about `name` rests on NULL being the only
    // spelling of "missing". Postgres `text` would otherwise accept '' and
    // '   ', which read as translated and render as nothing at all.
    let app = spawn_app(IdempotencyEngine::None).await;

    // Static SQL per case: the column name varies, and interpolating it would
    // (rightly) trip the dynamic-SQL lint.
    let rejected = [
        (
            "an empty translation",
            "INSERT INTO product_categories (key_de, slug, display_order, name_en)
             VALUES ('Kanonisch', 'blank-en', 0, '')",
        ),
        (
            "a whitespace-only translation",
            "INSERT INTO product_categories (key_de, slug, display_order, name_fr)
             VALUES ('Kanonisch', 'blank-fr', 0, '   ')",
        ),
        (
            "a blank canonical label",
            "INSERT INTO product_categories (key_de, slug, display_order, name_en)
             VALUES ('', 'blank-de', 0, 'Canonical')",
        ),
    ];

    for (description, statement) in rejected {
        let error = sqlx::query(statement)
            .execute(&app.db_pool)
            .await
            .expect_err(&format!("{description} must be rejected"));

        assert!(
            violates_the_label_constraint(&error),
            "{description} should trip the label constraint, got: {error}"
        );
    }

    // NULL stays the supported way to say "not translated yet" — the constraint
    // must not have made the columns effectively mandatory.
    sqlx::query(
        "INSERT INTO product_categories (key_de, slug, display_order, name_en)
         VALUES ('Kanonisch', 'null-en', 0, NULL)",
    )
    .execute(&app.db_pool)
    .await
    .expect("an untranslated category must still be insertable");
}

#[tokio::test]
async fn is_cacheable() {
    // The vocabulary is a startup snapshot, so it cannot change until the app
    // restarts. Clients on the critical path (pickers, type-aheads) should not
    // re-fetch it on every navigation, and the doc comment claiming it is a
    // "good candidate for caching" is worth nothing without the header.
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    let response = app
        .api_client
        .get(format!("{}/taxonomy", app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    let cache_control = response
        .headers()
        .get("cache-control")
        .expect("Cache-Control must be set")
        .to_str()
        .unwrap()
        .to_string();

    assert!(cache_control.contains("public"), "got: {cache_control}");
    assert!(cache_control.contains("max-age="), "got: {cache_control}");
}

#[tokio::test]
async fn a_rejected_language_is_not_cached() {
    // A 400 must not be cached as though it were the vocabulary — a client that
    // fixed its `lang` should not keep being served the error.
    let app = spawn_app(IdempotencyEngine::None).await;

    let response = app
        .api_client
        .get(format!("{}/taxonomy?lang=es", app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    assert_eq!(response.status().as_u16(), StatusCode::BAD_REQUEST.as_u16());
    assert!(
        response.headers().get("cache-control").is_none(),
        "a 400 carried a Cache-Control header"
    );
}
