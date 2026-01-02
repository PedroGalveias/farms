use crate::configuration::{DatabaseSettings, RedisSettings, Settings};
use crate::routes::{farms, health_check};
use actix_web::{dev::Server, web, web::Data, App, HttpServer};
use deadpool_redis::{Config, Pool, Runtime};
use secrecy::ExposeSecret;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::net::TcpListener;
use tracing_actix_web::TracingLogger;

pub struct Application {
    port: u16,
    server: Server,
}
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

pub async fn run(
    listener: TcpListener,
    configuration: Settings,
    db_pool: PgPool,
    redis_pool: Pool,
) -> Result<Server, anyhow::Error> {
    //let redis_store = RedisSessionStore::new_pooled(redis_pool.clone());
    // Wrap the connection in a smart pointer
    let db_pool = Data::new(db_pool);
    let redis_pool = Data::new(redis_pool);
    let configuration = Data::new(configuration);

    // Capture the `connection` from the surrounding environment
    let server = HttpServer::new(move || {
        App::new()
            // Middlewares are added using the `wrap` method on `App`
            //.wrap(SessionMiddleware::new(
            //    redis_store.clone(),
            //    secret_key.clone()
            //))
            .wrap(TracingLogger::default())
            .route("/health_check", web::get().to(health_check))
            .route("/farms", web::post().to(farms::create))
            .route("/farms", web::get().to(farms::get_all))
            // Get pointer copy and attach it to the application state
            .app_data(db_pool.clone())
            .app_data(configuration.clone())
            .app_data(redis_pool.clone())
    })
    .listen(listener)?
    .run();

    Ok(server)
}
