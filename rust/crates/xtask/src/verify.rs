//! `verify-stage` execution: run every registered command for real, collect
//! real exit codes / logs / evidence existence, and emit the machine-
//! readable result. This module never inspects PASS strings — a scenario
//! passes if and only if its referenced commands exited 0 within their
//! timeout AND their declared evidence files exist.
//!
//! R02 stage-repair R1 hardening:
//! - **F04 evidence freshness**: a declared evidence path that ALREADY
//!   exists before the command runs means stale evidence would mask this
//!   run — the command is refused (never spawned), the outcome fails
//!   explicitly (`evidence_preexisting`), and [`verify_stage`] refuses a
//!   non-empty evidence root entirely (one fresh directory per run;
//!   history is never overwritten or reused).
//! - **F05 process-tree timeout cleanup**: the pre-fix timeout path ran a
//!   bare `child.kill()` and relied on the shell's EXIT trap for the rest
//!   of the tree — a killed shell never runs its trap, so grandchildren
//!   survived the gate. The timeout path now TERMs the whole runner
//!   process group first (so every shell that catches TERM still runs its
//!   own cleanup trap), watches a ≤3 s grace, then KILLs the group with a
//!   ≤2 s settle, reaps the direct child, and records an observable
//!   [`CleanupReport`]. Any survivor after the budget is reported
//!   (`survivors_after`), never hidden: the sweep's completeness is
//!   reported truthfully, not assumed.
//! - **R7-F02 → R8-F02 → R9-F02 object ownership**: every command is
//!   spawned as its own SESSION leader (`setsid`), which also makes it a
//!   fresh process-group leader, and the cleanup signals only the GROUP
//!   (`kill -SIG -<pgid>` — the kernel resolves the member set; no bare
//!   numeric pid is ever signalled). R8 kept the root unreaped so a
//!   recycled NUMBER could never receive our signal, but the group alone
//!   still could not prove membership: `process_group(0)` left the child
//!   in the caller's session, and setpgid(2) lets any SAME-SESSION
//!   process join an existing group. R9 closes that with the session
//!   boundary: session membership is only inherited by fork (a process's
//!   own setsid always creates a NEW session — joining is impossible), so
//!   the private session's group contains only this run's descendants and
//!   group membership PROVES ownership. Observability uses one consistent
//!   `ps` snapshot at a time — identity fields are captured together per
//!   row and membership is recomputed from the CURRENT table only. The
//!   R7 design registered a per-pid birth `lstart` string taken from a
//!   SECOND, independent `ps` read after the `pgrep` discovery snapshot —
//!   an interleaving that can register a foreign replacement's birth as
//!   ours — and `ps lstart` is second-granular, so two objects born in
//!   one second satisfy the string comparison. Both credential paths are
//!   gone. A descendant that leaves the group (its own setsid) and any
//!   unreadable snapshot are reported as survivors — never assumed clean.
//!   Non-Unix builds report honestly that the tree was not observable
//!   (`survivors_after: true`) instead of claiming a sweep that never
//!   ran. See the cleanup-module comment for the full invariants.
//!
//! R02 stage-repair R13 / F01 (R13-F01): the gate now CONSUMES the R00
//! REQUIRED_SUPPLEMENTAL leaf scenarios bound to the stage instead of only
//! listing them. Before any command runs, the stage map's declared
//! `supplementalLeafScenarios` set is cross-checked for EQUALITY against
//! the R00 acceptance ledger (`docs/rust-tauri/R00/ACCEPTANCE_MAP.json`,
//! the consumption R00-T07_REPORT §11 planned for verify-stage): a dropped
//! leaf, an invented leaf, or a re-graded requirement is a hard abort. Each
//! leaf then rolls up from the REAL command outcomes — every referenced
//! command must have passed and every declared evidence artifact must
//! exist, an unreferenced/never-run command counts as failed, and the two
//! `client_only_stage_boundary_conflict` leaves are held BLOCKED (they can
//! never be PASS) because their original R02 marking conflicts with the
//! stage boundary and only a root disposition recorded in the map may
//! change that. `overall` is PASS only when the base scenarios AND every
//! supplemental leaf pass, so 16/16 green can no longer masquerade as
//! complete stage acceptance while R00-required leaf shares went unevidenced.
//!
//! R02 stage-repair R14 / F01 (R14-F01, same-root closure of R13-F01's
//! remaining gaps): the cross-check now compares the leaf's FULL original
//! R00 record (feature id, kind, dual-stage task/stage responsibility,
//! ledger status, result/test registries, original `then`, the complete
//! assertion list from FEATURE_STAGE_ACCEPTANCE.json, and the due line)
//! against the map's `r00*` mirrors — swapping a binding to a different
//! R00 object is a hard abort even when id and requirement stay put. Every
//! non-conflict leaf must declare an `assertionContract`; the gate resolves
//! the producer command's structured case file and re-verifies each
//! declared case's ACTUAL value against the expectation pinned in the map
//! (and against the file's own recorded expectation), so a green command
//! exit can never pass a leaf whose PINNED case did not hold — the
//! concrete fake-green path was the ws-ticket leaf (R00 original: no
//! principal → 403) riding a script that graded 401 as success. Leaf
//! evidence paths are freshness-checked before the run like command
//! evidence.
//! R15-F01：案例值与图钉相同，仍不能证明它观察到了不同 R00 叶项的行为。
//! 21 项 `auth_primitive_only` 复用通用认证案例；即使案例通过，这些叶项
//! 仍保持 BLOCKED，缺少逐叶专属证据时 overall 不得 PASS。

use std::collections::HashSet;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::stage_map::{
    is_stage_neutral_kind, LeafDeferredCase, StageMap, SupplementalLeaf, BASIS_AUTH_PRIMITIVE_ONLY,
    BASIS_CLIENT_ONLY_STAGE_CONFLICT, BASIS_DEFERRED_TO_LATER_STAGE, BASIS_DEFERRED_TO_R07,
    BASIS_FULL_ORIGINAL_BEHAVIOR, BASIS_PROTOCOL, BASIS_R02_SHARE_SATISFIED,
    BASIS_ROUTE_PRESENT_STATIC, BASIS_STAGE_SHARE_SATISFIED,
};
use crate::RESULT_VERSION;

/// Repo-root-relative path of the R00 acceptance ledger this gate consumes
/// (R13-F01). R00-T07_REPORT §11 planned exactly this: "R02 建立 xtask
/// verify-stage 时应消费本账本（05 §3 的既定路线）". The ledger is the
/// machine-readable source of truth for which REQUIRED_SUPPLEMENTAL leaf
/// scenarios R00 binds to each stage, so the anti-filtering guarantee comes
/// from comparing the stage map's declared set against THIS file at verify
/// time — a stage map can add dispositions, but it can never silently drop
/// or re-grade an R00 obligation.
pub const R00_ACCEPTANCE_MAP_PATH: &str = "docs/rust-tauri/R00/ACCEPTANCE_MAP.json";

/// Repo-root-relative path of the R00 FEATURE_STAGE_ACCEPTANCE ledger —
/// the second R00 record each supplemental leaf is mirrored from (R14-F01):
/// it owns the leaf's original `assertions` list, the `due` line, and the
/// per-A-ID coverage relations (GAP/INDIRECT) that prove the base
/// scenarios alone cannot cover a leaf.
pub const R00_FEATURE_STAGE_ACCEPTANCE_PATH: &str =
    "docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json";

/// The R00-side expectation for one supplemental leaf (R14-F01: the FULL
/// original record, not just id+requirement). Every field is compared
/// verbatim against the stage map's `r00*` mirror before any command runs,
/// so a map cannot re-bind a leaf's feature, kind, dual-stage
/// responsibility, ledger status, execution-result registries, original
/// `then` text, assertion list, or due line to a different R00 object.
struct R00LeafExpectation {
    id: String,
    requirement: String,
    feature_id: String,
    kind: String,
    task_ids: Vec<String>,
    execution_stage_ids: Vec<String>,
    ledger_status: String,
    result_ids: Vec<String>,
    test_ids: Vec<String>,
    then: String,
    assertions: Vec<String>,
    due: String,
}

