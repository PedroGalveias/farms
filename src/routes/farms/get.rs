use crate::{
    domain::farm::{Address, Canton, Categories, Name, Point},
    routes::farms::{Farm, FarmError},
};
use actix_web::{HttpResponse, web};
use anyhow::Context;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct FarmPath {
    id: String,
}

pub async fn get_all(pool: web::Data<PgPool>) -> Result<HttpResponse, FarmError> {
    let farms = get_farms(&pool).await?;

    Ok(HttpResponse::Ok().json(farms))
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

pub async fn get_by_id(
    path: web::Path<FarmPath>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, FarmError> {
    let farm_id = Uuid::parse_str(&path.id)
        .map_err(|_| FarmError::ValidationError("Invalid farm id.".to_string()))?;

    let farm = get_farm_by_id(farm_id, &pool).await?;

    match farm {
        Some(farm) => Ok(HttpResponse::Ok().json(farm)),
        None => Err(FarmError::NotFound),
    }
}

pub async fn get_farm_by_id(farm_id: Uuid, pool: &PgPool) -> Result<Option<Farm>, FarmError> {
    let farm = sqlx::query_as!(
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
        WHERE id = $1
        "#,
        farm_id
    )
    .fetch_optional(pool)
    .await
    .context("Failed to fetch farm from the database.")?;

    Ok(farm)
}
