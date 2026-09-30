//! Canonical JSON (`lingxi-canonical-json-v1`) — the single serialization
//! profile used for golden fixtures, content digests and cross-language
//! byte-equality checks.
//!
//! Profile definition (must match the TypeScript consumer in
//! `tests/migration/r01-t02/canonical-json.mjs` byte-for-byte):
//!   - UTF-8, no whitespace, no trailing newline;
//!   - object keys sorted by UTF-16 CODE UNIT sequence — the exact order
//!     of the TS consumer's `Object.keys().sort()` (whose default
//!     comparison walks UTF-16 code units). R04-T01 closes RR-T02-F1:
//!     code-point order (the old serde_json BTreeMap byte order) and
//!     UTF-16 unit order DISAGREE exactly when an astral key (≥ U+10000,
//!     encoded as a surrogate pair D800–DFFF) is compared with a key in
//!     U+E000..=U+FFFF; the two implementations must agree for every key
//!     shape, so this writer sorts by UTF-16 units and the divergence
//!     class has a golden regression below;
//!   - non-ASCII characters emitted raw (no `\uXXXX` escaping);
//!   - integers only; the protocol wire carries no floats and no u64
//!     numbers (u64 quantities are decimal strings, see `Seq`). The TS
//!     side hard-fails on non-safe-integer numbers; the Rust boundaries
//!     that consume digests over untrusted values (R04-T01 tool
//!     arguments) enforce the same safe-integer rejection themselves.

use serde::Serialize;

/// Identifier of this canonicalization profile, embedded in digests.
pub const CANONICALIZATION_ID: &str = "lingxi-canonical-json-v1";

/// Serializes a `serde_json::Value` to canonical bytes.
pub fn canonical_json_bytes(value: &serde_json::Value) -> Vec<u8> {
    let mut out = Vec::new();
    write_canonical(value, &mut out);
    out
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

fn write_canonical(value: &serde_json::Value, out: &mut Vec<u8>) {
    match value {
        serde_json::Value::Null => out.extend_from_slice(b"null"),
        serde_json::Value::Bool(true) => out.extend_from_slice(b"true"),
        serde_json::Value::Bool(false) => out.extend_from_slice(b"false"),
        serde_json::Value::Number(number) => {
            // serde_json's number form is the wire form (decimal integers;
            // the profile carries no floats/u64 — the boundaries that
            // digest untrusted values enforce that separately).
            let text = number.to_string();
            out.extend_from_slice(text.as_bytes());
        }
        serde_json::Value::String(text) => write_json_string(text, out),
        serde_json::Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        serde_json::Value::Object(map) => {
            // RR-T02-F1: UTF-16 code-unit key order (TS
            // `Array.prototype.sort()` parity), NOT the map's BTreeMap
            // byte order.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| cmp_utf16(a, b));
            out.push(b'{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_json_string(key, out);
                out.push(b':');
                write_canonical(&map[*key], out);
            }
            out.push(b'}');
        }
    }
}

/// Orders two strings by their UTF-16 code unit sequences — the order of
/// the TS consumer's default string comparison. Differs from Rust's
/// `str` Ord (= UTF-8 byte order = code-point order) exactly when an
/// astral character (surrogate pair) meets a character in U+E000..=U+FFFF.
fn cmp_utf16(a: &str, b: &str) -> std::cmp::Ordering {
    let mut left = a.encode_utf16();
    let mut right = b.encode_utf16();
    loop {
        match (left.next(), right.next()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(u), Some(v)) => match u.cmp(&v) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            },
        }
    }
}

/// Writes a JSON string byte-identically to serde_json's compact form
/// (and to TS `JSON.stringify` for well-formed strings): quote/backslash
/// escaped, the five short control escapes, `\u00xx` (lowercase hex) for
/// other control characters, non-ASCII raw.
fn write_json_string(text: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for c in text.chars() {
        match c {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{08}' => out.extend_from_slice(b"\\b"),
            '\u{09}' => out.extend_from_slice(b"\\t"),
            '\u{0a}' => out.extend_from_slice(b"\\n"),
            '\u{0c}' => out.extend_from_slice(b"\\f"),
            '\u{0d}' => out.extend_from_slice(b"\\r"),
            c if (c as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes());
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
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
            r#"{"a":"灵犀","b":1,"m":[3,2,1],"z":null}"#.as_bytes().to_vec()
        );
    }

    #[test]
    fn canonical_form_has_no_whitespace_or_trailing_newline() {
        let v = json!({"x": {"y": [1]}});
        let s = String::from_utf8(canonical_json_bytes(&v)).unwrap();
        assert_eq!(s, r#"{"x":{"y":[1]}}"#);
        assert!(!s.ends_with('\n'));
    }

    /// RR-T02-F1 golden regression (R04-T01 closure): an astral key
    /// (U+1F004, UTF-16 surrogate pair D83D DE04) sorts BEFORE a key in
    /// the private-use area (U+E000) under UTF-16 code-unit order — the
    /// TS consumer's order — while the old code-point/byte order sorted
    /// them the other way. The two implementations must agree for every
    /// key shape.
    #[test]
    fn non_bmp_keys_sort_in_utf16_unit_order_ts_parity() {
        let astral = "\u{1F004}"; // surrogate pair 0xD83D 0xDE04
        let private_use = "\u{E000}"; // 0xE000
        let ascii = "z";
        let v = json!({ (private_use): 1, (astral): 2, (ascii): 3 });
        let s = String::from_utf8(canonical_json_bytes(&v)).unwrap();
        // UTF-16 units: D83D… < E000 < 'z'(0x7A)? No: 'z' is 0x7A, which is
        // LESS than both D83D and E000 — so the order is z, astral, PUA.
        let expected = format!(
            "{{\"{ascii}\":3,\"{astral}\":2,\"{private_use}\":1}}",
            ascii = ascii,
            astral = astral,
            private_use = private_use
        );
        assert_eq!(s, expected);
        // And the divergence case directly: astral BEFORE private use
        // (code-point order would say the opposite).
        let pair = json!({ (private_use): 1, (astral): 2 });
        let pair_text = String::from_utf8(canonical_json_bytes(&pair)).unwrap();
        assert!(
            pair_text.starts_with(&format!("{{\"{astral}\"")),
            "astral key must sort first: {pair_text}"
        );
    }

    /// The serialized bytes stay identical to serde_json's compact form
    /// for ordinary values (the writer only changed KEY ORDER, never the
    /// scalar encodings).
    #[test]
    fn scalar_encodings_match_serde_json_compact_form() {
        for v in [
            json!({"s": "quote \" backslash \\ newline \n ctl \u{1} bmp 灵犀"}),
            json!({"i": 0, "neg": -42, "big": 9007199254740991i64}),
            json!([true, false, null]),
        ] {
            assert_eq!(
                canonical_json_bytes(&v),
                serde_json::to_vec(&v).expect("serde_json compact form"),
                "canonical scalar encodings must equal serde_json's compact form"
            );
        }
    }
}
