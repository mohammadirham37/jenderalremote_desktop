//! Host-side engine session.
//!
//! Responsibilities (rancangan teknis §8.1):
//! 1. Listen on signaling; surface `session.request` to the UI.
//! 2. On approval: negotiate WebRTC as the offerer (DC `control`/`video`).
//! 3. While Active: capture → JPEG → chunk → video channel; apply input
//!    events from the control channel *only* when the state machine and
//!    policy allow (§5.3, §8.3).
//! 4. On end/revoke/disconnect: stop capture, release all pressed keys
//!    (§7.3), notify UI.

use crate::config::EngineConfig;
use crate::encoder::encode_jpeg;
use crate::error::EngineError;
use crate::peer::{self, PeerEvent};
use crate::signaling::SignalingClient;
use remote_capture::{CaptureConfig, ScreenCapture};
use remote_core::{SessionEvent, SessionStateMachine, SessionStatus};
use remote_input::InputController;
use remote_protocol::{
    AccessMode, ControlFrame, DeviceHeartbeat, DeviceId, DeviceRegister, Envelope, HostStats,
    IceCandidateJson, MessageType, Payload, PermissionSet, SessionDecision, SessionEnd, SessionId,
    SessionRequest, chunk_frame, new_request_id,
};
use remote_security::{AuditResult, AuditSink, DeviceIdentity, HostAccessPolicy, LoggingAuditSink};
use remote_telemetry::SessionMetrics;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use webrtc::api::APIBuilder;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

/// Factory creating a fresh capture backend per session.
pub type CaptureFactory = Arc<dyn Fn() -> Box<dyn ScreenCapture> + Send + Sync>;

/// Commands sent from the UI to the engine task.
#[derive(Debug)]
pub enum HostCmd {
    /// Answer a pending `session.request`.
    Respond {
        session_id: SessionId,
        approved: bool,
        permissions: PermissionSet,
    },
    /// Terminate the running/pending session (host always can, §8.1).
    End { reason: Option<String> },
    /// Stop the engine entirely.
    Shutdown,
}

/// Events from the engine task to the UI.
#[derive(Debug)]
pub enum HostEvent {
    /// A fresh device token was issued by the server — persist it in the
    /// OS credential vault.
    TokenIssued(String),
    /// A controller wants a session; the host user must decide.
    SessionRequest {
        session_id: SessionId,
        controller_device_id: DeviceId,
        mode: AccessMode,
        permissions: PermissionSet,
    },
    StateChanged {
        status: String,
        detail: Option<String>,
    },
    Stats(remote_telemetry::MetricsSnapshot),
    SessionEnded {
        reason: Option<String>,
    },
    Error(String),
}

pub struct HostHandle {
    cmd_tx: mpsc::UnboundedSender<HostCmd>,
    events_rx: mpsc::UnboundedReceiver<HostEvent>,
    join: tokio::task::JoinHandle<()>,
}

impl HostHandle {
    pub fn cmd_tx(&self) -> mpsc::UnboundedSender<HostCmd> {
        self.cmd_tx.clone()
    }

    pub async fn next_event(&mut self) -> Option<HostEvent> {
        self.events_rx.recv().await
    }

    pub async fn shutdown(self) {
        let _ = self.cmd_tx.send(HostCmd::Shutdown);
        let _ = self.join.await;
    }
}

pub struct HostEngine;

impl HostEngine {
    /// Starts the host engine task.
    pub fn start(
        config: EngineConfig,
        identity: DeviceIdentity,
        device_token: Option<String>,
        capture_factory: CaptureFactory,
        input: Arc<dyn InputController>,
        policy: HostAccessPolicy,
    ) -> Result<HostHandle, EngineError> {
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, events_rx) = mpsc::unbounded_channel();
        let audit: Arc<dyn AuditSink> = Arc::new(LoggingAuditSink);
        let metrics = Arc::new(SessionMetrics::default());

        let join = tokio::spawn(async move {
            if let Err(error) = run_host(
                config,
                identity,
                device_token,
                capture_factory,
                input,
                policy,
                &mut cmd_rx,
                event_tx.clone(),
                metrics,
                audit,
            )
            .await
            {
                let _ = event_tx.send(HostEvent::Error(error.to_string()));
            }
        });

