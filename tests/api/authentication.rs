async fn assert_invalid_credentials() {}

#[tokio::test]
async fn validate_credentials_returns_user_id_for_valid_credentials() {}

#[tokio::test]
async fn validate_credentials_rejects_unknown_email() {}

#[tokio::test]
async fn validate_credentials_returns_unexpected_error_when_the_query_fails() {}

#[tokio::test]
async fn change_password_allows_login_with_new_password() {}
