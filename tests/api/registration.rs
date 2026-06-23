use crate::helpers::{TestApp, spawn_app};
use actix_web::http::StatusCode;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

const VALID_PASSWORD: &str = "a-long-enough-password";

fn unique_email() -> String {
    format!("user-{}@example.com", Uuid::new_v4())
}

/// Accept any number of outbound emails with a 200 response.
async fn mount_email_ok(app: &TestApp) {
    Mock::given(method("POST"))
        .and(path("/v1.1/email"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&app.email_server)
        .await;
}

#[tokio::test]
async fn register_returns_202_for_valid_input() {
    let app = spawn_app().await;
    mount_email_ok(&app).await;

    let response = app
        .post_register(&serde_json::json!({
            "email": unique_email(),
            "password": VALID_PASSWORD,
        }))
        .await;

    assert_eq!(StatusCode::ACCEPTED.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn register_returns_202_for_existing_email() {
    let app = spawn_app().await;
    mount_email_ok(&app).await;
    let email = unique_email();
    let body = serde_json::json!({ "email": email, "password": VALID_PASSWORD });

    let first = app.post_register(&body).await;
    let second = app.post_register(&body).await;

    assert_eq!(StatusCode::ACCEPTED.as_u16(), first.status().as_u16());
    // Duplicate registration is indistinguishable from the first.
    assert_eq!(StatusCode::ACCEPTED.as_u16(), second.status().as_u16());

    // Only one user row exists for that email.
    let count = sqlx::query!(
        r#"SELECT COUNT(*) as "count!" FROM users WHERE email_normalised = $1"#,
        email.to_lowercase(),
    )
    .fetch_one(&app.db_pool)
    .await
    .unwrap()
    .count;
    assert_eq!(1, count);
}

#[tokio::test]
async fn register_returns_400_for_invalid_email() {
    let app = spawn_app().await;

    let response = app
        .post_register(&serde_json::json!({
            "email": "not-an-email",
            "password": VALID_PASSWORD,
        }))
        .await;

    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn register_returns_400_for_short_password() {
    let app = spawn_app().await;

    let response = app
        .post_register(&serde_json::json!({
            "email": unique_email(),
            "password": "short", // < 12 chars
        }))
        .await;

    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn register_stores_pending_user_with_hashed_password() {
    let app = spawn_app().await;
    mount_email_ok(&app).await;
    // Mixed case to exercise normalisation.
    let email = format!("User-{}@Example.COM", Uuid::new_v4());

    app.post_register(&serde_json::json!({
        "email": email,
        "password": VALID_PASSWORD,
    }))
    .await;

    let stored = app.get_user(&email).await.expect("User was not stored.");
    assert_eq!("PENDING_VERIFICATION", stored.status);
    assert_eq!(email.to_lowercase(), stored.email_normalised);
    assert_eq!(email, stored.email); // original casing preserved
    assert!(
        stored.password_hash.starts_with("$argon2id$"),
        "Password should be stored as an Argon2id hash, got: {}",
        stored.password_hash
    );
    assert_ne!(VALID_PASSWORD, stored.password_hash);
}

#[tokio::test]
async fn register_stores_token_hash_not_raw_token() {
    let app = spawn_app().await;
    mount_email_ok(&app).await;
    let email = unique_email();

    app.post_register(&serde_json::json!({
        "email": email,
        "password": VALID_PASSWORD,
    }))
    .await;

    let raw_token = app.verification_token_from_email().await;
    let stored_hash = app
        .get_verification_token_hash(&email)
        .await
        .expect("No verification token stored.");

    assert_ne!(raw_token, stored_hash);
    assert_eq!(32, raw_token.len()); // raw token length
    assert_eq!(64, stored_hash.len()); // SHA-256 hex length
}

#[tokio::test]
async fn login_rejects_pending_user() {
    let app = spawn_app().await;
    mount_email_ok(&app).await;
    let email = unique_email();
    app.post_register(&serde_json::json!({
        "email": email,
        "password": VALID_PASSWORD,
    }))
    .await;

    let response = app
        .post_login(&serde_json::json!({
            "email": email,
            "password": VALID_PASSWORD,
        }))
        .await;

    assert_eq!(
        StatusCode::UNAUTHORIZED.as_u16(),
        response.status().as_u16()
    );
}

#[tokio::test]
async fn verify_email_activates_user_and_allows_login() {
    let app = spawn_app().await;
    mount_email_ok(&app).await;
    let email = unique_email();
    app.post_register(&serde_json::json!({
        "email": email,
        "password": VALID_PASSWORD,
    }))
    .await;
    let token = app.verification_token_from_email().await;

    let verify = app
        .post_verify_email(&serde_json::json!({ "token": token }))
        .await;
    assert_eq!(StatusCode::OK.as_u16(), verify.status().as_u16());

    let stored = app.get_user(&email).await.unwrap();
    assert_eq!("ACTIVE", stored.status);

    let login = app
        .post_login(&serde_json::json!({
            "email": email,
            "password": VALID_PASSWORD,
        }))
        .await;
    assert_eq!(StatusCode::OK.as_u16(), login.status().as_u16());
}

#[tokio::test]
async fn verify_email_rejects_used_token() {
    let app = spawn_app().await;
    mount_email_ok(&app).await;
    let email = unique_email();
    app.post_register(&serde_json::json!({
        "email": email,
        "password": VALID_PASSWORD,
    }))
    .await;
    let token = app.verification_token_from_email().await;

    let first = app
        .post_verify_email(&serde_json::json!({ "token": token }))
        .await;
    let second = app
        .post_verify_email(&serde_json::json!({ "token": token }))
        .await;

    assert_eq!(StatusCode::OK.as_u16(), first.status().as_u16());
    assert_eq!(
        StatusCode::BAD_REQUEST.as_u16(),
        second.status().as_u16(),
        "A verification token must be single-use."
    );
}

#[tokio::test]
async fn verify_email_rejects_expired_token() {
    let app = spawn_app().await;
    mount_email_ok(&app).await;
    let email = unique_email();
    app.post_register(&serde_json::json!({
        "email": email,
        "password": VALID_PASSWORD,
    }))
    .await;
    let token = app.verification_token_from_email().await;
    app.expire_verification_tokens(&email).await;

    let response = app
        .post_verify_email(&serde_json::json!({ "token": token }))
        .await;

    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn verify_email_rejects_unknown_token() {
    let app = spawn_app().await;

    let response = app
        .post_verify_email(&serde_json::json!({ "token": "does-not-exist" }))
        .await;

    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn register_is_rate_limited() {
    let app = spawn_app().await;
    mount_email_ok(&app).await;
    let max_requests = app.configuration.registration.rate_limit.max_requests;
    let email = unique_email();
    let body = serde_json::json!({ "email": email, "password": VALID_PASSWORD });

    // The first `max_requests` attempts are accepted...
    for _ in 0..max_requests {
        let response = app.post_register(&body).await;
        assert_eq!(StatusCode::ACCEPTED.as_u16(), response.status().as_u16());
    }

    // ...the next one trips the limit.
    let limited = app.post_register(&body).await;
    assert_eq!(
        StatusCode::TOO_MANY_REQUESTS.as_u16(),
        limited.status().as_u16()
    );
}
