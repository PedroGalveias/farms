use chrono::Utc;
use deadpool_redis::{
    Pool,
    redis::{AsyncTypedCommands, RedisError},
};
use farms::authentication::change_password;
use farms::{
    configuration::{
        DatabaseSettings, LogFormat, LoggingLevel, LoggingSettings, Settings, TelemetrySettings,
        get_configuration,
    },
    domain::user::Role,
    startup::{Application, get_connection_pool, get_redis_connection_pool},
    telemetry::init_telemetry,
};
use once_cell::sync::Lazy;
use secrecy::SecretString;
use sqlx::{Connection, Executor, PgConnection, PgPool};
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

pub struct TestUser {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub password: String,
    pub role: Role,
}

impl TestUser {
    pub fn generate_user() -> Self {
        Self::generate_with_role(Role::User)
    }

    pub fn generate_admin() -> Self {
        Self::generate_with_role(Role::Admin)
    }

    pub fn generate_with_role(role: Role) -> Self {
        let id = Uuid::new_v4();
        let username = format!("user-{}", id);
        let email = format!("{}@example.com", id);
        let password = format!("password-{}", Uuid::new_v4());

        Self {
            id,
            username,
            email,
            password,
            role,
        }
    }

    pub fn password_secret(&self) -> SecretString {
        SecretString::from(self.password.clone())
    }

    pub async fn store(&self, pool: &PgPool) {
        let now = Utc::now();
        sqlx::query!(
            r#"
        INSERT INTO users
            (id, username, email, email_normalised, password_hash, role,
             status, email_verified_at, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6::user_role, 'ACTIVE', $7, $7, NULL)
        "#,
            self.id,
            &self.username,
            &self.email,
            self.email.trim().to_lowercase(),
            "placeholder-hash",
            self.role as Role,
            now,
        )
        .execute(pool)
        .await
        .expect("Failed to insert test user.");

        change_password(self.id, self.password_secret(), pool)
            .await
            .expect("Failed to set test user password.");
    }
}

// Ensure that the `tracing` stack is only initialised once using `once_cell`
static TRACING: Lazy<()> = Lazy::new(|| {
    let logging = LoggingSettings {
        level: LoggingLevel::Debug,
        format: LogFormat::Pretty,
    };
    let telemetry = TelemetrySettings {
        enabled: false,
        service_name: "farms-tests".to_string(),
        endpoint: "".to_string(),
        environment: "test".to_string(),
    };

    // We cannot assign the output of `get_subscriber` to a variable based on the
    // value TEST_LOG` because the sink is part of the type returned by
    // `get_subscriber`, therefore they are not the same type. We could work around
    // it, but this is the most straight-forward way of moving forward.
    if std::env::var("TEST_LOG").is_ok() {
        init_telemetry(logging, telemetry, std::io::stdout).expect("Failed to init logging");
    } else {
        init_telemetry(logging, telemetry, std::io::sink).expect("Failed to init logging");
    };
});

pub struct TestApp {
    #[allow(dead_code)]
    pub address: String,
    pub db_pool: PgPool,
    #[allow(dead_code)]
    pub redis_pool: Pool,
    #[allow(dead_code)]
    pub configuration: Settings,
    #[allow(dead_code)]
    pub api_client: reqwest::Client,
    #[allow(dead_code)]
    pub email_server: wiremock::MockServer,
}
impl TestApp {
    #[allow(dead_code)]
    pub async fn get_farms(&self) -> reqwest::Response {
        self.api_client
            .get(format!("{}/farms", self.address))
            .send()
            .await
            .expect("Failed to execute request.")
    }

    #[allow(dead_code)]
    pub async fn get_farm(&self, farm_id: Uuid) -> reqwest::Response {
        self.get_farm_by_raw_id(&farm_id.to_string()).await
    }

    #[allow(dead_code)]
    pub async fn get_farm_by_raw_id(&self, farm_id: &str) -> reqwest::Response {
        self.api_client
            .get(format!("{}/farms/{}", self.address, farm_id))
            .send()
            .await
            .expect("Failed to execute request.")
    }

    #[allow(dead_code)]
    pub async fn post_farm(&self, body: &serde_json::Value) -> reqwest::Response {
        self.api_client
            .post(format!("{}/farms", &self.address))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .expect("Failed to execute request.")
    }

    #[allow(dead_code)]
    pub async fn post_login(&self, body: &serde_json::Value) -> reqwest::Response {
        self.api_client
            .post(format!("{}/login", self.address))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .expect("Failed to execute request.")
    }

    #[allow(dead_code)]
    pub async fn post_logout(&self) -> reqwest::Response {
        self.api_client
            .post(format!("{}/logout", &self.address))
            .send()
            .await
            .expect("Failed to execute request.")
    }

