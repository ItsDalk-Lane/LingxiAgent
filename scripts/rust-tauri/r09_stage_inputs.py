#!/usr/bin/env python3
"""生成 R09 阶段图与输入镜像；只镜像 R00 两本冻结原账字段。

R09 = Tauri 桌面宿主。本生成器负责：
  1) 从 R00 原账独立提取 16 个基础场景 + 74 个去重补充叶（T03/T07 共享 46 叶
     按叶 ID 去重），任何数量/集合不符立即失败——预期集合绝不从「当前剩余
     任务」反向生成；
  2) 把基础场景绑定到真实存在的生产者套件（scripts/rust-tauri/r09_t0*.py），
     套件未实现时生产者以 exit 2 失败（门红），不冒充通过；
  3) 补充叶：逐断言证据绑定登记在 FULL_BINDINGS（唯一来源）；未登记的叶保持
     required_behavior_pending（BLOCKED，永不计 PASS）；
  4) 生成 docs/rust-tauri/R09/R09_LEAF_LEDGER.json 机器清单：原 ID/全部断言 →
     实现入口 → 真实消费者 → 命令/证据 → 平台/状态；
  5) --check 自检：输入摘要 + 整图逐字节一致（手改地图/放宽绑定立即失败）。

用法：
  python3 -B scripts/rust-tauri/r09_stage_inputs.py                      # 生成
  python3 -B scripts/rust-tauri/r09_stage_inputs.py --check --evidence <dir>
  python3 -B scripts/rust-tauri/r09_stage_inputs.py --ledger-only        # 只重建台账
"""

from __future__ import annotations

import hashlib
import json
import os
import sys
from pathlib import Path

ROOT = Path(os.environ.get("LINGXI_ROOT", Path(__file__).resolve().parents[2]))
ACCEPTANCE_PATH = Path("docs/rust-tauri/R00/ACCEPTANCE_MAP.json")
FEATURE_PATH = Path("docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json")
MAP_PATH = Path("rust/crates/xtask/src/stage_maps/R09.json")
INPUT_PATH = Path("rust/crates/xtask/src/stage_maps/R09_INPUTS.json")
LEDGER_PATH = Path("docs/rust-tauri/R09/R09_LEAF_LEDGER.json")
BASELINE_PATH = Path("docs/rust-tauri/R00/BASELINE.json")

STAGE = "R09"
BASE_IDS = [f"R09-A{i:02}" for i in range(1, 17)]
TASKS = [f"R09-T{i:02}" for i in range(1, 9)]

# 任务 → 本阶段真实验证面（套件名 → 生产者脚本；每个脚本必须真实存在）。
TASK_SUITES = {
    "R09-T01": ["r09_t01_react_parity", "r09_t01_release_offline"],
    "R09-T02": ["r09_t02_supervisor_ready", "r09_t02_process_ownership"],
    "R09-T03": ["r09_t03_native_matrix", "r09_t03_ipc_negative"],
    "R09-T04": ["r09_t04_permission_matrix", "r09_t04_remote_boundary"],
    "R09-T05": ["r09_t05_browser_pdf", "r09_t05_untrusted_preview"],
    "R09-T06": ["r09_t06_capability_matrix", "r09_t06_release_attack_surface"],
    "R09-T07": ["r09_t07_native_e2e", "r09_t07_release_smoke"],
    "R09-T08": ["r09_t08_shell_parity", "r09_t08_host_completeness"],
}

