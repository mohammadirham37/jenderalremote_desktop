//! Access modes and per-feature permissions (rancangan teknis §8.3).

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// How a session is authorized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessMode {
    /// Host user must explicitly approve each session.
    Attended,
    /// Pre-authorized via a trusted-controller policy configured by the owner.
    Unattended,
}

/// A capability that can be requested/granted for a session.
///
/// Defaults follow the permission model in §8.3: clipboard and audio are
/// disabled until explicitly allowed, file transfer and administrative
/// commands are out of MVP scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    ViewScreen,
    ControlInput,
    #[allow(dead_code)]
    Clipboard,
    #[allow(dead_code)]
    Audio,
}

impl Permission {
    pub fn as_str(&self) -> &'static str {
        match self {
            Permission::ViewScreen => "view_screen",
            Permission::ControlInput => "control_input",
            Permission::Clipboard => "clipboard",
            Permission::Audio => "audio",
        }
    }
}

/// A set of permissions. `ViewScreen` is required for any remote session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionSet(BTreeSet<Permission>);

impl PermissionSet {
    pub fn empty() -> Self {
        Self(BTreeSet::new())
    }

    pub fn from_permissions<I: IntoIterator<Item = Permission>>(perms: I) -> Self {
        Self(perms.into_iter().collect())
    }

    pub fn grant(&mut self, perm: Permission) {
        self.0.insert(perm);
    }

    pub fn revoke(&mut self, perm: Permission) {
        self.0.remove(&perm);
    }

    pub fn contains(&self, perm: Permission) -> bool {
        self.0.contains(&perm)
    }

    pub fn is_subset_of(&self, other: &PermissionSet) -> bool {
        self.0.is_subset(&other.0)
    }

    pub fn iter(&self) -> impl Iterator<Item = Permission> + '_ {
        self.0.iter().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Validates a requested permission set. Returns an error message when the
/// request is invalid for an MVP session (e.g. missing `view_screen`).
pub fn validate_request(requested: &PermissionSet) -> Result<(), &'static str> {
    if !requested.contains(Permission::ViewScreen) {
        return Err("view_screen permission is required for every session");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_json_uses_snake_case() {
        let p = Permission::ControlInput;
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, "\"control_input\"");
    }

    #[test]
    fn view_screen_is_mandatory() {
        let mut set = PermissionSet::empty();
        set.grant(Permission::ControlInput);
        assert!(validate_request(&set).is_err());

        set.grant(Permission::ViewScreen);
        assert!(validate_request(&set).is_ok());
    }

    #[test]
    fn subset_check() {
        let a = PermissionSet::from_permissions([Permission::ViewScreen]);
        let b = PermissionSet::from_permissions([Permission::ViewScreen, Permission::ControlInput]);
        assert!(a.is_subset_of(&b));
        assert!(!b.is_subset_of(&a));
    }
}
