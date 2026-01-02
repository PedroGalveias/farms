use deadpool_redis::{
    redis::{AsyncTypedCommands, RedisError},
    Pool,
};
use farms::{
    configuration::{get_configuration, DatabaseSettings, Settings},
    startup::{get_connection_pool, get_redis_connection_pool, Application},
    telemetry::{get_subscriber, init_subscriber},
};
use once_cell::sync::Lazy;
use sqlx::{Connection, Executor, PgConnection, PgPool};
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

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
    pub redis_pool: Pool,
    pub configuration: Settings,
    pub api_client: reqwest::Client,
}
impl TestApp {
    pub async fn get_farms(&self) -> reqwest::Response {
        self.api_client
            .get(format!("{}/farms", self.address))
            .send()
            .await
            .expect("Failed to execute request.")
    }

    pub async fn post_farm(&self, body: &serde_json::Value) -> reqwest::Response {
        self.api_client
            .post(&format!("{}/farms", &self.address))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .expect("Failed to execute request.")
    }
}

// Launch the application in the background
pub async fn spawn_app() -> TestApp {
    // The first time `initialize` is invoked the code in `TRACING` is executed.
    // All other invocations will instead skip execution.
    Lazy::force(&TRACING);

    let configuration = {
        let mut c = get_configuration().expect("Failed to read configuration.");
        // Unique DB name for a fresh DB for each test
        c.database.database_name = Uuid::new_v4().to_string();
        c.application.host = "127.0.0.1".to_string();
        // Random available port
        c.application.port = 0;

        c
    };
    configure_database(&configuration.database).await;

    let application = Application::build(configuration.clone())
        .await
        .expect("Failed to build application.");
    let application_port = application.port();

    // Launch the server as a background task
    // tokio::spawn returns a handle to the spawned future,
    // but we have no use for it here, hence the non-binding let
    let _ = tokio::spawn(application.run_until_stopped());

    let api_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    // Return the TestApp struct to the caller!
    TestApp {
        address: format!("http://127.0.0.1:{}", application_port),
        db_pool: get_connection_pool(&configuration.database),
        redis_pool: get_redis_connection_pool(&configuration.redis)
            .expect("Failed to create redis pool"),
        configuration,
        api_client,
    }
}

pub async fn configure_database(config: &DatabaseSettings) -> PgPool {
    // Create database
    let mut connection = PgConnection::connect_with(&config.without_db())
        .await
        .expect("Failed to connect to Postgres.");

    connection
        .execute(format!(r#"CREATE DATABASE "{}";"#, config.database_name).as_str())
        .await
        .expect("Failed to create database.");

    // Migrate database
    let connection_pool = PgPool::connect_with(config.with_db())
        .await
        .expect("Failed to connect to Postgres.");
    sqlx::migrate!("./migrations")
        .run(&connection_pool)
        .await
        .expect("Failed to migrate the database");

    connection_pool
}

pub async fn redis_exists_with_retry(
    connection: &mut deadpool_redis::Connection,
    key: &str,
    max_attempts: u32,
    delay_ms: u64,
) -> Result<bool, RedisError> {
    let mut attempts = 0;

    loop {
        attempts += 1;

        match connection.exists(key).await {
            Ok(exists) => return Ok(exists),
            Err(err) => {
                if attempts >= max_attempts {
                    return Err(err);
                }

                sleep(Duration::from_millis(delay_ms)).await;
            }
        }
    }
}
