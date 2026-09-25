//! Canonical JSON (`lingxi-canonical-json-v1`) — the single serialization
//! profile used for golden fixtures, content digests and cross-language
//! byte-equality checks.
//!
//! Profile definition (must match the TypeScript consumer in
//! `tests/migration/r01-t02/canonical-json.mjs` byte-for-byte):
//!   - UTF-8, no whitespace, no trailing newline;
//!   - object keys sorted by Unicode code point (serde_json's default map
//!     is a BTreeMap, so iterating `Value::Object` is already sorted);
//!   - non-ASCII characters emitted raw (no `\uXXXX` escaping);
//!   - integers only; the protocol wire carries no floats and no u64
//!     numbers (u64 quantities are decimal strings, see `Seq`).

use serde::Serialize;

/// Identifier of this canonicalization profile, embedded in digests.
pub const CANONICALIZATION_ID: &str = "lingxi-canonical-json-v1";

/// Serializes a `serde_json::Value` to canonical bytes.
pub fn canonical_json_bytes(value: &serde_json::Value) -> Vec<u8> {
    // serde_json without the `preserve_order` feature stores objects in a
    // BTreeMap, i.e. key-sorted; `to_vec` is the compact form with raw
    // UTF-8 output. That is exactly the canonical profile above.
    serde_json::to_vec(value).expect("serializing a Value cannot fail")
}

/// Serializes any `Serialize` type to canonical bytes (via `Value`, so map
/// ordering is normalized even for flattened/catch-all maps).
pub fn canonical_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    let v = serde_json::to_value(value).expect("wire types must serialize to JSON");
    canonical_json_bytes(&v)
}

/// Canonical string form.
pub fn canonical_string<T: Serialize>(value: &T) -> String {
    String::from_utf8(canonical_bytes(value)).expect("canonical JSON is UTF-8")
}

/// SHA-256 hex of the canonical form of `value` — the `args_digest` /
/// content digest primitive shared with the TS consumer.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Digest of the canonical JSON form of a value (e.g. normalized tool
/// arguments, per contract §5 approval binding).
pub fn canonical_sha256_hex(value: &serde_json::Value) -> String {
    sha256_hex(&canonical_json_bytes(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_form_sorts_keys_and_keeps_utf8() {
        let v = json!({"b": 1, "a": "灵犀", "z": null, "m": [3, 2, 1]});
        assert_eq!(
            canonical_json_bytes(&v),
            r#"{"a":"灵犀","b":1,"m":[3,2,1],"z":null}"#
                .as_bytes()
                .to_vec()
        );
    }

    #[test]
    fn canonical_form_has_no_whitespace_or_trailing_newline() {
        let v = json!({"x": {"y": [1]}});
        let s = String::from_utf8(canonical_json_bytes(&v)).unwrap();
        assert_eq!(s, r#"{"x":{"y":[1]}}"#);
        assert!(!s.ends_with('\n'));
    }
}
