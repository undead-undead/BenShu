use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::warn;

type HmacSha256 = Hmac<Sha256>;

/// Utility for verifying request signatures to prevent tampering in local IPC.
pub struct AntiTamper;

impl AntiTamper {
    /// Verify a signature against a message and a shared secret.
    /// Signature should be a hex-encoded HMAC-SHA256.
    pub fn verify(secret: &[u8], message: &[u8], signature: &str) -> bool {
        let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC can take key of any size");
        mac.update(message);

        let sig_bytes = match hex::decode(signature) {
            Ok(b) => b,
            Err(_) => return false,
        };

        if mac.verify_slice(&sig_bytes).is_ok() {
            true
        } else {
            warn!("Anti-Tamper: Signature mismatch detected!");
            false
        }
    }

    /// Sign a message with a shared secret.
    pub fn sign(secret: &[u8], message: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC can take key of any size");
        mac.update(message);
        let result = mac.finalize();
        hex::encode(result.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anti_tamper_sign_verify() {
        let secret = b"super_secret_key";
        let message = b"{\"action\":\"delete_database\"}";

        // Sign
        let signature = AntiTamper::sign(secret, message);
        assert!(!signature.is_empty());

        // Verify with correct secret and message
        assert!(AntiTamper::verify(secret, message, &signature));

        // Verify with wrong secret
        assert!(!AntiTamper::verify(b"wrong_key", message, &signature));

        // Verify with tampered message
        let tampered_msg = b"{\"action\":\"delete_all_databases\"}";
        assert!(!AntiTamper::verify(secret, tampered_msg, &signature));

        // Verify with invalid hex signature
        assert!(!AntiTamper::verify(secret, message, "not_a_hex_string"));
    }
}
