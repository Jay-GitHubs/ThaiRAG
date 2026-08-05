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

    /// Prefix marking a value as vault-encrypted (versioned for future
    /// algorithm changes). Values without it are treated as legacy plaintext.
    pub const ENC_MARKER: &'static str = "vault:v1:";

    /// Encrypt and tag a value so readers can distinguish it from plaintext.
    pub fn encrypt_marked(&self, plaintext: &str) -> String {
        format!("{}{}", Self::ENC_MARKER, self.encrypt(plaintext))
    }

    /// Whether a stored value carries the encryption marker.
    pub fn is_marked(value: &str) -> bool {
        value.starts_with(Self::ENC_MARKER)
    }

    /// Decrypt a marked value; legacy plaintext passes through unchanged.
    /// Undecryptable ciphertext (rotated/lost master key) becomes EMPTY so the
    /// provider surfaces as unconfigured instead of sending garbage upstream.
    pub fn try_decrypt_marked(&self, value: &str) -> String {
        match value.strip_prefix(Self::ENC_MARKER) {
            None => value.to_string(),
            Some(ct) => self.decrypt(ct).unwrap_or_else(|e| {
                tracing::error!(
                    error = %e,
                    "failed to decrypt stored api_key (master key changed?) — treating as unset"
                );
                String::new()
            }),
        }
    }

    /// Encrypt every non-empty `api_key` in a provider config for at-rest
    /// persistence. Idempotent — already-marked values are left alone.
    pub fn encrypt_provider_api_keys(&self, pc: &mut thairag_config::schema::ProvidersConfig) {
        let mut enc = |key: &mut String| {
            if !key.is_empty() && !Self::is_marked(key) {
                *key = self.encrypt_marked(key);
            }
        };
        enc(&mut pc.llm.api_key);
        enc(&mut pc.embedding.api_key);
        enc(&mut pc.vector_store.api_key);
        enc(&mut pc.reranker.api_key);
        if let Some(vision) = pc.doc_vision_llm.as_mut() {
            enc(&mut vision.api_key);
        }
    }

    /// Reverse of [`encrypt_provider_api_keys`] for configs loaded from the
    /// store. Legacy plaintext rows pass through unchanged.
    pub fn decrypt_provider_api_keys(&self, pc: &mut thairag_config::schema::ProvidersConfig) {
        let mut dec = |key: &mut String| {
            if !key.is_empty() {
                *key = self.try_decrypt_marked(key);
            }
        };
        dec(&mut pc.llm.api_key);
        dec(&mut pc.embedding.api_key);
        dec(&mut pc.vector_store.api_key);
        dec(&mut pc.reranker.api_key);
        if let Some(vision) = pc.doc_vision_llm.as_mut() {
            dec(&mut vision.api_key);
        }
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
    fn marked_roundtrip_and_plaintext_passthrough() {
        let vault = test_vault();
        let marked = vault.encrypt_marked("sk-secret");
        assert!(Vault::is_marked(&marked));
        assert!(!marked.contains("sk-secret"));
        assert_eq!(vault.try_decrypt_marked(&marked), "sk-secret");
        // Legacy plaintext rows pass through unchanged.
        assert_eq!(
            vault.try_decrypt_marked("sk-legacy-plain"),
            "sk-legacy-plain"
        );
        // Corrupted ciphertext degrades to unset, not garbage.
        assert_eq!(vault.try_decrypt_marked("vault:v1:deadbeef"), "");
    }

    #[test]
    fn provider_api_keys_encrypt_at_rest_and_restore() {
        let vault = test_vault();
        let mut pc: thairag_config::schema::ProvidersConfig =
            serde_json::from_value(serde_json::json!({
                "llm": { "kind": "open_ai_compatible", "model": "m", "base_url": "http://gw/v1", "api_key": "sk-llm" },
                "embedding": { "kind": "open_ai", "model": "e", "dimension": 1024, "api_key": "sk-emb" },
                "vector_store": { "kind": "qdrant", "url": "http://q:6334", "collection": "c" },
                "text_search": { "kind": "tantivy", "index_path": "/tmp/x" },
                "reranker": { "kind": "passthrough" },
            }))
            .expect("test ProvidersConfig must parse");

        vault.encrypt_provider_api_keys(&mut pc);
        let json = serde_json::to_string(&pc).unwrap();
        assert!(!json.contains("sk-llm") && !json.contains("sk-emb"));
        assert!(pc.reranker.api_key.is_empty());

        // Idempotent: a second pass must not double-encrypt.
        let once = pc.llm.api_key.clone();
        vault.encrypt_provider_api_keys(&mut pc);
        assert_eq!(pc.llm.api_key, once);

        vault.decrypt_provider_api_keys(&mut pc);
        assert_eq!(pc.llm.api_key, "sk-llm");
        assert_eq!(pc.embedding.api_key, "sk-emb");
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
