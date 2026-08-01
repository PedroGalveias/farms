//! Display labels for taxonomy entries, in a requested language.
//!
//! Every taxonomy row carries a canonical German label (`key_de`, which also
//! serves as its natural key) plus optional columns for the other languages.
//! Translations are added over time, so a lookup has to cope with the requested
//! one being absent.

use crate::domain::language::Language;

/// The labels a taxonomy row holds, one slot per supported language.
///
/// German is not optional: it is the canonical label and doubles as the row's
/// natural key, so it is always present and always available as a last resort.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalisedLabels {
    pub de: String,
    pub en: Option<String>,
    pub fr: Option<String>,
    pub it: Option<String>,
    pub rm: Option<String>,
}

impl LocalisedLabels {
    /// The label in `language`, or the closest available substitute.
    ///
    /// Order: the requested language, then English, then German.
    ///
    /// English sits in the middle rather than German because it is the wider
    /// second language of the two for this audience — a French or Italian
    /// speaker without a translation is much more likely to read "Pineapple"
    /// than "Ananas" as intended, and Romansh speakers are all fluent in German
    /// anyway, so nobody is left without something readable.
    ///
    /// The chain never yields an empty string: German is non-optional, so the
    /// caller always gets a real label and never has to invent a placeholder.
    pub fn resolve(&self, language: Language) -> &str {
        let requested = match language {
            Language::De => return &self.de,
            Language::En => self.en.as_deref(),
            Language::Fr => self.fr.as_deref(),
            Language::It => self.it.as_deref(),
            Language::Rm => self.rm.as_deref(),
        };

        requested.or(self.en.as_deref()).unwrap_or(&self.de)
    }

    /// Whether `language` has its own translation, as opposed to being served a
    /// fallback. Lets callers report coverage without re-deriving the chain.
    pub fn has(&self, language: Language) -> bool {
        match language {
            Language::De => true,
            Language::En => self.en.is_some(),
            Language::Fr => self.fr.is_some(),
            Language::It => self.it.is_some(),
            Language::Rm => self.rm.is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> LocalisedLabels {
        LocalisedLabels {
            de: "Gemüse".into(),
            en: Some("Vegetables".into()),
            fr: Some("Légumes".into()),
            it: Some("Verdura".into()),
            rm: Some("Verduras".into()),
        }
    }

    /// German only — the state every product is in until #162 is done.
    fn german_and_english_only() -> LocalisedLabels {
        LocalisedLabels {
            de: "Ananas".into(),
            en: Some("Pineapple".into()),
            ..Default::default()
        }
    }

    #[test]
    fn returns_the_requested_language_when_present() {
        let labels = full();
        assert_eq!(labels.resolve(Language::De), "Gemüse");
        assert_eq!(labels.resolve(Language::En), "Vegetables");
        assert_eq!(labels.resolve(Language::Fr), "Légumes");
        assert_eq!(labels.resolve(Language::It), "Verdura");
        assert_eq!(labels.resolve(Language::Rm), "Verduras");
    }

    #[test]
    fn falls_back_to_english_when_the_translation_is_missing() {
        let labels = german_and_english_only();
        for language in [Language::Fr, Language::It, Language::Rm] {
            assert_eq!(
                labels.resolve(language),
                "Pineapple",
                "{language} should fall back to English"
            );
        }
    }

    #[test]
    fn falls_back_to_german_when_english_is_missing_too() {
        let labels = LocalisedLabels {
            de: "Sonstiges".into(),
            ..Default::default()
        };
        for language in Language::ALL {
            assert_eq!(labels.resolve(language), "Sonstiges", "{language}");
        }
    }

    #[test]
    fn german_never_falls_back() {
        // German is the canonical label, so asking for it must not consult the
        // chain even when every other column is populated.
        let labels = full();
        assert_eq!(labels.resolve(Language::De), "Gemüse");
    }

    #[test]
    fn resolve_never_returns_an_empty_string() {
        // The point of German being non-optional: callers never have to invent
        // a placeholder or render a blank chip.
        let labels = LocalisedLabels {
            de: "Auter".into(),
            ..Default::default()
        };
        for language in Language::ALL {
            assert!(!labels.resolve(language).is_empty());
        }
    }

    #[test]
    fn has_reports_real_translations_not_fallbacks() {
        let labels = german_and_english_only();
        assert!(labels.has(Language::De));
        assert!(labels.has(Language::En));
        // These resolve to English, but they are not translated.
        assert!(!labels.has(Language::Fr));
        assert!(!labels.has(Language::It));
        assert!(!labels.has(Language::Rm));
    }

    #[test]
    fn has_is_always_true_for_german() {
        assert!(
            LocalisedLabels {
                de: "x".into(),
                ..Default::default()
            }
            .has(Language::De)
        );
    }
}
