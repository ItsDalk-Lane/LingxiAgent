#!/usr/bin/env python3
"""Generate the 130 R00 REQUIRED_SUPPLEMENTAL leaves bound to R05 and merge
them into rust/crates/xtask/src/stage_maps/R05.json (R05-T08).

The leaves are mirrored VERBATIM from both R00 ledgers (ACCEPTANCE_MAP.json
for identity/responsibility, FEATURE_STAGE_ACCEPTANCE.json for assertions
and the due line) — the xtask cross-check compares every mirror field
before any command runs.

Stage-share policy (R05 §6 split): every R05-bound leaf's R05 share is the
MODEL-PLANE substrate the leaf's product behavior consumes — the model
routing/capability resolution (T01), credential resolution (T02), protocol
families + tool-result round-trip (T03), operation adapters + worker
callbacks (T06) or the usage/trace ledger (T07) that later stages (R06/R07)
build the leaf's user-facing behavior on. That substrate is delivered and
machine-verified by this gate: each leaf pins ONE named case whose actual
is recomputed per run by the r05_stage_suites producer (the case is green
iff the leaf's mapped suite executed its EXACT pinned count green). The
leaf's own product behavior stays REQUIRED and belongs to its later stage
(named in laterShare) — nothing is "继承完成".

Run:  python3 scripts/rust-tauri/r05_t08_generate_leaves.py
"""
import json
import pathlib

REPO = pathlib.Path(__file__).resolve().parents[2]
MAP = REPO / "rust/crates/xtask/src/stage_maps/R05.json"

acceptance = json.loads((REPO / "docs/rust-tauri/R00/ACCEPTANCE_MAP.json").read_text())
fsa = json.loads((REPO / "docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json").read_text())
fsa_by_id = {e["id"]: e for e in fsa["supplemental_scenarios"]}

r05_leaves = {
    sid: entry
    for sid, entry in acceptance["scenarios"].items()
    if "R05" in entry.get("execution_stage_ids", [])
}
assert len(r05_leaves) == 130, f"expected 130 R05-bound leaves, got {len(r05_leaves)}"

PRODUCER = "r05_stage_suites"
CASES_PATH = "{EVIDENCE}/R05_SUITES/leaf-cases.json"
EVIDENCE_NOTE = (
    "本阶段份额由 r05_stage_suites 生产者在每次门禁运行时重算：案例绿=该叶映射套件"
    "在本轮以精确钉数全绿（lingxi.leaf-case-results.v1，actual==图钉）；份额图钉全过"
    "即 R05 份额 PASS，后续阶段份额仍是 REQUIRED（不继承完成）"
)

# per-task-family share/later texts; a leaf's group is its FIRST R05 task id
FAMILY = {
    "R05-T01": {
        "primary": "svc:r05_t01_model_plane",
        "stage": (
            "R05 份额=模型面机制：该叶消费的统一 ModelGateway 路由/能力解析/凭证解析与"
            "最小交换契约在真实配置链上可用（chat 与辅助槽、同名模型 pin-pair、"
            "不支持能力零请求、reload 代次），由 r05_t01_model_plane 套件真实链钉住"
        ),
        "later": "叶自身的用户可见行为/外围接入=后续阶段（按 r00ExecutionStageIds 归属），本阶段不判其产品形态",
    },
    "R05-T02": {
        "primary": "svc:r05_t02_credentials",
        "stage": (
            "R05 份额=凭证面机制：该叶消费的 per-provider 凭证解析/刷新协调/撤销栅栏/"
            "脱敏经 CredentialService 唯一出口，由 r05_t02_credentials 套件真实链钉住"
        ),
        "later": "叶自身的用户可见行为/外围接入=后续阶段（按 r00ExecutionStageIds 归属），本阶段不判其产品形态",
    },
    "R05-T03": {
        "primary": "svc:r05_t03_protocol_adapters",
        "stage": (
            "R05 份额=协议面机制：该叶消费的五族协议真实编码解码/工具声明与结果回传/"
            "外部↔内部调用 ID 关联/opaque 状态保真，由 r05_t03_protocol_adapters 套件"
            "真实链钉住"
        ),
        "later": "叶自身的用户可见行为/外围接入=后续阶段（按 r00ExecutionStageIds 归属），本阶段不判其产品形态",
    },
    "R05-T06": {
        "primary": "adp:r05_t06_operations",
        "stage": (
            "R05 份额=操作面机制：该叶消费的 embedding/speech/transcribe/media/rerank "
            "操作适配与 worker 模型回调（同一配额/预算/凭证），由 r05_t06_operations "
            "套件真实链钉住"
        ),
        "later": "叶自身的用户可见行为/外围接入=后续阶段（按 r00ExecutionStageIds 归属），本阶段不判其产品形态",
    },
    "R05-T07": {
        "primary": "svc:r05_t07_usage_trace",
        "stage": (
            "R05 份额=用量面机制：该叶消费的 ModelCall 关联/usage 归一四态/owner 范围"
            "查询与先提交后发布，由 r05_t07_usage_trace 套件真实链钉住"
        ),
        "later": "叶自身的用户可见行为/外围接入=后续阶段（按 r00ExecutionStageIds 归属），本阶段不判其产品形态",
    },
}

