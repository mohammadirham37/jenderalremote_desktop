//! Protocol version constants.
//!
//! Every envelope carries a `version` field. Receivers must reject messages
//! with an unsupported major version instead of attempting to parse the
//! payload (defense-in-depth alongside schema validation).

/// Current protocol version emitted by this build.
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum signaling message size accepted from any peer (256 KiB).
/// Larger messages must be rejected before parsing to bound memory use.
pub const MAX_MESSAGE_SIZE: usize = 256 * 1024;

/// Returns true when `version` can be handled by this build.
pub fn is_supported(version: u32) -> bool {
    version == PROTOCOL_VERSION
}
