use crate::errors::error_chain_fmt;
use crate::idempotency::persistence::IdempotencyPersistenceError;

#[derive(thiserror::Error)]
pub enum IdempotencyError {
    #[error(transparent)]
    PersistenceError(#[from] IdempotencyPersistenceError),
    #[error("Failed to validate Idempotency Key: {0}")]
    KeyValidation(String),
    #[error("We expected a saved response, we didn't find it")]
    ExpectedResponseNotFoundError,
    #[error("Selected Idempotency engine is not supported")]
    InvalidEngineError,
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}
impl std::fmt::Debug for IdempotencyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