        Ok(HostHandle {
            cmd_tx,
            events_rx,
            join,
        })
    }
}

struct ActiveSession {
    session_id: SessionId,
    sm: SessionStateMachine,
    controller_device_id: DeviceId,
    peer: Arc<webrtc::peer_connection::RTCPeerConnection>,
    control: Arc<webrtc::data_channel::RTCDataChannel>,
    video: Arc<webrtc::data_channel::RTCDataChannel>,
    cancel_capture: tokio::sync::watch::Sender<bool>,
}

struct PendingRequest {
    session_id: SessionId,
    controller_device_id: DeviceId,
    mode: AccessMode,
    requested: PermissionSet,
}

#[allow(clippy::too_many_arguments)]
async fn run_host(
    config: EngineConfig,
    identity: DeviceIdentity,
    device_token: Option<String>,
    capture_factory: CaptureFactory,
    input: Arc<dyn InputController>,
    policy: HostAccessPolicy,
    cmd_rx: &mut mpsc::UnboundedReceiver<HostCmd>,
    event_tx: mpsc::UnboundedSender<HostEvent>,
    metrics: Arc<SessionMetrics>,
    audit: Arc<dyn AuditSink>,
) -> Result<(), EngineError> {
    let device_id = identity.device_id().clone();

    let (mut signaling, mut signaling_rx) =
        SignalingClient::connect(&config.signaling_url, device_token.as_deref().unwrap_or(""))
            .await?;

    // Register (or refresh) this device. The server rotates the device
    // token and answers with `device.registered`.
    signaling.send(&register_envelope(&identity)).await?;
    wait_for_registration(&mut signaling_rx, &event_tx).await?;
    tracing::info!(device = %device_id, "host registered on signaling server");

    let api = APIBuilder::new().build();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(config.heartbeat_secs.max(5)));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let (peer_tx, mut peer_rx) = mpsc::unbounded_channel::<PeerEvent>();
    let mut pending: Option<PendingRequest> = None;
    let mut active: Option<ActiveSession> = None;

    loop {
        let cmd;
        tokio::select! {
            maybe_cmd = cmd_rx.recv() => { cmd = maybe_cmd; }
            maybe_envelope = signaling_rx.recv() => {
                let Some(envelope) = maybe_envelope else { break };
                let outcome = handle_signaling(
                    &config, &api, &policy, envelope, &mut signaling, &capture_factory,
                    &input, &event_tx, &metrics, &audit, &mut pending, &mut active, &peer_tx,
                ).await;
                if let Err(error) = outcome {
                    let _ = event_tx.send(HostEvent::Error(error.to_string()));
                }
                continue;
            }
            maybe_peer = peer_rx.recv() => {
                let Some(event) = maybe_peer else { continue };
                let outcome = handle_peer_event(
                    event, &mut signaling, &input, &event_tx, &metrics, &audit, &mut active, &config, &capture_factory,
                ).await;
                if let Err(error) = outcome {
                    let _ = event_tx.send(HostEvent::Error(error.to_string()));
                }
                continue;
            }
            _ = heartbeat.tick() => {
                let env = Envelope::new(
                    MessageType::DeviceHeartbeat,
                    new_request_id(),
                    DeviceHeartbeat { status: "online".into() },
                );
                if signaling.send(&env).await.is_err() { break; }
                continue;
            }
        }

        match cmd {
            Some(HostCmd::Shutdown) | None => break,
            Some(HostCmd::End { reason }) => {
                if let Some(session) = active.take() {
                    teardown_session(
                        &mut signaling,
                        session,
                        &input,
                        &event_tx,
                        &audit,
                        "host_ended",
                        reason.as_deref(),
                    )
                    .await;
                }
            }
            Some(HostCmd::Respond {
                session_id,
                approved,
                permissions,
            }) => {
                let request = match pending.take() {
                    Some(request) if request.session_id == session_id => request,
                    Some(other) => {
                        pending = Some(other);
                        let _ = event_tx.send(HostEvent::Error("stale session respond".into()));
                        continue;
                    }
                    None => continue,
                };
                if let Err(error) = respond_to_request(
                    &config,
                    &api,
                    request,
                    approved,
                    permissions,
                    &mut signaling,
                    &capture_factory,
                    &event_tx,
                    &audit,
                    &mut active,
                    &peer_tx,
                )
                .await
                {
                    let _ = event_tx.send(HostEvent::Error(error.to_string()));
                }
            }
        }
    }

    if let Some(session) = active.take() {
        teardown_session(
            &mut signaling,
            session,
            &input,
            &event_tx,
            &audit,
            "host_ended",
            None,
        )
        .await;
    }
    Ok(())
}

