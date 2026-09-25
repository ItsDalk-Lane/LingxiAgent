//! R01-T03 schema-validation prototype driver (taskbook step 2, W10/W11):
//! validates T02's generated JSON Schemas against their golden instances using
//! the `jsonschema` crate (the validation-side candidate; generation stays
//! with schemars). Runs one positive and one negative case. Exit 0 only when
//! the positive validates and the negative is rejected.

use lingxi_spike::schema::{contracts_generated_dir, validate_file};

fn main() {
    let gen = contracts_generated_dir();
    let schema = gen.join("jsonschema/ClientHello.schema.json");

    let golden_path = gen.join("golden/client-hello.json");
    let golden_text = std::fs::read_to_string(&golden_path).unwrap_or_else(|e| {
        eprintln!("SPIKE_SCHEMA_FAIL read {}: {e}", golden_path.display());
        std::process::exit(1);
    });
    let golden: serde_json::Value = serde_json::from_str(&golden_text).unwrap_or_else(|e| {
        eprintln!("SPIKE_SCHEMA_FAIL parse golden: {e}");
        std::process::exit(1);
    });

    // Positive: golden must validate.
    let errors = match validate_file(&schema, &golden) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("SPIKE_SCHEMA_FAIL schema compile: {e}");
            std::process::exit(1);
        }
    };
    if !errors.is_empty() {
        eprintln!("SPIKE_SCHEMA_FAIL golden rejected: {errors:?}");
        std::process::exit(1);
    }

    // Negative: tampered instance must be rejected.
    let mut tampered = golden.clone();
    tampered["protocolMin"] = serde_json::Value::String("one".into());
    match validate_file(&schema, &tampered) {
        Ok(errors) if !errors.is_empty() => {
            println!(
                "SPIKE_SCHEMA_OK schema=ClientHello.schema.json positive=valid negative=rejected({})",
                errors[0]
            );
        }
        Ok(_) => {
            eprintln!("SPIKE_SCHEMA_FAIL tampered instance was NOT rejected");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("SPIKE_SCHEMA_FAIL schema compile (negative): {e}");
            std::process::exit(1);
        }
    }
}
