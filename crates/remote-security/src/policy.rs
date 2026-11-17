//! Host-side access policy (§8.2, §8.3, §11).
//!
//! Unattended access is deny-by-default: it requires the owner to enable
//! it explicitly AND to have registered the controller as trusted with a
//! permission set and optional expiry. Attended access requires a human
//! approval on the host.

use remote_protocol::{DeviceId, Permission, PermissionSet};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("unattended access is disabled on this host")]
    UnattendedDisabled,
    #[error("controller is not a trusted controller")]
    ControllerNotTrusted,
    #[error("trusted-controller grant has expired")]
    ControllerExpired,
    #[error("trusted-controller grant was revoked")]
    ControllerRevoked,
    #[error("permission {0:?} is not granted to this controller")]
    PermissionNotGranted(Permission),
    #[error("the host user denied the request")]
    Denied,
    #[error("session is not active")]
    NotActive,
}

/// A grant issued by the device owner to a specific controller device.
#[derive(Debug, Clone)]
pub struct TrustedController {
    pub controller_device_id: DeviceId,
    pub granted_by: DeviceId,
    pub permissions: PermissionSet,
    pub created_at_ms: u64,
    pub expires_at_ms: Option<u64>,
    pub revoked: bool,
}

/// Local policy state kept by the host engine. This is the authoritative
/// gate at the trust boundary — the UI is never the source of
/// authorization (§8.3).
#[derive(Debug, Clone, Default)]
pub struct HostAccessPolicy {
    pub unattended_enabled: bool,
    pub trusted_controllers: Vec<TrustedController>,
}

impl HostAccessPolicy {
    /// Attended access: the host user must approve; only approved
    /// permissions are returned (the host may narrow the request).
    pub fn authorize_attended(
        approved: bool,
        granted: &PermissionSet,
    ) -> Result<PermissionSet, PolicyError> {
        if !approved {
            return Err(PolicyError::Denied);
        }
        if !granted.contains(Permission::ViewScreen) {
            return Err(PolicyError::Denied);
        }
        Ok(granted.clone())
    }

    /// Unattended access: validate trusted-controller status, expiry, and
    /// that every requested permission is covered by the grant.
    pub fn authorize_unattended(
        &self,
        controller: &DeviceId,
        requested: &PermissionSet,
        now_ms: u64,
    ) -> Result<PermissionSet, PolicyError> {
        if !self.unattended_enabled {
            return Err(PolicyError::UnattendedDisabled);
        }
        let grant = self
            .trusted_controllers
            .iter()
            .find(|g| &g.controller_device_id == controller)
            .ok_or(PolicyError::ControllerNotTrusted)?;
        if grant.revoked {
            return Err(PolicyError::ControllerRevoked);
        }
        if let Some(expires) = grant.expires_at_ms
            && now_ms >= expires
        {
            return Err(PolicyError::ControllerExpired);
        }
        for permission in requested.iter() {
            if !grant.permissions.contains(permission) {
                return Err(PolicyError::PermissionNotGranted(permission));
            }
        }
        Ok(requested.clone())
    }

    /// Adds or refreshes a trusted-controller grant (owner action).
    pub fn grant_trusted_controller(
        &mut self,
        controller: DeviceId,
        granted_by: DeviceId,
        permissions: PermissionSet,
        expires_at_ms: Option<u64>,
    ) {
        self.trusted_controllers
            .retain(|g| g.controller_device_id != controller);
        self.trusted_controllers.push(TrustedController {
            controller_device_id: controller,
            granted_by,
            permissions,
            created_at_ms: 0,
            expires_at_ms,
            revoked: false,
        });
    }

    /// Revokes a trusted-controller grant (owner action, §11.4).
    pub fn revoke_trusted_controller(&mut self, controller: &DeviceId) {
        for grant in &mut self.trusted_controllers {
            if &grant.controller_device_id == controller {
                grant.revoked = true;
            }
        }
    }
}
