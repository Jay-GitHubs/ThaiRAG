use chrono::Utc;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use thairag_core::ThaiRagError;

use crate::claims::AuthClaims;

/// Short-lived, single-document grant embedded in a citation link's `token`
/// query param. Lets a browser open a cited source without an auth header —
/// the signed token authorizes exactly one `doc` until `exp`.
#[derive(Debug, Serialize, Deserialize)]
struct CitationClaims {
    doc: String,
    iat: usize,
    exp: usize,
}

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

    /// Mint a signed, time-limited token scoped to a single document, for use
    /// in clickable citation links.
    pub fn encode_citation(&self, doc_id: &str, ttl_hours: u64) -> Result<String, ThaiRagError> {
        let now = Utc::now().timestamp() as usize;
        let claims = CitationClaims {
            doc: doc_id.to_string(),
            iat: now,
            exp: now + (ttl_hours as usize * 3600),
        };
        encode(&Header::default(), &claims, &self.encoding_key)
            .map_err(|e| ThaiRagError::Auth(e.to_string()))
    }

    /// Validate a citation token (signature + expiry) and return the doc id it
    /// grants access to.
    pub fn decode_citation(&self, token: &str) -> Result<String, ThaiRagError> {
        let data = decode::<CitationClaims>(token, &self.decoding_key, &Validation::default())
            .map_err(|e| ThaiRagError::Auth(e.to_string()))?;
        Ok(data.claims.doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn citation_token_round_trips() {
        let svc = JwtService::new("test-secret", 24);
        let token = svc.encode_citation("doc-123", 24).unwrap();
        assert_eq!(svc.decode_citation(&token).unwrap(), "doc-123");
    }

    #[test]
    fn citation_token_rejected_under_wrong_secret() {
        let signer = JwtService::new("real-secret", 24);
        let attacker = JwtService::new("other-secret", 24);
        let token = signer.encode_citation("doc-123", 24).unwrap();
        assert!(attacker.decode_citation(&token).is_err());
    }

    #[test]
    fn tampered_citation_token_rejected() {
        let svc = JwtService::new("test-secret", 24);
        let mut token = svc.encode_citation("doc-123", 24).unwrap();
        // Flip a character in the signature segment.
        let last = token.pop().unwrap();
        token.push(if last == 'a' { 'b' } else { 'a' });
        assert!(svc.decode_citation(&token).is_err());
    }
}