# 阶段基础设施命令（不属于任务套件；由对应场景引用，反向验证正式门消费的
# 地图/证据合同本身不可被悄悄放宽）。证据由生产者真实写出。
EXTRA_COMMANDS = {
    "r09_service_seam_probe": {
        "argv": ["python3", "-B", "scripts/rust-tauri/r09_service_seam_probe.py",
                 "--evidence", "{EVIDENCE}/SUITES/r09_service_seam_probe"],
        "timeoutSecs": 300,
        "evidencePaths": [
            "{EVIDENCE}/SUITES/r09_service_seam_probe/result.json",
            "{EVIDENCE}/SUITES/r09_service_seam_probe/output.log",
        ],
    },
    "r09_capability_audit": {
        "argv": ["python3", "-B", "scripts/rust-tauri/r09_capability_audit.py",
                 "--evidence", "{EVIDENCE}/SUITES/r09_capability_audit"],
        "timeoutSecs": 300,
        "evidencePaths": [
            "{EVIDENCE}/SUITES/r09_capability_audit/result.json",
            "{EVIDENCE}/SUITES/r09_capability_audit/output.log",
            "{EVIDENCE}/SUITES/r09_capability_audit/cases.json",
        ],
    },
    "r09_gate_negative": {
        "argv": ["python3", "-B", "scripts/rust-tauri/r09_gate_negative_probe.py",
                 "--evidence", "{EVIDENCE}/SUITES/r09_gate_negative"],
        "timeoutSecs": 300,
        "evidencePaths": [
            "{EVIDENCE}/SUITES/r09_gate_negative/result.json",
            "{EVIDENCE}/SUITES/r09_gate_negative/output.log",
            "{EVIDENCE}/SUITES/r09_gate_negative/gate-negative-cases.json",
        ],
    },
    "r09_map_integrity": {
        "argv": ["python3", "-B", "scripts/rust-tauri/r09_map_integrity.py",
                 "--evidence", "{EVIDENCE}/SUITES/r09_map_integrity"],
        "timeoutSecs": 600,
        "evidencePaths": [
            "{EVIDENCE}/SUITES/r09_map_integrity/result.json",
            "{EVIDENCE}/SUITES/r09_map_integrity/output.log",
            "{EVIDENCE}/SUITES/r09_map_integrity/map-integrity-cases.json",
        ],
    },
}

# R09-A15 前置「对照双方 hash 已固定」的机器证据：地图合同完整性负例。
MAP_INTEGRITY_CASES = [
    "map::contract-valid",
    "map::base-scenario-set-16-equal-r00",
    "map::deduped-leaf-coverage-74-equal-r00",
    "map::original-assertions-168-verbatim",
    "map::task-share-counts",
    "map::dropped-leaf-rejected",
    "map::deleted-task-share-rejected",
    "map::deleted-shared-task-share-rejected",
    "map::unknown-leaf-rejected",
    "map::rewritten-original-assertion-rejected",
    "map::base-mirror-drift-rejected",
    "map::orphan-command-rejected",
    "map::missing-command-rejected",
    "map::empty-scenario-rejected",
    "map::incomplete-assertion-coverage-rejected",
    "map::missing-evidence-paths-rejected",
    "map::source-drift-rejected",
    "map::source-tree-restored",
    "map::restored-map-valid",
]

# 套件 → 生产者脚本（python）。统一 dispatcher 按套件前缀分派到
# scripts/rust-tauri/r09_t0X.py（任务模块，按需创建）；未实现套件由
# dispatcher 以 exit 2 + result.json status=NOT_IMPLEMENTED 失败（门红）。
SUITE_SCRIPTS = {suite: "scripts/rust-tauri/r09_suites.py"
                 for suites in TASK_SUITES.values() for suite in suites}