/// Loads the R00 acceptance ledger and returns the REQUIRED-supplemental
/// leaves it binds to `stage`, sorted by id. Fails closed on every error:
/// an unreadable, unparsable, or structurally drifted ledger means leaf
/// coverage CANNOT be verified, which must abort the gate — never pass.
fn load_r00_supplemental_expectations(
    repo_root: &Path,
    stage: &str,
) -> Result<Vec<R00LeafExpectation>, String> {
    let path = repo_root.join(R00_ACCEPTANCE_MAP_PATH);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "cannot read the R00 acceptance ledger {}: {e} (the stage gate \
             cross-checks its REQUIRED_SUPPLEMENTAL leaf bindings — failing \
             closed, a missing ledger can never mean zero obligations)",
            path.display()
        )
    })?;
    let value: Value = serde_json::from_str(&text).map_err(|e| {
        format!(
            "R00 acceptance ledger {} is not valid JSON: {e}",
            path.display()
        )
    })?;
    let scenarios = value
        .get("scenarios")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            format!(
                "R00 acceptance ledger {} has no scenarios object",
                path.display()
            )
        })?;

    // R14-F01: the assertions/due fields live in the SECOND R00 ledger
    // (FEATURE_STAGE_ACCEPTANCE.json). Load it once and index by id — a
    // leaf missing there is as fatal as a leaf missing here.
    let fsa_path = repo_root.join(R00_FEATURE_STAGE_ACCEPTANCE_PATH);
    let fsa_text = std::fs::read_to_string(&fsa_path).map_err(|e| {
        format!(
            "cannot read the R00 FEATURE_STAGE_ACCEPTANCE ledger {}: {e} (the \
             stage gate cross-checks each leaf's original assertions and due \
             line — failing closed, a missing ledger can never mean no \
             obligations)",
            fsa_path.display()
        )
    })?;
    let fsa: Value = serde_json::from_str(&fsa_text).map_err(|e| {
        format!(
            "R00 FEATURE_STAGE_ACCEPTANCE ledger {} is not valid JSON: {e}",
            fsa_path.display()
        )
    })?;
    let fsa_by_id: std::collections::HashMap<String, &Value> = fsa
        .get("supplemental_scenarios")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            format!(
                "R00 FEATURE_STAGE_ACCEPTANCE ledger {} has no supplemental_scenarios array",
                fsa_path.display()
            )
        })?
        .iter()
        .filter_map(|entry| {
            entry
                .get("id")
                .and_then(Value::as_str)
                .map(|id| (id.to_string(), entry))
        })
        .collect();

    /// Reads one required string field of an R00 record.
    fn req_str(entry: &Value, field: &str, id: &str, ledger: &str) -> Result<String, String> {
        entry
            .get(field)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("R00 ledger {ledger} scenario {id:?} has no {field} string"))
    }
    /// Reads one required string-array field of an R00 record.
    fn req_str_array(
        entry: &Value,
        field: &str,
        id: &str,
        ledger: &str,
    ) -> Result<Vec<String>, String> {
        entry
            .get(field)
            .and_then(Value::as_array)
            .ok_or_else(|| {
                format!("R00 ledger {ledger} scenario {id:?} has no {field} array")
            })?
            .iter()
            .map(|v| {
                v.as_str().map(str::to_string).ok_or_else(|| {
                    format!("R00 ledger {ledger} scenario {id:?} field {field} has a non-string element")
                })
            })
            .collect()
    }

    let mut found = Vec::new();
    for (id, entry) in scenarios {
        let entry = entry.as_object().ok_or_else(|| {
            format!(
                "R00 acceptance ledger {} scenario {id:?} is not an object",
                path.display()
            )
        })?;
        let bound_to_stage = entry
            .get("execution_stage_ids")
            .and_then(Value::as_array)
            .is_some_and(|stages| stages.iter().any(|s| s.as_str() == Some(stage)));
        if bound_to_stage {
            let entry_value = Value::Object(entry.clone());
            let fsa_entry = fsa_by_id.get(id).copied().ok_or_else(|| {
                format!(
                    "supplemental leaf {id:?} is bound to stage {stage} in {} but has \
                     no record in {} — the two R00 ledgers disagree, failing closed",
                    R00_ACCEPTANCE_MAP_PATH, R00_FEATURE_STAGE_ACCEPTANCE_PATH
                )
            })?;
            found.push(R00LeafExpectation {
                id: id.clone(),
                requirement: req_str(&entry_value, "requirement", id, "ACCEPTANCE_MAP")?,
                feature_id: req_str(&entry_value, "feature_id", id, "ACCEPTANCE_MAP")?,
                kind: req_str(&entry_value, "kind", id, "ACCEPTANCE_MAP")?,
                task_ids: req_str_array(&entry_value, "task_ids", id, "ACCEPTANCE_MAP")?,
                execution_stage_ids: req_str_array(
                    &entry_value,
                    "execution_stage_ids",
                    id,
                    "ACCEPTANCE_MAP",
                )?,
                ledger_status: req_str(&entry_value, "ledger_status", id, "ACCEPTANCE_MAP")?,
                result_ids: req_str_array(&entry_value, "result_ids", id, "ACCEPTANCE_MAP")?,
                test_ids: req_str_array(&entry_value, "test_ids", id, "ACCEPTANCE_MAP")?,
                then: req_str(&entry_value, "then", id, "ACCEPTANCE_MAP")?,
                assertions: req_str_array(fsa_entry, "assertions", id, "FEATURE_STAGE_ACCEPTANCE")?,
                due: req_str(fsa_entry, "due", id, "FEATURE_STAGE_ACCEPTANCE")?,
            });
        }
    }
    found.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(found)
}

/// Cross-checks the stage map's declared supplemental leaves against the
/// R00 acceptance ledger (R13-F01): the id sets must be EQUAL and every
/// requirement must match R00's recorded value. Missing entries mean
/// required R00 obligations were filtered out of the gate; extra entries
/// mean unknown leaves; a requirement mismatch means the map tried to
/// re-grade an R00 obligation. All three are hard errors before any
/// command runs.
///
/// R14-F01 extends the comparison to the leaf's FULL original record:
/// feature id, kind, dual-stage task/stage responsibility, ledger status,
/// execution-result/test registries, original `then` text, the complete
/// original assertion list, and the due line. Keeping the id and the
/// requirement while swapping any other binding to a different R00 object
/// is now equally fatal — "错误绑定硬拒" covers the whole record.
fn cross_check_supplemental_coverage(
    map: &StageMap,
    repo_root: &Path,
) -> Result<Vec<R00LeafExpectation>, String> {
    let expected = load_r00_supplemental_expectations(repo_root, &map.stage)?;
    let missing: Vec<&str> = expected
        .iter()
        .filter(|e| !map.supplemental_leaves.iter().any(|leaf| leaf.id == e.id))
        .map(|e| e.id.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "stage map for {} drops {} REQUIRED_SUPPLEMENTAL leaf scenario(s) that the \
             R00 acceptance ledger binds to this stage: {:?} — required leaves may not \
             be filtered out of the stage gate (R13-F01)",
            map.stage,
            missing.len(),
            missing
        ));
    }
    let extra: Vec<&str> = map
        .supplemental_leaves
        .iter()
        .filter(|leaf| !expected.iter().any(|e| e.id == leaf.id))
        .map(|leaf| leaf.id.as_str())
        .collect();
    if !extra.is_empty() {
        return Err(format!(
            "stage map for {} declares {} supplemental leaf scenario(s) unknown to the \
             R00 acceptance ledger: {:?} — only R00-bound leaves belong in the gate",
            map.stage,
            extra.len(),
            extra
        ));
    }
    for leaf in &map.supplemental_leaves {
        let Some(recorded) = expected.iter().find(|e| e.id == leaf.id) else {
            continue;
        };
        // R14-F01: one mismatch collector per leaf so the error names the
        // drifted field(s) precisely instead of one cryptic first failure.
        let mut drift: Vec<String> = Vec::new();
        if recorded.requirement != leaf.requirement {
            drift.push(format!(
                "requirement map={:?} r00={:?}",
                leaf.requirement, recorded.requirement
            ));
        }
        if recorded.feature_id != leaf.feature_id {
            drift.push(format!(
                "featureId map={:?} r00={:?}",
                leaf.feature_id, recorded.feature_id
            ));
        }
        if recorded.kind != leaf.r00_kind {
            drift.push(format!(
                "r00Kind map={:?} r00={:?}",
                leaf.r00_kind, recorded.kind
            ));
        }
        if recorded.task_ids != leaf.r00_task_ids {
            drift.push(format!(
                "r00TaskIds map={:?} r00={:?}",
                leaf.r00_task_ids, recorded.task_ids
            ));
        }
        if recorded.execution_stage_ids != leaf.r00_execution_stage_ids {
            drift.push(format!(
                "r00ExecutionStageIds map={:?} r00={:?}",
                leaf.r00_execution_stage_ids, recorded.execution_stage_ids
            ));
        }
        if recorded.ledger_status != leaf.r00_ledger_status {
            drift.push(format!(
                "r00LedgerStatus map={:?} r00={:?}",
                leaf.r00_ledger_status, recorded.ledger_status
            ));
        }
        if recorded.result_ids != leaf.r00_result_ids {
            drift.push(format!(
                "r00ResultIds map={:?} r00={:?}",
                leaf.r00_result_ids, recorded.result_ids
            ));
        }
        if recorded.test_ids != leaf.r00_test_ids {
            drift.push(format!(
                "r00TestIds map={:?} r00={:?}",
                leaf.r00_test_ids, recorded.test_ids
            ));
        }
        if recorded.then != leaf.r00_then {
            drift.push(format!(
                "r00Then map={:?} r00={:?}",
                leaf.r00_then, recorded.then
            ));
        }
        if recorded.assertions != leaf.r00_assertions {
            drift.push(format!(
                "r00Assertions map={:?} ({} entries) r00={:?} ({} entries)",
                leaf.r00_assertions,
                leaf.r00_assertions.len(),
                recorded.assertions,
                recorded.assertions.len()
            ));
        }
        if recorded.due != leaf.r00_due {
            drift.push(format!(
                "r00Due map={:?} r00={:?}",
                leaf.r00_due, recorded.due
            ));
        }
        if !drift.is_empty() {
            return Err(format!(
                "supplemental leaf {} mirrors an R00 record that does not match the \
                 ledgers — the stage map may not re-bind or re-grade an R00 obligation \
                 (R14-F01): {}",
                leaf.id,
                drift.join("; ")
            ));
        }
    }
    Ok(expected)
}

/// The schema string every leaf-case evidence file must declare (R14-F01):
/// `{"schema": "lingxi.leaf-case-results.v1", "cases": [{"case": str,
/// "expect": int, "actual": int, "ok": bool}]}`. Producers (the registered
/// gate scripts) write it; this gate reads it and re-verifies every case
/// the stage map declares against the ACTUAL recorded value.
pub const LEAF_CASE_RESULTS_SCHEMA: &str = "lingxi.leaf-case-results.v1";

