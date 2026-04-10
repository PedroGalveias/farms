use crate::authentication::TypedSession;
use actix_session::Session;
use actix_web::HttpResponse;

#[tracing::instrument(name = "Log out a user", skip(session))]
pub async fn log_out(session: Session) -> HttpResponse {
    TypedSession::from(session).log_out();

    HttpResponse::Ok().finish()
}