# 逐叶实现/证据绑定（唯一事实来源）。键 = R00 叶 ID；值：
#   producer       证据命令（必须 TASK_SUITES 内）
#   evidence_file  producer 目录下逐断言案例文件（schema lingxi.leaf-case-results.v1）
#   assertion_cases 与叶 r00Assertions 同序：每条原断言 → 专属 case 名列表
# 未登记的叶一律 required_behavior_pending（BLOCKED）。
FULL_BINDINGS: dict[str, dict] = {
    "R00-T02-LA-03A9E92909C6": {
        "producer": "r09_t05_browser_pdf",
        "evidence_file": "cases.json",
        "assertion_cases": [
            ["leaf:R00-T02-LA-03A9E92909C6::a1-clear-site-data-success-and-failure-receipt"],
            ["leaf:R00-T02-LA-03A9E92909C6::a2-clear-state-matches-actual-side-effect"],
        ],
    },
    "R00-T02-LA-178C8CBA29C2": {
        "producer": "r09_t05_browser_pdf",
        "evidence_file": "cases.json",
        "assertion_cases": [
            ["leaf:R00-T02-LA-178C8CBA29C2::a1-close-session-real-stop-and-state-broadcast"],
            ["leaf:R00-T02-LA-178C8CBA29C2::a2-close-invalid-unauthorized-downstream-explicit"],
        ],
    },
    "R00-T02-LA-1809C8E13EA3": {
        "producer": "r09_t05_browser_pdf",
        "evidence_file": "cases.json",
        "assertion_cases": [
            ["leaf:R00-T02-LA-1809C8E13EA3::a1-read-prefs-and-running-state-no-write"],
            ["leaf:R00-T02-LA-1809C8E13EA3::a2-put-apply-no-widened-write-or-disclosure"],
        ],
    },
    "R00-T02-LA-367052089D11": {
        "producer": "r09_t05_browser_pdf",
        "evidence_file": "cases.json",
        "assertion_cases": [
            ["leaf:R00-T02-LA-367052089D11::a1-states-running-resumable-unavailable-projection"],
            ["leaf:R00-T02-LA-367052089D11::a2-states-invalid-unauthorized-downstream-explicit"],
        ],
    },
    "R00-T02-LA-593534A4F2AB": {
        "producer": "r09_t05_browser_pdf",
        "evidence_file": "cases.json",
        "assertion_cases": [
            ["leaf:R00-T02-LA-593534A4F2AB::a1-sessions-list-cold-url-retained"],
            ["leaf:R00-T02-LA-593534A4F2AB::a2-sessions-invalid-unauthorized-explicit"],
        ],
    },
    "R00-T02-LA-7D212CBD9EEF": {
        "producer": "r09_t05_browser_pdf",
        "evidence_file": "cases.json",
        "assertion_cases": [
            ["leaf:R00-T02-LA-7D212CBD9EEF::a1-verify-success-persists-only-after-real-success"],
            ["leaf:R00-T02-LA-7D212CBD9EEF::a2-verify-reject-no-widened-persistence-visible-error"],
        ],
    },
    "R00-T02-LA-8A45B19A18B4": {
        "producer": "r09_t05_browser_pdf",
        "evidence_file": "cases.json",
        "assertion_cases": [
            ["leaf:R00-T02-LA-8A45B19A18B4::a1-real-back-forward-reload-url-title-flags"],
            ["leaf:R00-T02-LA-8A45B19A18B4::a2-no-active-view-and-illegal-urls-explicit"],
        ],
    },
    "R00-T02-LA-A09B78985A14": {
        "producer": "r09_t05_browser_pdf",
        "evidence_file": "cases.json",
        "assertion_cases": [
            ["leaf:R00-T02-LA-A09B78985A14::a1-emergency-stop-revoke-until-new-user-run"],
            ["leaf:R00-T02-LA-A09B78985A14::a2-no-legacy-local-instance-and-close-failure-visible"],
        ],
    },
    "R00-T02-LA-A30E4BDF2FA2": {
        "producer": "r09_t05_browser_pdf",
        "evidence_file": "cases.json",
        "assertion_cases": [
            ["leaf:R00-T02-LA-A30E4BDF2FA2::a1-navigation-title-and-snapshot-match"],
            ["leaf:R00-T02-LA-A30E4BDF2FA2::a2-stale-ref-explicit-failure"],
            ["leaf:R00-T02-LA-A30E4BDF2FA2::a3-session-a-state-not-leaked-to-b"],
            ["leaf:R00-T02-LA-A30E4BDF2FA2::a4-no-session-blocked-or-browser-error-explicit-no-cross-session"],
        ],
    },
    "R00-T02-LA-A67D342381C0": {
        "producer": "r09_t05_browser_pdf",
        "evidence_file": "cases.json",
        "assertion_cases": [
            ["leaf:R00-T02-LA-A67D342381C0::a1-tabs-and-active-sync-server-snapshot"],
            ["leaf:R00-T02-LA-A67D342381C0::a2-invalid-tab-and-close-failure-explicit"],
        ],
    },
    "R00-T02-LA-D1B56A275C3A": {
        "producer": "r09_t05_browser_pdf",
        "evidence_file": "cases.json",
        "assertion_cases": [
            ["leaf:R00-T02-LA-D1B56A275C3A::a1-search-real-links-and-provider"],
            ["leaf:R00-T02-LA-D1B56A275C3A::a2-no-config-not-fake-success"],
            ["leaf:R00-T02-LA-D1B56A275C3A::a3-missing-key-rate-limit-failure-explicit"],
        ],
    },
    "R00-T02-LA-DCD85A04A7E9": {
        "producer": "r09_t05_browser_pdf",
        "evidence_file": "cases.json",
        "assertion_cases": [
            ["leaf:R00-T02-LA-DCD85A04A7E9::a1-open-session-explicit-resume-cold-metadata"],
            ["leaf:R00-T02-LA-DCD85A04A7E9::a2-open-invalid-unauthorized-downstream-explicit"],
        ],
    },
    "R00-T02-LA-ED5FD5B469B8": {
        "producer": "r09_t05_browser_pdf",
        "evidence_file": "cases.json",
        "assertion_cases": [
            ["leaf:R00-T02-LA-ED5FD5B469B8::a1-update-prefs-save-and-return-new-state"],
            ["leaf:R00-T02-LA-ED5FD5B469B8::a2-apply-failure-400-readback-no-auto-rollback"],
        ],
    },
    "R00-T02-LA-F1A5597E3E78": {
        "producer": "r09_t05_browser_pdf",
        "evidence_file": "cases.json",
        "assertion_cases": [
            ["leaf:R00-T02-LA-F1A5597E3E78::a1-close-and-revoke-authority-real"],
            ["leaf:R00-T02-LA-F1A5597E3E78::a2-revoke-invalid-unknown-explicit"],
        ],
    },
    "R00-T02-LA-FD7A5A76B8EA": {
        "producer": "r09_t05_browser_pdf",
        "evidence_file": "cases.json",
        "assertion_cases": [
            ["leaf:R00-T02-LA-FD7A5A76B8EA::a1-fetch-body-and-final-url-session-file"],
            ["leaf:R00-T02-LA-FD7A5A76B8EA::a2-blocked-page-not-normal-content"],
            ["leaf:R00-T02-LA-FD7A5A76B8EA::a3-invalid-url-redirect-network-error-no-workspace-write"],
        ],
    },
}
# 47 个 T03/T07 共享叶与 19 个 T05 叶、9 个 T04 叶的分组元数据（供机器清单）。
_LEAF_GROUP_PLAN = {
    # execution_task_ids 元组 → 实现入口 / 真实消费者 / 平台范围 / 计划套件
    ("R09-T03", "R09-T07"): {
        "implementationEntry": "desktop/src-tauri/src/host/（Tauri commands+宿主状态） + desktop/src/react/host/tauri-desktop-host.ts（DesktopHost-Tauri）",
        "realConsumer": "desktop/src/react（现有业务组件经 getDesktopHost()/usePlatform() 消费；不重写组件）",
        "platformScope": ["macos-arm64"],
        "plannedSuite": "r09_t03_native_matrix",
        "notes": "46 叶共享 T03 实现与 T07 跨平台原生 E2E；本机仅 macos-arm64，其余平台 BLOCKED_PLATFORM。",
    },
    ("R01-T04", "R07-T04", "R09-T05"): {
        "implementationEntry": "rust/crates/lingxi-service（BrowserPort/受控 Chromium+CDP） + desktop/src-tauri/src/host/browser.rs（可见窗口/接管/急停桥）",
        "realConsumer": "desktop/src/react 浏览器 viewer UI + 服务工具调用（browser_* 工具）",
        "platformScope": ["macos-arm64"],
        "plannedSuite": "r09_t05_browser_pdf",
        "notes": "18 叶：浏览器宿主在 Rust 服务侧，Tauri 只做可见宿主接线与进程所有权；Windows/Linux 未验证。",
    },
    ("R04-T04", "R06-T05", "R09-T05"): {
        "implementationEntry": "desktop/src-tauri/src/host/files.rs（文件预览派发） + 现有 Resource/Office 服务链",
        "realConsumer": "desktop/src/react MediaViewer / 文件预览组件",
        "platformScope": ["macos-arm64"],
        "plannedSuite": "r09_t05_browser_pdf",
        "notes": "1 叶：文件预览（文本/Markdown/Office）路径在 Tauri 下不得依赖 Electron。",
    },
    ("R09-T04",): {
        "implementationEntry": "desktop/src-tauri/src/host/permissions.rs（TCC/系统权限+helper 租约） + Rust 授权边界",
        "realConsumer": "desktop/src/react 语音/截图/computer-use 设置与任务入口",
        "platformScope": ["macos-arm64"],
        "plannedSuite": "r09_t04_permission_matrix",
        "notes": "9 叶：语音/屏幕/辅助功能权限；真实撤销需人工系统设置操作（见 BLOCKED-SYSTEM-PERMISSIONS）。",
    },
}

