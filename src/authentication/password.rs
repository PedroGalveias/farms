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
mod tests {}
