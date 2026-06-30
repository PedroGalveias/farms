//! User-chosen username with validation and normalisation.

use std::fmt::Display;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Username(String);

#[derive(Debug, Error)]
pub enum UsernameError {
    #[error("Username must be at least 3 characters long.")]
    TooShort,
    #[error("Username must be at most 30 characters long.")]
    TooLong,
    #[error(
        "Username may only contain letters, digits, underscores and hyphens, \
         and must start with a letter or digit."
    )]
    InvalidCharacters,
    #[error("That username is reserved.")]
    Reserved,
}

/// Usernames that must not be claimable by ordinary users (impersonation of
/// system/staff accounts, routing collisions, etc.).
const RESERVED: &[&str] = &[
    "admin",
    "administrator",
    "root",
    "support",
    "help",
    "system",
    "farms",
    "api",
    "moderator",
    "mod",
    "null",
    "undefined",
];

impl Username {
    /// Parse and normalise a user-chosen username.
    ///
    /// Rules: 3-30 characters; ASCII letters, digits, `_` or `-`; must start
    /// with a letter or digit. The value is lowercased so uniqueness is
    /// case-insensitive and a single canonical column suffices (mirroring how
    /// `Email` is stored).
    pub fn parse(s: String) -> Result<Self, UsernameError> {
        let trimmed = s.trim();
        let char_count = trimmed.chars().count();

        if char_count < 3 {
            return Err(UsernameError::TooShort);
        }
        if char_count > 30 {
            return Err(UsernameError::TooLong);
        }

        let first = trimmed.chars().next().expect("length checked above");
        if !first.is_ascii_alphanumeric() {
            return Err(UsernameError::InvalidCharacters);
        }
        if !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(UsernameError::InvalidCharacters);
        }

        let normalised = trimmed.to_lowercase();
        if RESERVED.contains(&normalised.as_str()) {
            return Err(UsernameError::Reserved);
        }

        Ok(Self(normalised))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Username {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for Username {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};

    #[test]
    fn a_valid_username_is_accepted() {
        assert_ok!(Username::parse("pedro_g".to_string()));
        assert_ok!(Username::parse("farm-hand-7".to_string()));
        assert_ok!(Username::parse("abc".to_string()));
    }

    #[test]
    fn username_is_lowercased() {
        let username = Username::parse("PedroG".to_string()).unwrap();
        assert_eq!("pedrog", username.as_str());
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        let username = Username::parse("  pedro  ".to_string()).unwrap();
        assert_eq!("pedro", username.as_str());
    }

    #[test]
    fn too_short_is_rejected() {
        assert_err!(Username::parse("ab".to_string()));
    }

    #[test]
    fn too_long_is_rejected() {
        assert_err!(Username::parse("a".repeat(31)));
    }

    #[test]
    fn invalid_characters_are_rejected() {
        assert_err!(Username::parse("bad name".to_string()));
        assert_err!(Username::parse("user@host".to_string()));
        assert_err!(Username::parse("emoji\u{1F600}".to_string()));
    }

    #[test]
    fn must_start_with_a_letter_or_digit() {
        assert_err!(Username::parse("-leading".to_string()));
        assert_err!(Username::parse("_leading".to_string()));
    }

    #[test]
    fn reserved_usernames_are_rejected_case_insensitively() {
        assert_err!(Username::parse("admin".to_string()));
        assert_err!(Username::parse("ADMIN".to_string()));
        assert_err!(Username::parse("Root".to_string()));
    }
}
