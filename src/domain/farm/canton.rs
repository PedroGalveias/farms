//! Swiss canton validation and type safety.
//!
//! Provides a validated `Canton` type that ensures only official Swiss canton
//! abbreviations are accepted.

use crate::impl_sqlx_for_string_domain_type;
use std::fmt::Display;

#[derive(Debug, Clone, PartialEq)]
pub struct Canton(String);

#[derive(Debug, thiserror::Error)]
pub enum CantonError {
    #[error(
        "Invalid canton code: {0}. Must be a valid Swiss canton abbreviation (e.g., 'ZH', 'BE', 'LU')."
    )]
    InvalidCanton(String),

    #[error("Canton code cannot be empty.")]
    EmptyCanton,
}

impl Canton {
    const VALID_CANTONS: [&'static str; 26] = [
        "AG", "AI", "AR", "BE", "BL", "BS", "FR", "GE", "GL", "GR", "JU", "LU", "NE", "NW", "OW",
        "SG", "SH", "SO", "SZ", "TG", "TI", "UR", "VD", "VS", "ZG", "ZH",
    ];

    pub fn parse(s: String) -> Result<Self, CantonError> {
        let trimmed = s.trim();

        if trimmed.is_empty() {
            return Err(CantonError::EmptyCanton);
        }

        let uppercased = trimmed.to_uppercase();

        if Self::VALID_CANTONS.contains(&uppercased.as_str()) {
            Ok(Canton(uppercased))
        } else {
            Err(CantonError::InvalidCanton(s))
        }
    }

    /// Returns the address as a string slice. Useful for logging and display.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Canton {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for Canton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// Serialize for JSON API responses
impl serde::Serialize for Canton {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

// Deserialize from JSON API requests
impl<'de> serde::Deserialize<'de> for Canton {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Canton::parse(s).map_err(serde::de::Error::custom)
    }
}

impl_sqlx_for_string_domain_type!(Canton);

#[cfg(test)]
mod tests {
    use super::Canton;
    use claims::{assert_err, assert_ok};

    #[test]
    fn valid_canton_uppercase() {
        let canton = "ZH";
        assert_ok!(Canton::parse(canton.to_string()));
    }

    #[test]
    fn valid_canton_lowercase() {
        let canton = "ag";
        assert_ok!(Canton::parse(canton.to_string()));
    }

    #[test]
    fn invalid_canton_rejected() {
        let canton = "DE";
        assert_err!(Canton::parse(canton.to_string()));
    }

    #[test]
    fn invalid_canton_empty_string() {
        let canton = "";
        assert_err!(Canton::parse(canton.to_string()));
    }

    #[test]
    fn as_ref_returns_the_underlying_str() {
        let canton = Canton::parse("zh".to_string()).unwrap();
        assert_eq!(canton.as_ref(), canton.as_str());
    }

    #[test]
    fn display_shows_the_uppercased_code() {
        let canton = Canton::parse("zh".to_string()).unwrap();
        assert_eq!(canton.to_string(), "ZH");
    }

    #[test]
    fn deserialize_valid_canton_from_json() {
        let json = r#""ZH""#;
        let canton: Canton = serde_json::from_str(json).unwrap();
        assert_eq!(canton.as_str(), "ZH");
    }

    #[test]
    fn deserialize_uppercases_a_lowercase_code() {
        let json = r#""zh""#;
        let canton: Canton = serde_json::from_str(json).unwrap();
        assert_eq!(canton.as_str(), "ZH");
    }

    #[test]
    fn deserialize_empty_canton_fails() {
        let json = r#""""#;
        let result: Result<Canton, _> = serde_json::from_str(json);
        assert_err!(result);
    }

    #[test]
    fn deserialize_invalid_canton_fails() {
        let json = r#""DE""#;
        let result: Result<Canton, _> = serde_json::from_str(json);
        assert_err!(result);
    }

    #[test]
    fn roundtrip_serde_json() {
        let original = Canton::parse("ZH".to_string()).unwrap();
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: Canton = serde_json::from_str(&json).unwrap();
        assert_eq!(original, deserialized);
    }
}
