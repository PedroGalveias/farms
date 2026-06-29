#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, serde::Serialize, serde::Deserialize)]
#[sqlx(type_name = "user_status", rename_all = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "snake_case")]
pub enum UserStatus {
    PendingVerification,
    Active,
    Disabled,
}
