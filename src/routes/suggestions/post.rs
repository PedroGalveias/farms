use crate::{
    authentication::CurrentUser,
    configuration::Settings,
    domain::suggestion::SuggestionAction,
    rate_limit::{RateLimitDecision, check_rate_limit},
    routes::suggestions::error::SuggestionError,
    taxonomy::TaxonomySnapshot,
};
use actix_web::{HttpRequest, HttpResponse, web};
use anyhow::Context;
use chrono::Utc;
use deadpool_redis::Pool;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, serde::Deserialize)]
pub struct SuggestionRequest {
    /// Product slug being suggested.
    product: String,
    action: SuggestionAction,
    note: Option<String>,
}

// Actix injects each piece of state as its own argument; this handler needs
// eight, which is normal for a write path that also rate-limits.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(
    name = "Submit product suggestion",
    skip(body, pool, redis_pool, taxonomy, configuration, request)
)]
pub async fn submit_suggestion(
    current_user: CurrentUser,
    path: web::Path<Uuid>,
    body: web::Json<SuggestionRequest>,
    pool: web::Data<PgPool>,
    redis_pool: web::Data<Pool>,
    taxonomy: web::Data<TaxonomySnapshot>,
    configuration: web::Data<Settings>,
    request: HttpRequest,
) -> Result<HttpResponse, SuggestionError> {
    let farm_id = path.into_inner();
    let body = body.into_inner();

    // Resolve the product slug (early 400 on an unknown slug).
    let product_id = taxonomy
        .id_for_slug(body.product.trim())
        .ok_or_else(|| SuggestionError::ValidationError("Unknown product.".to_string()))?;

    // Normalize the note: trim, and store an absent/blank note as NULL so
    // downstream consumers see one representation of "no note" instead of
    // sometimes-empty-string, sometimes-null.
    let note = body
        .note
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_owned);
    if let Some(note) = &note
        && note.chars().count() > 500
    {
        return Err(SuggestionError::ValidationError(
            "Note is too long (max 500 characters).".to_string(),
        ));
    }

    // Rate limit per user and per IP (suggestion spam is the abuse vector).
    enforce_rate_limit(&redis_pool, current_user.id, &request, &configuration).await?;

    // The farm must exist (a fabricated id must 404, not FK-error).
    let farm_exists = sqlx::query!(r#"SELECT id FROM farms WHERE id = $1"#, farm_id)
        .fetch_optional(pool.get_ref())
        .await
        .context("Failed to check farm existence.")?
        .is_some();
    if !farm_exists {
        return Err(SuggestionError::FarmNotFound);
    }

    // A user may have only one PENDING suggestion per (farm, product); a
    // repeat submit is an idempotent no-op rather than queue noise.
    sqlx::query!(
        r#"
        INSERT INTO farm_product_suggestions
            (id, farm_id, product_id, action, note, submitted_by, status, created_at)
        VALUES ($1, $2, $3, $4::suggestion_action, $5, $6, 'PENDING', $7)
        ON CONFLICT (farm_id, product_id, submitted_by) WHERE status = 'PENDING'
        DO NOTHING
        "#,
        Uuid::new_v4(),
        farm_id,
        product_id,
        body.action as SuggestionAction,
        note,
        current_user.id,
        Utc::now(),
    )
    .execute(pool.get_ref())
    .await
    .context("Failed to store the suggestion.")?;

    Ok(HttpResponse::Accepted().finish())
}

/// Fixed-window limit keyed by user and by client IP. Fails open so a Redis
/// blip never blocks a legitimate suggestion.
#[tracing::instrument(
    name = "Enforce suggestion rate limit",
    skip(redis_pool, configuration, request)
)]
async fn enforce_rate_limit(
    redis_pool: &Pool,
    user_id: Uuid,
    request: &HttpRequest,
    configuration: &Settings,
) -> Result<(), SuggestionError> {
    // Reuse the registration rate-limit tunables.
    let limits = &configuration.registration.rate_limit;
    let client_ip = request
        .connection_info()
        .realip_remote_addr()
        .unwrap_or("unknown")
        .to_string();

    let prefix = &limits.key_prefix;
    let keys = [
        format!("{prefix}:suggestion:user:{user_id}"),
        format!("{prefix}:suggestion:ip:{client_ip}"),
    ];

    for key in keys {
        match check_rate_limit(redis_pool, &key, limits.max_requests, limits.window_seconds).await {
            Ok(RateLimitDecision::Allowed) => {}
            Ok(RateLimitDecision::Limited) => return Err(SuggestionError::RateLimited),
            Err(e) => {
                tracing::warn!(
                    error = ?e,
                    "Suggestion rate limit check failed; allowing (fail-open)."
                );
            }
        }
    }

    Ok(())
}
