//! Media-plane framing used over WebRTC DataChannels.
//!
//! MVP (fase 0/1, lihat `docs/adr/0001-video-over-datachannel.md`):
//! - `control` channel: ordered + reliable, carries input events, ping/pong
//!   and host stats as compact JSON text frames.
//! - `video` channel: unordered + unreliable (max retransmits 0), carries
//!   JPEG frames as binary chunks. Stale/incomplete frames are dropped by
//!   the reassembler using monotonic frame ids.
//!
//! Input events MUST be ordered and reliable (rancangan teknis §7.3).

use serde::{Deserialize, Serialize};

/// First byte of every binary frame on the video channel.
pub const VIDEO_CHUNK_KIND: u8 = 0x01;

/// Maximum payload per SCTP message; kept below the 64 KiB message limit.
pub const MAX_CHUNK_PAYLOAD: usize = 60_000;

/// Header size: kind(1) + frame_id(4) + chunk_index(2) + total_chunks(2)
/// + width(2) + height(2).
const VIDEO_HEADER_SIZE: usize = 13;

// ---------------------------------------------------------------------------
// Input events (shared by controller UI, host policy gate, native input)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseAction {
    Press,
    Release,
    Move,
    Scroll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Meta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyAction {
    Press,
    Release,
}

/// Normalized mouse event. Coordinates are host-screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MouseEvent {
    pub action: MouseAction,
    pub button: MouseButton,
    pub x: f64,
    pub y: f64,
    /// Scroll deltas; positive = right/down.
    pub scroll_dx: f64,
    pub scroll_dy: f64,
}

/// Normalized keyboard event. `key` is a single unicode character or a
/// named key (`"Enter"`, `"Backspace"`, `"Shift"`, `"ArrowLeft"`, ...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardEvent {
    pub action: KeyAction,
    pub key: String,
    pub modifiers: Vec<Modifier>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputEvent {
    Mouse(MouseEvent),
    Keyboard(KeyboardEvent),
}

// ---------------------------------------------------------------------------
// Control channel frames (JSON text)
// ---------------------------------------------------------------------------

/// Runtime stats pushed by the host so the controller can display quality.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HostStats {
    pub fps: f32,
    pub encode_ms: f32,
    pub capture_ms: f32,
    pub frames_skipped: u64,
}

/// Frames exchanged on the `control` DataChannel.
///
/// Input events are split into `Mouse`/`Keyboard` variants instead of a
/// nested tagged enum to keep a single `type` discriminator on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlFrame {
    Mouse(MouseEvent),
    Keyboard(KeyboardEvent),
    /// Latency probe; host echoes `sent_ms` untouched inside `Pong`.
    Ping {
        sent_ms: i64,
    },
    Pong {
        sent_ms: i64,
    },
    Stats(HostStats),
}

impl ControlFrame {
    /// Converts to the shared [`InputEvent`] domain value when this frame
    /// carries input.
    pub fn as_input_event(&self) -> Option<InputEvent> {
        match self {
            ControlFrame::Mouse(event) => Some(InputEvent::Mouse(*event)),
            ControlFrame::Keyboard(event) => Some(InputEvent::Keyboard(event.clone())),
            _ => None,
        }
    }
}

impl ControlFrame {
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("control frame is JSON-serializable")
    }

    pub fn decode(data: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(data)
    }
}

// ---------------------------------------------------------------------------
// Video chunk framing (binary)
// ---------------------------------------------------------------------------

/// One SCTP-sized piece of a JPEG frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoChunk {
    pub frame_id: u32,
    pub chunk_index: u16,
    pub total_chunks: u16,
    pub width: u16,
    pub height: u16,
    pub data: Vec<u8>,
}