leaves = []
for sid, entry in sorted(r05_leaves.items()):
    fsa_entry = fsa_by_id.get(sid)
    assert fsa_entry is not None, f"{sid} missing from FEATURE_STAGE_ACCEPTANCE"
    tasks = [t for t in entry["task_ids"] if t.startswith("R05-")]
    assert tasks, f"{sid} has no R05 task id"
    family = FAMILY[tasks[0]]
    suffix = sid.split("-")[-1]
    case = f"r05-share-{suffix}"
    leaves.append({
        "id": sid,
        "featureId": entry["feature_id"],
        "requirement": entry["requirement"],
        "basisKind": "stage_share_satisfied",
        "stageShare": family["stage"],
        "laterShare": family["later"],
        "evidenceRequired": EVIDENCE_NOTE,
        "evidenceCommandRefs": [PRODUCER],
        "r00Kind": entry["kind"],
        "r00TaskIds": entry["task_ids"],
        "r00ExecutionStageIds": entry["execution_stage_ids"],
        "r00LedgerStatus": entry["ledger_status"],
        "r00ResultIds": entry["result_ids"],
        "r00TestIds": entry["test_ids"],
        "r00Then": fsa_entry["then"],
        "r00Assertions": fsa_entry["assertions"],
        "r00Due": fsa_entry["due"],
        "assertionContract": {
            "producerCommand": PRODUCER,
            "evidencePath": CASES_PATH,
            "cases": [{"case": case, "expect": 1}],
        },
    })

stage_map = json.loads(MAP.read_text())
assert "supplementalLeafScenarios" not in stage_map or not stage_map["supplementalLeafScenarios"], \
    "R05 map already carries leaves; regenerate from a leaf-free map"
stage_map["supplementalLeafScenarios"] = leaves
stage_map["supplementalCoverageNote"] = (
    stage_map["supplementalCoverageNote"]
    + " R00 台账绑定 R05 的 130 个 REQUIRED_SUPPLEMENTAL 叶全部镜像声明：每叶 R05 份额"
    "=其消费的模型面基底（T01 路由/T02 凭证/T03 协议/T06 操作/T07 用量），由生产者的"
    "leaf-cases（lingxi.leaf-case-results.v1）逐叶钉住（案例绿=映射套件本轮精确钉数"
    "全绿）；叶自身产品行为仍是后续阶段的 REQUIRED 义务（laterShare），不继承完成。"
)
MAP.write_text(json.dumps(stage_map, indent=2, ensure_ascii=False) + "\n")
print(f"R05 map now carries {len(leaves)} supplemental leaves "
      f"(families: " + ", ".join(f"{k}:{sum(1 for l in leaves if l['r00TaskIds'][0] == k or any(t == k for t in l['r00TaskIds'] if t.startswith('R05')))}" for k in FAMILY) + ")")
