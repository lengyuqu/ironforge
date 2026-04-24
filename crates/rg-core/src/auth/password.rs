//! Password hashing and verification using Argon2id.

use anyhow::Result;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

/// Hash a plaintext password. Returns a PHC-format string (includes algorithm, params, salt, hash).
pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("password hashing failed: {}", e))?;
    Ok(hash.to_string())
}

/// Verify a plaintext password against a stored PHC hash.
pub fn verify_password(password: &str, hash: &str) -> Result<bool> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| anyhow::anyhow!("invalid password hash: {}", e))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify() {
        let hash = hash_password("hunter2").unwrap();
        assert!(verify_password("hunter2", &hash).unwrap());
        assert!(!verify_password("wrong", &hash).unwrap());
    }
}
