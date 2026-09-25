//! Version negotiation (R01-T02 step 2/6, acceptance R01-A04).
//!
//! The new wire protocol version is its own axis, `lingxi.wire`, distinct
//! from the incumbent `shared/contract-versions.json` triplet
//! (PRELOAD_API_VERSION / SERVER_PROTOCOL_VERSION / DATA_EPOCH, all 1 at the
//! R00 baseline). See PROTOCOL_SPEC.md §2.
//!
//! Negotiation is range-intersection with **no default guessing**: the
//! client offers `[protocol_min, protocol_max]`; the server selects the
//! highest mutually supported version; a disjoint range fails with an
//! explicit `version_incompatible` error carrying the supported range in
//! `details`. The server never silently picks a version outside the
//! client's offer, and never proceeds without a selected version.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ErrorCode, ProtocolError};

/// Lowest wire protocol version this implementation can serve.
pub const WIRE_PROTOCOL_MIN_SUPPORTED: u32 = 1;
/// Highest wire protocol version this implementation can serve.
pub const WIRE_PROTOCOL_MAX_SUPPORTED: u32 = 1;

/// Wire name of the protocol family, carried in handshake diagnostics.
pub const WIRE_PROTOCOL_NAME: &str = "lingxi.wire";

/// Client → server handshake opening. Closed struct: unknown fields are a
/// hard `invalid_message` error, because guessing at negotiation inputs is
/// exactly the failure mode R01-A04 forbids.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientHello {
    /// Protocol family; must be `lingxi.wire`.
    pub protocol: String,
    /// Client kind, e.g. `desktop`, `cli`, `web`. Free-form, informational.
    pub client_kind: String,
    /// Client implementation version (informational, e.g. app version).
    pub client_version: String,
    /// Lowest wire protocol version the client can speak.
    pub protocol_min: u32,
    /// Highest wire protocol version the client can speak.
    pub protocol_max: u32,
    /// Optional capability tokens the client wants; unknown tokens are
    /// reported back in `ServerHello::rejected_caps`, never silently
    /// dropped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caps: Vec<String>,
}

/// Server → client handshake acceptance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerHello {
    pub protocol: String,
    /// The single negotiated wire protocol version.
    pub selected_protocol: u32,
    pub wire_protocol_min: u32,
    pub wire_protocol_max: u32,
    /// Data epoch of the server's stores (mirrors DATA_EPOCH; epoch
    /// migration strategy itself is R01-T07 scope).
    pub data_epoch: u32,
    /// Server identity for diagnostics (kind + version, informational).
    pub server_kind: String,
    pub server_version: String,
    /// Capability tokens from `ClientHello::caps` the server does not
    /// recognize. Reported, never silently dropped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected_caps: Vec<String>,
}

/// Negotiates a single wire protocol version.
///
/// Returns the highest mutually supported version, or a
/// `version_incompatible` error whose `details` carry
/// `client_min` / `client_max` / `supported_min` / `supported_max` so the
/// failure is diagnosable without logs.
pub fn negotiate_protocol(client: &ClientHello) -> Result<u32, ProtocolError> {
    if client.protocol != WIRE_PROTOCOL_NAME {
        return Err(ProtocolError::new(
            ErrorCode::InvalidMessage,
            format!(
                "unknown protocol family {:?}; expected {:?}",
                client.protocol, WIRE_PROTOCOL_NAME
            ),
            false,
        ));
    }
    if client.protocol_min > client.protocol_max {
        return Err(ProtocolError::new(
            ErrorCode::InvalidMessage,
            format!(
                "malformed version range: protocol_min {} > protocol_max {}",
                client.protocol_min, client.protocol_max
            ),
            false,
        ));
    }
    let lo = client.protocol_min.max(WIRE_PROTOCOL_MIN_SUPPORTED);
    let hi = client.protocol_max.min(WIRE_PROTOCOL_MAX_SUPPORTED);
    if lo <= hi {
        return Ok(hi);
    }
    Err(
        ProtocolError::new(
            ErrorCode::VersionIncompatible,
            format!(
                "client supports {WIRE_PROTOCOL_NAME} {}..={}, server supports {}..={}; \
                 no common version exists and no default is guessed",
                client.protocol_min,
                client.protocol_max,
                WIRE_PROTOCOL_MIN_SUPPORTED,
                WIRE_PROTOCOL_MAX_SUPPORTED,
            ),
            false,
        )
        .with_details(serde_json::Map::from_iter([
            ("protocol".into(), WIRE_PROTOCOL_NAME.into()),
            ("clientMin".into(), client.protocol_min.into()),
            ("clientMax".into(), client.protocol_max.into()),
            ("supportedMin".into(), WIRE_PROTOCOL_MIN_SUPPORTED.into()),
            ("supportedMax".into(), WIRE_PROTOCOL_MAX_SUPPORTED.into()),
        ])),
    )
}

