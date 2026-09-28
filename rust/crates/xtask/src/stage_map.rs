//! Stage-map parsing and validation for `verify-stage`.
//!
//! The map is the stage's implementation-acceptance contract: scenario ids
//! (R0x-Ayy) bound to REAL registered commands. Every structural gap is a
//! hard parse error so that "no tests registered" can never become a green
//! result downstream.

use serde_json::Value;

/// The result-document version this runner emits — and the ONLY version a
/// stage map may declare (R02 stage-repair R1 / F05). The parse below
/// rejects any other value loudly: a map written for a result format the
/// runner does not understand must never sail through and get stamped with
/// the runner's own version anyway.
pub const RESULT_VERSION: &str = "lingxi.xtask.verify-stage.v1";

/// Every resultVersion value this runner accepts. Today exactly one; the
/// list exists so a future v2 is an EXPLICIT additive decision here, never
/// an unnoticed pass-through.
pub const SUPPORTED_RESULT_VERSIONS: &[&str] = &[RESULT_VERSION];

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

/// Basis kinds for `supplementalLeafScenarios` entries (R13-F01: the R00
/// REQUIRED_SUPPLEMENTAL leaf scenarios co-marked R02/R07 must be CONSUMED
/// by this stage gate, not merely listed). The first three kinds describe
/// HOW the R02 server-side share of the leaf is evidenced by this stage's
/// registered commands; the fourth marks the leaves whose ORIGINAL R00
/// marking (R02-T03 + stage-R02 acceptance due) conflicts with the stage
/// boundary (pure client behavior, no identified server share) — the gate
/// holds those BLOCKED pending a root disposition; they can never be PASS.
/// The fifth is reserved for the complete ORIGINAL behavior, including
/// the formerly R07-owned client branches, genuinely exercised now.
pub const BASIS_ROUTE_PRESENT_STATIC: &str = "route_basis_present_static";
pub const BASIS_PROTOCOL: &str = "protocol_basis";
pub const BASIS_AUTH_PRIMITIVE_ONLY: &str = "auth_primitive_only";
pub const BASIS_CLIENT_ONLY_STAGE_CONFLICT: &str = "client_only_stage_boundary_conflict";
/// 原行为（含前端正反分支）已由本阶段独立生产者完整实测；必须逐条列出
/// R00 原断言与专属案例的对应关系，不能只把原 R07 份额写成字符串后放行。
pub const BASIS_FULL_ORIGINAL_BEHAVIOR: &str = "full_original_behavior";
pub const SUPPLEMENTAL_BASIS_KINDS: &[&str] = &[
    BASIS_ROUTE_PRESENT_STATIC,
    BASIS_PROTOCOL,
    BASIS_AUTH_PRIMITIVE_ONLY,
    BASIS_CLIENT_ONLY_STAGE_CONFLICT,
    BASIS_FULL_ORIGINAL_BEHAVIOR,
];

/// One machine-checked case expectation of a leaf's `assertionContract`
/// (R14-F01). `expect` is the ORIGINAL R00 assertion value the gate must
/// observe in the producer's structured evidence file (an HTTP status, a
/// normalized boolean, an exit code) — the gate compares the ACTUAL value
/// recorded by the producer, so a green command exit alone can never
/// satisfy a leaf whose original assertion did not hold.
#[derive(Debug, Clone, PartialEq)]
pub struct LeafCaseExpect {
    pub case: String,
    pub expect: i64,
}

/// The per-leaf assertion contract (R14-F01): WHICH registered command
/// produces the leaf's structured case evidence, WHERE that file lands
/// (`{EVIDENCE}` form), and WHICH case/expect pairs the gate must verify
/// against the leaf's original R00 assertions.
#[derive(Debug, Clone, PartialEq)]
pub struct LeafAssertionContract {
    pub producer_command: String,
    pub evidence_path: String,
    pub cases: Vec<LeafCaseExpect>,
}

