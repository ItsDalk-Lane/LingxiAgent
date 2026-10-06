#!/usr/bin/env python3
"""Generate the 130 R00 REQUIRED_SUPPLEMENTAL leaves bound to R05, the
leaf-case TSV, and merge both into the R05 stage map (R05-T08; R05 RR1 F25
rewrote the share policy for the six R05-EXCLUSIVE OAuth leaves).

The leaves are mirrored VERBATIM from both R00 ledgers (ACCEPTANCE_MAP.json
for identity/responsibility, FEATURE_STAGE_ACCEPTANCE.json for assertions
and the due line) — the xtask cross-check compares every mirror field
before any command runs.

Stage-share policy (R05 §6 split, post-F25):
- A leaf whose R00 execution_stage_ids name a LATER stage declares
  stage_share_satisfied: its R05 share is the MODEL-PLANE substrate the
  leaf's product behavior consumes (routing/capabilities T01, credentials
  T02, protocol families T03, operation adapters + worker callbacks T06,
  usage/trace T07), delivered and machine-verified by this gate — each
  share leaf pins ONE named suite-level case (green iff the mapped suite
  ran its EXACT pinned count green). The leaf's own product behavior stays
  REQUIRED and belongs to its later stage (laterShare) — nothing is
  "继承完成".
- A leaf EXCLUSIVE to R05 (execution_stage_ids == ["R05"]) declares
  full_original_behavior: every original R00 assertion is pinned by its OWN
  named case whose evidence is a NAMED TEST of the mapped suite (the
  leafcase line carries the test name; the producer computes actual from
  that test's `... ok` line, never from suite-level green). The six
  R05-only OAuth leaves (G01) are the historical offenders the frozen
  candidate mis-released as shares with deferredToStages=[].

Run:  python3 scripts/rust-tauri/r05_t08_generate_leaves.py
"""
import json
import pathlib

