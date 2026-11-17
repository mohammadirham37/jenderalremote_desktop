//! Controller-side engine session.
//!
//! Flow (§8.1): request a session → await host approval → receive SDP
//! offer → answer → ICE (both directions) → Active. Video chunks are
//! reassembled into JPEG frames and surfaced for rendering; input events
//! are converted to control-channel frames (only while the session is
//! Active).

use crate::config::EngineConfig;
use crate::error::EngineError;
use crate::peer::{self, PeerEvent};
use crate::signaling::SignalingClient;
use remote_core::{SessionEvent, SessionStateMachine, SessionStatus};
use remote_protocol::{
    AccessMode, ControlFrame, DeviceHeartbeat, DeviceId, DeviceRegister, Envelope, FrameAssembler,
    HostStats, IceCandidateJson, KeyboardEvent, MessageType, Payload, PermissionSet, SessionId,
    SessionRequest, VideoChunk, new_request_id,
};
use remote_security::DeviceIdentity;
use remote_telemetry::SessionMetrics;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

/// Commands from the UI to the controller engine.
#[derive(Debug)]
pub enum ControllerCmd {
    /// Normalized pointer position (0.0-1.0) mapped to stream pixels.
    Mouse {
        nx: f64,
        ny: f64,
        action: remote_protocol::MouseAction,
        button: remote_protocol::MouseButton,
        scroll_dx: f64,
        scroll_dy: f64,
    },
    Keyboard(KeyboardEvent),
    End {
        reason: Option<String>,
    },
    Shutdown,
}

/// Events from the controller engine to the UI.
#[derive(Debug)]
pub enum ControllerEvent {
    TokenIssued(String),
    StateChanged {
        status: String,
        detail: Option<String>,
    },
    /// A complete JPEG frame ready for rendering.
    Frame {
        frame_id: u32,
        width: u16,
        height: u16,
        jpeg: Arc<Vec<u8>>,
    },
    Stats {
        host: HostStats,
        rtt_ms: u32,
    },
    Ended {
        reason: Option<String>,
    },
    Error(String),
}

pub struct ControllerHandle {
    cmd_tx: mpsc::UnboundedSender<ControllerCmd>,
    events_rx: mpsc::UnboundedReceiver<ControllerEvent>,
    join: tokio::task::JoinHandle<()>,
}

impl ControllerHandle {
    pub fn cmd_tx(&self) -> mpsc::UnboundedSender<ControllerCmd> {
        self.cmd_tx.clone()
    }

    pub async fn next_event(&mut self) -> Option<ControllerEvent> {
        self.events_rx.recv().await
    }

    pub async fn shutdown(self) {
        let _ = self.cmd_tx.send(ControllerCmd::Shutdown);
        let _ = self.join.await;
    }
}

pub struct ControllerEngine;

impl ControllerEngine {
    pub fn start(
        config: EngineConfig,
        identity: DeviceIdentity,
        device_token: Option<String>,
        target: DeviceId,
        mode: AccessMode,
        requested: PermissionSet,
    ) -> Result<ControllerHandle, EngineError> {
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
        let (event_tx, events_rx) = mpsc::unbounded_channel();

        let join = tokio::spawn(async move {
            let metrics = Arc::new(SessionMetrics::default());
            if let Err(error) = run_controller(
                config,
                identity,
                device_token,
                target,
                mode,
                requested,
                &mut cmd_rx,
                event_tx.clone(),
                metrics,
            )
            .await
            {
                let _ = event_tx.send(ControllerEvent::Error(error.to_string()));
                let _ = event_tx.send(ControllerEvent::Ended { reason: None });
            }
        });

        Ok(ControllerHandle {
            cmd_tx,
            events_rx,
            join,
        })
    }
}

struct ControllerSession {
    session_id: SessionId,
    sm: SessionStateMachine,
    peer: Arc<webrtc::peer_connection::RTCPeerConnection>,
    control: Option<Arc<webrtc::data_channel::RTCDataChannel>>,
    /// Last known stream dimensions (for pointer scaling).
    stream_size: (u16, u16),
    /// ICE candidates received before the remote description was set.
    pending_ice: Vec<RTCIceCandidateInit>,
}

impl ControllerSession {
    fn control_channel(&self) -> Option<&Arc<webrtc::data_channel::RTCDataChannel>> {
        self.control.as_ref()
    }