/// 核对单个叶项的结构化案例：每个登记案例都必须存在、`ok == true`，
/// 实际值与证据文件自带期望都必须等于阶段图的图钉值。这只能证明案例数值，
/// 不能自行证明案例观察到了 R00 原行为；语义缺口由叶项汇总另行阻断。
fn check_leaf_assertion_contract(
    leaf: &SupplementalLeaf,
    outcomes: &[CommandOutcome],
    repo_root: &Path,
    evidence_root: &Path,
) -> (Vec<Value>, String) {
    let Some(contract) = leaf.assertion_contract.as_ref() else {
        // The parser rejects a non-conflict leaf without a contract; if
        // this is reached the invariant broke — fail closed, never pass.
        return (
            Vec::new(),
            "assertion contract missing (parser invariant violated — failing closed)".to_string(),
        );
    };
    // The producer must have PASSED this run. (The refs check below already
    // covers it since the parser forces producer ∈ refs, but the contract
    // check names the producer explicitly.)
    let producer_passed = outcomes
        .iter()
        .any(|o| o.key == contract.producer_command && o.passed());
    if !producer_passed {
        return (
            vec![json!({
                "producerCommand": contract.producer_command,
                "passed": false,
            })],
            format!(
                "evidence producer command {:?} did not pass in this run — the leaf's \
                 case evidence cannot be trusted",
                contract.producer_command
            ),
        );
    }
    let path = resolve_evidence_path(&contract.evidence_path, repo_root, evidence_root);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => {
            return (
                vec![json!({
                    "evidencePath": contract.evidence_path,
                    "exists": false,
                })],
                format!(
                    "leaf evidence file {} is missing ({e}) — a leaf without its \
                     producer's structured case file has no legal evidence",
                    contract.evidence_path
                ),
            );
        }
    };
    let parsed: Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(e) => {
            return (
                vec![json!({"evidencePath": contract.evidence_path, "parseOk": false})],
                format!(
                    "leaf evidence file {} is not valid JSON: {e}",
                    contract.evidence_path
                ),
            );
        }
    };
    if parsed.get("schema").and_then(Value::as_str) != Some(LEAF_CASE_RESULTS_SCHEMA) {
        return (
            vec![json!({
                "evidencePath": contract.evidence_path,
                "schema": parsed.get("schema").cloned().unwrap_or(Value::Null),
            })],
            format!(
                "leaf evidence file {} does not declare schema {:?}",
                contract.evidence_path, LEAF_CASE_RESULTS_SCHEMA
            ),
        );
    }
    let Some(file_cases) = parsed.get("cases").and_then(Value::as_array) else {
        return (
            vec![json!({"evidencePath": contract.evidence_path, "casesValid": false})],
            format!(
                "leaf evidence file {} has no cases array",
                contract.evidence_path
            ),
        );
    };
    let mut results = Vec::with_capacity(contract.cases.len());
    let mut failures = Vec::new();
    // 一个证据文件可供多个叶项消费。先检查整份记录，避免生产者退出 0 后
    // 同名案例的首条成功掩盖后条失败，或未被当前叶钉住的失败被忽略。
    let mut names = HashSet::new();
    for (index, recorded) in file_cases.iter().enumerate() {
        let Some(name) = recorded.get("case").and_then(Value::as_str) else {
            failures.push(format!("evidence case #{index} has no case name"));
            continue;
        };
        if !names.insert(name) {
            failures.push(format!("duplicate evidence case name {name:?}"));
        }
        let ok = recorded.get("ok").and_then(Value::as_bool);
        let actual = recorded.get("actual").and_then(Value::as_i64);
        let expect = recorded.get("expect").and_then(Value::as_i64);
        if ok != Some(true) || actual.is_none() || actual != expect {
            failures.push(format!(
                "evidence case {name:?} at index {index} did not hold: \
                 ok={ok:?} actual={actual:?} expect={expect:?}"
            ));
        }
    }
    for pinned in &contract.cases {
        let Some(recorded) = file_cases
            .iter()
            .find(|c| c.get("case").and_then(Value::as_str) == Some(pinned.case.as_str()))
        else {
            failures.push(format!(
                "declared case {:?} not recorded in {}",
                pinned.case, contract.evidence_path
            ));
            results.push(json!({
                "case": pinned.case,
                "expect": pinned.expect,
                "status": "MISSING",
            }));
            continue;
        };
        let ok = recorded.get("ok").and_then(Value::as_bool);
        let actual = recorded.get("actual").and_then(Value::as_i64);
        let file_expect = recorded.get("expect").and_then(Value::as_i64);
        let case_ok =
            ok == Some(true) && actual == Some(pinned.expect) && file_expect == Some(pinned.expect);
        if !case_ok {
            failures.push(format!(
                "case {:?}: map pins expect {} but evidence records ok={:?} \
                 actual={:?} expect={:?} — the original assertion did NOT hold as \
                 recorded",
                pinned.case, pinned.expect, ok, actual, file_expect
            ));
        }
        results.push(json!({
            "case": pinned.case,
            "expect": pinned.expect,
            "actual": actual,
            "recordedOk": ok,
            "status": if case_ok { "PASS" } else { "FAIL" },
        }));
    }
    (results, failures.join("; "))
}

/// 根据已执行命令与证据汇总一个 R00 补充叶项。门禁叶（协议/份额/完整
/// 行为）要求所有引用命令、证据文件和图钉案例通过；未执行命令不能算
/// 通过。阶段边界冲突项保持 BLOCKED。R15-F01：通用认证案例即使全绿，
/// 也不能证明各叶项原行为；`auth_primitive_only` 在登记逐叶专属证据前
/// 保持 BLOCKED（旧图回放防护——现行 R02 图已不再使用该 kind）。
/// R02-final group 1（阶段所有权恢复）：
/// - `r02_share_satisfied`：全部份额图钉 ok 且命令通过即本叶 R02 份额
///   PASS，结果附带 r07Share 义务文本与 deferredToStage=R07——R07 余款
///   不再是 R02 PASS 的前置（撤销 R17 轮的越界门禁语义）。
/// - `deferred_to_r07`：无 R02 门禁份额，固定输出 DEFERRED_TO_R07（仍
///   REQUIRED、验收归 R07），附 r07Share 文本与（若有）earlyEvidence
///   观察。递延仅限显式声明的叶，不允许任何叶静默跳过。
/// - 旧「另属后续阶段→BLOCKED」分支仅保留给旧图的局部 kind
///   （protocol_basis/route_basis_present_static）回放防护；对
///   full_original_behavior 与 r02_share_satisfied 不再拦截。
#[derive(Debug, Clone, PartialEq)]
pub struct LeafRollUp {
    pub status: String,
    pub reason: String,
    pub assertion_results: Vec<Value>,
    pub deferred_case_results: Vec<Value>,
    /// 递延叶的提前实现命令健康观察（非门禁）。
    pub early_evidence: Option<Value>,
}

/// 观察一个递延 R07 案例的记录值：只读取、不判定。生产者未通过或文件
/// 缺失时返回 NOT_OBSERVED 并说明原因——观察失败不是叶失败，但命令
/// 失败已在 overall 层面按命令 FAIL 处理（提前实现的代码必须保持健康）。
fn observe_deferred_case(
    record: &LeafDeferredCase,
    outcomes: &[CommandOutcome],
    repo_root: &Path,
    evidence_root: &Path,
) -> Value {
    let producer_passed = outcomes
        .iter()
        .any(|o| o.key == record.producer_command && o.passed());
    if !producer_passed {
        return json!({
            "case": record.case,
            "expect": record.expect,
            "producerCommand": record.producer_command,
            "evidencePath": record.evidence_path,
            "status": "NOT_OBSERVED",
            "reason": format!(
                "early-evidence producer {:?} did not pass in this run",
                record.producer_command
            ),
        });
    }
    let path = resolve_evidence_path(&record.evidence_path, repo_root, evidence_root);
    let observed = std::fs::read_to_string(&path)
        .map_err(|e| format!("missing ({e})"))
        .and_then(|text| {
            serde_json::from_str::<Value>(&text).map_err(|e| format!("not valid JSON: {e}"))
        })
        .and_then(|parsed| {
            let schema_ok =
                parsed.get("schema").and_then(Value::as_str) == Some(LEAF_CASE_RESULTS_SCHEMA);
            if !schema_ok {
                return Err("schema mismatch".to_string());
            }
            parsed
                .get("cases")
                .and_then(Value::as_array)
                .cloned()
                .ok_or_else(|| "no cases array".to_string())
        });
    let cases = match observed {
        Ok(cases) => cases,
        Err(reason) => {
            return json!({
                "case": record.case,
                "expect": record.expect,
                "producerCommand": record.producer_command,
                "evidencePath": record.evidence_path,
                "status": "NOT_OBSERVED",
                "reason": reason,
            });
        }
    };
    let matches: Vec<&Value> = cases
        .iter()
        .filter(|c| c.get("case").and_then(Value::as_str) == Some(record.case.as_str()))
        .collect();
    if matches.len() != 1 {
        return json!({
            "case": record.case,
            "expect": record.expect,
            "producerCommand": record.producer_command,
            "evidencePath": record.evidence_path,
            "status": "NOT_OBSERVED",
            "reason": format!("expected exactly one recorded entry, found {}", matches.len()),
        });
    }
    let recorded = matches[0];
    let actual = recorded.get("actual").and_then(Value::as_i64);
    let ok = recorded.get("ok").and_then(Value::as_bool);
    let held = ok == Some(true) && actual == Some(record.expect);
    json!({
        "case": record.case,
        "expect": record.expect,
        "actual": actual,
        "recordedOk": ok,
        "producerCommand": record.producer_command,
        "evidencePath": record.evidence_path,
        "status": if held { "OBSERVED_HELD" } else { "OBSERVED_NOT_HELD" },
    })
}

/// The stages after `stage` this leaf's acceptance also belongs to
/// (derived from the mirrored R00 execution_stage_ids — never invented).
fn later_stages_of(leaf: &SupplementalLeaf, stage: &str) -> Vec<String> {
    leaf.r00_execution_stage_ids
        .iter()
        .filter(|id| id.as_str() != stage)
        .cloned()
        .collect()
}

