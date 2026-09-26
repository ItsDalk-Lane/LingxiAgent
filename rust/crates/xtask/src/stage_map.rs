//! Stage-map parsing and validation for `verify-stage`.
//!
//! The map is the stage's implementation-acceptance contract: scenario ids
//! (R0x-Ayy) bound to REAL registered commands. Every structural gap is a
//! hard parse error so that "no tests registered" can never become a green
//! result downstream.

use serde_json::Value;

/// One registered command: a real argv executed at the repo root, a
/// timeout, and the evidence files that must exist afterwards.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandSpec {
    pub key: String,
    pub argv: Vec<String>,
    pub timeout_secs: u64,
    /// Files (or dirs) relative to the evidence root — `{EVIDENCE}`-form —
    /// that must exist after the command succeeds.
    pub evidence_paths: Vec<String>,
}

/// One acceptance scenario bound to one or more registered commands.
#[derive(Debug, Clone, PartialEq)]
pub struct Scenario {
    pub id: String,
    pub requirement: String,
    pub command_refs: Vec<String>,
}

/// A parsed stage map.
#[derive(Debug, Clone, PartialEq)]
pub struct StageMap {
    pub stage: String,
    pub result_version: String,
    pub default_timeout_secs: u64,
    pub commands: Vec<CommandSpec>,
    pub scenarios: Vec<Scenario>,
}

const DEFAULT_TIMEOUT_SECS: u64 = 1200;

fn err(detail: &str) -> String {
    format!("stage map invalid: {detail}")
}

fn non_empty_string(value: &Value, what: &str) -> Result<String, String> {
    value
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| err(&format!("{what} must be a non-empty string, got {value}")))
}

fn positive_u64(value: &Value, what: &str) -> Result<u64, String> {
    value
        .as_u64()
        .filter(|v| *v > 0)
        .ok_or_else(|| err(&format!("{what} must be a positive integer, got {value}")))
}

