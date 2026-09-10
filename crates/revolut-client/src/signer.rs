use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SignerError {
    #[error("Invalid hex key format: {0}")]
    HexDecodeError(#[from] hex::FromHexError),
    #[error("Invalid Ed25519 private key length: expected 32 bytes")]
    InvalidKeyLength,
}

#[derive(Clone)]
pub struct Ed25519Signer {
    signing_key: SigningKey,
    api_key: String,
}

impl Ed25519Signer {
    pub fn from_hex(api_key: impl Into<String>, private_key_hex: &str) -> Result<Self, SignerError> {
        let key_bytes = hex::decode(private_key_hex)?;
        if key_bytes.len() != 32 {
            return Err(SignerError::InvalidKeyLength);
        }
        let mut array = [0u8; 32];
        array.copy_from_slice(&key_bytes);
        let signing_key = SigningKey::from_bytes(&array);
        Ok(Self {
            signing_key,
            api_key: api_key.into(),
        })
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Sign canonical payload: timestamp + method + path + body
    pub fn sign_payload(&self, timestamp_ms: i64, method: &str, path: &str, body: &str) -> String {
        let canonical_str = format!("{}{}{}{}", timestamp_ms, method.to_uppercase(), path, body);
        let signature = self.signing_key.sign(canonical_str.as_bytes());
        BASE64.encode(signature.to_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ed25519_signer() {
        // 32-byte dummy hex key
        let hex_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let signer = Ed25519Signer::from_hex("revx_api_key_123", hex_key).unwrap();

        let sig = signer.sign_payload(1700000000000, "POST", "/api/v1/orders", "{\"symbol\":\"BTC-GBP\"}");
        assert!(!sig.is_empty());
        assert_eq!(signer.api_key(), "revx_api_key_123");
    }
}
