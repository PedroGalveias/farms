//! Per-(farm, product) availability state.

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, serde::Serialize, serde::Deserialize)]
#[sqlx(type_name = "stock_status", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "snake_case")]
pub enum StockStatus {
    Available,
    Seasonal,
    Unavailable,
}
