use crate::helpers::spawn_app;
use farms::configuration::IdempotencyEngine;

#[tokio::test]
async fn health_check() {
    // Arrange
    let app = spawn_app(IdempotencyEngine::None).await;
    let client = app.api_client;

    // Act
    let response = client
        .get(format!("{}/health_check", &app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert
    assert!(response.status().is_success());
    assert_eq!(Some(0), response.content_length());
}
