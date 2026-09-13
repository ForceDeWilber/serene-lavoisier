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

    pub fn from_pem(api_key: impl Into<String>, pem_str: &str) -> Result<Self, SignerError> {
        // Try PKCS8 PEM decoding first
        if let Ok(key) = ed25519_dalek::pkcs8::DecodePrivateKey::from_pkcs8_pem(pem_str) {
            return Ok(Self {
                signing_key: key,
                api_key: api_key.into(),
            });
        }

        // Fallback: extract base64 from PEM and check for 32-byte seed
        let stripped: String = pem_str
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect::<Vec<_>>()
            .concat();

        let der = BASE64.decode(stripped.trim()).map_err(|_| SignerError::InvalidKeyLength)?;
        if der.len() == 32 {
            let mut array = [0u8; 32];
            array.copy_from_slice(&der);
            return Ok(Self {
                signing_key: SigningKey::from_bytes(&array),
                api_key: api_key.into(),
            });
        }

        // In OpenSSL Ed25519 PKCS8 (48 bytes), last 32 bytes is the private key seed
        if der.len() >= 32 {
            let seed_slice = &der[der.len() - 32..];
            let mut array = [0u8; 32];
            array.copy_from_slice(seed_slice);
            return Ok(Self {
                signing_key: SigningKey::from_bytes(&array),
                api_key: api_key.into(),
            });
        }

        Err(SignerError::InvalidKeyLength)
    }

    pub fn from_file_or_hex(api_key: impl Into<String>, key_source: &str) -> Result<Self, SignerError> {
        let api_key_str = api_key.into();
        let path = std::path::Path::new(key_source);
        if path.exists() {
            let content = std::fs::read_to_string(path).map_err(|_| SignerError::InvalidKeyLength)?;
            if content.contains("-----BEGIN") {
                return Self::from_pem(api_key_str, &content);
            } else {
                return Self::from_hex(api_key_str, content.trim());
            }
        }

        if key_source.contains("-----BEGIN") {
            Self::from_pem(api_key_str, key_source)
        } else {
            Self::from_hex(api_key_str, key_source.trim())
        }
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Sign canonical payload: timestamp + method + path (without '?') + query + body
    pub fn sign_payload(&self, timestamp_ms: i64, method: &str, path: &str, body: &str) -> String {
        let (clean_path, query) = match path.split_once('?') {
            Some((p, q)) => (p, q),
            None => (path, ""),
        };
        let canonical_str = format!("{}{}{}{}{}", timestamp_ms, method.to_uppercase(), clean_path, query, body);
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

    #[test]
    fn test_load_real_pem_key() {
        let pem_path = "../../backend/credentials/revolut_private.pem";
        if std::path::Path::new(pem_path).exists() {
            let signer = Ed25519Signer::from_file_or_hex("test_key", pem_path);
            assert!(signer.is_ok(), "Failed to load real PEM key: {:?}", signer.err());
        }
    }
}
