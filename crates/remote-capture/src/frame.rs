//! Frame representation shared by capture, encoder, and transport.

use std::sync::Arc;
use std::time::Instant;

/// Pixel layout of the frame buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameFormat {
    Rgba8,
    Bgra8,
}

/// A captured frame. `data.len() == width * height * 4`.
#[derive(Debug)]
pub struct CapturedFrame {
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub format: FrameFormat,
    pub data: Arc<Vec<u8>>,
    pub captured_at: Instant,
}

impl CapturedFrame {
    pub fn new(sequence: u64, width: u32, height: u32, format: FrameFormat, data: Vec<u8>) -> Self {
        debug_assert_eq!(data.len(), (width * height * 4) as usize);
        Self {
            sequence,
            width,
            height,
            format,
            data: Arc::new(data),
            captured_at: Instant::now(),
        }
    }
}

/// Converts BGRA8 pixels to RGBA8 in place (channel swap 0<->2).
pub fn bgra_to_rgba(data: &mut [u8]) {
    for px in data.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_swap() {
        let mut data = [1u8, 2, 3, 4]; // B,G,R,A
        bgra_to_rgba(&mut data);
        assert_eq!(data, [3, 2, 1, 4]); // R,G,B,A
    }
}
