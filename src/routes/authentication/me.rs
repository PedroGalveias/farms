use crate::authentication::CurrentUser;
use crate::domain::user::Role;
use actix_web::{HttpResponse, error, web};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(serde::Serialize)]
pub struct MeResponse {
    user_id: Uuid,
    username: String,
    role: Role,
}

#[tracing::instrument(name = "Get current user", skip(pool))]
pub async fn get_me(
    current_user: CurrentUser,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, actix_web::Error> {
    // The session only carries id + role; the display name (username) lives in
    // the users table, so the frontend can show it instead of the role.
    let row = sqlx::query!(
        r#"SELECT username FROM users WHERE id = $1"#,
        current_user.id
    )
    .fetch_optional(pool.get_ref())
    .await
    .map_err(|e| {
        tracing::error!(error = ?e, "Failed to load the current user.");
        error::ErrorInternalServerError("Failed to load the current user.")
    })?;

    match row {
        Some(row) => Ok(HttpResponse::Ok().json(MeResponse {
            user_id: current_user.id,
            username: row.username,
            role: current_user.role,
        })),
        // Session valid but the user row is gone (deleted) — treat as unauth.
        None => Err(error::ErrorUnauthorized("User no longer exists.")),
    }
}