/// Builds the success response for a negotiated hello.
pub fn accept_hello(
    client: &ClientHello,
    selected: u32,
    server_kind: &str,
    server_version: &str,
    data_epoch: u32,
    known_caps: &[&str],
) -> ServerHello {
    let rejected_caps: Vec<String> = client
        .caps
        .iter()
        .filter(|c| !known_caps.contains(&c.as_str()))
        .cloned()
        .collect();
    ServerHello {
        protocol: WIRE_PROTOCOL_NAME.to_string(),
        selected_protocol: selected,
        wire_protocol_min: WIRE_PROTOCOL_MIN_SUPPORTED,
        wire_protocol_max: WIRE_PROTOCOL_MAX_SUPPORTED,
        data_epoch,
        server_kind: server_kind.to_string(),
        server_version: server_version.to_string(),
        rejected_caps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello(min: u32, max: u32) -> ClientHello {
        ClientHello {
            protocol: WIRE_PROTOCOL_NAME.to_string(),
            client_kind: "desktop".into(),
            client_version: "0.0.0-test".into(),
            protocol_min: min,
            protocol_max: max,
            caps: vec![],
        }
    }

    #[test]
    fn intersection_selects_highest_common() {
        assert_eq!(negotiate_protocol(&hello(1, 1)).unwrap(), 1);
        assert_eq!(negotiate_protocol(&hello(0, 9)).unwrap(), 1);
    }

    #[test]
    fn disjoint_ranges_fail_explicitly() {
        for (min, max) in [(2u32, 2u32), (5, 9), (0, 0)] {
            let err = negotiate_protocol(&hello(min, max)).unwrap_err();
            assert_eq!(err.code, ErrorCode::VersionIncompatible);
            assert!(!err.retryable);
            let details = err.details.as_ref().expect("diagnosable details");
            assert_eq!(details["supportedMin"], 1);
            assert_eq!(details["supportedMax"], 1);
            assert_eq!(details["clientMin"], min);
            assert_eq!(details["clientMax"], max);
        }
    }

    #[test]
    fn wrong_family_and_malformed_range_are_invalid_message() {
        let mut h = hello(1, 1);
        h.protocol = "lingxi.legacy".into();
        assert_eq!(
            negotiate_protocol(&h).unwrap_err().code,
            ErrorCode::InvalidMessage
        );
        assert_eq!(
            negotiate_protocol(&hello(3, 2)).unwrap_err().code,
            ErrorCode::InvalidMessage
        );
    }

    #[test]
    fn unknown_caps_are_reported_not_dropped() {
        let mut h = hello(1, 1);
        h.caps = vec!["events.v1".into(), "teleport".into()];
        let hello_back = accept_hello(&h, 1, "lingxi-proto-server", "0.0.0", 1, &["events.v1"]);
        assert_eq!(hello_back.rejected_caps, vec!["teleport"]);
    }

    #[test]
    fn client_hello_rejects_unknown_fields() {
        let raw = r#"{"protocol":"lingxi.wire","clientKind":"cli","clientVersion":"1",
                      "protocolMin":1,"protocolMax":1,"smuggled":true}"#;
        assert!(serde_json::from_str::<ClientHello>(raw).is_err());
    }
}
