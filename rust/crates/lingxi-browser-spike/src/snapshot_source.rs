//! Extract the production SNAPSHOT_SCRIPT (AXTree-like DOM walker that injects
//! data-hana-ref attributes) from desktop/main.cjs at runtime, read-only, so
//! the spike and the production browser perceive pages with the SAME algorithm.
//! The sha256 of the extracted script is recorded in the evidence transcript.

use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug)]
pub struct SnapshotSource {
    pub script: String,
    pub sha256: String,
    pub source_file: String,
    pub source_lines: (usize, usize),
}

pub fn extract_snapshot_script(repo_root: &Path) -> Result<SnapshotSource, String> {
    let file = repo_root.join("desktop/main.cjs");
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let lines: Vec<&str> = text.lines().collect();
    let marker = "const SNAPSHOT_SCRIPT = `";
    let start_idx = lines
        .iter()
        .position(|l| l.starts_with(marker))
        .ok_or("SNAPSHOT_SCRIPT marker not found")?;
    // template ends at the line "})()`;" that terminates the IIFE literal
    let mut end_idx = None;
    for (i, l) in lines.iter().enumerate().skip(start_idx + 1) {
        if l.trim_end() == "})()`;" {
            end_idx = Some(i);
            break;
        }
    }
    let end_idx = end_idx.ok_or("SNAPSHOT_SCRIPT terminator not found")?;
    // script body = between the backticks: from start line after the marker to
    // the last backtick of the end line
    let first_line_body = lines[start_idx][marker.len()..].to_string();
    let mut body_lines = vec![first_line_body];
    for l in &lines[start_idx + 1..=end_idx] {
        body_lines.push((*l).to_string());
    }
    let mut body = body_lines.join("\n");
    // the terminator line is "})()`;" — strip the closing "`;" of the template
    if !body.ends_with("`;") {
        return Err("extracted script does not end with backtick-semicolon".into());
    }
    body.truncate(body.len() - 2);
    let sha = hex_sha256(body.as_bytes());
    Ok(SnapshotSource {
        script: body,
        sha256: sha,
        source_file: "desktop/main.cjs".to_string(),
        source_lines: (start_idx + 1, end_idx + 1),
    })
}

pub fn hex_sha256(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_stable() {
        assert_eq!(
            hex_sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn extracts_from_repo() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap()
            .to_path_buf();
        let src = extract_snapshot_script(&root).expect("extract");
        assert!(src.script.contains("data-hana-ref"));
        assert!(src.script.contains("MAX_TREE"));
        assert_eq!(src.sha256.len(), 64);
    }
}
