//! User-chosen password with policy validation.

use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;

pub struct UserPassword(SecretString);

#[derive(Debug, Error)]
pub enum UserPasswordError {
    #[error("Password must be at least 12 characters long.")]
    TooShort,
    #[error("Password must not exceed 1024 bytes.")]
    TooLong,
}

impl UserPassword {
    /// Min 12 chars (length, not composition rules), max 1024 bytes
    /// to bound Argon2 hashing cost. Unicode is allowed.
    pub fn parse(s: SecretString) -> Result<Self, UserPasswordError> {
        let raw = s.expose_secret();

        if raw.chars().count() < 12 {
            return Err(UserPasswordError::TooShort);
        }
        if raw.len() > 1024 {
            return Err(UserPasswordError::TooLong);
        }

        Ok(Self(s))
    }

    /// Consume the wrapper to hand the secret to the hashing code.
    pub fn into_secret(self) -> SecretString {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{UserPassword, UserPasswordError};
    use claims::assert_ok;
    use secrecy::SecretString;

    #[test]
    fn a_twelve_character_password_is_accepted() {
        assert_ok!(UserPassword::parse(SecretString::from("a".repeat(12))));
    }

    #[test]
    fn a_password_shorter_than_twelve_characters_is_rejected() {
        let result = UserPassword::parse(SecretString::from("a".repeat(11)));
        assert!(matches!(result, Err(UserPasswordError::TooShort)));
    }

    #[test]
    fn a_unicode_password_is_accepted() {
        // 16 characters, multi-byte: passes the char-count check.
        assert_ok!(UserPassword::parse(SecretString::from(
            "pāsswörd-café-日本語".to_string()
        )));
    }

    #[test]
    fn a_password_over_the_byte_limit_is_rejected() {
        let result = UserPassword::parse(SecretString::from("a".repeat(1025)));
        assert!(matches!(result, Err(UserPasswordError::TooLong)));
    }

    #[test]
    fn a_long_but_valid_password_is_accepted() {
        assert_ok!(UserPassword::parse(SecretString::from("a".repeat(1024))));
    }
}
