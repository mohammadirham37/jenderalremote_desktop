//! Audit events (§10.6, §11).
//!
//! Audit records must never contain passwords, session tokens, private
//! keys, clipboard or file payloads. `redact` is applied defensively at
//! the sink.

use remote_protocol::{DeviceId, SessionId};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditResult {
    Allowed,
    Denied,
    Success,
    Failure,
}

impl AuditResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuditResult::Allowed => "allowed",
            AuditResult::Denied => "denied",
            AuditResult::Success => "success",
            AuditResult::Failure => "failure",
        }
    }
}

/// A security-relevant event. `metadata` must already be redacted.
#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub event_type: String,
    pub actor: Option<DeviceId>,
    pub device: Option<DeviceId>,
    pub session: Option<SessionId>,
    pub result: AuditResult,
    pub created_at_ms: u64,
    pub metadata: BTreeMap<String, String>,
}

impl AuditEvent {
    pub fn new(event_type: impl Into<String>, result: AuditResult) -> Self {
        Self {
            event_type: event_type.into(),
            actor: None,
            device: None,
            session: None,
            result,
            created_at_ms: now_ms(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn with_actor(mut self, actor: DeviceId) -> Self {
        self.actor = Some(actor);
        self
    }

    pub fn with_device(mut self, device: DeviceId) -> Self {
        self.device = Some(device);
        self
    }

    pub fn with_session(mut self, session: SessionId) -> Self {
        self.session = Some(session);
        self
    }

    pub fn with_metadata(mut self, key: &str, value: impl Into<String>) -> Self {
        self.metadata.insert(key.to_string(), value.into());
        self
    }
}

/// Replaces anything that looks like a secret before logging.
pub fn redact(value: &str) -> String {
    if value.contains("token") || value.contains("secret") || value.contains("password") {
        "«redacted»".to_string()
    } else {
        value.to_string()
    }
}

/// Sink for audit events. Implementations: structured logging now,
/// PostgreSQL in the signaling service.
pub trait AuditSink: Send + Sync {
    fn record(&self, event: AuditEvent);
}

/// Default sink: structured `tracing` output. No secrets are logged.
#[derive(Debug, Default, Clone, Copy)]
pub struct LoggingAuditSink;

impl AuditSink for LoggingAuditSink {
    fn record(&self, event: AuditEvent) {
        let metadata: BTreeMap<String, String> = event
            .metadata
            .iter()
            .map(|(k, v)| (k.clone(), redact(v)))
            .collect();
        tracing::info!(
            target: "jenderalremote::audit",
            event_type = %event.event_type,
            result = event.result.as_str(),
            actor = event.actor.as_ref().map(|a| a.as_str()).unwrap_or("-"),
            device = event.device.as_ref().map(|d| d.as_str()).unwrap_or("-"),
            session = event.session.as_ref().map(|s| s.as_str()).unwrap_or("-"),
            metadata = ?metadata,
            "audit"
        );
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_hides_secret_like_values() {
        assert_eq!(redact("session-token-value"), "«redacted»");
        assert_eq!(redact("plain-reason"), "plain-reason");
    }
}
