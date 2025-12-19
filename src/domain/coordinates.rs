use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Coordinates(String);

#[derive(Debug, Error)]
pub enum CoordinatesError {
    #[error("Invalid coordinate format. Expected 'latitude,longitude' (e.g., '47.3769,8.5417')")]
    InvalidFormat,

    #[error("Invalid latitude: {0}. Must be between -90 and 90")]
    InvalidLatitude(f64),

    #[error("Invalid longitude: {0}. Must be between -180 and 180")]
    InvalidLongitude(f64),

    #[error("Coordinates not within Switzerland boundaries. Latitude: {lat}, Longitude: {long}")]
    NotWithinSwitzerland { lat: f64, long: f64 },
}

impl Coordinates {
    // Switzerland boundaries (approximate)
    const MIN_LAT: f64 = 45.8;
    const MAX_LAT: f64 = 47.9;
    const MIN_LONG: f64 = 5.9;
    const MAX_LONG: f64 = 10.6;

    fn is_within_switzerland(lat: f64, long: f64) -> bool {
        (Self::MIN_LAT..=Self::MAX_LAT).contains(&lat)
            && (Self::MIN_LONG..=Self::MAX_LONG).contains(&long)
    }

    /// Parse and validate coordinates string
    ///
    /// Expected format: "latitude, longitude" (e.g., "47.3769,8.5417")
    /// Validates that coordinate are within Switzerland boundaries
    pub fn parse(s: String) -> Result<Self, CoordinatesError> {
        let parts: Vec<&str> = s.split(',').collect();

        if parts.len() != 2 {
            return Err(CoordinatesError::InvalidFormat);
        }

        let lat = parts[0]
            .trim()
            .parse::<f64>()
            .map_err(|_| CoordinatesError::InvalidFormat)?;

        let long = parts[1]
            .trim()
            .parse::<f64>()
            .map_err(|_| CoordinatesError::InvalidFormat)?;

        if !(-90.0..=90.0).contains(&lat) {
            return Err(CoordinatesError::InvalidLatitude(lat));
        }

        if !(-180.0..=180.0).contains(&long) {
            return Err(CoordinatesError::InvalidLongitude(long));
        }

        if !Self::is_within_switzerland(lat, long) {
            return Err(CoordinatesError::NotWithinSwitzerland { lat, long });
        }

        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn parse_components(&self) -> (f64, f64) {
        let parts: Vec<&str> = self.0.split(',').collect();
        let lat = parts[0].trim().parse::<f64>().expect("Already validated");
        let long = parts[1].trim().parse::<f64>().expect("Already validated");

        (lat, long)
    }

    pub fn latitude(&self) -> f64 {
        self.parse_components().0
    }

    pub fn longitude(&self) -> f64 {
        self.parse_components().1
    }
}

impl AsRef<str> for Coordinates {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Coordinates {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::Coordinates;
    use claims::{assert_err, assert_ok};

    #[test]
    fn all_canton_capitals_are_within_switzerland() {
        // All 26 Swiss canton capitals with their coordinates
        let canton_capitals = vec![
            ("Zürich", "47.3769,8.5417"),       // ZH
            ("Bern", "46.9481,7.4474"),         // BE
            ("Lucerne", "47.0502,8.3093"),      // LU
            ("Altdorf", "46.8805,8.6444"),      // UR
            ("Schwyz", "47.0207,8.6532"),       // SZ
            ("Sarnen", "46.8960,8.2461"),       // OW
            ("Stans", "46.9579,8.3659"),        // NW
            ("Glarus", "47.0404,9.0679"),       // GL
            ("Zug", "47.1724,8.5153"),          // ZG
            ("Fribourg", "46.8063,7.1608"),     // FR
            ("Solothurn", "47.2084,7.5371"),    // SO
            ("Basel", "47.5596,7.5886"),        // BS
            ("Liestal", "47.4814,7.7343"),      // BL
            ("Schaffhausen", "47.6979,8.6344"), // SH
            ("Herisau", "47.3859,9.2792"),      // AR
            ("Appenzell", "47.3316,9.4094"),    // AI
            ("St. Gallen", "47.4245,9.3767"),   // SG
            ("Chur", "46.8499,9.5331"),         // GR
            ("Aarau", "47.3925,8.0457"),        // AG
            ("Frauenfeld", "47.5536,8.8988"),   // TG
            ("Bellinzona", "46.1930,9.0208"),   // TI
            ("Lausanne", "46.5197,6.6323"),     // VD
            ("Sion", "46.2310,7.3603"),         // VS
            ("Neuchâtel", "46.9896,6.9294"),    // NE
            ("Geneva", "46.2044,6.1432"),       // GE
            ("Delémont", "47.3653,7.3453"),     // JU
        ];

        for (city, coordinates) in canton_capitals {
            let res = Coordinates::parse(coordinates.to_string());
            assert_ok!(
                &res,
                "Canton capital {} with coordinates {} is valid",
                city,
                coordinates
            );
        }
    }

    #[test]
    fn coordinates_with_spaces_are_valid() {
        let coordinates = Coordinates::parse("47.3769, 8.5417".to_string());
        assert_ok!(coordinates);
    }

    #[test]
    fn invalid_coordinates_empty() {
        let empty_coordinates = Coordinates::parse(" ".to_string());
        assert_err!(empty_coordinates);

        let empty_with_space_coordinates = Coordinates::parse(" ".to_string());
        assert_err!(empty_with_space_coordinates);
    }

    #[test]
    fn invalid_format_single_number() {
        let coordinates = Coordinates::parse("47.3769".to_string());
        assert_err!(coordinates);
    }

    #[test]
    fn invalid_format_three_numbers() {
        let coordinates = Coordinates::parse("47.3769,8.5417,100.2334".to_string());
        assert_err!(coordinates);
    }

    #[test]
    fn invalid_format_non_numeric() {
        let coordinates = Coordinates::parse("abc,def".to_string());
        assert_err!(coordinates);
    }

    #[test]
    fn latitude_too_high() {
        let coordinates = Coordinates::parse("91.0,8.5417".to_string());
        assert_err!(coordinates);
    }

    #[test]
    fn longitude_too_high() {
        let coordinates = Coordinates::parse("47.3769,181.0".to_string());
        assert_err!(coordinates);
    }

    #[test]
    fn invalid_coordinates_outside_of_switzerland() {
        // Testing with Berlin coordinates
        let coordinates = Coordinates::parse("52.5200, 13.4050".to_string());
        assert_err!(coordinates);
    }
}