impl VideoChunk {
    /// Serializes the chunk with its binary header.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(VIDEO_HEADER_SIZE + self.data.len());
        buf.push(VIDEO_CHUNK_KIND);
        buf.extend_from_slice(&self.frame_id.to_be_bytes());
        buf.extend_from_slice(&self.chunk_index.to_be_bytes());
        buf.extend_from_slice(&self.total_chunks.to_be_bytes());
        buf.extend_from_slice(&self.width.to_be_bytes());
        buf.extend_from_slice(&self.height.to_be_bytes());
        buf.extend_from_slice(&self.data);
        buf
    }

    /// Parses a binary chunk previously produced by [`VideoChunk::encode`].
    pub fn decode(data: &[u8]) -> Result<Self, String> {
        if data.len() < VIDEO_HEADER_SIZE {
            return Err(format!(
                "video chunk too short: {} bytes (need >= {VIDEO_HEADER_SIZE})",
                data.len()
            ));
        }
        if data[0] != VIDEO_CHUNK_KIND {
            return Err(format!("unexpected frame kind 0x{:02x}", data[0]));
        }
        let be = |range: std::ops::Range<usize>| -> Vec<u8> { data[range].to_vec() };
        let u16be = |offset: usize| u16::from_be_bytes([data[offset], data[offset + 1]]);
        let frame_id = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);
        Ok(VideoChunk {
            frame_id,
            chunk_index: u16be(5),
            total_chunks: u16be(7),
            width: u16be(9),
            height: u16be(11),
            data: be(VIDEO_HEADER_SIZE..data.len()),
        })
    }
}

/// Splits one JPEG frame into wire-ready chunks. An empty frame still
/// produces a single (empty-payload) chunk.
pub fn chunk_frame(frame_id: u32, width: u16, height: u16, jpeg: &[u8]) -> Vec<Vec<u8>> {
    let pieces: Vec<Vec<u8>> = if jpeg.is_empty() {
        vec![Vec::new()]
    } else {
        jpeg.chunks(MAX_CHUNK_PAYLOAD).map(<[u8]>::to_vec).collect()
    };
    let total = pieces.len() as u16;
    let mut out = Vec::with_capacity(pieces.len());
    for (index, part) in pieces.into_iter().enumerate() {
        let chunk = VideoChunk {
            frame_id,
            chunk_index: index as u16,
            total_chunks: total,
            width,
            height,
            data: part,
        };
        out.push(chunk.encode());
    }
    out
}

/// Reassembles video chunks into complete frames.
///
/// Because the video channel is unreliable, an incomplete frame is dropped
/// as soon as a newer frame id starts arriving, and older frame ids are
/// ignored entirely. Output frames are therefore monotonic.
#[derive(Debug, Default)]
pub struct FrameAssembler {
    last_rendered: u32,
    current_id: Option<u32>,
    total_chunks: u16,
    received: Vec<bool>,
    parts: Vec<Option<Vec<u8>>>,
    width: u16,
    height: u16,
}

