use actix_session::Session;
use anyhow::Context;
use uuid::Uuid;

const USER_ID_SESSION_KEY: &str = "user_id";

#[derive(Clone)]
pub struct TypedSession(Session);

impl TypedSession {
    pub fn renew(&self) {
        self.0.renew();
    }

    pub fn insert_user_id(&self, user_id: Uuid) -> Result<(), anyhow::Error> {
        self.0
            .insert(USER_ID_SESSION_KEY, user_id)
            .context("Failed to insert user id into session.")
    }

    pub fn get_user_id(&self) -> Result<Option<Uuid>, anyhow::Error> {
        self.0
            .get(USER_ID_SESSION_KEY)
            .context("Failed to get user id from session.")
    }

    pub fn log_out(&self) {
        self.0.purge();
    }
}

impl From<Session> for TypedSession {
    fn from(session: Session) -> Self {
        Self(session)
    }
}
