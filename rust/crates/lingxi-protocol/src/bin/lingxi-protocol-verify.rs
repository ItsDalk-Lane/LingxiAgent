//! lingxi-protocol-verify — the Rust leg of the cross-language round-trip
//! gate (R01-A03).
//!
//!   1. Reads every golden file (Rust-encoded canonical bytes).
//!   2. Deserializes it into the authoritative Rust type named in
//!      `golden/index.json`, re-serializes canonically, and asserts
//!      byte-equality (Rust encode → Rust read closure).
//!   3. Reads the TypeScript consumer's re-encoded copy from `--ts DIR` and
//!      asserts byte-equality with the golden (Rust encode → TS decode →
//!      TS encode → Rust read closure).
//!
//! Exit 0 only if every sample passes both closures.

use std::path::Path;
use std::process::ExitCode;

use lingxi_protocol::canon::{canonical_bytes, sha256_hex};
use lingxi_protocol::*;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Deserialize `bytes` into the authoritative type `type_name`, re-encode
/// canonically, and return the re-encoded bytes.
fn reencode_typed(type_name: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    fn go<T: DeserializeOwned + serde::Serialize>(bytes: &[u8]) -> Result<Vec<u8>, String> {
        let value: T = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        Ok(canonical_bytes(&value))
    }
    match type_name {
        "ClientHello" => go::<ClientHello>(bytes),
        "ServerHello" => go::<ServerHello>(bytes),
        "ProtocolError" => go::<ProtocolError>(bytes),
        "ContractVersions" => go::<ContractVersions>(bytes),
        "EventEnvelope" => go::<EventEnvelope>(bytes),
        "HistoryPage" => go::<Page<HistoryEntry>>(bytes),
        "ModelRequest" => go::<ModelRequest>(bytes),
        "ApprovalRequest" => go::<ApprovalRequest>(bytes),
        "ApprovalDecision" => go::<ApprovalDecision>(bytes),
        "ToolSchemaDocument" => go::<ToolSchemaDocument>(bytes),
        other => Err(format!("no authoritative type registered for {other:?}")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let golden_dir = args
        .windows(2)
        .find(|w| w[0] == "--golden")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(3)
                .unwrap()
                .join("contracts/generated/golden")
                .to_string_lossy()
                .into_owned()
        });
    let ts_dir = args
        .windows(2)
        .find(|w| w[0] == "--ts")
        .map(|w| w[1].clone());

    let index_path = Path::new(&golden_dir).join("index.json");
    let index: Value = serde_json::from_slice(
        &std::fs::read(&index_path).expect("read golden/index.json"),
    )
    .expect("parse golden/index.json");

    let mut failures: Vec<String> = Vec::new();
    let mut count = 0usize;

    for sample in index["samples"].as_array().expect("samples array") {
        count += 1;
        let file = sample["file"].as_str().unwrap();
        let type_name = sample["type"].as_str().unwrap();
        let expected_sha = sample["sha256"].as_str().unwrap();
        let golden_path = Path::new(&golden_dir).join(file);
        let golden = std::fs::read(&golden_path).expect("read golden file");

        if sha256_hex(&golden) != expected_sha {
            failures.push(format!("{file}: sha256 mismatch vs golden/index.json"));
        }

        match reencode_typed(type_name, &golden) {
            Ok(re) if re == golden => {}
            Ok(re) => failures.push(format!(
                "{file}: Rust decode→encode drift ({} vs {} bytes)",
                re.len(),
                golden.len()
            )),
            Err(e) => failures.push(format!("{file}: Rust decode failed: {e}")),
        }

        if let Some(dir) = &ts_dir {
            let ts_path = Path::new(dir).join(file);
            match std::fs::read(&ts_path) {
                Ok(ts_bytes) if ts_bytes == golden => {}
                Ok(ts_bytes) => {
                    // Diagnose: does the TS output at least parse as the same
                    // typed value (semantic equality) even if bytes differ?
                    let note = match reencode_typed(type_name, &ts_bytes) {
                        Ok(re) if re == golden => {
                            " (semantically equal after Rust re-canonicalization; \
                             TS canonicalization differs)"
                        }
                        _ => " (NOT semantically equal)",
                    };
                    failures.push(format!(
                        "{file}: TS re-encoded bytes differ from golden{note}"
                    ));
                }
                Err(e) => failures.push(format!("{file}: TS re-encoded copy unreadable: {e}")),
            }
        }

        // Sample-specific checks recorded in golden/index.json
        for check in sample["checks"].as_array().map(|v| v.as_slice()).unwrap_or(&[]) {
            if check["kind"] == "big_seq_string" {
                let v: Value = serde_json::from_slice(&golden).unwrap();
                let seq = v["seq"].as_str().expect("seq must be a string");
                let parsed: u64 = seq.parse().expect("seq must be a u64 decimal string");
                if parsed <= (1u64 << 53) - 1 {
                    failures.push(format!(
                        "{file}: big_seq_string sample does not exceed 2^53-1"
                    ));
                }
            }
        }
    }

    if failures.is_empty() {
        println!(
            "OK: {count} golden samples verified (Rust decode→encode byte-stable{})",
            if ts_dir.is_some() {
                "; TS re-encoded copies byte-identical"
            } else {
                ""
            }
        );
        ExitCode::SUCCESS
    } else {
        eprintln!("ROUND-TRIP FAILURES ({}):", failures.len());
        for f in &failures {
            eprintln!("  - {f}");
        }
        ExitCode::FAILURE
    }
}
