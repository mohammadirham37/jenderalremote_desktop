//! Shared server state: device registry, session registry, metrics,
//! rate limiting, audit sink.

use crate::metrics::MetricsRegistry;
use crate::rate_limit::RateLimiter;
use remote_protocol::{AccessMode, DeviceId, Envelope, MessageType, PermissionSet, SessionId};
use remote_security::{
    AuditEvent, AuditResult, AuditSink, LoggingAuditSink, hash_token, new_device_token,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

/// A connected device.
pub struct DeviceEntry {
    pub display_name: String,
    pub platform: String,
    pub public_key: String,
    pub token_hash: String,
    pub last_seen_ms: u64,
    /// Channel to push envelopes into the device's WebSocket.
    pub sender: mpsc::UnboundedSender<Envelope>,
}

/// A session known to the server.
pub struct SessionEntry {
    pub controller_device_id: DeviceId,
    pub host_device_id: DeviceId,
    pub mode: AccessMode,
    pub status: String,
    pub requested_permissions: PermissionSet,
    pub granted_permissions: PermissionSet,
    pub created_at_ms: u64,
}

#[derive(Default)]
struct Registry {
    devices: HashMap<DeviceId, DeviceEntry>,
    sessions: HashMap<SessionId, SessionEntry>,
}

pub struct AppState {
    registry: RwLock<Registry>,
    pub audit: Arc<dyn AuditSink>,
    pub rate_limiter: RateLimiter,
    pub metrics_registry: MetricsRegistry,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            registry: RwLock::new(Registry::default()),
            audit: Arc::new(LoggingAuditSink),
            rate_limiter: RateLimiter::new(30.0, 10.0),
            metrics_registry: MetricsRegistry::default(),
        }
    }

    // -- devices -----------------------------------------------------------

    /// Validates a Bearer token; returns the device id when valid.
    pub fn authenticate(&self, token: &str) -> Option<DeviceId> {
        let hash = hash_token(token);
        let registry = self.registry.read().expect("registry lock");
        registry
            .devices
            .iter()
            .find(|(_, entry)| entry.token_hash == hash)
            .map(|(id, _)| id.clone())
    }

    /// Registers or refreshes a device. Returns a fresh device token, or
    /// `None` when the device id exists with a different public key
    /// (possible hijack attempt — reject, §11.3).
    pub fn register_device(
        &self,
        device_id: DeviceId,
        display_name: String,
        platform: String,
        public_key: String,
        sender: mpsc::UnboundedSender<Envelope>,
    ) -> Option<String> {
        let mut registry = self.registry.write().expect("registry lock");
        if let Some(existing) = registry.devices.get(&device_id)
            && existing.public_key != public_key
        {
            self.audit.record(
                AuditEvent::new("device.register_rejected", AuditResult::Denied)
                    .with_device(device_id.clone())
                    .with_metadata("reason", "public key mismatch"),
            );
            return None;
        }
        let (token, token_hash) = new_device_token();
        registry.devices.insert(
            device_id.clone(),
            DeviceEntry {
                display_name,
                platform,
                public_key,
                token_hash,
                last_seen_ms: now_ms(),
                sender,
            },
        );
        self.metrics_registry.inc_registered_devices();
        Some(token)
    }

    pub fn device_present(&self, device_id: &DeviceId) -> bool {
        self.registry
            .read()
            .expect("registry lock")
            .devices
            .contains_key(device_id)
    }

    pub fn sender_for(&self, device_id: &DeviceId) -> Option<mpsc::UnboundedSender<Envelope>> {
        self.registry
            .read()
            .expect("registry lock")
            .devices
            .get(device_id)
            .map(|entry| entry.sender.clone())
    }

    pub fn touch_device(&self, device_id: &DeviceId) {
        if let Some(entry) = self
            .registry
            .write()
            .expect("registry lock")
            .devices
            .get_mut(device_id)
        {
            entry.last_seen_ms = now_ms();
        }
    }

    /// Removes presence when the socket with `sender` disconnects.
    pub fn disconnect_device(
        &self,
        device_id: &DeviceId,
        sender: &mpsc::UnboundedSender<Envelope>,
    ) {
        let mut registry = self.registry.write().expect("registry lock");
        if registry
            .devices
            .get(device_id)
            .is_some_and(|entry| entry.sender.same_channel(sender))
        {
            registry.devices.remove(device_id);
        }
    }

    /// Online devices (no secrets). Devices whose socket died without a
    /// close frame are dropped lazily via this call.
    pub fn online_devices(&self) -> Vec<serde_json::Value> {
        self.registry
            .read()
            .expect("registry lock")
            .devices
            .iter()
            .map(|(id, entry)| {
                serde_json::json!({
                    "device_id": id.as_str(),
                    "display_name": entry.display_name,
                    "platform": entry.platform,
                    "last_seen_at": entry.last_seen_ms,
                })
            })
            .collect()
    }

    // -- sessions ----------------------------------------------------------

    /// Creates a session in `awaiting_approval` and returns its id.
    pub fn create_session(
        &self,
        controller: DeviceId,
        host: DeviceId,
        mode: AccessMode,
        requested: PermissionSet,
    ) -> SessionId {
        let session_id = SessionId::new(format!("sess_{}", uuid::Uuid::new_v4().simple()));
        let mut registry = self.registry.write().expect("registry lock");
        registry.sessions.insert(
            session_id.clone(),
            SessionEntry {
                controller_device_id: controller,
                host_device_id: host,
                mode,
                status: "awaiting_approval".to_string(),
                requested_permissions: requested,
                granted_permissions: PermissionSet::empty(),
                created_at_ms: now_ms(),
            },
        );
        drop(registry);
        self.metrics_registry.inc_sessions_created();
        session_id
    }

    pub fn session(&self, session_id: &SessionId) -> Option<(DeviceId, DeviceId, String)> {
        let registry = self.registry.read().expect("registry lock");
        registry.sessions.get(session_id).map(|entry| {
            (
                entry.controller_device_id.clone(),
                entry.host_device_id.clone(),
                entry.status.clone(),
            )
        })
    }

    /// Sets session status (server-side mirror of the lifecycle).
    pub fn set_session_status(&self, session_id: &SessionId, status: &str) {
        let mut registry = self.registry.write().expect("registry lock");
        if let Some(entry) = registry.sessions.get_mut(session_id) {
            entry.status = status.to_string();
        }
    }

    pub fn set_granted_permissions(&self, session_id: &SessionId, granted: PermissionSet) {
        let mut registry = self.registry.write().expect("registry lock");
        if let Some(entry) = registry.sessions.get_mut(session_id) {
            entry.granted_permissions = granted;
        }
    }

    pub fn finish_session(&self, session_id: &SessionId, status: &str) {
        self.set_session_status(session_id, status);
        self.metrics_registry.inc_sessions_ended();
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Server-side helper: envelope reply kind for a forwarded message.
pub fn reply_kind(envelope: &Envelope) -> Option<MessageType> {
    envelope.message_type().ok()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
