use actix_web::{web, HttpResponse};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct FormData {
    name: String,
    address: String,
    canton: String,
    coordinates: String,
    //#[serde(default)]
    categories: Vec<String>,
}

#[derive(serde::Deserialize, serde::Serialize, sqlx::FromRow)]
pub struct Farm {
    pub id: Uuid,
    pub name: String,
    pub address: String,
    pub canton: String,
    pub coordinates: String,
    pub categories: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[allow(clippy::async_yields_async)]
#[tracing::instrument(name = "Adding a new farm", skip(body, pool))]
pub async fn create(body: web::Json<FormData>, pool: web::Data<PgPool>) -> HttpResponse {
    // Record form fields in the tracing span
    let span = tracing::Span::current();
    span.record("create_name", body.name.as_str());
    span.record("create_address", body.address.as_str());
    span.record("create_canton", body.canton.as_str());
    span.record("create_coordinates", body.coordinates.as_str());
    span.record("create_categories", tracing::field::debug(&body.categories));

    match insert_farm(&pool, &body).await {
        Ok(_) => HttpResponse::Ok().finish(),
        Err(e) => {
            tracing::error!("Failed to insert farm: {:?}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

pub async fn farms(pool: web::Data<PgPool>) -> HttpResponse {
    match get_farms(&pool).await {
        Ok(farms) => HttpResponse::Ok().json(farms),
        Err(e) => {
            tracing::error!("Failed to fetch farms: {:?}", e);

            #[cfg(debug_assertions)]
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to fetch farms",
                "details": e.to_string() // Only in debug builds
            }));

            #[cfg(not(debug_assertions))]
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to fetch farms"
            }))
        }
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
                &form.categories,
                Utc::now(),
                Option::<DateTime<Utc>>::None
            )
        .execute(pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to execute query: {:?}", e);
            e
        })?;
    Ok(())
}

#[tracing::instrument(name = "Get all farms", skip(pool))]
pub async fn get_farms(pool: &PgPool) -> Result<Vec<Farm>, sqlx::Error> {
    let farms = sqlx::query_as!(
        Farm,
        r#"
        SELECT
            id,
            name,
            address,
            canton,
            coordinates,
            categories as "categories: Vec<String>",
            created_at,
            updated_at
        FROM farms
        ORDER BY created_at DESC
        "#
    )
    .fetch_all(pool)
    .await?;

    Ok(farms)
}
