use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::{PgArgumentBuffer, PgTypeInfo, PgValueRef};
use sqlx::{Decode, Encode, Postgres, Type};

/// Represents a PostgreSQL POINT (longitude, latitude) datatype
/// PostgreSQL POINT stores coordinates as (x, y) which maps to (longitude, latitude)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub longitude: f64,
    pub latitude: f64,
}

impl Point {
    pub fn new(latitude: f64, longitude: f64) -> Self {
        Self {
            latitude,
            longitude,
        }
    }

    /// Parse from "latitude,longitude" string format
    pub fn from_string(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.split(',').collect();

        if parts.len() != 2 {
            return Err("Invalid format. Expected 'latitude,longitude'".to_string());
        }

        let lat = parts[0]
            .trim()
            .parse::<f64>()
            .map_err(|_| "Invalid latitude")?;

        let lon = parts[1]
            .trim()
            .parse::<f64>()
            .map_err(|_| "Invalid longitude")?;

        Ok(Self::new(lat, lon))
    }

    /// Convert to "latitude,longitude" string format (for API responses)
    pub fn to_string_format(&self) -> String {
        format!("{},{}", self.latitude, self.longitude)
    }
}

// SQLx Type implementation for PostgreSQL POINT
impl Type<Postgres> for Point {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("point")
    }
}

// Encode Point to PostgreSQL
impl Encode<'_, Postgres> for Point {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        // PostgreSQL POINT format: (x, y) = (longitude, latitude)
        let point_str = format!("({},{})", self.longitude, self.latitude);
        buf.extend_from_slice(point_str.as_bytes());
        Ok(IsNull::No) // Changed: Now returns Result
    }
}

// Decode Point from PostgreSQL
impl Decode<'_, Postgres> for Point {
    fn decode(value: PgValueRef<'_>) -> Result<Self, BoxDynError> {
        // PostgreSQL returns POINT as "(x,y)" string
        let s = <&str as Decode<Postgres>>::decode(value)?;

        // Remove parentheses and split
        let s = s.trim_matches(|c| c == '(' || c == ')');
        let parts: Vec<&str> = s.split(',').collect();

        if parts.len() != 2 {
            return Err("Invalid POINT format".into());
        }

        let longitude = parts[0].trim().parse::<f64>()?;
        let latitude = parts[1].trim().parse::<f64>()?;

        Ok(Point {
            longitude,
            latitude,
        })
    }
}

// Serialize for JSON API responses
impl serde::Serialize for Point {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Return as "latitude,longitude" string for API compatibility
        serializer.serialize_str(&self.to_string_format())
    }
}

// Deserialize from JSON API requests
impl<'de> serde::Deserialize<'de> for Point {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Point::from_string(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::Point;

    // ========================================
    // Construction Tests
    // ========================================

    #[test]
    fn create_point_with_new() {
        let point = Point::new(47.3769, 8.5417);
        assert_eq!(point.latitude, 47.3769);
        assert_eq!(point.longitude, 8.5417);
    }

    #[test]
    fn point_implements_copy() {
        let point1 = Point::new(47.3769, 8.5417);
        let point2 = point1; // Should copy, not move

        // Both should be usable
        assert_eq!(point1.latitude, 47.3769);
        assert_eq!(point2.latitude, 47.3769);
    }

    // ========================================
    // String Parsing Tests
    // ========================================

    #[test]
    fn parse_valid_coordinates_string() {
        let result = Point::from_string("47.3769,8.5417");
        assert!(result.is_ok());

        let point = result.unwrap();
        assert_eq!(point.latitude, 47.3769);
        assert_eq!(point.longitude, 8.5417);
    }

    #[test]
    fn parse_coordinates_with_spaces() {
        let result = Point::from_string("47.3769, 8.5417");
        assert!(result.is_ok());

        let point = result.unwrap();
        assert_eq!(point.latitude, 47.3769);
        assert_eq!(point.longitude, 8.5417);
    }

    #[test]
    fn parse_coordinates_with_extra_whitespace() {
        let result = Point::from_string("  47.3769  ,  8.5417  ");
        assert!(result.is_ok());

        let point = result.unwrap();
        assert_eq!(point.latitude, 47.3769);
        assert_eq!(point.longitude, 8.5417);
    }

    #[test]
    fn parse_negative_coordinates() {
        let result = Point::from_string("-45.0,170.5");
        assert!(result.is_ok());

        let point = result.unwrap();
        assert_eq!(point.latitude, -45.0);
        assert_eq!(point.longitude, 170.5);
    }

    #[test]
    fn parse_zero_coordinates() {
        let result = Point::from_string("0.0,0.0");
        assert!(result.is_ok());

        let point = result.unwrap();
        assert_eq!(point.latitude, 0.0);
        assert_eq!(point.longitude, 0.0);
    }

    #[test]
    fn parse_invalid_format_single_number() {
        let result = Point::from_string("47.3769");
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "Invalid format. Expected 'latitude,longitude'"
        );
    }