PLATFORM_SCOPE_ORDER = ["macos-arm64", "macos-x64", "windows-x64", "linux-x64"]


def load(path: Path):
    return json.loads((ROOT / path).read_text(encoding="utf-8"))


def sha(value) -> str:
    return hashlib.sha256(
        json.dumps(value, ensure_ascii=False, sort_keys=True).encode()).hexdigest()


def canonical_tree() -> tuple[dict, dict, dict]:
    """独立读取冻结原账并做集合/数量断言；返回 (base, leaves, fsa_leaves)。"""
    acceptance = load(ACCEPTANCE_PATH)["scenarios"]
    feature_ledger = load(FEATURE_PATH)["supplemental_scenarios"]
    features = {row["id"]: row for row in feature_ledger}

    base = {k: v for k, v in acceptance.items()
            if v.get("kind") == "base" and v.get("stage_id") == STAGE}
    if sorted(base) != BASE_IDS:
        raise SystemExit(f"R09 基础场景集合与 R00 冻结原账不一致: {sorted(base)}")

    leaves = {k: v for k, v in acceptance.items()
              if v.get("kind") == "supplemental"
              and STAGE in (v.get("execution_stage_ids") or [])}
    fsa_leaves = {k: v for k, v in features.items()
                  if STAGE in (v.get("execution_stage_ids") or [])}
    if set(leaves) != set(fsa_leaves):
        only_a = sorted(set(leaves) - set(fsa_leaves))
        only_b = sorted(set(fsa_leaves) - set(leaves))
        raise SystemExit(f"R09 补充叶在两本冻结原账间不一致: acceptance-only={only_a} fsa-only={only_b}")
    if len(leaves) != 74:
        raise SystemExit(f"R09 去重补充叶数量应为 74，实为 {len(leaves)}")

    total_assertions = sum(len(fsa_leaves[lid]["assertions"]) for lid in leaves)
    if total_assertions != 168:
        raise SystemExit(f"R09 原断言总数应为 168，实为 {total_assertions}")

    # 任务去重口径：T03/T07 共享 46 叶只计一次（按叶 ID），T05 19、T04 9。
    by_task: dict[str, set[str]] = {task: set() for task in TASKS}
    for lid, row in leaves.items():
        for task in row.get("task_ids") or []:
            if task in by_task:
                by_task[task].add(lid)
    if len(by_task["R09-T03"]) != 46 or by_task["R09-T03"] != by_task["R09-T07"]:
        raise SystemExit("R09-T03/T07 共享 46 叶口径与冻结原账不一致")
    if len(by_task["R09-T05"]) != 19:
        raise SystemExit(f"R09-T05 应为 19 叶，实为 {len(by_task['R09-T05'])}")
    if len(by_task["R09-T04"]) != 9:
        raise SystemExit(f"R09-T04 应为 9 叶，实为 {len(by_task['R09-T04'])}")
    union = set().union(*by_task.values())
    if union != set(leaves):
        raise SystemExit(f"任务叶覆盖不等于 74 叶集合: union={len(union)} leaves={len(leaves)}")

    for suite in SUITE_SCRIPTS.values():
        if not (ROOT / suite).is_file():
            raise SystemExit(f"套件生产者脚本不存在: {suite}")
    return base, leaves, fsa_leaves


