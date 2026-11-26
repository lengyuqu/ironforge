//! AES-256-GCM encryption utilities for sensitive data at rest.

use crate::error::{CoreError, CoreResult};
use aes_gcm::{
    aead::{Aead, OsRng},
    Aes256Gcm, Key, KeyInit, Nonce,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use sha2::{Digest, Sha256};

pub fn derive_key(secret: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

pub fn encrypt(plaintext: &str, key: &[u8; 32]) -> CoreResult<String> {
    // `GenericArray::from_slice` is deprecated since generic-array 0.14.9; the
    // array conversions below are the replacement (both sides are fixed-size).
    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(*key));
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from(nonce_bytes);

    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|e| CoreError::internal(format!("encryption failed: {}", e)))?;

    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(URL_SAFE_NO_PAD.encode(&combined))
}

pub fn decrypt(encoded: &str, key: &[u8; 32]) -> CoreResult<String> {
    let combined = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|e| CoreError::internal(format!("base64 decode error: {}", e)))?;
    if combined.len() < 12 {
        return Err(CoreError::InvalidInput("ciphertext too short".into()));
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce_arr: [u8; 12] = nonce_bytes
        .try_into()
        .map_err(|_| CoreError::InvalidInput("ciphertext too short".into()))?;
    let nonce = Nonce::from(nonce_arr);
    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(*key));

    let plaintext = cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|e| CoreError::internal(format!("decryption failed: {}", e)))?;

    String::from_utf8(plaintext).map_err(|e| CoreError::internal(format!("invalid UTF-8: {}", e)))
}

/// Replace sensitive values in output before it is persisted or streamed. `secrets` must be decrypted plaintext, never the stored ciphertext (both the HTTP log-upload path and the rg-ci runner decrypt before calling this). Values shorter than 4 bytes are skipped silently, so callers must reject short secrets at input time (`api/ci_secrets.rs` does: 4..=65536 bytes; `ci_secret_ops::upsert` itself does not validate).
pub fn mask_values(input: &str, secrets: &[String]) -> String {
    let mut masked = input.to_owned();
    let mut values = secrets
        .iter()
        .filter(|value| value.len() >= 4)
        .collect::<Vec<_>>();
    values.sort_by_key(|value| std::cmp::Reverse(value.len()));
    for value in values {
        masked = masked.replace(value.as_str(), "***");
    }
    masked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = derive_key("test-secret-key");
        let plaintext = "JBSWY3DPEHPK3PXP";
        let encrypted = encrypt(plaintext, &key).unwrap();
        let decrypted = decrypt(&encrypted, &key).unwrap();
        assert_eq!(plaintext, decrypted);
    }
}
