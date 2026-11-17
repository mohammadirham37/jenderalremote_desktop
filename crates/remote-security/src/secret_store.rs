//! Secret storage abstraction.
//!
//! Preferred backend: the OS credential vault via the `keyring` crate
//! (macOS Keychain, Windows Credential Manager, Linux Secret Service).
//! A file-based store is provided for development and CI only — it
//! refuses to be used silently and logs a warning.

use crate::identity::DeviceIdentity;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretStoreError {
    #[error("keychain error: {0}")]
    Keyring(#[from] keyring::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("stored identity is corrupted: {0}")]
    Corrupted(String),
}

/// Where the device identity seed lives. Never in a database or log.
pub trait SecretStore: Send + Sync {
    fn save_identity(&self, identity: &DeviceIdentity) -> Result<(), SecretStoreError>;
    fn load_identity(&self) -> Result<Option<DeviceIdentity>, SecretStoreError>;
    fn delete_identity(&self) -> Result<(), SecretStoreError>;
}

/// OS credential vault backend.
pub struct KeyringStore {
    service: String,
    account: String,
}

impl KeyringStore {
    pub fn new(service: impl Into<String>, account: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            account: account.into(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, SecretStoreError> {
        Ok(keyring::Entry::new(&self.service, &self.account)?)
    }
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self::new("jenderalremote.desktop", "device-identity")
    }
}

impl SecretStore for KeyringStore {
    fn save_identity(&self, identity: &DeviceIdentity) -> Result<(), SecretStoreError> {
        let entry = self.entry()?;
        entry.set_password(&STANDARD.encode(identity.seed()))?;
        Ok(())
    }

    fn load_identity(&self) -> Result<Option<DeviceIdentity>, SecretStoreError> {
        let entry = self.entry()?;
        match entry.get_password() {
            Ok(encoded) => {
                let bytes = STANDARD
                    .decode(encoded)
                    .map_err(|e| SecretStoreError::Corrupted(e.to_string()))?;
                let seed: [u8; 32] = bytes
                    .try_into()
                    .map_err(|_| SecretStoreError::Corrupted("seed must be 32 bytes".into()))?;
                Ok(Some(DeviceIdentity::from_seed(seed)))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn delete_identity(&self) -> Result<(), SecretStoreError> {
        let entry = self.entry()?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// Development-only file store. Permissions are tightened to 0600 on unix;
/// production must use [`KeyringStore`].
pub struct FileSecretStore {
    path: PathBuf,
}

impl FileSecretStore {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            path: directory.join("device-identity.seed"),
        }
    }
}

impl SecretStore for FileSecretStore {
    fn save_identity(&self, identity: &DeviceIdentity) -> Result<(), SecretStoreError> {
        tracing::warn!(
            path = %self.path.display(),
            "FileSecretStore is a development backend; use the OS keychain in production"
        );
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, identity.seed())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    fn load_identity(&self) -> Result<Option<DeviceIdentity>, SecretStoreError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => {
                let seed: [u8; 32] = bytes.try_into().map_err(|_| {
                    SecretStoreError::Corrupted("seed file must be 32 bytes".into())
                })?;
                Ok(Some(DeviceIdentity::from_seed(seed)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn delete_identity(&self) -> Result<(), SecretStoreError> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