async fn wait_for_registration(
    signaling_rx: &mut mpsc::UnboundedReceiver<Envelope>,
    event_tx: &mpsc::UnboundedSender<HostEvent>,
) -> Result<(), EngineError> {
    loop {
        match signaling_rx.recv().await {
            Some(envelope) => match envelope.payload_typed()? {
                Payload::DeviceRegistered(registered) => {
                    let _ = event_tx.send(HostEvent::TokenIssued(registered.device_token));
                    return Ok(());
                }
                Payload::ServerError(error) => {
                    return Err(EngineError::SignalingRejected {
                        code: error.code,
                        message: error.message,
                    });
                }
                _ => {}
            },
            None => return Err(EngineError::Closed),
        }
    }
}

fn register_envelope(identity: &DeviceIdentity) -> Envelope {
    Envelope::new(
        MessageType::DeviceRegister,
        new_request_id(),
        DeviceRegister {
            device_id: identity.device_id().clone(),
            display_name: format!("{} host", std::env::consts::OS),
            platform: std::env::consts::OS.to_string(),
            public_key: identity.public_key_spki_base64(),
        },
    )
}

fn map_webrtc(error: webrtc::Error) -> EngineError {
    EngineError::WebRtc(error.to_string())
}

fn emit_state(event_tx: &mpsc::UnboundedSender<HostEvent>, sm: &SessionStateMachine) {
    let _ = event_tx.send(HostEvent::StateChanged {
        status: sm.status().as_str().to_string(),
        detail: None,
    });
}

/// Records an audit event without leaking secrets.
fn audit_event(
    audit: &Arc<dyn AuditSink>,
    event_type: &str,
    controller: Option<&DeviceId>,
    session: Option<&SessionId>,
    result: AuditResult,
) {
    let mut event = remote_security::AuditEvent::new(event_type, result);
    if let Some(controller) = controller {
        event = event.with_actor(controller.clone());
    }
    if let Some(session) = session {
        event = event.with_session(session.clone());
    }
    audit.record(event);
}