REPO = pathlib.Path(__file__).resolve().parents[2]
MAP = REPO / "rust/crates/xtask/src/stage_maps/R05.json"
LEAF_TSV = REPO / "docs/rust-tauri/R05/r05_leaf_case_map.tsv"

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
FULL_EVIDENCE_NOTE = (
    "R05 独占叶（r00ExecutionStageIds 仅 R05）按 full_original_behavior 验收：每条原"
    "断言各钉一个具名案例，案例证据=映射套件里具名测试的本轮 `... ok` 行（由"
    " r05_stage_suites 生产者按 r05_leaf_case_map.tsv 的测试级 leafcase 行重算，"
    "绝不以整套件全绿代替）"
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

# ── the six R05-EXCLUSIVE OAuth leaves (G01/F25): per-assertion named tests ──
# Each entry: leaf id -> one test name per original R00 assertion, in order.
# The tests live in svc:r05_t02_credentials (the F04 closed-loop battery).
FULL_LEAVES = {
    # 添加 OAuth 自定义模型：A1 成功加入+刷新清单；A2 边界（空 id/非 OAuth 拒绝、
    # 不扩权不扩散）——同一测试的两个断言段。
    "R00-T02-LA-16CEB6D12A6A": [
        "rr1_f04::rr1_f04_add_custom_model_id_refreshes_and_persists",
        "rr1_f04::rr1_f04_add_custom_model_id_refreshes_and_persists",
    ],
    # 删除 OAuth 自定义模型：A1 移除+刷新；A2 边界（provider 必须 OAuth、id 来自
    # 路径、不扩权）。
    "R00-T02-LA-8060BE8AA02C": [
        "rr1_f04::rr1_f04_remove_custom_model_id_refreshes_the_list",
        "rr1_f04::rr1_f04_remove_custom_model_id_refreshes_the_list",
    ],
    # 列出 OAuth 模型：A1 返回清单；A2 非 OAuth 拒绝（R00 原文 404；实现为
    # 409+oauth_only_surface——RR1_LEAF_DEVIATIONS.md 条目 1 的已登记偏差，语义
    # 类“明确拒绝”由同一测试钉住）。
    "R00-T02-LA-CFEC64F68DDE": [
        "rr1_f04::rr1_f04_oauth_model_listing_and_non_oauth_rejection",
        "rr1_f04::rr1_f04_oauth_model_listing_and_non_oauth_rejection",
    ],
    # 登录流：A1 start 指引未称登录；A2 手输码 callback 刷新（有效腿在 A1 测试内）
    # 与无效/过期拒绝；A3 设备码 pending→done 刷新、error 不假登录。
    "R00-T02-LA-99D6C304D697": [
        "rr1_f04::rr1_f04_pkce_start_manual_callback_install_and_real_model_call",
        "rr1_f04::rr1_f04_expired_state_refuses_even_with_the_correct_state",
        "rr1_f04::rr1_f04_device_start_poll_done_then_logout",
    ],
    # 状态：A1 loggedIn+可用模型数；A2 非 oauth 凭证不视为登录（apiKey 行
    # loggedIn=false）。
    "R00-T02-LA-CA0BF9A7AEA9": [
        "rr1_f04::rr1_f04_status_reports_logged_in_and_available_model_counts",
        "rr1_f04::rr1_f04_status_reports_logged_in_and_available_model_counts",
    ],
    # 注销：A1 删凭证+清缓存+刷新清单；A2 未登录注销/失败路径结果诚实。
    "R00-T02-LA-FC80B6C4FBE4": [
        "rr1_f04::rr1_f04_logout_clears_credentials_cache_and_refreshes_models",
        "rr1_f04::rr1_f04_logout_clears_credentials_cache_and_refreshes_models",
    ],
}
DEVIATION_NOTES = {
    "R00-T02-LA-CFEC64F68DDE": (
        " 偏差登记：原断言的非 OAuth 边界为 404，实现为 409+oauth_only_surface"
        "（docs/rust-tauri/R05/repair-current/RR1_LEAF_DEVIATIONS.md 条目 1：总控"
        " 2026-10-04 F04 文本要求“非 OAuth 明确拒绝”未规定状态码；404 会掩盖已知"
        " provider 存在）——具名测试按“明确拒绝”语义类钉住该边界。"
    ),
}
FULL_RUN = "svc:r05_t02_credentials"

leaves = []
leafcase_lines = []
for sid, entry in sorted(r05_leaves.items()):
    fsa_entry = fsa_by_id.get(sid)
    assert fsa_entry is not None, f"{sid} missing from FEATURE_STAGE_ACCEPTANCE"
    tasks = [t for t in entry["task_ids"] if t.startswith("R05-")]
    assert tasks, f"{sid} has no R05 task id"
    stages = entry["execution_stage_ids"]
    suffix = sid.split("-")[-1]
    exclusive = stages == ["R05"]
    base = {
        "id": sid,
        "featureId": entry["feature_id"],
        "requirement": entry["requirement"],
        "r00Kind": entry["kind"],
        "r00TaskIds": entry["task_ids"],
        "r00ExecutionStageIds": entry["execution_stage_ids"],
        "r00LedgerStatus": entry["ledger_status"],
        "r00ResultIds": entry["result_ids"],
        "r00TestIds": entry["test_ids"],
        "r00Then": fsa_entry["then"],
        "r00Assertions": fsa_entry["assertions"],
        "r00Due": fsa_entry["due"],
    }
    if exclusive:
        assert sid in FULL_LEAVES, (
            f"{sid} is EXCLUSIVE to R05 but has no per-assertion test mapping — "
            "a share with no later stage would be an unowned remainder (F25)"
        )
        tests = FULL_LEAVES[sid]
        assert len(tests) == len(fsa_entry["assertions"]), (
            f"{sid}: {len(tests)} pinned tests for "
            f"{len(fsa_entry['assertions'])} original assertions"
        )
        cases = [f"r05-full-{suffix}-a{i + 1}" for i in range(len(tests))]
        note = FULL_EVIDENCE_NOTE + DEVIATION_NOTES.get(sid, "")
        leaves.append({
            **base,
            "basisKind": "full_original_behavior",
            "stageShare": "",
            "laterShare": "",
            "evidenceRequired": note,
            "evidenceCommandRefs": [PRODUCER],
            "originalAssertionCases": [[case] for case in cases],
            "assertionContract": {
                "producerCommand": PRODUCER,
                "evidencePath": CASES_PATH,
                "cases": [{"case": case, "expect": 1} for case in cases],
            },
        })
        for case, test in zip(cases, tests):
            leafcase_lines.append(f"leafcase {case} {FULL_RUN} {test}")
    else:
        family = FAMILY[tasks[0]]
        case = f"r05-share-{suffix}"
        leaves.append({
            **base,
            "basisKind": "stage_share_satisfied",
            "stageShare": family["stage"],
            "laterShare": family["later"],
            "evidenceRequired": EVIDENCE_NOTE,
            "evidenceCommandRefs": [PRODUCER],
            "assertionContract": {
                "producerCommand": PRODUCER,
                "evidencePath": CASES_PATH,
                "cases": [{"case": case, "expect": 1}],
            },
        })
        leafcase_lines.append(f"leafcase {case} {family['primary']}")

assert len(leaves) == 130, f"leaf count drifted: {len(leaves)}"
expected_lines = (130 - len(FULL_LEAVES)) + sum(len(v) for v in FULL_LEAVES.values())
assert len(leafcase_lines) == expected_lines, (
    f"leaf-case line count drifted: {len(leafcase_lines)} != {expected_lines}"
)
LEAF_TSV.write_text("\n".join(sorted(leafcase_lines)) + "\n")

stage_map = json.loads(MAP.read_text())
assert "supplementalLeafScenarios" not in stage_map or not stage_map["supplementalLeafScenarios"], \
    "R05 map already carries leaves; regenerate from a leaf-free map"
stage_map["supplementalLeafScenarios"] = leaves
stage_map["supplementalCoverageNote"] = (
    stage_map["supplementalCoverageNote"]
    + " R00 台账绑定 R05 的 130 个 REQUIRED_SUPPLEMENTAL 叶全部镜像声明：R05 独占叶"
    "（executionStageIds 仅 R05，六个 OAuth 叶）按 full_original_behavior 逐断言以具名"
    "测试钉住（leafcase 测试级行）；其余每叶 R05 份额=其消费的模型面基底（T01 路由"
    "/T02 凭证/T03 协议/T06 操作/T07 用量），由生产者的 leaf-cases"
    "（lingxi.leaf-case-results.v1）逐叶钉住（案例绿=映射套件本轮精确钉数全绿）；叶"
    "自身产品行为仍是后续阶段的 REQUIRED 义务（laterShare），不继承完成。"
)
MAP.write_text(json.dumps(stage_map, indent=2, ensure_ascii=False) + "\n")
full_count = sum(1 for l in leaves if l["basisKind"] == "full_original_behavior")
print(
    f"R05 map now carries {len(leaves)} supplemental leaves "
    f"({full_count} full_original_behavior / {len(leaves) - full_count} stage_share); "
    f"{len(leafcase_lines)} leafcase lines ({sum(len(v) for v in FULL_LEAVES.values())} "
    "test-level for the exclusive leaves)"
)
