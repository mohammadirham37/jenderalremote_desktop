//! Device identity based on an Ed25519 key pair (§11.1).

use crate::secret_store::SecretStore;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
use remote_protocol::DeviceId;

/// DER prefix for an Ed25519 SubjectPublicKeyInfo (RFC 8410).
const ED25519_SPKI_PREFIX: &[u8] = &[
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

/// A device identity. The signing key is held in memory only; persistence
/// goes through a [`SecretStore`] (OS vault).
#[derive(Debug, Clone)]
pub struct DeviceIdentity {
    signing_key: SigningKey,
    device_id: DeviceId,
}

impl DeviceIdentity {
    /// Generates a fresh identity key pair from an OS-sourced seed.
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut seed);
        Self::from_seed(seed)
    }

    /// Restores an identity from a 32-byte seed previously stored in a
    /// secret store.
    pub fn from_seed(seed: [u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(&seed);
        let device_id = derive_device_id(&signaling_bytes(&signing_key));
        Self {
            signing_key,
            device_id,
        }
    }

    pub fn device_id(&self) -> &DeviceId {
        &self.device_id
    }

    pub fn seed(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Public key as base64 SPKI DER, suitable for `device.register`.
    pub fn public_key_spki_base64(&self) -> String {
        let vk = self.verifying_key();
        let mut spki = Vec::with_capacity(ED25519_SPKI_PREFIX.len() + 32);
        spki.extend_from_slice(ED25519_SPKI_PREFIX);
        spki.extend_from_slice(&vk.to_bytes());
        STANDARD.encode(spki)
    }

    pub fn sign_message(&self, message: &[u8]) -> Vec<u8> {
        self.signing_key.sign(message).to_bytes().to_vec()
    }

    pub fn verify_message(&self, message: &[u8], signature: &[u8]) -> bool {
        let Ok(sig) = ed25519_dalek::Signature::from_slice(signature) else {
            return false;
        };
        self.verifying_key().verify(message, &sig).is_ok()
    }

    /// Persists the identity seed into `store`.
    pub fn save_to(
        &self,
        store: &dyn SecretStore,
    ) -> Result<(), crate::secret_store::SecretStoreError> {
        store.save_identity(self)
    }

    /// Loads the identity from `store`, if one was saved.
    pub fn load_from(
        store: &dyn SecretStore,
    ) -> Result<Option<Self>, crate::secret_store::SecretStoreError> {
        store.load_identity()
    }
}

/// Stable, non-secret device id derived from the public key:
/// `dev-` + first 16 hex chars of SHA-256(public key).
fn derive_device_id(public: &[u8]) -> DeviceId {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(public);
    DeviceId::new(format!("dev-{}", hex::encode(&digest[..8])))
}

fn signaling_bytes(key: &SigningKey) -> [u8; 32] {
    key.verifying_key().to_bytes()
}
