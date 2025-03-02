#[tokio::test]
async fn health_check() {
    // No .await, no expect
    spawn_app();

    // We need to bring in `reqwest`
    // to perform HTTP requests against our application
    let client = reqwest::Client::new();

    // Act
    let response = client
        .get("http://127.0.0.1:8000/health_check")
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert
    assert!(response.status().is_success());
    assert_eq!(Some(0), response.content_length());
}

// Launch the application in the background
fn spawn_app() {
    let server = farms::run().expect("Failed to bind address.");

    // Launch the server as a background task
    // tokio::spawn returns a handle to the spawned future,
    // but we have no use for it here, hence the non-binding let
    let _ = tokio::spawn(server);
}
