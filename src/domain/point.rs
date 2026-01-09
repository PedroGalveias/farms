use sqlx::encode::IsNull;
use sqlx::postgres::types::PgPoint;
use sqlx::postgres::{PgArgumentBuffer, PgTypeInfo, PgValueRef};
use sqlx::{Decode, Encode, Postgres, Type};
use thiserror::Error;

/// Represents a PostgreSQL POINT (longitude, latitude) datatype
/// with validation for Switzerland boundaries
/// PostgreSQL POINT stores coordinates as (x, y) which maps to (longitude, latitude)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub longitude: f64,
    pub latitude: f64,
}

#[derive(Debug, Error)]
pub enum PointError {
    #[error("Invalid coordinate format. Expected 'latitude,longitude' (e.g., '47.3769,8.5417')")]
    InvalidFormat,

    #[error("Invalid latitude: {0}. Must be between -90 and 90")]
    InvalidLatitude(f64),

    #[error("Invalid longitude: {0}. Must be between -180 and 180")]
    InvalidLongitude(f64),

    #[error("Coordinates not within Switzerland boundaries. Latitude: {lat}, Longitude: {lon}")]
    NotInSwitzerland { lat: f64, lon: f64 },
}

impl Point {
    // Switzerland boundaries (approximate)
    const MIN_LATITUDE: f64 = 45.8;
    const MAX_LATITUDE: f64 = 47.9;
    const MIN_LONGITUDE: f64 = 5.9;
    const MAX_LONGITUDE: f64 = 10.6;

    pub fn new(latitude: f64, longitude: f64) -> Self {
        Self {
            latitude,
            longitude,
        }
    }

    /// Check if coordinates are within Switzerland boundaries
    fn is_within_switzerland(lat: f64, lon: f64) -> bool {
        (Self::MIN_LATITUDE..=Self::MAX_LATITUDE).contains(&lat)
            && (Self::MIN_LONGITUDE..=Self::MAX_LONGITUDE).contains(&lon)
    }

    /// Parse from "latitude,longitude" string format with Switzerland validation
    ///
    /// Expected format: "latitude,longitude" (e.g., "47.3769,8.5417")
    /// Validates that coordinates are within Switzerland boundaries
    pub fn parse(s: &str) -> Result<Self, PointError> {
        let parts: Vec<&str> = s.split(',').collect();

        if parts.len() != 2 {
            return Err(PointError::InvalidFormat);
        }

        let lat = parts[0]
            .trim()
            .parse::<f64>()
            .map_err(|_| PointError::InvalidFormat)?;

        let lon = parts[1]
            .trim()
            .parse::<f64>()
            .map_err(|_| PointError::InvalidFormat)?;

        // Validate basic coordinate ranges
        if !(-90.0..=90.0).contains(&lat) {
            return Err(PointError::InvalidLatitude(lat));
        }

        if !(-180.0..=180.0).contains(&lon) {
            return Err(PointError::InvalidLongitude(lon));
        }

        // Validate Switzerland boundaries
        if !Self::is_within_switzerland(lat, lon) {
            return Err(PointError::NotInSwitzerland { lat, lon });
        }

        Ok(Self::new(lat, lon))
    }

    /// Convert to "latitude,longitude" string format (for API responses)
    pub fn to_string_format(&self) -> String {
        format!("{},{}", self.latitude, self.longitude)
    }

    /// Get latitude
    pub fn latitude(&self) -> f64 {
        self.latitude
    }

    /// Get longitude
    pub fn longitude(&self) -> f64 {
        self.longitude
    }

    /// Get both coordinates as tuple (latitude, longitude)
    pub fn parse_components(&self) -> (f64, f64) {
        (self.latitude, self.longitude)
    }

    /// Return as string (useful for logging/display)
    pub fn as_str(&self) -> String {
        self.to_string_format()
    }
}

// Display trait for easy printing
impl std::fmt::Display for Point {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string_format())
    }
}

