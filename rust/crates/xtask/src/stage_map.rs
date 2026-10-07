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
/// R02-final repair group 1 adds the stage-ownership kinds commanded by
/// the R02 closeout directive (the R17-era "pull the R07-T09 client
/// behavior into the R02 gate" authorization is revoked for GATING
/// purposes; the early implementations stay registered and running):
/// `r02_share_satisfied` declares that the leaf's pinned cases are exactly
/// the R02 share — all share pins holding is a PASS for the R02 share
/// (the R07 remainder is carried as text, never as a gate precondition),
/// and `deferred_to_r07` declares a leaf with NO R02 gating share: it is
/// still REQUIRED, its acceptance moves to R07, and it may only bind
/// NON-gating `earlyEvidenceCommandRefs` (health of the pre-implemented
/// code) — never a gating contract.
pub const BASIS_ROUTE_PRESENT_STATIC: &str = "route_basis_present_static";
pub const BASIS_PROTOCOL: &str = "protocol_basis";
pub const BASIS_AUTH_PRIMITIVE_ONLY: &str = "auth_primitive_only";
pub const BASIS_CLIENT_ONLY_STAGE_CONFLICT: &str = "client_only_stage_boundary_conflict";
/// 原行为（含前端正反分支）已由本阶段独立生产者完整实测；必须逐条列出
/// R00 原断言与专属案例的对应关系，不能只把原 R07 份额写成字符串后放行。
pub const BASIS_FULL_ORIGINAL_BEHAVIOR: &str = "full_original_behavior";
/// R02 份额已证：本叶钉住的案例即 R02 份额全部内容，份额图钉全过即本叶
/// R02 份额 PASS；R07 余款以 r07Share 文本随结果输出，不作为 R02 门禁。
pub const BASIS_R02_SHARE_SATISFIED: &str = "r02_share_satisfied";
/// 无 R02 门禁份额：仍 REQUIRED，验收归属 R07（DEFERRED_TO_R07 ≠
/// NOT_APPLICABLE/optional，最终产品要求不变）。仅 r00_execution_stage_ids
/// 含后续阶段的叶允许声明；不得带门禁 assertionContract/门禁 commandRefs。
pub const BASIS_DEFERRED_TO_R07: &str = "deferred_to_r07";
/// R03-T08 阶段中立份额类（语义与 r02_share_satisfied 同构）：本阶段钉住
/// 的案例即本阶段份额全部内容（`stageShare` 写明份额边界），份额图钉全
/// 过即本阶段份额 PASS；余款仍 REQUIRED，随结果以 `laterShare` 文本 +
/// `deferredToStages`（由 r00_execution_stage_ids 去本阶段派生）输出，不
/// 作为本阶段门禁。必须带 assertionContract（R14-F01 反假绿规则同构）。
pub const BASIS_STAGE_SHARE_SATISFIED: &str = "stage_share_satisfied";
/// 阶段中立递延类（语义与 deferred_to_r07 同构，递延目标不限于 R07）：
/// 无本阶段门禁份额，仍 REQUIRED，验收归属 r00_execution_stage_ids 中的
/// 后续阶段（R06/R07/R08…）。仅含后续阶段的叶允许声明；不得带门禁
/// assertionContract/门禁 commandRefs/可过 evidencePaths；可带非门禁
/// earlyEvidenceCommandRefs（提前实现代码的健康照跑）。
pub const BASIS_DEFERRED_TO_LATER_STAGE: &str = "deferred_to_later_stage";
pub const SUPPLEMENTAL_BASIS_KINDS: &[&str] = &[
    BASIS_ROUTE_PRESENT_STATIC,
    BASIS_PROTOCOL,
    BASIS_AUTH_PRIMITIVE_ONLY,
    BASIS_CLIENT_ONLY_STAGE_CONFLICT,
    BASIS_FULL_ORIGINAL_BEHAVIOR,
    BASIS_R02_SHARE_SATISFIED,
    BASIS_DEFERRED_TO_R07,
    BASIS_STAGE_SHARE_SATISFIED,
    BASIS_DEFERRED_TO_LATER_STAGE,
];

/// The stage-neutral share kinds (R03+): their share text lives in the
/// `stageShare`/`laterShare` fields instead of the R02-named
/// `r02Share`/`r07Share` pair. Both pairs are carried in the struct so
/// the legacy R02 map and the stage-neutral maps share one parser; each
/// branch only ever reads its own pair.
pub fn is_stage_neutral_kind(basis_kind: &str) -> bool {
    basis_kind == BASIS_STAGE_SHARE_SATISFIED || basis_kind == BASIS_DEFERRED_TO_LATER_STAGE
}

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

