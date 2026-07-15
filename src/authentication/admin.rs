use crate::authentication::CurrentUser;
use crate::domain::user::Role;
use actix_web::{FromRequest, HttpRequest, dev::Payload};
use std::future::Future;
use std::pin::Pin;

/// Extractor that only succeeds for ADMIN users; otherwise 403.
///
/// Builds on `CurrentUser` (which authenticates the session) and adds the
/// role gate, so admin-only routes take `AdminUser` instead of re-checking.
#[derive(Debug)]
pub struct AdminUser(pub CurrentUser);

impl FromRequest for AdminUser {
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let current_user = CurrentUser::from_request(req, payload);
        Box::pin(async move {
            let user = current_user.await?;
            if user.role == Role::Admin {
                Ok(AdminUser(user))
            } else {
                Err(actix_web::error::ErrorForbidden("Admin access required."))
            }
        })
    }
}
