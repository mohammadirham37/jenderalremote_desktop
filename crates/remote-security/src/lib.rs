//! # remote-security
//!
//! Identity, policy, and audit primitives (rancangan teknis §11).
//!
//! - Device identity is an Ed25519 key pair; the private key lives in the
//!   OS credential vault, never in the database.
//! - Device ID is a public identifier and never proof of authorization.
//! - Unattended access is deny-by-default and only runs against an
//!   explicit trusted-controller policy with expiry.

pub mod audit;
pub mod identity;
pub mod policy;
pub mod secret_store;
pub mod token;

pub use audit::{AuditEvent, AuditResult, AuditSink, LoggingAuditSink};
pub use identity::DeviceIdentity;
pub use policy::{HostAccessPolicy, PolicyError, TrustedController};
pub use secret_store::{FileSecretStore, KeyringStore, SecretStore, SecretStoreError};
pub use token::{hash_token, new_device_token};

#[cfg(test)]
mod tests {
    use super::*;
    use remote_protocol::{DeviceId, Permission, PermissionSet};

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    #[test]
    fn identity_sign_verify_roundtrip() {
        let identity = DeviceIdentity::generate();
        let sig = identity.sign_message(b"challenge");
        assert!(identity.verify_message(b"challenge", &sig));
        assert!(!identity.verify_message(b"tampered", &sig));

        let spki = identity.public_key_spki_base64();
        let other = DeviceIdentity::generate();
        assert_ne!(spki, other.public_key_spki_base64());
    }

    #[test]
    fn token_hash_is_stable_and_not_reversible() {
        let (token, hash) = new_device_token();
        assert_ne!(token, hash);
        assert_eq!(hash_token(&token), hash);
    }

    #[test]
    fn unattended_disabled_by_default() {
        let policy = HostAccessPolicy::default();
        let controller = DeviceId::new("controller-1");
        let requested = PermissionSet::from_permissions([Permission::ViewScreen]);
        assert!(matches!(
            policy.authorize_unattended(&controller, &requested, now_ms()),
            Err(PolicyError::UnattendedDisabled)
        ));
    }

    #[test]
    fn unattended_requires_trusted_active_controller() {
        let controller = DeviceId::new("controller-1");
        let mut policy = HostAccessPolicy {
            unattended_enabled: true,
            ..Default::default()
        };
        let requested = PermissionSet::from_permissions([Permission::ViewScreen]);
        assert!(matches!(
            policy.authorize_unattended(&controller, &requested, now_ms()),
            Err(PolicyError::ControllerNotTrusted)
        ));

        policy.grant_trusted_controller(
            controller.clone(),
            DeviceId::new("owner"),
            PermissionSet::from_permissions([Permission::ViewScreen]),
            None,
        );
        assert!(
            policy
                .authorize_unattended(&controller, &requested, now_ms())
                .is_ok()
        );

        let too_much =
            PermissionSet::from_permissions([Permission::ViewScreen, Permission::ControlInput]);
        assert!(matches!(
            policy.authorize_unattended(&controller, &too_much, now_ms()),
            Err(PolicyError::PermissionNotGranted(_))
        ));
    }

    #[test]
    fn unattended_respects_expiry_and_revocation() {
        let controller = DeviceId::new("controller-1");
        let mut policy = HostAccessPolicy {
            unattended_enabled: true,
            ..Default::default()
        };
        let now = now_ms();
        policy.grant_trusted_controller(
            controller.clone(),
            DeviceId::new("owner"),
            PermissionSet::from_permissions([Permission::ViewScreen]),
            Some(now - 1000), // already expired
        );
        let requested = PermissionSet::from_permissions([Permission::ViewScreen]);
        assert!(matches!(
            policy.authorize_unattended(&controller, &requested, now),
            Err(PolicyError::ControllerExpired)
        ));

        policy.grant_trusted_controller(
            controller.clone(),
            DeviceId::new("owner"),
            PermissionSet::from_permissions([Permission::ViewScreen]),
            None,
        );
        policy.revoke_trusted_controller(&controller);
        assert!(matches!(
            policy.authorize_unattended(&controller, &requested, now),
            Err(PolicyError::ControllerRevoked)
        ));
    }

    #[test]
    fn file_secret_store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSecretStore::new(dir.path().to_path_buf());
        let identity = DeviceIdentity::generate();
        store.save_identity(&identity).unwrap();
        let loaded = store.load_identity().unwrap().expect("identity saved");
        assert_eq!(loaded.device_id(), identity.device_id());
        assert_eq!(
            loaded.public_key_spki_base64(),
            identity.public_key_spki_base64()
        );
        store.delete_identity().unwrap();
        assert!(store.load_identity().unwrap().is_none());
    }
}
