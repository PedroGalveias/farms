use crate::{
    authentication::AdminUser, domain::suggestion::SuggestionAction,
    routes::admin::error::AdminError,
};
use actix_web::{HttpResponse, web};
use anyhow::Context;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, serde::Deserialize)]
pub struct ListQuery {
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    50
}

#[derive(serde::Serialize)]
pub struct SuggestionView {
    id: Uuid,
    farm_id: Uuid,
    product_slug: String,
    action: SuggestionAction,
    note: Option<String>,
    submitted_by: Uuid,
    created_at: DateTime<Utc>,
}

/// GET /admin/product-suggestions — the pending moderation queue.
#[tracing::instrument(name = "List pending suggestions", skip(pool))]
pub async fn list_pending(
    _admin: AdminUser,
    query: web::Query<ListQuery>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, AdminError> {
    let limit = query.limit.clamp(1, 200);

    let rows = sqlx::query!(
        r#"
        SELECT
            s.id,
            s.farm_id,
            p.slug        AS "product_slug!",
            s.action      AS "action: SuggestionAction",
            s.note,
            s.submitted_by,
            s.created_at
        FROM farm_product_suggestions s
        JOIN products p ON p.id = s.product_id
        WHERE s.status = 'PENDING'
        ORDER BY s.created_at DESC, s.id DESC
        LIMIT $1
        "#,
        limit,
    )
    .fetch_all(pool.get_ref())
    .await
    .context("Failed to list pending suggestions.")?;

    let views: Vec<SuggestionView> = rows
        .into_iter()
        .map(|r| SuggestionView {
            id: r.id,
            farm_id: r.farm_id,
            product_slug: r.product_slug,
            action: r.action,
            note: r.note,
            submitted_by: r.submitted_by,
            created_at: r.created_at,
        })
        .collect();

    Ok(HttpResponse::Ok().json(views))
}

/// POST /admin/product-suggestions/{id}/approve — claim the row and apply it.
#[tracing::instrument(name = "Approve suggestion", skip(pool))]
pub async fn approve(
    admin: AdminUser,
    path: web::Path<Uuid>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, AdminError> {
    let id = path.into_inner();
    let mut transaction = pool
        .begin()
        .await
        .context("Failed to acquire a Postgres connection from the pool.")?;

    // Atomic state transition: only a PENDING row can be claimed. Concurrent
    // approvers race here and exactly one wins.
    let claimed = sqlx::query!(
        r#"
        UPDATE farm_product_suggestions
        SET status = 'APPROVED', reviewed_by = $1, reviewed_at = $2
        WHERE id = $3 AND status = 'PENDING'
        RETURNING farm_id, product_id, action AS "action: SuggestionAction"
        "#,
        admin.0.id,
        Utc::now(),
        id,
    )
    .fetch_optional(&mut *transaction)
    .await
    .context("Failed to claim the suggestion.")?;

    let Some(claimed) = claimed else {
        // Already reviewed, or does not exist.
        return Err(AdminError::Conflict);
    };

    match claimed.action {
        SuggestionAction::Add => {
            // Applying an ADD also refreshes availability: the product is
            // (re)confirmed available as of now.
            sqlx::query!(
                r#"
                INSERT INTO farm_products (farm_id, product_id, last_confirmed_at)
                VALUES ($1, $2, $3)
                ON CONFLICT (farm_id, product_id)
                DO UPDATE SET last_confirmed_at = EXCLUDED.last_confirmed_at,
                              status = 'AVAILABLE'
                "#,
                claimed.farm_id,
                claimed.product_id,
                Utc::now(),
            )
            .execute(&mut *transaction)
            .await
            .context("Failed to apply ADD.")?;
        }
        SuggestionAction::Remove => {
            sqlx::query!(
                r#"DELETE FROM farm_products WHERE farm_id = $1 AND product_id = $2"#,
                claimed.farm_id,
                claimed.product_id,
            )
            .execute(&mut *transaction)
            .await
            .context("Failed to apply REMOVE.")?;
        }
    }

    transaction
        .commit()
        .await
        .context("Failed to commit approval.")?;

    Ok(HttpResponse::Ok().finish())
}

/// POST /admin/product-suggestions/{id}/reject — same claim, no apply step.
#[tracing::instrument(name = "Reject suggestion", skip(pool))]
pub async fn reject(
    admin: AdminUser,
    path: web::Path<Uuid>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, AdminError> {
    let id = path.into_inner();

    let claimed = sqlx::query!(
        r#"
        UPDATE farm_product_suggestions
        SET status = 'REJECTED', reviewed_by = $1, reviewed_at = $2
        WHERE id = $3 AND status = 'PENDING'
        RETURNING id
        "#,
        admin.0.id,
        Utc::now(),
        id,
    )
    .fetch_optional(pool.get_ref())
    .await
    .context("Failed to reject the suggestion.")?;

    if claimed.is_none() {
        return Err(AdminError::Conflict);
    }

    Ok(HttpResponse::Ok().finish())
}
