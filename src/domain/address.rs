use crate::impl_sqlx_for_string_domain_type;
use std::fmt::Display;
use thiserror::Error;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, PartialEq)]
pub struct Address(String);

#[derive(Debug, Error)]
pub enum AddressError {
    #[error("Address cannot be empty.")]
    EmptyAddress,

    #[error("Address is too long (max 200 characters, got {0}).")]
    TooLong(usize),

    #[error("Address is too short (min 5 characters, got {0}).")]
    TooShort(usize),
}

impl Address {
    // Typical format: "Street Number, Postal Code City"
    const MIN_LENGTH: usize = 5;
    const MAX_LENGTH: usize = 200;

    /// Parse an address string into a validated Address
    ///
    /// Rules:
    /// - Cannot be empty or only whitespaces
    /// - Must be between 5 and 200 characters
    /// - Automatically trims whitespace
    /// - Accepts various Swiss address formats:
    ///   * "Street Number, Postal Code City" (most common)
    ///   * "Street Number\nPostal Code City" (multiline)
    ///   * PO Box addresses
    ///   * Addresses with apartment/building details
    pub fn parse(s: String) -> Result<Address, AddressError> {
        let trimmed = s.trim();

        if trimmed.is_empty() {
            return Err(AddressError::EmptyAddress);
        }

        let char_count = trimmed.graphemes(true).count();

        if char_count < Self::MIN_LENGTH {
            return Err(AddressError::TooShort(char_count));
        }

        if char_count > Self::MAX_LENGTH {
            return Err(AddressError::TooLong(char_count));
        }

        Ok(Self(trimmed.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Address {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl serde::Serialize for Address {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Address {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Address::parse(s).map_err(serde::de::Error::custom)
    }
}

impl_sqlx_for_string_domain_type!(Address);

#[cfg(test)]
mod tests {
    use super::Address;
    use crate::domain::test_data::VALID_SWISS_ADDRESSES;
    use claims::{assert_err, assert_ok};

    #[test]
    fn address_with_min_length_is_valid() {
        let address = "A 1 B".to_string(); // 5 characters
        assert_ok!(Address::parse(address));
    }

    #[test]
    fn address_shorter_than_min_length_is_rejected() {
        let address = "A 1".to_string(); // 3 characters
        assert_err!(Address::parse(address));
    }

    #[test]
    fn address_with_max_length_is_valid() {
        let address = "a".repeat(Address::MAX_LENGTH);
        assert_ok!(Address::parse(address));
    }

    #[test]
    fn address_longer_than_max_length_is_rejected() {
        let address = "a".repeat(Address::MAX_LENGTH + 1);
        assert_err!(Address::parse(address));
    }

    #[test]
    fn empty_address_is_rejected() {
        let address = "".to_string();
        assert_err!(Address::parse(address));
    }

    #[test]
    fn whitespace_only_address_is_rejected() {
        let address = "   ".to_string();
        assert_err!(Address::parse(address));
    }

    #[test]
    fn address_with_leading_and_trailing_whitespace_is_trimmed() {
        let address = "  Bahnhofstrasse 1, 8001 Zürich  ".to_string();
        let parsed = Address::parse(address).unwrap();
        assert_eq!(parsed.as_str(), "Bahnhofstrasse 1, 8001 Zürich");
    }

    #[test]
    fn all_valid_swiss_addresses_from_test_data_are_accepted() {
        for address in VALID_SWISS_ADDRESSES {
            assert_ok!(
                Address::parse(address.to_string()),
                "Failed to parse address: {}",
                address
            );
        }
    }

    #[test]
    fn multiline_address_format_is_valid() {
        let address = "Bahnhofstrasse 1\n8001 Zürich".to_string();
        assert_ok!(Address::parse(address));
    }

    #[test]
    fn address_formatting_preserves_content() {
        let original = "Bahnhofstrasse 1, 8001 Zürich";
        let parsed = Address::parse(original.to_string()).unwrap();
        assert_eq!(parsed.to_string(), original);
        assert_eq!(parsed.as_str(), original);
    }
}
