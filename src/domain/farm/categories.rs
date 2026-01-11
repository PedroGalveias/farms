//!
//! Provides a validated `Categories` type that manages farm classification
//! categories. Ensures categories are non-empty, deduplicated (case-insensitive),
//! and within reasonable limits.

use crate::impl_sqlx_for_vec_string_domain_type;
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Categories(Vec<String>);

#[derive(Debug, Error)]
pub enum CategoriesError {
    #[error("Categories list cannot be empty.")]
    EmptyCategories,

    #[error("Category '{0}' is empty or whitespace.")]
    EmptyCategoryValue(String),

    #[error(
        "Categories '{category}' exceeds maximum length of {max} characters (actual: {actual}."
    )]
    CategoryLengthTooLong {
        category: String,
        max: usize,
        actual: usize,
    },

    #[error("Too many categories: {count}. Maximim allowed is {max}.")]
    TooManyCategories { count: usize, max: usize },

    #[error("Duplicate category: '{0}'.")]
    DuplicateCategory(String),
}

impl Categories {
    const MAX_CATEGORIES: usize = 50;
    const MAX_CATEGORY_NAME_LENGTH: usize = 50;

    /// Parse and validate a list of farm categories
    ///
    /// Rules:
    /// .Cannot be empty
    /// .Each category must be non-empty and <= 50 characters
    /// .Maximum 50 categories
    /// .No duplicates (case-insensitive)
    /// .Trims whitespace from each category
    pub fn parse(categories: Vec<String>) -> Result<Self, CategoriesError> {
        if categories.is_empty() {
            return Err(CategoriesError::EmptyCategories);
        }

        if categories.len() > Self::MAX_CATEGORIES {
            return Err(CategoriesError::TooManyCategories {
                count: categories.len(),
                max: Self::MAX_CATEGORIES,
            });
        }

        let mut validated: Vec<String> = Vec::new();
        let mut already_seen_lowercase: HashSet<String> = HashSet::new();

        for category in categories {
            let trimmed = category.trim().to_string();

            if trimmed.is_empty() {
                return Err(CategoriesError::EmptyCategoryValue(category));
            }

            if trimmed.len() > Self::MAX_CATEGORIES {
                return Err(CategoriesError::CategoryLengthTooLong {
                    category: trimmed.clone(),
                    max: Self::MAX_CATEGORY_NAME_LENGTH,
                    actual: trimmed.len(),
                });
            }

            let lowercase = trimmed.to_lowercase();

            // Tries to insert. If the category already exists, it returns false, otherwise, it returns an Error.
            if !(already_seen_lowercase).insert(lowercase) {
                return Err(CategoriesError::DuplicateCategory(trimmed));
            }

            validated.push(trimmed);
        }

        Ok(Self(validated))
    }

    /// Returns a reference to the categories as a slice.
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    /// Returns a reference to the underlying vector of categories.
    pub fn as_vec(&self) -> &Vec<String> {
        &self.0
    }

    /// Consumes the `Categories` and returns the underlying vector. Useful for APIs that return a vector of categories.
    pub fn into_inner(self) -> Vec<String> {
        self.0
    }

    /// Returns the number of categories.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if there are no categories.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Checks if a category exists in the list (case-insensitive).
    pub fn contains(&self, category: &str) -> bool {
        let lowercased = category.to_lowercase();
        self.0.iter().any(|c| c.to_lowercase() == lowercased)
    }
}

impl PartialEq for Categories {
    fn eq(&self, other: &Self) -> bool {
        if self.0.len() != other.0.len() {
            return false;
        }

        let self_set: HashSet<&String> = self.0.iter().collect();
        let other_set: HashSet<&String> = other.0.iter().collect();

        self_set == other_set
    }
}
impl Eq for Categories {}

impl AsRef<Vec<String>> for Categories {
    fn as_ref(&self) -> &Vec<String> {
        &self.0
    }
}

impl std::fmt::Display for Categories {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.join(", "))
    }
}

// Serialize for JSON API responses
impl serde::Serialize for Categories {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

// Deserialize from JSON API requests
impl<'de> serde::Deserialize<'de> for Categories {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let vec = Vec::<String>::deserialize(deserializer)?;
        Categories::parse(vec).map_err(serde::de::Error::custom)
    }
}

// Implement sqlx traits (Type, Encode, Decode) for PostgreSQL TEXT[] array support.
impl_sqlx_for_vec_string_domain_type!(Categories);