impl FrameAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pushes a decoded chunk; returns `Some((frame_id, width, height, jpeg))`
    /// when this chunk completes the current frame.
    pub fn push(&mut self, chunk: VideoChunk) -> Option<(u32, u16, u16, Vec<u8>)> {
        if chunk.total_chunks == 0 {
            return None;
        }
        match self.current_id {
            // Stale frame id: ignore entirely.
            Some(current) if chunk.frame_id < current => return None,
            // A newer frame id starts arriving: drop the incomplete one.
            Some(current) if chunk.frame_id > current => self.begin(&chunk),
            Some(_) => {}
            None => {
                if chunk.frame_id <= self.last_rendered {
                    return None;
                }
                self.begin(&chunk);
            }
        }

        let idx = chunk.chunk_index as usize;
        if idx >= self.received.len() {
            return None;
        }
        if !self.received[idx] {
            self.received[idx] = true;
            self.parts[idx] = Some(chunk.data);
        }
        if self.received.iter().all(|&ok| ok) {
            self.finish(chunk.frame_id)
        } else {
            None
        }
    }

    fn begin(&mut self, chunk: &VideoChunk) {
        self.current_id = Some(chunk.frame_id);
        self.total_chunks = chunk.total_chunks;
        self.received = vec![false; chunk.total_chunks as usize];
        self.parts = vec![None; chunk.total_chunks as usize];
        self.width = chunk.width;
        self.height = chunk.height;
    }

    fn finish(&mut self, frame_id: u32) -> Option<(u32, u16, u16, Vec<u8>)> {
        let width = self.width;
        let height = self.height;
        let mut jpeg = Vec::new();
        for part in self.parts.iter_mut() {
            jpeg.append(&mut part.take().expect("all chunks received"));
        }
        self.last_rendered = frame_id;
        self.current_id = None;
        self.received.clear();
        self.parts.clear();
        Some((frame_id, width, height, jpeg))
    }

    pub fn last_rendered_frame(&self) -> u32 {
        self.last_rendered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk_bytes(frame_id: u32, index: u16, total: u16, data: &[u8]) -> Vec<u8> {
        VideoChunk {
            frame_id,
            chunk_index: index,
            total_chunks: total,
            width: 1920,
            height: 1080,
            data: data.to_vec(),
        }
        .encode()
    }

    #[test]
    fn chunk_roundtrip() {
        let raw = chunk_bytes(7, 1, 3, b"hello");
        let parsed = VideoChunk::decode(&raw).unwrap();
        assert_eq!(parsed.frame_id, 7);
        assert_eq!(parsed.chunk_index, 1);
        assert_eq!(parsed.total_chunks, 3);
        assert_eq!(parsed.width, 1920);
        assert_eq!(parsed.data, b"hello");
    }

    #[test]
    fn chunk_rejects_short_and_wrong_kind() {
        assert!(VideoChunk::decode(&[0x01, 0x00]).is_err());
        let mut raw = chunk_bytes(1, 0, 1, b"x");
        raw[0] = 0x99;
        assert!(VideoChunk::decode(&raw).is_err());
    }

    #[test]
    fn chunker_splits_large_frames() {
        let jpeg = vec![0xAB; MAX_CHUNK_PAYLOAD * 2 + 100];
        let chunks = chunk_frame(3, 800, 600, &jpeg);
        assert_eq!(chunks.len(), 3);

        let mut asm = FrameAssembler::new();
        let mut completed = None;
        for raw in chunks {
            if let Some((id, w, h, data)) = asm.push(VideoChunk::decode(&raw).unwrap()) {
                completed = Some((id, w, h, data));
            }
        }
        let (id, w, h, data) = completed.expect("frame should complete");
        assert_eq!((id, w, h), (3, 800, 600));
        assert_eq!(data, jpeg);
    }

    #[test]
    fn assembler_drops_incomplete_frame_when_newer_arrives() {
        let mut asm = FrameAssembler::new();
        // frame 1 chunk 0 of 2 — never completed
        assert!(
            asm.push(VideoChunk::decode(&chunk_bytes(1, 0, 2, b"aa")).unwrap())
                .is_none()
        );
        // frame 2 arrives complete (single chunk) — frame 1 is dropped
        let done = asm.push(VideoChunk::decode(&chunk_bytes(2, 0, 1, b"bb")).unwrap());
        assert!(done.is_some());
        // stale frame 1 chunk must be ignored afterwards
        assert!(
            asm.push(VideoChunk::decode(&chunk_bytes(1, 1, 2, b"cc")).unwrap())
                .is_none()
        );
    }

    #[test]
    fn control_frame_roundtrip() {
        let frame = ControlFrame::Mouse(MouseEvent {
            action: MouseAction::Move,
            button: MouseButton::Left,
            x: 12.5,
            y: 30.0,
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        });
        let encoded = frame.encode();
        let decoded = ControlFrame::decode(&encoded).unwrap();
        assert_eq!(frame, decoded);
        assert_eq!(
            decoded.as_input_event(),
            Some(InputEvent::Mouse(MouseEvent {
                action: MouseAction::Move,
                button: MouseButton::Left,
                x: 12.5,
                y: 30.0,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            }))
        );

        let keyboard = ControlFrame::Keyboard(KeyboardEvent {
            action: KeyAction::Press,
            key: "a".into(),
            modifiers: vec![Modifier::Ctrl],
        });
        assert_eq!(ControlFrame::decode(&keyboard.encode()).unwrap(), keyboard);

        let ping = ControlFrame::Ping { sent_ms: 1234 };
        assert_eq!(ControlFrame::decode(&ping.encode()).unwrap(), ping);
    }

    #[test]
    fn chunk_frame_zero_length() {
        let chunks = chunk_frame(9, 10, 10, b"");
        assert_eq!(chunks.len(), 1);
    }
}
