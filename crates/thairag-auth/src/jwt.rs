use chrono::Utc;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use thairag_core::ThaiRagError;

use crate::claims::AuthClaims;

pub struct JwtService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    expiry_hours: u64,
}

impl JwtService {
    pub fn new(secret: &str, expiry_hours: u64) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(secret.as_bytes()),
            expiry_hours,
        }
    }

    pub fn encode(&self, sub: &str, email: &str) -> Result<String, ThaiRagError> {
        let now = Utc::now().timestamp() as usize;
        let claims = AuthClaims {
            sub: sub.to_string(),
            email: email.to_string(),
            iat: now,
            exp: now + (self.expiry_hours as usize * 3600),
        };
        encode(&Header::default(), &claims, &self.encoding_key)
            .map_err(|e| ThaiRagError::Auth(e.to_string()))
    }

    pub fn decode(&self, token: &str) -> Result<AuthClaims, ThaiRagError> {
        let data = decode::<AuthClaims>(token, &self.decoding_key, &Validation::default())
            .map_err(|e| ThaiRagError::Auth(e.to_string()))?;
        Ok(data.claims)
    }
}
