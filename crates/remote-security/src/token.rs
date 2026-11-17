//! Device/session tokens (§11.1, §11.2).
//!
//! The server stores only the SHA-256 hash of a token. Tokens are
//! high-entropy random values; the hash comparison is not
//! timing-sensitive because attackers never control the stored target.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// Generates a new 192-bit token. Returns `(plaintext, sha256_hash)`,
/// both base64url-encoded.
pub fn new_device_token() -> (String, String) {
    let mut bytes = [0u8; 24];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);
    let hash = hash_token(&token);
    (token, hash)
}

/// Computes the stored form of a token.
pub fn hash_token(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique() {
        let (a, _) = new_device_token();
        let (b, _) = new_device_token();
        assert_ne!(a, b);
    }
}
