use crate::idempotency::IdempotencyError;

#[derive(Debug)]
pub struct IdempotencyKey(String);

impl TryFrom<String> for IdempotencyKey {
    type Error = IdempotencyError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(IdempotencyError::KeyValidation(
                "The idempotency key cannot be empty or whitespace".to_string(),
            ));
        }

        let max_len = 50;
        if value.len() >= max_len {
            return Err(IdempotencyError::KeyValidation(
                "The idempotency key must be shorter than {max_len} characters".to_string(),
            ));
        }

        Ok(Self(value))
    }
}
impl From<IdempotencyKey> for String {
    fn from(key: IdempotencyKey) -> Self {
        key.0
    }
}
impl AsRef<str> for IdempotencyKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
