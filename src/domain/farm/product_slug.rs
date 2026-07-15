//! A validated product slug supplied by API clients (search filters, create
//! requests). Shape-only validation; whether the slug *exists* is checked
//! against the taxonomy by the caller.

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProductSlug(String);

#[derive(Debug, Error)]
pub enum ProductSlugError {
    #[error("Product slug cannot be empty.")]
    Empty,
    #[error("Product slug is too long (max 64 characters).")]
    TooLong,
    #[error("Product slug may only contain lowercase letters, digits and hyphens.")]
    InvalidCharacters,
}

impl ProductSlug {
    pub fn parse(s: String) -> Result<Self, ProductSlugError> {
        let trimmed = s.trim().to_lowercase();

        if trimmed.is_empty() {
            return Err(ProductSlugError::Empty);
        }
        if trimmed.len() > 64 {
            return Err(ProductSlugError::TooLong);
        }
        if !trimmed
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(ProductSlugError::InvalidCharacters);
        }

        Ok(Self(trimmed))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ProductSlug {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};

    #[test]
    fn a_valid_slug_is_accepted() {
        assert_ok!(ProductSlug::parse("strawberries".to_string()));
        assert_ok!(ProductSlug::parse("stone-fruits".to_string()));
        assert_ok!(ProductSlug::parse("cheese2".to_string()));
    }

    #[test]
    fn it_is_lowercased_and_trimmed() {
        let slug = ProductSlug::parse("  Strawberries  ".to_string()).unwrap();
        assert_eq!("strawberries", slug.as_str());
    }

    #[test]
    fn empty_is_rejected() {
        assert_err!(ProductSlug::parse("   ".to_string()));
    }

    #[test]
    fn invalid_characters_are_rejected() {
        assert_err!(ProductSlug::parse("straw berries".to_string()));
        assert_err!(ProductSlug::parse("straw_berries".to_string()));
        assert_err!(ProductSlug::parse("straw!".to_string()));
    }

    #[test]
    fn overly_long_is_rejected() {
        assert_err!(ProductSlug::parse("a".repeat(65)));
    }
}
