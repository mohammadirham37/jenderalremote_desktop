//! Validated session state machine (rancangan teknis §5.3).
//!
//! ```text
//! Idle -> Requesting -> AwaitingApproval -> Negotiating -> Connecting -> Active -> Ending -> Ended
//! Requesting/AwaitingApproval -> Rejected
//! Negotiating/Connecting -> Failed
//! Active -> Revoked
//! Active -> Disconnected -> Reconnecting -> Active
//! ```
//!
//! Every transition is validated. Input events are only allowed while the
//! session is `Active` **and** `control_input` was granted — never during
//! `Negotiating` or `AwaitingApproval`.

use crate::session::SessionStatus;
use remote_protocol::{Permission, PermissionSet};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug, Error)]
#[error("invalid transition: {from} cannot handle {event} (session {session_id})")]
pub struct StateMachineError {
    pub from: SessionStatus,
    pub event: &'static str,
    pub session_id: String,
}

/// Events driving the session lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    /// Controller sent `session.request`.
    RequestSent,
    /// Host received a `session.request` (host-side entry point).
    IncomingRequest,
    /// Server routed the request to the host; waiting for the host user.
    ApprovalRequired,
    /// Host approved (possibly with narrowed permissions).
    Approved { permissions: PermissionSet },
    /// Host or server rejected the request.
    Rejected,
    /// SDP offer/answer exchange started.
    NegotiationStarted,
    /// ICE/DTLS connection attempt underway.
    ConnectionAttemptStarted,
    /// Peer connection established; media may flow.
    Connected,
    /// Connection attempt or negotiation failed.
    ConnectionFailed,
    /// Live connection dropped.
    Disconnected,
    /// Reconnection attempt started after a drop.
    ReconnectStarted,
    /// Reconnection succeeded.
    Reconnected,
    /// Local side requested termination.
    EndRequested,
    /// Session terminated (either side, server, or teardown complete).
    Ended,
    /// Authorization withdrawn while the session was live or pending.
    Revoked,
}

impl SessionEvent {
    fn name(&self) -> &'static str {
        match self {
            SessionEvent::RequestSent => "request_sent",
            SessionEvent::IncomingRequest => "incoming_request",
            SessionEvent::ApprovalRequired => "approval_required",
            SessionEvent::Approved { .. } => "approved",
            SessionEvent::Rejected => "rejected",
            SessionEvent::NegotiationStarted => "negotiation_started",
            SessionEvent::ConnectionAttemptStarted => "connection_attempt_started",
            SessionEvent::Connected => "connected",
            SessionEvent::ConnectionFailed => "connection_failed",
            SessionEvent::Disconnected => "disconnected",
            SessionEvent::ReconnectStarted => "reconnect_started",
            SessionEvent::Reconnected => "reconnected",
            SessionEvent::EndRequested => "end_requested",
            SessionEvent::Ended => "ended",
            SessionEvent::Revoked => "revoked",
        }
    }
}

/// Computes the target state for `(current, event)`; `None` means the
/// transition is invalid.
pub fn target_state(current: SessionStatus, event: &SessionEvent) -> Option<SessionStatus> {
    use SessionEvent as E;
    use SessionStatus as S;
    Some(match (current, event) {
        (S::Idle, E::RequestSent) => S::Requesting,
        (S::Idle, E::IncomingRequest) => S::AwaitingApproval,
        (S::Requesting, E::ApprovalRequired) => S::AwaitingApproval,
        (S::AwaitingApproval, E::Approved { .. }) => S::Negotiating,
        // The host engine emits this when it begins the SDP exchange;
        // approval has already moved the session into Negotiating.
        (S::Negotiating, E::NegotiationStarted) => S::Negotiating,
        (S::Requesting | S::AwaitingApproval, E::Rejected) => S::Rejected,
        (S::Negotiating, E::ConnectionAttemptStarted) => S::Connecting,
        (S::Connecting, E::Connected) => S::Active,
        (S::Negotiating, E::Connected) => S::Active,
        (S::Negotiating | S::Connecting | S::Reconnecting, E::ConnectionFailed) => S::Failed,
        (S::Active, E::Disconnected) => S::Disconnected,
        (S::Disconnected, E::ReconnectStarted) => S::Reconnecting,
        (S::Reconnecting, E::Reconnected) => S::Active,
        (S::Active, E::EndRequested) => S::Ending,
        (S::Ending, E::Ended) => S::Ended,
        (S::Active | S::Disconnected, E::Ended) => S::Ended,
        (
            S::AwaitingApproval | S::Negotiating | S::Connecting | S::Active | S::Reconnecting,
            E::Revoked,
        ) => S::Revoked,
        // The host may terminate without an explicit local "ending" phase.
        (S::AwaitingApproval | S::Negotiating | S::Connecting | S::Reconnecting, E::Ended) => {
            S::Ended
        }
        _ => return None,
    })
}

/// Validated, permission-aware session state machine.
#[derive(Debug)]
pub struct SessionStateMachine {
    session_id: String,
    status: SessionStatus,
    permissions: PermissionSet,
    updated_at_ms: u64,
    transitions: u32,
}

impl SessionStateMachine {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            status: SessionStatus::Idle,
            permissions: PermissionSet::empty(),
            updated_at_ms: now_ms(),
            transitions: 0,
        }
    }

    pub fn status(&self) -> SessionStatus {
        self.status
    }

    pub fn permissions(&self) -> &PermissionSet {
        &self.permissions
    }

    pub fn transition_count(&self) -> u32 {
        self.transitions
    }

    /// Applies an event, returning the new status.
    pub fn apply(&mut self, event: SessionEvent) -> Result<SessionStatus, StateMachineError> {
        if let SessionEvent::Approved { permissions } = &event
            && !permissions.contains(Permission::ViewScreen)
        {
            // An approval without view_screen cannot produce a session.
            return Err(StateMachineError {
                from: self.status,
                event: event.name(),
                session_id: self.session_id.clone(),
            });
        }
        let next = target_state(self.status, &event).ok_or_else(|| StateMachineError {
            from: self.status,
            event: event.name(),
            session_id: self.session_id.clone(),
        })?;
        if let SessionEvent::Approved { permissions } = event {
            self.permissions = permissions;
        }
        self.status = next;
        self.updated_at_ms = now_ms();
        self.transitions += 1;
        tracing::debug!(
            session = %self.session_id,
            status = next.as_str(),
            "session state transition"
        );
        Ok(next)
    }

    /// Narrow permissions while the session runs (e.g. clipboard revoked).
    pub fn revoke_permission(&mut self, permission: Permission) {
        self.permissions.revoke(permission);
    }

    /// Input is only allowed while Active **and** `control_input` granted.
    pub fn input_allowed(&self) -> bool {
        self.status == SessionStatus::Active && self.permissions.contains(Permission::ControlInput)
    }

    pub fn view_allowed(&self) -> bool {
        self.status == SessionStatus::Active && self.permissions.contains(Permission::ViewScreen)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