    #[test]
    fn parse_invalid_format_three_numbers() {
        let result = Point::from_string("47.3769,8.5417,100");
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            "Invalid format. Expected 'latitude,longitude'"
        );
    }

    #[test]
    fn parse_invalid_format_empty_string() {
        let result = Point::from_string("");
        assert!(result.is_err());
    }

    #[test]
    fn parse_invalid_latitude_non_numeric() {
        let result = Point::from_string("abc,8.5417");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Invalid latitude");
    }

    #[test]
    fn parse_invalid_longitude_non_numeric() {
        let result = Point::from_string("47.3769,xyz");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Invalid longitude");
    }

    // ========================================
    // String Formatting Tests
    // ========================================

    #[test]
    fn format_point_to_string() {
        let point = Point::new(47.3769, 8.5417);
        assert_eq!(point.to_string_format(), "47.3769,8.5417");
    }

    #[test]
    fn format_negative_coordinates() {
        let point = Point::new(-45.0, 170.5);
        assert_eq!(point.to_string_format(), "-45,170.5");
    }

    #[test]
    fn format_zero_coordinates() {
        let point = Point::new(0.0, 0.0);
        assert_eq!(point.to_string_format(), "0,0");
    }

    #[test]
    fn roundtrip_parse_and_format() {
        let original = "47.3769,8.5417";
        let point = Point::from_string(original).unwrap();
        let formatted = point.to_string_format();
        assert_eq!(formatted, original);
    }

    // ========================================
    // Equality Tests
    // ========================================

    #[test]
    fn points_with_same_coordinates_are_equal() {
        let point1 = Point::new(47.3769, 8.5417);
        let point2 = Point::new(47.3769, 8.5417);
        assert_eq!(point1, point2);
    }

    #[test]
    fn points_with_different_coordinates_are_not_equal() {
        let point1 = Point::new(47.3769, 8.5417);
        let point2 = Point::new(46.9481, 7.4474);
        assert_ne!(point1, point2);
    }

    #[test]
    fn points_with_different_latitude_are_not_equal() {
        let point1 = Point::new(47.3769, 8.5417);
        let point2 = Point::new(47.3770, 8.5417);
        assert_ne!(point1, point2);
    }

    #[test]
    fn points_with_different_longitude_are_not_equal() {
        let point1 = Point::new(47.3769, 8.5417);
        let point2 = Point::new(47.3769, 8.5418);
        assert_ne!(point1, point2);
    }

    // ========================================
    // Swiss Canton Capital Tests
    // ========================================

    #[test]
    fn all_canton_capitals_parse_correctly() {
        use crate::domain::test_data::CANTON_CAPITALS;

        for (city, coords) in CANTON_CAPITALS {
            let result = Point::from_string(coords);
            assert!(
                result.is_ok(),
                "Failed to parse coordinates for {}: {}",
                city,
                coords
            );
        }
    }

    // ========================================
    // Serde JSON Tests
    // ========================================

    #[test]
    fn serialize_point_to_json() {
        let point = Point::new(47.3769, 8.5417);
        let json = serde_json::to_string(&point).unwrap();
        assert_eq!(json, r#""47.3769,8.5417""#);
    }

    #[test]
    fn deserialize_point_from_json() {
        let json = r#""47.3769,8.5417""#;
        let point: Point = serde_json::from_str(json).unwrap();
        assert_eq!(point.latitude, 47.3769);
        assert_eq!(point.longitude, 8.5417);
    }

    #[test]
    fn roundtrip_serde_json() {
        let original = Point::new(47.3769, 8.5417);
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: Point = serde_json::from_str(&json).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn deserialize_point_with_spaces_from_json() {
        let json = r#""47.3769, 8.5417""#;
        let point: Point = serde_json::from_str(json).unwrap();
        assert_eq!(point.latitude, 47.3769);
        assert_eq!(point.longitude, 8.5417);
    }

    #[test]
    fn deserialize_invalid_json_format_fails() {
        let json = r#""not-a-coordinate""#;
        let result: Result<Point, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    // ========================================
    // Debug and Display Tests
    // ========================================

    #[test]
    fn debug_format_shows_structure() {
        let point = Point::new(47.3769, 8.5417);
        let debug_str = format!("{:?}", point);
        assert!(debug_str.contains("Point"));
        assert!(debug_str.contains("47.3769"));
        assert!(debug_str.contains("8.5417"));
    }

    #[test]
    fn clone_creates_independent_copy() {
        let point1 = Point::new(47.3769, 8.5417);
        let point2 = point1.clone();

        assert_eq!(point1, point2);
        assert_eq!(point1.latitude, point2.latitude);
        assert_eq!(point1.longitude, point2.longitude);
    }

    // ========================================
    // Edge Case Tests
    // ========================================

    #[test]
    fn parse_very_large_coordinates() {
        let result = Point::from_string("89.999999,179.999999");
        assert!(result.is_ok());

        let point = result.unwrap();
        assert_eq!(point.latitude, 89.999999);
        assert_eq!(point.longitude, 179.999999);
    }

    #[test]
    fn parse_very_small_coordinates() {
        let result = Point::from_string("-89.999999,-179.999999");
        assert!(result.is_ok());

        let point = result.unwrap();
        assert_eq!(point.latitude, -89.999999);
        assert_eq!(point.longitude, -179.999999);
    }

    #[test]
    fn parse_high_precision_coordinates() {
        let result = Point::from_string("47.123456789,8.987654321");
        assert!(result.is_ok());

        let point = result.unwrap();
        assert_eq!(point.latitude, 47.123456789);
        assert_eq!(point.longitude, 8.987654321);
    }

    #[test]
    fn parse_integer_coordinates() {
        let result = Point::from_string("47,8");
        assert!(result.is_ok());

        let point = result.unwrap();
        assert_eq!(point.latitude, 47.0);
        assert_eq!(point.longitude, 8.0);
    }
}
