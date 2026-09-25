//! JSON Schema validation-side proof (W11): the `jsonschema` crate validates
//! instances against the schemas that T02 generates from the Rust protocol
//! types with schemars (W10). Generation stays with schemars; validation of
//! untrusted third-party tool schemas is the job `jsonschema` is evaluated for.

use std::path::{Path, PathBuf};

/// Root of the T02 generated contract tree, resolved relative to this crate's
/// manifest (rust/crates/lingxi-spike -> repo root).
pub fn contracts_generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../contracts/generated")
}

/// Validate `instance` against the schema document at `schema_path`.
/// Returns the list of validation error messages (empty = valid). Schema
/// compilation errors are fatal and returned as Err (no silent fallback to
/// "accept everything").
pub fn validate_file(
    schema_path: &Path,
    instance: &serde_json::Value,
) -> Result<Vec<String>, String> {
    let schema_text = std::fs::read_to_string(schema_path)
        .map_err(|e| format!("read schema {}: {e}", schema_path.display()))?;
    let schema: serde_json::Value = serde_json::from_str(&schema_text)
        .map_err(|e| format!("parse schema {}: {e}", schema_path.display()))?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|e| format!("compile schema {}: {e}", schema_path.display()))?;
    Ok(validator
        .iter_errors(instance)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_golden(name: &str) -> serde_json::Value {
        let path = contracts_generated_dir().join("golden").join(name);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read golden {}: {e}", path.display()));
        serde_json::from_str(&text).expect("golden parses as JSON")
    }

    #[test]
    fn golden_client_hello_validates_against_generated_schema() {
        let instance = load_golden("client-hello.json");
        let errors = validate_file(
            &contracts_generated_dir().join("jsonschema/ClientHello.schema.json"),
            &instance,
        )
        .expect("schema compiles");
        assert!(errors.is_empty(), "golden must validate, got: {errors:?}");
    }

    #[test]
    fn tampered_client_hello_is_rejected() {
        let mut instance = load_golden("client-hello.json");
        // protocolMin is an integer in the schema; a string must be rejected.
        instance["protocolMin"] = serde_json::Value::String("one".into());
        let errors = validate_file(
            &contracts_generated_dir().join("jsonschema/ClientHello.schema.json"),
            &instance,
        )
        .expect("schema compiles");
        assert!(!errors.is_empty(), "tampered instance must be rejected");
    }

    #[test]
    fn golden_server_hello_validates() {
        let instance = load_golden("server-hello.json");
        let errors = validate_file(
            &contracts_generated_dir().join("jsonschema/ServerHello.schema.json"),
            &instance,
        )
        .expect("schema compiles");
        assert!(errors.is_empty(), "golden must validate, got: {errors:?}");
    }
}
