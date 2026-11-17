//! JPEG encoding for the spike video path (RGBA8 → JPEG bytes).

use crate::error::EngineError;
use image::ExtendedColorType;
use image::codecs::jpeg::JpegEncoder;

/// Encodes an RGBA8 buffer to JPEG with the given quality (1-100).
pub fn encode_jpeg(
    rgba: &[u8],
    width: u32,
    height: u32,
    quality: u8,
) -> Result<Vec<u8>, EngineError> {
    // JPEG has no alpha: strip to RGB first (also halves the input size).
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for px in rgba.as_chunks::<4>().0 {
        rgb.extend_from_slice(&px[..3]);
    }
    let mut out = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
    encoder
        .encode(&rgb, width, height, ExtendedColorType::Rgb8)
        .map_err(|e| EngineError::WebRtc(format!("jpeg encode: {e}")))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_solid_frame() {
        let w = 32;
        let h = 24;
        let rgba: Vec<u8> = (0..w * h).flat_map(|_| [200u8, 30, 40, 255]).collect();
        let jpeg = encode_jpeg(&rgba, w, h, 60).unwrap();
        assert!(!jpeg.is_empty());
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]); // JPEG SOI
    }
}
