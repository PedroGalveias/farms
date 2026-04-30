use crate::configuration::{
    DatabaseSettings, RedisSettings, SessionSameSite, SessionSettings, Settings,
};
use crate::routes::{authentication, farms, health_check};
use actix_session::{
    SessionMiddleware,
    config::{CookieContentSecurity, PersistentSession, TtlExtensionPolicy},
    storage::RedisSessionStore,
};
use actix_web::{
    App, HttpServer,
    cookie::{Key, SameSite, time::Duration},
    dev::Server,
    web,
    web::Data,
};
use anyhow::Context;
use deadpool_redis::{Config, Pool, Runtime};
use secrecy::ExposeSecret;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::net::TcpListener;
use tracing_actix_web::TracingLogger;

pub struct Application {
    port: u16,
    server: Server,
}

/// Builds the application
impl Application {
    pub async fn build(configuration: Settings) -> Result<Self, anyhow::Error> {
        let connection_pool = get_connection_pool(&configuration.database);
        let redis_pool = get_redis_connection_pool(&configuration.redis)
            .expect("Failed to create Redis connection pool");

        let address = format!(
            "{}:{}",
            configuration.application.host, configuration.application.port
        );
        let listener = TcpListener::bind(address).expect("Failed to bind port");
        let port = listener.local_addr()?.port();

        let server = run(listener, configuration, connection_pool, redis_pool).await?;

        Ok(Self { port, server })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
        self.server.await
    }
}

pub fn get_connection_pool(configuration: &DatabaseSettings) -> PgPool {
    let max_connections = configuration.max_connections.unwrap_or(10);
    let timeout = configuration.timeout_seconds.unwrap_or(2);

    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(std::time::Duration::from_secs(timeout))
        .connect_lazy_with(configuration.with_db())
}

pub fn get_redis_connection_pool(configuration: &RedisSettings) -> Result<Pool, anyhow::Error> {
    let max_connections = configuration.pool_max_size.unwrap_or(10);
    let config = Config::from_url(configuration.uri.expose_secret());
    let pool = config
        .builder()?
        .max_size(max_connections)
        .runtime(Runtime::Tokio1)
        .build()?;

    Ok(pool)
}

/// Convert the session cookie policy from our application configuration
/// into the `SameSite` type expected by Actix.
fn to_same_site(value: &SessionSameSite) -> SameSite {
    match value {
        SessionSameSite::Lax => SameSite::Lax,
        SessionSameSite::Strict => SameSite::Strict,
        SessionSameSite::None => SameSite::None,
    }
}

/// Validate session-related configuration before the server starts.
///
/// This is a fail safeguard.
fn validate_session_settings(settings: &SessionSettings) -> Result<(), anyhow::Error> {
    if matches!(settings.cookie_same_site, SessionSameSite::None) && !settings.cookie_secure {
        return Err(anyhow::anyhow!(
            "SameSite=None requires cookie_secure=true."
        ));
    }

    Ok(())
}

/// Build the Redis-backed session store used by `SessionMiddleware`.
async fn build_session_store(
    redis_pool: Pool,
    settings: &RedisSettings,
) -> Result<RedisSessionStore, anyhow::Error> {
    let prefix = settings.session_key_prefix.clone();

    RedisSessionStore::builder_pooled(redis_pool)
        .cache_keygen(move |session_key| format!("{prefix}:{session_key}"))
        .build()
        .await
        .context("Failed to create Redis-backed session store.")
}

/// Build the Actix session middleware.
fn build_session_middleware(
    store: RedisSessionStore,
    settings: &SessionSettings,
) -> SessionMiddleware<RedisSessionStore> {
    let secret_key = Key::derive_from(settings.secret_key.expose_secret().as_bytes());

    SessionMiddleware::builder(store, secret_key)
        .cookie_name(settings.cookie_name.clone())
        .cookie_secure(settings.cookie_secure)
        // Prevent JavaScript access to the session cookie.
        .cookie_http_only(settings.cookie_http_only)
        // Control whether browsers send the cookie on cross-site requests.
        .cookie_same_site(to_same_site(&settings.cookie_same_site))
        .cookie_content_security(CookieContentSecurity::Signed)
        .session_lifecycle(
            PersistentSession::default()
                .session_ttl(Duration::seconds(settings.ttl_seconds))
                .session_ttl_extension_policy(TtlExtensionPolicy::OnEveryRequest),
        )
        .build()
}

/// Build and run the Actix HTTP server.
pub async fn run(
    listener: TcpListener,
    configuration: Settings,
    db_pool: PgPool,
    redis_pool: Pool,
) -> Result<Server, anyhow::Error> {
    // Validate session config before booting the app.
    validate_session_settings(&configuration.session)?;

    // Build the Redis-backed session store once at startup.
    let session_store = build_session_store(redis_pool.clone(), &configuration.redis).await?;
    let session_settings = configuration.session.clone();

    // Wrap the connection in a smart pointer
    let db_pool = Data::new(db_pool);
    let redis_pool = Data::new(redis_pool);
    let configuration = Data::new(configuration);

    // Capture the `connection` from the surrounding environment
    let server = HttpServer::new(move || {
        App::new()
            // Middlewares are added using the `wrap` method on `App`
            .wrap(build_session_middleware(
                session_store.clone(),
                &session_settings,
            ))
            .wrap(TracingLogger::default())
            .route("/health_check", web::get().to(health_check))
            .route("/farms", web::post().to(farms::create))
            .route("/farms", web::get().to(farms::get_all))
            .route("/farms/{id}", web::get().to(farms::get_by_id))
            .route("/login", web::post().to(authentication::log_in))
            .route("/logout", web::post().to(authentication::log_out))
            .route("/me", web::get().to(authentication::get_me))
            // Get pointer copy and attach it to the application state
            .app_data(db_pool.clone())
            .app_data(configuration.clone())
            .app_data(redis_pool.clone())
    })
    .listen(listener)?
    .run();

    Ok(server)
}
