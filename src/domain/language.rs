//! Supported API languages.
//!
//! The API separates *stable keys* from *display labels*: filtering always uses
//! language-independent slugs, while the language selected here only decides
//! which human-readable label is returned. That split is what lets a caller ask
//! for `?category=vegetables&lang=de` and get German text back without the
//! filter itself becoming language-dependent.
//!
//! Switzerland has four national languages, and the platform additionally
//! serves English, so five codes are supported.

use std::fmt::Display;
use std::str::FromStr;

/// A language the API can return display labels in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// English — also the fallback when a translation is missing.
    #[default]
    En,
    /// German (Deutsch).
    De,
    /// French (français).
    Fr,
    /// Italian (italiano).
    It,
    /// Romansh (rumantsch).
    Rm,
}

#[derive(Debug, thiserror::Error)]
pub enum LanguageError {
    // Generated from `ALL` rather than spelled out, so the message cannot drift
    // from the set actually accepted. A hardcoded list would still compile, and
    // still read plausibly, after a language was added.
    #[error(
        "Unsupported language '{0}'. Supported languages are: {codes}.",
        codes = Language::ALL.map(|language| language.code()).join(", ")
    )]
    Unsupported(String),
}

impl Language {
    /// Every supported language, in a stable order suitable for documentation
    /// and for building an "available languages" response.
    pub const ALL: [Language; 5] = [
        Language::En,
        Language::De,
        Language::Fr,
        Language::It,
        Language::Rm,
    ];

    /// The two-letter code, as accepted by `?lang=` and returned in responses.
    pub fn code(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::De => "de",
            Language::Fr => "fr",
            Language::It => "it",
            Language::Rm => "rm",
        }
    }

    /// Parse a language tag.
    ///
    /// Accepts a bare code (`de`) or a regional tag (`de-CH`, `de_CH`), because
    /// browsers and mobile clients routinely send the regional form and every
    /// Swiss regional variant maps onto the same label set we hold. Matching is
    /// case-insensitive: `DE`, `de` and `de-CH` are the same request.
    pub fn parse(value: &str) -> Result<Self, LanguageError> {
        let trimmed = value.trim();
        // Take the primary subtag: "de-CH" and "de_CH" both mean German here.
        let primary = trimmed
            .split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();

        match primary.as_str() {
            "en" => Ok(Language::En),
            "de" => Ok(Language::De),
            "fr" => Ok(Language::Fr),
            "it" => Ok(Language::It),
            "rm" => Ok(Language::Rm),
            // Report the caller's original input, not the normalised primary
            // subtag, so the message names what they actually sent.
            _ => Err(LanguageError::Unsupported(trimmed.to_string())),
        }
    }

    /// Resolve an optional `?lang=` value, defaulting when it is absent.
    ///
    /// An absent language is not an error — it selects the default. An
    /// unsupported one is, matching how unknown category and product slugs are
    /// rejected rather than silently ignored: a caller who asked for something
    /// specific should hear that they did not get it.
    pub fn from_query(value: Option<&str>) -> Result<Self, LanguageError> {
        match value {
            None => Ok(Language::default()),
            // `?lang=` with no value reads as "unset" rather than as a typo.
            Some(raw) if raw.trim().is_empty() => Ok(Language::default()),
            Some(raw) => Language::parse(raw),
        }
    }
}

impl Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code())
    }
}

impl FromStr for Language {
    type Err = LanguageError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Language::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_supported_code() {
        for language in Language::ALL {
            assert_eq!(Language::parse(language.code()).unwrap(), language);
        }
    }

    #[test]
    fn parsing_is_case_insensitive() {
        assert_eq!(Language::parse("DE").unwrap(), Language::De);
        assert_eq!(Language::parse("Rm").unwrap(), Language::Rm);
    }

    #[test]
    fn accepts_regional_tags() {
        // Swiss clients commonly send these; they all want the same labels.
        for tag in ["de-CH", "de_CH", "fr-CH", "it-CH", "rm-CH", "en-GB"] {
            assert!(Language::parse(tag).is_ok(), "expected {tag} to parse");
        }
        assert_eq!(Language::parse("de-CH").unwrap(), Language::De);
    }

    #[test]
    fn ignores_surrounding_whitespace() {
        assert_eq!(Language::parse("  fr  ").unwrap(), Language::Fr);
    }

    #[test]
    fn rejects_an_unsupported_language() {
        let error = Language::parse("es").unwrap_err();
        // The message must name the offending input and list what is allowed,
        // so a caller can fix the request without reading the source.
        let message = error.to_string();
        assert!(message.contains("es"), "got: {message}");
        // Checked against ALL rather than a frozen "en, de, fr, it, rm": a
        // literal here would keep passing after a sixth language was added,
        // while the message quietly told callers it was unsupported.
        for language in Language::ALL {
            assert!(
                message.contains(language.code()),
                "{language} is supported but missing from: {message}"
            );
        }
    }

    #[test]
    fn reports_the_original_input_not_the_primary_subtag() {
        let message = Language::parse("es-AR").unwrap_err().to_string();
        assert!(message.contains("es-AR"), "got: {message}");
    }

    #[test]
    fn rejects_nonsense_that_merely_starts_with_a_valid_code() {
        // "den" must not be accepted as German: only a subtag boundary counts.
        assert!(Language::parse("den").is_err());
        assert!(Language::parse("english").is_err());
    }

    #[test]
    fn english_is_the_default() {
        assert_eq!(Language::default(), Language::En);
        assert_eq!(Language::from_query(None).unwrap(), Language::En);
    }

    #[test]
    fn an_empty_lang_parameter_selects_the_default() {
        assert_eq!(Language::from_query(Some("")).unwrap(), Language::En);
        assert_eq!(Language::from_query(Some("   ")).unwrap(), Language::En);
    }

    #[test]
    fn from_query_rejects_an_unsupported_value() {
        assert!(Language::from_query(Some("es")).is_err());
    }

    #[test]
    fn serializes_as_its_lowercase_code() {
        let json = serde_json::to_string(&Language::De).unwrap();
        assert_eq!(json, "\"de\"");
    }

    #[test]
    fn displays_as_its_code() {
        assert_eq!(Language::De.to_string(), "de");
        assert_eq!(Language::Rm.to_string(), "rm");
    }

    #[test]
    fn from_str_matches_parse() {
        let via_from_str: Language = "it".parse().unwrap();
        assert_eq!(via_from_str, Language::parse("it").unwrap());
    }
}
