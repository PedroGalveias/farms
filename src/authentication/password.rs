use anyhow::Context;
use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
    password_hash::{SaltString, rand_core::OsRng},
};
use secrecy::{ExposeSecret, SecretString};

pub fn compute_password_hash(password: SecretString) -> Result<SecretString, anyhow::Error> {
    // Generate Salt
    let salt = SaltString::generate(&mut OsRng);

    // Create Digest
    let digest = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(15000, 2, 1, None).context("Failed to build Argon2 parameters.")?,
    )
    .hash_password(password.expose_secret().as_bytes(), &salt)
    .context("Failed to hash password")?
    .to_string();

    // Return Digest
    Ok(SecretString::from(digest))
}

pub fn verify_password_hash(
    expected_password: SecretString,
    password_candidate: SecretString,
) -> Result<(), anyhow::Error> {
    let expected_password_hash = PasswordHash::new(expected_password.expose_secret())
        .context("Failed to parse hash in PHC string format.")?;

    Argon2::default()
        .verify_password(
            password_candidate.expose_secret().as_bytes(),
            &expected_password_hash,
        )
        .context("Invalid password")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_password_hash_succeeds_with_valid_password() {
        let password = SecretString::from("valid_password_123!".to_string());

        let result = compute_password_hash(password);

        assert!(result.is_ok(), "Hashing should succeed.");
    }

    #[test]
    fn compute_password_hash_produces_phc_string_format() {
        let password = SecretString::from("another_valid_password_123!".to_string());

        let hash = compute_password_hash(password).unwrap();
        let hash_str = hash.expose_secret();

        assert!(
            hash_str.starts_with("$argon2id$"),
            "Hash should start with $argon2id$, got: {}.",
            hash_str
        );
        assert!(
            hash_str.contains("$v=19$"),
            "Hash should contain version v=19."
        );
        assert!(
            hash_str.contains("$m=15000,t=2,p=1$"),
            "Hash should contain correct parameters."
        );
    }

    #[test]
    fn compute_password_hash_with_same_password_produces_different_hashes() {
        let password_a = SecretString::from("same_password".to_string());
        let password_b = SecretString::from("same_password".to_string());

        let hash1 = compute_password_hash(password_a).unwrap();
        let hash2 = compute_password_hash(password_b).unwrap();

        assert_ne!(
            hash1.expose_secret(),
            hash2.expose_secret(),
            "Same password should produce different hashes due to random salt."
        );
    }

    #[test]
    fn compute_password_hash_with_special_characters() {
        let password = SecretString::from("p@$$w0rd!#%&*()[]{}".to_string());

        let result = compute_password_hash(password);

        assert!(result.is_ok(), "Should handle special characters");
    }

    #[test]
    fn compute_password_hash_with_long_password() {
        let long_password = "a".repeat(1000);
        let password = SecretString::from(long_password);

        let result = compute_password_hash(password);

        assert!(result.is_ok(), "Should handle long passwords");
    }

    #[test]
    fn compute_password_hash_with_unicode_characters() {
        let password = SecretString::from("pāsswörd123🔒".to_string());

        let result = compute_password_hash(password);

        assert!(result.is_ok(), "Should handle Unicode characters");
    }

    #[test]
    fn verify_password_hash_fails_with_incorrect_password() {
        let correct_password = SecretString::from("correct_password".to_string());
        let wrong_password = SecretString::from("wrong_password".to_string());
        let hash = compute_password_hash(correct_password).unwrap();

        let result = verify_password_hash(hash, wrong_password);

        assert!(
            result.is_err(),
            "Verification should fail with incorrect password"
        );
        let error_message = result.unwrap_err().to_string();
        assert!(
            error_message.contains("Invalid password"),
            "Error should mention invalid password, got: {}",
            error_message
        );
    }

    #[test]
    fn verify_password_hash_succeeds_with_correct_password() {
        let password = SecretString::from("correct_password".to_string());
        let hash = compute_password_hash(password.clone()).unwrap();

        let result = verify_password_hash(hash, password);

        assert!(
            result.is_ok(),
            "Verification should succeed with correct password"
        );
    }

    #[test]
    fn verify_password_hash_fails_with_slightly_different_password() {
        // Arrange
        let password = SecretString::from("Password123".to_string());
        let wrong_password = SecretString::from("password123".to_string()); // Different case
        let hash = compute_password_hash(password).unwrap();

        // Act
        let result = verify_password_hash(hash, wrong_password);

        // Assert
        assert!(
            result.is_err(),
            "Verification should be case-sensitive and fail"
        );
    }

    #[test]
    fn verify_password_hash_with_invalid_hash_format() {
        // Arrange
        let password = SecretString::from("test_password".to_string());
        let invalid_hash = SecretString::from("not_a_valid_hash_format".to_string());

        // Act
        let result = verify_password_hash(invalid_hash, password);

        // Assert
        assert!(result.is_err(), "Should fail with invalid hash format");
        let error_message = result.unwrap_err().to_string();
        assert!(
            error_message.contains("Failed to parse hash in PHC string format"),
            "Error should mention PHC format parsing, got: {}",
            error_message
        );
    }

    #[test]
    fn verify_password_hash_with_corrupted_hash() {
        // Arrange
        let password = SecretString::from("test_password".to_string());
        // Valid format but corrupted data
        let corrupted_hash =
            SecretString::from("$argon2id$v=19$m=15000,t=2,p=1$CORRUPTED$CORRUPTED".to_string());

        // Act
        let result = verify_password_hash(corrupted_hash, password);

        // Assert
        assert!(result.is_err(), "Should fail with corrupted hash");
    }

    #[test]
    fn verify_password_hash_with_different_algorithm_fails() {
        // Arrange
        let password = SecretString::from("test".to_string());
        // This is a bcrypt hash (different algorithm)
        let bcrypt_hash = SecretString::from("$2b$12$invalid_bcrypt_hash_format".to_string());

        // Act
        let result = verify_password_hash(bcrypt_hash, password);

        // Assert
        assert!(
            result.is_err(),
            "Should reject hashes from different algorithms"
        );
    }

    #[test]
    fn verify_password_hash_empty_password_against_empty_hash() {
        // Arrange
        let empty_password = SecretString::from("".to_string());
        let hash = compute_password_hash(empty_password.clone()).unwrap();

        // Act
        let result = verify_password_hash(hash, empty_password);

        // Assert
        assert!(result.is_ok(), "Should verify empty password correctly");
    }
}
