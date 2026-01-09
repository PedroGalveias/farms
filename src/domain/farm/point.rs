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

    #[error("Invalid latitude. Must be between -90 and 90")]
    InvalidLatitude(f64),

    #[error("Invalid longitude. Must be between -180 and 180")]
    InvalidLongitude(f64),

    #[error("Coordinates not within Switzerland boundaries")]
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
    use fake::Fake;

    fn random_swiss_coordinates() -> (f64, f64) {
        let lat = (Point::MIN_LATITUDE..=Point::MAX_LATITUDE).fake::<f64>();
        let lon = (Point::MIN_LONGITUDE..=Point::MAX_LONGITUDE).fake::<f64>();
        (lat, lon)
    }

    fn random_non_swiss_coordinates() -> (f64, f64) {
        // Using ranges that don't overlap with Swiss boundaries
        let options = [
            ((50.0..=60.0).fake::<f64>(), (5.0..=15.0).fake::<f64>()),
            // Southern Europe (below Switzerland)
            ((40.0..=45.0).fake::<f64>(), (5.0..=15.0).fake::<f64>()),
            // Eastern Europe (east of Switzerland)
            ((45.0..=50.0).fake::<f64>(), (15.0..=25.0).fake::<f64>()),
            // Western Europe (west of Switzerland)
            ((45.0..=50.0).fake::<f64>(), (0.0..=5.0).fake::<f64>()),
        ];

        let index = (0..options.len()).fake::<usize>();
        options[index]
    }

    #[test]
    fn create_point_with_new() {
        let (lat, lon) = random_swiss_coordinates();
        let point = Point::new(lat, lon);
        assert_eq!(point.latitude, lat);
        assert_eq!(point.longitude, lon);
    }

    #[test]
    fn point_implements_copy() {
        let (lat, lon) = random_swiss_coordinates();
        let point1 = Point::new(lat, lon);
        let point2 = point1;

        assert_eq!(point1.latitude, lat);
        assert_eq!(point2.latitude, lat);
    }

    #[test]
    fn parse_valid_coordinates_string() {
        let (lat, lon) = random_swiss_coordinates();
        let coord_string = format!("{},{}", lat, lon);

        let result = Point::parse(&coord_string);
        assert_ok!(&result);

        let point = result.unwrap();
        assert_eq!(point.latitude, lat);
        assert_eq!(point.longitude, lon);
    }

    #[test]
    fn parse_coordinates_with_spaces() {
        let (lat, lon) = random_swiss_coordinates();
        let coord_string = format!("{}, {}", lat, lon);

        let result = Point::parse(&coord_string);
        assert_ok!(&result);

        let point = result.unwrap();
        assert_eq!(point.latitude, lat);
        assert_eq!(point.longitude, lon);
    }

    #[test]
    fn parse_coordinates_with_extra_whitespace() {
        let (lat, lon) = random_swiss_coordinates();
        let coord_string = format!("  {}  ,  {}  ", lat, lon);

        let result = Point::parse(&coord_string);
        assert_ok!(&result);

        let point = result.unwrap();
        assert_eq!(point.latitude, lat);
        assert_eq!(point.longitude, lon);
    }

    #[test]
    fn parse_invalid_format_single_number() {
        let (lat, _) = random_swiss_coordinates();
        let coord_string = format!("{}", lat);
        let result = Point::parse(&coord_string);
        assert_err!(result);
    }

    #[test]
    fn parse_invalid_format_three_numbers() {
        let (lat, lon) = random_swiss_coordinates();
        let altitude = (100.0..=1000.0).fake::<f64>();
        let coord_string = format!("{},{},{}", lat, lon, altitude);
        let result = Point::parse(&coord_string);
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
        let (_, lon) = random_swiss_coordinates();
        let coord_string = format!("abc,{}", lon);
        let result = Point::parse(&coord_string);
        assert_err!(result);
    }

    #[test]
    fn parse_invalid_longitude_non_numeric() {
        let (lat, _) = random_swiss_coordinates();
        let coord_string = format!("{},xyz", lat);
        let result = Point::parse(&coord_string);
        assert_err!(result);
    }

    #[test]
    fn latitude_too_high() {
        let lat = (91.0..=180.0).fake::<f64>();
        let (_, lon) = random_swiss_coordinates();
        let coord_string = format!("{},{}", lat, lon);
        let result = Point::parse(&coord_string);
        assert_err!(result);
    }

    #[test]
    fn latitude_too_low() {
        let lat = (-180.0..=-91.0).fake::<f64>();
        let (_, lon) = random_swiss_coordinates();
        let coord_string = format!("{},{}", lat, lon);
        let result = Point::parse(&coord_string);
        assert_err!(result);
    }

    #[test]
    fn longitude_too_high() {
        let (lat, _) = random_swiss_coordinates();
        let lon = (181.0..=360.0).fake::<f64>();
        let coord_string = format!("{},{}", lat, lon);
        let result = Point::parse(&coord_string);
        assert_err!(result);
    }

    #[test]
    fn longitude_too_low() {
        let (lat, _) = random_swiss_coordinates();
        let lon = (-360.0..=-181.0).fake::<f64>();
        let coord_string = format!("{},{}", lat, lon);
        let result = Point::parse(&coord_string);
        assert_err!(result);
    }

    #[test]
    fn coordinates_outside_switzerland_rejected() {
        let (lat, lon) = random_non_swiss_coordinates();
        let coord_string = format!("{},{}", lat, lon);
        let result = Point::parse(&coord_string);
        assert_err!(result);
    }

    #[test]
    fn format_point_to_string() {
        let (lat, lon) = random_swiss_coordinates();
        let point = Point::new(lat, lon);
        assert_eq!(point.to_string_format(), format!("{},{}", lat, lon));
    }

    #[test]
    fn format_negative_coordinates() {
        // Generate coordinates with negative values (not in Switzerland, just for formatting test)
        let lat = (-89.0..=-45.0).fake::<f64>();
        let lon = (170.0..=179.0).fake::<f64>();
        let point = Point::new(lat, lon);

        let formatted = point.to_string_format();
        assert!(formatted.contains(&lat.to_string()));
        assert!(formatted.contains(&lon.to_string()));
    }

    #[test]
    fn format_zero_coordinates() {
        let point = Point::new(0.0, 0.0);
        assert_eq!(point.to_string_format(), "0,0");
    }

    #[test]
    fn roundtrip_parse_and_format() {
        let (lat, lon) = random_swiss_coordinates();
        let original = format!("{},{}", lat, lon);
        let point = Point::parse(&original).unwrap();
        let formatted = point.to_string_format();
        assert_eq!(formatted, original);
    }

    #[test]
    fn display_trait_works() {
        let (lat, lon) = random_swiss_coordinates();
        let point = Point::new(lat, lon);
        let displayed = format!("{}", point);
        assert_eq!(displayed, format!("{},{}", lat, lon));
    }

    #[test]
    fn latitude_method_returns_correct_value() {
        let (lat, lon) = random_swiss_coordinates();
        let point = Point::new(lat, lon);
        assert_eq!(point.latitude(), lat);
    }

    #[test]
    fn longitude_method_returns_correct_value() {
        let (lat, lon) = random_swiss_coordinates();
        let point = Point::new(lat, lon);
        assert_eq!(point.longitude(), lon);
    }

    #[test]
    fn parse_components_returns_tuple() {
        let (lat, lon) = random_swiss_coordinates();
        let point = Point::new(lat, lon);
        let (parsed_lat, parsed_lon) = point.parse_components();
        assert_eq!(parsed_lat, lat);
        assert_eq!(parsed_lon, lon);
    }

    #[test]
    fn as_str_returns_formatted_string() {
        let (lat, lon) = random_swiss_coordinates();
        let point = Point::new(lat, lon);
        assert_eq!(point.as_str(), format!("{},{}", lat, lon));
    }

    #[test]
    fn points_with_same_coordinates_are_equal() {
        let (lat, lon) = random_swiss_coordinates();
        let point1 = Point::new(lat, lon);
        let point2 = Point::new(lat, lon);
        assert_eq!(point1, point2);
    }

    #[test]
    fn points_with_different_coordinates_are_not_equal() {
        let (lat1, lon1) = random_swiss_coordinates();
        let (lat2, lon2) = random_swiss_coordinates();

        let point1 = Point::new(lat1, lon1);
        let point2 = Point::new(lat2, lon2);

        // Only assert inequality if coordinates are actually different
        if lat1 != lat2 || lon1 != lon2 {
            assert_ne!(point1, point2);
        }
    }

    #[test]
    fn points_with_different_latitude_are_not_equal() {
        let (lat1, lon) = random_swiss_coordinates();
        let mut lat2 = lat1;
        // Ensure lat2 is different but still within Switzerland
        while lat2 == lat1 {
            lat2 = (Point::MIN_LATITUDE..=Point::MAX_LATITUDE).fake::<f64>();
        }

        let point1 = Point::new(lat1, lon);
        let point2 = Point::new(lat2, lon);
        assert_ne!(point1, point2);
    }

    #[test]
    fn points_with_different_longitude_are_not_equal() {
        let (lat, lon1) = random_swiss_coordinates();
        let mut lon2 = lon1;
        // Ensure lon2 is different but still within Switzerland
        while lon2 == lon1 {
            lon2 = (Point::MIN_LONGITUDE..=Point::MAX_LONGITUDE).fake::<f64>();
        }

        let point1 = Point::new(lat, lon1);
        let point2 = Point::new(lat, lon2);
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
        let result = Point::parse(&format!("{},{}", Point::MIN_LATITUDE, 8.0));
        assert_ok!(result);
    }

    #[test]
    fn latitude_at_max_switzerland_boundary() {
        let result = Point::parse(&format!("{},{}", Point::MAX_LATITUDE, 8.0));
        assert_ok!(result);
    }

    #[test]
    fn latitude_just_below_switzerland_boundary() {
        let lat = Point::MIN_LATITUDE - 0.01;
        let result = Point::parse(&format!("{},{}", lat, 8.0));
        assert_err!(result);
    }

    #[test]
    fn latitude_just_above_switzerland_boundary() {
        let lat = Point::MAX_LATITUDE + 0.01;
        let result = Point::parse(&format!("{},{}", lat, 8.0));
        assert_err!(result);
    }

    #[test]
    fn longitude_at_min_switzerland_boundary() {
        let result = Point::parse(&format!("{},{}", 47.0, Point::MIN_LONGITUDE));
        assert_ok!(result);
    }

    #[test]
    fn longitude_at_max_switzerland_boundary() {
        let result = Point::parse(&format!("{},{}", 47.0, Point::MAX_LONGITUDE));
        assert_ok!(result);
    }

    #[test]
    fn longitude_just_below_switzerland_boundary() {
        let lon = Point::MIN_LONGITUDE - 0.01;
        let result = Point::parse(&format!("{},{}", 47.0, lon));
        assert_err!(result);
    }

    #[test]
    fn longitude_just_above_switzerland_boundary() {
        let lon = Point::MAX_LONGITUDE + 0.01;
        let result = Point::parse(&format!("{},{}", 47.0, lon));
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
        let (lat, lon) = random_swiss_coordinates();
        let point = Point::new(lat, lon);
        let json = serde_json::to_string(&point).unwrap();
        let expected = format!(r#""{},{}""#, lat, lon);
        assert_eq!(json, expected);
    }

    #[test]
    fn deserialize_point_from_json() {
        let (lat, lon) = random_swiss_coordinates();
        let json = format!(r#""{},{}""#, lat, lon);
        let point: Point = serde_json::from_str(&json).unwrap();
        assert_eq!(point.latitude, lat);
        assert_eq!(point.longitude, lon);
    }

    #[test]
    fn roundtrip_serde_json() {
        let (lat, lon) = random_swiss_coordinates();
        let original = Point::new(lat, lon);
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: Point = serde_json::from_str(&json).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn deserialize_point_with_spaces_from_json() {
        let (lat, lon) = random_swiss_coordinates();
        let json = format!(r#""{}, {}""#, lat, lon);
        let point: Point = serde_json::from_str(&json).unwrap();
        assert_eq!(point.latitude, lat);
        assert_eq!(point.longitude, lon);
    }

    #[test]
    fn deserialize_invalid_json_format_fails() {
        let json = r#""not-a-coordinate""#;
        let result: Result<Point, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_coordinates_outside_switzerland_fails() {
        let (lat, lon) = random_non_swiss_coordinates();
        let json = format!(r#""{},{}""#, lat, lon);
        let result: Result<Point, _> = serde_json::from_str(&json);
        assert!(result.is_err());
    }

    #[test]
    fn debug_format_shows_structure() {
        let (lat, lon) = random_swiss_coordinates();
        let point = Point::new(lat, lon);
        let debug_str = format!("{:?}", point);
        assert!(debug_str.contains("Point"));
        assert!(debug_str.contains(&lat.to_string()));
        assert!(debug_str.contains(&lon.to_string()));
    }

    #[test]
    fn clone_creates_independent_copy() {
        let (lat, lon) = random_swiss_coordinates();
        let point1 = Point::new(lat, lon);
        let point2 = point1.clone();

        assert_eq!(point1, point2);
        assert_eq!(point1.latitude, point2.latitude);
        assert_eq!(point1.longitude, point2.longitude);
    }

    #[test]
    fn parse_high_precision_coordinates() {
        // Generate high-precision coordinates within Switzerland
        let lat = (Point::MIN_LATITUDE..=Point::MAX_LATITUDE).fake::<f64>();
        let lon = (Point::MIN_LONGITUDE..=Point::MAX_LONGITUDE).fake::<f64>();

        // Format with high precision
        let coord_string = format!("{:.9},{:.9}", lat, lon);

        let result = Point::parse(&coord_string);
        assert_ok!(&result);

        let point = result.unwrap();
        // Check equality with small epsilon due to floating point precision
        assert!((point.latitude - lat).abs() < 1e-9);
        assert!((point.longitude - lon).abs() < 1e-9);
    }

    #[test]
    fn parse_integer_coordinates() {
        // Generate integer coordinates within Switzerland bounds
        let lat =
            (Point::MIN_LATITUDE.ceil() as i32..=Point::MAX_LATITUDE.floor() as i32).fake::<i32>();
        let lon = (Point::MIN_LONGITUDE.ceil() as i32..=Point::MAX_LONGITUDE.floor() as i32)
            .fake::<i32>();

        let coord_string = format!("{},{}", lat, lon);
        let result = Point::parse(&coord_string);
        assert_ok!(&result);

        let point = result.unwrap();
        assert_eq!(point.latitude, lat as f64);
        assert_eq!(point.longitude, lon as f64);
    }

    #[test]
    fn convert_to_pgpoint() {
        use sqlx::postgres::types::PgPoint;

        let (lat, lon) = random_swiss_coordinates();
        let point = Point::new(lat, lon);
        let pg_point: PgPoint = point.into();
        assert_eq!(pg_point.x, lon);
        assert_eq!(pg_point.y, lat);
    }

    #[test]
    fn convert_from_pgpoint() {
        use sqlx::postgres::types::PgPoint;

        let (lat, lon) = random_swiss_coordinates();
        let pg_point = PgPoint { x: lon, y: lat };
        let point: Point = pg_point.into();
        assert_eq!(point.latitude, lat);
        assert_eq!(point.longitude, lon);
    }

    #[test]
    fn roundtrip_pgpoint_conversion() {
        use sqlx::postgres::types::PgPoint;

        let (lat, lon) = random_swiss_coordinates();
        let original = Point::new(lat, lon);
        let pg_point: PgPoint = original.into();
        let converted: Point = pg_point.into();
        assert_eq!(original, converted);
    }
}