def command_for_suite(suite: str) -> dict:
    script = SUITE_SCRIPTS[suite]
    return {
        "argv": ["python3", "-B", script, "--suite", suite,
                 "--evidence", "{EVIDENCE}/SUITES/" + suite],
        "timeoutSecs": 3600,
        "evidencePaths": ["{EVIDENCE}/SUITES/" + suite + "/result.json",
                          "{EVIDENCE}/SUITES/" + suite + "/output.log"],
    }


def leaf_group_plan(row: dict) -> dict:
    key = tuple(row.get("task_ids") or [])
    plan = _LEAF_GROUP_PLAN.get(key)
    if plan is None:
        raise SystemExit(f"叶 {row['id']} 的任务组 {key} 没有实现/平台计划元数据")
    return plan


def build() -> tuple[dict, dict, dict]:
    base, leaves, fsa_leaves = canonical_tree()

    commands = {
        **EXTRA_COMMANDS,
        "r09_inputs_check": {
            "argv": ["python3", "-B", "scripts/rust-tauri/r09_stage_inputs.py",
                     "--check", "--evidence", "{EVIDENCE}/R09_INPUTS"],
            "timeoutSecs": 180,
            "evidencePaths": ["{EVIDENCE}/R09_INPUTS/inputs-check.json"],
        },
    }
    scenarios = []
    for aid in BASE_IDS:
        entry = base[aid]
        spec = entry["spec"]
        task_ids = entry["task_ids"]
        refs = ["r09_inputs_check"] if aid == "R09-A01" else []
        for task in task_ids:
            for suite in TASK_SUITES.get(task, []):
                commands.setdefault(suite, command_for_suite(suite))
                if suite not in refs:
                    refs.append(suite)
        if not refs:
            raise SystemExit(f"基础场景 {aid} 没有绑定任何真实命令")
        scenario = {
            "id": aid,
            "requirement": "REQUIRED",
            "commandRefs": refs,
            "specMirror": {
                "name": spec["name"], "given": spec["given"], "when": spec["when"],
                "then": spec["then"], "evidence": spec["evidence"],
                "environment": spec["environment"], "taskIds": task_ids,
                "featureIds": entry["feature_ids"],
                "ledgerStatus": entry["ledger_status"],
            },
            "note": "命令全绿只证明套件通过；逐断言合同缺位时该场景不得判正式完成。",
        }
        if aid == "R09-A10":
            # Origin/CSP 宿主接缝的 Lead 侧真实服务证据（与 T05 的浏览器侧证据互补）。
            if "r09_service_seam_probe" not in refs:
                refs.append("r09_service_seam_probe")
            scenario["commandRefs"] = refs
        if aid == "R09-A11":
            # capability 展开/多 capability 叠加的静态审计（T06 运行期矩阵另行生产）。
            if "r09_capability_audit" not in refs:
                refs.append("r09_capability_audit")
            scenario["commandRefs"] = refs
        if aid == "R09-A15":
            refs.append("r09_map_integrity")
            # 零匹配/关键 skip/失败案例的隔离负例：证明门的“通过”不是空集假绿。
            if "r09_gate_negative" not in refs:
                refs.append("r09_gate_negative")
            scenario["commandRefs"] = refs
            scenario["assertionContract"] = {
                "producerCommand": "r09_map_integrity",
                "evidencePath": "{EVIDENCE}/SUITES/r09_map_integrity/map-integrity-cases.json",
                "cases": [{"case": name, "expect": 1} for name in MAP_INTEGRITY_CASES],
            }
            scenario["specMirror"]["ledgerStatus"] = base[aid]["ledger_status"]
        scenarios.append(scenario)

    leaf_rows = []
    ledger_rows = []
    for lid in sorted(leaves):
        src = leaves[lid]
        fsa = fsa_leaves[lid]
        plan = leaf_group_plan(src)
        bind = FULL_BINDINGS.get(lid)
        row = {
            "id": lid,
            "featureId": src["feature_id"],
            "requirement": src["requirement"],
            "basisKind": "required_behavior_pending",
            "r00Kind": src["kind"],
            "r00TaskIds": src["task_ids"],
            "r00ExecutionStageIds": src["execution_stage_ids"],
            "r00LedgerStatus": src["ledger_status"],
            "r00ResultIds": src["result_ids"],
            "r00TestIds": src["test_ids"],
            "r00Then": src["then"],
            "r00Assertions": fsa["assertions"],
            "r00Due": fsa["due"],
            "evidenceCommandRefs": [],
            "evidencePaths": [],
            "evidenceRequired": "保持原条款与原断言；每条原断言须绑定本阶段真实专属正反行为证据后方可判完成。",
        }
        ledger = {
            "id": lid,
            "featureId": src["feature_id"],
            "owner": fsa.get("execution_owner"),
            "taskIds": src["task_ids"],
            "executionTaskIds": src["task_ids"],
            "then": src["then"],
            "assertions": [
                {"index": i + 1, "text": text, "caseStatus": "PLANNED", "cases": []}
                for i, text in enumerate(fsa["assertions"])
            ],
            "implementationEntry": plan["implementationEntry"],
            "realConsumer": plan["realConsumer"],
            "plannedSuite": plan["plannedSuite"],
            "platformScope": plan["platformScope"],
            "platformStatus": {p: ("PLANNED" if p in plan["platformScope"] else "BLOCKED_PLATFORM")
                               for p in PLATFORM_SCOPE_ORDER},
            "evidence": {"producer": None, "command": None, "evidencePath": None},
            "status": "PLANNED",
            "notes": plan["notes"],
        }
        if bind is not None:
            if len(bind["assertion_cases"]) != len(fsa["assertions"]):
                raise SystemExit(f"叶 {lid} 的 assertion_cases 与原断言数不一致")
            producer = bind["producer"]
            if producer not in SUITE_SCRIPTS:
                raise SystemExit(f"叶 {lid} 绑定未注册套件 {producer}")
            evidence_file = bind.get("evidence_file", "cases.json")
            evidence_path = f"{{EVIDENCE}}/SUITES/{producer}/{evidence_file}"
            row.update({
                "basisKind": "full_original_behavior",
                "evidenceCommandRefs": [producer],
                "evidencePaths": [evidence_path],
                "evidenceRequired": bind.get(
                    "evidence_required",
                    "逐断言专属案例全覆盖（正路径 + 负路径/失败分支真实观察），机器核对案例文件。"),
                "assertionContract": {
                    "producerCommand": producer,
                    "evidencePath": evidence_path,
                    "cases": [{"case": c, "expect": 1}
                              for group in bind["assertion_cases"] for c in group],
                },
                "originalAssertionCases": bind["assertion_cases"],
            })
            ledger["evidence"] = {"producer": producer,
                                  "command": producer,
                                  "evidencePath": evidence_path}
            ledger["status"] = "EVIDENCED"
            for assertion, cases in zip(ledger["assertions"], bind["assertion_cases"]):
                assertion["caseStatus"] = "BOUND"
                assertion["cases"] = cases
        leaf_rows.append(row)
        ledger_rows.append(ledger)

    inputs = {
        "schema": "lingxi.r09.stage-inputs.v1",
        "stage": STAGE,
        "baseScenarios": [{"id": s["id"], "specMirror": s["specMirror"]} for s in scenarios],
        "requiredLeaves": [{"id": l["id"], "taskIds": l["r00TaskIds"],
                            "executionTaskIds": leaves[l["id"]]["task_ids"],
                            "executionStageIds": l["r00ExecutionStageIds"]} for l in leaf_rows],
        "taskSuites": {task: suites for task, suites in TASK_SUITES.items()},
        "leafBindings": {lid: {"producer": b["producer"],
                               "assertionCases": b["assertion_cases"]}
                         for lid, b in sorted(FULL_BINDINGS.items())},
        "counts": {
            "tasks": 8,
            "baseScenarios": 16,
            "dedupedSupplementalLeaves": 74,
            "originalAssertions": sum(len(l["r00Assertions"]) for l in leaf_rows),
            "t03t07SharedLeaves": 46,
            "t05Leaves": 19,
            "t04Leaves": 9,
        },
        "currentStatus": "MAP_CONTRACT_ESTABLISHED_EVIDENCE_PENDING",
        "coverageHonesty": (
            f"{sum(1 for l in leaf_rows if l['basisKind'] == 'full_original_behavior')}/74 叶已绑定"
            "逐断言证据；其余 required_behavior_pending（BLOCKED，永不计 PASS）。"),
    }
    graph = {
        "schemaVersion": 1,
        "stage": STAGE,
        "resultVersion": "lingxi.xtask.verify-stage.v1",
        "defaultTimeoutSecs": 3600,
        "commands": commands,
        "scenarios": scenarios,
        "supplementalLeafScenarios": leaf_rows,
        "inputDigestSha256": sha(inputs),
    }

    # 结构完整性：命令不得只注册不引用；引用必须已注册；场景不得为空绑定。
    referenced = {ref for s in scenarios for ref in s["commandRefs"]}
    referenced |= {ref for l in leaf_rows for ref in l["evidenceCommandRefs"]}
    unreferenced = sorted(set(commands) - referenced)
    if unreferenced:
        raise SystemExit(f"命令只注册未引用（不会被门执行）: {unreferenced}")
    missing_commands = sorted(referenced - set(commands))
    if missing_commands:
        raise SystemExit(f"场景/叶引用了未注册命令: {missing_commands}")
    empty_scenarios = [s["id"] for s in scenarios if not s["commandRefs"]]
    if empty_scenarios:
        raise SystemExit(f"场景未引用任何命令: {empty_scenarios}")

    ledger_doc = {
        "schema": "lingxi.r09.leaf-ledger.v1",
        "stage": STAGE,
        "generatedFrom": {
            "acceptanceMap": str(ACCEPTANCE_PATH),
            "featureStageAcceptance": str(FEATURE_PATH),
            "generator": "scripts/rust-tauri/r09_stage_inputs.py",
        },
        "counts": inputs["counts"],
        "platformAvailability": {
            "macos-arm64": "AVAILABLE",
            "macos-x64": "BLOCKED_PLATFORM",
            "windows-x64": "BLOCKED_PLATFORM",
            "linux-x64": "BLOCKED_PLATFORM",
        },
        "status": "PLANNED",
        "leaves": ledger_rows,
    }
    return inputs, graph, ledger_doc


