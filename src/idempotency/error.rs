use crate::{errors::error_chain_fmt, idempotency::persistence::IdempotencyPersistenceError};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistence_error_debug_forwards_to_the_inner_error() {
        // `#[error(transparent)]` makes the outer Display/source delegate
        // entirely to the inner error, so this must read exactly like the
        // inner `IdempotencyPersistenceError`'s own Debug output.
        let err = IdempotencyError::PersistenceError(
            IdempotencyPersistenceError::ExpectedResponseNotFoundError,
        );

        let output = format!("{err:?}");

        assert_eq!(
            output,
            "We expected a saved response, we didn't find it\n\n"
        );
    }

    #[test]
    fn key_validation_debug_prints_just_the_message() {
        let err = IdempotencyError::KeyValidation("must be a valid UUID".to_string());

        let output = format!("{err:?}");

        assert_eq!(
            output,
            "Failed to validate Idempotency Key: must be a valid UUID\n\n"
        );
    }

    #[test]
    fn expected_response_not_found_error_debug_prints_just_the_message() {
        let err = IdempotencyError::ExpectedResponseNotFoundError;

        let output = format!("{err:?}");

        assert_eq!(
            output,
            "We expected a saved response, we didn't find it\n\n"
        );
    }

    #[test]
    fn invalid_engine_error_debug_prints_just_the_message() {
        let err = IdempotencyError::InvalidEngineError;

        let output = format!("{err:?}");

        assert_eq!(output, "Selected Idempotency engine is not supported\n\n");
    }

    #[test]
    fn unexpected_error_debug_prints_the_source_chain() {
        let source =
            anyhow::anyhow!("connection refused").context("Failed to fetch idempotency key.");
        let err = IdempotencyError::UnexpectedError(source);

        let output = format!("{err:?}");

        assert!(output.starts_with("Failed to fetch idempotency key.\n"));
        assert!(output.contains("Caused by:\n\tconnection refused"));
    }
}
