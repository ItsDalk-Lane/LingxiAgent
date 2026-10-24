#!/usr/bin/env python3
"""生成 R08 阶段图与输入镜像；只镜像 R00 原账字段，缺逐断言行为绑定的叶保持
required_behavior_pending，不冒充完成（R07 生成器同构）。

用法：
  python3 scripts/rust-tauri/r08_stage_inputs.py            # 生成 stage map + inputs
  python3 scripts/rust-tauri/r08_stage_inputs.py --check --evidence <dir>  # 输入自检
  python3 scripts/rust-tauri/r08_stage_inputs.py --run-suite <suite> --evidence <dir>  # 生产者模式
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ACCEPTANCE_PATH = Path("docs/rust-tauri/R00/ACCEPTANCE_MAP.json")
FEATURE_PATH = Path("docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json")
MAP_PATH = Path("rust/crates/xtask/src/stage_maps/R08.json")
INPUT_PATH = Path("rust/crates/xtask/src/stage_maps/R08_INPUTS.json")

# 任务 → 现役真实验证面（每个键都必须在 SUITE_REGISTRY 内有真实 runner）。
TASK_SUITES = {
    "R08-T01": ["r08_t01_a01_a02_ws", "r08_t01_env_deps"],
    "R08-T02": ["r08_desktop_boundary_vitest"],
    "R08-T03": ["r08_t03_a05_drill", "r08_t03_a06_drill"],
    "R08-T04": ["r08_t04_sessionfiles_v2", "r08_registry_amend_proof", "r08_registry_consistency"],
    "R08-T05": ["r08_t05_seed_guard", "r08_t06_d1", "r08_t06_d2", "r08_t06_d3"],
    "R08-T06": ["r08_t06_d1", "r08_t06_d2", "r08_t06_d3"],
    "R08-T07": ["r08_t07_ab", "r08_t07_real_format"],
    "R08-T08": ["r08_a15_isolation", "r08_a16_versions"],
}

# A/B 线束已实测平价的份额图钉（case 名 → r08_t07_ab 生产者 cases.json 的键）。
SHARE_PINS = {
    "R00-T02-LA-38EEA99077E8": {
        "share": "正路径：同一数据两内核经 UI 绑定层返回同数会话列表（计数与置顶面平价）",
        "cases": [("ab::sessions.list.count", 1), ("ab::sessions.list.parity", 1)],
    },
    "R00-T02-LA-FDB0FE6528EE": {
        "share": "正路径：改名成功且重载持久化（双内核）",
        "cases": [("ab::sessions.rename.ok", 1), ("ab::sessions.rename.persisted", 1)],
    },
    "R00-T02-LA-0D4F0EFC9DAE": {
        "share": "正路径：置顶与取消置顶调用成功（双内核）；已删 Agent 禁置顶等分支未测",
        "cases": [("ab::sessions.pin.pin", 1), ("ab::sessions.pin.unpin", 1)],
    },
    "R00-T02-LA-45C5CE361CCB": {
        "share": "正路径：完成调用成功（双内核）；广播与重复写幂等未测",
        "cases": [("ab::sessions.todos.complete", 1)],
    },
    "R00-T02-LA-5CD35D9EED20": {
        "share": "正路径：取消调用成功（双内核）；不终止其他任务未测",
        "cases": [("ab::sessions.todos.cancel", 1)],
    },
    "R00-T02-LA-17FCFE45576B": {
        "share": "正路径：收纳调用成功（双内核）；仅已结束且未收纳未测",
        "cases": [("ab::sessions.todos.dismiss", 1)],
    },
    "R00-T02-LA-B0F05BC32D62": {
        "share": "正路径：返回删除数量形状（双内核）；实际按期限删除未测",
        "cases": [("ab::sessions.maintenance.cleanup", 1)],
    },
    "R00-T02-LA-21A99DB98735": {
        "share": "正路径：返回数量形状（双内核）；实际归档与跳过运行中未测",
        "cases": [("ab::sessions.maintenance.sweep", 1)],
    },
    "R00-T02-LA-25C4FF66FEE5": {
        "share": "正路径：返回同一 permissionMode（双内核 auto）；未设置时 ASK 未测",
        "cases": [("ab::permissions.default.parity", 1)],
    },
}

# 套件注册表（runner 形态见 run_suite 的逐套件分支；argv 全字面量）。
SUITE_REGISTRY = {
    "r08_t01_a01_a02_ws", "r08_t01_env_deps", "r08_a04_no_final_ws",
    "r08_t04_sessionfiles_v2", "r08_t05_seed_guard",
    "r08_registry_amend_proof", "r08_t06_d1", "r08_t06_d2", "r08_t06_d3",
    "r08_t07_real_format", "r08_t07_ab",
    "r08_t03_a05_drill", "r08_t03_a06_drill",
    "r08_desktop_boundary_vitest", "r08_registry_consistency",
    "r08_a15_isolation", "r08_a16_versions",
}
# 已达成「每条原断言均有专属案例」全覆盖的叶（full_original_behavior）：
# 断言 i → 专属案例名列表（跨断言不共用；全部经 r08_t07_ab cases.json 机器核对）。
FULL_BINDINGS = {
    "R00-T02-LA-FDB0FE6528EE": {  # 重命名会话：断言0=保存成功+空标题拒绝；断言1=明确错误
        "assertion_cases": [
            ["ab::sessions.rename.ok", "ab::sessions.rename.persisted", "leaf::rename.empty-title.rejected"],
            ["leaf::rename.invalid-session.explicit-error"],
        ],
    },
    "R00-T02-LA-38EEA99077E8": {  # 查看可见会话列表：断言0=投影字段+双内核同数；断言1=鉴权不足明确拒绝
        "assertion_cases": [
            ["ab::sessions.list.count", "ab::sessions.list.parity", "leaf::list.revision-field-present", "leaf::list.summary-or-badge-present"],
            ["leaf::list.unauthorized-rejected"],
        ],
    },
    "R00-T02-LA-DD47275AC5AE": {  # 查看 MCP 连接状态：断言0=连接器状态+延迟加载开关投影；断言1=鉴权不足明确拒绝
        "assertion_cases": [
            ["leaf::mcp.state.shape"],
            ["leaf::mcp.state.unauthorized-rejected"],
        ],
    },
}

CONTRACT_PINNED_CASES = {
    case
    for binding in FULL_BINDINGS.values()
    for group in binding["assertion_cases"]
    for case in group
}

CARGO_TEST_SUITES = {"r08_t01_a01_a02_ws", "r08_t01_env_deps", "r08_a04_no_final_ws",
                     "r08_t04_sessionfiles_v2", "r08_t05_seed_guard"}


def load(path: Path):
    return json.loads((ROOT / path).read_text(encoding="utf-8"))


def sha(value) -> str:
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True).encode()).hexdigest()


def command_for_suite(suite: str) -> dict:
    return {
        "argv": ["python3", "-B", "scripts/rust-tauri/r08_stage_inputs.py",
                 "--run-suite", suite, "--evidence", "{EVIDENCE}/SUITES/" + suite],
        "timeoutSecs": 3600,
        "evidencePaths": ["{EVIDENCE}/SUITES/" + suite + "/result.json",
                          "{EVIDENCE}/SUITES/" + suite + "/output.log"],
    }


def derive_ab_cases(out_dir: Path):
    """从 A/B 观测派生叶份额图钉：case=1 当且仅当双内核字段真值成立。"""
    try:
        a = json.loads((out_dir / "ab-node.json").read_text(encoding="utf-8"))["observations"]
        b = json.loads((out_dir / "ab-rust.json").read_text(encoding="utf-8"))["observations"]
    except Exception as error:  # noqa: BLE001 - 派生失败=全 0 图钉（响亮）
        print(f"ab cases derive failed: {error}")
        return {}

    def truthy(obs, task, field):
        return bool(obs.get(task, {}).get("observed", {}).get(field))

    def both(task, field):
        return 1 if (truthy(a, task, field) and truthy(b, task, field)) else 0

    a_list = a.get("sessions.list", {}).get("observed", {})
    b_list = b.get("sessions.list", {}).get("observed", {})
    pin, pin_b = (a.get("sessions.pin", {}).get("observed", {}),
                  b.get("sessions.pin", {}).get("observed", {}))
    todos, todos_b = (a.get("sessions.todos", {}).get("observed", {}),
                      b.get("sessions.todos", {}).get("observed", {}))
    maint, maint_b = (a.get("sessions.maintenance", {}).get("observed", {}),
                      b.get("sessions.maintenance", {}).get("observed", {}))
    perm = a.get("permissions.default", {}).get("observed", {})
    perm = {"mode": perm.get("mode")} if isinstance(perm, dict) else {}
    perm_b = b.get("permissions.default", {}).get("observed", {})
    perm_b = {"mode": perm_b.get("mode")} if isinstance(perm_b, dict) else {}
    cases = {
        "ab::sessions.list.count": 1 if (a_list.get("count", 0) > 0 and b_list.get("count", 0) > 0) else 0,
        "ab::sessions.list.parity": 1 if a_list.get("count") == b_list.get("count") else 0,
        "ab::sessions.rename.ok": both("sessions.rename", "ok"),
        "ab::sessions.rename.persisted": both("sessions.rename", "persisted"),
        "ab::sessions.pin.pin": 1 if (pin.get("pinA") and pin.get("pinB")
                                       and pin_b.get("pinA") and pin_b.get("pinB")) else 0,
        "ab::sessions.pin.unpin": 1 if (pin.get("unpinA") and pin.get("unpinB")
                                         and pin_b.get("unpinA") and pin_b.get("unpinB")) else 0,
        "ab::sessions.todos.complete": 1 if (todos.get("complete") and todos_b.get("complete")) else 0,
        "ab::sessions.todos.cancel": 1 if (todos.get("cancel") and todos_b.get("cancel")) else 0,
        "ab::sessions.todos.dismiss": 1 if (todos.get("dismiss") and todos_b.get("dismiss")) else 0,
        "ab::sessions.maintenance.cleanup": 1 if (maint.get("cleanupDeletedIsNumber")
                                                   and maint_b.get("cleanupDeletedIsNumber")) else 0,
        "ab::sessions.maintenance.sweep": 1 if (maint.get("sweptIsNumber")
                                                 and maint_b.get("sweptIsNumber")) else 0,
        "ab::permissions.default.parity": 1 if (perm.get("mode") == perm_b.get("mode")
                                                 and perm.get("mode") is not None) else 0,
        "leaf::permissions.mode-nonnull": 1 if (perm.get("mode") is not None and perm_b.get("mode") is not None) else 0,
    }
    neg = a.get("leaf.rename.negative", {}).get("observed", {})
    neg_b = b.get("leaf.rename.negative", {}).get("observed", {})
    pin_neg = a.get("leaf.pin.negative", {}).get("observed", {})
    pin_neg_b = b.get("leaf.pin.negative", {}).get("observed", {})
    todos_neg = a.get("leaf.todos.negative", {}).get("observed", {})
    todos_neg_b = b.get("leaf.todos.negative", {}).get("observed", {})
    clean = a.get("leaf.cleanup.boundary", {}).get("observed", {})
    clean_b = b.get("leaf.cleanup.boundary", {}).get("observed", {})
    fields = a.get("leaf.list.fields", {}).get("observed", {})
    fields_b = b.get("leaf.list.fields", {}).get("observed", {})
    pidem = a.get("leaf.permissions.read-idempotent", {}).get("observed", {})
    pidem_b = b.get("leaf.permissions.read-idempotent", {}).get("observed", {})
    mcp = a.get("leaf.mcp.state", {}).get("observed", {})
    mcp_b = b.get("leaf.mcp.state", {}).get("observed", {})
    mcpu = a.get("leaf.mcp.state.unauthorized", {}).get("observed", {})
    mcpu_b = b.get("leaf.mcp.state.unauthorized", {}).get("observed", {})
    cases.update({
        "leaf::rename.empty-title.rejected": 1 if (neg.get("emptyRejected") and neg_b.get("emptyRejected")) else 0,
        "leaf::rename.invalid-session.explicit-error": 1 if (neg.get("invalidRejected") and neg_b.get("invalidRejected")) else 0,
        "leaf::pin.invalid-session.explicit-error": 1 if (pin_neg.get("rejected") and pin_neg_b.get("rejected")) else 0,
        "leaf::todos.invalid-session.explicit-error": 1 if (todos_neg.get("rejected") and todos_neg_b.get("rejected")) else 0,
        "leaf::cleanup.fresh-archive-kept": 1 if (clean.get("freshKept") and clean_b.get("freshKept")) else 0,
        "leaf::cleanup.archived-delete-with-effect": 1 if (clean.get("purgedOk") and clean_b.get("purgedOk") and clean.get("expiringGone") and clean_b.get("expiringGone")) else 0,
        "leaf::list.revision-field-present": 1 if (fields.get("hasRevision") and fields_b.get("hasRevision")) else 0,
        "leaf::list.summary-or-badge-present": 1 if (fields.get("hasSummaryOrBadge") and fields_b.get("hasSummaryOrBadge")) else 0,
        "leaf::list.unauthorized-rejected": 1 if (a.get("error-path.unauthorized", {}).get("observed", {}).get("rejected")
                                                  and b.get("error-path.unauthorized", {}).get("observed", {}).get("rejected")) else 0,
        "leaf::permissions.read-idempotent": 1 if (pidem.get("identical") and pidem_b.get("identical") and pidem.get("mode") == pidem_b.get("mode")) else 0,
        "leaf::mcp.state.shape": 1 if (mcp.get("ok") and mcp_b.get("ok") and mcp.get("enabledIsBool")
                                        and mcp_b.get("enabledIsBool") and mcp.get("connectorsIsArray")
                                        and mcp_b.get("connectorsIsArray")) else 0,
        "leaf::mcp.state.unauthorized-rejected": 1 if (mcpu.get("rejected") and mcpu_b.get("rejected")) else 0,
    })
    return cases


def run_suite(suite: str, evidence: Path) -> int:
    """生产者模式：白名单校验 → 逐套件字面 argv（列表参数、shell=False）。"""
    if not re.fullmatch(r"[a-z0-9_]+", suite) or suite not in SUITE_REGISTRY:
        raise SystemExit(f"未登记或非法套件名: {suite}")
    evidence.mkdir(parents=True, exist_ok=True)
    scratch = Path(os.environ.get("LINGXI_R08_SUITE_RUNTIME", "/private/tmp/lingxi-r08-verify")) \
        / f"{suite}-{os.getpid()}-{int(time.time() * 1000)}"
    env = dict(os.environ)
    env.pop("ELECTRON_RUN_AS_NODE", None)
    started = time.time()
    if suite in CARGO_TEST_SUITES:
        env.update({"CARGO_TERM_COLOR": "never", "LINGXI_PRODUCT_DIR": str(ROOT / "lib"),
                    "TMPDIR": str(scratch), "LINGXI_HOME": str(scratch / "home")})
        (scratch / "home").mkdir(parents=True, exist_ok=True)
        proc = subprocess.run(["cargo", "test", "--manifest-path", "rust/Cargo.toml",
                               "--locked", "-p", "lingxi-service", "--test", suite],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_registry_amend_proof":
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t06_registry_amend.mjs", "--bin",
                               "rust/target/debug/lingxi-service", "--scratch", scratch.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_t06_d1":
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t06_d1_drill.mjs", "--bin",
                               "rust/target/debug/lingxi-service", "--work", scratch.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_t06_d2":
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t06_d2_drill.mjs", "--bin",
                               "rust/target/debug/lingxi-service", "--scratch", scratch.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_t06_d3":
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t06_d3_drill.mjs", "--bin",
                               "rust/target/debug/lingxi-service", "--work", scratch.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_t07_real_format":
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t07_real_format_probe.mjs", "--bin",
                               "rust/target/debug/lingxi-service", "--work", scratch.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_t07_ab":
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t07_ab_drill.mjs", "--bin",
                               "rust/target/debug/lingxi-service", "--work", scratch.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_t03_a05_drill":
        scratch.mkdir(parents=True, exist_ok=True)
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t03_a05_drill.mjs", "--out",
                               scratch.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_t03_a06_drill":
        scratch.mkdir(parents=True, exist_ok=True)
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t03_a06_drill.mjs", "--out",
                               scratch.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_a15_isolation":
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t08_isolation_check.mjs", "--out",
                               evidence.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_a16_versions":
        proc = subprocess.run(["node", "scripts/rust-tauri/r08_t08_version_manifest.mjs", "--out",
                               evidence.as_posix()],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_desktop_boundary_vitest":
        proc = subprocess.run(["npx", "vitest", "run",
                               "desktop/src/react/__tests__/host/r08-a03-web-desktop-host.test.tsx",
                               "desktop/src/react/__tests__/services/r08-a01-ui-projection.test.ts",
                               "desktop/src/react/__tests__/services/r08-p1-data-epoch-hello.test.ts",
                               "desktop/src/react/__tests__/services/r08-session-legacy-compat.test.ts"],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    elif suite == "r08_registry_consistency":
        proc = subprocess.run(["python3", "scripts/rust-tauri/r02_registry_consistency.py"],
                              cwd=ROOT, env=env, capture_output=True, text=True, timeout=3000)
    else:
        raise SystemExit(f"套件 {suite} 在注册表内但无分支（登记缺口）")
    log = proc.stdout + proc.stderr
    (evidence / "output.log").write_text(log, encoding="utf-8")
    if suite == "r08_t07_ab":
        # 门合同格式 lingxi.leaf-case-results.v1：每案例 {case,expect,actual,ok}
        # （expect 钉 1=双内核断言成立；actual=实测；ok=expect==actual）。
        derived = derive_ab_cases(scratch / "out")
        # 合同证据文件只收录阶段图钉住的案例（门规：整份记录必须全部成立）。
        # 未钉案例（如 cleanup 期限边界——D-6 真差异）不入此文件，其差异在
        # T07_UI_DIFFERENCE_REPORT.md / FEATURE_PARITY_RESULTS.json 如实披露。
        pinned = CONTRACT_PINNED_CASES
        rows = [{"case": case, "expect": 1, "actual": derived.get(case, 0), "ok": derived.get(case, 0) == 1}
                for case in sorted(pinned)]
        (evidence / "cases.json").write_text(
            json.dumps({"schema": "lingxi.leaf-case-results.v1", "cases": rows},
                       ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    receipt = {
        "schema": "lingxi.r08.suite-receipt.v1",
        "suite": suite,
        "exitCode": proc.returncode,
        "durationSecs": round(time.time() - started, 3),
        "logSha256": hashlib.sha256(log.encode()).hexdigest(),
    }
    (evidence / "result.json").write_text(
        json.dumps(receipt, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"suite {suite}: exitCode={proc.returncode}")
    return proc.returncode


def main() -> int:
    if "--run-suite" in sys.argv:
        suite = sys.argv[sys.argv.index("--run-suite") + 1]
        evidence = Path(sys.argv[sys.argv.index("--evidence") + 1])
        return run_suite(suite, evidence)

    acceptance = load(ACCEPTANCE_PATH)["scenarios"]
    features = {row["id"]: row for row in load(FEATURE_PATH)["supplemental_scenarios"]}

    base = {k: v for k, v in acceptance.items()
            if v.get("kind") == "base" and v.get("stage_id") == "R08"}
    if sorted(base) != [f"R08-A{i:02}" for i in range(1, 17)]:
        raise SystemExit("R08 基础场景集合与 R00 原账不一致")

    leaves = {k: v for k, v in acceptance.items()
              if v.get("kind") == "supplemental" and "R08" in (v.get("execution_stage_ids") or [])}
    fsa_r08 = {k: v for k, v in features.items() if "R08" in (v.get("execution_stage_ids") or [])}
    if set(leaves) != set(fsa_r08) or len(leaves) != 126:
        raise SystemExit(f"R08 补充叶集合在两本原账间不一致或数量不为 126（{len(leaves)}/{len(fsa_r08)}）")

    for task, suites in TASK_SUITES.items():
        for suite in suites:
            if suite not in SUITE_REGISTRY:
                raise SystemExit(f"{task} 登记的套件不在注册表: {suite}")

    commands = {
        "r08_inputs_check": {
            "argv": ["python3", "-B", "scripts/rust-tauri/r08_stage_inputs.py",
                     "--check", "--evidence", "{EVIDENCE}/R08_INPUTS"],
            "timeoutSecs": 120,
            "evidencePaths": ["{EVIDENCE}/R08_INPUTS/inputs-check.json"],
        },
    }
    scenarios = []
    for aid in sorted(base):
        spec = base[aid]["spec"]
        task_ids = base[aid]["task_ids"]
        refs = []
        if aid == "R08-A01":
            refs.append("r08_inputs_check")
        for task in task_ids:
            for suite in TASK_SUITES.get(task, []):
                commands.setdefault(suite, command_for_suite(suite))
                refs.append(suite)
        scenarios.append({
            "id": aid,
            "requirement": "REQUIRED",
            "commandRefs": refs,
            "specMirror": {
                "name": spec["name"], "given": spec["given"], "when": spec["when"],
                "then": spec["then"], "evidence": spec["evidence"],
                "environment": spec["environment"], "taskIds": task_ids,
                "featureIds": base[aid]["feature_ids"],
                "ledgerStatus": base[aid]["ledger_status"],
            },
            "note": "命令全绿只证明套件通过；逐断言合同缺位时该场景不得判正式完成。",
        })
    commands.setdefault("r08_a04_no_final_ws", command_for_suite("r08_a04_no_final_ws"))

    leaf_rows = []
    for lid in sorted(leaves):
        src = leaves[lid]
        fsa = fsa_r08[lid]
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
        full = FULL_BINDINGS.get(lid)
        if full is not None and len(full["assertion_cases"]) == len(fsa["assertions"]):
            row["basisKind"] = "full_original_behavior"
            row["evidenceCommandRefs"] = ["r08_t07_ab"]
            row["evidencePaths"] = ["{EVIDENCE}/SUITES/r08_t07_ab/cases.json"]
            row["evidenceRequired"] = "逐断言专属案例全覆盖（正路径平价 + 负路径双内核显式拒绝），机器核对 cases.json。"
            flat = [c for group in full["assertion_cases"] for c in group]
            row["assertionContract"] = {
                "producerCommand": "r08_t07_ab",
                "evidencePath": "{EVIDENCE}/SUITES/r08_t07_ab/cases.json",
                "cases": [{"case": c, "expect": 1} for c in flat],
            }
            row["originalAssertionCases"] = full["assertion_cases"]
        else:
            # 门规（R05 RR1 F25）：R08 是末位阶段，份额拆分无 later stage 承载 →
            # 保持 required_behavior_pending；已实测的正路径平价份额记入注记
            # （非门禁）；升级 full_original_behavior 须逐断言专属案例（在 FULL_BINDINGS）。
            pin = SHARE_PINS.get(lid)
            if pin is not None:
                row["evidenceRequired"] = (
                    "保持原条款与原断言；每条原断言须绑定本阶段真实专属正反行为证据后方可判完成。"
                    f"【已实测份额（非门禁）】{pin['share']}；图钉见 r08_t07_ab cases.json："
                    + ", ".join(case for case, _ in pin["cases"]))
        leaf_rows.append(row)

    inputs = {
        "schema": "lingxi.r08.stage-inputs.v1",
        "stage": "R08",
        "baseScenarios": [{"id": s["id"], "specMirror": s["specMirror"]} for s in scenarios],
        "requiredLeaves": [{"id": l["id"], "taskIds": l["r00TaskIds"],
                            "executionStageIds": l["r00ExecutionStageIds"]} for l in leaf_rows],
        "currentStatus": "IMPLEMENTATION_REGISTERED_ACCEPTANCE_PARTIAL",
        "coverageHonesty": "9/126 叶绑定本阶段份额图钉（A/B 正路径平价）；117 叶 required_behavior_pending；"
                           "A05 呈 PPG-1/PPG-2 双 BLOCKED（T03 裁决）；D-4 差异未决（T07 差异报告）。",
    }
    graph = {
        "schemaVersion": 1,
        "stage": "R08",
        "resultVersion": "lingxi.xtask.verify-stage.v1",
        "defaultTimeoutSecs": 3600,
        "commands": commands,
        "scenarios": scenarios,
        "supplementalLeafScenarios": leaf_rows,
        "inputDigestSha256": sha(inputs),
    }

    out_map = "--check" not in sys.argv
    out_dir = (ROOT / MAP_PATH).parent
    if "--out" in sys.argv:
        out_dir = Path(sys.argv[sys.argv.index("--out") + 1])
    if out_map:
        (out_dir / INPUT_PATH.name).write_text(json.dumps(inputs, ensure_ascii=False, indent=2) + "\n")
        (out_dir / MAP_PATH.name).write_text(json.dumps(graph, ensure_ascii=False, indent=2) + "\n")
        pending = sum(1 for l in leaf_rows if l["basisKind"] == "required_behavior_pending")
        print(f"R08 map written: {len(commands)} commands, {len(scenarios)} scenarios, "
              f"{len(leaf_rows)} leaves ({len(leaf_rows) - pending} share-bound, {pending} pending)")
    else:
        digest = sha(inputs)
        current = json.loads((ROOT / MAP_PATH).read_text(encoding="utf-8"))
        ok = current.get("inputDigestSha256") == digest
        if "--evidence" in sys.argv:
            receipt_dir = Path(sys.argv[sys.argv.index("--evidence") + 1])
            receipt_dir.mkdir(parents=True, exist_ok=True)
            (receipt_dir / "inputs-check.json").write_text(json.dumps({
                "schema": "lingxi.r08.inputs-check.v1",
                "digestMatch": ok,
                "expectedDigestSha256": digest,
                "mapDigestSha256": current.get("inputDigestSha256"),
            }, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        print("inputs digest " + ("match" if ok else "MISMATCH"))
        return 0 if ok else 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