/// One R00 REQUIRED_SUPPLEMENTAL leaf scenario whose R02 share this stage
/// gate consumes (R13-F01). `evidence_command_refs`/`evidence_paths` are
/// the machine check for that R02 share: the leaf passes only when every
/// referenced command passed AND every declared evidence artifact exists.
/// A `client_only_stage_boundary_conflict` leaf declares no evidence
/// bindings on purpose — it is BLOCKED by the gate until a root
/// disposition, which is the auditable alternative to silently waiving an
/// R00 REQUIRED obligation.
///
/// R14-F01: the `r00_*` mirror fields copy the leaf's ORIGINAL R00 ledger
/// record (both R00 ledgers) into the map so the runner can compare them
/// verbatim — feature, kind, dual-stage task/stage responsibility,
/// ledger status, original `then` text, the full original assertion list,
/// the due line, and the still-empty execution-result/test registries.
/// A map that re-binds any of those to a different R00 object is a hard
/// parse/verify error, never a re-grade. `assertion_contract` is the
/// 各非冲突叶项必须登记 `assertion_contract`，机器据此核对案例图钉与
/// 实际记录值。但通用案例的数值一致不等于观察到原叶行为；
/// `auth_primitive_only` 在汇总时保持 BLOCKED。对仍归属后续阶段的原叶，
/// 当前阶段的案例也不能单独证明完整原叶结果。
#[derive(Debug, Clone, PartialEq)]
pub struct SupplementalLeaf {
    pub id: String,
    pub feature_id: String,
    /// The requirement R00 recorded for this leaf; the runner cross-checks
    /// it against the R00 acceptance ledger so the map can never re-grade
    /// an R00 obligation.
    pub requirement: String,
    pub basis_kind: String,
    pub r02_share: String,
    pub r07_share: String,
    pub evidence_required: String,
    pub evidence_command_refs: Vec<String>,
    pub evidence_paths: Vec<String>,
    pub r00_kind: String,
    pub r00_task_ids: Vec<String>,
    pub r00_execution_stage_ids: Vec<String>,
    pub r00_ledger_status: String,
    pub r00_result_ids: Vec<String>,
    pub r00_test_ids: Vec<String>,
    pub r00_then: String,
    pub r00_assertions: Vec<String>,
    pub r00_due: String,
    pub assertion_contract: Option<LeafAssertionContract>,
    /// 与 r00_assertions 同序。仅 full_original_behavior 可声明；每条原
    /// 断言必须由不同的专属案例覆盖，且所有钉住的案例都须被归属。
    pub original_assertion_cases: Vec<Vec<String>>,
}

