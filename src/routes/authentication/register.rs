use crate::authentication::{RegisterUserError, register_user};
use crate::configuration::Settings;
use crate::domain::user::{Email, UserPassword, Username};
use crate::email_client::EmailClient;
use crate::rate_limit::{RateLimitDecision, check_rate_limit};
use crate::routes::authentication::error::RegisterError;
use actix_web::{HttpRequest, HttpResponse, web};
use deadpool_redis::Pool;
use secrecy::SecretString;
use sqlx::PgPool;

#[derive(serde::Deserialize)]
pub struct RegisterRequest {
    username: String,
    email: String,
    password: SecretString,
}

#[tracing::instrument(
    name = "Register a user",
    skip(body, pool, redis_pool, email_client, configuration, request)
)]
pub async fn register(
    body: web::Json<RegisterRequest>,
    pool: web::Data<PgPool>,
    redis_pool: web::Data<Pool>,
    email_client: web::Data<EmailClient>,
    configuration: web::Data<Settings>,
    request: HttpRequest,
) -> Result<HttpResponse, RegisterError> {
    let body = body.into_inner();

    let username = Username::parse(body.username)
        .map_err(|e| RegisterError::ValidationError(e.to_string()))?;
    let email =
        Email::parse(body.email).map_err(|e| RegisterError::ValidationError(e.to_string()))?;
    let password = UserPassword::parse(body.password)
        .map_err(|e| RegisterError::ValidationError(e.to_string()))?;

    enforce_rate_limits(&request, &email, &redis_pool, &configuration).await?;

    match register_user(
        username,
        email,
        password,
        pool.get_ref(),
        email_client.get_ref(),
        &configuration.registration,
    )
    .await
    {
        Ok(()) => {}
        // A username is public, so a clash is reported rather than hidden.
        Err(RegisterUserError::UsernameTaken) => return Err(RegisterError::UsernameTaken),
        // The account exists; only the email could not be delivered. We still
        // return the generic success below so the caller cannot tell whether
        // the address was new. The user stays pending and can request a resend.
        Err(RegisterUserError::EmailDeliveryError(e)) => {
            tracing::error!(
                error = ?e,
                "Registration succeeded but the verification email could not be delivered."
            );
        }
        Err(RegisterUserError::UnexpectedError(e)) => {
            return Err(RegisterError::UnexpectedError(e));
        }
    }

    // Always 202 with an empty body: success and duplicate email are
    // indistinguishable to the caller, which limits account enumeration.
    Ok(HttpResponse::Accepted().finish())
}

/// Apply Valkey-backed fixed-window limits by client IP and by normalised
/// email. Fails open: if Valkey is unavailable we log and allow the request
/// rather than blocking all registrations.
#[tracing::instrument(
    name = "Enforce registration rate limits",
    skip(request, redis_pool, configuration)
)]
async fn enforce_rate_limits(
    request: &HttpRequest,
    email: &Email,
    redis_pool: &Pool,
    configuration: &Settings,
) -> Result<(), RegisterError> {
    let limits = &configuration.registration.rate_limit;

    // Behind a proxy (e.g. Render) this reads `X-Forwarded-For` so we limit the
    // real client rather than the load balancer.
    let client_ip = request
        .connection_info()
        .realip_remote_addr()
        .unwrap_or("unknown")
        .to_string();

    let prefix = &limits.key_prefix;
    let keys = [
        format!("{prefix}:register:ip:{client_ip}"),
        format!("{prefix}:register:email:{}", email.as_str()),
    ];

    for key in keys {
        match check_rate_limit(redis_pool, &key, limits.max_requests, limits.window_seconds).await {
            Ok(RateLimitDecision::Allowed) => {}
            Ok(RateLimitDecision::Limited) => return Err(RegisterError::RateLimited),
            Err(e) => {
                tracing::warn!(error = ?e, "Rate limit check failed; allowing request (fail-open).");
            }
        }
    }

    Ok(())
}
