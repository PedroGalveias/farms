use crate::{
    domain::farm::{Address, Canton, Categories, Name, Point},
    routes::farms::{Farm, FarmError},
};
use actix_web::{web, HttpResponse};
use anyhow::Context;
use sqlx::PgPool;

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
