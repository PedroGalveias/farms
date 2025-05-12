use actix_web::dev::Server;
use actix_web::{web, App, HttpResponse, HttpServer};
use sqlx::PgPool;
use std::net::TcpListener;

pub mod configuration;
pub mod routes;
pub mod startup;

async fn health_check() -> HttpResponse {
    HttpResponse::Ok().finish()
}

async fn create_farm() -> HttpResponse {
    HttpResponse::Ok().finish()
}

pub fn run(listener: TcpListener, db_pool: PgPool) -> Result<Server, std::io::Error> {
    // Wrap the pool using web::Data, which boils down to an Arc smart pointer
    let db_pool = web::Data::new(db_pool);

    let server = HttpServer::new(move || {
        App::new()
            .route("/health_check", web::get().to(health_check))
            .route("/farm", web::post().to(create_farm))
            .app_data(db_pool.clone())
    })
    .listen(listener)?
    .run();

    // No .await here
    Ok(server)
}
