use crate::authentication::CurrentUser;
use crate::domain::user::Role;
use actix_web::HttpResponse;
use uuid::Uuid;

#[derive(serde::Serialize)]
pub struct MeResponse {
    user_id: Uuid,
    role: Role,
}

pub async fn get_me(current_user: CurrentUser) -> HttpResponse {
    HttpResponse::Ok().json(MeResponse {
        user_id: current_user.id,
        role: current_user.role,
    })
}
