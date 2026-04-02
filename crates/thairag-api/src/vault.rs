use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, OsRng},
};

pub struct Vault {
    cipher: Aes256Gcm,
}

impl Vault {
    /// Initialize vault encryption.
    /// 1. Check THAIRAG_ENCRYPTION_KEY env var (hex-encoded 32 bytes)
    /// 2. If not set, check {data_dir}/encryption.key file
    /// 3. If neither exist, generate random key, save to file, log warning
    pub fn init(data_dir: &str) -> Self {
        let key_bytes = if let Ok(hex_key) = std::env::var("THAIRAG_ENCRYPTION_KEY") {
            let bytes = hex::decode(&hex_key)
                .expect("THAIRAG_ENCRYPTION_KEY must be valid hex (64 hex chars = 32 bytes)");
            assert_eq!(
                bytes.len(),
                32,
                "THAIRAG_ENCRYPTION_KEY must be exactly 32 bytes (64 hex chars)"
            );
            tracing::info!("Vault: using encryption key from THAIRAG_ENCRYPTION_KEY env var");
            bytes
        } else {
            let key_path = std::path::Path::new(data_dir).join("encryption.key");
            // Try to load existing key file; fall through to generation if invalid
            let loaded = if key_path.exists() {
                std::fs::read_to_string(&key_path)
                    .ok()
                    .and_then(|hex_key| {
                        let trimmed = hex_key.trim();
                        if trimmed.is_empty() {
                            return None;
                        }
                        hex::decode(trimmed).ok()
                    })
                    .filter(|bytes| bytes.len() == 32)
            } else {
                None
            };
            if let Some(bytes) = loaded {
                tracing::info!(
                    path = %key_path.display(),
                    "Vault: loaded encryption key from file"
                );
                bytes
            } else {
                // Generate random key
                let mut key = [0u8; 32];
                OsRng.fill_bytes(&mut key);
                let hex_key = hex::encode(key);

                // Ensure data dir exists
                std::fs::create_dir_all(data_dir).ok();
                std::fs::write(&key_path, &hex_key).expect("Failed to write encryption.key file");

                // Try to set file permissions on Unix
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))
                        .ok();
                }

                tracing::warn!(
                    path = %key_path.display(),
                    "Vault: generated new encryption key and saved to file. \
                     For production, set THAIRAG_ENCRYPTION_KEY env var instead."
                );
                key.to_vec()
            }
        };

        let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&key_bytes);
        let cipher = Aes256Gcm::new(key);

        Self { cipher }
    }

    /// Encrypt plaintext -> hex(12-byte nonce || ciphertext || tag)
    pub fn encrypt(&self, plaintext: &str) -> String {
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_bytes())
            .expect("Vault encryption failed");

        // Concatenate nonce + ciphertext
        let mut result = nonce_bytes.to_vec();
        result.extend_from_slice(&ciphertext);
        hex::encode(&result)
    }

    /// Decrypt hex(nonce || ciphertext) -> plaintext
    pub fn decrypt(&self, hex_ct: &str) -> Result<String, String> {
        let bytes = hex::decode(hex_ct).map_err(|e| format!("Invalid hex: {e}"))?;
        if bytes.len() < 12 {
            return Err("Ciphertext too short".into());
        }
        let (nonce_bytes, ciphertext) = bytes.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = self
            .cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| format!("Decryption failed: {e}"))?;

        String::from_utf8(plaintext).map_err(|e| format!("Invalid UTF-8: {e}"))
    }

    /// Mask an API key for display: "sk-proj-abc123xyz" -> "sk-p...xyz"
    pub fn mask(key: &str) -> String {
        let len = key.len();
        if len <= 8 {
            return "*".repeat(len);
        }
        let prefix = &key[..4.min(len)];
        let suffix = &key[len.saturating_sub(4)..];
        format!("{prefix}...{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_vault() -> Vault {
        let key = [0x42u8; 32]; // fixed test key
        let aes_key = aes_gcm::Key::<Aes256Gcm>::from_slice(&key);
        Vault {
            cipher: Aes256Gcm::new(aes_key),
        }
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let vault = test_vault();
        let plaintext = "sk-proj-abc123def456";
        let encrypted = vault.encrypt(plaintext);
        let decrypted = vault.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn different_encryptions_differ() {
        let vault = test_vault();
        let e1 = vault.encrypt("test");
        let e2 = vault.encrypt("test");
        assert_ne!(e1, e2); // different nonces
    }

    #[test]
    fn mask_key() {
        assert_eq!(Vault::mask("sk-proj-abc123def456xyz"), "sk-p...6xyz");
        assert_eq!(Vault::mask("short"), "*****");
        assert_eq!(Vault::mask("12345678"), "********");
        assert_eq!(Vault::mask(""), "");
    }

    #[test]
    fn decrypt_invalid_hex() {
        let vault = test_vault();
        assert!(vault.decrypt("not-hex!").is_err());
    }

    #[test]
    fn decrypt_too_short() {
        let vault = test_vault();
        assert!(vault.decrypt("aabb").is_err());
    }
}
