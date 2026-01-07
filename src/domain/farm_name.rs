use crate::impl_sqlx_for_string_domain_type;
use std::fmt::Display;
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, PartialEq)]
pub struct FarmName(String);

#[derive(Debug, Error)]
pub enum FarmNameError {
    #[error("Farm name cannot be empty.")]
    EmptyName,

    #[error("Farm name is too long (max 256 characters, got {0}.")]
    TooLong(usize),

    #[error("Farm name contains forbidden characters: {0}")]
    ForbiddenCharacters(String),
}

impl FarmName {
    /// Parse a farm name string into a validated FarmName
    ///
    /// Cannot be empty or only whitespaces
    /// Must be between 1 and 256 characters
    /// No forbidden characters
    /// Automatically trims whitespace
    pub fn parse(s: String) -> Result<FarmName, FarmNameError> {
        let is_empty_or_whitespace = s.trim().is_empty();

        if is_empty_or_whitespace {
            return Err(FarmNameError::EmptyName);
        }

        let char_count = s.graphemes(true).count();
        let is_too_long = char_count > 256;

        if is_too_long {
            return Err(FarmNameError::TooLong(char_count));
        }

        let forbidden_characters = ['/', '(', ')', '"', '<', '>', '\\', '{', '}'];
        if let Some(forbidden) = s.chars().find(|g| forbidden_characters.contains(g)) {
            return Err(FarmNameError::ForbiddenCharacters(format!(
                "'{}'",
                forbidden
            )));
        }

        Ok(Self(s.trim().to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for FarmName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for FarmName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl serde::Serialize for FarmName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for FarmName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        // When deserializing from API responses, trust the data is valid
        // since it came from our own database
        Ok(FarmName(s))
    }
}

impl_sqlx_for_string_domain_type!(FarmName);

#[cfg(test)]
mod tests {
    use super::FarmName;
    use claims::{assert_err, assert_ok};

    #[test]
    fn farm_name_256_characters_long_are_valid() {
        let farm_name = "k".repeat(256);
        assert_ok!(FarmName::parse(farm_name));
    }

    #[test]
    fn farm_name_longer_than_256_characters_are_rejected() {
        let farm_name = "k".repeat(257);
        assert_err!(FarmName::parse(farm_name));
    }

    #[test]
    fn farm_name_whitespace_only_are_rejected() {
        let farm_name = " ".to_string();
        assert_err!(FarmName::parse(farm_name));
    }

    #[test]
    fn farm_name_empty_string_are_rejected() {
        let farm_name = "".to_string();
        assert_err!(FarmName::parse(farm_name));
    }

    #[test]
    fn farm_name_contains_forbidden_characters_are_rejected() {
        for forbidden_char in &['/', '(', ')', '"', '<', '>', '\\', '{', '}'] {
            assert_err!(FarmName::parse(forbidden_char.to_string()));
        }
    }

    #[test]
    fn a_valid_farm_name_is_parsed_successfully() {
        let farm_name = "Ackermatthof 24h Bio Milchautomat".to_string();
        assert_ok!(FarmName::parse(farm_name));
    }

    #[test]
    fn farm_names_with_accents_are_valid() {
        let farm_name = "Hofträumli - Hofladen".to_string();
        assert_ok!(FarmName::parse(farm_name));
    }
}
