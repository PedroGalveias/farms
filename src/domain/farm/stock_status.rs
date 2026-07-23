//! Per-(farm, product) availability state.

// JSON casing MUST match the PostgreSQL enum labels (SCREAMING_SNAKE_CASE) so
// the wire value equals the stored value — `AVAILABLE`, `SEASONAL`,
// `UNAVAILABLE`. The frontend's `StockStatus` union and the Bruno docs expect
// exactly these. (An earlier `snake_case` serde attr silently shipped
// lowercase JSON, which the frontend's `status === "AVAILABLE"` checks never
// matched.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, serde::Serialize, serde::Deserialize)]
#[sqlx(type_name = "stock_status", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StockStatus {
    Available,
    Seasonal,
    Unavailable,
}

#[cfg(test)]
mod tests {
    use super::StockStatus;

    #[test]
    fn serializes_to_screaming_snake_case_matching_the_db_and_frontend() {
        assert_eq!(
            serde_json::to_string(&StockStatus::Available).unwrap(),
            "\"AVAILABLE\""
        );
        assert_eq!(
            serde_json::to_string(&StockStatus::Seasonal).unwrap(),
            "\"SEASONAL\""
        );
        assert_eq!(
            serde_json::to_string(&StockStatus::Unavailable).unwrap(),
            "\"UNAVAILABLE\""
        );
    }

    #[test]
    fn deserializes_from_the_same_uppercase_labels() {
        let parsed: StockStatus = serde_json::from_str("\"SEASONAL\"").unwrap();
        assert_eq!(parsed, StockStatus::Seasonal);
    }
}