fn roll_up_supplemental_leaf(
    leaf: &SupplementalLeaf,
    stage: &str,
    outcomes: &[CommandOutcome],
    repo_root: &Path,
    evidence_root: &Path,
) -> LeafRollUp {
    if leaf.basis_kind == BASIS_CLIENT_ONLY_STAGE_CONFLICT {
        return LeafRollUp {
            status: "BLOCKED".to_string(),
            reason: format!(
                "stage-boundary conflict with the R00 original is unresolved — this leaf \
                 can never be PASS in the gate; see evidenceRequired for the required \
                 root disposition: {}",
                leaf.evidence_required
            ),
            assertion_results: Vec::new(),
            deferred_case_results: Vec::new(),
            early_evidence: None,
        };
    }
    // 递延叶：固定 DEFERRED_TO_R07，附 r07Share 义务文本与 earlyEvidence
    // 观察。提前实现命令未通过不改叶状态（验收不归本门禁），但命令
    // FAIL 由 overall 的全命令通过条件拦下，绝不静默变绿。
    if leaf.basis_kind == BASIS_DEFERRED_TO_LATER_STAGE {
        let deferred_case_results = leaf
            .deferred_r07_cases
            .iter()
            .map(|record| observe_deferred_case(record, outcomes, repo_root, evidence_root))
            .collect::<Vec<_>>();
        let early_not_passing: Vec<&str> = leaf
            .early_evidence_command_refs
            .iter()
            .filter(|key| !outcomes.iter().any(|o| o.key == **key && o.passed()))
            .map(|key| key.as_str())
            .collect();
        let later_stages = later_stages_of(leaf, stage);
        let mut reason = format!(
            "deferred to later stage(s) {later_stages:?}: this R00 leaf is still REQUIRED but \
             has no {stage} gating share; its acceptance belongs to those stages, which must \
             consume the remainder: {}",
            leaf.later_share
        );
        if !early_not_passing.is_empty() {
            reason.push_str(&format!(
                " (early-evidence commands not passing this run (still command FAILs, health \
                 of the pre-implemented code is mandatory): {early_not_passing:?})"
            ));
        }
        let early_evidence = json!({
            "commandRefs": leaf.early_evidence_command_refs,
            "commandsNotPassing": early_not_passing,
            "note": "non-gating: producers of pre-implemented later-stage behavior stay \
                     registered and run every gate; their failures are command FAILs but never \
                     gate this leaf here",
        });
        return LeafRollUp {
            status: "DEFERRED_TO_LATER_STAGE".to_string(),
            reason,
            assertion_results: Vec::new(),
            deferred_case_results,
            early_evidence: Some(early_evidence),
        };
    }
    if leaf.basis_kind == BASIS_DEFERRED_TO_R07 {
        let deferred_case_results = leaf
            .deferred_r07_cases
            .iter()
            .map(|record| observe_deferred_case(record, outcomes, repo_root, evidence_root))
            .collect::<Vec<_>>();
        let early_not_passing: Vec<&str> = leaf
            .early_evidence_command_refs
            .iter()
            .filter(|key| !outcomes.iter().any(|o| o.key == **key && o.passed()))
            .map(|key| key.as_str())
            .collect();
        let mut reason = format!(
            "deferred to R07: this R00 leaf is still REQUIRED but has no R02 gating share; \
             its acceptance belongs to the R07 stage, which must consume the remainder: {}",
            leaf.r07_share
        );
        if !early_not_passing.is_empty() {
            reason.push_str(&format!(
                " (early-evidence commands not passing this run (still command FAILs, health \
                 of the pre-implemented code is mandatory): {early_not_passing:?})"
            ));
        }
        let early_evidence = json!({
            "commandRefs": leaf.early_evidence_command_refs,
            "commandsNotPassing": early_not_passing,
            "note": "non-gating: producers of the pre-implemented R07 behavior stay registered \
                     and run every gate; their failures are command FAILs but never gate this \
                     leaf in R02",
        });
        return LeafRollUp {
            status: "DEFERRED_TO_R07".to_string(),
            reason,
            assertion_results: Vec::new(),
            deferred_case_results,
            early_evidence: Some(early_evidence),
        };
    }
    let failed_refs: Vec<&str> = leaf
        .evidence_command_refs
        .iter()
        .filter(|key| !outcomes.iter().any(|o| o.key == **key && o.passed()))
        .map(|key| key.as_str())
        .collect();
    let missing_paths: Vec<&str> = leaf
        .evidence_paths
        .iter()
        .filter(|entry| !resolve_evidence_path(entry, repo_root, evidence_root).exists())
        .map(|entry| entry.as_str())
        .collect();
    // R14-F01 核对实际记录值与图钉，不只看命令退出码。R15-F01 进一步
    // 核对证据范围：通用认证案例不能证明每项原始叶行为。
    let (assertion_results, assertion_failures) =
        check_leaf_assertion_contract(leaf, outcomes, repo_root, evidence_root);
    // 递延案例只观察不判定（份额叶拆分出的 R07 余款案例）。
    let deferred_case_results = leaf
        .deferred_r07_cases
        .iter()
        .map(|record| observe_deferred_case(record, outcomes, repo_root, evidence_root))
        .collect::<Vec<_>>();
    if failed_refs.is_empty() && missing_paths.is_empty() && assertion_failures.is_empty() {
        if leaf.basis_kind == BASIS_AUTH_PRIMITIVE_ONLY {
            return LeafRollUp {
                status: "BLOCKED".to_string(),
                reason: format!(
                    "authentication-primitives passed, but their generic status-code cases do \
                     not prove this R00 leaf's original behavior or side effects; leaf-specific \
                     positive and negative evidence is still required: {}",
                    leaf.evidence_required
                ),
                assertion_results,
                deferred_case_results,
                early_evidence: None,
            };
        }
        if leaf.basis_kind == BASIS_FULL_ORIGINAL_BEHAVIOR {
            // The parser enforces this shape for loaded maps. Recheck here
            // too: tests and future callers can construct StageMap directly.
            let pinned: Vec<&str> = leaf
                .assertion_contract
                .as_ref()
                .map(|contract| {
                    contract
                        .cases
                        .iter()
                        .map(|case| case.case.as_str())
                        .collect()
                })
                .unwrap_or_default();
            let covered: Vec<&str> = leaf
                .original_assertion_cases
                .iter()
                .flat_map(|group| group.iter().map(String::as_str))
                .collect();
            let coverage_complete = leaf.original_assertion_cases.len()
                == leaf.r00_assertions.len()
                && leaf
                    .original_assertion_cases
                    .iter()
                    .all(|group| !group.is_empty())
                && covered.len() == pinned.len()
                && covered.iter().all(|case| pinned.contains(case))
                && covered
                    .iter()
                    .enumerate()
                    .all(|(index, case)| !covered[..index].contains(case));
            if !coverage_complete {
                return LeafRollUp {
                    status: "FAIL".to_string(),
                    reason: "full_original_behavior lacks a unique executed case for every \
                             original R00 assertion (parser invariant violated)"
                        .to_string(),
                    assertion_results,
                    deferred_case_results,
                    early_evidence: None,
                };
            }
            return LeafRollUp {
                status: "PASS".to_string(),
                reason: String::new(),
                assertion_results,
                deferred_case_results,
                early_evidence: None,
            };
        }
        // 旧图回放防护：局部 kind 的双阶段叶仍按原语义阻断（现行 R02 图
        // 中已没有这类叶——份额叶一律显式声明 r02_share_satisfied）。
        // full_original_behavior 与 r02_share_satisfied 不再被拦截：要求
        // R02 先证 R07 客户端行为属阶段所有权越界（本轮撤销）。
        if (leaf.basis_kind == BASIS_PROTOCOL || leaf.basis_kind == BASIS_ROUTE_PRESENT_STATIC)
            && leaf
                .r00_execution_stage_ids
                .iter()
                .any(|id| id.as_str() != stage)
        {
            return LeafRollUp {
                status: "BLOCKED".to_string(),
                reason: format!(
                    "this R00 leaf is also due in another stage ({:?}); the legacy \
                     partial basisKind {:?} does not declare an explicit stage-share split \
                     (migrate the leaf to r02_share_satisfied or deferred_to_r07)",
                    leaf.r00_execution_stage_ids, leaf.basis_kind
                ),
                assertion_results,
                deferred_case_results,
                early_evidence: None,
            };
        }
        let reason = if leaf.basis_kind == BASIS_R02_SHARE_SATISFIED {
            format!(
                "R02 share satisfied: all share pins held; the R07 remainder stays REQUIRED \
                 and moves with deferredToStage=R07: {}",
                leaf.r07_share
            )
        } else if leaf.basis_kind == BASIS_STAGE_SHARE_SATISFIED {
            format!(
                "{stage} share satisfied: all share pins held; the remainder stays REQUIRED \
                 and moves with deferredToStages={:?}: {}",
                later_stages_of(leaf, stage),
                leaf.later_share
            )
        } else {
            String::new()
        };
        return LeafRollUp {
            status: "PASS".to_string(),
            reason,
            assertion_results,
            deferred_case_results,
            early_evidence: None,
        };
    }
    let mut reasons = Vec::new();
    if !failed_refs.is_empty() {
        reasons.push(format!(
            "evidence commands not passing (or not executed): {failed_refs:?}"
        ));
    }
    if !missing_paths.is_empty() {
        reasons.push(format!(
            "missing required evidence artifacts: {missing_paths:?}"
        ));
    }
    if !assertion_failures.is_empty() {
        reasons.push(format!(
            "original-assertion cases not holding: {assertion_failures}"
        ));
    }
    LeafRollUp {
        status: "FAIL".to_string(),
        reason: format!(
            "no legal evidence for the R02 share of this leaf: {} — a required \
             leaf without evidence (or whose original assertions did not hold as \
             recorded) is a failure, never a pass",
            reasons.join("; ")
        ),
        assertion_results,
        deferred_case_results,
        early_evidence: None,
    }
}

/// Observable record of the process-tree cleanup after a timeout (F05).
#[derive(Debug, Clone, PartialEq)]
pub struct CleanupReport {
    /// SIGTERM was delivered to the runner's process group.
    pub term_sent: bool,
    /// SIGKILL was actually DELIVERED to the group: at least one
    /// `kill -KILL -- -<pgid>` invocation succeeded during the KILL phase
    /// (R10-F03: the field records the REAL delivery result, never a
    /// pre-written intent — a KILL attempt that failed, or a group that
    /// vanished before the first attempt, leaves this false; the report
    /// never claims a signal that was not sent).
    pub kill_sent: bool,
    /// Wall-clock the cleanup took.
    pub elapsed_ms: u64,
    /// The direct child was reaped within the bounded reap budget (never
    /// left a zombie while reapable). R12-F01: `false` means the child
    /// was NOT reapable within the budget (uninterruptible D-state) and
    /// is recorded as residue — the cleanup never blocks indefinitely
    /// on it and never assumes a reap it did not observe.
    pub reaped: bool,
    /// Tree members still alive after the whole cleanup (aggregate, must
    /// be false; recorded, never hidden — also true when completeness
    /// could not be proven: a descendant that left the group, or an
    /// unobservable process table).
    pub survivors_after: bool,
    /// R10-F03 — WHY `survivors_after` is true, listed separately so a
    /// machine consumer never conflates the causes: a live member was
    /// OBSERVED in the final process-table snapshot. False when nothing
    /// live was seen — an unreadable FINAL snapshot records nothing
    /// observed and only sets `survivors_unobserved`.
    pub survivors_observed: bool,
    /// R10-F03 — completeness could NOT be proven (any snapshot during
    /// the cleanup failed — including the final one — or a group leaver
    /// was seen whose exit can never be proven after re-parenting).
    /// Reported WITHOUT claiming any live member, and never
    /// externalized as a "foreign process we mistakenly killed": the
    /// R9-F02 session boundary keeps every group member provably ours,
    /// so an unprovable exit is OUR residue until observed otherwise.
    pub survivors_unobserved: bool,
}

