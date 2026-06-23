//! User email type with validation and normalisation.

use std::fmt::Display;
use thiserror::Error;
use validator::ValidateEmail;

#[derive(Debug, Clone, PartialEq)]
pub struct Email(String);

#[derive(Debug, Error)]
pub enum EmailError {
    #[error("Email cannot be empty.")]
    Empty,
    #[error("Email is too long (max 254 bytes, got {0}).")]
    TooLong(usize),
    #[error("Email is not a valid email address.")]
    Invalid,
}

impl Email {
    /// Parse an email string into a validated `Email`.
    ///
    /// Trims surrounding whitespace, rejects empty/oversized/invalid values.
    pub fn parse(s: String) -> Result<Email, EmailError> {
        let trimmed = s.trim();

        if trimmed.is_empty() {
            return Err(EmailError::Empty);
        }
        if trimmed.len() > 254 {
            return Err(EmailError::TooLong(trimmed.len()));
        }
        if !trimmed.validate_email() {
            return Err(EmailError::Invalid);
        }

        Ok(Self(trimmed.to_string()))
    }

    /// The canonical form used for uniqueness checks and lookups.
    /// This is THE one place email normalisation is defined.
    pub fn normalise(raw: &str) -> String {
        raw.trim().to_lowercase()
    }

    pub fn normalised(&self) -> String {
        Self::normalise(&self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Email {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for Email {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::Email;
    use claims::{assert_err, assert_ok};

    #[test]
    fn a_valid_email_is_parsed_successfully() {
        assert_ok!(Email::parse("person@example.com".to_string()));
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        let email = Email::parse("  person@example.com  ".to_string()).unwrap();
        assert_eq!("person@example.com", email.as_str());
    }

    #[test]
    fn empty_email_is_rejected() {
        assert_err!(Email::parse("".to_string()));
        assert_err!(Email::parse("   ".to_string()));
    }

    #[test]
    fn email_missing_at_symbol_is_rejected() {
        assert_err!(Email::parse("person.example.com".to_string()));
    }

    #[test]
    fn email_missing_subject_is_rejected() {
        assert_err!(Email::parse("@example.com".to_string()));
    }

    #[test]
    fn overly_long_email_is_rejected() {
        let local = "a".repeat(250);
        assert_err!(Email::parse(format!("{local}@example.com")));
    }

    #[test]
    fn normalise_lowercases_and_trims() {
        assert_eq!(
            "person@example.com",
            Email::normalise("  Person@Example.COM  ")
        );
    }

    #[test]
    fn normalised_matches_normalise() {
        let email = Email::parse("Person@Example.COM".to_string()).unwrap();
        assert_eq!("person@example.com", email.normalised());
    }
}
