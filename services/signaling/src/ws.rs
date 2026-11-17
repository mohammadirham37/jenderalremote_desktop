//! WebSocket endpoint: authentication, schema/rate validation, session
//! routing, and WebRTC relay (§7.1, §7.2).
//!
//! Identity comes ONLY from the authenticated connection — client-sent
//! identity fields are never trusted (§7.1).

use crate::state::AppState;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use remote_protocol::{
    DeviceRegistered, Envelope, ErrorPayload, MessageType, PROTOCOL_VERSION, Payload,
    SessionRequest, SessionStateChange,
};
use remote_security::{AuditEvent, AuditResult};
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_string);

    let auth_device = token.as_deref().and_then(|t| state.authenticate(t));
    if token.is_some() && auth_device.is_none() {
        // A presented-but-invalid token is suspicious: refuse early.
        return (axum::http::StatusCode::UNAUTHORIZED, "invalid device token").into_response();
    }

    ws.on_upgrade(move |socket| handle_socket(socket, state, auth_device))
}

async fn handle_socket(
    socket: WebSocket,
    state: Arc<AppState>,
    mut auth_device: Option<remote_protocol::DeviceId>,
) {
    state.metrics_registry.inc_ws_connections();
    let (mut writer, mut reader) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Envelope>();

    // Writer task: pushes envelopes to the client.
    let writer_task = tokio::spawn(async move {
        while let Some(envelope) = out_rx.recv().await {
            if writer
                .send(Message::Text(envelope.to_json().into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    while let Some(message) = reader.next().await {
        let Ok(message) = message else { break };
        match message {
            Message::Text(text) => {
                let reply = process_text(&state, &out_tx, &mut auth_device, text.as_bytes());
                if let Some(envelope) = reply {
                    let _ = out_tx.send(envelope);
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    if let Some(device) = &auth_device {
        state.disconnect_device(device, &out_tx);
        state.audit.record(
            AuditEvent::new("device.disconnected", AuditResult::Success)
                .with_device(device.clone()),
        );
    }
    drop(out_tx);
    let _ = writer_task.await;
}

fn process_text(
    state: &Arc<AppState>,
    out_tx: &mpsc::UnboundedSender<Envelope>,
    auth_device: &mut Option<remote_protocol::DeviceId>,
    data: &[u8],
) -> Option<Envelope> {
    state.metrics_registry.inc_messages();

    // Size + schema validation before any routing (§7.2).
    let envelope = match Envelope::from_bytes(data) {
        Ok(envelope) => envelope,
        Err(error) => {
            state.metrics_registry.inc_messages_rejected();
            return Some(error_reply("0", "invalid_envelope", &error.to_string()));
        }
    };
    let kind = match envelope.message_type() {
        Ok(kind) => kind,
        Err(error) => {
            state.metrics_registry.inc_messages_rejected();
            return Some(error_reply(
                &envelope.request_id,
                "unknown_type",
                &error.to_string(),
            ));
        }
    };

    // device.register is the only message allowed pre-auth.
    if kind == MessageType::DeviceRegister {
        return handle_register(state, out_tx, auth_device, envelope);
    }

    let Some(device) = auth_device.clone() else {
        state.metrics_registry.inc_messages_rejected();
        return Some(error_reply(
            &envelope.request_id,
            "unauthorized",
            "device not authenticated",
        ));
    };

    // Rate limit per authenticated device.
    if !state.rate_limiter.allow(&device) {
        state.metrics_registry.inc_messages_rejected();
        state.audit.record(
            AuditEvent::new("signaling.rate_limited", AuditResult::Denied).with_device(device),
        );
        return Some(error_reply(
            &envelope.request_id,
            "rate_limited",
            "too many messages",
        ));
    }
    state.touch_device(&device);

    let result = match kind {
        MessageType::DeviceHeartbeat => Ok(None),
        MessageType::SessionRequest => handle_session_request(state, out_tx, &device, envelope),
        MessageType::SessionApprove | MessageType::SessionReject => {
            handle_session_decision(state, out_tx, &device, kind, envelope)
        }
        MessageType::RtcOffer | MessageType::RtcAnswer | MessageType::RtcIceCandidate => {
            relay_to_peer(state, out_tx, &device, envelope)
        }
        MessageType::SessionEnd => handle_session_end(state, out_tx, &device, envelope),
        MessageType::SessionRevoke => handle_session_revoke(state, out_tx, &device, envelope),
        MessageType::DeviceRemove => handle_device_remove(state, &device, envelope),
        _ => Err(error_reply(
            &envelope.request_id,
            "unexpected_type",
            "message not handled here",
        )),
    };
    match result {
        Ok(Some(reply)) => Some(reply),
        Ok(None) => None,
        Err(reply) => {
            state.metrics_registry.inc_messages_rejected();
            Some(reply)
        }
    }
}

fn handle_register(
    state: &Arc<AppState>,
    out_tx: &mpsc::UnboundedSender<Envelope>,
    auth_device: &mut Option<remote_protocol::DeviceId>,
    envelope: Envelope,
) -> Option<Envelope> {
    let payload = match envelope.payload_typed() {
        Ok(Payload::DeviceRegister(register)) => register,
        _ => {
            state.metrics_registry.inc_messages_rejected();
            return Some(error_reply(
                &envelope.request_id,
                "invalid_payload",
                "device.register payload mismatch",
            ));
        }
    };

    // A valid Bearer token may only re-register the same device id.
    if let Some(auth) = &auth_device
        && *auth != payload.device_id
    {
        state.metrics_registry.inc_messages_rejected();
        return Some(error_reply(
            &envelope.request_id,
            "identity_mismatch",
            "token belongs to another device",
        ));
    }

    match state.register_device(
        payload.device_id.clone(),
        payload.display_name.clone(),
        payload.platform.clone(),
        payload.public_key.clone(),
        out_tx.clone(),
    ) {
        Some(token) => {
            *auth_device = Some(payload.device_id.clone());
            state.audit.record(
                AuditEvent::new("device.registered", AuditResult::Success)
                    .with_device(payload.device_id.clone()),
            );
            Some(Envelope::new(
                MessageType::DeviceRegistered,
                remote_protocol::new_request_id(),
                DeviceRegistered {
                    device_id: payload.device_id.clone(),
                    device_token: token,
                },
            ))
        }
        None => Some(error_reply(
            &envelope.request_id,
            "register_rejected",
            "device id exists with a different public key",
        )),
    }
}

fn handle_session_request(
    state: &Arc<AppState>,
    _out_tx: &mpsc::UnboundedSender<Envelope>,
    controller: &remote_protocol::DeviceId,
    envelope: Envelope,
) -> Result<Option<Envelope>, Envelope> {
    let Ok(Payload::SessionRequest(mut request)) = envelope.payload_typed() else {
        return Err(error_reply(
            &envelope.request_id,
            "invalid_payload",
            "session.request payload mismatch",
        ));
    };

    // Server-attested identity (never trust the client field).
    request.controller_device_id = Some(controller.clone());

    if !state.device_present(&request.target_device_id) {
        return Err(error_reply(
            &envelope.request_id,
            "target_offline",
            "target device is not online",
        ));
    }
    if request.target_device_id == *controller {
        return Err(error_reply(
            &envelope.request_id,
            "invalid_target",
            "cannot target self",
        ));
    }
    if let Err(reason) = remote_protocol::validate_request(&request.permissions) {
        return Err(error_reply(
            &envelope.request_id,
            "invalid_permissions",
            reason,
        ));
    }

    let session_id = state.create_session(
        controller.clone(),
        request.target_device_id.clone(),
        request.mode,
        request.permissions.clone(),
    );
    state.audit.record(
        AuditEvent::new("session.request", AuditResult::Allowed)
            .with_actor(controller.clone())
            .with_device(request.target_device_id.clone())
            .with_session(session_id.clone()),
    );

    // Forward the attested request to the host.
    let to_host = Envelope::new(
        MessageType::SessionRequest,
        remote_protocol::new_request_id(),
        SessionRequest {
            target_device_id: request.target_device_id.clone(),
            controller_device_id: Some(controller.clone()),
            mode: request.mode,
            permissions: request.permissions,
        },
    )
    .with_session(session_id.clone());
    let _ = state
        .sender_for(&request.target_device_id)
        .map(|tx| tx.send(to_host));

    // Tell the controller it is awaiting approval.
    let state_note = Envelope::new(
        MessageType::SessionState,
        remote_protocol::new_request_id(),
        SessionStateChange {
            status: "awaiting_approval".into(),
            detail: None,
        },
    )
    .with_session(session_id);
    Ok(Some(state_note))
}

fn handle_session_decision(
    state: &Arc<AppState>,
    _out_tx: &mpsc::UnboundedSender<Envelope>,
    host: &remote_protocol::DeviceId,
    kind: MessageType,
    envelope: Envelope,
) -> Result<Option<Envelope>, Envelope> {
    let Some(session_id) = envelope.session_id.clone() else {
        return Err(error_reply(
            &envelope.request_id,
            "missing_session",
            "session id required",
        ));
    };
    let Ok(Payload::SessionDecision(decision)) = envelope.payload_typed() else {
        return Err(error_reply(
            &envelope.request_id,
            "invalid_payload",
            "decision payload mismatch",
        ));
    };
    let Some((controller, session_host, status)) = state.session(&session_id) else {
        return Err(error_reply(
            &envelope.request_id,
            "unknown_session",
            "session not found",
        ));
    };
    if session_host != *host {
        return Err(error_reply(
            &envelope.request_id,
            "forbidden",
            "only the host may decide",
        ));
    }
    if status != "awaiting_approval" {
        return Err(error_reply(
            &envelope.request_id,
            "invalid_state",
            "session not awaiting approval",
        ));
    }

    if decision.approved {
        state.set_granted_permissions(&session_id, decision.granted_permissions.clone());
        state.set_session_status(&session_id, "negotiating");
    } else {
        state.finish_session(&session_id, "rejected");
    }
    state.audit.record(
        AuditEvent::new(
            if decision.approved {
                "session.approved"
            } else {
                "session.rejected"
            },
            if decision.approved {
                AuditResult::Allowed
            } else {
                AuditResult::Denied
            },
        )
        .with_actor(host.clone())
        .with_device(controller.clone())
        .with_session(session_id.clone()),
    );

    // Relay the decision to the controller.
    let to_controller =
        Envelope::new(kind, remote_protocol::new_request_id(), decision).with_session(session_id);
    let _ = state
        .sender_for(&controller)
        .map(|tx| tx.send(to_controller));
    Ok(None)
}

/// Relays `rtc.*` messages between session participants only while the
/// session is negotiating/connecting.
fn relay_to_peer(
    state: &Arc<AppState>,
    _out_tx: &mpsc::UnboundedSender<Envelope>,
    sender: &remote_protocol::DeviceId,
    envelope: Envelope,
) -> Result<Option<Envelope>, Envelope> {
    let Some(session_id) = envelope.session_id.clone() else {
        return Err(error_reply(
            &envelope.request_id,
            "missing_session",
            "session id required",
        ));
    };
    let Some((controller, host, status)) = state.session(&session_id) else {
        return Err(error_reply(
            &envelope.request_id,
            "unknown_session",
            "session not found",
        ));
    };
    let peer = if *sender == controller {
        host
    } else if *sender == host {
        controller
    } else {
        return Err(error_reply(
            &envelope.request_id,
            "forbidden",
            "not a session participant",
        ));
    };
    if !matches!(status.as_str(), "negotiating" | "connecting" | "active") {
        return Err(error_reply(
            &envelope.request_id,
            "invalid_state",
            "session not in signaling phase",
        ));
    }
    if status == "negotiating" {
        state.set_session_status(&session_id, "connecting");
    }

    let _ = state.sender_for(&peer).map(|tx| tx.send(envelope));
    Ok(None)
}

fn handle_session_end(
    state: &Arc<AppState>,
    _out_tx: &mpsc::UnboundedSender<Envelope>,
    sender: &remote_protocol::DeviceId,
    envelope: Envelope,
) -> Result<Option<Envelope>, Envelope> {
    let Some(session_id) = envelope.session_id.clone() else {
        return Err(error_reply(
            &envelope.request_id,
            "missing_session",
            "session id required",
        ));
    };
    let Some((controller, host, _status)) = state.session(&session_id) else {
        return Err(error_reply(
            &envelope.request_id,
            "unknown_session",
            "session not found",
        ));
    };
    let peer = if *sender == controller {
        host
    } else if *sender == host {
        controller
    } else {
        return Err(error_reply(
            &envelope.request_id,
            "forbidden",
            "not a session participant",
        ));
    };
    state.finish_session(&session_id, "ended");
    state.audit.record(
        AuditEvent::new("session.end", AuditResult::Success)
            .with_actor(sender.clone())
            .with_session(session_id.clone()),
    );
    let _ = state.sender_for(&peer).map(|tx| tx.send(envelope));
    Ok(None)
}

fn handle_session_revoke(
    state: &Arc<AppState>,
    out_tx: &mpsc::UnboundedSender<Envelope>,
    host: &remote_protocol::DeviceId,
    envelope: Envelope,
) -> Result<Option<Envelope>, Envelope> {
    let Some(session_id) = envelope.session_id.clone() else {
        return Err(error_reply(
            &envelope.request_id,
            "missing_session",
            "session id required",
        ));
    };
    let Some((controller, session_host, _status)) = state.session(&session_id) else {
        return Err(error_reply(
            &envelope.request_id,
            "unknown_session",
            "session not found",
        ));
    };
    if session_host != *host {
        return Err(error_reply(
            &envelope.request_id,
            "forbidden",
            "only the host may revoke",
        ));
    }
    state.finish_session(&session_id, "revoked");
    state.audit.record(
        AuditEvent::new("session.revoked", AuditResult::Success)
            .with_actor(host.clone())
            .with_device(controller.clone())
            .with_session(session_id.clone()),
    );
    let _ = state.sender_for(&controller).map(|tx| tx.send(envelope));
    let _ = out_tx;
    Ok(None)
}

fn handle_device_remove(
    state: &Arc<AppState>,
    device: &remote_protocol::DeviceId,
    envelope: Envelope,
) -> Result<Option<Envelope>, Envelope> {
    let Ok(Payload::DeviceRemove(remove)) = envelope.payload_typed() else {
        return Err(error_reply(
            &envelope.request_id,
            "invalid_payload",
            "device.remove payload mismatch",
        ));
    };
    // MVP: a device may only remove (revoke) itself.
    if remove.device_id != *device {
        return Err(error_reply(
            &envelope.request_id,
            "forbidden",
            "can only remove self",
        ));
    }
    state.disconnect_device(device, &mpsc::unbounded_channel::<Envelope>().0);
    state.audit.record(
        AuditEvent::new("device.removed", AuditResult::Success).with_device(device.clone()),
    );
    Ok(None)
}

fn error_reply(request_id: &str, code: &str, message: &str) -> Envelope {
    Envelope {
        version: PROTOCOL_VERSION,
        kind: "error".into(),
        request_id: format!("{request_id}_reply"),
        session_id: None,
        payload: serde_json::to_value(ErrorPayload {
            code: code.into(),
            message: message.into(),
        })
        .expect("error payload serializable"),
    }
}