/// A parsed stage map.
#[derive(Debug, Clone, PartialEq)]
pub struct StageMap {
    pub stage: String,
    pub result_version: String,
    pub default_timeout_secs: u64,
    pub commands: Vec<CommandSpec>,
    pub scenarios: Vec<Scenario>,
    /// R00 REQUIRED_SUPPLEMENTAL leaves bound to this stage (R13-F01).
    /// Optional at parse level: a stage with no such leaves omits the key.
    /// The runner cross-checks the declared set for EQUALITY against the
    /// R00 acceptance ledger at verify time, so dropping required leaves
    /// (or inventing unknown ones) is a hard failure, never a silent pass.
    pub supplemental_leaves: Vec<SupplementalLeaf>,
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

/// R14-F01: a string-array field of an R00 mirror. Elements must be
/// non-empty strings; the array itself may only be empty when
/// `allow_empty` (the still-unexecuted result/test registries are empty
/// arrays in R00, the responsibility/asset fields never are).
fn string_array_field(
    object: &serde_json::Map<String, Value>,
    key: &str,
    what: &str,
    allow_empty: bool,
) -> Result<Vec<String>, String> {
    let list = object
        .get(key)
        .ok_or_else(|| err(&format!("{what} missing (R00 mirror)")))?
        .as_array()
        .ok_or_else(|| err(&format!("{what} must be an array of strings")))?;
    if list.is_empty() && !allow_empty {
        return Err(err(&format!(
            "{what} must not be empty (an R00 mirror dropping the original entries \
             would unbind the leaf's recorded responsibility — R14-F01)"
        )));
    }
    let mut values = Vec::with_capacity(list.len());
    for item in list {
        values.push(non_empty_string(item, &format!("{what} element"))?);
    }
    Ok(values)
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
    // R02 stage-repair R1 / F05: the declared result version must be one
    // this runner actually implements — missing/empty/unknown are all hard
    // errors (missing/empty already fail in non_empty_string above).
    if !SUPPORTED_RESULT_VERSIONS.contains(&result_version.as_str()) {
        return Err(err(&format!(
            "unsupported resultVersion {result_version:?} (supported: {SUPPORTED_RESULT_VERSIONS:?})"
        )));
    }
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

    // R13-F01: the stage map may declare the R00 REQUIRED_SUPPLEMENTAL
    // leaves whose R02 share this gate consumes. Structural violations are
    // hard parse errors, exactly like the sections above — the consumer
    // (verify.rs) additionally cross-checks the declared id set for
    // EQUALITY against the R00 acceptance ledger, so this parser only
    // guards shape, never coverage.
    let supplemental_leaves = match object.get("supplementalLeafScenarios") {
        None => Vec::new(),
        Some(value) => {
            let list = value
                .as_array()
                .ok_or_else(|| err("supplementalLeafScenarios must be an array of leaf objects"))?;
            if list.is_empty() {
                return Err(err(
                    "supplementalLeafScenarios must not be EMPTY (declaring the \
                     section with zero entries would drop every R00 \
                     REQUIRED_SUPPLEMENTAL obligation from the gate; a stage with \
                     no such leaves omits the key — the runner cross-checks the \
                     declared set against the R00 acceptance ledger at verify time)",
                ));
            }
            let mut leaves = Vec::with_capacity(list.len());
            for (index, value) in list.iter().enumerate() {
                let entry = value.as_object().ok_or_else(|| {
                    err(&format!(
                        "supplementalLeafScenarios #{index} must be an object"
                    ))
                })?;
                let id = non_empty_string(
                    entry.get("id").ok_or_else(|| {
                        err(&format!("supplementalLeafScenarios #{index} missing id"))
                    })?,
                    &format!("supplementalLeafScenarios #{index} id"),
                )?;
                if leaves.iter().any(|leaf: &SupplementalLeaf| leaf.id == id) {
                    return Err(err(&format!("duplicate supplemental leaf id {id:?}")));
                }
                let feature_id = non_empty_string(
                    entry.get("featureId").ok_or_else(|| {
                        err(&format!("supplemental leaf {id:?} missing featureId"))
                    })?,
                    &format!("supplemental leaf {id:?} featureId"),
                )?;
                let requirement = non_empty_string(
                    entry.get("requirement").ok_or_else(|| {
                        err(&format!("supplemental leaf {id:?} missing requirement"))
                    })?,
                    &format!("supplemental leaf {id:?} requirement"),
                )?;
                let basis_kind = non_empty_string(
                    entry.get("basisKind").ok_or_else(|| {
                        err(&format!("supplemental leaf {id:?} missing basisKind"))
                    })?,
                    &format!("supplemental leaf {id:?} basisKind"),
                )?;
                if !SUPPLEMENTAL_BASIS_KINDS.contains(&basis_kind.as_str()) {
                    return Err(err(&format!(
                        "supplemental leaf {id:?} has unknown basisKind {basis_kind:?} \
                         (known kinds: {SUPPLEMENTAL_BASIS_KINDS:?})"
                    )));
                }
                let r02_share = non_empty_string(
                    entry.get("r02Share").ok_or_else(|| {
                        err(&format!("supplemental leaf {id:?} missing r02Share"))
                    })?,
                    &format!("supplemental leaf {id:?} r02Share"),
                )?;
                let r07_share = non_empty_string(
                    entry.get("r07Share").ok_or_else(|| {
                        err(&format!("supplemental leaf {id:?} missing r07Share"))
                    })?,
                    &format!("supplemental leaf {id:?} r07Share"),
                )?;
                let evidence_required = non_empty_string(
                    entry.get("evidenceRequired").ok_or_else(|| {
                        err(&format!(
                            "supplemental leaf {id:?} missing evidenceRequired"
                        ))
                    })?,
                    &format!("supplemental leaf {id:?} evidenceRequired"),
                )?;
                let refs_value = entry.get("evidenceCommandRefs").ok_or_else(|| {
                    err(&format!(
                        "supplemental leaf {id:?} missing evidenceCommandRefs"
                    ))
                })?;
                let refs_list = refs_value.as_array().ok_or_else(|| {
                    err(&format!(
                        "supplemental leaf {id:?} evidenceCommandRefs must be an array"
                    ))
                })?;
                let mut evidence_command_refs = Vec::with_capacity(refs_list.len());
                for reference in refs_list {
                    let key = non_empty_string(
                        reference,
                        &format!("supplemental leaf {id:?} evidenceCommandRefs element"),
                    )?;
                    if !commands_value.contains_key(&key) {
                        return Err(err(&format!(
                            "supplemental leaf {id:?} references unknown command {key:?}"
                        )));
                    }
                    evidence_command_refs.push(key);
                }
                if basis_kind == BASIS_CLIENT_ONLY_STAGE_CONFLICT {
                    if !evidence_command_refs.is_empty() {
                        return Err(err(&format!(
                            "supplemental leaf {id:?} is a stage-boundary conflict and must \
                             not bind evidence commands (binding would silently resolve the \
                             conflict the gate is required to hold BLOCKED)"
                        )));
                    }
                } else if evidence_command_refs.is_empty() {
                    return Err(err(&format!(
                        "supplemental leaf {id:?} evidenceCommandRefs must not be empty (a \
                         leaf with an R02 share bound to no evidence command is a hole in \
                         the map, not a pass)"
                    )));
                }
                let evidence_paths = match entry.get("evidencePaths") {
                    Some(v) => {
                        let list = v.as_array().ok_or_else(|| {
                            err(&format!(
                                "supplemental leaf {id:?} evidencePaths must be an array"
                            ))
                        })?;
                        let mut paths = Vec::with_capacity(list.len());
                        for path in list {
                            paths.push(non_empty_string(
                                path,
                                &format!("supplemental leaf {id:?} evidencePaths element"),
                            )?);
                        }
                        if basis_kind == BASIS_CLIENT_ONLY_STAGE_CONFLICT && !paths.is_empty() {
                            return Err(err(&format!(
                                "supplemental leaf {id:?} is BLOCKED by stage-boundary \
                                 conflict and must not declare pass-able evidencePaths"
                            )));
                        }
                        paths
                    }
                    None => Vec::new(),
                };

                // R14-F01: the r00_* mirror fields — verbatim copies of the
                // leaf's ORIGINAL record in the R00 ledgers. The runner
                // compares each of these against the ledgers before any
                // command runs, so the map can never silently re-bind a
                // leaf's feature, kind, dual-stage responsibility, status,
                // original assertions, or due line to a different object.
                let r00_kind = non_empty_string(
                    entry.get("r00Kind").ok_or_else(|| {
                        err(&format!(
                            "supplemental leaf {id:?} missing r00Kind (R00 mirror)"
                        ))
                    })?,
                    &format!("supplemental leaf {id:?} r00Kind"),
                )?;
                let r00_task_ids = string_array_field(
                    entry,
                    "r00TaskIds",
                    &format!("supplemental leaf {id:?} r00TaskIds"),
                    false,
                )?;
                let r00_execution_stage_ids = string_array_field(
                    entry,
                    "r00ExecutionStageIds",
                    &format!("supplemental leaf {id:?} r00ExecutionStageIds"),
                    false,
                )?;
                let r00_ledger_status = non_empty_string(
                    entry.get("r00LedgerStatus").ok_or_else(|| {
                        err(&format!(
                            "supplemental leaf {id:?} missing r00LedgerStatus (R00 mirror)"
                        ))
                    })?,
                    &format!("supplemental leaf {id:?} r00LedgerStatus"),
                )?;
                let r00_result_ids = string_array_field(
                    entry,
                    "r00ResultIds",
                    &format!("supplemental leaf {id:?} r00ResultIds"),
                    true,
                )?;
                let r00_test_ids = string_array_field(
                    entry,
                    "r00TestIds",
                    &format!("supplemental leaf {id:?} r00TestIds"),
                    true,
                )?;
                let r00_then = non_empty_string(
                    entry.get("r00Then").ok_or_else(|| {
                        err(&format!(
                            "supplemental leaf {id:?} missing r00Then (R00 mirror)"
                        ))
                    })?,
                    &format!("supplemental leaf {id:?} r00Then"),
                )?;
                let r00_assertions = string_array_field(
                    entry,
                    "r00Assertions",
                    &format!("supplemental leaf {id:?} r00Assertions"),
                    false,
                )?;
                let r00_due = non_empty_string(
                    entry.get("r00Due").ok_or_else(|| {
                        err(&format!(
                            "supplemental leaf {id:?} missing r00Due (R00 mirror)"
                        ))
                    })?,
                    &format!("supplemental leaf {id:?} r00Due"),
                )?;

                // R14-F01: the per-leaf assertion contract. Every
                // NON-conflict leaf MUST declare one — a leaf whose PASS is
                // justified only by a green command exit (with no case the
                // gate can re-verify against the original assertion) is
                // exactly the fake-green path R14-F01 closes. A conflict
                // leaf must NOT declare one (it can never be PASS).
                let assertion_contract = match entry.get("assertionContract") {
                    Some(v) => {
                        if basis_kind == BASIS_CLIENT_ONLY_STAGE_CONFLICT {
                            return Err(err(&format!(
                                "supplemental leaf {id:?} is a stage-boundary conflict and \
                                 must not declare an assertionContract (it can never be PASS)"
                            )));
                        }
                        let contract = v.as_object().ok_or_else(|| {
                            err(&format!(
                                "supplemental leaf {id:?} assertionContract must be an object"
                            ))
                        })?;
                        let producer_command = non_empty_string(
                            contract.get("producerCommand").ok_or_else(|| {
                                err(&format!(
                                    "supplemental leaf {id:?} assertionContract missing \
                                     producerCommand"
                                ))
                            })?,
                            &format!("supplemental leaf {id:?} assertionContract producerCommand"),
                        )?;
                        if !commands_value.contains_key(&producer_command) {
                            return Err(err(&format!(
                                "supplemental leaf {id:?} assertionContract references \
                                 unknown producer command {producer_command:?}"
                            )));
                        }
                        if !evidence_command_refs.contains(&producer_command) {
                            return Err(err(&format!(
                                "supplemental leaf {id:?} assertionContract producer \
                                 {producer_command:?} is not in the leaf's \
                                 evidenceCommandRefs — a producer that never runs can never \
                                 write the leaf's evidence"
                            )));
                        }
                        let evidence_path = non_empty_string(
                            contract.get("evidencePath").ok_or_else(|| {
                                err(&format!(
                                    "supplemental leaf {id:?} assertionContract missing \
                                     evidencePath"
                                ))
                            })?,
                            &format!("supplemental leaf {id:?} assertionContract evidencePath"),
                        )?;
                        let cases_value = contract
                            .get("cases")
                            .ok_or_else(|| {
                                err(&format!(
                                    "supplemental leaf {id:?} assertionContract missing cases"
                                ))
                            })?
                            .as_array()
                            .ok_or_else(|| {
                                err(&format!(
                                    "supplemental leaf {id:?} assertionContract cases must \
                                     be an array"
                                ))
                            })?;
                        if cases_value.is_empty() {
                            return Err(err(&format!(
                                "supplemental leaf {id:?} assertionContract cases must not \
                                 be empty (a leaf with no machine-checkable case is exactly \
                                 the exit-0-only fake-green path R14-F01 closes)"
                            )));
                        }
                        let mut cases = Vec::with_capacity(cases_value.len());
                        for case_value in cases_value {
                            let case_object = case_value.as_object().ok_or_else(|| {
                                err(&format!(
                                    "supplemental leaf {id:?} assertionContract case must \
                                     be an object"
                                ))
                            })?;
                            let case = non_empty_string(
                                case_object.get("case").ok_or_else(|| {
                                    err(&format!(
                                        "supplemental leaf {id:?} assertionContract case \
                                         missing case name"
                                    ))
                                })?,
                                &format!("supplemental leaf {id:?} assertionContract case name"),
                            )?;
                            if cases
                                .iter()
                                .any(|pinned: &LeafCaseExpect| pinned.case == case)
                            {
                                return Err(err(&format!(
                                    "supplemental leaf {id:?} assertionContract repeats \
                                     case name {case:?} — each pinned observation must \
                                     have its own identity"
                                )));
                            }
                            let expect = case_object
                                .get("expect")
                                .and_then(Value::as_i64)
                                .ok_or_else(|| {
                                    err(&format!(
                                        "supplemental leaf {id:?} assertionContract case \
                                         {case:?} must have an integer expect value"
                                    ))
                                })?;
                            cases.push(LeafCaseExpect { case, expect });
                        }
                        Some(LeafAssertionContract {
                            producer_command,
                            evidence_path,
                            cases,
                        })
                    }
                    None => {
                        if basis_kind != BASIS_CLIENT_ONLY_STAGE_CONFLICT {
                            return Err(err(&format!(
                                "supplemental leaf {id:?} missing assertionContract (every \
                                 non-conflict leaf must declare the machine-checked cases \
                                 that consume its original R00 assertions — R14-F01)"
                            )));
                        }
                        None
                    }
                };
                let original_assertion_cases = match entry.get("originalAssertionCases") {
                    Some(value) if basis_kind == BASIS_FULL_ORIGINAL_BEHAVIOR => {
                        let groups = value.as_array().ok_or_else(|| {
                            err(&format!(
                                "supplemental leaf {id:?} originalAssertionCases must be an array"
                            ))
                        })?;
                        if groups.len() != r00_assertions.len() {
                            return Err(err(&format!(
                                "supplemental leaf {id:?} originalAssertionCases must cover \
                                 all {} original R00 assertions in order (got {})",
                                r00_assertions.len(),
                                groups.len()
                            )));
                        }
                        let contract = assertion_contract.as_ref().ok_or_else(|| {
                            err(&format!(
                                "supplemental leaf {id:?} complete behavior has no assertionContract"
                            ))
                        })?;
                        let mut seen = Vec::new();
                        let mut coverage = Vec::with_capacity(groups.len());
                        for (assertion_index, group) in groups.iter().enumerate() {
                            let refs = group.as_array().ok_or_else(|| {
                                err(&format!(
                                    "supplemental leaf {id:?} original assertion \
                                     #{assertion_index} cases must be an array"
                                ))
                            })?;
                            if refs.is_empty() {
                                return Err(err(&format!(
                                    "supplemental leaf {id:?} original assertion \
                                     #{assertion_index} has no case evidence"
                                )));
                            }
                            let mut names = Vec::with_capacity(refs.len());
                            for reference in refs {
                                let name = non_empty_string(
                                    reference,
                                    &format!(
                                        "supplemental leaf {id:?} original assertion \
                                         #{assertion_index} case"
                                    ),
                                )?;
                                if !contract.cases.iter().any(|pinned| pinned.case == name) {
                                    return Err(err(&format!(
                                        "supplemental leaf {id:?} original assertion \
                                         #{assertion_index} references unpinned case {name:?}"
                                    )));
                                }
                                if seen.contains(&name) {
                                    return Err(err(&format!(
                                        "supplemental leaf {id:?} reuses original-behavior \
                                         case {name:?} across assertions"
                                    )));
                                }
                                seen.push(name.clone());
                                names.push(name);
                            }
                            coverage.push(names);
                        }
                        if seen.len() != contract.cases.len() {
                            return Err(err(&format!(
                                "supplemental leaf {id:?} has pinned cases not assigned \
                                 to an original R00 assertion"
                            )));
                        }
                        coverage
                    }
                    Some(_) => {
                        return Err(err(&format!(
                            "supplemental leaf {id:?} declares originalAssertionCases \
                             without full_original_behavior evidence"
                        )));
                    }
                    None if basis_kind == BASIS_FULL_ORIGINAL_BEHAVIOR => {
                        return Err(err(&format!(
                            "supplemental leaf {id:?} full_original_behavior requires \
                             originalAssertionCases for every R00 assertion"
                        )));
                    }
                    None => Vec::new(),
                };
                leaves.push(SupplementalLeaf {
                    id,
                    feature_id,
                    requirement,
                    basis_kind,
                    r02_share,
                    r07_share,
                    evidence_required,
                    evidence_command_refs,
                    evidence_paths,
                    r00_kind,
                    r00_task_ids,
                    r00_execution_stage_ids,
                    r00_ledger_status,
                    r00_result_ids,
                    r00_test_ids,
                    r00_then,
                    r00_assertions,
                    r00_due,
                    assertion_contract,
                    original_assertion_cases,
                });
            }
            leaves
        }
    };

    Ok(StageMap {
        stage,
        result_version,
        default_timeout_secs,
        commands,
        scenarios,
        supplemental_leaves,
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

    // R02 stage-repair R1 / F05: resultVersion is part of the contract.
    #[test]
    fn unknown_result_version_is_a_hard_error() {
        let text = VALID.replace(
            r#""resultVersion": "lingxi.xtask.verify-stage.v1""#,
            r#""resultVersion": "UNSUPPORTED-V999""#,
        );
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("unsupported resultVersion"), "{err}");
    }

    #[test]
    fn missing_result_version_is_a_hard_error() {
        let text = VALID.replace(r#""resultVersion": "lingxi.xtask.verify-stage.v1","#, "");
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("missing resultVersion"), "{err}");
    }

    #[test]
    fn empty_result_version_is_a_hard_error() {
        let text = VALID.replace(
            r#""resultVersion": "lingxi.xtask.verify-stage.v1""#,
            r#""resultVersion": "  ""#,
        );
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("non-empty string"), "{err}");
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

    // R13-F01: supplemental leaf section (parse shape only — the runner
    // owns the R00-ledger equality cross-check). R14-F01: the section now
    // also carries the r00_* mirror fields and the per-leaf
    // assertionContract.
    const SUPPLEMENTAL_VALID: &str = r#"{
        "schemaVersion": 1,
        "resultVersion": "lingxi.xtask.verify-stage.v1",
        "stage": "RX",
        "commands": {
            "a_ok": {
                "argv": ["bash", "gate.sh", "{EVIDENCE}/RX"],
                "evidencePaths": ["{EVIDENCE}/RX/out.txt"]
            }
        },
        "scenarios": [
            {"id": "RX-A01", "requirement": "REQUIRED", "commandRefs": ["a_ok"]}
        ],
        "supplementalLeafScenarios": [
            {
                "id": "R00-T02-LA-TESTOK000000000",
                "featureId": "F-D20-TEST-OK",
                "requirement": "REQUIRED_SUPPLEMENTAL",
                "r00Kind": "supplemental",
                "r00TaskIds": ["R02-T03", "R07-T09"],
                "r00ExecutionStageIds": ["R02", "R07"],
                "r00LedgerStatus": "SPECIFIED_NOT_EXECUTED",
                "r00ResultIds": [],
                "r00TestIds": [],
                "r00Then": "original R00 then text",
                "r00Assertions": ["original R00 assertion one", "original R00 assertion two"],
                "r00Due": "due line from R00",
                "assertionContract": {
                    "producerCommand": "a_ok",
                    "evidencePath": "{EVIDENCE}/RX/leaf-cases.json",
                    "cases": [{"case": "case-no-credential", "expect": 401}]
                },
                "basisKind": "protocol_basis",
                "r02Share": "server-side share of the leaf",
                "r07Share": "client-side remainder",
                "evidenceRequired": "executed records of the referenced commands",
                "evidenceCommandRefs": ["a_ok"]
            }
        ]
    }"#;

    #[test]
    fn parses_a_supplemental_leaf_section() {
        let map = parse_stage_map(SUPPLEMENTAL_VALID).expect("valid supplemental map");
        assert_eq!(map.supplemental_leaves.len(), 1);
        let leaf = &map.supplemental_leaves[0];
        assert_eq!(leaf.basis_kind, BASIS_PROTOCOL);
        assert_eq!(leaf.evidence_command_refs, vec!["a_ok"]);
        assert!(leaf.evidence_paths.is_empty());
        assert_eq!(leaf.r00_kind, "supplemental");
        assert_eq!(leaf.r00_task_ids, vec!["R02-T03", "R07-T09"]);
        assert_eq!(leaf.r00_then, "original R00 then text");
        assert_eq!(leaf.r00_assertions.len(), 2);
        let contract = leaf.assertion_contract.as_ref().expect("contract parsed");
        assert_eq!(contract.producer_command, "a_ok");
        assert_eq!(contract.evidence_path, "{EVIDENCE}/RX/leaf-cases.json");
        assert_eq!(contract.cases.len(), 1);
        assert_eq!(contract.cases[0].expect, 401);
    }

    #[test]
    fn duplicate_pinned_case_name_is_a_hard_error() {
        let text = SUPPLEMENTAL_VALID.replace(
            r#""cases": [{"case": "case-no-credential", "expect": 401}]"#,
            r#""cases": [{"case": "case-no-credential", "expect": 401}, {"case": "case-no-credential", "expect": 403}]"#,
        );
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("repeats case name"), "{err}");
    }

