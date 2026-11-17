//! PeerConnection construction shared by host and controller.
//!
//! Channel policy (§7.3):
//! - `control` — ordered + reliable: input events, ping/pong, stats.
//! - `video`   — unordered, max retransmits 0: JPEG chunks (spike path).
//! - `clipboard` — ordered + reliable, opened only when the permission is
//!   granted (reserved for the next phase).
//!
//! Note: `webrtc` 0.14 handlers are async (`FnMut(..) -> Pin<Box<Future>>`),
//! so every callback below is `Box::pin(async move { .. })`; they only
//! forward into the engine's unbounded channel, so they never block.

use std::sync::Arc;
use webrtc::api::API;
use webrtc::data_channel::RTCDataChannel;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;

/// Events surfaced by WebRTC callbacks into the engine select loop.
///
/// Not `Debug` because `RTCDataChannel` does not implement it.
pub enum PeerEvent {
    StateChanged(RTCPeerConnectionState),
    LocalCandidate(RTCIceCandidateInit),
    /// A data channel opened, with its label and handle.
    ChannelOpened(String, Arc<RTCDataChannel>),
    ControlMessage(String),
    VideoMessage(Vec<u8>),
}

pub const CONTROL_CHANNEL: &str = "control";
pub const VIDEO_CHANNEL: &str = "video";
pub const CLIPBOARD_CHANNEL: &str = "clipboard";

/// Wires connection-state and ICE callbacks to `events`.
pub fn install_peer_callbacks(
    peer: &Arc<RTCPeerConnection>,
    events: tokio::sync::mpsc::UnboundedSender<PeerEvent>,
) {
    let state_events = events.clone();
    peer.on_peer_connection_state_change(Box::new(move |state| {
        let events = state_events.clone();
        Box::pin(async move {
            let _ = events.send(PeerEvent::StateChanged(state));
        })
    }));

    let candidate_events = events.clone();
    peer.on_ice_candidate(Box::new(move |candidate| {
        let events = candidate_events.clone();
        Box::pin(async move {
            if let Some(candidate) = candidate
                && let Ok(init) = candidate.to_json()
            {
                let _ = events.send(PeerEvent::LocalCandidate(init));
            }
        })
    }));
}

/// Host side: creates the peer plus both data channels (host is the
/// offerer, so channels are created locally).
pub async fn create_host_peer(
    api: &API,
    config: RTCConfiguration,
    events: tokio::sync::mpsc::UnboundedSender<PeerEvent>,
) -> Result<
    (
        Arc<RTCPeerConnection>,
        Arc<RTCDataChannel>,
        Arc<RTCDataChannel>,
    ),
    webrtc::Error,
> {
    let peer = Arc::new(api.new_peer_connection(config).await?);
    install_peer_callbacks(&peer, events.clone());

    let control = peer
        .create_data_channel(
            CONTROL_CHANNEL,
            Some(RTCDataChannelInit {
                ordered: Some(true),
                ..Default::default()
            }),
        )
        .await?;
    let video = peer
        .create_data_channel(
            VIDEO_CHANNEL,
            Some(RTCDataChannelInit {
                ordered: Some(false),
                max_retransmits: Some(0),
                ..Default::default()
            }),
        )
        .await?;

    // Surface readiness (with handles) for the engine.
    let control_for_open = events.clone();
    let control_handle = control.clone();
    control.on_open(Box::new(move || {
        let events = control_for_open.clone();
        let handle = control_handle.clone();
        Box::pin(async move {
            let _ = events.send(PeerEvent::ChannelOpened(CONTROL_CHANNEL.into(), handle));
        })
    }));
    // The host receives input events on the control channel.
    let control_for_msg = events.clone();
    control.on_message(Box::new(move |message| {
        let events = control_for_msg.clone();
        Box::pin(async move {
            if message.is_string {
                let text = String::from_utf8_lossy(&message.data).into_owned();
                let _ = events.send(PeerEvent::ControlMessage(text));
            }
        })
    }));
    let video_for_open = events.clone();
    let video_handle = video.clone();
    video.on_open(Box::new(move || {
        let events = video_for_open.clone();
        let handle = video_handle.clone();
        Box::pin(async move {
            let _ = events.send(PeerEvent::ChannelOpened(VIDEO_CHANNEL.into(), handle));
        })
    }));

    Ok((peer, control, video))
}

/// Controller side: creates the peer and registers handlers for channels
/// created by the host.
pub async fn create_controller_peer(
    api: &API,
    config: RTCConfiguration,
    events: tokio::sync::mpsc::UnboundedSender<PeerEvent>,
) -> Result<Arc<RTCPeerConnection>, webrtc::Error> {
    let peer = Arc::new(api.new_peer_connection(config).await?);
    install_peer_callbacks(&peer, events.clone());

    peer.on_data_channel(Box::new(move |channel: Arc<RTCDataChannel>| {
        let events = events.clone();
        Box::pin(async move {
            let label = channel.label().to_string();
            match label.as_str() {
                CONTROL_CHANNEL => {
                    let open_events = events.clone();
                    let open_handle = channel.clone();
                    channel.on_open(Box::new(move || {
                        let events = open_events.clone();
                        let handle = open_handle.clone();
                        Box::pin(async move {
                            let _ = events
                                .send(PeerEvent::ChannelOpened(CONTROL_CHANNEL.into(), handle));
                        })
                    }));
                    let message_events = events.clone();
                    channel.on_message(Box::new(move |message| {
                        let events = message_events.clone();
                        Box::pin(async move {
                            if message.is_string {
                                let text = String::from_utf8_lossy(&message.data).into_owned();
                                let _ = events.send(PeerEvent::ControlMessage(text));
                            }
                        })
                    }));
                }
                VIDEO_CHANNEL => {
                    let open_events = events.clone();
                    let open_handle = channel.clone();
                    channel.on_open(Box::new(move || {
                        let events = open_events.clone();
                        let handle = open_handle.clone();
                        Box::pin(async move {
                            let _ =
                                events.send(PeerEvent::ChannelOpened(VIDEO_CHANNEL.into(), handle));
                        })
                    }));
                    let message_events = events.clone();
                    channel.on_message(Box::new(move |message| {
                        let events = message_events.clone();
                        Box::pin(async move {
                            let _ = events.send(PeerEvent::VideoMessage(message.data.to_vec()));
                        })
                    }));
                }
                CLIPBOARD_CHANNEL => {
                    // Clipboard traffic is phase 2; ignore payloads for now.
                    let open_events = events.clone();
                    let open_handle = channel.clone();
                    channel.on_open(Box::new(move || {
                        let events = open_events.clone();
                        let handle = open_handle.clone();
                        Box::pin(async move {
                            let _ = events
                                .send(PeerEvent::ChannelOpened(CLIPBOARD_CHANNEL.into(), handle));
                        })
                    }));
                }
                other => {
                    tracing::debug!(channel = other, "unknown data channel opened");
                }
            }
        })
    }));

    Ok(peer)
}