def write_outputs(inputs: dict, graph: dict, ledger_doc: dict) -> None:
    out_dir = Path(os.environ.get(
        "LINGXI_R09_MAP_OUT",
        str((ROOT / MAP_PATH).parent)))
    (out_dir / INPUT_PATH.name).write_text(
        json.dumps(inputs, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    (out_dir / MAP_PATH.name).write_text(
        json.dumps(graph, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    ledger_path = ROOT / LEDGER_PATH
    ledger_path.parent.mkdir(parents=True, exist_ok=True)
    ledger_path.write_text(
        json.dumps(ledger_doc, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def run_check(inputs: dict, graph: dict) -> int:
    current = json.loads((ROOT / MAP_PATH).read_text(encoding="utf-8"))
    digest = sha(inputs)
    digest_ok = current.get("inputDigestSha256") == digest
    regenerated = json.dumps(graph, ensure_ascii=False, indent=2) + "\n"
    committed = (ROOT / MAP_PATH).read_text(encoding="utf-8")
    graph_match = regenerated == committed
    ok = digest_ok and graph_match
    if "--evidence" in sys.argv:
        receipt_dir = Path(sys.argv[sys.argv.index("--evidence") + 1])
        receipt_dir.mkdir(parents=True, exist_ok=True)
        (receipt_dir / "inputs-check.json").write_text(json.dumps({
            "schema": "lingxi.r09.inputs-check.v1",
            "digestMatch": digest_ok,
            "expectedDigestSha256": digest,
            "mapDigestSha256": current.get("inputDigestSha256"),
            "graphByteMatch": graph_match,
            "counts": inputs["counts"],
        }, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print("inputs digest " + ("match" if digest_ok else "MISMATCH")
          + ("" if graph_match else "; graph bytes MISMATCH"))
    return 0 if ok else 1


def main() -> int:
    inputs, graph, ledger_doc = build()
    if "--check" in sys.argv:
        return run_check(inputs, graph)
    write_outputs(inputs, graph, ledger_doc)
    bound = sum(1 for l in graph["supplementalLeafScenarios"]
                if l["basisKind"] == "full_original_behavior")
    print(f"R09 map written: {len(graph['commands'])} commands, "
          f"{len(graph['scenarios'])} scenarios, "
          f"{len(graph['supplementalLeafScenarios'])} leaves "
          f"({bound} evidenced, {len(graph['supplementalLeafScenarios']) - bound} pending)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
