//! State machine unit tests (rancangan teknis §13.1).

use crate::session::SessionStatus;
use crate::state_machine::{SessionEvent, SessionStateMachine, target_state};
use remote_protocol::{Permission, PermissionSet};

fn full_perms() -> PermissionSet {
    PermissionSet::from_permissions([Permission::ViewScreen, Permission::ControlInput])
}

fn view_only() -> PermissionSet {
    PermissionSet::from_permissions([Permission::ViewScreen])
}

/// Happy path from Idle to Ended.
#[test]
fn attended_happy_path() {
    let mut sm = SessionStateMachine::new("sess-1");
    assert_eq!(sm.status(), SessionStatus::Idle);

    sm.apply(SessionEvent::RequestSent).unwrap();
    sm.apply(SessionEvent::ApprovalRequired).unwrap();
    assert!(!sm.input_allowed(), "no input before approval");
    sm.apply(SessionEvent::Approved {
        permissions: full_perms(),
    })
    .unwrap();
    sm.apply(SessionEvent::NegotiationStarted).unwrap();
    sm.apply(SessionEvent::ConnectionAttemptStarted).unwrap();
    assert!(!sm.input_allowed(), "no input while connecting");
    sm.apply(SessionEvent::Connected).unwrap();
    assert_eq!(sm.status(), SessionStatus::Active);
    assert!(sm.input_allowed());

    sm.apply(SessionEvent::EndRequested).unwrap();
    assert_eq!(sm.status(), SessionStatus::Ending);
    assert!(!sm.input_allowed(), "no input while ending");
    sm.apply(SessionEvent::Ended).unwrap();
    assert_eq!(sm.status(), SessionStatus::Ended);
    assert!(sm.status().is_terminal());
}

#[test]
fn rejection_paths() {
    let mut sm = SessionStateMachine::new("s");
    sm.apply(SessionEvent::RequestSent).unwrap();
    sm.apply(SessionEvent::Rejected).unwrap();
    assert_eq!(sm.status(), SessionStatus::Rejected);

    let mut sm = SessionStateMachine::new("s");
    sm.apply(SessionEvent::RequestSent).unwrap();
    sm.apply(SessionEvent::ApprovalRequired).unwrap();
    sm.apply(SessionEvent::Rejected).unwrap();
    assert_eq!(sm.status(), SessionStatus::Rejected);
}

#[test]
fn negotiation_and_connecting_failures() {
    for stop_at_connecting in [false, true] {
        let mut sm = SessionStateMachine::new("s");
        sm.apply(SessionEvent::RequestSent).unwrap();
        sm.apply(SessionEvent::ApprovalRequired).unwrap();
        sm.apply(SessionEvent::Approved {
            permissions: view_only(),
        })
        .unwrap();
        sm.apply(SessionEvent::NegotiationStarted).unwrap();
        if stop_at_connecting {
            sm.apply(SessionEvent::ConnectionAttemptStarted).unwrap();
        }
        sm.apply(SessionEvent::ConnectionFailed).unwrap();
        assert_eq!(sm.status(), SessionStatus::Failed);
    }
}

#[test]
fn disconnect_reconnect_loop() {
    let mut sm = SessionStateMachine::new("s");
    for event in [
        SessionEvent::RequestSent,
        SessionEvent::ApprovalRequired,
        SessionEvent::Approved {
            permissions: full_perms(),
        },
        SessionEvent::NegotiationStarted,
        SessionEvent::ConnectionAttemptStarted,
        SessionEvent::Connected,
    ] {
        sm.apply(event).unwrap();
    }
    sm.apply(SessionEvent::Disconnected).unwrap();
    assert!(!sm.input_allowed());
    sm.apply(SessionEvent::ReconnectStarted).unwrap();
    sm.apply(SessionEvent::Reconnected).unwrap();
    assert_eq!(sm.status(), SessionStatus::Active);
    assert!(sm.input_allowed());
}

#[test]
fn revoked_from_active() {
    let mut sm = SessionStateMachine::new("s");
    for event in [
        SessionEvent::RequestSent,
        SessionEvent::ApprovalRequired,
        SessionEvent::Approved {
            permissions: full_perms(),
        },
        SessionEvent::NegotiationStarted,
        SessionEvent::Connected,
    ] {
        sm.apply(event).unwrap();
    }
    sm.apply(SessionEvent::Revoked).unwrap();
    assert_eq!(sm.status(), SessionStatus::Revoked);
    // Terminal: no further transitions.
    assert!(sm.apply(SessionEvent::Connected).is_err());
}

#[test]
fn invalid_transitions_rejected() {
    let mut sm = SessionStateMachine::new("s");
    assert!(sm.apply(SessionEvent::Connected).is_err());
    assert!(
        sm.apply(SessionEvent::Approved {
            permissions: full_perms()
        })
        .is_err()
    );

    sm.apply(SessionEvent::RequestSent).unwrap();
    // Cannot skip approval.
    assert!(sm.apply(SessionEvent::NegotiationStarted).is_err());
    assert!(sm.apply(SessionEvent::Connected).is_err());
}

#[test]
fn approval_without_view_screen_rejected() {
    let mut sm = SessionStateMachine::new("s");
    sm.apply(SessionEvent::RequestSent).unwrap();
    sm.apply(SessionEvent::ApprovalRequired).unwrap();
    let no_view = PermissionSet::from_permissions([Permission::ControlInput]);
    assert!(
        sm.apply(SessionEvent::Approved {
            permissions: no_view
        })
        .is_err()
    );
    assert_eq!(sm.status(), SessionStatus::AwaitingApproval);
}

#[test]
fn input_blocked_without_control_permission() {
    let mut sm = SessionStateMachine::new("s");
    for event in [
        SessionEvent::RequestSent,
        SessionEvent::ApprovalRequired,
        SessionEvent::Approved {
            permissions: view_only(),
        },
        SessionEvent::NegotiationStarted,
        SessionEvent::Connected,
    ] {
        sm.apply(event).unwrap();
    }
    assert_eq!(sm.status(), SessionStatus::Active);
    assert!(
        !sm.input_allowed(),
        "view-only session must not accept input"
    );
    assert!(sm.view_allowed());
}

#[test]
fn runtime_permission_narrowing() {
    let mut sm = SessionStateMachine::new("s");
    for event in [
        SessionEvent::RequestSent,
        SessionEvent::ApprovalRequired,
        SessionEvent::Approved {
            permissions: full_perms(),
        },
        SessionEvent::NegotiationStarted,
        SessionEvent::Connected,
    ] {
        sm.apply(event).unwrap();
    }
    assert!(sm.input_allowed());
    sm.revoke_permission(Permission::ControlInput);
    assert!(!sm.input_allowed());
    assert!(sm.view_allowed());
}

#[test]
fn target_state_table_is_exhaustive_for_happy_path() {
    use SessionStatus as S;
    assert_eq!(
        target_state(S::Idle, &SessionEvent::RequestSent),
        Some(S::Requesting)
    );
    assert_eq!(target_state(S::Ended, &SessionEvent::RequestSent), None);
    assert_eq!(target_state(S::Idle, &SessionEvent::Ended), None);
}