// SQLx Type implementation for PostgreSQL POINT
impl Type<Postgres> for Point {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("point")
    }
}

// Convert between our Point and sqlx::types::PgPoint
impl From<Point> for PgPoint {
    fn from(point: Point) -> Self {
        PgPoint {
            x: point.longitude,
            y: point.latitude,
        }
    }
}

impl From<PgPoint> for Point {
    fn from(pg_point: PgPoint) -> Self {
        Point {
            longitude: pg_point.x,
            latitude: pg_point.y,
        }
    }
}

// Encode Point to PostgreSQL
impl Encode<'_, Postgres> for Point {
    fn encode_by_ref(
        &self,
        buf: &mut PgArgumentBuffer,
    ) -> Result<IsNull, Box<dyn std::error::Error + Send + Sync>> {
        let pg_point: PgPoint = (*self).into();
        <PgPoint as Encode<Postgres>>::encode_by_ref(&pg_point, buf)
    }
}

// Decode Point from PostgreSQL
impl<'r> Decode<'r, Postgres> for Point {
    fn decode(value: PgValueRef<'r>) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let pg_point = <PgPoint as Decode<Postgres>>::decode(value)?;
        Ok(pg_point.into())
    }
}

// Serialize for JSON API responses
impl serde::Serialize for Point {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
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
        Point::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::Point;
    use claims::{assert_err, assert_ok};

    #[test]
    fn create_point_with_new() {
        let point = Point::new(47.3769, 8.5417);
        assert_eq!(point.latitude, 47.3769);
        assert_eq!(point.longitude, 8.5417);
    }

    #[test]
    fn point_implements_copy() {
        let point1 = Point::new(47.3769, 8.5417);
        let point2 = point1;

        assert_eq!(point1.latitude, 47.3769);
        assert_eq!(point2.latitude, 47.3769);
    }

    #[test]
    fn parse_valid_coordinates_string() {
        let result = Point::parse("47.3769,8.5417");
        assert_ok!(&result);

        let point = result.unwrap();
        assert_eq!(point.latitude, 47.3769);
        assert_eq!(point.longitude, 8.5417);
    }

    #[test]
    fn parse_coordinates_with_spaces() {
        let result = Point::parse("47.3769, 8.5417");
        assert_ok!(&result);

        let point = result.unwrap();
        assert_eq!(point.latitude, 47.3769);
        assert_eq!(point.longitude, 8.5417);
    }

    #[test]
    fn parse_coordinates_with_extra_whitespace() {
        let result = Point::parse("  47.3769  ,  8.5417  ");
        assert_ok!(&result);

        let point = result.unwrap();
        assert_eq!(point.latitude, 47.3769);
        assert_eq!(point.longitude, 8.5417);
    }

    #[test]
    fn parse_invalid_format_single_number() {
        let result = Point::parse("47.3769");
        assert_err!(result);
    }

    #[test]
    fn parse_invalid_format_three_numbers() {
        let result = Point::parse("47.3769,8.5417,100");
        assert_err!(result);
    }

    #[test]
    fn parse_invalid_format_empty_string() {
        let result = Point::parse("");
        assert_err!(result);
    }

    #[test]
    fn parse_invalid_format_whitespace_only() {
        let result = Point::parse(" ");
        assert_err!(result);
    }

    #[test]
    fn parse_invalid_latitude_non_numeric() {
        let result = Point::parse("abc,8.5417");
        assert_err!(result);
    }

    #[test]
    fn parse_invalid_longitude_non_numeric() {
        let result = Point::parse("47.3769,xyz");
        assert_err!(result);
    }

    #[test]
    fn latitude_too_high() {
        let result = Point::parse("91.0,8.5417");
        assert_err!(result);
    }

    #[test]
    fn latitude_too_low() {
        let result = Point::parse("-91.0,8.5417");
        assert_err!(result);
    }

    #[test]
    fn longitude_too_high() {
        let result = Point::parse("47.3769,181.0");
        assert_err!(result);
    }

