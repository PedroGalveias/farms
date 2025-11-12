use chrono::{DateTime, Utc};
use farms::configuration::{get_configuration, DatabaseSettings};
use farms::routes::Farm;
use farms::startup::run;
use farms::telemetry::{get_subscriber, init_subscriber};
use once_cell::sync::Lazy;
use secrecy::ExposeSecret;
use sqlx::types::Uuid;
use sqlx::{Connection, Executor, PgConnection, PgPool};
use std::collections::HashSet;
use std::net::TcpListener;

// Ensure that the `tracing` stack is only initialised once using `once_cell`
static TRACING: Lazy<()> = Lazy::new(|| {
    let default_filter_level = "info".to_string();
    let subscriber_name = "test".to_string();
    // We cannot assign the output of `get_subscriber` to a variable based on the
    // value TEST_LOG` because the sink is part of the type returned by
    // `get_subscriber`, therefore they are not the same type. We could work around
    // it, but this is the most straight-forward way of moving forward.
    if std::env::var("TEST_LOG").is_ok() {
        let subscriber = get_subscriber(subscriber_name, default_filter_level, std::io::stdout);
        init_subscriber(subscriber);
    } else {
        let subscriber = get_subscriber(subscriber_name, default_filter_level, std::io::sink);
        init_subscriber(subscriber);
    };
});

pub struct TestApp {
    pub address: String,
    pub db_pool: PgPool,
}

// Launch the application in the background
async fn spawn_app() -> TestApp {
    // The first time `initialize` is invoked the code in `TRACING` is executed.
    // All other invocations will instead skip execution.
    Lazy::force(&TRACING);

    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind random port");
    // We retrieve the port assigned to us by the OS
    let port = listener.local_addr().unwrap().port();
    let address = format!("127.0.0.1:{}", port);

    let mut configuration = get_configuration().expect("Failed to read configuration.");
    configuration.database.database_name = Uuid::new_v4().to_string();

    let connection_pool = configure_database(&configuration.database).await;

    let server = run(listener, connection_pool.clone()).expect("Failed to bind address");

    // Launch the server as a background task
    // tokio::spawn returns a handle to the spawned future,
    // but we have no use for it here, hence the non-binding let
    let _ = tokio::spawn(server);

    // Return the TestApp struct to the caller!
    TestApp {
        address,
        db_pool: connection_pool,
    }
}

pub async fn configure_database(config: &DatabaseSettings) -> PgPool {
    // Create database
    let mut connection =
        PgConnection::connect(&config.connection_string_without_db().expose_secret())
            .await
            .expect("Failed to connect to Postgres.");
    connection
        .execute(format!(r#"CREATE DATABASE "{}";"#, config.database_name).as_str())
        .await
        .expect("Failed to create database.");

    // Migrate database
    let connection_pool = PgPool::connect(&config.connection_string().expose_secret())
        .await
        .expect("Failed to connect to Postgres.");
    sqlx::migrate!("./migrations")
        .run(&connection_pool)
        .await
        .expect("Failed to migrate the database");

    connection_pool
}

#[tokio::test]
async fn health_check() {
    // Arrange
    let app = spawn_app().await;
    let client = reqwest::Client::new();

    // Act
    let response = client
        .get(&format!("http://{}/health_check", &app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert
    assert!(response.status().is_success());
    assert_eq!(Some(0), response.content_length());
}

#[tokio::test]
async fn create_farm_returns_a_200_for_valid_form_data() {
    // Arrange
    let app = spawn_app().await;
    let client = reqwest::Client::new();

    // Act
    let body = "name=Farmy&address=Bahnhofstrasse%2C%205401%20Baden&canton=Aargau&coordinates=F8G5%2BJ3&categories[]=Organic&categories[]=Fruit&categories[]=Vegetables";

    let response = client
        .post(&format!("http://{}/farms", &app.address))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert
    assert_eq!(200, response.status().as_u16());

    let saved = sqlx::query!("SELECT * FROM farms",)
        .fetch_one(&app.db_pool)
        .await
        .expect("Failed to fetch saved subscription.");

    assert_eq!(saved.name, "Farmy");
    assert_eq!(saved.address, "Bahnhofstrasse, 5401 Baden");
    assert_eq!(saved.canton, "Aargau");
    assert_eq!(saved.coordinates, "F8G5+J3");
    assert_eq!(
        saved.categories.into_iter().collect::<HashSet<_>>(),
        ["Organic", "Fruit", "Vegetables"]
            .into_iter()
            .map(String::from)
            .collect::<HashSet<_>>()
    );
}

// TODO: Insert multiple farms and test. But first, have the test running with a single farm.
#[tokio::test]
async fn get_farms_returns_200_and_list_of_farms() {
    // Arrange
    let app = spawn_app().await;
    let client = reqwest::Client::new();

    // Insert test data
    sqlx::query!(r#" INSERT INTO farms (id, name, address, canton, coordinates, categories, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
                Uuid::new_v4(),
                "Test Farm 1",
                "Address 1, 5401 Baden",
                "Aargau",
                "F8G5+J3",
                &vec!["Organic".to_string(), "Fruit".to_string()] as &Vec<String>,
                Utc::now(),
                Option::<DateTime<Utc>>::None
            )
        .execute(&app.db_pool)
        .await
        .expect("Failed to execute query");

    // Act
    let response = client
        .get(&format!("http://{}/farms", &app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    println!("Response status: {}", &response.status());

    // let response_text = response.text().await.expect("Failed to get response body");
    //  println!("Response body: {}", &response_text);

    // Assert
    assert_eq!(200, response.status().as_u16());

    let farms: Vec<Farm> = response
        .json()
        .await
        .expect("Failed to parse response as JSON.");

    assert_eq!(farms.len(), 1);
    // assert_eq!(farms[0].name, "Test Farm 2"); // Most recent first
    // assert_eq!(farms[0].canton, "Zurich");
    assert_eq!(farms[0].name, "Test Farm 1");
    assert_eq!(farms[0].canton, "Aargau");
}

#[tokio::test]
async fn get_farms_returns_empty_list_when_no_farms_exist() {
    // Arrange
    let app = spawn_app().await;
    let client = reqwest::Client::new();

    // Act
    let response = client
        .get(&format!("http://{}/farms", &app.address))
        .send()
        .await
        .expect("Failed to execute request.");

    // Assert
    assert_eq!(200, response.status().as_u16());

    let farms: Vec<Farm> = response
        .json()
        .await
        .expect("Failed to parse response as JSON.");

    assert_eq!(farms.len(), 0);
}