/// One NON-gating deferred R07 case record (R02-final group 1): the case
/// belongs to the leaf's R07 remainder (client rendering, deferred
/// capability), so the gate may OBSERVE its recorded value but never uses
/// it to judge the leaf in R02. `producer_command`/`evidence_path` name
/// where the observation comes from — the same producer machinery as the
/// gating contract, minus the gate.
#[derive(Debug, Clone, PartialEq)]
pub struct LeafDeferredCase {
    pub case: String,
    pub expect: i64,
    pub producer_command: String,
    pub evidence_path: String,
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
    /// Stage-neutral share texts (stage_share_satisfied /
    /// deferred_to_later_stage). Empty for the R02-named kinds.
    pub stage_share: String,
    pub later_share: String,
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
    /// R07 递延案例登记（仅 r02_share_satisfied / deferred_to_r07 可声明）：
    /// 记录归属 R07 验收的案例（如 sessions 叶的 4 个 CLI 渲染案例、
    /// 纯客户端叶的全部提前实现案例）。仅观察、不判 R02。
    pub deferred_r07_cases: Vec<LeafDeferredCase>,
    /// 非门禁提前实现证据命令（仅 deferred_to_r07 可声明）：命令保留注册
    /// 并照常运行（失败仍是命令 FAIL——提前实现的已提交代码必须保持
    /// 健康），但不构成本叶的 R02 门禁。
    pub early_evidence_command_refs: Vec<String>,
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
                let (r02_share, r07_share, stage_share, later_share) =
                    if basis_kind == BASIS_FULL_ORIGINAL_BEHAVIOR {
                        // R05 RR1 F25: a FULL leaf owns its original behavior
                        // in THIS stage — there is no share split at all, so
                        // none of the four share texts is required; the
                        // evidence is the per-assertion case coverage
                        // (originalAssertionCases, validated below).
                        (String::new(), String::new(), String::new(), String::new())
                    } else if is_stage_neutral_kind(&basis_kind) {
                        let stage_share = non_empty_string(
                            entry.get("stageShare").ok_or_else(|| {
                                err(&format!(
                                "supplemental leaf {id:?} missing stageShare (the stage-neutral \
                                 share kinds declare their share here, not r02Share)"
                            ))
                            })?,
                            &format!("supplemental leaf {id:?} stageShare"),
                        )?;
                        let later_share = non_empty_string(
                            entry.get("laterShare").ok_or_else(|| {
                                err(&format!("supplemental leaf {id:?} missing laterShare"))
                            })?,
                            &format!("supplemental leaf {id:?} laterShare"),
                        )?;
                        (String::new(), String::new(), stage_share, later_share)
                    } else {
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
                        (r02_share, r07_share, String::new(), String::new())
                    };
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
                } else if basis_kind == BASIS_DEFERRED_TO_R07
                    || basis_kind == BASIS_DEFERRED_TO_LATER_STAGE
                {
                    // 递延叶的后续阶段义务不由本门禁判定：绑定门禁证据命令
                    // 等于把后续阶段验收又拉回本阶段，与递延声明自相矛盾。
                    if !evidence_command_refs.is_empty() {
                        return Err(err(&format!(
                            "supplemental leaf {id:?} is deferred to R07 and must not bind \
                             GATING evidence commands (its acceptance belongs to R07; only \
                             non-gating earlyEvidenceCommandRefs may keep the pre-implemented \
                             producers running)"
                        )));
                    }
                } else if evidence_command_refs.is_empty() {
                    return Err(err(&format!(
                        "supplemental leaf {id:?} evidenceCommandRefs must not be empty (a \
                         leaf with an R02 share bound to no evidence command is a hole in \
                         the map, not a pass)"
                    )));
                }
                // 非门禁提前实现证据命令：仅递延叶可声明，且必须是已注册
                // 命令（这些命令照常运行、照常计入 freshness/清理检查）。
                let early_evidence_command_refs = match entry.get("earlyEvidenceCommandRefs") {
                    Some(v) => {
                        if basis_kind != BASIS_DEFERRED_TO_R07
                            && basis_kind != BASIS_DEFERRED_TO_LATER_STAGE
                        {
                            return Err(err(&format!(
                                "supplemental leaf {id:?} declares earlyEvidenceCommandRefs \
                                 without deferred_to_r07 (only a deferred leaf may keep \
                                 non-gating early-evidence producers running)"
                            )));
                        }
                        let list = v.as_array().ok_or_else(|| {
                            err(&format!(
                                "supplemental leaf {id:?} earlyEvidenceCommandRefs must be \
                                 an array"
                            ))
                        })?;
                        let mut refs = Vec::with_capacity(list.len());
                        for reference in list {
                            let key = non_empty_string(
                                reference,
                                &format!(
                                    "supplemental leaf {id:?} earlyEvidenceCommandRefs element"
                                ),
                            )?;
                            if !commands_value.contains_key(&key) {
                                return Err(err(&format!(
                                    "supplemental leaf {id:?} references unknown early-evidence \
                                     command {key:?}"
                                )));
                            }
                            refs.push(key);
                        }
                        refs
                    }
                    None => Vec::new(),
                };
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
                        if basis_kind == BASIS_DEFERRED_TO_R07 && !paths.is_empty() {
                            return Err(err(&format!(
                                "supplemental leaf {id:?} is deferred to R07 and must not \
                                 declare pass-able evidencePaths (its acceptance is not judged \
                                 by this gate; the early-evidence producers' own declared \
                                 evidencePaths carry the freshness checks)"
                            )));
                        }
                        if basis_kind == BASIS_DEFERRED_TO_LATER_STAGE && !paths.is_empty() {
                            return Err(err(&format!(
                                "supplemental leaf {id:?} is deferred to a later stage and \
                                 must not declare pass-able evidencePaths (its acceptance is \
                                 not judged by this gate; the early-evidence producers' own \
                                 declared evidencePaths carry the freshness checks)"
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
                // 递延只对真实双阶段（含后续阶段）的叶合法：一个只归属
                // 本阶段的叶没有可递延的验收归属，标 deferred 只会是
                // 把本阶段义务静默扫地出门。
                if (basis_kind == BASIS_DEFERRED_TO_R07
                    || basis_kind == BASIS_DEFERRED_TO_LATER_STAGE)
                    && !r00_execution_stage_ids.iter().any(|s| s.as_str() != stage)
                {
                    return Err(err(&format!(
                        "supplemental leaf {id:?} is marked deferred_to_r07 but its \
                         r00ExecutionStageIds {r00_execution_stage_ids:?} contain no later \
                         stage — deferral requires a later stage to carry the acceptance \
                         (a single-{stage} leaf may never defer)"
                    )));
                }
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
                // NON-conflict, NON-deferred leaf MUST declare one — a leaf
                // whose PASS is justified only by a green command exit (with
                // no case the gate can re-verify against the original
                // assertion) is exactly the fake-green path R14-F01 closes.
                // A conflict leaf must NOT declare one (it can never be
                // PASS); a deferred_to_r07 leaf must not either (its R07
                // acceptance is not judged by this gate — the deferred cases
                // below are observations, never gates).
                let uncontracted_kind = basis_kind == BASIS_CLIENT_ONLY_STAGE_CONFLICT
                    || basis_kind == BASIS_DEFERRED_TO_R07
                    || basis_kind == BASIS_DEFERRED_TO_LATER_STAGE;
                let assertion_contract = match entry.get("assertionContract") {
                    Some(v) => {
                        if uncontracted_kind {
                            return Err(err(&format!(
                                "supplemental leaf {id:?} must not declare an assertionContract \
                                 ({} can never be judged PASS by this gate)",
                                basis_kind
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
                        if !uncontracted_kind {
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
                // R02-final group 1: the deferred R07 case records — which
                // cases of this leaf belong to the R07 remainder (never
                // judged by this gate, only observed). Only the
                // stage-ownership kinds may declare them: a legacy-kind or
                // full_original_behavior leaf pinning a "deferred" case
                // would blur which cases are the gate.
                let deferred_r07_cases = match entry.get("deferredR07Cases") {
                    Some(v) => {
                        if basis_kind != BASIS_R02_SHARE_SATISFIED
                            && basis_kind != BASIS_DEFERRED_TO_R07
                            && basis_kind != BASIS_STAGE_SHARE_SATISFIED
                            && basis_kind != BASIS_DEFERRED_TO_LATER_STAGE
                        {
                            return Err(err(&format!(
                                "supplemental leaf {id:?} declares deferredR07Cases with \
                                 basisKind {basis_kind:?} (only r02_share_satisfied and \
                                 deferred_to_r07 may record R07-remainder cases)"
                            )));
                        }
                        let list = v.as_array().ok_or_else(|| {
                            err(&format!(
                                "supplemental leaf {id:?} deferredR07Cases must be an array"
                            ))
                        })?;
                        let mut deferred = Vec::with_capacity(list.len());
                        for item in list {
                            let object = item.as_object().ok_or_else(|| {
                                err(&format!(
                                    "supplemental leaf {id:?} deferredR07Cases entry must be \
                                     an object"
                                ))
                            })?;
                            let case = non_empty_string(
                                object.get("case").ok_or_else(|| {
                                    err(&format!(
                                        "supplemental leaf {id:?} deferredR07Cases entry \
                                         missing case name"
                                    ))
                                })?,
                                &format!("supplemental leaf {id:?} deferredR07Cases case name"),
                            )?;
                            if deferred
                                .iter()
                                .any(|recorded: &LeafDeferredCase| recorded.case == case)
                            {
                                return Err(err(&format!(
                                    "supplemental leaf {id:?} deferredR07Cases repeats case \
                                     name {case:?}"
                                )));
                            }
                            if let Some(contract) = assertion_contract.as_ref() {
                                if contract.cases.iter().any(|pinned| pinned.case == case) {
                                    return Err(err(&format!(
                                        "supplemental leaf {id:?} case {case:?} is both a \
                                         gating pin and a deferred R07 case — each case must \
                                         have exactly one ownership"
                                    )));
                                }
                            }
                            let expect =
                                object
                                    .get("expect")
                                    .and_then(Value::as_i64)
                                    .ok_or_else(|| {
                                        err(&format!(
                                            "supplemental leaf {id:?} deferredR07Cases case \
                                         {case:?} must have an integer expect value"
                                        ))
                                    })?;
                            let producer_command = non_empty_string(
                                object.get("producerCommand").ok_or_else(|| {
                                    err(&format!(
                                        "supplemental leaf {id:?} deferredR07Cases case \
                                         {case:?} missing producerCommand"
                                    ))
                                })?,
                                &format!(
                                    "supplemental leaf {id:?} deferredR07Cases case {case:?} \
                                     producerCommand"
                                ),
                            )?;
                            if !commands_value.contains_key(&producer_command) {
                                return Err(err(&format!(
                                    "supplemental leaf {id:?} deferredR07Cases case {case:?} \
                                     references unknown producer command {producer_command:?}"
                                )));
                            }
                            let runs_for_this_leaf = evidence_command_refs
                                .contains(&producer_command)
                                || early_evidence_command_refs.contains(&producer_command);
                            if !runs_for_this_leaf {
                                return Err(err(&format!(
                                    "supplemental leaf {id:?} deferredR07Cases case {case:?} \
                                     producer {producer_command:?} is neither a gating nor an \
                                     early-evidence command of this leaf — a producer that \
                                     never runs can never write the case's evidence"
                                )));
                            }
                            let evidence_path = non_empty_string(
                                object.get("evidencePath").ok_or_else(|| {
                                    err(&format!(
                                        "supplemental leaf {id:?} deferredR07Cases case \
                                         {case:?} missing evidencePath"
                                    ))
                                })?,
                                &format!(
                                    "supplemental leaf {id:?} deferredR07Cases case {case:?} \
                                     evidencePath"
                                ),
                            )?;
                            deferred.push(LeafDeferredCase {
                                case,
                                expect,
                                producer_command,
                                evidence_path,
                            });
                        }
                        deferred
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
                    stage_share,
                    later_share,
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
                    deferred_r07_cases,
                    early_evidence_command_refs,
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

    // ── R02-final group 1: r02_share_satisfied / deferred_to_r07 ─────────

    /// Builds a test map by mutating SUPPLEMENTAL_VALID's single leaf with
    /// `edit` (a serde_json::Value → Value in-place closure) and parses it.
    fn edited_map(edit: impl FnOnce(&mut serde_json::Value)) -> Result<StageMap, String> {
        let mut value: serde_json::Value =
            serde_json::from_str(SUPPLEMENTAL_VALID).expect("valid test fixture");
        edit(&mut value);
        parse_stage_map(&value.to_string())
    }

    /// Rewrites the leaf into a minimal deferred_to_r07 shape (drops the
    /// contract and the gating refs); extra mutations on top.
    fn deferred_leaf(edit: impl FnOnce(&mut serde_json::Value)) -> Result<StageMap, String> {
        edited_map(|value| {
            let leaf = &mut value["supplementalLeafScenarios"][0];
            leaf["basisKind"] = serde_json::json!("deferred_to_r07");
            leaf["evidenceCommandRefs"] = serde_json::json!([]);
            leaf.as_object_mut().unwrap().remove("assertionContract");
            edit(value);
        })
    }

    #[test]
    fn share_satisfied_leaf_parses_with_contract() {
        let map = edited_map(|value| {
            value["supplementalLeafScenarios"][0]["basisKind"] =
                serde_json::json!(BASIS_R02_SHARE_SATISFIED);
        })
        .expect("share leaf parses");
        let leaf = &map.supplemental_leaves[0];
        assert_eq!(leaf.basis_kind, BASIS_R02_SHARE_SATISFIED);
        assert!(leaf.assertion_contract.is_some());
        assert!(leaf.deferred_r07_cases.is_empty());
    }

    #[test]
    fn share_satisfied_leaf_without_contract_is_a_hard_error() {
        let err = edited_map(|value| {
            value["supplementalLeafScenarios"][0]["basisKind"] =
                serde_json::json!(BASIS_R02_SHARE_SATISFIED);
            value["supplementalLeafScenarios"][0]
                .as_object_mut()
                .unwrap()
                .remove("assertionContract");
        })
        .unwrap_err();
        assert!(err.contains("missing assertionContract"), "{err}");
    }

    #[test]
    fn deferred_leaf_parses_with_early_evidence_and_deferred_cases() {
        let map = deferred_leaf(|value| {
            let leaf = &mut value["supplementalLeafScenarios"][0];
            leaf["earlyEvidenceCommandRefs"] = serde_json::json!(["a_ok"]);
            leaf["deferredR07Cases"] = serde_json::json!([{
                "case": "client-render",
                "expect": 1,
                "producerCommand": "a_ok",
                "evidencePath": "{EVIDENCE}/RX/leaf-cases.json",
            }]);
        })
        .expect("deferred leaf parses");
        let leaf = &map.supplemental_leaves[0];
        assert_eq!(leaf.basis_kind, BASIS_DEFERRED_TO_R07);
        assert!(leaf.evidence_command_refs.is_empty());
        assert!(leaf.assertion_contract.is_none());
        assert_eq!(leaf.early_evidence_command_refs, vec!["a_ok"]);
        assert_eq!(leaf.deferred_r07_cases.len(), 1);
        assert_eq!(leaf.deferred_r07_cases[0].case, "client-render");
        assert_eq!(leaf.deferred_r07_cases[0].expect, 1);
    }

    #[test]
    fn deferred_leaf_with_assertion_contract_is_a_hard_error() {
        let err = edited_map(|value| {
            value["supplementalLeafScenarios"][0]["basisKind"] =
                serde_json::json!(BASIS_DEFERRED_TO_R07);
            value["supplementalLeafScenarios"][0]["evidenceCommandRefs"] = serde_json::json!([]);
            // keep the assertionContract on purpose
        })
        .unwrap_err();
        assert!(
            err.contains("must not declare an assertionContract"),
            "{err}"
        );
    }

    #[test]
    fn deferred_leaf_with_gating_refs_is_a_hard_error() {
        // Keeping GATING evidenceCommandRefs on a deferred leaf must not
        // parse — gating refs would pull the R07 acceptance back into R02.
        let err = edited_map(|value| {
            value["supplementalLeafScenarios"][0]["basisKind"] =
                serde_json::json!(BASIS_DEFERRED_TO_R07);
            value["supplementalLeafScenarios"][0]
                .as_object_mut()
                .unwrap()
                .remove("assertionContract");
            // evidenceCommandRefs stays ["a_ok"]
        })
        .unwrap_err();
        assert!(
            err.contains("must not bind GATING evidence commands"),
            "{err}"
        );
    }

    #[test]
    fn deferred_leaf_with_evidence_paths_is_a_hard_error() {
        let err = deferred_leaf(|value| {
            value["supplementalLeafScenarios"][0]["earlyEvidenceCommandRefs"] =
                serde_json::json!(["a_ok"]);
            value["supplementalLeafScenarios"][0]["evidencePaths"] =
                serde_json::json!(["{EVIDENCE}/RX/out.txt"]);
        })
        .unwrap_err();
        assert!(
            err.contains("must not declare pass-able evidencePaths"),
            "{err}"
        );
    }

    #[test]
    fn single_stage_leaf_cannot_defer() {
        let err = deferred_leaf(|value| {
            value["supplementalLeafScenarios"][0]["r00ExecutionStageIds"] =
                serde_json::json!(["RX"]);
        })
        .unwrap_err();
        assert!(err.contains("contain no later stage"), "{err}");
    }

    #[test]
    fn deferred_cases_on_a_legacy_kind_are_a_hard_error() {
        let err = edited_map(|value| {
            value["supplementalLeafScenarios"][0]["deferredR07Cases"] = serde_json::json!([{
                "case": "client-render",
                "expect": 1,
                "producerCommand": "a_ok",
                "evidencePath": "{EVIDENCE}/RX/leaf-cases.json",
            }]);
        })
        .unwrap_err();
        assert!(
            err.contains("only r02_share_satisfied and deferred_to_r07"),
            "{err}"
        );
    }

    #[test]
    fn deferred_case_with_unknown_producer_is_a_hard_error() {
        let err = deferred_leaf(|value| {
            let leaf = &mut value["supplementalLeafScenarios"][0];
            leaf["earlyEvidenceCommandRefs"] = serde_json::json!(["a_ok"]);
            leaf["deferredR07Cases"] = serde_json::json!([{
                "case": "client-render",
                "expect": 1,
                "producerCommand": "not_registered",
                "evidencePath": "{EVIDENCE}/RX/leaf-cases.json",
            }]);
        })
        .unwrap_err();
        assert!(err.contains("references unknown producer command"), "{err}");
    }

    #[test]
    fn deferred_case_producer_outside_leaf_commands_is_a_hard_error() {
        // Producer is REGISTERED (a second command) but neither a gating nor
        // an early-evidence command of this leaf — it would never run for
        // the leaf, so its case evidence could never be written this run.
        let err = edited_map(|value| {
            let leaf = &mut value["supplementalLeafScenarios"][0];
            leaf["basisKind"] = serde_json::json!(BASIS_R02_SHARE_SATISFIED);
            leaf["deferredR07Cases"] = serde_json::json!([{
                "case": "client-render",
                "expect": 1,
                "producerCommand": "b_two",
                "evidencePath": "{EVIDENCE}/RX/out2.txt",
            }]);
            value["commands"]["b_two"] = serde_json::json!({
                "argv": ["bash", "gate2.sh", "{EVIDENCE}/RX"],
                "evidencePaths": ["{EVIDENCE}/RX/out2.txt"]
            });
        })
        .unwrap_err();
        assert!(
            err.contains("is neither a gating nor an early-evidence command"),
            "{err}"
        );
    }

    #[test]
    fn deferred_case_names_must_be_unique() {
        let err = deferred_leaf(|value| {
            let leaf = &mut value["supplementalLeafScenarios"][0];
            leaf["earlyEvidenceCommandRefs"] = serde_json::json!(["a_ok"]);
            leaf["deferredR07Cases"] = serde_json::json!([
                { "case": "client-render", "expect": 1, "producerCommand": "a_ok",
                  "evidencePath": "{EVIDENCE}/RX/leaf-cases.json" },
                { "case": "client-render", "expect": 1, "producerCommand": "a_ok",
                  "evidencePath": "{EVIDENCE}/RX/leaf-cases.json" },
            ]);
        })
        .unwrap_err();
        assert!(err.contains("repeats case name"), "{err}");
    }

    #[test]
    fn a_case_cannot_be_both_gating_and_deferred() {
        let err = edited_map(|value| {
            let leaf = &mut value["supplementalLeafScenarios"][0];
            leaf["basisKind"] = serde_json::json!(BASIS_R02_SHARE_SATISFIED);
            // "case-no-credential" is already a gating pin of the contract.
            leaf["deferredR07Cases"] = serde_json::json!([{
                "case": "case-no-credential",
                "expect": 401,
                "producerCommand": "a_ok",
                "evidencePath": "{EVIDENCE}/RX/leaf-cases.json",
            }]);
        })
        .unwrap_err();
        assert!(
            err.contains("both a gating pin and a deferred R07 case"),
            "{err}"
        );
    }

    #[test]
    fn early_evidence_refs_require_a_deferred_leaf() {
        let err = edited_map(|value| {
            value["supplementalLeafScenarios"][0]["earlyEvidenceCommandRefs"] =
                serde_json::json!(["a_ok"]);
        })
        .unwrap_err();
        assert!(err.contains("without deferred_to_r07"), "{err}");
    }

    #[test]
    fn early_evidence_refs_must_reference_registered_commands() {
        let err = deferred_leaf(|value| {
            value["supplementalLeafScenarios"][0]["earlyEvidenceCommandRefs"] =
                serde_json::json!(["nope"]);
        })
        .unwrap_err();
        assert!(err.contains("unknown early-evidence command"), "{err}");
    }

    // F1 regression: an assertionContract producer that is not a registered
    // command must be a LOAD-time hard error (the R02 map once referenced
    // `supplemental_cli_rust_matrix` before registering it).
    #[test]
    fn unregistered_producer_command_is_a_hard_parse_error() {
        let err = edited_map(|value| {
            let leaf = &mut value["supplementalLeafScenarios"][0];
            leaf["evidenceCommandRefs"] = serde_json::json!(["ghost_producer"]);
            leaf["assertionContract"]["producerCommand"] = serde_json::json!("ghost_producer");
        })
        .unwrap_err();
        // The leaf-level refs check fires first and names the command.
        assert!(err.contains("references unknown command"), "{err}");
        // Pointing ONLY the producer at the ghost (refs still a_ok) hits the
        // producer-specific message instead.
        let err = edited_map(|value| {
            value["supplementalLeafScenarios"][0]["assertionContract"]["producerCommand"] =
                serde_json::json!("ghost_producer");
        })
        .unwrap_err();
        assert!(err.contains("references unknown producer command"), "{err}");
    }

    // ── R03 repair round G07 / F08: pin the PRODUCTION R03 map ───────────
    //
    // The gate's own cross-checks guard the R00 supplemental leaves and the
    // per-command evidence, but a base SCENARIO registration exists only in
    // the map itself: deleting R03-RP01 (or the repair command, or the
    // producer's pin table) would otherwise leave a silent hole the gate
    // cannot see. These tests pin the real registered map — they run inside
    // `cargo test` (itself the gate's `rust_test_workspace` command), so any
    // deletion/drift turns the stage gate red with the gap named here.

    /// The REAL registered R03 stage map (same bytes `verify-stage R03`
    /// loads via STAGE_MAPS).
    const R03_PRODUCTION: &str = include_str!("stage_maps/R03.json");

    /// The F01–F07 repair suites plus the RR2 fixed-repair suite, and
    /// their EXACT pinned test counts, as the producer script's
    /// `pin <suite> <count> <F-ID>` table must declare them (G07/F08-C02:
    /// no missing suite, no extra suite, no count drift).
    /// RR2 increment (R03-RR2-F05-01, 2026-09-30): the tenth entry
    /// `("request_id_canonicalization", 7, "RR2-F05")` — the canonical
    /// requestId chain cases C01–C05; nothing above was removed or lowered.
    const R03_REPAIR_PIN_TABLE: &[(&str, u32, &str)] = &[
        ("cancel_link_inheritance", 7, "F01"),
        ("subagent_closeout", 8, "F02"),
        ("cancel_terminal_race", 13, "F03"),
        ("tool_receipt_unknown", 6, "F04"),
        ("admission_dedup_consistency", 5, "F05"),
        ("admission_dedup_adversarial", 5, "F05"),
        ("input_payload_fidelity", 5, "F06"),
        ("input_budget_refusal", 2, "F06"),
        ("background_steering", 8, "F07"),
        ("request_id_canonicalization", 7, "RR2-F05"),
    ];

    fn parse_production_r03() -> StageMap {
        parse_stage_map(R03_PRODUCTION)
            .expect("the registered R03 stage map must parse with this runner")
    }

    #[test]
    fn r03_production_map_keeps_the_sixteen_a_scenarios_verbatim() {
        let map = parse_production_r03();
        // The frozen T08 registration: id → commandRefs, byte-for-byte.
        let expected: &[(&str, &[&str])] = &[
            ("R03-A01", &["rust_test_workspace", "a15_combo_and_leaves"]),
            ("R03-A02", &["rust_test_workspace", "r02_storage_tx"]),
            ("R03-A03", &["rust_test_workspace"]),
            ("R03-A04", &["rust_test_workspace"]),
            ("R03-A05", &["rust_test_workspace", "a15_combo_and_leaves"]),
            ("R03-A06", &["rust_test_workspace", "a15_combo_and_leaves"]),
            ("R03-A07", &["rust_test_workspace", "a15_combo_and_leaves"]),
            ("R03-A08", &["rust_test_workspace", "a15_combo_and_leaves"]),
            ("R03-A09", &["rust_test_workspace"]),
            ("R03-A10", &["rust_test_workspace"]),
            ("R03-A11", &["rust_test_workspace", "a15_combo_and_leaves"]),
            ("R03-A12", &["rust_test_workspace", "a15_combo_and_leaves"]),
            (
                "R03-A13",
                &[
                    "rust_test_workspace",
                    "a15_combo_and_leaves",
                    "r02_full_chain",
                    "r02_backup_restore",
                ],
            ),
            ("R03-A14", &["rust_test_workspace", "r02_recovery_drill"]),
            (
                "R03-A15",
                &[
                    "a15_combo_and_leaves",
                    "rust_test_workspace",
                    "rust_fmt",
                    "rust_clippy",
                    "check_contracts",
                    "check_boundaries",
                    "r02_auth_matrix",
                    "r02_legacy_regression",
                ],
            ),
            ("R03-A16", &["a16_seed_mechanism", "rust_test_workspace"]),
        ];
        for (id, refs) in expected {
            let scenario = map
                .scenarios
                .iter()
                .find(|s| s.id == *id)
                .unwrap_or_else(|| panic!("R03 map dropped original scenario {id}"));
            assert_eq!(scenario.requirement, "REQUIRED", "{id} re-graded");
            assert_eq!(
                &scenario
                    .command_refs
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                refs,
                "scenario {id} commandRefs drifted from the T08 registration"
            );
        }
    }

    #[test]
    fn r03_production_map_registers_the_repair_scenario_and_producer() {
        let map = parse_production_r03();
        let scenario = map
            .scenarios
            .iter()
            .find(|s| s.id == "R03-RP01")
            .unwrap_or_else(|| {
                panic!(
                    "R03 map dropped the G07/F08 repair scenario R03-RP01 — the F01-F07 \
                     adversarial-repair counterexamples would leave the formal acceptance"
                )
            });
        assert_eq!(scenario.requirement, "REQUIRED");
        assert_eq!(
            scenario.command_refs,
            vec!["repair_suites", "rust_test_workspace"]
        );
        let command = map
            .commands
            .iter()
            .find(|c| c.key == "repair_suites")
            .unwrap_or_else(|| panic!("R03 map dropped the repair_suites command"));
        assert_eq!(
            command.argv,
            vec![
                "bash",
                "scripts/rust-tauri/r03_g07_repair_suites.sh",
                "{EVIDENCE}/G07_REPAIR"
            ]
        );
        assert_eq!(
            command.evidence_paths,
            vec![
                "{EVIDENCE}/G07_REPAIR/repair-cases.json",
                "{EVIDENCE}/G07_REPAIR/summary.txt"
            ]
        );
    }

    #[test]
    fn r03_production_map_keeps_the_seven_directed_r02_chains() {
        let map = parse_production_r03();
        let chains = [
            "r02_auth_matrix",
            "r02_storage_tx",
            "r02_events_matrix",
            "r02_backup_restore",
            "r02_recovery_drill",
            "r02_full_chain",
            "r02_legacy_regression",
        ];
        for key in chains {
            assert!(
                map.commands.iter().any(|c| c.key == key),
                "R03 map dropped directed R02 chain command {key}"
            );
            // A chain command must stay EXECUTED: referenced by a scenario's
            // commandRefs or by a supplemental leaf's evidenceCommandRefs
            // (the leaf refs also pull commands into the run order).
            let referenced = map
                .scenarios
                .iter()
                .any(|s| s.command_refs.iter().any(|r| r == key))
                || map
                    .supplemental_leaves
                    .iter()
                    .any(|l| l.evidence_command_refs.iter().any(|r| r == key));
            assert!(
                referenced,
                "directed R02 chain command {key} is referenced by neither a scenario \
                 nor a supplemental leaf — it would never run"
            );
        }
    }

    #[test]
    fn r03_production_map_keeps_forty_eight_leaves_seventeen_share() {
        let map = parse_production_r03();
        assert_eq!(
            map.supplemental_leaves.len(),
            48,
            "the R03 map must keep exactly the 48 R00-bound supplemental leaves"
        );
        let share = map
            .supplemental_leaves
            .iter()
            .filter(|l| l.basis_kind == BASIS_STAGE_SHARE_SATISFIED)
            .count();
        let deferred = map
            .supplemental_leaves
            .iter()
            .filter(|l| l.basis_kind == BASIS_DEFERRED_TO_LATER_STAGE)
            .count();
        assert_eq!(share, 17, "17 stage_share_satisfied leaves required");
        assert_eq!(deferred, 31, "31 deferred_to_later_stage leaves required");
        assert!(
            map.supplemental_leaves
                .iter()
                .filter(|l| l.basis_kind == BASIS_STAGE_SHARE_SATISFIED)
                .all(|l| l.assertion_contract.is_some()),
            "every share leaf must keep its assertion contract (R14-F01)"
        );
    }

    #[test]
    fn r03_repair_producer_pin_table_matches_the_registered_suites() {
        // The producer's `pin <suite> <count> <F-ID>` table must mirror the
        // canonical G01–G06 registration EXACTLY: a dropped suite line, an
        // added line, or a lowered count is the fake-green hole this pins.
        use std::path::Path;
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("xtask lives at rust/crates/xtask");
        let script =
            std::fs::read_to_string(repo_root.join("scripts/rust-tauri/r03_g07_repair_suites.sh"))
                .expect("the registered repair_suites producer script must exist");
        let mut declared: Vec<(String, u32, String)> = Vec::new();
        for line in script.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("pin ") {
                let mut fields = rest.split_whitespace();
                match (fields.next(), fields.next(), fields.next(), fields.next()) {
                    (Some(suite), Some(count), Some(fid), None) => declared.push((
                        suite.to_string(),
                        count.parse().expect("pin count is an integer"),
                        fid.to_string(),
                    )),
                    _ => panic!("unparseable pin line in the producer script: {line:?}"),
                }
            }
        }
        let expected: Vec<(String, u32, String)> = R03_REPAIR_PIN_TABLE
            .iter()
            .map(|(s, c, f)| (s.to_string(), *c, f.to_string()))
            .collect();
        assert_eq!(
            declared, expected,
            "the repair_suites producer pin table drifted from the registered \
             F01-F07 suite/count mapping (missing suite, extra suite, or count drift)"
        );
    }
    // ── R04-T08: pin the PRODUCTION R04 map ─────────────────────────────
    //
    // Same reasoning as the R03 pinning block above: the gate's
    // cross-checks guard the R00 supplemental leaves and per-command
    // evidence, but the base scenario registration, the supplemental-duty
    // scenarios and the share-case inventory exist only in the map —
    // deleting any of them would otherwise leave a silent hole. These
    // tests pin the registered R04 map (they run inside `cargo test`,
    // itself the gate's `rust_test_workspace` command).

    /// The REAL registered R04 stage map (same bytes `verify-stage R04`
    /// loads via STAGE_MAPS).
    const R04_PRODUCTION: &str = include_str!("stage_maps/R04.json");

    /// The 56 case names the matrix producer records (the exact set the
    /// share leaves' assertion contracts may pin — a map pinning anything
    /// else, or the producer losing a case, is a named gap).
    const R04_MATRIX_CASES: &[&str] = &[
        "a15-directory-claim-refused",
        "a15-gateway-audit-directory-ref-failed",
        "a15-gateway-audit-fake-ref-failed",
        "a15-gateway-audit-real-ref-passes",
        "a15-missing-claim-refused",
        "a15-out-of-grant-claim-refused",
        "a15-real-delivery-registered",
        "a15-structure-violation-refused",
        "a16-alias-route-covered",
        "a16-history-preserved",
        "approval-answer-executes-once",
        "approval-duplicate-idempotent",
        "approval-reject-zero-dispatch",
        "catalog-face-lists-mcp-tools-with-permission",
        "future-tool-shape-ast_edit-discoverable-not-callable",
        "future-tool-shape-ast_grep-discoverable-not-callable",
        "future-tool-shape-file-discoverable-not-callable",
        "future-tool-shape-find-discoverable-not-callable",
        "future-tool-shape-grep-discoverable-not-callable",
        "future-tool-shape-ls-discoverable-not-callable",
        "future-tool-shape-lsp-discoverable-not-callable",
        "future-tool-shape-materialize-discoverable-not-callable",
        "future-tool-shape-run_code-discoverable-not-callable",
        "future-tool-shape-security_scan-discoverable-not-callable",
        "future-tool-shape-stage_files-discoverable-not-callable",
        "gateway-each-call-independent-permission",
        "matrix-lifecycle-disable-holes",
        "matrix-lifecycle-generation-refusals",
        "matrix-lifecycle-uninstall-holes",
        "matrix-permission-consistency",
        "matrix-route-consistency",
        "matrix-tool-family-count",
        "mcp-connector-catalog-sync",
        "mcp-connector-register-handshake",
        "mcp-describe-real-identity",
        "mcp-search-namespaced",
        "mcp-tool-call-full-chain",
        "mcp-tool-permission-face",
        "permission-face-modes-verifiable",
        "preauthorization-single-session-scoped",
        "semantics-cancel-leaves-no-fabricated-receipt",
        "semantics-failed-never-dispatched-receipt",
        "semantics-success-receipt-dispatched",
        "semantics-unknown-receipt-honest",
        "sup01-ask-subagent-write-refused",
        "terminal-close-stops-terminal",
        "terminal-snapshot-current-transcript",
        "terminal-tail-cursor-continuation",
        "tool-edit-conflict-preserves-user-version",
        "tool-edit-real-chain",
        "tool-exec-cancel-cleanup",
        "tool-exec-command-real-chain",
        "tool-read-real-chain",
        "tool-write-real-chain",
        "tool-write-stdin-continuation",
        "tool-write-stdin-foreign-writes",
    ];

    /// The subset of producer cases that evidence the BASE scenarios
    /// (R04-A15/A16, the SUP-01 refusal cell, the matrix cell counters and
    /// the T08 unified outcome semantics) rather than any one leaf's
    /// share — they still run every gate (the producer records them) and
    /// still must exist in R04_MATRIX_CASES, but no leaf owns them.
    const R04_SCENARIO_EVIDENCE_CASES: &[&str] = &[
        "a15-directory-claim-refused",
        "a15-gateway-audit-directory-ref-failed",
        "a15-gateway-audit-fake-ref-failed",
        "a15-gateway-audit-real-ref-passes",
        "a15-missing-claim-refused",
        "a15-out-of-grant-claim-refused",
        "a15-structure-violation-refused",
        "a16-alias-route-covered",
        "a16-history-preserved",
        "matrix-permission-consistency",
        "matrix-tool-family-count",
        "semantics-cancel-leaves-no-fabricated-receipt",
        "semantics-failed-never-dispatched-receipt",
        "semantics-success-receipt-dispatched",
        "semantics-unknown-receipt-honest",
        "sup01-ask-subagent-write-refused",
    ];

    /// The 124 R00 leaves bound to R04 split 46 full / 9 share / 69
    /// deferred. R05 RR3 F54 (M-01): the 46 R04-EXCLUSIVE leaves
    /// (r00ExecutionStageIds == ["R04"]) are full_original_behavior — a
    /// share with no later stage would leave an unowned remainder (the
    /// R05 RR1 F25 rule). The 9 share leaves are the DUAL-STAGE natives
    /// (read/write/edit) + six file-family shapes whose R06 remainder has
    /// a real later owner. The generator script
    /// `scripts/rust-tauri/r04_t08_generate_stage_map.py` holds the table;
    /// these numbers pin it — a re-classification that silently drops
    /// coverage turns this red until the map and the decision agree.
    const R04_LEAF_COUNTS: (usize, usize, usize) = (46, 9, 69);

    fn parse_production_r04() -> StageMap {
        parse_stage_map(R04_PRODUCTION)
            .expect("the registered R04 stage map must parse with this runner")
    }

    use std::path::Path;

    #[test]
    fn r04_production_map_keeps_the_sixteen_a_scenarios_verbatim() {
        let map = parse_production_r04();
        // The frozen T01..T08 registration: id → commandRefs.
        let expected: &[(&str, &[&str])] = &[
            ("R04-A01", &["rust_test_workspace", "r04_tool_matrix"]),
            ("R04-A02", &["rust_test_workspace", "r04_tool_matrix"]),
            ("R04-A03", &["rust_test_workspace", "r04_tool_matrix"]),
            ("R04-A04", &["rust_test_workspace"]),
            ("R04-A05", &["rust_test_workspace", "r04_tool_matrix"]),
            ("R04-A06", &["rust_test_workspace", "r04_tool_matrix"]),
            ("R04-A07", &["rust_test_workspace", "r04_tool_matrix"]),
            ("R04-A08", &["rust_test_workspace"]),
            ("R04-A09", &["rust_test_workspace", "r04_tool_matrix"]),
            ("R04-A10", &["rust_test_workspace", "r04_tool_matrix"]),
            ("R04-A11", &["rust_test_workspace"]),
            ("R04-A12", &["rust_test_workspace"]),
            ("R04-A13", &["rust_test_workspace", "r04_tool_matrix"]),
            ("R04-A14", &["rust_test_workspace", "r04_tool_matrix"]),
            // A15 additionally pins the STANDARD battery (the R03-A15
            // pattern): an unreferenced command would be silently skipped
            // by the runner, so fmt/clippy/contracts/boundaries must be
            // referenced here to actually run inside the gate.
            (
                "R04-A15",
                &[
                    "r04_tool_matrix",
                    "rust_test_workspace",
                    "rust_fmt",
                    "rust_clippy",
                    "check_contracts",
                    "check_boundaries",
                ],
            ),
            ("R04-A16", &["r04_tool_matrix", "rust_test_workspace"]),
        ];
        for (id, refs) in expected {
            let scenario = map
                .scenarios
                .iter()
                .find(|s| s.id == *id)
                .unwrap_or_else(|| panic!("R04 map dropped original scenario {id}"));
            assert_eq!(scenario.requirement, "REQUIRED", "{id} re-graded");
            assert_eq!(
                &scenario
                    .command_refs
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                refs,
                "scenario {id} commandRefs drifted from the T08 registration"
            );
        }
    }

    #[test]
    fn r04_production_map_registers_the_supplemental_duty_scenarios() {
        let map = parse_production_r04();
        for (id, refs) in [
            ("R04-SUP01", vec!["rust_test_workspace", "r04_tool_matrix"]),
            ("R04-SUP03", vec!["rust_test_workspace"]),
            (
                "R04-SUP05",
                vec!["r03_regression_gate", "rust_test_workspace"],
            ),
        ] {
            let scenario = map
                .scenarios
                .iter()
                .find(|s| s.id == id)
                .unwrap_or_else(|| panic!("R04 map dropped the supplemental-duty scenario {id}"));
            assert_eq!(scenario.requirement, "REQUIRED");
            assert_eq!(&scenario.command_refs, &refs, "{id} commandRefs drifted");
        }
    }

    #[test]
    fn r04_production_map_registers_the_matrix_and_regression_producers() {
        let map = parse_production_r04();
        let matrix = map
            .commands
            .iter()
            .find(|c| c.key == "r04_tool_matrix")
            .unwrap_or_else(|| panic!("R04 map dropped the r04_tool_matrix producer"));
        assert_eq!(
            matrix.argv,
            vec![
                "bash",
                "scripts/rust-tauri/r04_t08_matrix.sh",
                "{EVIDENCE}/R04_MATRIX"
            ]
        );
        assert!(matrix
            .evidence_paths
            .contains(&"{EVIDENCE}/R04_MATRIX/leaf-cases.json".to_string()));
        let regression = map
            .commands
            .iter()
            .find(|c| c.key == "r03_regression_gate")
            .unwrap_or_else(|| panic!("R04 map dropped the r03_regression_gate command"));
        assert!(regression.argv.contains(&"verify-stage".to_string()));
        assert!(regression.argv.contains(&"R03".to_string()));
        assert!(regression
            .evidence_paths
            .contains(&"{EVIDENCE}/R03_REGRESSION/verify-stage-result.json".to_string()));
        // The producer script must exist in the tree (a deleted producer is
        // a command that can only ever fail).
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("xtask lives at rust/crates/xtask");
        assert!(
            repo_root
                .join("scripts/rust-tauri/r04_t08_matrix.sh")
                .is_file(),
            "the registered r04_tool_matrix producer script must exist"
        );
    }

    #[test]
    fn r04_production_map_keeps_the_124_leaf_split() {
        let map = parse_production_r04();
        assert_eq!(
            map.supplemental_leaves.len(),
            124,
            "the R04 map must keep exactly the 124 R00-bound supplemental leaves"
        );
        let full = map
            .supplemental_leaves
            .iter()
            .filter(|l| l.basis_kind == BASIS_FULL_ORIGINAL_BEHAVIOR)
            .count();
        let share = map
            .supplemental_leaves
            .iter()
            .filter(|l| l.basis_kind == BASIS_STAGE_SHARE_SATISFIED)
            .count();
        let deferred = map
            .supplemental_leaves
            .iter()
            .filter(|l| l.basis_kind == BASIS_DEFERRED_TO_LATER_STAGE)
            .count();
        assert_eq!(
            (full, share, deferred),
            R04_LEAF_COUNTS,
            "the full/share/deferred split drifted from the registered decision"
        );
        // R05 RR3 F54: classification follows stage EXCLUSIVITY (the R05
        // F25 rule mirrored onto the R04 map). An R04-EXCLUSIVE leaf must
        // be full_original_behavior with one case group per original R00
        // assertion; a share leaf must have a real later stage to own the
        // remainder.
        for leaf in &map.supplemental_leaves {
            let exclusive =
                leaf.r00_execution_stage_ids.len() == 1 && leaf.r00_execution_stage_ids[0] == "R04";
            if exclusive {
                assert_eq!(
                    leaf.basis_kind, BASIS_FULL_ORIGINAL_BEHAVIOR,
                    "leaf {} is EXCLUSIVE to R04 — a stage_share_satisfied classification \
                     would leave an unowned remainder (F54)",
                    leaf.id
                );
                assert_eq!(
                    leaf.original_assertion_cases.len(),
                    leaf.r00_assertions.len(),
                    "leaf {} must pin at least one case per original R00 assertion",
                    leaf.id
                );
                for group in &leaf.original_assertion_cases {
                    assert!(
                        !group.is_empty(),
                        "leaf {} pinned an original assertion with no case",
                        leaf.id
                    );
                }
            } else if leaf.basis_kind == BASIS_STAGE_SHARE_SATISFIED {
                assert!(
                    leaf.r00_execution_stage_ids
                        .iter()
                        .any(|id| id.as_str() != "R04"),
                    "leaf {} is classified as a share but no later stage exists to own \
                     the remainder (F54)",
                    leaf.id
                );
            }
        }
        assert!(
            map.supplemental_leaves
                .iter()
                .filter(|l| l.basis_kind == BASIS_STAGE_SHARE_SATISFIED)
                .all(|l| l.assertion_contract.is_some()),
            "every R04 share leaf must keep its assertion contract (R14-F01)"
        );
        assert!(
            map.supplemental_leaves
                .iter()
                .filter(|l| l.basis_kind == BASIS_FULL_ORIGINAL_BEHAVIOR)
                .all(|l| l.assertion_contract.is_some() && !l.original_assertion_cases.is_empty()),
            "every R04 full leaf must keep its assertion contract and per-assertion \
             case assignment (R14-F01 / F54)"
        );
        // Every share leaf pins only REAL producer cases, and every known
        // case is pinned by at least one leaf (an unpinned case is dead
        // evidence; an unknown pin can never hold).
        let mut pinned: Vec<&str> = Vec::new();
        for leaf in &map.supplemental_leaves {
            if let Some(contract) = leaf.assertion_contract.as_ref() {
                for case in &contract.cases {
                    assert!(
                        R04_MATRIX_CASES.contains(&case.case.as_str()),
                        "leaf {} pins unknown matrix case {:?}",
                        leaf.id,
                        case.case
                    );
                    pinned.push(case.case.as_str());
                }
            }
        }
        for known in R04_MATRIX_CASES {
            if R04_SCENARIO_EVIDENCE_CASES.contains(known) {
                // Base-scenario evidence (A15/A16, the SUP-01 cell, the
                // matrix cell counters and the unified outcome semantics):
                // consumed by the scenario roll-up on command outcomes, not
                // by any single leaf's contract.
                continue;
            }
            assert!(
                pinned.contains(known),
                "matrix case {known} is recorded by the producer but pinned by NO leaf — \
                 it must be bound to a share contract or removed from the producer"
            );
        }
    }

    /// The EXPECTATION each producer case records (mirrored from the
    /// producer's `record_case` calls in
    /// lingxi-service/tests/r04_t08_tool_matrix.rs — the future-tool-shape
    /// cases are emitted through a `format!` there, all with expect 1).
    /// The generator's share table once pinned `matrix-lifecycle-
    /// disable-holes` as 1 while the producer (and the zero-holes
    /// semantics it encodes) records 0 — the full gate caught it, but a
    /// name-only pin cannot; this table pins the VALUES so an expect typo
    /// fails `cargo test` instead of a whole gate run.
    fn r04_case_expect(case: &str) -> Option<i64> {
        if case.starts_with("future-tool-shape-") {
            return Some(1);
        }
        const EXPECTS: &[(&str, i64)] = &[
            ("a15-directory-claim-refused", 1),
            ("a15-gateway-audit-directory-ref-failed", 1),
            ("a15-gateway-audit-fake-ref-failed", 1),
            ("a15-gateway-audit-real-ref-passes", 1),
            ("a15-missing-claim-refused", 1),
            ("a15-out-of-grant-claim-refused", 1),
            ("a15-real-delivery-registered", 1),
            ("a15-structure-violation-refused", 1),
            ("a16-alias-route-covered", 1),
            ("a16-history-preserved", 1),
            ("approval-answer-executes-once", 1),
            ("approval-duplicate-idempotent", 1),
            ("approval-reject-zero-dispatch", 0),
            ("catalog-face-lists-mcp-tools-with-permission", 1),
            ("gateway-each-call-independent-permission", 1),
            ("matrix-lifecycle-disable-holes", 0),
            ("matrix-lifecycle-generation-refusals", 0),
            ("matrix-lifecycle-uninstall-holes", 0),
            ("matrix-permission-consistency", 1),
            ("matrix-route-consistency", 1),
            ("matrix-tool-family-count", 7),
            ("mcp-connector-catalog-sync", 1),
            ("mcp-connector-register-handshake", 1),
            ("mcp-describe-real-identity", 1),
            ("mcp-search-namespaced", 1),
            ("mcp-tool-call-full-chain", 1),
            ("mcp-tool-permission-face", 1),
            ("permission-face-modes-verifiable", 1),
            ("preauthorization-single-session-scoped", 1),
            ("semantics-cancel-leaves-no-fabricated-receipt", 1),
            ("semantics-failed-never-dispatched-receipt", 1),
            ("semantics-success-receipt-dispatched", 1),
            ("semantics-unknown-receipt-honest", 1),
            ("sup01-ask-subagent-write-refused", 1),
            ("terminal-close-stops-terminal", 1),
            ("terminal-snapshot-current-transcript", 1),
            ("terminal-tail-cursor-continuation", 1),
            ("tool-edit-conflict-preserves-user-version", 1),
            ("tool-edit-real-chain", 1),
            ("tool-exec-cancel-cleanup", 1),
            ("tool-exec-command-real-chain", 1),
            ("tool-read-real-chain", 1),
            ("tool-write-real-chain", 1),
            ("tool-write-stdin-continuation", 1),
            ("tool-write-stdin-foreign-writes", 0),
        ];
        EXPECTS
            .iter()
            .find(|(name, _)| *name == case)
            .map(|(_, expect)| *expect)
    }

    #[test]
    fn r04_production_map_pins_the_real_case_expectations() {
        let map = parse_production_r04();
        // Every known producer case must have a mirrored expectation here
        // (a missing mirror is itself a gap — it means this table drifted
        // from the producer), and every PIN in the map must equal it.
        for known in R04_MATRIX_CASES {
            let expect = r04_case_expect(known).unwrap_or_else(|| {
                panic!("case {known} has no mirrored expectation in r04_case_expect")
            });
            for leaf in &map.supplemental_leaves {
                if let Some(contract) = leaf.assertion_contract.as_ref() {
                    for case in &contract.cases {
                        if case.case == *known {
                            assert_eq!(
                                case.expect, expect,
                                "leaf {} pins case {} with the wrong expectation (the producer \
                                 records {expect})",
                                leaf.id, case.case
                            );
                        }
                    }
                }
            }
        }
    }

    // ── R04 RR1 repair round G05 (CLOSE-C01, 2026-10-01): pin the RR1
    // registration ─────────────────────────────────────────────────────────
    //
    // The five adversarial-repair findings F01–F05 (repair candidates
    // G01..G04 = 1692d2314/da15c4bd9/614fab1af/1285c3bf6) entered the
    // formal R04 gate as first-class REQUIRED scenarios with their own
    // producer (`r04_rr1_repair_suites` →
    // scripts/rust-tauri/r04_rr1_g05_repair_suites.sh). The scenario
    // registration, the producer's run pin table and its per-C-ID test
    // ownership table exist only in the map + the script — deleting any
    // of them, lowering a pinned count, or dropping a C-ID would
    // otherwise leave a silent hole the gate cannot see. These tests pin
    // all three; they run inside `cargo test` (itself the gate's
    // `rust_test_workspace` command), so any deletion/drift turns the
    // stage gate red with the gap named here.

    /// The RR1 repair runs and their EXACT pinned executed-test counts, as
    /// the producer script's `pin <run> <count> <F-ID>` table must declare
    /// them (no missing run, no extra run, no count drift). A `lib/`-
    /// prefixed run is one exact lib unit test; a bare run is one
    /// integration suite. 6 integration suites (37 tests) + 26 lib unit
    /// tests = 63 tests (the F04 pure-logic family and the G04 exectools
    /// output-integrity family are unit tests by design — the G04 report's
    /// C-ID evidence mapping is mirrored verbatim).
    const R04_RR1_PIN_TABLE: &[(&str, u32, &str)] = &[
        ("r04_t05_registry_capacity", 9, "RR1-F01"),
        ("r04_rr1_f02_reaper_cleanup", 11, "RR1-F02"),
        ("r04_rr1_f03_stop_honesty", 7, "RR1-F03"),
        ("r04_rr1_f04_pty_consumption", 1, "RR1-F04"),
        ("r04_rr1_f05_output_integrity", 7, "RR1-F05"),
        ("r04_rr1_f05_spill_failure", 2, "RR1-F05"),
        (
            "lib/procsupervisor::tests::live_slot_reservation_enforces_the_cap_atomically",
            1,
            "RR1-F01",
        ),
        (
            "lib/procsupervisor::tests::live_slot_commit_transfers_release_to_settle_exactly_once",
            1,
            "RR1-F01",
        ),
        (
            "lib/procsupervisor::tests::live_slot_drop_after_panic_style_abandon_still_releases",
            1,
            "RR1-F01",
        ),
        (
            "lib/procsupervisor::tests::group_signal_is_skipped_once_the_child_reap_is_published",
            1,
            "RR1-F02",
        ),
        (
            "lib/procsupervisor::tests::group_signal_is_skipped_when_the_kernel_identity_disagrees",
            1,
            "RR1-F02",
        ),
        (
            "lib/procsupervisor::tests::exit_fact_status_codes_follow_the_shell_convention",
            1,
            "RR1-F03",
        ),
        (
            "lib/procsupervisor::tests::terminal_facts_never_fabricate_an_exit_for_unconfirmed_or_live_phases",
            1,
            "RR1-F03",
        ),
        (
            "lib/procsupervisor::tests::transcript_delivers_split_multibyte_characters_intact",
            1,
            "RR1-F04",
        ),
        (
            "lib/procsupervisor::tests::transcript_mixed_chunk_prefix_is_consumed_byte_exactly_r04_rr1_f04_c01",
            1,
            "RR1-F04",
        ),
        (
            "lib/procsupervisor::tests::transcript_idle_polls_after_a_partial_delivery_hold_back_without_loss",
            1,
            "RR1-F04",
        ),
        (
            "lib/procsupervisor::tests::transcript_property_valid_input_reassembles_exactly",
            1,
            "RR1-F04",
        ),
        (
            "lib/procsupervisor::tests::transcript_property_invalid_bytes_replaced_exactly_once",
            1,
            "RR1-F04",
        ),
        (
            "lib/procsupervisor::tests::transcript_property_eviction_accounting_counts_only_real_loss",
            1,
            "RR1-F04",
        ),
        (
            "lib/procsupervisor::tests::transcript_ring_overflow_while_holding_back_counts_only_real_evictions",
            1,
            "RR1-F04",
        ),
        (
            "lib/procsupervisor::tests::transcript_boundary_across_many_chunks_with_interleaved_polls",
            1,
            "RR1-F04",
        ),
        (
            "lib/procsupervisor::tests::transcript_force_delivery_flushes_a_dangling_partial",
            1,
            "RR1-F04",
        ),
        (
            "lib/procsupervisor::tests::transcript_ring_drop_of_undelivered_is_counted_honestly",
            1,
            "RR1-F04",
        ),
        (
            "lib/exectools::tests::transcript_spill_claim_vocabulary_is_state_exclusive",
            1,
            "RR1-F05",
        ),
        (
            "lib/exectools::tests::full_output_claim_vocabulary_is_state_exclusive",
            1,
            "RR1-F05",
        ),
        (
            "lib/exectools::tests::spill_resource_ref_never_claims_full_when_capped_or_failed",
            1,
            "RR1-F05",
        ),
        (
            "lib/exectools::tests::assemble_retained_output_small_stream_is_whole_and_exact",
            1,
            "RR1-F05",
        ),
        (
            "lib/exectools::tests::assemble_retained_output_evicted_middle_is_counted_and_marked",
            1,
            "RR1-F05",
        ),
        (
            "lib/exectools::tests::truncate_head_tail_single_huge_ascii_line_keeps_head_and_tail",
            1,
            "RR1-F05",
        ),
        (
            "lib/exectools::tests::truncate_head_tail_multibyte_line_cuts_on_char_boundaries",
            1,
            "RR1-F05",
        ),
        (
            "lib/exectools::tests::truncate_head_tail_newline_only_at_end_keeps_both_markers",
            1,
            "RR1-F05",
        ),
        (
            "lib/exectools::tests::truncate_head_tail_small_output_stays_whole",
            1,
            "RR1-F05",
        ),
    ];

    /// The 22 five-F acceptance C-IDs of the RR1 checklist and the EXACT
    /// number of executed tests the producer's `cid` table must own for
    /// each (the checklist's other four C-IDs, CLOSE-C01..C04, are
    /// closeout checks executed by the orchestrator — never stage-map
    /// scenarios). A dropped/added/renamed C-ID line, or a case losing or
    /// gaining a test, turns this red.
    const R04_RR1_CASE_TABLE: &[(&str, usize)] = &[
        ("R04-RR1-F01-C01", 3),
        ("R04-RR1-F01-C02", 3),
        ("R04-RR1-F01-C03", 5),
        ("R04-RR1-F01-C04", 1),
        ("R04-RR1-F02-C01", 4),
        ("R04-RR1-F02-C02", 5),
        ("R04-RR1-F02-C03", 3),
        ("R04-RR1-F02-C04", 1),
        ("R04-RR1-F03-C01", 2),
        ("R04-RR1-F03-C02", 1),
        ("R04-RR1-F03-C03", 1),
        ("R04-RR1-F03-C04", 3),
        ("R04-RR1-F03-C05", 2),
        ("R04-RR1-F04-C01", 2),
        ("R04-RR1-F04-C02", 1),
        ("R04-RR1-F04-C03", 7),
        ("R04-RR1-F04-C04", 1),
        ("R04-RR1-F05-C01", 5),
        ("R04-RR1-F05-C02", 5),
        ("R04-RR1-F05-C03", 4),
        ("R04-RR1-F05-C04", 2),
        ("R04-RR1-F05-C05", 2),
    ];

    fn read_rr1_producer_script() -> String {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("xtask lives at rust/crates/xtask");
        std::fs::read_to_string(repo_root.join("scripts/rust-tauri/r04_rr1_g05_repair_suites.sh"))
            .expect("the registered r04_rr1_repair_suites producer script must exist")
    }

    fn parse_rr1_pin_lines(script: &str) -> Vec<(String, u32, String)> {
        let mut declared = Vec::new();
        for line in script.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("pin ") {
                let mut fields = rest.split_whitespace();
                match (fields.next(), fields.next(), fields.next(), fields.next()) {
                    (Some(run), Some(count), Some(fid), None) => declared.push((
                        run.to_string(),
                        count.parse().expect("pin count is an integer"),
                        fid.to_string(),
                    )),
                    _ => panic!("unparseable pin line in the RR1 producer script: {line:?}"),
                }
            }
        }
        declared
    }

    #[test]
    fn r04_production_map_registers_the_rr1_repair_scenarios() {
        let map = parse_production_r04();
        // Additive-only: the frozen T01..T08 registration (16 A-IDs +
        // SUP01/03/05) plus exactly the five RR1-F scenarios — nothing may
        // be dropped or re-graded. The per-id membership check runs FIRST
        // so a deleted scenario is named, then the total pins the
        // additive-only count (an invented extra scenario is equally red).
        for id in [
            "R04-RR1-F01",
            "R04-RR1-F02",
            "R04-RR1-F03",
            "R04-RR1-F04",
            "R04-RR1-F05",
        ] {
            let scenario = map
                .scenarios
                .iter()
                .find(|s| s.id == id)
                .unwrap_or_else(|| {
                    panic!(
                        "R04 map dropped the RR1 repair scenario {id} — the F01-F05 \
                         adversarial-repair counterexamples would leave the formal acceptance"
                    )
                });
            assert_eq!(scenario.requirement, "REQUIRED", "{id} re-graded");
            assert_eq!(
                scenario.command_refs,
                vec!["r04_rr1_repair_suites", "rust_test_workspace"],
                "scenario {id} commandRefs drifted from the G05 registration"
            );
        }
        assert_eq!(
            map.scenarios.len(),
            24,
            "the R04 map must keep exactly the 16 A-IDs + SUP01/03/05 + the five \
             R04-RR1-F01..F05 repair scenarios (additive-only — an invented extra \
             scenario is as red as a dropped one)"
        );
        let command = map
            .commands
            .iter()
            .find(|c| c.key == "r04_rr1_repair_suites")
            .unwrap_or_else(|| panic!("R04 map dropped the r04_rr1_repair_suites producer"));
        assert_eq!(
            command.argv,
            vec![
                "bash",
                "scripts/rust-tauri/r04_rr1_g05_repair_suites.sh",
                "{EVIDENCE}/R04_RR1_REPAIR"
            ]
        );
        assert!(command
            .evidence_paths
            .contains(&"{EVIDENCE}/R04_RR1_REPAIR/rr1-cases.json".to_string()));
        assert!(command
            .evidence_paths
            .contains(&"{EVIDENCE}/R04_RR1_REPAIR/summary.txt".to_string()));
        // The producer script must exist in the tree (a deleted producer is
        // a command that can only ever fail).
        let _ = read_rr1_producer_script();
    }

    #[test]
    fn r04_rr1_producer_pin_table_matches_the_registered_suites() {
        // The producer's `pin <run> <count> <F-ID>` table must mirror the
        // G05 registration EXACTLY: a dropped run line, an added line, or a
        // lowered count is the fake-green hole this pins.
        let declared = parse_rr1_pin_lines(&read_rr1_producer_script());
        let expected: Vec<(String, u32, String)> = R04_RR1_PIN_TABLE
            .iter()
            .map(|(run, count, fid)| (run.to_string(), *count, fid.to_string()))
            .collect();
        assert_eq!(
            declared, expected,
            "the RR1 repair_suites producer pin table drifted from the registered \
             run/count/F-ID mapping (missing run, extra run, or count drift)"
        );
    }

    #[test]
    fn r04_rr1_producer_case_table_owns_every_check_id_exactly() {
        let script = read_rr1_producer_script();
        let pins = parse_rr1_pin_lines(&script);
        // Parse the `cid <C-ID> <run> <name1>+<name2>...` ownership lines.
        let mut declared: Vec<(String, String, Vec<String>)> = Vec::new();
        for line in script.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("cid ") {
                let mut fields = rest.split_whitespace();
                match (fields.next(), fields.next(), fields.next(), fields.next()) {
                    (Some(cid), Some(run), Some(names), None) => {
                        let names: Vec<String> = names.split('+').map(str::to_string).collect();
                        assert!(
                            !names.is_empty(),
                            "cid line {line:?} owns no test name — a case with no \
                             machine-checked test is the exit-0-only fake-green path"
                        );
                        declared.push((cid.to_string(), run.to_string(), names));
                    }
                    _ => panic!("unparseable cid line in the RR1 producer script: {line:?}"),
                }
            }
        }
        // (a) The owned C-ID set and the per-case test-name counts must
        //     equal the registered checklist table exactly (漏 ID fails
        //     closed; so does an invented one).
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        for (cid, _run, names) in &declared {
            *counts.entry(cid.clone()).or_insert(0) += names.len();
        }
        let actual: Vec<(String, usize)> = counts.into_iter().collect();
        let expected: Vec<(String, usize)> = R04_RR1_CASE_TABLE
            .iter()
            .map(|(cid, count)| (cid.to_string(), *count))
            .collect();
        assert_eq!(
            actual, expected,
            "the RR1 producer cid table drifted from the 22 five-F acceptance C-IDs \
             (dropped, added, or re-counted case)"
        );
        // (b) Every cid line must reference a REGISTERED pin-table run whose
        //     F-ID matches the case id — a case evidenced by a run that
        //     never happens (or belongs to another finding) is a hole.
        for (cid, run, _) in &declared {
            let pin = pins
                .iter()
                .find(|(pinned_run, _, _)| pinned_run == run)
                .unwrap_or_else(|| {
                    panic!("cid {cid} references run {run:?} which the producer pin table does not register")
                });
            assert!(
                cid.starts_with(&format!("R04-{}-", pin.2)),
                "cid {cid} is evidenced by run {run:?} pinned for {FID}",
                FID = pin.2
            );
        }
        // (c) No test name may be owned by two C-IDs.
        let mut seen: Vec<(&str, &str)> = Vec::new();
        for (cid, run, names) in &declared {
            for name in names {
                let key = (run.as_str(), name.as_str());
                assert!(
                    !seen.contains(&key),
                    "test {name:?} in run {run:?} is claimed by two C-IDs (last {cid})"
                );
                seen.push(key);
            }
        }
        // (d) The union of owned names must equal the total pinned executed
        //     tests — every executed test is owned by exactly one C-ID, so
        //     a run that executes tests no case owns cannot pass silently.
        let total_names: usize = declared.iter().map(|(_, _, names)| names.len()).sum();
        let total_pinned: u32 = pins.iter().map(|(_, count, _)| *count).sum();
        assert_eq!(
            total_names as u32, total_pinned,
            "the cid table owns {total_names} test names but the pin table executes \
             {total_pinned} tests — orphan executions or phantom ownership"
        );
    }

    // ── R05 (registered 2026-10-03 by R05-T08) ─────────────────────────────
    const R05_PRODUCTION: &str = include_str!("stage_maps/R05.json");

    fn parse_production_r05() -> StageMap {
        parse_stage_map(R05_PRODUCTION)
            .expect("the registered R05 stage map must parse with this runner")
    }

    fn r05_repo_root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("xtask lives at rust/crates/xtask")
    }

    /// The pinned suite table of the R05 producer (pin <run> <count> <tag>),
    /// read from the SINGLE source of truth TSV in docs/.
    fn parse_r05_pin_table() -> Vec<(String, u32, String)> {
        let text = std::fs::read_to_string(
            r05_repo_root().join("docs/rust-tauri/R05/r05_stage_pins.tsv"),
        )
        .expect("docs/rust-tauri/R05/r05_stage_pins.tsv must exist (the R05 producer's pin table)");
        let mut pins = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("pin ") {
                let mut fields = rest.split_whitespace();
                match (fields.next(), fields.next(), fields.next(), fields.next()) {
                    (Some(run), Some(count), Some(tag), None) => pins.push((
                        run.to_string(),
                        count.parse::<u32>().expect("numeric pin count"),
                        tag.to_string(),
                    )),
                    _ => panic!("unparseable pin line in the R05 pin TSV: {line:?}"),
                }
            }
        }
        pins
    }

    /// The C-ID ownership table of the R05 producer
    /// (cid <C-ID> <run> <name1>+<name2>...), read from the TSV.
    fn parse_r05_cid_table() -> Vec<(String, String, Vec<String>)> {
        let text = std::fs::read_to_string(
            r05_repo_root().join("docs/rust-tauri/R05/r05_stage_cids.tsv"),
        )
        .expect("docs/rust-tauri/R05/r05_stage_cids.tsv must exist (the R05 producer's cid table)");
        let mut declared = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("cid ") {
                let mut fields = rest.split_whitespace();
                match (fields.next(), fields.next(), fields.next(), fields.next()) {
                    (Some(cid), Some(run), Some(names), None) => {
                        let names: Vec<String> = names.split('+').map(str::to_string).collect();
                        assert!(
                            !names.is_empty(),
                            "cid line {line:?} owns no test name — a case with no \
                             machine-checked test is the exit-0-only fake-green path"
                        );
                        declared.push((cid.to_string(), run.to_string(), names));
                    }
                    _ => panic!("unparseable cid line in the R05 cid TSV: {line:?}"),
                }
            }
        }
        declared
    }

    #[test]
    fn r05_production_map_keeps_the_sixteen_a_scenarios_verbatim() {
        let map = parse_production_r05();
        // The frozen Appendix-A registration: id → commandRefs. The A15
        // pattern pins the STANDARD battery; A16 additionally pins the R04
        // regression gate (关键失败可复原 consumes the recovery chains).
        let expected: &[(&str, &[&str])] = &[
            ("R05-A01", &["rust_test_workspace", "r05_stage_suites"]),
            ("R05-A02", &["rust_test_workspace", "r05_stage_suites"]),
            ("R05-A03", &["rust_test_workspace", "r05_stage_suites"]),
            ("R05-A04", &["rust_test_workspace", "r05_stage_suites"]),
            ("R05-A05", &["rust_test_workspace", "r05_stage_suites"]),
            ("R05-A06", &["rust_test_workspace", "r05_stage_suites"]),
            ("R05-A07", &["rust_test_workspace", "r05_stage_suites"]),
            ("R05-A08", &["rust_test_workspace", "r05_stage_suites"]),
            ("R05-A09", &["rust_test_workspace", "r05_stage_suites"]),
            ("R05-A10", &["rust_test_workspace", "r05_stage_suites"]),
            ("R05-A11", &["rust_test_workspace", "r05_stage_suites"]),
            ("R05-A12", &["rust_test_workspace", "r05_stage_suites"]),
            ("R05-A13", &["rust_test_workspace", "r05_stage_suites"]),
            ("R05-A14", &["rust_test_workspace", "r05_stage_suites"]),
            (
                "R05-A15",
                &[
                    "r05_stage_suites",
                    "rust_test_workspace",
                    "rust_fmt",
                    "rust_clippy",
                    "check_contracts",
                    "check_boundaries",
                ],
            ),
            (
                "R05-A16",
                &[
                    "r05_stage_suites",
                    "rust_test_workspace",
                    "r04_regression_gate",
                ],
            ),
        ];
        for (id, refs) in expected {
            let scenario = map
                .scenarios
                .iter()
                .find(|s| s.id == *id)
                .unwrap_or_else(|| panic!("R05 map dropped original scenario {id}"));
            assert_eq!(scenario.requirement, "REQUIRED", "{id} re-graded");
            assert_eq!(
                &scenario
                    .command_refs
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                refs,
                "scenario {id} commandRefs drifted from the T08 registration"
            );
        }
        // No scenario beyond the 16 A-IDs + the two supplemental duty ids —
        // an invented scenario that greens nothing must not register.
        let known: Vec<&str> = map.scenarios.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            known.len(),
            18,
            "the R05 map must carry exactly 16 A-IDs + 2 supplemental duties, got {known:?}"
        );
    }

    #[test]
    fn r05_production_map_registers_the_supplemental_duty_scenarios() {
        let map = parse_production_r05();
        for (id, refs) in [
            (
                "R05-SUP-R04REG",
                vec!["r04_regression_gate", "rust_test_workspace"],
            ),
            (
                "R05-SUP-SCOPE",
                vec!["check_boundaries", "rust_test_workspace"],
            ),
        ] {
            let scenario = map
                .scenarios
                .iter()
                .find(|s| s.id == id)
                .unwrap_or_else(|| panic!("R05 map dropped the supplemental-duty scenario {id}"));
            assert_eq!(scenario.requirement, "REQUIRED");
            assert_eq!(&scenario.command_refs, &refs, "{id} commandRefs drifted");
        }
    }

    #[test]
    fn r05_production_map_registers_the_suite_producer_and_r04_regression_gate() {
        let map = parse_production_r05();
        let producer = map
            .commands
            .iter()
            .find(|c| c.key == "r05_stage_suites")
            .unwrap_or_else(|| panic!("R05 map dropped the r05_stage_suites producer"));
        assert_eq!(
            producer.argv,
            vec![
                "bash",
                "scripts/rust-tauri/r05_t08_stage_suites.sh",
                "{EVIDENCE}/R05_SUITES"
            ]
        );
        assert!(producer
            .evidence_paths
            .contains(&"{EVIDENCE}/R05_SUITES/r05-cases.json".to_string()));
        let regression = map
            .commands
            .iter()
            .find(|c| c.key == "r04_regression_gate")
            .unwrap_or_else(|| panic!("R05 map dropped the r04_regression_gate command"));
        assert!(regression.argv.contains(&"verify-stage".to_string()));
        assert!(regression.argv.contains(&"R04".to_string()));
        assert!(regression
            .evidence_paths
            .contains(&"{EVIDENCE}/R04_REGRESSION/verify-stage-result.json".to_string()));
        // The producer script and both TSVs must exist in the tree (a
        // deleted producer or table is a command that can only ever fail).
        let root = r05_repo_root();
        assert!(
            root.join("scripts/rust-tauri/r05_t08_stage_suites.sh")
                .is_file(),
            "the registered r05_stage_suites producer script must exist"
        );
        assert!(root
            .join("docs/rust-tauri/R05/r05_stage_pins.tsv")
            .is_file());
        assert!(root
            .join("docs/rust-tauri/R05/r05_stage_cids.tsv")
            .is_file());
    }

    #[test]
    fn r05_production_map_mirrors_every_r00_supplemental_leaf_bound_to_r05() {
        let map = parse_production_r05();
        // The R00 ACCEPTANCE ledger (the runner's binding source) binds 130
        // REQUIRED_SUPPLEMENTAL leaves to R05; the map must mirror the set
        // EXACTLY (the runner cross-checks for equality before commands).
        let ledger = std::fs::read_to_string(
            r05_repo_root().join("docs/rust-tauri/R00/ACCEPTANCE_MAP.json"),
        )
        .expect("the R00 ACCEPTANCE_MAP ledger must be readable");
        let value: serde_json::Value =
            serde_json::from_str(&ledger).expect("the R00 ledger is valid JSON");
        let bound: std::collections::BTreeSet<String> = value["scenarios"]
            .as_object()
            .expect("scenarios object")
            .iter()
            .filter(|(_, entry)| {
                entry["execution_stage_ids"]
                    .as_array()
                    .is_some_and(|stages| stages.iter().any(|s| s == "R05"))
            })
            .map(|(id, _)| id.clone())
            .collect();
        assert_eq!(
            bound.len(),
            130,
            "the R00 ledger's R05-bound leaf count drifted (was 130 at registration)"
        );
        let declared: std::collections::BTreeSet<&str> = map
            .supplemental_leaves
            .iter()
            .map(|l| l.id.as_str())
            .collect();
        let missing: Vec<&str> = bound
            .iter()
            .map(String::as_str)
            .filter(|id| !declared.contains(id))
            .collect();
        assert!(
            missing.is_empty(),
            "the R05 map dropped R00-bound supplemental leaves {missing:?} (the \
             runner refuses the map before any command)"
        );
        assert_eq!(
            declared.len(),
            bound.len(),
            "the R05 map invents supplemental leaves unknown to the R00 ledger"
        );
        // R05 RR1 F25 (G01): classification follows stage EXCLUSIVITY. A leaf
        // whose R00 execution_stage_ids contain ONLY R05 is EXCLUSIVE — it
        // must be full_original_behavior with one case PER original
        // assertion (never a share: that would leave an unowned remainder).
        // Every other leaf is stage_share_satisfied with a non-empty later
        // stage set and a per-leaf case pin against the registered
        // producer.
        let mut share_leaves = 0usize;
        let mut full_leaves = 0usize;
        for leaf in &map.supplemental_leaves {
            let exclusive =
                leaf.r00_execution_stage_ids.len() == 1 && leaf.r00_execution_stage_ids[0] == "R05";
            let contract = leaf
                .assertion_contract
                .as_ref()
                .unwrap_or_else(|| panic!("leaf {} lost its assertion contract", leaf.id));
            assert_eq!(contract.producer_command, "r05_stage_suites");
            assert_eq!(
                contract.evidence_path,
                "{EVIDENCE}/R05_SUITES/leaf-cases.json"
            );
            assert!(
                leaf.evidence_command_refs
                    .contains(&"r05_stage_suites".to_string()),
                "leaf {} must reference its producer",
                leaf.id
            );
            if exclusive {
                assert_eq!(
                    leaf.basis_kind,
                    crate::stage_map::BASIS_FULL_ORIGINAL_BEHAVIOR,
                    "leaf {} is EXCLUSIVE to R05 — a stage_share_satisfied classification \
                     would leave an unowned remainder (F25)",
                    leaf.id
                );
                full_leaves += 1;
                assert_eq!(
                    leaf.original_assertion_cases.len(),
                    leaf.r00_assertions.len(),
                    "leaf {} must pin one case per original assertion",
                    leaf.id
                );
                for group in &leaf.original_assertion_cases {
                    assert_eq!(
                        group.len(),
                        1,
                        "one named case per assertion (leaf {})",
                        leaf.id
                    );
                }
            } else {
                assert_eq!(
                    leaf.basis_kind,
                    crate::stage_map::BASIS_STAGE_SHARE_SATISFIED,
                    "leaf {} must classify its R05 share explicitly",
                    leaf.id
                );
                assert!(
                    leaf.r00_execution_stage_ids
                        .iter()
                        .any(|id| id.as_str() != "R05"),
                    "leaf {} is classified as a share but no later stage exists to own \
                     the remainder (F25)",
                    leaf.id
                );
                share_leaves += 1;
            }
        }
        assert_eq!(full_leaves, 6, "the six R05-only OAuth leaves must be full");
        assert_eq!(share_leaves, 124);
        // The leaf-case mapping TSV: 124 suite-level lines + 13 TEST-LEVEL
        // lines (the six exclusive leaves' per-assertion cases), and every
        // case the map pins must appear exactly once.
        let tsv = std::fs::read_to_string(
            r05_repo_root().join("docs/rust-tauri/R05/r05_leaf_case_map.tsv"),
        )
        .expect("docs/rust-tauri/R05/r05_leaf_case_map.tsv must exist");
        let mut case_names: Vec<&str> = Vec::new();
        let mut test_level = 0usize;
        for line in tsv.lines().filter(|l| l.starts_with("leafcase ")) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            assert!(
                fields.len() == 3 || fields.len() == 4,
                "leafcase lines are suite-level (3 fields) or test-level (4 fields): {line}"
            );
            if fields.len() == 4 {
                test_level += 1;
            }
            case_names.push(fields[1]);
        }
        assert_eq!(case_names.len(), 137, "the leaf-case map drifted from 137");
        assert_eq!(
            test_level, 13,
            "the six exclusive leaves pin 13 test-level cases"
        );
        case_names.sort_unstable();
        case_names.dedup();
        assert_eq!(case_names.len(), 137, "duplicate leaf case names");
        let pinned_in_map: std::collections::BTreeSet<&str> = map
            .supplemental_leaves
            .iter()
            .flat_map(|l| {
                l.assertion_contract
                    .as_ref()
                    .map(|c| c.cases.iter().map(|p| p.case.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default()
            })
            .collect();
        let tsv_set: std::collections::BTreeSet<&str> = case_names.iter().copied().collect();
        assert_eq!(
            pinned_in_map, tsv_set,
            "the map's pinned leaf cases and the TSV must match exactly"
        );
    }

    #[test]
    fn r05_stage_pin_table_matches_the_registered_suites() {
        let pins = parse_r05_pin_table();
        // The 27 integration suites with their EXACT pinned counts (dropping
        // or re-counting a suite here is the N03 fake-green shape). R05 RR1
        // re-registered 2026-10-05: the original 18 suites plus the nine RR1
        // repair batteries, with the RR1-grown counts.
        let expected: &[(&str, u32)] = &[
            ("adp:r05_t02_oauth_flows", 21),
            ("adp:r05_t03_goldens", 3),
            ("adp:r05_t03_rr1_replay", 33),
            ("adp:r05_t04_rr1_batch_terminal", 10),
            ("adp:r05_t04_streaming", 18),
            ("adp:r05_t05_compat", 1),
            ("adp:r05_t05_timeouts", 13),
            ("adp:r05_t06_operations", 27),
            ("adp:r05_t07_rr1_usage_strict", 7),
            ("adp:r05_t07_usage_families", 14),
            ("svc:r05_t01_binary_wiring", 2),
            ("svc:r05_t01_model_plane", 24),
            ("svc:r05_t02_credentials", 38),
            ("svc:r05_t03_protocol_adapters", 12),
            ("svc:r05_t04_streaming", 18),
            ("svc:r05_t05_network", 21),
            ("svc:r05_t05_timeouts", 9),
            ("svc:r05_t06_operations", 7),
            ("svc:r05_t06_rr1_media_resource", 17),
            ("svc:r05_t06_rr1_system_speech", 10),
            ("svc:r05_t06_worker_model", 10),
            ("svc:r05_t07_persistence", 5),
            ("svc:r05_t07_rr1_usage_ledger", 15),
            ("svc:r05_t07_usage_trace", 9),
            ("svc:r05_t08_closed_loop", 11),
            ("svc:r05_t08_production_tools", 6),
            ("svc:r05_t08_resources", 2),
        ];
        let mut suite_pins: Vec<(String, u32)> = pins
            .iter()
            .filter(|(run, _, _)| run.starts_with("svc:") || run.starts_with("adp:"))
            .map(|(run, count, _)| (run.clone(), *count))
            .collect();
        suite_pins.sort();
        let mut expected_sorted: Vec<(String, u32)> = expected
            .iter()
            .map(|(run, count)| (run.to_string(), *count))
            .collect();
        expected_sorted.sort();
        assert_eq!(
            suite_pins, expected_sorted,
            "the R05 pin table's suite registrations drifted (dropped, added, or re-counted)"
        );
        // The 64 lib pins each execute EXACTLY one test (--exact).
        let lib_pins: Vec<&(String, u32, String)> = pins
            .iter()
            .filter(|(run, _, _)| run.starts_with("lib-"))
            .collect();
        assert_eq!(lib_pins.len(), 64, "the registered lib pin count drifted");
        for (run, count, _) in &lib_pins {
            assert_eq!(
                *count, 1,
                "lib pin {run} must execute exactly one test (--exact)"
            );
            let rest = run.strip_prefix("lib-").expect("lib- prefix");
            let (pkg, _path) = rest.split_once('/').expect("lib-<pkg>/<path> shape");
            assert!(
                matches!(pkg, "adapters" | "kernel" | "service"),
                "unknown lib pin package in {run}"
            );
        }
    }

    /// Parses docs/rust-tauri/R05/r05_required_cids.tsv (the AUTHORITATIVE
    /// required-C-ID registry of R05 RR1 F26: 103 = 100 original + 3
    /// appended). Returns (cid, binding) pairs.
    fn parse_r05_required_cid_registry() -> Vec<(String, String)> {
        let text = std::fs::read_to_string(
            r05_repo_root().join("docs/rust-tauri/R05/r05_required_cids.tsv"),
        )
        .expect("docs/rust-tauri/R05/r05_required_cids.tsv must exist");
        text.lines()
            .filter_map(|l| l.strip_prefix("reqcid "))
            .map(|rest| {
                let mut fields = rest.split_whitespace();
                let cid = fields.next().expect("reqcid <C-ID>").to_string();
                let binding = fields.next().expect("reqcid <C-ID> <binding>").to_string();
                assert!(
                    fields.next().is_none(),
                    "reqcid line carries extra fields: {rest:?}"
                );
                (cid, binding)
            })
            .collect()
    }

    #[test]
    fn r05_stage_cid_table_owns_every_pinned_test_exactly_once() {
        let pins = parse_r05_pin_table();
        let declared = parse_r05_cid_table();
        // (a) R05 RR1 F26 (G02/N01/N03): the cid table's C-ID set is
        //     EXACTLY the `cid`-bound half of the AUTHORITATIVE registry —
        //     not a count, not nine examples: the full identity set. The
        //     rename probe (R05-T02-C01 → R05-T99-C99, count unchanged)
        //     turns THIS assert red.
        let registry = parse_r05_required_cid_registry();
        assert_eq!(
            registry.len(),
            103,
            "the authoritative registry must carry the 103 required C-IDs"
        );
        let mut seen_registry: Vec<&str> = Vec::new();
        for (cid, binding) in &registry {
            assert!(
                !seen_registry.contains(&cid.as_str()),
                "duplicate registry entry {cid}"
            );
            seen_registry.push(cid);
            assert!(
                binding == "cid" || binding.starts_with("command:"),
                "registry entry {cid} has an unknown binding {binding:?}"
            );
        }
        let expected_cid_owned: std::collections::BTreeSet<&str> = registry
            .iter()
            .filter(|(_, binding)| binding == "cid")
            .map(|(cid, _)| cid.as_str())
            .collect();
        let cids: std::collections::BTreeSet<&str> =
            declared.iter().map(|(cid, _, _)| cid.as_str()).collect();
        assert_eq!(
            cids,
            expected_cid_owned,
            "the cid table must own EXACTLY the registry's cid-bound set — a fabricated, \
             renamed, or dropped C-ID is a mismatch (fabricated/unregistered: {:?}; \
             missing: {:?})",
            cids.difference(&expected_cid_owned).collect::<Vec<_>>(),
            expected_cid_owned.difference(&cids).collect::<Vec<_>>()
        );
        // The command-bound half must name REGISTERED stage-map commands
        // (the shared-execution bindings point at real gate commands).
        let map = parse_production_r05();
        let command_keys: std::collections::BTreeSet<&str> =
            map.commands.iter().map(|c| c.key.as_str()).collect();
        for (cid, binding) in &registry {
            if let Some(key) = binding.strip_prefix("command:") {
                assert!(
                    command_keys.contains(key),
                    "registry entry {cid} binds to command {key:?} which the R05 map \
                     does not register"
                );
            }
        }
        // (b) Every cid line references a REGISTERED pin-table run.
        for (cid, run, _) in &declared {
            assert!(
                pins.iter().any(|(pinned_run, _, _)| pinned_run == run),
                "cid {cid} references run {run:?} which the pin table does not register"
            );
        }
        // (c) No test name may be owned by two C-IDs.
        let mut seen: Vec<(&str, &str)> = Vec::new();
        for (cid, run, names) in &declared {
            for name in names {
                let key = (run.as_str(), name.as_str());
                assert!(
                    !seen.contains(&key),
                    "test {name:?} in run {run:?} is claimed by two C-IDs (last {cid})"
                );
                seen.push(key);
            }
        }
        // (d) Every PINNED run's executed tests are fully owned: per-run
        //     owned names == the run's pinned count.
        for (run, count, _) in &pins {
            let owned = declared
                .iter()
                .filter(|(_, r, _)| r == run)
                .map(|(_, _, names)| names.len())
                .sum::<usize>() as u32;
            assert_eq!(
                owned, *count,
                "run {run:?} pins {count} executed tests but the cid table owns {owned}"
            );
        }
    }

    #[test]
    fn r05_scope_matrix_covers_every_r05_bound_leaf_honestly() {
        // R05-T01-C01's named producer for the command-bound registry entry
        // (R05 RR1 F26): the checked-in R05_SCOPE_MATRIX.json must cover the
        // SAME 130-leaf set as the stage map, classify the six R05-exclusive
        // OAuth leaves as FULL R05 ownership (F25 — the frozen matrix
        // released them as "share" with no R07 remainder), and never defer a
        // single-stage leaf.
        let root = r05_repo_root();
        let text = std::fs::read_to_string(root.join("docs/rust-tauri/R05/R05_SCOPE_MATRIX.json"))
            .expect("R05_SCOPE_MATRIX.json readable");
        let value: serde_json::Value = serde_json::from_str(&text).expect("scope matrix JSON");
        let map = parse_production_r05();
        let map_ids: std::collections::BTreeSet<String> = map
            .supplemental_leaves
            .iter()
            .map(|l| l.id.clone())
            .collect();
        let mut matrix_ids: std::collections::BTreeSet<String> = Default::default();
        for entry in value["supplemental_leaves"]
            .as_array()
            .expect("supplemental_leaves array")
        {
            let id = entry["id"].as_str().expect("leaf id").to_string();
            let stages: Vec<&str> = entry["r00_execution_stage_ids"]
                .as_array()
                .expect("stage ids")
                .iter()
                .map(|s| s.as_str().expect("stage str"))
                .collect();
            let disposition = entry["disposition"].as_str().expect("disposition");
            let exclusive = stages.len() == 1 && stages[0] == "R05";
            if exclusive {
                assert_eq!(
                    disposition, "full",
                    "leaf {id} is R05-exclusive: the scope matrix must own it as FULL \
                     behavior (F25 — a share would leave an unowned remainder)"
                );
                assert!(
                    entry["r07_remainder"].is_null(),
                    "leaf {id} declares an R07 remainder but has no later stage"
                );
            }
            // Non-exclusive leaves may legally be share OR deferred (the
            // R1/R2 rules defer genuine later-stage business entries — the
            // deferred_to set names the owning stage, checked by the
            // generator).
            matrix_ids.insert(id);
        }
        let missing: Vec<String> = map_ids
            .iter()
            .filter(|id| !matrix_ids.contains(id.as_str()))
            .cloned()
            .collect();
        assert!(
            missing.is_empty(),
            "the scope matrix dropped R05-bound leaves: {missing:?}"
        );
        assert_eq!(
            map_ids.len(),
            matrix_ids.len(),
            "the scope matrix invented leaves"
        );
        let full = value["counts"]["full"].as_u64().expect("full count");
        assert_eq!(
            full, 6,
            "exactly the six OAuth leaves are R05-exclusive full"
        );
    }

    #[test]
    fn r05_map_declares_no_live_lane_and_only_offline_commands() {
        // N14 protection: the R05 gate is offline-only by registration. A
        // command that picks up real credentials from the environment (or
        // any command beyond the seven registered offline ones) turns this
        // mirror red BEFORE any run could silently外发.
        let map = parse_production_r05();
        let mut keys: Vec<&str> = map.commands.iter().map(|c| c.key.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "check_boundaries",
                "check_contracts",
                "r04_regression_gate",
                "r05_stage_suites",
                "rust_clippy",
                "rust_fmt",
                "rust_test_workspace",
            ],
            "the R05 map's command set drifted — new commands need explicit review \
             against the offline-only policy"
        );
        for command in &map.commands {
            for arg in &command.argv {
                let upper = arg.to_ascii_uppercase();
                assert!(
                    !upper.contains("API_KEY")
                        && !upper.contains("TOKEN")
                        && !upper.contains("SECRET")
                        && !upper.contains("LIVE"),
                    "command {} references credential/live material in argv {arg:?} — \
                     LIVE is a separately-authorized lane, never an env pickup",
                    command.key
                );
            }
        }
    }
}
