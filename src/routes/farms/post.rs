use crate::{
    configuration::Settings,
    domain::farm::{Address, Canton, Categories, Name, Point},
    idempotency::{IdempotencyError, IdempotencyNextAction, save_response, try_processing},
    routes::farms::FarmError,
};
use actix_web::{HttpResponse, web};
use anyhow::Context;
use chrono::{DateTime, Utc};
use deadpool_redis::Pool;
use sqlx::{Executor, PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct FormData {
    name: String,
    address: String,
    canton: String,
    coordinates: String,
    categories: Vec<String>,
    idempotency_key: String,
}

#[allow(clippy::async_yields_async)]
#[tracing::instrument(
    name = "Adding a new farm",
    skip(body, pool, redis_pool, configuration)
)]
pub async fn create(
    body: web::Json<FormData>,
    pool: web::Data<PgPool>,
    redis_pool: web::Data<Pool>,
    configuration: web::Data<Settings>,
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
    span.record("idempotency_key", body.idempotency_key.as_str());

    let mut transaction = match try_processing(
        &redis_pool,
        &pool,
        body.idempotency_key.as_str(),
        &configuration.idempotency,
    )
    .await
    .map_err(|e| match e {
        IdempotencyError::ExpectedResponseNotFoundError => FarmError::DuplicateRequestConflict(e),
        _ => FarmError::UnexpectedError(e.into()),
    })? {
        IdempotencyNextAction::ReturnSavedResponse(saved_response) => {
            return Ok(saved_response);
        }
        IdempotencyNextAction::StartProcessing(transaction) => transaction,
    };

    insert_farm(
        &mut transaction,
        name,
        address,
        canton,
        coordinates,
        categories,
    )
    .await?;

    let response = HttpResponse::Created().finish();
    let (response, transaction) = save_response(
        &redis_pool,
        transaction,
        body.idempotency_key.as_str(),
        &configuration.idempotency,
        response,
    )
    .await
    .map_err(|e| FarmError::UnexpectedError(e.into()))?;

    transaction
        .commit()
        .await
        .map_err(|e| FarmError::UnexpectedError(e.into()))?;

    Ok(response)
}

#[tracing::instrument(name = "Saving new farm details in the database", skip(transaction))]
pub async fn insert_farm(
    transaction: &mut Transaction<'_, Postgres>,
    name: Name,
    address: Address,
    canton: Canton,
    coordinates: Point,
    categories: Categories,
) -> Result<(), FarmError> {
    let query = sqlx::query!(
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
    );
    transaction
        .execute(query)
        .await
        .context("Failed to insert new farm in the database.")?;

    Ok(())
}