/// Parses and validates a stage map from JSON text.
pub fn parse_stage_map(text: &str) -> Result<StageMap, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| err(&format!("not valid JSON: {e}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| err("top level must be a JSON object"))?;

    let schema_version = object
        .get("schemaVersion")
        .ok_or_else(|| err("missing schemaVersion"))?
        .as_u64()
        .ok_or_else(|| err("schemaVersion must be an integer"))?;
    if schema_version != 1 {
        return Err(err(&format!(
            "unsupported schemaVersion {schema_version} (expected 1)"
        )));
    }

    let result_version = non_empty_string(
        object
            .get("resultVersion")
            .ok_or_else(|| err("missing resultVersion"))?,
        "resultVersion",
    )?;
    let stage = non_empty_string(
        object.get("stage").ok_or_else(|| err("missing stage"))?,
        "stage",
    )?;

    let default_timeout_secs = match object.get("defaultTimeoutSecs") {
        Some(v) => positive_u64(v, "defaultTimeoutSecs")?,
        None => DEFAULT_TIMEOUT_SECS,
    };

    let commands_value = object
        .get("commands")
        .ok_or_else(|| err("missing commands"))?
        .as_object()
        .ok_or_else(|| err("commands must be an object keyed by command key"))?;
    if commands_value.is_empty() {
        return Err(err("commands must not be empty"));
    }
    let mut commands = Vec::with_capacity(commands_value.len());
    for (key, spec) in commands_value {
        let spec = spec
            .as_object()
            .ok_or_else(|| err(&format!("command {key:?} must be an object")))?;
        let argv_value = spec
            .get("argv")
            .ok_or_else(|| err(&format!("command {key:?} missing argv")))?;
        let argv_list = argv_value
            .as_array()
            .ok_or_else(|| err(&format!("command {key:?} argv must be an array")))?;
        if argv_list.is_empty() {
            return Err(err(&format!("command {key:?} argv must not be empty")));
        }
        let mut argv = Vec::with_capacity(argv_list.len());
        for part in argv_list {
            argv.push(non_empty_string(
                part,
                &format!("command {key:?} argv element"),
            )?);
        }
        let timeout_secs = match spec.get("timeoutSecs") {
            Some(v) => positive_u64(v, &format!("command {key:?} timeoutSecs"))?,
            None => default_timeout_secs,
        };
        let evidence_paths = match spec.get("evidencePaths") {
            Some(v) => {
                let list = v.as_array().ok_or_else(|| {
                    err(&format!("command {key:?} evidencePaths must be an array"))
                })?;
                if list.is_empty() {
                    return Err(err(&format!(
                        "command {key:?} evidencePaths must not be empty (missing \
                         evidence must be a hard failure, never a pass)"
                    )));
                }
                let mut paths = Vec::with_capacity(list.len());
                for path in list {
                    paths.push(non_empty_string(
                        path,
                        &format!("command {key:?} evidencePaths element"),
                    )?);
                }
                paths
            }
            None => {
                return Err(err(&format!(
                    "command {key:?} missing evidencePaths (missing evidence must \
                     be a hard failure, never a pass)"
                )))
            }
        };
        commands.push(CommandSpec {
            key: key.clone(),
            argv,
            timeout_secs,
            evidence_paths,
        });
    }

    let scenarios_value = object
        .get("scenarios")
        .ok_or_else(|| err("missing scenarios"))?
        .as_array()
        .ok_or_else(|| err("scenarios must be an array"))?;
    if scenarios_value.is_empty() {
        return Err(err(
            "scenarios must not be EMPTY (an empty test collection must exit \
             non-zero, never pass)",
        ));
    }
    let mut scenarios = Vec::with_capacity(scenarios_value.len());
    for (index, value) in scenarios_value.iter().enumerate() {
        let object = value
            .as_object()
            .ok_or_else(|| err(&format!("scenario #{index} must be an object")))?;
        let id = non_empty_string(
            object
                .get("id")
                .ok_or_else(|| err(&format!("scenario #{index} missing id")))?,
            &format!("scenario #{index} id"),
        )?;
        if scenarios.iter().any(|s: &Scenario| s.id == id) {
            return Err(err(&format!("duplicate scenario id {id:?}")));
        }
        let requirement = non_empty_string(
            object
                .get("requirement")
                .ok_or_else(|| err(&format!("scenario {id:?} missing requirement")))?,
            &format!("scenario {id:?} requirement"),
        )?;
        let refs_value = object
            .get("commandRefs")
            .ok_or_else(|| err(&format!("scenario {id:?} missing commandRefs")))?;
        let refs_list = refs_value
            .as_array()
            .ok_or_else(|| err(&format!("scenario {id:?} commandRefs must be an array")))?;
        if refs_list.is_empty() {
            return Err(err(&format!(
                "scenario {id:?} commandRefs must not be empty (a scenario bound \
                 to no test is a hole in the map, not a pass)"
            )));
        }
        let mut command_refs = Vec::with_capacity(refs_list.len());
        for reference in refs_list {
            let key = non_empty_string(reference, &format!("scenario {id:?} commandRefs element"))?;
            if !commands_value.contains_key(&key) {
                return Err(err(&format!(
                    "scenario {id:?} references unknown command {key:?}"
                )));
            }
            command_refs.push(key);
        }
        scenarios.push(Scenario {
            id,
            requirement,
            command_refs,
        });
    }

    Ok(StageMap {
        stage,
        result_version,
        default_timeout_secs,
        commands,
        scenarios,
    })
}

#[cfg(test)]
#[cfg(test)]
mod map_tests {
    use super::*;

    const VALID: &str = r#"{
        "schemaVersion": 1,
        "resultVersion": "lingxi.xtask.verify-stage.v1",
        "stage": "RX",
        "defaultTimeoutSecs": 60,
        "commands": {
            "a_ok": {
                "argv": ["bash", "gate.sh", "{EVIDENCE}/RX"],
                "timeoutSecs": 30,
                "evidencePaths": ["{EVIDENCE}/RX/out.txt"]
            }
        },
        "scenarios": [
            {"id": "RX-A01", "requirement": "REQUIRED", "commandRefs": ["a_ok"]}
        ]
    }"#;

    #[test]
    fn parses_a_valid_map() {
        let map = parse_stage_map(VALID).expect("valid map");
        assert_eq!(map.stage, "RX");
        assert_eq!(map.default_timeout_secs, 60);
        assert_eq!(map.commands.len(), 1);
        assert_eq!(map.commands[0].timeout_secs, 30);
        assert_eq!(map.scenarios.len(), 1);
        assert_eq!(map.scenarios[0].command_refs, vec!["a_ok"]);
    }

    #[test]
    fn empty_scenario_set_is_a_hard_error() {
        let text = VALID.replace(
            r#""scenarios": [
            {"id": "RX-A01", "requirement": "REQUIRED", "commandRefs": ["a_ok"]}
        ]"#,
            r#""scenarios": []"#,
        );
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("must not be EMPTY"), "{err}");
    }

    #[test]
    fn unknown_command_ref_is_a_hard_error() {
        let text = VALID.replace(r#""commandRefs": ["a_ok"]"#, r#""commandRefs": ["nope"]"#);
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("unknown command"), "{err}");
    }

    #[test]
    fn scenario_without_command_refs_is_a_hard_error() {
        let text = VALID.replace(r#""commandRefs": ["a_ok"]"#, r#""commandRefs": []"#);
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("must not be empty"), "{err}");
    }

    #[test]
    fn command_without_evidence_paths_is_a_hard_error() {
        let text = VALID.replace(
            r#""evidencePaths": ["{EVIDENCE}/RX/out.txt"]"#,
            r#""evidencePaths": []"#,
        );
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("evidencePaths"), "{err}");
    }

    #[test]
    fn duplicate_scenario_ids_are_a_hard_error() {
        let text = VALID.replace(
            r#"{"id": "RX-A01", "requirement": "REQUIRED", "commandRefs": ["a_ok"]}
        ]"#,
            r#"{"id": "RX-A01", "requirement": "REQUIRED", "commandRefs": ["a_ok"]},
            {"id": "RX-A01", "requirement": "REQUIRED", "commandRefs": ["a_ok"]}
        ]"#,
        );
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("duplicate scenario id"), "{err}");
    }

    #[test]
    fn wrong_schema_version_is_a_hard_error() {
        let text = VALID.replace(r#""schemaVersion": 1"#, r#""schemaVersion": 99"#);
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("unsupported schemaVersion"), "{err}");
    }

    #[test]
    fn placeholder_values_are_rejected() {
        let text = VALID.replace(r#""stage": "RX""#, r#""stage": "  ""#);
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("non-empty string"), "{err}");
    }

    #[test]
    fn invalid_json_is_a_hard_error() {
        assert!(parse_stage_map("{not json").is_err());
    }
}
