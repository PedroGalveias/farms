use crate::idempotency::IdempotencyError;

#[derive(Debug)]
pub struct IdempotencyKey(String);

impl TryFrom<String> for IdempotencyKey {
    type Error = IdempotencyError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let value = value.trim().to_string();

        if value.is_empty() {
            return Err(IdempotencyError::KeyValidation(
                "The idempotency key cannot be empty or whitespace".to_string(),
            ));
        }

        let max_len = 80;
        if value.len() >= max_len {
            return Err(IdempotencyError::KeyValidation(format!(
                "The idempotency key must be shorter than {} characters, found {}",
                max_len,
                value.len()
            )));
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

#[cfg(test)]
mod tests {
    use super::IdempotencyKey;
    use claims::{assert_err, assert_ok};
    use uuid::Uuid;

    #[test]
    fn empty_string_is_rejected() {
        let key = "".to_string();
        assert_err!(IdempotencyKey::try_from(key));
    }

    #[test]
    fn too_long_string_is_rejected() {
        let key = "a".repeat(81).to_string();
        assert_err!(IdempotencyKey::try_from(key));
    }

    #[test]
    fn whitespace_string_is_rejected() {
        let key = "\t ".to_string();
        assert_err!(IdempotencyKey::try_from(key));
    }

    #[test]
    fn valid_key_is_accepted() {
        let key = Uuid::new_v4().to_string();
        assert_ok!(IdempotencyKey::try_from(key));
    }

    #[test]
    fn valid_key_with_prefix_is_accepted() {
        let key = format!("{}:{}", "idem", Uuid::new_v4().to_string());
        assert_ok!(IdempotencyKey::try_from(key));
    }

    #[test]
    fn valid_key_with_prefix_and_user_id_is_accepted() {
        let key = format!(
            "{}:{}:{}",
            "idem",
            Uuid::new_v4().to_string(),
            Uuid::new_v4().to_string()
        );
        assert_ok!(IdempotencyKey::try_from(key));
    }
}