#[cfg(test)]
mod tests {
    use super::Categories;
    use claims::{assert_err, assert_ok};

    #[test]
    fn valid_single_category_is_valid() {
        let category = Categories::parse(vec!["Dairy".to_string()]);
        assert_ok!(category);
    }

    #[test]
    fn multiple_categories_is_valid() {
        let categories = Categories::parse(vec![
            "Dairy".to_string(),
            "Egg".to_string(),
            "Fruit".to_string(),
            "Vegetables".to_string(),
        ]);

        assert_ok!(&categories);
        assert_eq!(categories.unwrap().len(), 4);
    }

    #[test]
    fn empty_categories_vector_is_rejected() {
        let empty_categories = Categories::parse(vec![]);

        assert_err!(empty_categories);
    }

    #[test]
    fn empty_category_is_rejected() {
        let vec_with_empty_category = Categories::parse(vec!["Dairy".to_string(), "".to_string()]);

        assert_err!(vec_with_empty_category);
    }

    #[test]
    fn whitespace_only_category_is_rejected() {
        let vec_with_whitespace_category =
            Categories::parse(vec!["Dairy".to_string(), " ".to_string()]);

        assert_err!(vec_with_whitespace_category);
    }

    #[test]
    fn max_category_name_length_is_valid() {
        let category_with_max_name_length = "k".repeat(Categories::MAX_CATEGORY_NAME_LENGTH);
        let categories = Categories::parse(vec![
            "Dairy".to_string(),
            category_with_max_name_length.to_string(),
        ]);

        assert_ok!(categories);
    }

    #[test]
    fn more_than_max_category_name_length_is_rejected() {
        let category_with_max_name_length = "k".repeat(Categories::MAX_CATEGORY_NAME_LENGTH + 1);
        let categories = Categories::parse(vec![
            "Dairy".to_string(),
            category_with_max_name_length.to_string(),
        ]);

        assert_err!(categories);
    }

    #[test]
    fn max_number_of_categories_is_valid() {
        let max_number_of_categories: Vec<String> = (0..Categories::MAX_CATEGORIES)
            .map(|i| format!("Category{}", i))
            .collect();

        let categories = Categories::parse(max_number_of_categories);

        assert_ok!(categories);
    }

    #[test]
    fn more_than_max_number_of_categories_is_rejected() {
        let max_number_of_categories: Vec<String> = (0..Categories::MAX_CATEGORIES + 1)
            .map(|i| format!("Category{}", i))
            .collect();

        let categories = Categories::parse(max_number_of_categories);

        assert_err!(categories);
    }

    #[test]
    fn duplicate_categories_are_rejected() {
        let categories = Categories::parse(vec![
            "Dairy".to_string(),
            "Egg".to_string(),
            "Egg".to_string(),
            "Vegetables".to_string(),
        ]);

        assert_err!(categories);
    }

    #[test]
    fn duplicate_categories_case_insensitive_are_rejected() {
        let categories = Categories::parse(vec![
            "Dairy".to_string(),
            "Egg".to_string(),
            "EGG".to_string(),
            "Vegetables".to_string(),
        ]);

        assert_err!(categories);
    }

    #[test]
    fn duplicate_categories_mixed_case_insensitive_are_rejected() {
        let categories = Categories::parse(vec![
            "Dairy".to_string(),
            "Egg".to_string(),
            "EGG".to_string(),
            "EgG".to_string(),
            "eGg".to_string(),
            "egG".to_string(),
            "Vegetables".to_string(),
        ]);

        assert_err!(categories);
    }

    #[test]
    fn categories_are_trimmed() {
        let categories = Categories::parse(vec![
            "  Dairy".to_string(),
            "Egg  ".to_string(),
            "  Vegetables   ".to_string(),
        ]);

        assert_ok!(&categories);
        let categories = categories.unwrap();
        assert_eq!(categories.as_slice()[0], "Dairy");
        assert_eq!(categories.as_slice()[1], "Egg");
        assert_eq!(categories.as_slice()[2], "Vegetables");
    }

    #[test]
    fn implements_checks_case_insensitive() {
        let categories = Categories::parse(vec!["Dairy".to_string(), "Egg".to_string()]).unwrap();

        assert!(categories.contains("dairy"));
        assert!(categories.contains("DAIRY"));
        assert!(categories.contains("Dairy"));
        assert!(categories.contains("DaIrY"));

        assert!(categories.contains("egg"));
        assert!(categories.contains("eGg"));
        assert!(categories.contains("egG"));
        assert!(categories.contains("eGG"));
        assert!(categories.contains("EGG"));
    }

