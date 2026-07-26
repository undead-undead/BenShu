use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use rand::{thread_rng, RngCore};
use sha2::{Digest, Sha256};

/// AES-GCM 256 Encryption/Decryption helper
pub struct FactEncryptor {
    key: [u8; 32],
}

impl FactEncryptor {
    pub fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    /// Encrypt plaintext content using AES-GCM-256
    /// Returns a hex-encoded string containing (Nonce + Ciphertext)
    pub fn encrypt(&self, plaintext: &str) -> anyhow::Result<String> {
        Ok(format!(
            "enc:{}",
            hex::encode(self.encrypt_bytes(plaintext.as_bytes())?)
        ))
    }

    /// Decrypt hex-encoded string
    pub fn decrypt(&self, encrypted_hex: &str) -> anyhow::Result<String> {
        if !encrypted_hex.starts_with("enc:") {
            return Ok(encrypted_hex.to_string());
        }

        let combined = hex::decode(&encrypted_hex[4..])
            .map_err(|e| anyhow::anyhow!("Hex decode failed: {}", e))?;
        Ok(String::from_utf8(self.decrypt_bytes(&combined)?)?)
    }

    pub fn encrypt_bytes(&self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
        let cipher = Aes256Gcm::new(&self.key.into());
        let mut nonce_bytes = [0u8; 12];
        thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

        let mut combined = nonce_bytes.to_vec();
        combined.extend(ciphertext);
        Ok(combined)
    }

    pub fn decrypt_bytes(&self, encrypted: &[u8]) -> anyhow::Result<Vec<u8>> {
        if encrypted.len() < 12 {
            return Err(anyhow::anyhow!("Invalid encrypted payload"));
        }

        let (nonce_bytes, ciphertext) = encrypted.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);
        let cipher = Aes256Gcm::new(&self.key.into());

        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))
    }

    pub fn fingerprint(&self) -> String {
        hex::encode(Sha256::digest(self.key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fact_encryptor_roundtrip() {
        let key = [42u8; 32];
        let encryptor = FactEncryptor::new(key);

        let plaintext = "Top secret AI algorithm data: xyz123";
        let encrypted = encryptor.encrypt(plaintext).unwrap();

        // Ensure it has the prefix
        assert!(encrypted.starts_with("enc:"));
        assert_ne!(encrypted, plaintext);
        assert!(!encrypted.contains("Top secret"));

        let decrypted = encryptor.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);

        let bytes_encrypted = encryptor.encrypt_bytes(plaintext.as_bytes()).unwrap();
        let bytes_decrypted = encryptor.decrypt_bytes(&bytes_encrypted).unwrap();
        assert_eq!(bytes_decrypted, plaintext.as_bytes());
    }

    #[test]
    fn test_fact_encryptor_wrong_key() {
        let key1 = [1u8; 32];
        let key2 = [2u8; 32];

        let enc1 = FactEncryptor::new(key1);
        let enc2 = FactEncryptor::new(key2);

        let encrypted = enc1.encrypt("Secret message").unwrap();

        // Decrypting with wrong key should fail
        assert!(enc2.decrypt(&encrypted).is_err());
    }

    #[test]
    fn test_fact_encryptor_unencrypted_passthrough() {
        let key = [0u8; 32];
        let encryptor = FactEncryptor::new(key);

        let plaintext = "Just a normal string";
        let decrypted = encryptor.decrypt(plaintext).unwrap();

        assert_eq!(decrypted, plaintext);
    }
}
