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

#[tokio::test]
async fn lists_every_product_with_its_category() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    let body = taxonomy(&app, "").await;
    let products = body["products"].as_array().expect("products array");

    // A picker needs the whole vocabulary, not just what happens to be attached
    // to the farms on the current page.
    assert_eq!(products.len(), 4, "got: {products:?}");

    let strawberries = products
        .iter()
        .find(|p| p["slug"] == "strawberries")
        .expect("strawberries");
    assert_eq!(strawberries["name"], "Strawberries");
    assert_eq!(
        strawberries["category"], "fruits",
        "a product must name its group so a client can build a grouped picker \
         without a second lookup"
    );
}

#[tokio::test]
async fn products_are_grouped_by_category_in_display_order() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    let body = taxonomy(&app, "").await;
    let categories: Vec<&str> = body["products"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["category"].as_str().unwrap())
        .collect();

    // fruits has display_order 0, vegetables 1. A client should be able to
    // render the list top to bottom without sorting it first.
    assert_eq!(categories, ["fruits", "fruits", "fruits", "vegetables"]);
}

#[tokio::test]
async fn a_product_is_localised_when_a_translation_exists() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    for (code, label) in [
        ("de", "Erdbeeren"),
        ("en", "Strawberries"),
        ("fr", "Fraises"),
        ("it", "Fragole"),
        ("rm", "Fraivas"),
    ] {
        let body = taxonomy(&app, &format!("?lang={code}")).await;
        let strawberries = body["products"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["slug"] == "strawberries")
            .expect("strawberries")
            .clone();

        assert_eq!(strawberries["name"], label, "lang={code}");
        assert_eq!(strawberries["translated"], true, "lang={code}");
    }
}

#[tokio::test]
async fn an_untranslated_product_falls_back_to_english_and_says_so() {
    // The behaviour issue #130 specifies, checked through the endpoint rather
    // than against the resolver in isolation.
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    for code in ["fr", "it", "rm"] {
        let body = taxonomy(&app, &format!("?lang={code}")).await;
        let cherries = body["products"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["slug"] == "cherries")
            .expect("cherries")
            .clone();

        assert_eq!(
            cherries["name"], "Cherries",
            "lang={code} should fall back to English"
        );
        assert_eq!(
            cherries["translated"], false,
            "lang={code} is a fallback, and the response must not claim otherwise"
        );
        // The stable key survives the fallback — that is the whole point of
        // separating identity from display.
        assert_eq!(cherries["slug"], "cherries", "lang={code}");
    }
}

#[tokio::test]
async fn german_is_the_last_resort_for_a_product() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    // `damsons` boots with a German label and nothing else.
    for code in ["en", "fr", "it", "rm"] {
        let body = taxonomy(&app, &format!("?lang={code}")).await;
        let damsons = body["products"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["slug"] == "damsons")
            .expect("damsons")
            .clone();

        assert_eq!(
            damsons["name"], "Zwetschgen",
            "lang={code} should reach the German label rather than return nothing"
        );
        assert_eq!(
            damsons["translated"], false,
            "lang={code} is served the canonical label, not a translation"
        );
    }

    // And German itself is not a fallback.
    let body = taxonomy(&app, "?lang=de").await;
    let damsons = body["products"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["slug"] == "damsons")
        .expect("damsons")
        .clone();
    assert_eq!(damsons["name"], "Zwetschgen");
    assert_eq!(damsons["translated"], true);
}

#[tokio::test]
async fn the_database_rejects_a_blank_product_label() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;
    let fruits = taxonomy.fruits_category_id;

    let error = sqlx::query(
        "INSERT INTO products (category_id, key_de, slug, name_en)
         VALUES ($1, 'Leer', 'blank-probe', '   ')",
    )
    .bind(fruits)
    .execute(&app.db_pool)
    .await
    .expect_err("a whitespace-only product label must be rejected");

    assert_eq!(
        error
            .as_database_error()
            .and_then(|db| db.constraint())
            .unwrap_or_default(),
        "products_labels_not_blank",
        "got: {error}"
    );
}