    fn register_control_channel(&mut self, channel: Arc<webrtc::data_channel::RTCDataChannel>) {
        if channel.label() == peer::CONTROL_CHANNEL {
            self.control = Some(channel);
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_controller(
    config: EngineConfig,
    identity: DeviceIdentity,
    device_token: Option<String>,
    target: DeviceId,
    mode: AccessMode,
    requested: PermissionSet,
    cmd_rx: &mut mpsc::UnboundedReceiver<ControllerCmd>,
    event_tx: mpsc::UnboundedSender<ControllerEvent>,
    metrics: Arc<SessionMetrics>,
) -> Result<(), EngineError> {
    let (mut signaling, mut signaling_rx) =
        SignalingClient::connect(&config.signaling_url, device_token.as_deref().unwrap_or(""))
            .await?;

    signaling.send(&register_envelope(&identity)).await?;
    // Await token rotation before requesting the session.
    loop {
        match signaling_rx.recv().await {
            Some(envelope) => match envelope.payload_typed()? {
                Payload::DeviceRegistered(registered) => {
                    let _ = event_tx.send(ControllerEvent::TokenIssued(registered.device_token));
                    break;
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

    // Pre-session state machine (drives the request/approval phases).
    let mut phase = SessionStateMachine::new("controller");
    phase.apply(SessionEvent::RequestSent)?;
    emit_state(&event_tx, &phase);

    let request = SessionRequest {
        target_device_id: target,
        controller_device_id: None, // server fills this from auth
        mode,
        permissions: requested,
    };
    signaling
        .send(&Envelope::new(
            MessageType::SessionRequest,
            new_request_id(),
            request,
        ))
        .await?;

    let api = APIBuilder::new().build();
    let (peer_tx, mut peer_rx) = mpsc::unbounded_channel::<PeerEvent>();
    let mut session: Option<ControllerSession> = None;
    let mut assembler = FrameAssembler::new();
    let mut last_rtt_ms: u32 = 0;

    let mut heartbeat = tokio::time::interval(Duration::from_secs(config.heartbeat_secs.max(5)));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut ping_tick = tokio::time::interval(Duration::from_secs(2));
    ping_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        let cmd;
        tokio::select! {
            maybe_cmd = cmd_rx.recv() => { cmd = maybe_cmd; }
            maybe_envelope = signaling_rx.recv() => {
                let Some(envelope) = maybe_envelope else { break };
                let stop = handle_signaling(
                    &api, &config, envelope, &mut signaling, &event_tx, &mut phase,
                    &mut session, &peer_tx,
                )
                .await?;
                if stop { break; }
                continue;
            }
            maybe_peer = peer_rx.recv() => {
                let Some(event) = maybe_peer else { continue };
                handle_peer_event(
                    event, &mut signaling, &event_tx, &metrics, &mut session, &mut assembler,
                    &mut last_rtt_ms,
                )
                .await;
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
            _ = ping_tick.tick() => {
                if let Some(active) = session.as_ref()
                    && active.sm.status() == SessionStatus::Active
                        && let Some(control) = active.control_channel() {
                            let _ = control
                                .send_text(&ControlFrame::Ping { sent_ms: now_millis() as i64 }.encode())
                                .await;
                        }
                continue;
            }
        }

        match cmd {
            Some(ControllerCmd::Shutdown) | None => break,
            Some(ControllerCmd::End { reason }) => {
                if let Some(active) = session.as_mut() {
                    let _ = active.sm.apply(SessionEvent::EndRequested);
                    let _ = signaling
                        .send(
                            &Envelope::new(
                                MessageType::SessionEnd,
                                new_request_id(),
                                remote_protocol::SessionEnd {
                                    reason: reason.clone(),
                                },
                            )
                            .with_session(active.session_id.clone()),
                        )
                        .await;
                }
                break;
            }
            Some(ControllerCmd::Mouse {
                nx,
                ny,
                action,
                button,
                scroll_dx,
                scroll_dy,
            }) => {
                let Some(active) = session.as_ref() else {
                    tracing::debug!("mouse cmd dropped: no session");
                    continue;
                };
                if active.sm.status() != SessionStatus::Active {
                    tracing::debug!(status = %active.sm.status(), "mouse cmd dropped: not active");
                    continue;
                }
                let Some(control) = active.control_channel() else {
                    tracing::debug!("mouse cmd dropped: no control channel");
                    continue;
                };
                let (w, h) = active.stream_size;
                if w == 0 || h == 0 {
                    tracing::debug!("mouse cmd dropped: no stream size yet");
                    continue;
                }
                let event = remote_protocol::MouseEvent {
                    action,
                    button,
                    x: nx.clamp(0.0, 1.0) * w as f64,
                    y: ny.clamp(0.0, 1.0) * h as f64,
                    scroll_dx,
                    scroll_dy,
                };
                match control
                    .send_text(&ControlFrame::Mouse(event).encode())
                    .await
                {
                    Ok(_) => {
                        tracing::debug!("mouse event sent to host");
                        metrics.inc_input_applied();
                    }
                    Err(error) => tracing::warn!(%error, "mouse send failed"),
                }
            }
            Some(ControllerCmd::Keyboard(keyboard)) => {
                let Some(active) = session.as_ref() else {
                    continue;
                };
                if active.sm.status() != SessionStatus::Active {
                    continue;
                }
                let Some(control) = active.control_channel() else {
                    continue;
                };
                let _ = control
                    .send_text(&ControlFrame::Keyboard(keyboard).encode())
                    .await;
                metrics.inc_input_applied();
            }
        }
    }

    if let Some(mut active) = session.take() {
        let _ = active.sm.apply(SessionEvent::Ended);
        let _ = active.peer.close().await;
        emit_state(&event_tx, &active.sm);
    }
    let _ = event_tx.send(ControllerEvent::Ended { reason: None });
    Ok(())
}

fn register_envelope(identity: &DeviceIdentity) -> Envelope {
    Envelope::new(
        MessageType::DeviceRegister,
        new_request_id(),
        DeviceRegister {
            device_id: identity.device_id().clone(),
            display_name: format!("{} controller", std::env::consts::OS),
            platform: std::env::consts::OS.to_string(),
            public_key: identity.public_key_spki_base64(),
        },
    )
}

fn emit_state(event_tx: &mpsc::UnboundedSender<ControllerEvent>, sm: &SessionStateMachine) {
    let _ = event_tx.send(ControllerEvent::StateChanged {
        status: sm.status().as_str().to_string(),
        detail: None,
    });
}

/// Returns Ok(true) when the controller loop should stop.
#[allow(clippy::too_many_arguments)]
async fn handle_signaling(
    api: &webrtc::api::API,
    config: &EngineConfig,
    envelope: Envelope,
    signaling: &mut SignalingClient,
    event_tx: &mpsc::UnboundedSender<ControllerEvent>,
    phase: &mut SessionStateMachine,
    session: &mut Option<ControllerSession>,
    peer_tx: &mpsc::UnboundedSender<PeerEvent>,
) -> Result<bool, EngineError> {
    match envelope.payload_typed()? {
        Payload::DeviceRegistered(registered) => {
            let _ = event_tx.send(ControllerEvent::TokenIssued(registered.device_token));
            Ok(false)
        }
        Payload::ServerError(error) => {
            let _ = event_tx.send(ControllerEvent::Ended {
                reason: Some(format!("{}: {}", error.code, error.message)),
            });
            Ok(true)
        }
        Payload::SessionState(state) => {
            if state.status == SessionStatus::AwaitingApproval.as_str() {
                let _ = phase.apply(SessionEvent::ApprovalRequired);
                emit_state(event_tx, phase);
            }
            Ok(false)
        }
        Payload::SessionDecision(decision) => {
            if !decision.approved {
                let _ = phase.apply(SessionEvent::Rejected);
                emit_state(event_tx, phase);
                let _ = event_tx.send(ControllerEvent::Ended {
                    reason: decision.reason.clone().or(Some("rejected".into())),
                });
                return Ok(true);
            }
            let _ = phase.apply(SessionEvent::Approved {
                permissions: decision.granted_permissions.clone(),
            });
            emit_state(event_tx, phase);

            let peer =
                peer::create_controller_peer(api, config.rtc_configuration(), peer_tx.clone())
                    .await
                    .map_err(|e| EngineError::WebRtc(e.to_string()))?;
            // Continue the phase state machine (requesting → awaiting →
            // approved/negotiating) inside the session.
            let mut sm = std::mem::replace(phase, SessionStateMachine::new("controller"));
            let _ = sm.apply(SessionEvent::NegotiationStarted);
            *session = Some(ControllerSession {
                session_id: envelope
                    .session_id
                    .clone()
                    .unwrap_or_else(|| SessionId::new("unknown")),
                sm,
                peer,
                control: None,
                stream_size: (0, 0),
                pending_ice: Vec::new(),
            });
            Ok(false)
        }
        Payload::RtcOffer(offer) => {
            let Some(active) = session.as_mut() else {
                return Ok(false);
            };
            active
                .peer
                .set_remote_description(
                    RTCSessionDescription::offer(offer.sdp)
                        .map_err(|e| EngineError::WebRtc(e.to_string()))?,
                )
                .await
                .map_err(|e| EngineError::WebRtc(e.to_string()))?;
            let answer = active
                .peer
                .create_answer(None)
                .await
                .map_err(|e| EngineError::WebRtc(e.to_string()))?;
            active
                .peer
                .set_local_description(answer.clone())
                .await
                .map_err(|e| EngineError::WebRtc(e.to_string()))?;
            signaling
                .send(
                    &Envelope::new(
                        MessageType::RtcAnswer,
                        new_request_id(),
                        remote_protocol::RtcAnswer { sdp: answer.sdp },
                    )
                    .with_session(active.session_id.clone()),
                )
                .await?;
            let _ = active.sm.apply(SessionEvent::ConnectionAttemptStarted);
            emit_state(event_tx, &active.sm);

            // Apply ICE candidates buffered before the offer arrived.
            let buffered = std::mem::take(&mut active.pending_ice);
            for candidate in buffered {
                let _ = active.peer.add_ice_candidate(candidate).await;
            }
            Ok(false)
        }
        Payload::IceCandidate(candidate) => {
            let Some(active) = session.as_mut() else {
                return Ok(false);
            };
            let init = RTCIceCandidateInit {
                candidate: candidate.candidate,
                sdp_mid: candidate.sdp_mid,
                sdp_mline_index: candidate.sdp_mline_index,
                username_fragment: None,
            };
            if active.peer.remote_description().await.is_some() {
                let _ = active.peer.add_ice_candidate(init).await;
            } else {
                active.pending_ice.push(init);
            }
            Ok(false)
        }
        Payload::SessionEnd(end) => {
            let sm = match session.as_mut() {
                Some(active) => &mut active.sm,
                None => phase,
            };
            let _ = sm.apply(SessionEvent::Ended);
            emit_state(event_tx, sm);
            let _ = event_tx.send(ControllerEvent::Ended { reason: end.reason });
            Ok(true)
        }
        Payload::SessionRevoke(revoke) => {
            let sm = match session.as_mut() {
                Some(active) => &mut active.sm,
                None => phase,
            };
            let _ = sm.apply(SessionEvent::Revoked);
            emit_state(event_tx, sm);
            let _ = event_tx.send(ControllerEvent::Ended {
                reason: revoke.reason,
            });
            Ok(true)
        }
        _ => Ok(false),
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_peer_event(
    event: PeerEvent,
    signaling: &mut SignalingClient,
    event_tx: &mpsc::UnboundedSender<ControllerEvent>,
    metrics: &Arc<SessionMetrics>,
    session: &mut Option<ControllerSession>,
    assembler: &mut FrameAssembler,
    last_rtt_ms: &mut u32,
) {
    // Register the control channel handle as soon as it opens.
    if let PeerEvent::ChannelOpened(label, channel) = &event {
        if label == peer::CONTROL_CHANNEL {
            tracing::info!("control channel open");
            if let Some(active) = session.as_mut() {
                active.register_control_channel(channel.clone());
            }
        } else if label == peer::VIDEO_CHANNEL {
            tracing::info!("video channel open");
        }
    }

    // ICE candidates gathered on the controller must reach the host (§7.2).
    if let PeerEvent::LocalCandidate(candidate) = &event
        && let Some(active) = session.as_ref()
    {
        let payload = IceCandidateJson {
            candidate: candidate.candidate.clone(),
            sdp_mid: candidate.sdp_mid.clone(),
            sdp_mline_index: candidate.sdp_mline_index,
        };
        let _ = signaling
            .send(
                &Envelope::new(MessageType::RtcIceCandidate, new_request_id(), payload)
                    .with_session(active.session_id.clone()),
            )
            .await;
    }

    let Some(active) = session.as_mut() else {
        return;
    };
    match event {
        PeerEvent::StateChanged(state) => {
            use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::*;
            match state {
                Connected => {
                    if active.sm.status() != SessionStatus::Active {
                        let _ = active.sm.apply(SessionEvent::Connected);
                        emit_state(event_tx, &active.sm);
                    }
                }
                Disconnected | Failed | Closed => {
                    let _ = active.sm.apply(SessionEvent::Disconnected);
                    emit_state(event_tx, &active.sm);
                    let _ = event_tx.send(ControllerEvent::Ended {
                        reason: Some("connection_lost".into()),
                    });
                }
                _ => {}
            }
        }
        PeerEvent::ControlMessage(text) => {
            if let Ok(frame) = ControlFrame::decode(&text) {
                match frame {
                    ControlFrame::Pong { sent_ms } => {
                        let rtt = (now_millis() as i64 - sent_ms).max(0) as u32;
                        metrics.observe_rtt_ms(rtt);
                        *last_rtt_ms = rtt;
                    }
                    ControlFrame::Stats(host) => {
                        let _ = event_tx.send(ControllerEvent::Stats {
                            host,
                            rtt_ms: *last_rtt_ms,
                        });
                    }
                    _ => {}
                }
            }
        }
        PeerEvent::VideoMessage(data) => {
            if let Ok(chunk) = VideoChunk::decode(&data) {
                active.stream_size = (chunk.width, chunk.height);
                if let Some((frame_id, width, height, jpeg)) = assembler.push(chunk) {
                    let _ = event_tx.send(ControllerEvent::Frame {
                        frame_id,
                        width,
                        height,
                        jpeg: Arc::new(jpeg),
                    });
                }
            }
        }
        PeerEvent::ChannelOpened(..) | PeerEvent::LocalCandidate(_) => {}
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
