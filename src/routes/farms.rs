use actix_web::{web, HttpResponse};
use chrono::{DateTime, Utc};
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

#[allow(clippy::async_yields_async)]
#[tracing::instrument(
    name = "Adding a new farm",
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

#[tracing::instrument(name = "Saving new farm details in the database", skip(form, pool))]
pub async fn insert_farm(pool: &PgPool, form: &FormData) -> Result<(), sqlx::Error> {
    sqlx::query!(r#" INSERT INTO farms (id, name, address, canton, coordinates, categories, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
                Uuid::new_v4(),
                form.name,
                form.address,
                form.canton,
                form.coordinates,
                form.categories,
                Utc::now(),
                Option::<DateTime<Utc>>::None,

            )
                .execute(pool)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to execute query: {:?}", e);
                    e
                })?;
    Ok(())
}