/// Outcome of one executed command.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandOutcome {
    pub key: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// 命令目录、日志、启动或等待失败时保留的原始错误。
    pub internal_error: Option<String>,
    pub evidence_missing: Vec<String>,
    /// Declared evidence paths that ALREADY EXISTED before the command ran
    /// (F04): stale evidence from an earlier run. Non-empty ⇒ the command
    /// was refused (never spawned) and the outcome fails.
    pub evidence_preexisting: Vec<String>,
    /// Process-tree cleanup record when the command timed out (F05).
    pub cleanup: Option<CleanupReport>,
}

impl CommandOutcome {
    pub fn passed(&self) -> bool {
        !self.timed_out
            && self.internal_error.is_none()
            && self.exit_code == Some(0)
            && self.evidence_missing.is_empty()
            && self.evidence_preexisting.is_empty()
    }
}

/// Placeholder replaced with the resolved evidence root in `argv` and in
/// `evidencePaths` entries.
pub const EVIDENCE_PLACEHOLDER: &str = "{EVIDENCE}";
/// Placeholder replaced with the repo root (for scripts that must be
/// addressed by absolute path).
pub const REPO_ROOT_PLACEHOLDER: &str = "{REPO_ROOT}";

fn substitute(text: &str, repo_root: &Path, evidence_root: &Path) -> String {
    text.replace(REPO_ROOT_PLACEHOLDER, &repo_root.to_string_lossy())
        .replace(EVIDENCE_PLACEHOLDER, &evidence_root.to_string_lossy())
}

/// Executes ONE command under `timeout`, capturing stdout/stderr into
/// `<command_dir>/stdout.log` / `stderr.log`.
///
/// Freshness binding (F04): `command_dir` must not pre-exist (one command
/// writes one fresh directory — history is never overwritten), and every
/// declared evidence path must be ABSENT before the spawn; a pre-existing
/// evidence file is stale evidence that would mask this run, so the
/// command is REFUSED (never spawned) with an explicitly failing outcome.
pub fn run_command(
    spec: &crate::stage_map::CommandSpec,
    repo_root: &Path,
    evidence_root: &Path,
    command_dir: &Path,
) -> Result<CommandOutcome, String> {
    run_command_with_after_dir(spec, repo_root, evidence_root, command_dir, |_| {})
}

