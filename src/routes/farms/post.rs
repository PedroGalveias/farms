use crate::{
    authentication::CurrentUser,
    configuration::Settings,
    domain::farm::{Address, Canton, Name, Point, ProductSlug},
    idempotency::{IdempotencyError, IdempotencyNextAction, save_response, try_processing},
    routes::farms::FarmError,
    taxonomy::TaxonomySnapshot,
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
    /// Category (group) slugs the farm belongs to, e.g. ["vegetables"]. Use
    /// when only group-level classification is known (no specific product).
    #[serde(default)]
    categories: Vec<String>,
    /// Product slugs the farm offers, e.g. ["strawberries", "cherries"].
    #[serde(default)]
    products: Vec<String>,
    idempotency_key: String,
}

#[allow(clippy::async_yields_async)]
#[tracing::instrument(
    name = "Adding a new farm",
    skip(body, pool, redis_pool, taxonomy, configuration)
)]
pub async fn create(
    current_user: CurrentUser,
    body: web::Json<FormData>,
    pool: web::Data<PgPool>,
    redis_pool: web::Data<Pool>,
    taxonomy: web::Data<TaxonomySnapshot>,
    configuration: web::Data<Settings>,
) -> Result<HttpResponse, FarmError> {
    let body = body.into_inner();

    // Validate the farm's own fields.
    let name = Name::parse(body.name).map_err(|e| FarmError::ValidationError(e.to_string()))?;
    let address =
        Address::parse(body.address).map_err(|e| FarmError::ValidationError(e.to_string()))?;
    let canton =
        Canton::parse(body.canton).map_err(|e| FarmError::ValidationError(e.to_string()))?;
    let coordinates =
        Point::parse(&body.coordinates).map_err(|e| FarmError::ValidationError(e.to_string()))?;

    // Resolve product slugs (shape via ProductSlug, existence via the snapshot).
    let mut product_ids = Vec::with_capacity(body.products.len());
    for raw in body.products {
        let slug =
            ProductSlug::parse(raw).map_err(|e| FarmError::ValidationError(e.to_string()))?;
        let id = taxonomy.id_for_slug(slug.as_str()).ok_or_else(|| {
            FarmError::ValidationError(format!("Unknown product '{}'.", slug.as_str()))
        })?;
        product_ids.push(id);
    }
    product_ids.sort_unstable();
    product_ids.dedup();

    // Resolve category slugs (ProductSlug validates slug shape for either kind).
    let mut category_ids = Vec::with_capacity(body.categories.len());
    for raw in body.categories {
        let slug =
            ProductSlug::parse(raw).map_err(|e| FarmError::ValidationError(e.to_string()))?;
        let id = taxonomy
            .category_id_for_slug(slug.as_str())
            .ok_or_else(|| {
                FarmError::ValidationError(format!("Unknown category '{}'.", slug.as_str()))
            })?;
        category_ids.push(id);
    }
    category_ids.sort_unstable();
    category_ids.dedup();

    // A farm needs at least one classification — coarse (group) or granular
    // (product). The source data has both kinds, so accept either.
    if category_ids.is_empty() && product_ids.is_empty() {
        return Err(FarmError::ValidationError(
            "At least one category or product is required.".to_string(),
        ));
    }

    // Record form fields in the tracing span.
    let span = tracing::Span::current();
    span.record("create_name", name.as_str());
    span.record("create_address", address.as_str());
    span.record("create_canton", canton.as_str());
    span.record("create_coordinates", coordinates.as_str());
    span.record("idempotency_key", body.idempotency_key.as_str());

    let mut transaction = match try_processing(
        &redis_pool,
        &pool,
        body.idempotency_key.as_str(),
        current_user.id,
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

    let farm_id = insert_farm(&mut transaction, &name, &address, &canton, &coordinates).await?;
    insert_farm_categories(&mut transaction, farm_id, &category_ids).await?;
    insert_farm_products(&mut transaction, farm_id, &product_ids).await?;

    let response = HttpResponse::Created().finish();
    let (response, transaction) = save_response(
        &redis_pool,
        transaction,
        body.idempotency_key.as_str(),
        current_user.id,
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
async fn insert_farm(
    transaction: &mut Transaction<'_, Postgres>,
    name: &Name,
    address: &Address,
    canton: &Canton,
    coordinates: &Point,
) -> Result<Uuid, FarmError> {
    let farm_id = Uuid::new_v4();
    let query = sqlx::query!(
        r#"
        INSERT INTO farms (id, name, address, canton, coordinates, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        farm_id,
        name as &Name,
        address as &Address,
        canton as &Canton,
        coordinates as &Point,
        Utc::now(),
        Option::<DateTime<Utc>>::None,
    );
    transaction
        .execute(query)
        .await
        .context("Failed to insert new farm in the database.")?;

    Ok(farm_id)
}

#[tracing::instrument(name = "Linking farm to categories", skip(transaction))]
async fn insert_farm_categories(
    transaction: &mut Transaction<'_, Postgres>,
    farm_id: Uuid,
    category_ids: &[i16],
) -> Result<(), FarmError> {
    // Single round-trip bulk insert via UNNEST.
    let query = sqlx::query!(
        r#"
        INSERT INTO farm_categories (farm_id, category_id)
        SELECT $1, * FROM UNNEST($2::int2[])
        ON CONFLICT DO NOTHING
        "#,
        farm_id,
        category_ids,
    );
    transaction
        .execute(query)
        .await
        .context("Failed to link farm to categories.")?;

    Ok(())
}

#[tracing::instrument(name = "Linking farm to products", skip(transaction))]
async fn insert_farm_products(
    transaction: &mut Transaction<'_, Postgres>,
    farm_id: Uuid,
    product_ids: &[i32],
) -> Result<(), FarmError> {
    // Single round-trip bulk insert via UNNEST.
    let query = sqlx::query!(
        r#"
        INSERT INTO farm_products (farm_id, product_id)
        SELECT $1, * FROM UNNEST($2::int[])
        ON CONFLICT DO NOTHING
        "#,
        farm_id,
        product_ids,
    );
    transaction
        .execute(query)
        .await
        .context("Failed to link farm to products.")?;

    Ok(())
}