    #[test]
    fn longitude_too_low() {
        let result = Point::parse("47.3769,-181.0");
        assert_err!(result);
    }

    #[test]
    fn coordinates_outside_switzerland_rejected() {
        // Berlin coordinates
        let result = Point::parse("52.5200,13.4050");
        assert_err!(result);
    }

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
        let point = Point::parse(original).unwrap();
        let formatted = point.to_string_format();
        assert_eq!(formatted, original);
    }

    #[test]
    fn display_trait_works() {
        let point = Point::new(47.3769, 8.5417);
        let displayed = format!("{}", point);
        assert_eq!(displayed, "47.3769,8.5417");
    }

    #[test]
    fn latitude_method_returns_correct_value() {
        let point = Point::new(47.3769, 8.5417);
        assert_eq!(point.latitude(), 47.3769);
    }

    #[test]
    fn longitude_method_returns_correct_value() {
        let point = Point::new(47.3769, 8.5417);
        assert_eq!(point.longitude(), 8.5417);
    }

    #[test]
    fn parse_components_returns_tuple() {
        let point = Point::new(47.3769, 8.5417);
        let (lat, lon) = point.parse_components();
        assert_eq!(lat, 47.3769);
        assert_eq!(lon, 8.5417);
    }

    #[test]
    fn as_str_returns_formatted_string() {
        let point = Point::new(47.3769, 8.5417);
        assert_eq!(point.as_str(), "47.3769,8.5417");
    }

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

    #[test]
    fn all_canton_capitals_are_within_switzerland() {
        use crate::domain::test_data::CANTON_CAPITALS;

        for (city, coords) in CANTON_CAPITALS {
            let result = Point::parse(coords);
            assert_ok!(
                &result,
                "Canton capital {} with coordinates {} should be valid",
                city,
                coords
            );
        }
    }

    #[test]
    fn latitude_at_min_switzerland_boundary() {
        let result = Point::parse("45.8,8.0");
        assert_ok!(result);
    }

    #[test]
    fn latitude_at_max_switzerland_boundary() {
        let result = Point::parse("47.9,8.0");
        assert_ok!(result);
    }

    #[test]
    fn latitude_just_below_switzerland_boundary() {
        let result = Point::parse("45.79,8.0");
        assert_err!(result);
    }

    #[test]
    fn latitude_just_above_switzerland_boundary() {
        let result = Point::parse("47.91,8.0");
        assert_err!(result);
    }

    #[test]
    fn longitude_at_min_switzerland_boundary() {
        let result = Point::parse("47.0,5.9");
        assert_ok!(result);
    }

    #[test]
    fn longitude_at_max_switzerland_boundary() {
        let result = Point::parse("47.0,10.6");
        assert_ok!(result);
    }

    #[test]
    fn longitude_just_below_switzerland_boundary() {
        let result = Point::parse("47.0,5.89");
        assert_err!(result);
    }

    #[test]
    fn longitude_just_above_switzerland_boundary() {
        let result = Point::parse("47.0,10.61");
        assert_err!(result);
    }

    #[test]
    fn multiple_whitespace_variations_rejected() {
        assert_err!(Point::parse("   "));
        assert_err!(Point::parse("\t"));
        assert_err!(Point::parse("\n"));
    }

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

    #[test]
    fn deserialize_coordinates_outside_switzerland_fails() {
        let json = r#""52.5200,13.4050""#; // Berlin
        let result: Result<Point, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

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

    #[test]
    fn parse_high_precision_coordinates() {
        let result = Point::parse("47.123456789,8.987654321");
        assert_ok!(&result);

        let point = result.unwrap();
        assert_eq!(point.latitude, 47.123456789);
        assert_eq!(point.longitude, 8.987654321);
    }

    #[test]
    fn parse_integer_coordinates() {
        let result = Point::parse("47,8");
        assert_ok!(&result);

        let point = result.unwrap();
        assert_eq!(point.latitude, 47.0);
        assert_eq!(point.longitude, 8.0);
    }
}