fn run_command_with_after_dir<F: FnOnce(&Path)>(
    spec: &crate::stage_map::CommandSpec,
    repo_root: &Path,
    evidence_root: &Path,
    command_dir: &Path,
    after_dir: F,
) -> Result<CommandOutcome, String> {
    if command_dir.exists() {
        return Err(format!(
            "command dir {} already exists; refusing to reuse it (one run = one fresh \
             directory — reusing would mix this run's logs with history)",
            command_dir.display()
        ));
    }
    std::fs::create_dir_all(command_dir)
        .map_err(|e| format!("cannot create {}: {e}", command_dir.display()))?;
    after_dir(command_dir);

    // F04 pre-spawn freshness check: every declared evidence path must be
    // absent NOW. A pre-existing file can only be stale (from an earlier
    // run or a previous candidate); running anyway would let it mask this
    // run's missing output as a PASS.
    let evidence_preexisting: Vec<String> = spec
        .evidence_paths
        .iter()
        .filter(|entry| resolve_evidence_path(entry, repo_root, evidence_root).exists())
        .cloned()
        .collect();
    if !evidence_preexisting.is_empty() {
        let note = format!(
            "refused to run: declared evidence already exists before this run \
             (stale evidence would mask the result): {evidence_preexisting:?}\n"
        );
        // Record the refusal in the command's own logs — auditable, loud,
        // and the command was never spawned.
        let _ = std::fs::write(command_dir.join("stdout.log"), "");
        let _ = std::fs::write(command_dir.join("stderr.log"), &note);
        return Ok(CommandOutcome {
            key: spec.key.clone(),
            exit_code: None,
            timed_out: false,
            internal_error: None,
            evidence_missing: Vec::new(),
            evidence_preexisting,
            cleanup: None,
        });
    }

    let argv: Vec<String> = spec
        .argv
        .iter()
        .map(|part| substitute(part, repo_root, evidence_root))
        .collect();
    let (program, args) = argv.split_first().ok_or_else(|| "empty argv".to_string())?;

    let stdout_path = command_dir.join("stdout.log");
    let stderr_path = command_dir.join("stderr.log");
    let stdout_file = std::fs::File::create(&stdout_path)
        .map_err(|e| format!("cannot create {}: {e}", stdout_path.display()))?;
    let stderr_file = std::fs::File::create(&stderr_path)
        .map_err(|e| format!("cannot create {}: {e}", stderr_path.display()))?;

    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(repo_root)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .stdin(Stdio::null());
    // R9-F02: SESSION boundary. `process_group(0)` alone kept the child in
    // the CALLER's session, and setpgid(2) allows any process in the SAME
    // session to join an existing process group — so a group id alone
    // could never prove that every member belonged to this run (a
    // same-session foreign process could join and then receive our group
    // signals). The child is now created as its own SESSION leader via
    // setsid(2): session membership is only ever inherited by fork or
    // created by a process's own setsid (which always creates a NEW
    // session — joining an existing session is impossible), so this
    // session contains only the root and its descendants, and the root's
    // process group can never be joined from outside. Group membership
    // therefore PROVES descent from our root, and the timeout cleanup's
    // group signals remain kernel-resolved while being provably addressed
    // only at this run's own objects. setsid also makes the root the
    // leader of a fresh process group (replacing process_group(0)).
    // Descendants may still call setsid themselves and LEAVE — that
    // honesty boundary is reported conservatively (group-leaver survivor
    // reporting below), never assumed clean.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot spawn {program:?}: {e}"))?;

    let deadline = Instant::now() + Duration::from_secs(spec.timeout_secs);
    let mut exit_code = None;
    let mut timed_out = false;
    let mut cleanup = None;
    loop {
        let waited = match child.try_wait() {
            Ok(waited) => waited,
            Err(e) => {
                let cleanup = cleanup_process_tree(&mut child);
                return Ok(CommandOutcome {
                    key: spec.key.clone(),
                    exit_code: None,
                    timed_out: false,
                    internal_error: Some(format!("wait failed: {e}")),
                    evidence_missing: spec.evidence_paths.clone(),
                    evidence_preexisting,
                    cleanup: Some(cleanup),
                });
            }
        };
        match waited {
            Some(status) => {
                exit_code = status.code();
                break;
            }
            None if Instant::now() >= deadline => {
                timed_out = true;
                // F05: terminate the whole process tree with an observable,
                // bounded cleanup (TERM sweep → ≤3 s grace with re-sweeps →
                // KILL sweep → ≤2 s settle → reap → survivor probe). The
                // pre-fix bare `child.kill()` relied on the shell's EXIT
                // trap for the rest of the tree — a killed shell never ran
                // it, so grandchildren survived the gate.
                cleanup = Some(cleanup_process_tree(&mut child));
                break;
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }

    // Evidence check: declared files must EXIST after the run (freshness
    // was enforced pre-spawn, so anything present now was produced by
    // THIS run). Substituted paths are relative to the evidence root.
    let mut evidence_missing = Vec::new();
    for entry in &spec.evidence_paths {
        if !resolve_evidence_path(entry, repo_root, evidence_root).exists() {
            evidence_missing.push(entry.clone());
        }
    }

    Ok(CommandOutcome {
        key: spec.key.clone(),
        exit_code,
        timed_out,
        internal_error: None,
        evidence_missing,
        evidence_preexisting,
        cleanup,
    })
}

/// Resolves one declared evidence path (same substitution rule as the
/// post-run check): absolute after substitution, or relative to the
/// evidence root.
fn resolve_evidence_path(
    entry: &str,
    repo_root: &Path,
    evidence_root: &Path,
) -> std::path::PathBuf {
    let substituted = substitute(entry, repo_root, evidence_root);
    if Path::new(&substituted).is_absolute() {
        std::path::PathBuf::from(&substituted)
    } else {
        evidence_root.join(&substituted)
    }
}

// ── F05 process-tree cleanup (R8-F02 group-anchored ownership) ─────────────
//
// The stage map's commands are this repository's own gate scripts. When one
// times out, the direct child (a shell) may have live children of its own
// (`sleep`, a server, a cargo build). Killing only the shell leaks them: a
// killed shell never runs the EXIT trap the scripts use for their own
// cleanup. std exposes no process-group or tree primitive and the locked
// dependency set adds none, so the cleanup below uses the POSIX
// `ps`/`kill` utilities (fixed program names, never a shell).
//
// OWNERSHIP INVARIANTS (R8-F02 session-hardened in R9-F02; the R7 design
// discovered descendants with an independent `pgrep -P` snapshot and then
// read each number's `lstart` from a SECOND independent `ps` call — an
// interleaving that can register a foreign replacement's birth string as
// ours — and treated a second-granular `lstart` STRING as a per-pid
// credential that two objects born in the same second satisfy):
//   1. CREATION (R9-F02): [`run_command`] spawns every command with a
//      `setsid(2)` pre-exec hook — the direct child (root) IS the session
//      leader of a brand-new session AND the leader of a fresh process
//      group (group id == root pid). Session membership can only be
//      inherited by fork or created by a process's OWN setsid (which
//      always creates a NEW session — a process can never JOIN an
//      existing session), so the session contains exactly the root and
//      its descendants. setpgid(2) requires the joining process to be in
//      the SAME session as the target group; no foreign process is in
//      this session, therefore no foreign process can join the group:
//      `pgid == root` PROVES descent from our root (kernel truth that
//      also survives re-parenting of a member). A descendant may leave
//      via its own setsid — handled as a group LEAVER below, never
//      assumed clean.
//   2. IDENTITY HOLD: the root stays our UNREAPED direct child for the
//      whole cleanup — the FIRST and ONLY wait is the final reap. Alive
//      or zombie, the kernel keeps the root's pid AND its process group
//      assigned to OUR object: a zombie is still a group member, so the
//      group id cannot be reassigned while we hold the child unreaped.
//   3. SIGNALING: the ONLY signals are group signals `kill -SIG -<pgid>`
//      (plus the final `child.kill()` on our own `Child` handle). The
//      kernel resolves the member set at delivery time; no bare numeric
//      pid of a descendant is ever signalled, so a recycled number has no
//      path to our signal — and with the R9 session boundary the
//      delivered set is provably only this run's objects (invariant 1).
//      A member that exits stays a zombie under its (stopped-or-alive)
//      member parent or keeps its own group slot — either way the group
//      id stays pinned by the unreaped leader.
//   4. DISCOVERY/REPORTING: observability reads ONE consistent process
//      table per decision (`ps -axo pid=,ppid=,pgid=,stat=` — every row's
//      identity fields are captured together, never combined across
//      snapshots). Membership is recomputed from the CURRENT table only:
//      group members (pgid == root — proven ours by invariant 1, still
//      ours after re-parenting to launchd, R2-F03) plus rows still
//      reachable from the root by parent links inside that same table (a
//      descendant that left the group). Nothing is registered, cached, or
//      re-verified against a recorded credential: what a member was in an
//      EARLIER snapshot proves nothing and is never used.
//   5. RETIREMENT: a member that exits and is reaped vanishes from the
//      table (dropped — its number is retired with it). A number that
//      shows up with a foreign pgid/ppid in the CURRENT table is judged
//      by that table alone and is not ours. A group-LEAVER (still
//      parented under us but outside the group) can never be proven
//      gone after its parent dies — if one was ever observed, the report
//      says survivors, never assumes a clean tree.
//   6. ROOT EXIT: never observed via a reaping wait; a zombie root is a
//      non-live row (`stat` starting `Z`) in the snapshot. After the
//      final bounded reap nothing signals anyone — the number and the
//      group id may be recycled from that point on. R12-F01: the reap
//      itself is bounded (below); an unreapable root stays OUR reported
//      residue, it never blocks the cleanup forever.

/// One row of a single `ps` snapshot. All identity fields of one process
/// are captured TOGETHER in the same table — the R8-F02 fix for the
/// pgrep-then-ps interleaving that could pair one snapshot's pid with a
/// different object's identity from the next.
#[cfg(unix)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProcRow {
    pub(crate) pid: u32,
    pub(crate) ppid: u32,
    pub(crate) pgid: u32,
    pub(crate) stat: String,
}

/// Pure membership decision (unit-tested with synthetic tables; no
/// process, no signal): the rows of THIS snapshot that belong to the
/// runner rooted at `root` — group members (`pgid == root`, kernel truth
/// that survives re-parenting and — since the R9-F02 session boundary —
/// PROVABLY only this run's descendants, because no foreign process can
/// be in the root's private session or join its group) plus rows still
/// reachable from `root` by parent links inside this same table
/// (descendants that left the group). The reporting set NEVER feeds the
/// signal path: signals go to the group as a whole.
#[cfg(unix)]
pub(crate) fn tree_members(root: u32, table: &[ProcRow]) -> Vec<&ProcRow> {
    let mut reachable: Vec<u32> = vec![root];
    let mut changed = true;
    while changed {
        changed = false;
        for row in table {
            if reachable.contains(&row.pid) {
                continue;
            }
            if reachable.contains(&row.ppid) {
                reachable.push(row.pid);
                changed = true;
            }
        }
    }
    table
        .iter()
        .filter(|row| row.pgid == root || reachable.contains(&row.pid))
        .collect()
}

/// A live member: not a zombie. A zombie is a dead object whose number is
/// pinned until reaped; it needs no signal and is not a survivor.
#[cfg(unix)]
pub(crate) fn row_is_live(row: &ProcRow) -> bool {
    !row.stat.starts_with('Z')
}

/// True when a member row sits OUTSIDE the runner's group (a leaver —
/// e.g. a descendant that called setsid). Only reachable-by-parentage
/// rows can be leavers; their exit can never be proven once re-parented,
/// so observing one makes the final report say survivors.
#[cfg(unix)]
pub(crate) fn is_group_leaver(root: u32, row: &ProcRow) -> bool {
    row.pgid != root
}

/// R12-F01: bounded reap of the direct child. The old blocking
/// `child.wait()` had NO deadline — an unreapable direct child
/// (uninterruptible D-state) would hang the whole cleanup even though
/// every signal phase is bounded. Polls the non-blocking `try_wait`
/// under a ≤2 s budget (the same shape as the KILL settle): a reapable
/// (e.g. already-zombie) child returns immediately; at the deadline an
/// unreaped child is reported as residue (`false`), never waited on
/// indefinitely.
fn reap_child_bounded(child: &mut Child) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => {}
            Err(_) => return false,
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Terminates the direct child's whole process group after a timeout.
/// Bounded end to end: ≤3 s TERM grace, ≤2 s KILL settle, ≤2 s reap —
/// an unreapable direct child is reported (`reaped=false`) instead of
/// blocking the cleanup forever.
fn cleanup_process_tree(child: &mut Child) -> CleanupReport {
    let started = Instant::now();
    let root = child.id();

    // Non-unix platforms (R9-F02): no portable group/session tooling here,
    // so the tree beyond the direct child is NOT observable and was never
    // swept — completeness CANNOT be proven. Report the REAL capability
    // and results, fail closed: kill_sent only when the direct-child kill
    // actually succeeded, and survivors_after ALWAYS true (a clean tree
    // must never be claimed from an unobserved table — this matches the
    // CleanupReport contract on `survivors_after`).
    #[cfg(not(unix))]
    {
        let kill_ok = child.kill().is_ok();
        let reaped = reap_child_bounded(child);
        return CleanupReport {
            term_sent: false,
            kill_sent: kill_ok,
            elapsed_ms: started.elapsed().as_millis() as u64,
            reaped,
            survivors_after: true,
            // R10-F03: on this platform the tree beyond the direct child is
            // NOT observable at all — survivors are recorded as UNOBSERVED
            // (completeness unprovable), never as an observed live member
            // and never as a foreign mis-kill.
            survivors_observed: false,
            survivors_unobserved: true,
        };
    }

    #[cfg(unix)]
    {
        // Pre-TERM snapshot: the one observation point where a group
        // LEAVER is still visible under its (alive) parent — after the
        // TERM kills the parent the leaver re-parents away and its exit
        // can never be proven. Any unreadable snapshot anywhere makes
        // the final report say survivors (completeness is never assumed).
        let mut saw_group_leaver = false;
        let mut any_snapshot_failed = false;
        match snapshot_process_table() {
            Some(table) => {
                if tree_members(root, &table)
                    .iter()
                    .any(|row| is_group_leaver(root, row))
                {
                    saw_group_leaver = true;
                }
            }
            None => any_snapshot_failed = true,
        }
        let term_sent = signal_group(root, "-TERM");

        // ≤3 s of grace: watch ONE consistent snapshot at a time until no
        // LIVE member remains (a zombie root is not live). A snapshot
        // that cannot be read never widens any decision — the deadline
        // alone governs, and the KILL below is safe without any ps.
        // R10-F03: `grace_expired` is the CONTROL-FLOW flag (enter the
        // KILL phase); it is deliberately NOT the `kill_sent` report
        // field — the report only records a KILL that was actually
        // DELIVERED (see below).
        let grace_deadline = started + Duration::from_secs(3);
        let mut grace_expired = false;
        loop {
            std::thread::sleep(Duration::from_millis(100));
            match snapshot_process_table() {
                Some(table) => {
                    let members = tree_members(root, &table);
                    if members.iter().any(|row| is_group_leaver(root, row)) {
                        saw_group_leaver = true;
                    }
                    if !members.iter().any(|row| row_is_live(row)) {
                        break;
                    }
                }
                None => any_snapshot_failed = true,
            }
            if Instant::now() >= grace_deadline {
                grace_expired = true;
                break;
            }
        }

        // KILL phase (only when the grace expired): group KILLs until no
        // live member remains, ≤2 s. Members forked mid-cleanup inherit
        // the group and are caught by the next sweep; the group id stays
        // ours for every one of these calls (invariant 3). R10-F03:
        // `kill_sent` becomes true only when a `kill -KILL` invocation
        // SUCCEEDS — the previous code pre-wrote it at grace expiry and
        // discarded every real result, so a failing kill command (EPERM,
        // missing binary) or a group that vanished first was still
        // reported as "KILL sent". A failed delivery with members still
        // alive is caught by the survivor probe below (loud), never by
        // misreporting the send.
        let mut kill_sent = false;
        if grace_expired {
            let settle_deadline = Instant::now() + Duration::from_secs(2);
            loop {
                if signal_group(root, "-KILL") {
                    kill_sent = true;
                }
                std::thread::sleep(Duration::from_millis(100));
                match snapshot_process_table() {
                    Some(table) => {
                        if !tree_members(root, &table)
                            .iter()
                            .any(|row| row_is_live(row))
                        {
                            break;
                        }
                    }
                    None => any_snapshot_failed = true,
                }
                if Instant::now() >= settle_deadline {
                    break;
                }
            }
        }

        // Survivor probe BEFORE the reap: the unreaped root still pins the
        // group id and the parentage anchor, so this snapshot can still
        // prove membership. `saw_group_leaver` stays loud (a leaver's exit
        // can never be proven once its parent is gone), and an unreadable
        // table — here or in ANY earlier snapshot — is reported as
        // survivors: a clean tree is never ASSUMED. R10-F03: the two
        // causes are recorded SEPARATELY — a live member OBSERVED in the
        // final snapshot (`survivors_observed`) versus completeness that
        // could NOT be proven (`survivors_unobserved`: any failed
        // snapshot, including this final one, or a seen leaver) — so the
        // aggregate `survivors_after` never conflates "we saw it alive"
        // with "we could not look". Nothing here claims a FOREIGN
        // mis-kill: the session boundary keeps every group member
        // provably ours, and an unprovable exit stays our residue.
        let mut survivors_observed = false;
        let mut final_snapshot_failed = false;
        match snapshot_process_table() {
            Some(table) => {
                survivors_observed = tree_members(root, &table)
                    .iter()
                    .any(|row| row_is_live(row));
            }
            None => final_snapshot_failed = true,
        }
        let survivors_unobserved = any_snapshot_failed || final_snapshot_failed || saw_group_leaver;
        let survivors_after = survivors_observed || survivors_unobserved;

        // Reap the direct child when reapable (never leave a zombie we
        // can collect). This is the FIRST reap of the whole cleanup —
        // only now can the root number and the group id ever be
        // reassigned, and nothing signals either afterwards. R12-F01:
        // the blocking `wait()` here had NO deadline (an unreapable
        // D-state child would hang the cleanup past every bounded signal
        // phase above) — the bounded `try_wait` loop reports residue at
        // the budget instead.
        let _ = child.kill();
        let reaped = reap_child_bounded(child);

        CleanupReport {
            term_sent,
            kill_sent,
            elapsed_ms: started.elapsed().as_millis() as u64,
            reaped,
            survivors_after,
            survivors_observed,
            survivors_unobserved,
        }
    }
}

/// `ps -axo pid=,ppid=,pgid=,stat=` — ONE consistent process table: every
/// row's identity fields are captured together (never combined across
/// calls). `None` ⇒ the table could not be read or parsed — every
/// consumer must fail CLOSED on `None` (report survivors / let deadlines
/// govern), never assume content.
#[cfg(unix)]
fn snapshot_process_table() -> Option<Vec<ProcRow>> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid=,pgid=,stat="])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut rows = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut fields = line.split_whitespace();
        let parsed = (|| {
            let pid = fields.next()?.parse::<u32>().ok()?;
            let ppid = fields.next()?.parse::<u32>().ok()?;
            let pgid = fields.next()?.parse::<u32>().ok()?;
            let stat = fields.next()?.to_string();
            Some(ProcRow {
                pid,
                ppid,
                pgid,
                stat,
            })
        })();
        // A malformed or extra-column row means the layout drifted — the
        // whole snapshot fails closed instead of silently misparsing.
        let row = parsed?;
        if fields.next().is_some() {
            return None;
        }
        rows.push(row);
    }
    Some(rows)
}