    #[test]
    fn full_original_behavior_requires_distinct_cases_for_every_original_assertion() {
        let mut value: serde_json::Value =
            serde_json::from_str(SUPPLEMENTAL_VALID).expect("valid test fixture");
        value["supplementalLeafScenarios"][0]["basisKind"] =
            serde_json::json!(BASIS_FULL_ORIGINAL_BEHAVIOR);
        value["supplementalLeafScenarios"][0]["assertionContract"]["cases"] = serde_json::json!([
            {"case":"positive","expect":200},
            {"case":"negative","expect":403}
        ]);
        let missing = parse_stage_map(&value.to_string()).unwrap_err();
        assert!(
            missing.contains("requires originalAssertionCases"),
            "{missing}"
        );

        value["supplementalLeafScenarios"][0]["originalAssertionCases"] =
            serde_json::json!([["positive"], ["positive"]]);
        let reused = parse_stage_map(&value.to_string()).unwrap_err();
        assert!(reused.contains("reuses original-behavior case"), "{reused}");

        value["supplementalLeafScenarios"][0]["originalAssertionCases"] =
            serde_json::json!([["positive"], ["negative"]]);
        let parsed = parse_stage_map(&value.to_string()).expect("complete assertion coverage");
        assert_eq!(
            parsed.supplemental_leaves[0].original_assertion_cases.len(),
            2
        );
    }

