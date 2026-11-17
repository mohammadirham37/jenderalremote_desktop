//! Strongly-typed identifiers used across the protocol.
//!
//! Device IDs are public identifiers. They are never secrets and never
//! proof of authorization (rancangan teknis §2, prinsip produk).

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! string_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

string_id!(
    DeviceId,
    "Public identifier of a registered device. Not a secret, not proof of authorization."
);
string_id!(SessionId, "Identifier of a remote session.");
string_id!(
    RequestId,
    "Correlation / idempotency identifier for a signaling message."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_roundtrip_through_json() {
        let id = DeviceId::new("dev-123");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"dev-123\"");
        let back: DeviceId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }
}
