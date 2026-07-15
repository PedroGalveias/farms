use chrono::{DateTime, Utc};
use deadpool_redis::{
    Pool,
    redis::{AsyncTypedCommands, RedisError},
};
use farms::idempotency::{ExpiryOutcome, try_to_execute_task};
use farms::{
    authentication::change_password,
    configuration::{
        DatabaseSettings, EmailClientEngine, IdempotencyEngine, LogFormat, LoggingLevel,
        LoggingSettings, Settings, TelemetrySettings, get_configuration,
    },
    domain::user::Role,
    startup::{Application, get_connection_pool, get_redis_connection_pool},
    telemetry::init_telemetry,
};
use once_cell::sync::Lazy;
use secrecy::SecretString;
use sqlx::{AssertSqlSafe, Connection, PgConnection, PgPool};
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
            (id, username, email, password_hash, role,
             status, email_verified_at, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5::user_role, 'ACTIVE', $6, $6, NULL)
        "#,
            self.id,
            &self.username,
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

    /// Fetch the stored user row by (normalised) email, if any.
    #[allow(dead_code)]
    pub async fn get_user(&self, email: &str) -> Option<StoredUser> {
        sqlx::query_as!(
            StoredUser,
            r#"
            SELECT email, password_hash, status::text as "status!"
            FROM users
            WHERE email = $1
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
            WHERE u.email = $1
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
              AND users.email = $1
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

        // Both parts carry the link; prefer the text part of the first message.
        extract(
            body["Messages"][0]["TextPart"]
                .as_str()
                .expect("Missing TextPart."),
        )
    }

    #[allow(dead_code)]
    pub async fn run_idempotency_cleanup_worker(&self) -> Result<ExpiryOutcome, anyhow::Error> {
        try_to_execute_task(&self.db_pool).await
    }

    #[allow(dead_code)]
    pub async fn create_idempotency_row(
        &self,
        user_id: Uuid,
        idempotency_key: String,
        expire_at: DateTime<Utc>,
    ) {
        let now = Utc::now();
        sqlx::query!(
            r#"
        INSERT INTO idempotency
            (user_id, key, created_at, expire_at)
        VALUES ($1, $2, $3, $4)
        "#,
            user_id,
            idempotency_key.as_str(),
            now,
            expire_at
        )
        .execute(&self.db_pool)
        .await
        .expect("Failed to insert idempotency row.");
    }

    #[allow(dead_code)]
    pub async fn get_idempotency_rows(&self) -> u64 {
        sqlx::query("SELECT * FROM idempotency")
            .execute(&self.db_pool)
            .await
            .expect("Failed to query user.")
            .rows_affected()
    }
}

#[allow(dead_code)]
pub struct StoredUser {
    pub email: String,
    pub password_hash: String,
    pub status: String,
}

