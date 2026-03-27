pub use credentials::{
    AuthenticatedUser, ValidateCredentialsError, change_password, validate_credentials,
};

mod credentials;
mod password;