    #[test]
    fn displays_formats_correctly() {
        let categories = Categories::parse(vec!["Dairy".to_string(), "Egg".to_string()]).unwrap();

        assert_eq!(categories.to_string(), "Dairy, Egg");
    }

    #[test]
    fn common_farm_items_categories_are_valid() {
        let common_categories = vec![
            "Dairy".to_string(),
            "Organic".to_string(),
            "Vegetables".to_string(),
            "Fruits".to_string(),
            "Livestock".to_string(),
            "Grains".to_string(),
            "Poultry".to_string(),
            "Aquaculture".to_string(),
        ];

        let categories = Categories::parse(common_categories);
        assert_ok!(categories);
    }

    #[test]
    fn unicode_diacritics_in_categories() {
        let unicode_categories = vec![
            "Käse".to_string(),       // ä (German)
            "Gemüse".to_string(),     // ü (German)
            "Légumes".to_string(),    // é (French)
            "Château".to_string(),    // â (French)
            "Gruyère".to_string(),    // è (French)
            "Maraîchage".to_string(), // î (French)
        ];

        let categories = Categories::parse(unicode_categories);
        assert_ok!(categories);
    }

    #[test]
    fn swiss_categories_all_four_languages_are_valid() {
        let swiss_categories = vec![
            // German
            "Käse".to_string(),
            "Milchwirtschaft".to_string(),
            "Obstbau".to_string(),
            "Gemüsebau".to_string(),
            "Alpkäse".to_string(),
            "Berglandwirtschaft".to_string(),
            // French
            "Fromage".to_string(),
            "Viticulture".to_string(),
            "Vignoble".to_string(),
            "Agriculture bio".to_string(),
            "Maraîchage".to_string(),
            "Élevage".to_string(),
            // Italian
            "Formaggio".to_string(),
            "Viticoltura".to_string(),
            "Vigneto".to_string(),
            "Agricoltura".to_string(),
            "Castagne".to_string(),
            // Romansh
            "Chaschiel".to_string(),
            "Látg".to_string(),
            "Agricultura".to_string(),
            "Cultivaziun".to_string(),
            // Popular Swiss Products
            "Emmentaler".to_string(),
            "Gruyère".to_string(),
            "Appenzeller".to_string(),
            "Raclette".to_string(),
        ];

        let categories = Categories::parse(swiss_categories);
        assert_ok!(categories);
    }

    #[test]
    fn len_returns_correct_count() {
        let categories = Categories::parse(vec!["Dairy".to_string(), "Egg".to_string()]).unwrap();

        assert_eq!(categories.len(), 2);
    }

    #[test]
    fn is_empty_returns_false_after_validation() {
        let categories = Categories::parse(vec!["Dairy".to_string(), "Egg".to_string()]).unwrap();

        // Should not be empty after successful validation
        assert!(!categories.is_empty());
    }

    #[test]
    fn as_slice_returns_correct_data() {
        let categories = Categories::parse(vec!["Dairy".to_string(), "Egg".to_string()]).unwrap();

        let slice = categories.as_slice();
        assert_eq!(slice.len(), 2);
        assert_eq!(slice[0], "Dairy");
        assert_eq!(slice[1], "Egg");
    }

    #[test]
    fn into_inner_consumes_and_returns_vec() {
        let categories = Categories::parse(vec!["Dairy".to_string(), "Egg".to_string()]).unwrap();

        let slice = categories.into_inner();
        assert_eq!(slice.len(), 2);
        assert_eq!(slice[0], "Dairy");
        assert_eq!(slice[1], "Egg");
    }

    #[test]
    fn preserves_original_casing() {
        let categories = Categories::parse(vec![
            "DaIrY".to_string(),
            "EGG".to_string(),
            "vegetables".to_string(),
        ])
        .unwrap();

        assert_eq!(categories.as_slice()[0], "DaIrY");
        assert_eq!(categories.as_slice()[1], "EGG");
        assert_eq!(categories.as_slice()[2], "vegetables");
    }
    #[test]
    fn categories_equal_regardless_of_order() {
        let cat1 = Categories::parse(vec!["A".to_string(), "B".to_string()]).unwrap();
        let cat2 = Categories::parse(vec!["B".to_string(), "A".to_string()]).unwrap();
        assert_eq!(cat1, cat2);
    }
}