    #[allow(dead_code)]
    pub async fn get_me(&self) -> reqwest::Response {
        self.api_client
            .get(format!("{}/me", &self.address))
            .send()
            .await
            .expect("Failed to execute request.")
    }

    #[allow(dead_code)]
    pub async fn post_register(&self, body: &serde_json::Value) -> reqwest::Response {
        self.api_client
            .post(format!("{}/register", &self.address))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .expect("Failed to execute request.")
    }

    #[allow(dead_code)]
    pub async fn post_verify_email(&self, body: &serde_json::Value) -> reqwest::Response {
        self.api_client
            .post(format!("{}/verify-email", &self.address))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .expect("Failed to execute request.")
    }

    /// Fetch the stored user row by normalised email, if any.
    #[allow(dead_code)]
    pub async fn get_user(&self, email: &str) -> Option<StoredUser> {
        sqlx::query_as!(
            StoredUser,
            r#"
            SELECT email, email_normalised, password_hash, status::text as "status!"
            FROM users
            WHERE email_normalised = $1
            "#,
            email.trim().to_lowercase(),
        )
        .fetch_optional(&self.db_pool)
        .await
        .expect("Failed to query user.")
    }

    /// Fetch the (hashed) verification token row for a user by email, if any.
    #[allow(dead_code)]
    pub async fn get_verification_token_hash(&self, email: &str) -> Option<String> {
        sqlx::query!(
            r#"
            SELECT t.token_hash
            FROM email_verification_tokens t
            JOIN users u ON u.id = t.user_id
            WHERE u.email_normalised = $1
            ORDER BY t.created_at DESC
            LIMIT 1
            "#,
            email.trim().to_lowercase(),
        )
        .fetch_optional(&self.db_pool)
        .await
        .expect("Failed to query verification token.")
        .map(|r| r.token_hash)
    }

    /// Force-expire every verification token for a user (used by tests).
    #[allow(dead_code)]
    pub async fn expire_verification_tokens(&self, email: &str) {
        sqlx::query!(
            r#"
            UPDATE email_verification_tokens
            SET expires_at = now() - interval '1 hour'
            FROM users
            WHERE email_verification_tokens.user_id = users.id
              AND users.email_normalised = $1
            "#,
            email.trim().to_lowercase(),
        )
        .execute(&self.db_pool)
        .await
        .expect("Failed to expire verification tokens.");
    }

    /// Extract the raw verification token from the last email captured by the
    /// mock email server. The token only ever exists in the email body and the
    /// user's inbox - never in the database (which stores only its hash).
    #[allow(dead_code)]
    pub async fn verification_token_from_email(&self) -> String {
        let requests = self
            .email_server
            .received_requests()
            .await
            .expect("Mock email server captured no requests.");
        let last = requests.last().expect("No email was sent.");
        let body: serde_json::Value =
            serde_json::from_slice(&last.body).expect("Email body was not valid JSON.");

        let extract = |raw: &str| -> String {
            raw.split("token=")
                .nth(1)
                .expect("No token in email body.")
                .split_whitespace()
                .next()
                .expect("Empty token in email body.")
                .trim_end_matches(['"', '<', '>'])
                .to_string()
        };

        // Both bodies carry the link; prefer the text body.
        extract(body["TextBody"].as_str().expect("Missing TextBody."))
    }
}

#[allow(dead_code)]
pub struct StoredUser {
    pub email: String,
    pub email_normalised: String,
    pub password_hash: String,
    pub status: String,
}

// Launch the application in the background
pub async fn spawn_app() -> TestApp {
    // The first time `initialize` is invoked the code in `TRACING` is executed.
    // All other invocations will instead skip execution.
    Lazy::force(&TRACING);

    let email_server = wiremock::MockServer::start().await;

    let configuration = {
        let mut c = get_configuration().expect("Failed to read configuration.");
        // Unique DB name for a fresh DB for each test
        c.database.database_name = Uuid::new_v4().to_string();
        c.application.host = "127.0.0.1".to_string();
        // Random available port
        c.application.port = 0;
        c.email_client.base_url = email_server.uri();
        // Isolate this app's rate-limit counters in the shared Valkey so
        // parallel tests don't inflate each other's per-IP register limit.
        c.registration.rate_limit.key_prefix = format!("rltest:{}", Uuid::new_v4());

        c
    };
    configure_database(&configuration.database).await;

    let application = Application::build(configuration.clone())
        .await
        .expect("Failed to build application.");
    let application_port = application.port();

    // Launch the server as a background task
    // tokio::spawn returns a handle to the spawned future,
    // but we have no use for it here, hence the `drop()` usage.
    drop(tokio::spawn(application.run_until_stopped()));

    let api_client = reqwest::Client::builder()
        .cookie_store(true)
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
        email_server,
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

#[allow(dead_code)]
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
