use crate::errors::error_chain_fmt;
use std::fmt::Formatter;

#[derive(thiserror::Error)]
pub enum IdempotencyPersistenceError {
    #[error("Failed to acquire a connection from redis pool")]
    RedisPool(#[from] deadpool_redis::PoolError),
    #[error("Failed to run a command on redis")]
    Redis(#[from] deadpool_redis::redis::RedisError),
    #[error("Failed to decode Idempotency payload")]
    Decoding(#[from] rmp_serde::decode::Error),
    #[error("Failed to encode Idempotency payload")]
    Encoding(#[from] rmp_serde::encode::Error),
    #[error("Failed to connect to database")]
    SqlError(#[from] sqlx::Error),
    #[error("We expected a saved response, we didn't find it")]
    ExpectedResponseNotFoundError,
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}
impl std::fmt::Debug for IdempotencyPersistenceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redis_pool_debug_prints_the_source_chain() {
        let err = IdempotencyPersistenceError::RedisPool(deadpool_redis::PoolError::Closed);

        let output = format!("{err:?}");

        assert!(output.starts_with("Failed to acquire a connection from redis pool\n"));
        assert!(output.contains("Caused by:\n\tPool has been closed"));
    }

    #[test]
    fn redis_debug_prints_the_source_chain() {
        let source = deadpool_redis::redis::RedisError::from((
            deadpool_redis::redis::ErrorKind::IoError,
            "connection reset",
        ));
        let err = IdempotencyPersistenceError::Redis(source);

        let output = format!("{err:?}");

        assert!(output.starts_with("Failed to run a command on redis\n"));
        assert!(output.contains("Caused by:\n\tconnection reset"));
    }

    #[test]
    fn decoding_debug_prints_the_source_chain() {
        let source = rmp_serde::decode::Error::Uncategorized("bad payload".to_string());
        let err = IdempotencyPersistenceError::Decoding(source);

        let output = format!("{err:?}");

        assert!(output.starts_with("Failed to decode Idempotency payload\n"));
        assert!(output.contains("Caused by:\n\tuncategorized error: bad payload"));
    }

    #[test]
    fn encoding_debug_prints_the_source_chain() {
        let source = rmp_serde::encode::Error::UnknownLength;
        let err = IdempotencyPersistenceError::Encoding(source);

        let output = format!("{err:?}");

        assert!(output.starts_with("Failed to encode Idempotency payload\n"));
        assert!(output.contains(
            "Caused by:\n\tattempt to serialize struct, sequence or map with unknown length"
        ));
    }

    #[test]
    fn sql_error_debug_prints_the_source_chain() {
        let err = IdempotencyPersistenceError::SqlError(sqlx::Error::RowNotFound);

        let output = format!("{err:?}");

        assert!(output.starts_with("Failed to connect to database\n"));
        assert!(output.contains(
            "Caused by:\n\tno rows returned by a query that expected to return at least one row"
        ));
    }

    #[test]
    fn expected_response_not_found_error_debug_prints_just_the_message() {
        let err = IdempotencyPersistenceError::ExpectedResponseNotFoundError;

        let output = format!("{err:?}");

        assert_eq!(
            output,
            "We expected a saved response, we didn't find it\n\n"
        );
    }

    #[test]
    fn unexpected_error_debug_prints_the_source_chain() {
        let source = anyhow::anyhow!("connection refused").context("Failed to run redis command.");
        let err = IdempotencyPersistenceError::UnexpectedError(source);

        let output = format!("{err:?}");

        assert!(output.starts_with("Failed to run redis command.\n"));
        assert!(output.contains("Caused by:\n\tconnection refused"));
    }
}
