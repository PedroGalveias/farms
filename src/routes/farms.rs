use actix_web::{web, HttpResponse};
use chrono::Utc;
use sqlx::types::Uuid;
use sqlx::PgPool;

#[derive(serde::Deserialize)]
pub struct FormData {
    name: String,
    address: String,
    canton: String,
    coordinates: String,
    categories: String,
}

pub async fn create(form: web::Form<FormData>, pool: web::Data<PgPool>) -> HttpResponse {
    let request_id = uuid::Uuid::new_v4();
    tracing::info!(
        "request_id {} - Adding '{}' '{}' '{}' '{}' '{}' as a new farm.",
        request_id,
        form.name,
        form.address,
        form.canton,
        form.coordinates,
        form.categories,
    );
    tracing::info!(
        "request_id {} - Saving new farm details in the database",
        request_id
    );
    match sqlx::query!(r"INSERT INTO farms (id, name, address, canton, coordinates, categories, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        request_id,
        form.name,
        form.address,
        form.canton,
        form.coordinates,
        form.categories,
        Utc::now(),
    )   // We use `get_ref` to get an immutable reference to the `PgConnection`
        // wrapped by `web::Data`.
        .execute(pool.get_ref())
        .await {
        Ok(_) => {
            tracing::info!("requestId {} - New farm details have been saved", request_id);
            HttpResponse::Ok().finish()
        },
        Err(e) => {
            tracing::error!("request_id {} - Failed to execute query: {:?}", request_id, e);
            HttpResponse::InternalServerError().finish()
        }
    }
}
