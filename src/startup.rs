use crate::configuration::{DatabaseSettings, Settings};
use crate::routes::{farms, health_check};
use actix_session::storage::RedisSessionStore;
use actix_web::{dev::Server, web, web::Data, App, HttpServer};
use secrecy::{ExposeSecret, SecretString};
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
        let address = format!(
            "{}:{}",
            configuration.application.host, configuration.application.port
        );
        let listener = TcpListener::bind(address).expect("Failed to bind port");
        let port = listener.local_addr()?.port();

        let server = run(
            listener,
            connection_pool,
            configuration.application.base_url,
            configuration.redis_uri,
        )
        .await?;

        Ok(Self { port, server })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
        self.server.await
    }
}

pub struct ApplicationBaseUrl(pub String);

pub fn get_connection_pool(configuration: &DatabaseSettings) -> PgPool {
    PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect_lazy_with(configuration.with_db())
}

pub async fn run(
    listener: TcpListener,
    db_pool: PgPool,
    base_url: String,
    redis_uri: SecretString,
) -> Result<Server, anyhow::Error> {
    // Wrap the connection in a smart pointer
    let db_pool = Data::new(db_pool);
    let base_url = Data::new(ApplicationBaseUrl(base_url));
    let _redis_store = RedisSessionStore::new(redis_uri.expose_secret()).await?;

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
            .route("/farms", web::get().to(farms::get_all))
            .route("/farms", web::post().to(farms::create))
            // Get pointer copy and attach it to the application state
            .app_data(db_pool.clone())
            .app_data(base_url.clone())
    })
    .listen(listener)?
    .run();

    Ok(server)
}
