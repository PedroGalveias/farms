use crate::domain::{Address, Canton, Categories, Name, Point};
use crate::errors::error_chain_fmt;
use actix_web::{http::StatusCode, web, HttpResponse, ResponseError};
use anyhow::Context;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::fmt::Formatter;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct FormData {
    name: String,
    address: String,
    canton: String,
    coordinates: String,
    categories: Vec<String>,
}

#[derive(serde::Deserialize, serde::Serialize, sqlx::FromRow)]
pub struct Farm {
    pub id: Uuid,
    pub name: Name,
    pub address: Address,
    pub canton: Canton,
    pub coordinates: Point,
    pub categories: Categories,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
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

pub async fn farms(pool: web::Data<PgPool>) -> Result<HttpResponse, FarmError> {
    let farms = get_farms(&pool).await?;

    Ok(HttpResponse::Ok().json(farms))
}

#[derive(thiserror::Error)]
pub enum FarmError {
    // `error` Implements the Display for this enum variant
    #[error("{0}")]
    ValidationError(String),
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
    // `from` derives an implementation of From for the type
    // this field is also used as error `source`. this denotes what should be returned as root cause
}
impl ResponseError for FarmError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::ValidationError(_) => StatusCode::BAD_REQUEST,
            Self::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
impl std::fmt::Debug for FarmError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
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

#[tracing::instrument(name = "Get all farms", skip(pool))]
pub async fn get_farms(pool: &PgPool) -> Result<Vec<Farm>, FarmError> {
    let farms = sqlx::query_as!(
        Farm,
        r#"
        SELECT
            id,
            name as "name: Name",
            address as "address: Address",
            canton as "canton: Canton",
            coordinates as "coordinates: Point",
            categories as "categories: Categories",
            created_at,
            updated_at
        FROM farms
        ORDER BY created_at DESC
        "#
    )
    .fetch_all(pool)
    .await
    .context("Failed to fetch farms from the database.")?;
    // context method converts the error returned into anyhow::Error
    //  and enriches it with additional context around the intentions of the caller/

    Ok(farms)
}
