use anyhow::Context;
use argon2::{Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version};
use secrecy::{ExposeSecret, SecretString};

pub fn compute_password_hash(password: SecretString) -> Result<SecretString, anyhow::Error> {
    // `hash_password` draws a random 16-byte salt from the OS RNG (getrandom).
    let digest = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(15000, 2, 1, None).context("Failed to build Argon2 parameters.")?,
    )
    .hash_password(password.expose_secret().as_bytes())
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
    use fake::{Fake, faker::internet::en::Password as FakerPassword};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_PASSWORD_COUNTER: AtomicUsize = AtomicUsize::new(1);

    enum PasswordKind {
        Standard,
        SpecialCharacters,
        Unicode,
        Long(usize),
        Empty,
    }

    fn next_password_seed() -> usize {
        TEST_PASSWORD_COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    fn make_test_password(kind: PasswordKind) -> SecretString {
        let seed = next_password_seed();
        match kind {
            PasswordKind::Standard => {
                let fake_password: String = FakerPassword(12..24).fake();
                SecretString::from(format!("pw-{seed}-{fake_password}"))
            }
            PasswordKind::SpecialCharacters => {
                SecretString::from(format!("pw-{seed}-!@#$%^&*()[]{{}}"))
            }
            PasswordKind::Unicode => SecretString::from(format!("pw-{seed}-pāsswörd🔒")),
            PasswordKind::Long(length) => {
                let mut password = format!("pw-{seed}-{}", FakerPassword(12..24).fake::<String>());
                if password.len() < length {
                    while password.len() < length {
                        let remaining = length - password.len();
                        let chunk: String = FakerPassword(12..24).fake();
                        if chunk.len() <= remaining {
                            password.push_str(&chunk);
                        } else {
                            password.push_str(&chunk[..remaining]);
                        }
                    }
                } else {
                    password.truncate(length);
                }
                SecretString::from(password)
            }
            PasswordKind::Empty => SecretString::from(String::new()),
        }
    }

    #[test]
    fn compute_password_hash_succeeds_with_valid_password() {
        let password = make_test_password(PasswordKind::Standard);

        let result = compute_password_hash(password);

        assert!(result.is_ok(), "Hashing should succeed.");
    }

    #[test]
    fn compute_password_hash_produces_phc_string_format() {
        let password = make_test_password(PasswordKind::Standard);

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
        let password = make_test_password(PasswordKind::Standard);

        let hash1 = compute_password_hash(password.clone()).unwrap();
        let hash2 = compute_password_hash(password).unwrap();

        assert_ne!(
            hash1.expose_secret(),
            hash2.expose_secret(),
            "Same password should produce different hashes due to random salt."
        );
    }

    #[test]
    fn compute_password_hash_with_special_characters() {
        let password = make_test_password(PasswordKind::SpecialCharacters);

        let result = compute_password_hash(password);

        assert!(result.is_ok(), "Should handle special characters");
    }

    #[test]
    fn compute_password_hash_with_long_password() {
        let password = make_test_password(PasswordKind::Long((14..=100).fake::<usize>()));

        let result = compute_password_hash(password);

        assert!(result.is_ok(), "Should handle long passwords");
    }

    #[test]
    fn compute_password_hash_with_unicode_characters() {
        let password = make_test_password(PasswordKind::Unicode);

        let result = compute_password_hash(password);

        assert!(result.is_ok(), "Should handle Unicode characters");
    }

    #[test]
    fn verify_password_hash_fails_with_incorrect_password() {
        let correct_password = make_test_password(PasswordKind::Standard);
        let wrong_password = make_test_password(PasswordKind::Standard);
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
        let password = make_test_password(PasswordKind::Standard);
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
        let seed = next_password_seed();
        let base_password = format!("CaseCheck{seed}Xy");
        let password = SecretString::from(base_password.to_uppercase());
        let wrong_password = SecretString::from(base_password.to_lowercase());
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
        let password = make_test_password(PasswordKind::Standard);
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
        let password = make_test_password(PasswordKind::Standard);
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
        let password = make_test_password(PasswordKind::Standard);
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
        let empty_password = make_test_password(PasswordKind::Empty);
        let hash = compute_password_hash(empty_password.clone()).unwrap();

        // Act
        let result = verify_password_hash(hash, empty_password);

        // Assert
        assert!(result.is_ok(), "Should verify empty password correctly");
    }

    /// Produced by argon2 0.5.3 with the same algorithm and params as
    /// `compute_password_hash` (fixed salt), for password
    /// `ARGON2_0_5_FIXTURE_PASSWORD`. Guards that hashes already stored in the
    /// database keep verifying across argon2 crate upgrades.
    const ARGON2_0_5_FIXTURE_HASH: &str = "$argon2id$v=19$m=15000,t=2,p=1$ZmFybXMtMC41LWZpeHR1cg$yI1j7Katv4qaHWaFy1E/ag6GZ/x5jS9X47VS2MObjcA";
    const ARGON2_0_5_FIXTURE_PASSWORD: &str = "argon2-0.5-compat-pw";

    #[test]
    fn verify_password_hash_accepts_hash_from_previous_argon2_release() {
        let result = verify_password_hash(
            SecretString::from(ARGON2_0_5_FIXTURE_HASH),
            SecretString::from(ARGON2_0_5_FIXTURE_PASSWORD),
        );

        assert!(
            result.is_ok(),
            "Hash stored by argon2 0.5 should still verify, got: {result:?}"
        );
    }

    #[test]
    fn verify_password_hash_rejects_wrong_password_for_previous_argon2_release_hash() {
        let result = verify_password_hash(
            SecretString::from(ARGON2_0_5_FIXTURE_HASH),
            SecretString::from(format!("{ARGON2_0_5_FIXTURE_PASSWORD}-wrong")),
        );

        assert!(result.is_err(), "Wrong password must not verify");
    }

    #[test]
    fn compute_password_hash_keeps_algorithm_params_and_salt_length() {
        let hash = compute_password_hash(make_test_password(PasswordKind::Standard)).unwrap();
        let parsed = PasswordHash::new(hash.expose_secret()).unwrap();

        assert_eq!(parsed.algorithm.as_str(), "argon2id");
        assert_eq!(parsed.version, Some(0x13));
        assert_eq!(parsed.params.to_string(), "m=15000,t=2,p=1");
        // 16 random bytes, the same salt length argon2 0.5's SaltString::generate used.
        assert_eq!(parsed.salt.expect("salt").as_ref().len(), 16);
    }
}