/// Group signal: `kill <signal> -<pgid>`. The negative pid addresses the
/// whole process group (the kernel resolves the member set at delivery).
/// Safe for the entire cleanup because of the R9-F02 SESSION boundary:
/// the root was created with setsid (a fresh session no foreign process
/// can join — setpgid(2) requires same-session, and session membership
/// is only inherited by fork), so every member of this group is provably
/// a descendant of our root; and the group id stays pinned to our
/// unreaped root (invariants 2/3): a recycled NUMBER has no path into
/// this call.
#[cfg(unix)]
fn signal_group(root: u32, signal: &str) -> bool {
    Command::new("kill")
        .arg(signal)
        .arg(format!("-{root}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Runs the full stage: unique commands in first-reference order, then the
/// scenario roll-up. Returns the result JSON (not yet written to disk).
pub fn verify_stage(
    map: &StageMap,
    repo_root: &Path,
    evidence_root: &Path,
    tested_sha: &str,
    worktree_dirty: bool,
    started_at_unix_ms: u128,
) -> Result<Value, String> {
    verify_stage_with_checkpoint(
        map,
        repo_root,
        evidence_root,
        tested_sha,
        worktree_dirty,
        started_at_unix_ms,
        |_| {},
    )
}

/// 正式调用方在每条注册命令后拍摄候选快照，包括失败和超时分支。
/// 原入口保留给隔离的 runner 测试与按路径加载的探针。
pub fn verify_stage_with_checkpoint<F: FnMut(&str)>(
    map: &StageMap,
    repo_root: &Path,
    evidence_root: &Path,
    tested_sha: &str,
    worktree_dirty: bool,
    started_at_unix_ms: u128,
    after_command: F,
) -> Result<Value, String> {
    verify_stage_with_runner(
        VerifyInputs {
            map,
            repo_root,
            evidence_root,
            tested_sha,
            worktree_dirty,
            started_at_unix_ms,
        },
        after_command,
        run_command,
    )
}

struct VerifyInputs<'a> {
    map: &'a StageMap,
    repo_root: &'a Path,
    evidence_root: &'a Path,
    tested_sha: &'a str,
    worktree_dirty: bool,
    started_at_unix_ms: u128,
}

fn verify_stage_with_runner<F, G>(
    inputs: VerifyInputs<'_>,
    mut after_command: F,
    mut runner: G,
) -> Result<Value, String>
where
    F: FnMut(&str),
    G: FnMut(&crate::stage_map::CommandSpec, &Path, &Path, &Path) -> Result<CommandOutcome, String>,
{
    let VerifyInputs {
        map,
        repo_root,
        evidence_root,
        tested_sha,
        worktree_dirty,
        started_at_unix_ms,
    } = inputs;
    // R13-F01, before anything else: the stage map's declared supplemental
    // leaf set must EQUAL the R00 acceptance ledger's binding for this
    // stage. Running commands first and discovering a filtered map
    // afterwards would waste a run and, worse, let a doctored map lean on
    // the runner's legitimacy for the parts it did declare.
    let r00_expected = cross_check_supplemental_coverage(map, repo_root)?;

    // F04: one verify run writes one FRESH evidence root. A non-empty root
    // means stale evidence from an earlier run could mask this one — the
    // per-command freshness checks make the same guarantee for declared
    // evidence files, and refusing a reused root makes it true for the
    // tree as a whole. R14-F01 extends the same guarantee to LEAF-declared
    // evidence paths (the assertion-contract files and per-leaf pinned
    // artifacts): they must be absent NOW, so anything present later in
    // the run can only have been produced by THIS run's commands.
    if evidence_root.exists()
        && std::fs::read_dir(evidence_root)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false)
    {
        return Err(format!(
            "evidence root {} is not empty; refusing to reuse it (each verify run \
             writes a FRESH directory so stale evidence can never mask a run)",
            evidence_root.display()
        ));
    }
    std::fs::create_dir_all(evidence_root)
        .map_err(|e| format!("cannot create {}: {e}", evidence_root.display()))?;
    for leaf in &map.supplemental_leaves {
        let mut leaf_paths: Vec<&String> = leaf.evidence_paths.iter().collect();
        if let Some(contract) = leaf.assertion_contract.as_ref() {
            leaf_paths.push(&contract.evidence_path);
        }
        for entry in leaf_paths {
            let path = resolve_evidence_path(entry, repo_root, evidence_root);
            if path.exists() {
                return Err(format!(
                    "supplemental leaf {} declares evidence {entry:?} which already \
                     exists at {} before this run — stale leaf evidence would mask \
                     this run (refusing, R14-F01)",
                    leaf.id,
                    path.display()
                ));
            }
        }
    }

    // Execution order: first reference order of scenarios, then any
    // supplemental-leaf evidence command not already referenced (R13-F01:
    // a command that only a supplemental leaf binds must still RUN — an
    // unreferenced evidence command would otherwise be silently skipped
    // and its leaf could only ever fail on a missing outcome).
    let mut order: Vec<String> = Vec::new();
    for scenario in &map.scenarios {
        for key in &scenario.command_refs {
            if !order.contains(key) {
                order.push(key.clone());
            }
        }
    }
    for leaf in &map.supplemental_leaves {
        for key in &leaf.evidence_command_refs {
            if !order.contains(key) {
                order.push(key.clone());
            }
        }
        // R02-final group 1: a deferred leaf's NON-gating early-evidence
        // producers still RUN (health of the pre-implemented R07 behavior is
        // mandatory; their failures are command FAILs) — they are never
        // silently skipped either.
        for key in &leaf.early_evidence_command_refs {
            if !order.contains(key) {
                order.push(key.clone());
            }
        }
    }

    let mut command_json = Vec::new();
    let mut outcomes = Vec::new();
    for key in &order {
        let spec = map
            .commands
            .iter()
            .find(|c| &c.key == key)
            .ok_or_else(|| format!("internal: command {key:?} not in map"))?;
        let command_dir = evidence_root.join(&spec.key);
        println!(
            "xtask: verify-stage {} [{}] > {}",
            map.stage,
            spec.argv.join(" "),
            command_dir.display()
        );
        let started = Instant::now();
        let outcome = match runner(spec, repo_root, evidence_root, &command_dir) {
            Ok(outcome) => outcome,
            Err(err) => CommandOutcome {
                key: spec.key.clone(),
                exit_code: None,
                timed_out: false,
                internal_error: Some(err),
                evidence_missing: spec.evidence_paths.clone(),
                evidence_preexisting: Vec::new(),
                cleanup: None,
            },
        };
        after_command(&spec.key);
        let duration_ms = started.elapsed().as_millis() as u64;
        let passed = outcome.passed();
        println!(
            "xtask: verify-stage {} [{}] {}",
            map.stage,
            spec.key,
            if passed { "PASS" } else { "FAIL" }
        );
        command_json.push(json!({
            "key": spec.key,
            "argv": spec.argv,
            "exitCode": outcome.exit_code,
            "timedOut": outcome.timed_out,
            "internalError": outcome.internal_error,
            "durationMs": duration_ms,
            "evidencePaths": spec.evidence_paths,
            "missingEvidence": outcome.evidence_missing,
            "preExistingEvidence": outcome.evidence_preexisting,
            "cleanup": outcome.cleanup.as_ref().map(|report| json!({
                "termSent": report.term_sent,
                "killSent": report.kill_sent,
                "elapsedMs": report.elapsed_ms,
                "reaped": report.reaped,
                "survivorsAfter": report.survivors_after,
                // R10-F03: the survivors causes, listed separately —
                // observed-live vs unprovable-completeness — so the
                // aggregate is never the only thing a consumer sees.
                "survivorsObserved": report.survivors_observed,
                "survivorsUnobserved": report.survivors_unobserved,
            })),
            "status": if passed { "PASS" } else { "FAIL" },
        }));
        outcomes.push(outcome);
    }

    let scenario_json: Vec<Value> = map
        .scenarios
        .iter()
        .map(|scenario| {
            let passed = scenario.command_refs.iter().all(|key| {
                outcomes
                    .iter()
                    .find(|o| &o.key == key)
                    .is_some_and(|o| o.passed())
            });
            json!({
                "id": scenario.id,
                "requirement": scenario.requirement,
                "commandRefs": scenario.command_refs,
                "status": if passed { "PASS" } else { "FAIL" },
            })
        })
        .collect();

    // R13-F01 supplemental roll-up: every R00 REQUIRED_SUPPLEMENTAL leaf
    // bound to this stage gets a machine verdict from the REAL command
    // outcomes and evidence files. BLOCKED (stage-boundary conflict) and
    // FAIL (no legal evidence, or original assertions not holding as
    // recorded) both keep overall FAIL — the 16/16 base scenarios can
    // never again be the whole story.
    let mut supplemental_json = Vec::with_capacity(map.supplemental_leaves.len());
    let mut supplemental_pass = 0usize;
    let mut supplemental_deferred = 0usize;
    let mut supplemental_fail = 0usize;
    let mut supplemental_blocked = 0usize;
    let mut r00_mismatch = 0usize;
    let mut status_deferred_ids: Vec<String> = Vec::new();
    for leaf in &map.supplemental_leaves {
        let roll = roll_up_supplemental_leaf(leaf, &map.stage, &outcomes, repo_root, evidence_root);
        let status = roll.status;
        // R14-F01: surface the mirrored R00 binding next to the verdict so
        // a consumer sees WHICH original record this leaf was graded
        // against (the pre-run cross-check already guarantees equality —
        // reaching here means the mirror matched the ledgers).
        let mirror = r00_expected.iter().find(|e| e.id == leaf.id);
        let mirror_json = match mirror {
            Some(e) => json!({
                "featureId": e.feature_id,
                "kind": e.kind,
                "taskIds": e.task_ids,
                "executionStageIds": e.execution_stage_ids,
                "ledgerStatus": e.ledger_status,
                "resultIds": e.result_ids,
                "testIds": e.test_ids,
                "then": e.then,
                "assertions": e.assertions,
                "due": e.due,
            }),
            None => {
                r00_mismatch += 1;
                Value::Null
            }
        };
        println!(
            "xtask: verify-stage {} supplemental {} [{}] {}",
            map.stage, leaf.id, leaf.basis_kind, status
        );
        match status.as_str() {
            "PASS" => supplemental_pass += 1,
            "DEFERRED_TO_R07" | "DEFERRED_TO_LATER_STAGE" => {
                supplemental_deferred += 1;
                status_deferred_ids.push(leaf.id.clone());
            }
            "BLOCKED" => supplemental_blocked += 1,
            _ => supplemental_fail += 1,
        }
        // R02-final group 1 (V7): every leaf entry states its stage
        // ownership explicitly — the share kinds carry the R07 remainder as
        // `deferredToStage` so a consumer can never misread "R02 share
        // judged" as "whole leaf done"; deferred leaves list their R07
        // remainder cases with the observed (non-gating) values.
        let carries_r07_remainder = leaf.basis_kind == BASIS_R02_SHARE_SATISFIED
            || leaf.basis_kind == BASIS_DEFERRED_TO_R07;
        let stage_neutral = is_stage_neutral_kind(&leaf.basis_kind);
        supplemental_json.push(json!({
            "id": leaf.id,
            "featureId": leaf.feature_id,
            "requirement": leaf.requirement,
            "basisKind": leaf.basis_kind,
            "deferredToStage": if carries_r07_remainder { json!("R07") } else { Value::Null },
            "deferredToStages": if stage_neutral {
                json!(later_stages_of(leaf, &map.stage))
            } else {
                Value::Null
            },
            "r00Mirror": mirror_json,
            "r02Share": leaf.r02_share,
            "r07Share": leaf.r07_share,
            "stageShare": leaf.stage_share,
            "laterShare": leaf.later_share,
            "evidenceRequired": leaf.evidence_required,
            "evidenceCommandRefs": leaf.evidence_command_refs,
            "earlyEvidenceCommandRefs": leaf.early_evidence_command_refs,
            "evidencePaths": leaf.evidence_paths,
            "assertionContract": leaf.assertion_contract.as_ref().map(|c| json!({
                "producerCommand": c.producer_command,
                "evidencePath": c.evidence_path,
                "cases": c.cases.iter().map(|p| json!({
                    "case": p.case,
                    "expect": p.expect,
                })).collect::<Vec<_>>(),
            })),
            "assertionResults": roll.assertion_results,
            "deferredR07Cases": roll.deferred_case_results,
            "earlyEvidence": roll.early_evidence,
            "originalAssertionCases": leaf.original_assertion_cases.iter().enumerate()
                .map(|(index, cases)| json!({
                    "r00AssertionIndex": index,
                    "r00Assertion": leaf.r00_assertions.get(index),
                    "cases": cases,
                }))
                .collect::<Vec<_>>(),
            "status": status,
            "reason": roll.reason,
        }));
    }
    // Defensive: the pre-run cross-check guarantees every declared leaf
    // has an R00 record; a miss here means that invariant broke.
    if r00_mismatch != 0 {
        return Err(format!(
            "internal: {r00_mismatch} supplemental leaf/leaves had no R00 expectation \
             after the pre-run cross-check — invariant violation, failing closed"
        ));
    }
    // Defense in depth (V3): the DEFERRED set in the result must EQUAL the
    // set declared deferred_to_r07 in the stage map — the status derives
    // from the basis kind, but a mismatch here means an invariant broke and
    // must abort, never pass.
    let mut declared_deferred_ids: Vec<String> = map
        .supplemental_leaves
        .iter()
        .filter(|leaf| {
            leaf.basis_kind == BASIS_DEFERRED_TO_R07
                || leaf.basis_kind == BASIS_DEFERRED_TO_LATER_STAGE
        })
        .map(|leaf| leaf.id.clone())
        .collect();
    status_deferred_ids.sort();
    declared_deferred_ids.sort();
    if declared_deferred_ids != status_deferred_ids {
        return Err(format!(
            "internal: the DEFERRED_TO_R07 leaf set ({:?}) does not equal the \
             deferred_to_r07 declarations in the stage map ({:?}) — invariant violation, \
             failing closed",
            status_deferred_ids, declared_deferred_ids
        ));
    }
    // 覆盖计数：期望总数 = pass + deferred + fail + blocked。
    let supplemental_total = map.supplemental_leaves.len();
    if supplemental_pass + supplemental_deferred + supplemental_fail + supplemental_blocked
        != supplemental_total
    {
        return Err(format!(
            "internal: supplemental leaf counts (pass {} + deferred {} + fail {} + blocked {}) \
             do not add up to the {} declared leaves — invariant violation, failing closed",
            supplemental_pass,
            supplemental_deferred,
            supplemental_fail,
            supplemental_blocked,
            supplemental_total
        ));
    }
    // PASS∪DEFERRED 全覆盖才允许 overall PASS；deferred 集合与声明全等已
    // 上面核对。另：所有已执行命令必须全部通过——提前实现的
    // early-evidence 生产者（client/static 矩阵）失败仍是命令 FAIL，
    // 已提交的提前实现代码必须保持健康，不得静默变绿。
    let commands_not_passing: Vec<&str> = outcomes
        .iter()
        .filter(|outcome| !outcome.passed())
        .map(|outcome| outcome.key.as_str())
        .collect();
    let supplemental_covered = supplemental_pass + supplemental_deferred == supplemental_total
        && supplemental_total == r00_expected.len();

    let overall_pass = scenario_json.iter().all(|s| s["status"] == "PASS")
        && supplemental_covered
        && commands_not_passing.is_empty();
    let finished = crate::now_ms();

    Ok(json!({
        "resultVersion": RESULT_VERSION,
        "stage": map.stage,
        "testedSha": tested_sha,
        "worktreeDirty": worktree_dirty,
        "platform": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "family": std::env::consts::FAMILY,
        },
        "toolchainChannel": read_toolchain_channel(repo_root),
        "startedAtUnixMs": started_at_unix_ms,
        "finishedAtUnixMs": finished,
        "commands": command_json,
        "scenarios": scenario_json,
        "supplementalLeafScenarios": supplemental_json,
        "supplementalLeafCoverage": {
            "r00LedgerPath": R00_ACCEPTANCE_MAP_PATH,
            "r00FeatureStageAcceptancePath": R00_FEATURE_STAGE_ACCEPTANCE_PATH,
            "expectedFromR00Ledger": r00_expected.len(),
            "declaredInStageMap": map.supplemental_leaves.len(),
            "pass": supplemental_pass,
            "deferredToR07": map
                .supplemental_leaves
                .iter()
                .filter(|leaf| leaf.basis_kind == BASIS_DEFERRED_TO_R07)
                .count(),
            "deferredToLaterStage": map
                .supplemental_leaves
                .iter()
                .filter(|leaf| leaf.basis_kind == BASIS_DEFERRED_TO_LATER_STAGE)
                .count(),
            "fail": supplemental_fail,
            "blocked": supplemental_blocked,
            "deferredLeafIds": declared_deferred_ids,
            "shareSatisfiedLeafIds": map
                .supplemental_leaves
                .iter()
                .filter(|leaf| {
                    leaf.basis_kind == BASIS_R02_SHARE_SATISFIED
                        || leaf.basis_kind == BASIS_STAGE_SHARE_SATISFIED
                })
                .map(|leaf| leaf.id.as_str())
                .collect::<Vec<_>>(),
            "commandsNotPassing": commands_not_passing,
            "note": "R13-F01/R14-F01: this stage gate CONSUMES the R00 \
                     REQUIRED_SUPPLEMENTAL leaves bound to the stage (cross-checked for \
                     set equality AND per-leaf FULL-record equality — feature, kind, \
                     task/stage responsibility, status, result/test registries, then, \
                     assertions, due — against both R00 ledgers before any command \
                     ran). R02-final group 1 stage ownership: expected == pass + \
                     deferredToR07 + fail + blocked, and overall PASS requires \
                     PASS ∪ DEFERRED_TO_R07 to cover every leaf. DEFERRED_TO_R07 is \
                     NOT a waiver: the leaf stays REQUIRED and its acceptance \
                     belongs to R07, which must consume the remainder (deferredToStage \
                     + r07Share on each entry); the deferred set is verified EQUAL to \
                     the stage map's deferred_to_r07 declarations. Early-evidence \
                     producers of pre-implemented R07 behavior stay registered and \
                     run; any failing command (commandsNotPassing) still blocks \
                     overall PASS. A BLOCKED entry is an unresolved stage-boundary \
                     conflict, a generic-authentication leaf whose cases do not \
                     prove its original behavior, or a legacy partial-kind dual-stage \
                     leaf. A FAIL entry lacks legal evidence for its R02 share or its \
                     pinned cases did not hold as recorded in the producer's \
                     structured case file.",
        },
        "overall": if overall_pass { "PASS" } else { "FAIL" },
    }))
}

fn read_toolchain_channel(repo_root: &Path) -> Value {
    let toolchain = repo_root.join("rust-toolchain.toml");
    match std::fs::read_to_string(&toolchain) {
        Ok(text) => text
            .lines()
            .find_map(|line| {
                let line = line.trim();
                line.strip_prefix("channel = ")
                    .map(|rest| Value::String(rest.trim_matches('"').to_string()))
            })
            .unwrap_or(Value::Null),
        Err(_) => Value::Null,
    }
}

#[cfg(test)]
mod runner_tests;
