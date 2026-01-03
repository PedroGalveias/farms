use crate::{
    domain::farm::{Address, Canton, Categories, Name, Point},
    routes::farms::FarmError,
};
use actix_web::{web, HttpResponse};
use anyhow::Context;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct FormData {
    name: String,
    address: String,
    canton: String,
    coordinates: String,
    categories: Vec<String>,
}

#[allow(clippy::async_yields_async)]
#[tracing::instrument(name = "Adding a new farm", skip(body, pool))]
pub async fn create(
    body: web::Json<FormData>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, FarmError> {
    // Validate farm's name
    let name =
        Name::parse(body.name.clone()).map_err(|e| FarmError::ValidationError(e.to_string()))?;

    let address = Address::parse(body.address.clone())
        .map_err(|e| FarmError::ValidationError(e.to_string()))?;

    // Validate farm's canton
    let canton = Canton::parse(body.canton.clone())
        .map_err(|e| FarmError::ValidationError(e.to_string()))?;

    // Validate farm's coordinates
    let coordinates =
        Point::parse(&body.coordinates).map_err(|e| FarmError::ValidationError(e.to_string()))?;

    // Validate farm's categories
    let categories = Categories::parse(body.categories.clone())
        .map_err(|e| FarmError::ValidationError(e.to_string()))?;

    // Record form fields in the tracing span
    let span = tracing::Span::current();
    span.record("create_name", name.as_str());
    span.record("create_address", address.as_str());
    span.record("create_canton", canton.as_str());
    span.record("create_coordinates", coordinates.as_str());
    span.record(
        "create_categories",
        tracing::field::debug(&categories.as_vec()),
    );

    insert_farm(&pool, name, address, canton, coordinates, categories).await?;

    Ok(HttpResponse::Ok().finish())
}

#[tracing::instrument(name = "Saving new farm details in the database", skip(pool))]
pub async fn insert_farm(
    pool: &PgPool,
    name: Name,
    address: Address,
    canton: Canton,
    coordinates: Point,
    categories: Categories,
) -> Result<(), FarmError> {
    sqlx::query!(
        r#"
            INSERT INTO farms (
                 id, name, address, canton, coordinates, categories, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
        Uuid::new_v4(),
        &name as &Name,
        &address as &Address,
        &canton as &Canton,
        &coordinates as &Point,
        &categories as &Categories,
        Utc::now(),
        Option::<DateTime<Utc>>::None
    )
    .execute(pool)
    .await
    .context("Failed to insert new farm in the database.")?;

    Ok(())
}
