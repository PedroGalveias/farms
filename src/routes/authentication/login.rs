use crate::authentication::{AuthenticatedUser, TypedSession, validate_credentials};
use crate::domain::user::Role;
use crate::routes::authentication::error::LoginError;
use actix_session::Session;
use actix_web::{HttpResponse, web};
use secrecy::SecretString;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct LoginRequest {
    email: String,
    password: SecretString,
}

#[derive(serde::Serialize)]
pub struct LoginResponse {
    user_id: Uuid,
    role: Role,
}

impl From<AuthenticatedUser> for LoginResponse {
    fn from(value: AuthenticatedUser) -> Self {
        Self {
            user_id: value.id,
            role: value.role,
        }
    }
}

#[tracing::instrument(name = "Log in a user", skip(body, pool, session))]
pub async fn log_in(
    body: web::Json<LoginRequest>,
    pool: web::Data<PgPool>,
    session: Session,
) -> Result<HttpResponse, LoginError> {
    let authenticated_user =
        validate_credentials(&body.email, body.password.clone(), pool.get_ref()).await?;

    let session = TypedSession::from(session);

    // renew() helps defend against session fixation on successful login.
    session.renew();

    session
        .insert_user_id(authenticated_user.id)
        .map_err(LoginError::UnexpectedError)?;

    Ok(HttpResponse::Ok().json(LoginResponse::from(authenticated_user)))
}
