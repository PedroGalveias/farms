use crate::errors::error_chain_fmt;

#[derive(thiserror::Error)]
pub enum IdempotencyError {
    #[error("Failed to acquire a connection from redis pool")]
    RedisPool(#[from] deadpool_redis::PoolError),
    #[error("Failed to run a command on redis")]
    Redis(#[from] deadpool_redis::redis::RedisError),
    #[error("Failed to decode Idempotency payload")]
    Decoding(#[from] rmp_serde::decode::Error),
    #[error("Failed to encode Idempotency payload")]
    Encoding(#[from] rmp_serde::encode::Error),
    #[error("Failed to validate Idempotency Key: {0}")]
    KeyValidation(String),
    #[error("We expected a saved response, we didn't find it")]
    ExpectedResponseNotFoundError,
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}
impl std::fmt::Debug for IdempotencyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}
