#[derive(Debug, Clone)]
pub struct Canton(String);

#[derive(Debug, thiserror::Error)]
pub enum CantonError {
    #[error("Invalid canton code: {0}. Must be a valid Swiss canton abbreviation (e.g., 'ZH', 'BE', 'LU')."
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

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Canton {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Canton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// Canton domain tests; to be extracted to the tests module after successfully implementation

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_canton_uppercase() {
        assert!(Canton::parse("ZH".to_string()).is_ok());
    }

    #[test]
    fn valid_canton_lowercase() {
        let canton = Canton::parse("ag".to_string());
        assert!(canton.is_ok());
        assert_eq!(canton.unwrap().as_str(), "AG");
    }

    #[test]
    fn invalid_canton_rejected() {
        assert!(Canton::parse("DE".to_string()).is_err());
    }

    #[test]
    fn invalid_canton_empty_string() {
        assert!(Canton::parse("".to_string()).is_err());
    }
}