#[allow(clippy::too_many_arguments)]
async fn handle_signaling(
    config: &EngineConfig,
    api: &webrtc::api::API,
    policy: &HostAccessPolicy,
    envelope: Envelope,
    signaling: &mut SignalingClient,
    capture_factory: &CaptureFactory,
    input: &Arc<dyn InputController>,
    event_tx: &mpsc::UnboundedSender<HostEvent>,
    metrics: &Arc<SessionMetrics>,
    audit: &Arc<dyn AuditSink>,
    pending: &mut Option<PendingRequest>,
    active: &mut Option<ActiveSession>,
    peer_tx: &mpsc::UnboundedSender<PeerEvent>,
) -> Result<(), EngineError> {
    match envelope.payload_typed()? {
        Payload::DeviceRegistered(registered) => {
            let _ = event_tx.send(HostEvent::TokenIssued(registered.device_token));
            Ok(())
        }
        Payload::ServerError(error) => {
            let _ = event_tx.send(HostEvent::Error(format!(
                "{}: {}",
                error.code, error.message
            )));
            Ok(())
        }
        Payload::SessionRequest(request) => {
            handle_session_request(
                request, envelope, policy, signaling, audit, event_tx, pending, active,
            )
            .await
        }
        Payload::RtcAnswer(answer) => {
            if let Some(session) = active.as_mut() {
                session
                    .peer
                    .set_remote_description(
                        RTCSessionDescription::answer(answer.sdp).map_err(map_webrtc)?,
                    )
                    .await
                    .map_err(map_webrtc)?;
                session.sm.apply(SessionEvent::ConnectionAttemptStarted)?;
                emit_state(event_tx, &session.sm);
            }
            Ok(())
        }
        Payload::IceCandidate(candidate) => {
            if let Some(session) = active.as_ref() {
                session
                    .peer
                    .add_ice_candidate(to_ice_init(candidate))
                    .await
                    .map_err(map_webrtc)?;
            }
            Ok(())
        }
        Payload::SessionEnd(end) => {
            if let Some(mut session) = active.take() {
                let _ = session.sm.apply(SessionEvent::Ended);
                teardown_session(
                    signaling,
                    session,
                    input,
                    event_tx,
                    audit,
                    "controller_ended",
                    end.reason.as_deref(),
                )
                .await;
            }
            Ok(())
        }
        Payload::SessionRevoke(revoke) => {
            if let Some(mut session) = active.take() {
                let _ = session.sm.apply(SessionEvent::Revoked);
                teardown_session(
                    signaling,
                    session,
                    input,
                    event_tx,
                    audit,
                    "revoked",
                    revoke.reason.as_deref(),
                )
                .await;
            }
            Ok(())
        }
        // Anything else is not host business at this phase.
        _ => {
            let _ = (config, api, capture_factory, metrics, peer_tx);
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_session_request(
    request: Box<SessionRequest>,
    envelope: Envelope,
    policy: &HostAccessPolicy,
    signaling: &mut SignalingClient,
    audit: &Arc<dyn AuditSink>,
    event_tx: &mpsc::UnboundedSender<HostEvent>,
    pending: &mut Option<PendingRequest>,
    active: &mut Option<ActiveSession>,
) -> Result<(), EngineError> {
    if active.is_some() || pending.is_some() {
        let _ = event_tx.send(HostEvent::Error("session request while busy".into()));
        return Ok(());
    }
    let Some(controller) = request.controller_device_id.clone() else {
        let _ = event_tx.send(HostEvent::Error(
            "server did not attest controller identity".into(),
        ));
        return Ok(());
    };
    let Some(session_id) = envelope.session_id.clone() else {
        let _ = event_tx.send(HostEvent::Error(
            "session.request without session id".into(),
        ));
        return Ok(());
    };

    // Unattended requests must pass the local policy gate now (§8.2).
    if request.mode == AccessMode::Unattended {
        let now_ms = now_ms();
        match policy.authorize_unattended(&controller, &request.permissions, now_ms) {
            Ok(_) => {
                audit_event(
                    audit,
                    "session.unattended_allowed",
                    Some(&controller),
                    Some(&session_id),
                    AuditResult::Allowed,
                );
            }
            Err(error) => {
                audit_event(
                    audit,
                    "session.unattended_denied",
                    Some(&controller),
                    Some(&session_id),
                    AuditResult::Denied,
                );
                let _ = signaling
                    .send(
                        &Envelope::new(
                            MessageType::SessionReject,
                            new_request_id(),
                            SessionDecision {
                                approved: false,
                                reason: Some(format!("unattended policy: {error}")),
                                granted_permissions: PermissionSet::empty(),
                            },
                        )
                        .with_session(session_id),
                    )
                    .await;
                return Ok(());
            }
        }
    }

    *pending = Some(PendingRequest {
        session_id: session_id.clone(),
        controller_device_id: controller.clone(),
        mode: request.mode,
        requested: request.permissions.clone(),
    });
    let _ = event_tx.send(HostEvent::SessionRequest {
        session_id,
        controller_device_id: controller,
        mode: request.mode,
        permissions: request.permissions,
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn respond_to_request(
    config: &EngineConfig,
    api: &webrtc::api::API,
    request: PendingRequest,
    approved: bool,
    requested_permissions: PermissionSet,
    signaling: &mut SignalingClient,
    capture_factory: &CaptureFactory,
    event_tx: &mpsc::UnboundedSender<HostEvent>,
    audit: &Arc<dyn AuditSink>,
    active: &mut Option<ActiveSession>,
    peer_tx: &mpsc::UnboundedSender<PeerEvent>,
) -> Result<(), EngineError> {
    let session_id = request.session_id.clone();

    // The host may only narrow, never widen (§8.3).
    let permissions = PermissionSet::from_permissions(
        requested_permissions
            .iter()
            .filter(|p| request.requested.contains(*p)),
    );
    let granted = HostAccessPolicy::authorize_attended(approved, &permissions);

    let decision = match &granted {
        Ok(perms) => SessionDecision {
            approved: true,
            reason: None,
            granted_permissions: perms.clone(),
        },
        Err(_) => SessionDecision {
            approved: false,
            reason: Some("host user denied the request".into()),
            granted_permissions: PermissionSet::empty(),
        },
    };

    let kind = if decision.approved {
        MessageType::SessionApprove
    } else {
        MessageType::SessionReject
    };
    signaling
        .send(
            &Envelope::new(kind, new_request_id(), decision.clone())
                .with_session(session_id.clone()),
        )
        .await?;
    audit.record(
        remote_security::AuditEvent::new(
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
        .with_actor(request.controller_device_id.clone())
        .with_session(session_id.clone())
        .with_metadata(
            "mode",
            if request.mode == AccessMode::Unattended {
                "unattended"
            } else {
                "attended"
            },
        ),
    );

    let granted = match granted {
        Ok(granted) => granted,
        Err(error) => {
            let _ = event_tx.send(HostEvent::SessionEnded {
                reason: Some(error.to_string()),
            });
            return Ok(());
        }
    };

    // Build the offering peer.
    let (peer, control, video) =
        peer::create_host_peer(api, config.rtc_configuration(), peer_tx.clone())
            .await
            .map_err(map_webrtc)?;

    let mut sm = SessionStateMachine::new(session_id.as_str());
    sm.apply(SessionEvent::IncomingRequest)?;
    sm.apply(SessionEvent::Approved {
        permissions: granted,
    })?;
    sm.apply(SessionEvent::NegotiationStarted)?;

    let offer = peer.create_offer(None).await.map_err(map_webrtc)?;
    peer.set_local_description(offer.clone())
        .await
        .map_err(map_webrtc)?;
    signaling
        .send(
            &Envelope::new(
                MessageType::RtcOffer,
                new_request_id(),
                remote_protocol::RtcOffer { sdp: offer.sdp },
            )
            .with_session(session_id.clone()),
        )
        .await?;

    let (cancel_capture, _) = tokio::sync::watch::channel(false);
    *active = Some(ActiveSession {
        session_id,
        controller_device_id: request.controller_device_id.clone(),
        sm,
        peer,
        control,
        video,
        cancel_capture,
    });
    let session = active.as_ref().expect("just set");
    emit_state(event_tx, &session.sm);
    audit_event(
        audit,
        "session.negotiating",
        Some(&request.controller_device_id),
        Some(&session.session_id),
        AuditResult::Success,
    );
    let _ = capture_factory;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_peer_event(
    event: PeerEvent,
    signaling: &mut SignalingClient,
    input: &Arc<dyn InputController>,
    event_tx: &mpsc::UnboundedSender<HostEvent>,
    metrics: &Arc<SessionMetrics>,
    audit: &Arc<dyn AuditSink>,
    active: &mut Option<ActiveSession>,
    config: &EngineConfig,
    capture_factory: &CaptureFactory,
) -> Result<(), EngineError> {
    match event {
        PeerEvent::LocalCandidate(candidate) => {
            if let Some(session) = active.as_ref() {
                let payload = IceCandidateJson {
                    candidate: candidate.candidate.clone(),
                    sdp_mid: candidate.sdp_mid.clone(),
                    sdp_mline_index: candidate.sdp_mline_index,
                };
                signaling
                    .send(
                        &Envelope::new(MessageType::RtcIceCandidate, new_request_id(), payload)
                            .with_session(session.session_id.clone()),
                    )
                    .await?;
            }
            Ok(())
        }
        PeerEvent::StateChanged(state) => {
            use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::*;
            match state {
                Connected => {
                    let Some(session) = active.as_mut() else {
                        return Ok(());
                    };
                    if session.sm.status() != SessionStatus::Active {
                        session.sm.apply(SessionEvent::Connected)?;
                        emit_state(event_tx, &session.sm);
                        audit_event(
                            audit,
                            "session.active",
                            Some(&session.controller_device_id),
                            Some(&session.session_id),
                            AuditResult::Success,
                        );
                        activate_capture_loop(
                            session,
                            config.target_fps,
                            config.video_quality,
                            config.backpressure_bytes,
                            metrics.clone(),
                            event_tx.clone(),
                            capture_factory(),
                        );
                    }
                    Ok(())
                }
                Disconnected | Failed | Closed => {
                    if let Some(mut session) = active.take() {
                        let _ = session.sm.apply(SessionEvent::Disconnected);
                        // Real input backend releases pressed keys here.
                        let _ = input.release_all().await;
                        let _ = session.cancel_capture.send(true);
                        let _ = session.peer.close().await;
                        let _ = event_tx.send(HostEvent::SessionEnded {
                            reason: Some("connection_lost".into()),
                        });
                        let _ = event_tx.send(HostEvent::StateChanged {
                            status: SessionStatus::Ended.as_str().to_string(),
                            detail: None,
                        });
                    }
                    Ok(())
                }
                _ => Ok(()),
            }
        }
        PeerEvent::ChannelOpened(label, _) => {
            tracing::debug!(channel = %label, "data channel open (host side)");
            Ok(())
        }
        PeerEvent::ControlMessage(text) => {
            let Some(session) = active.as_ref() else {
                return Ok(());
            };
            let Ok(frame) = ControlFrame::decode(&text) else {
                tracing::warn!("undecodable control frame from controller");
                return Ok(());
            };
            tracing::debug!(kind = %text, "control frame received");
            match frame {
                ControlFrame::Ping { sent_ms } => {
                    let pong = ControlFrame::Pong { sent_ms };
                    let _ = session.control.send_text(&pong.encode()).await;
                    Ok(())
                }
                ControlFrame::Pong { .. } | ControlFrame::Stats(_) => Ok(()),
                ControlFrame::Mouse(mouse) => {
                    metrics.inc_input_received();
                    if session.sm.input_allowed() {
                        match input.send_mouse(mouse).await {
                            Ok(()) => metrics.inc_input_applied(),
                            Err(error) => {
                                metrics.inc_input_rejected();
                                tracing::warn!(%error, "mouse injection failed");
                            }
                        }
                    } else {
                        metrics.inc_input_rejected();
                        audit_event(
                            audit,
                            "input.rejected",
                            Some(&session.controller_device_id),
                            Some(&session.session_id),
                            AuditResult::Denied,
                        );
                    }
                    Ok(())
                }
                ControlFrame::Keyboard(keyboard) => {
                    metrics.inc_input_received();
                    if session.sm.input_allowed() {
                        match input.send_keyboard(keyboard).await {
                            Ok(()) => metrics.inc_input_applied(),
                            Err(error) => {
                                metrics.inc_input_rejected();
                                tracing::warn!(%error, "keyboard injection failed");
                            }
                        }
                    } else {
                        metrics.inc_input_rejected();
                        audit_event(
                            audit,
                            "input.rejected",
                            Some(&session.controller_device_id),
                            Some(&session.session_id),
                            AuditResult::Denied,
                        );
                    }
                    Ok(())
                }
            }
        }
        PeerEvent::VideoMessage(_) => Ok(()),
    }
}

/// Spawns the capture/encode/send loop for an active session.
fn activate_capture_loop(
    session: &mut ActiveSession,
    target_fps: f32,
    quality: u8,
    backpressure: usize,
    metrics: Arc<SessionMetrics>,
    event_tx: mpsc::UnboundedSender<HostEvent>,
    mut capture: Box<dyn ScreenCapture>,
) {
    let video = session.video.clone();
    let control = session.control.clone();
    let mut cancel = session.cancel_capture.subscribe();

    tokio::spawn(async move {
        let started = Instant::now();
        let mut stats_tick = tokio::time::interval(Duration::from_secs(2));
        let mut frame_id: u32 = 0;
        if let Err(error) = capture
            .start(CaptureConfig {
                monitor_index: 0,
                target_fps,
            })
            .await
        {
            let _ = event_tx.send(HostEvent::Error(error.to_string()));
            return;
        }
        loop {
            tokio::select! {
                _ = cancel.changed() => break,
                _ = stats_tick.tick() => {
                    let snapshot = metrics.snapshot(started);
                    let _ = event_tx.send(HostEvent::Stats(snapshot));
                    let stats = ControlFrame::Stats(HostStats {
                        fps: snapshot.fps,
                        encode_ms: snapshot.encode_ms,
                        capture_ms: snapshot.capture_ms,
                        frames_skipped: snapshot.frames_skipped,
                    });
                    let _ = control.send_text(&stats.encode()).await;
                }
                frame = capture.next_frame() => {
                    match frame {
                        Ok(frame) => {
                            metrics.inc_frames_captured();
                            if video.buffered_amount().await > backpressure {
                                metrics.inc_frames_skipped();
                                continue;
                            }
                            let encode_started = Instant::now();
                            match encode_jpeg(&frame.data, frame.width, frame.height, quality) {
                                Ok(jpeg) => {
                                    metrics.inc_frames_encoded(encode_started.elapsed().as_micros());
                                    frame_id = frame_id.wrapping_add(1);
                                    for chunk in chunk_frame(
                                        frame_id,
                                        frame.width as u16,
                                        frame.height as u16,
                                        &jpeg,
                                    ) {
                                        if video
                                            .send(&bytes::Bytes::from(chunk.clone()))
                                            .await
                                            .is_err()
                                        {
                                            break;
                                        }
                                        metrics.add_bytes_sent(chunk.len() as u64);
                                    }
                                }
                                Err(_) => metrics.inc_encode_error(),
                            }
                        }
                        Err(error) => {
                            let _ = event_tx.send(HostEvent::Error(error.to_string()));
                            break;
                        }
                    }
                }
            }
        }
        let _ = capture.stop().await;
        tracing::info!("host capture loop stopped");
    });
}

#[allow(clippy::too_many_arguments)]
async fn teardown_session(
    signaling: &mut SignalingClient,
    session: ActiveSession,
    input: &Arc<dyn InputController>,
    event_tx: &mpsc::UnboundedSender<HostEvent>,
    audit: &Arc<dyn AuditSink>,
    reason_kind: &str,
    reason_detail: Option<&str>,
) {
    let _ = session.cancel_capture.send(true);
    // No key stays pressed after a session ends (§7.3).
    let _ = input.release_all().await;
    let _ = signaling
        .send(
            &Envelope::new(
                MessageType::SessionEnd,
                new_request_id(),
                SessionEnd {
                    reason: Some(reason_kind.to_string()),
                },
            )
            .with_session(session.session_id.clone()),
        )
        .await;
    let _ = session.peer.close().await;
    audit_event(
        audit,
        &format!("session.{reason_kind}"),
        Some(&session.controller_device_id),
        Some(&session.session_id),
        AuditResult::Success,
    );
    let _ = event_tx.send(HostEvent::SessionEnded {
        reason: reason_detail
            .map(str::to_string)
            .or(Some(reason_kind.to_string())),
    });
    let _ = event_tx.send(HostEvent::StateChanged {
        status: SessionStatus::Ended.as_str().to_string(),
        detail: None,
    });
}

fn to_ice_init(
    candidate: IceCandidateJson,
) -> webrtc::ice_transport::ice_candidate::RTCIceCandidateInit {
    webrtc::ice_transport::ice_candidate::RTCIceCandidateInit {
        candidate: candidate.candidate,
        sdp_mid: candidate.sdp_mid,
        sdp_mline_index: candidate.sdp_mline_index,
        username_fragment: None,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