// Launch the application in the background
pub async fn spawn_app(idempotency_engine: IdempotencyEngine) -> TestApp {
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
        // Exercise the real HTTP email path against the mock server, regardless
        // of the local default ('log').
        c.email_client.engine = EmailClientEngine::Mailjet;
        c.email_client.base_url = email_server.uri();
        // Isolate this app's rate-limit counters in the shared Valkey so
        // parallel tests don't inflate each other's per-IP register limit.
        c.registration.rate_limit.key_prefix = format!("rltest:{}", Uuid::new_v4());
        c.idempotency.engine = idempotency_engine;

        c
    };
    let setup_pool = configure_database(&configuration.database).await;
    // Seed the taxonomy BEFORE the app boots: the app loads its taxonomy
    // snapshot once at startup, so anything a test needs to resolve by slug
    // (products/categories) must exist first.
    seed_standard_taxonomy(&setup_pool).await;

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

    let create_db_query = format!(r#"CREATE DATABASE "{}";"#, config.database_name);

    sqlx::query(AssertSqlSafe(create_db_query.as_str()))
        .execute(&mut connection)
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

// ---------------------------------------------------------------------------
// Product-taxonomy test fixtures (subcategories feature).
//
// Each test seeds its own tiny taxonomy; populating the real dataset is a
// separate concern. Two groups (fruits, vegetables) with three products.

#[allow(dead_code)]
pub struct TestTaxonomy {
    pub fruits_category_id: i16,
    pub vegetables_category_id: i16,
    pub strawberries_id: i32,
    pub cherries_id: i32,
    pub broccoli_id: i32,
}

/// Insert the standard test taxonomy. Called by `spawn_app` before the app
/// boots so the app's startup snapshot resolves these slugs.
async fn seed_standard_taxonomy(pool: &PgPool) {
    let fruits = insert_test_category(pool, "Früchte", "fruits", 0).await;
    let vegetables = insert_test_category(pool, "Gemüse", "vegetables", 1).await;
    insert_test_product(pool, fruits, "Erdbeeren", "strawberries", "Strawberries").await;
    insert_test_product(pool, fruits, "Kirschen", "cherries", "Cherries").await;
    insert_test_product(pool, vegetables, "Broccoli", "broccoli", "Broccoli").await;
}

/// The ids of the standard taxonomy `spawn_app` already seeded. (Named `seed_*`
/// for readability at call sites; the rows already exist, so this only reads.)
#[allow(dead_code)]
pub async fn seed_test_taxonomy(pool: &PgPool) -> TestTaxonomy {
    let categories = sqlx::query!(r#"SELECT id, slug FROM product_categories"#)
        .fetch_all(pool)
        .await
        .expect("Failed to read test categories.");
    let products = sqlx::query!(r#"SELECT id, slug FROM products"#)
        .fetch_all(pool)
        .await
        .expect("Failed to read test products.");
    let category = |slug: &str| categories.iter().find(|c| c.slug == slug).unwrap().id;
    let product = |slug: &str| products.iter().find(|p| p.slug == slug).unwrap().id;
    TestTaxonomy {
        fruits_category_id: category("fruits"),
        vegetables_category_id: category("vegetables"),
        strawberries_id: product("strawberries"),
        cherries_id: product("cherries"),
        broccoli_id: product("broccoli"),
    }
}

#[allow(dead_code)]
async fn insert_test_category(pool: &PgPool, key_de: &str, slug: &str, display_order: i16) -> i16 {
    sqlx::query!(
        r#"
        INSERT INTO product_categories (key_de, slug, display_order)
        VALUES ($1, $2, $3)
        RETURNING id
        "#,
        key_de,
        slug,
        display_order,
    )
    .fetch_one(pool)
    .await
    .expect("Failed to insert test category.")
    .id
}

#[allow(dead_code)]
async fn insert_test_product(
    pool: &PgPool,
    category_id: i16,
    key_de: &str,
    slug: &str,
    name_en: &str,
) -> i32 {
    sqlx::query!(
        r#"
        INSERT INTO products (category_id, key_de, slug, name_en)
        VALUES ($1, $2, $3, $4)
        RETURNING id
        "#,
        category_id,
        key_de,
        slug,
        name_en,
    )
    .fetch_one(pool)
    .await
    .expect("Failed to insert test product.")
    .id
}

/// Insert a bare farm (no category or product links) and return its id.
#[allow(dead_code)]
pub async fn insert_test_farm(pool: &PgPool, name: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query!(
        r#"
        INSERT INTO farms (id, name, address, canton, coordinates, created_at, updated_at)
        VALUES ($1, $2, 'Somewhere 1', 'ZH', POINT(8.5, 47.4), now(), NULL)
        "#,
        id,
        name,
    )
    .execute(pool)
    .await
    .expect("Failed to insert test farm.");
    id
}

/// Link a farm to a granular product.
#[allow(dead_code)]
pub async fn link_farm_product(pool: &PgPool, farm_id: Uuid, product_id: i32) {
    sqlx::query!(
        r#"INSERT INTO farm_products (farm_id, product_id) VALUES ($1, $2)"#,
        farm_id,
        product_id,
    )
    .execute(pool)
    .await
    .expect("Failed to link farm product.");
}

/// Link a farm to a category group (coarse membership).
#[allow(dead_code)]
pub async fn link_farm_category(pool: &PgPool, farm_id: Uuid, category_id: i16) {
    sqlx::query!(
        r#"INSERT INTO farm_categories (farm_id, category_id) VALUES ($1, $2)"#,
        farm_id,
        category_id,
    )
    .execute(pool)
    .await
    .expect("Failed to link farm category.");
}

impl TestApp {
    /// Create + store an ACTIVE plain user and establish a session. Returns the
    /// user id.
    #[allow(dead_code)]
    pub async fn log_in_active_user(&self) -> Uuid {
        self.log_in_role(Role::User).await
    }

    /// Create + store an ACTIVE admin user and establish a session. Returns the
    /// user id.
    #[allow(dead_code)]
    pub async fn log_in_admin_user(&self) -> Uuid {
        self.log_in_role(Role::Admin).await
    }

    #[allow(dead_code)]
    async fn log_in_role(&self, role: Role) -> Uuid {
        let user = TestUser::generate_with_role(role);
        user.store(&self.db_pool).await;
        let response = self
            .post_login(&serde_json::json!({
                "email": user.email,
                "password": user.password,
            }))
            .await;
        assert_eq!(
            200,
            response.status().as_u16(),
            "Test user login failed to establish a session."
        );
        user.id
    }
}