    #[test]
    fn empty_supplemental_section_is_a_hard_error() {
        // Truncate the map right after the section key and close it with an
        // empty array: declaring the section with zero entries would drop
        // every R00 REQUIRED_SUPPLEMENTAL obligation from the gate.
        let mut hacked = String::new();
        hacked.push_str(
            SUPPLEMENTAL_VALID
                .split("\"supplementalLeafScenarios\"")
                .next()
                .unwrap(),
        );
        hacked.push_str("\"supplementalLeafScenarios\": []}");
        let err = parse_stage_map(&hacked).unwrap_err();
        assert!(err.contains("must not be EMPTY"), "{err}");
    }

    #[test]
    fn unknown_supplemental_basis_kind_is_a_hard_error() {
        let text = SUPPLEMENTAL_VALID.replace(
            r#""basisKind": "protocol_basis""#,
            r#""basisKind": "mystery""#,
        );
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("unknown basisKind"), "{err}");
    }

    #[test]
    fn supplemental_leaf_without_command_refs_is_a_hard_error() {
        let text = SUPPLEMENTAL_VALID.replace(
            r#""evidenceCommandRefs": ["a_ok"]"#,
            r#""evidenceCommandRefs": []"#,
        );
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("hole in the map"), "{err}");
    }

    #[test]
    fn supplemental_leaf_with_unknown_command_ref_is_a_hard_error() {
        let text = SUPPLEMENTAL_VALID.replace(
            r#""evidenceCommandRefs": ["a_ok"]"#,
            r#""evidenceCommandRefs": ["nope"]"#,
        );
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("unknown command"), "{err}");
    }

    #[test]
    fn conflicted_leaf_must_not_bind_evidence() {
        let text = SUPPLEMENTAL_VALID.replace(
            r#""basisKind": "protocol_basis""#,
            r#""basisKind": "client_only_stage_boundary_conflict""#,
        );
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("must not bind evidence commands"), "{err}");
    }

    #[test]
    fn conflicted_leaf_parses_when_unbound() {
        let text = SUPPLEMENTAL_VALID
            .replace(
                r#""basisKind": "protocol_basis""#,
                r#""basisKind": "client_only_stage_boundary_conflict""#,
            )
            .replace(r#""evidenceCommandRefs": ["a_ok"]"#, r#""evidenceCommandRefs": []"#)
            // R14-F01: a conflict leaf must also drop the assertion
            // contract — it can never be PASS, so it has no pass-able
            // machine cases either.
            .replace(
                concat!(
                    r#"                "assertionContract": {"#, "\n",
                    r#"                    "producerCommand": "a_ok","#, "\n",
                    r#"                    "evidencePath": "{EVIDENCE}/RX/leaf-cases.json","#, "\n",
                    r#"                    "cases": [{"case": "case-no-credential", "expect": 401}]"#, "\n",
                    r#"                },"#, "\n",
                ),
                "",
            );
        let map = parse_stage_map(&text).expect("unbound conflicted leaf parses");
        assert_eq!(
            map.supplemental_leaves[0].basis_kind,
            BASIS_CLIENT_ONLY_STAGE_CONFLICT
        );
        assert!(map.supplemental_leaves[0].evidence_command_refs.is_empty());
        assert!(map.supplemental_leaves[0].assertion_contract.is_none());
    }

    // R14-F01: mirror fields and the assertion contract are hard parse
    // requirements — the fake-green paths they close are listed per test.
    #[test]
    fn missing_r00_mirror_field_is_a_hard_error() {
        let text = SUPPLEMENTAL_VALID.replace(
            r#"                "r00Then": "original R00 then text","#,
            "",
        );
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("missing r00Then"), "{err}");
    }

    #[test]
    fn empty_r00_responsibility_mirror_is_a_hard_error() {
        let text = SUPPLEMENTAL_VALID.replace(
            r#""r00TaskIds": ["R02-T03", "R07-T09"]"#,
            r#""r00TaskIds": []"#,
        );
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("r00TaskIds must not be empty"), "{err}");
    }

    #[test]
    fn missing_assertion_contract_is_a_hard_error() {
        // Dropping the contract would reduce the leaf's PASS to "command
        // exited 0" — exactly the exit-0-only fake-green path R14-F01
        // closes.
        let text = SUPPLEMENTAL_VALID.replace(
            concat!(
                r#"                "assertionContract": {"#,
                "\n",
                r#"                    "producerCommand": "a_ok","#,
                "\n",
                r#"                    "evidencePath": "{EVIDENCE}/RX/leaf-cases.json","#,
                "\n",
                r#"                    "cases": [{"case": "case-no-credential", "expect": 401}]"#,
                "\n",
                r#"                },"#,
                "\n",
            ),
            "",
        );
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("missing assertionContract"), "{err}");
    }

    #[test]
    fn conflicted_leaf_with_assertion_contract_is_a_hard_error() {
        let text = SUPPLEMENTAL_VALID
            .replace(
                r#""basisKind": "protocol_basis""#,
                r#""basisKind": "client_only_stage_boundary_conflict""#,
            )
            .replace(
                r#""evidenceCommandRefs": ["a_ok"]"#,
                r#""evidenceCommandRefs": []"#,
            );
        let err = parse_stage_map(&text).unwrap_err();
        assert!(
            err.contains("must not declare an assertionContract"),
            "{err}"
        );
    }

    #[test]
    fn producer_outside_leaf_refs_is_a_hard_error() {
        // A producer the leaf does not reference would never RUN (the
        // runner's execution order derives from evidenceCommandRefs), so
        // the leaf's evidence could never be written this run.
        // Add a second command b_two and point the contract at it while
        // the leaf still references only a_ok.
        let mut value: serde_json::Value =
            serde_json::from_str(SUPPLEMENTAL_VALID).expect("valid test fixture");
        value["commands"]["b_two"] = serde_json::json!({
            "argv": ["bash", "gate2.sh", "{EVIDENCE}/RX"],
            "evidencePaths": ["{EVIDENCE}/RX/out2.txt"]
        });
        value["supplementalLeafScenarios"][0]["assertionContract"]["producerCommand"] =
            serde_json::json!("b_two");
        let text = value.to_string();
        let err = parse_stage_map(&text).unwrap_err();
        assert!(
            err.contains("is not in the leaf's evidenceCommandRefs"),
            "{err}"
        );
    }

    #[test]
    fn empty_assertion_contract_cases_is_a_hard_error() {
        let text = SUPPLEMENTAL_VALID.replace(
            r#""cases": [{"case": "case-no-credential", "expect": 401}]"#,
            r#""cases": []"#,
        );
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("cases must not be empty"), "{err}");
    }

    #[test]
    fn non_integer_case_expect_is_a_hard_error() {
        let text = SUPPLEMENTAL_VALID.replace(r#""expect": 401"#, r#""expect": "401""#);
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("integer expect"), "{err}");
    }

    #[test]
    fn duplicate_supplemental_leaf_ids_are_a_hard_error() {
        let text = SUPPLEMENTAL_VALID.replace(
            r#""evidenceCommandRefs": ["a_ok"]
            }
        ]"#,
            r#""evidenceCommandRefs": ["a_ok"]
            },
            {
                "id": "R00-T02-LA-TESTOK000000000",
                "featureId": "F-D20-TEST-OK",
                "requirement": "REQUIRED_SUPPLEMENTAL",
                "basisKind": "protocol_basis",
                "r02Share": "server-side share of the leaf",
                "r07Share": "client-side remainder",
                "evidenceRequired": "executed records of the referenced commands",
                "evidenceCommandRefs": ["a_ok"]
            }
        ]"#,
        );
        let err = parse_stage_map(&text).unwrap_err();
        assert!(err.contains("duplicate supplemental leaf id"), "{err}");
    }
}
