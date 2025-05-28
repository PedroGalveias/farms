use actix_web::{web, HttpResponse};
use chrono::Utc;
use sqlx::types::Uuid;
use sqlx::PgPool;
use tracing::Instrument;

#[derive(serde::Deserialize)]
pub struct FormData {
    name: String,
    address: String,
    canton: String,
    coordinates: String,
    categories: String,
}

#[tracing::instrument(
    name = "Creating a new farm",
    skip(form, pool),
    fields(
        create_name = %form.name,
        create_address = %form.address,
        create_canton = %form.canton,
        create_coordinates = %form.coordinates,
        create_categories = %form.categories
    )
)]
pub async fn create(form: web::Form<FormData>, pool: web::Data<PgPool>) -> HttpResponse {
    match insert_farm(&pool, &form).await {
        Ok(_) => HttpResponse::Ok().finish(),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

pub async fn insert_farm(pool: &PgPool, form: &FormData) -> Result<(), sqlx::Error> {
    sqlx::query!(r#"INSERT INTO farms (id, name, address, canton, coordinates, categories, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
        Uuid::new_v4(),
        form.name,
        form.address,
        form.canton,
        form.coordinates,
        form.categories,
        Utc::now(),
    )
        .execute(pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to execute query: {:?}", e);
            e
            // Using the `?` operator to return early
            // if the function failed, returning a sqlx::Error
            // We will talk about error handling in depth later!
        })?;
    Ok(())
}
