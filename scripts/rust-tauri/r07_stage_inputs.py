#!/usr/bin/env python3
"""生成 R07 阶段输入与正式阶段图；只镜像 R00 两本原账与各前阶段地图的真实字段，
缺完整逐断言行为绑定的叶明确保持 required_behavior_pending，不冒充完成。"""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(os.environ.get("LINGXI_ROOT", Path(__file__).resolve().parents[2]))
ACCEPTANCE_PATH = Path("docs/rust-tauri/R00/ACCEPTANCE_MAP.json")
FEATURE_PATH = Path("docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json")
PARITY_PATH = Path("docs/rust-tauri/R07/FEATURE_PARITY_RESULTS.json")
MAP_PATH = Path("rust/crates/xtask/src/stage_maps/R07.json")
INPUT_PATH = Path("rust/crates/xtask/src/stage_maps/R07_INPUTS.json")
PRIOR_MAPS = ["R02", "R03", "R04", "R05", "R06"]

# 基础场景 R07-A01..A24 与其所属任务的现役真实测试面（每个二进制都须真实存在）。
TASK_SUITES = {
    "R07-T01": ["r07_scheduler", "r07_heartbeat_plan", "r07_heartbeat_runtime",
                "r07_daily_fresh_compact"],
    "R07-T02": ["r07_bridge_flow", "r07_bridge_native_flow", "r07_bridge_permission",
                "r07_bridge_notice", "r07_bridge_sdk_platforms", "r07_bridge_session_routes",
                "r07_bridge_worker", "r07_bridge_diagnostic", "r07_bridge_guest_context"],
    "R07-T03": ["r07_session_collab", "r07_phone_runtime"],
    "R07-T04": ["r07_browser_routes", "r07_browser_tools"],
    "R07-T05": ["r07_office_tools", "r07_desk_files"],
    "R07-T06": ["r07_t05_media", "r07_media_admin"],
    "R07-T07": ["r07_workbench", "r07_workflow", "r07_goal_loop", "r07_git_panel",
                "r07_conversation_tools", "r07_diary", "r07_web_tools"],
    "R07-T08": ["r07_t08_settings", "r07_t08_provider_matrix", "r07_t08_provider_http",
                "r07_t08_provider_capabilities", "r07_t08_speech_bindings",
                "r07_t08_model_configuration", "r07_t08_model_authority",
                "r07_t08_oauth_alias", "r07_t08_mcp_settings", "r07_t08_mcp_process",
                "r07_t08_mcp_remote", "r07_t08_mcp_session_permissions",
                "r07_t08_metadata_redaction", "r07_t08_search_clear",
                "r07_t08_skills_cards", "r07_t08_card_previews",
                "r07_t08_interface_preferences", "r07_t08_agent_target",
                "r07_t08_media_source"],
    "R07-T09": ["r07_t09_client_context"],
    "R07-T10": ["r07_t10_observability"],
    "R07-T11": ["r07_t11_dev_entry_and_unload"],
    "R07-T12": ["r07_t12_closure"],
}
# A23/A24 由 T12 的专属生产者与测试承载，不靠任务面套件冒充。

EXTENSION_IDS = ["F-D16-E-USER-PLUGIN-LEGACY", "F-D16-E-DYNAMIC-SLASH-PROTOCOL"]


def load(path: Path):
    return json.loads((ROOT / path).read_text(encoding="utf-8"))


def sha(value) -> str:
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True).encode()).hexdigest()


def command_for_suite(suite: str) -> dict:
    # 地图命令必须真实产出声明的证据（xtask 鲜度规则：声明即必须存在）。
    # 每个套件由本脚本的 --run-suite 模式执行并写 result.json + output.log。
    return {
        "argv": ["python3", "-B", "scripts/rust-tauri/r07_stage_inputs.py",
                 "--run-suite", suite, "--evidence", "{EVIDENCE}/SUITES/" + suite],
        "timeoutSecs": 3600,
        "evidencePaths": ["{EVIDENCE}/SUITES/" + suite + "/result.json",
                          "{EVIDENCE}/SUITES/" + suite + "/output.log"],
    }


def run_suite(suite: str, evidence: Path) -> int:
    """生产者模式：真实执行一个服务测试套件，把退出码与日志落到证据目录。
    自带隔离运行时环境（镜像 R06 生产者的 env_for 块），不依赖调用方外壳。
    运行时根必须放在仓库之外：git 发现会沿父目录爬升，落在仓库内的临时
    目录会把宿主仓库误判为夹具工作区（git_panel 非仓库断言因此失败）。"""
    if not re.fullmatch(r"[a-z0-9_]+", suite):
        raise SystemExit(f"非法套件名: {suite}")
    evidence.mkdir(parents=True, exist_ok=True)
    runtime = Path(os.environ.get("LINGXI_R07_SUITE_RUNTIME", "/private/tmp/lingxi-r07-verify")) \
        / f"{suite}-{os.getpid()}-{int(time.time() * 1000)}"
    home, temporary, raw = runtime / "home", runtime / "tmp", runtime / "raw"
    for path in (home, temporary, raw):
        path.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    env.pop("ELECTRON_RUN_AS_NODE", None)  # 宿主污染不得进入测试链
    if suite == "r08_admission_ast_lsp":
        # 该套件依赖官方 ast-grep CLI（AST_LSP_IMPLEMENTATION.md 记录的固定
        # 临时安装位置）；缺依赖时套件按自身断言诚实失败，不在此伪造。
        ast_tools = "/private/tmp/lingxi-r08-ast-tools-0510/node_modules/.bin"
        env["PATH"] = ast_tools + os.pathsep + env.get("PATH", "")
    if suite == "r08_admission_ptc":
        # 全量含真实 600 秒默认预算单项；lingxi-test-guard 的整进程墙钟需要
        # 少量余量（独审 ADMISSION_CANDIDATE_INDEPENDENT_REVIEW.md §5.6 已接受
        # 该差异；产品预算仍由测试内部断言，内存 8GiB 保护保留）。
        env["TEST_TIME_LIMIT_SEC"] = "660"
    if suite.startswith("r08_admission_"):
        # 准入套件按串行验证设计（全部既有回执均单线程）：并行时多测试的
        # Node 外围进程共享同一测试进程父 PID，会破坏 cancelling_* 测试
        # "恰好一个外围进程"的真实断言（f02-batch0-suites-01 的
        # r08_admission_ptc exit 101 红证据即该环境错配，非产品缺陷）。
        # 经 libtest 原生 RUST_TEST_THREADS 环境变量串行化，不改变命令构造。
        env["RUST_TEST_THREADS"] = "1"
    env.update({
        "LINGXI_HOME": str(home), "TMPDIR": str(temporary),
        "TMP": str(temporary), "TEMP": str(temporary),
        "LINGXI_PRODUCT_DIR": str(ROOT / "lib"),
        "CARGO_TERM_COLOR": "never",
        "LINGXI_F05_LOCAL_ROOT": str(runtime / "f05-local"),
        "R06_T08_RAW_OUT": str(raw / "t08"),
        "R06_T08_EVAL_OUT": str(raw / "t08-evaluations.jsonl"),
    })
    for name in ("LINGXI_W03_EVIDENCE", "LINGXI_F05_EVIDENCE",
                 "LINGXI_F08_EVIDENCE", "LINGXI_F13_EVIDENCE"):
        target = raw / name.lower()
        target.mkdir(parents=True, exist_ok=True)
        env[name] = str(target)
    started = time.time()
    proc = subprocess.run(
        ["cargo", "test", "--manifest-path", "rust/Cargo.toml", "--locked",
         "-p", "lingxi-service", "--test", suite],
        cwd=ROOT, env=env, capture_output=True, text=True)
    log = proc.stdout + proc.stderr
    (evidence / "output.log").write_text(log, encoding="utf-8")
    receipt = {
        "schema": "lingxi.r07.suite-receipt.v1",
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
    parity = load(PARITY_PATH)

    base = {k: v for k, v in acceptance.items()
            if v.get("kind") == "base" and v.get("stage_id") == "R07"}
    if sorted(base) != [f"R07-A{i:02}" for i in range(1, 25)]:
        raise SystemExit("R07 基础场景集合与 R00 原账不一致")

    leaves = {k: v for k, v in acceptance.items()
              if v.get("kind") == "supplemental" and "R07" in (v.get("execution_stage_ids") or [])}
    fsa_r07 = {k: v for k, v in features.items() if "R07" in (v.get("execution_stage_ids") or [])}
    if set(leaves) != set(fsa_r07) or len(leaves) != 408:
        raise SystemExit("R07 补充叶集合在两本原账间不一致或数量不为 408")

    if [row["featureId"] for row in parity["extensionBoundaries"]] != EXTENSION_IDS:
        raise SystemExit("扩展边界集合与 FEATURE_PARITY_RESULTS 不一致")

    # 移交甲：用户承诺的 35 项权威清单（R04 规划继承、R07 执行范围），逐项核对在 408 内。
    provenance = load(Path("artifacts/rust-tauri/R07/T12/35-handoff-source-resolved-20261018.json"))
    if provenance.get("schema") != "lingxi.r07.handoff35-provenance.v1" or len(provenance["items"]) != 35:
        raise SystemExit("35 项移交权威清单 schema/数量不符")
    handover35 = []
    for item in provenance["items"]:
        if item["id"] not in leaves:
            raise SystemExit(f"移交项 {item['id']} 不在 R07 的 408 补充叶内")
        handover35.append({
            "id": item["id"],
            "featureId": item["featureId"],
            "executionStageIds": item["executionStageIds"],
            "originalTaskIds": item["originalTaskIds"],
            "assertionCount": len(item["assertions"]),
        })

    # 移交乙：R02—R06 阶段地图里声明递延到 R07 的叶（与 35 不重叠的另一类来源），
    # 继承链重复项按稳定叶 ID 合并，来源全保留。
    handovers = []
    for stage in PRIOR_MAPS:
        prior = load(Path(f"rust/crates/xtask/src/stage_maps/{stage}.json"))
        for leaf in prior.get("supplementalLeafScenarios", []):
            if leaf.get("basisKind") in ("deferred_to_r07", "deferred_to_later_stage") \
                    and "R07" in leaf.get("r00ExecutionStageIds", []):
                handovers.append({
                    "id": leaf["id"],
                    "sourceStage": stage,
                    "sourceBasisKind": leaf["basisKind"],
                    "r00ExecutionStageIds": leaf["r00ExecutionStageIds"],
                    "r07Share": leaf.get("r07Share") or leaf.get("laterShare") or "",
                    "r07LeafPresent": leaf["id"] in leaves,
                    "r07TaskIds": (leaves.get(leaf["id"]) or {}).get("task_ids", []),
                })
    # 继承链重复项按稳定叶 ID 合并，来源全保留。
    merged = {}
    for item in handovers:
        slot = merged.setdefault(item["id"], {**item, "sourceStages": []})
        slot["sourceStages"].append(item.pop("sourceStage"))
    handover_items = list(merged.values())

    # 校验任务套件映射真实存在。
    tests_dir = Path(os.environ.get("LINGXI_TESTS_DIR",
                                    ROOT / "rust/crates/lingxi-service/tests"))
    for task, suites in TASK_SUITES.items():
        for suite in suites:
            if not (tests_dir / (suite + ".rs")).is_file():
                raise SystemExit(f"{task} 登记的测试套件不存在: {suite}")

    commands = {
        # 输入镜像自检：地图里的 R00 镜像字段必须与重新生成的输入逐字节一致，
        # 防止地图悄悄改写原账。绑定到首个基础场景（与 R06 先例一致）。
        "r07_inputs_check": {
            "argv": ["python3", "-B", "scripts/rust-tauri/r07_stage_inputs.py",
                     "--check", "--evidence", "{EVIDENCE}/R07_INPUTS"],
            "timeoutSecs": 120,
            "evidencePaths": ["{EVIDENCE}/R07_INPUTS/inputs-check.json"],
        },
    }
    scenarios = []
    for aid in sorted(base):
        spec = base[aid]["spec"]
        task_ids = base[aid]["task_ids"]
        refs = []
        if aid == "R07-A01":
            refs.append("r07_inputs_check")
        for task in task_ids:
            for suite in TASK_SUITES.get(task, []):
                commands.setdefault(suite, command_for_suite(suite))
                refs.append(suite)
        scenario = {
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
        }
        if aid == "R07-A23":
            # A23 双向映射报告由静态生产者真实生成并核对，不只靠运行时套件。
            commands["r07_behavior_matrix"] = {
                "argv": ["python3", "-B", "scripts/rust-tauri/r07_behavior_matrix.py",
                         "--out", "{EVIDENCE}/A23"],
                "timeoutSecs": 600,
                "evidencePaths": ["{EVIDENCE}/A23/R07_BIDIRECTIONAL_MAP.json"],
            }
            refs.append("r07_behavior_matrix")
        scenarios.append(scenario)

    leaf_rows = []
    for lid in sorted(leaves):
        src = leaves[lid]
        fsa = fsa_r07[lid]
        leaf_rows.append({
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
            # 此类叶不得绑定门禁证据命令/可通过证据（解析器硬规则：绑定即
            # 悄悄解除 BLOCKED）；键必须存在且为空数组。
            "evidenceCommandRefs": [],
            "evidencePaths": [],
            "evidenceRequired": "保持原条款与原断言；每条原断言须绑定本阶段真实专属正反行为证据后方可判完成。",
        })

    inputs = {
        "schema": "lingxi.r07.stage-inputs.v1",
        "stage": "R07",
        "baseScenarios": [{"id": s["id"], "specMirror": s["specMirror"]} for s in scenarios],
        "requiredLeaves": [{"id": l["id"], "taskIds": l["r00TaskIds"],
                            "executionStageIds": l["r00ExecutionStageIds"]} for l in leaf_rows],
        "extensionBoundaries": [
            {"featureId": fid, "taskId": "R07-T11",
             "acceptanceIds": ["R07-A21", "R07-A22"]} for fid in EXTENSION_IDS
        ],
        "handover35": handover35,
        "priorStageDeferred": [
            {"id": item["id"], "sourceStages": item["sourceStages"],
             "r00ExecutionStageIds": item["r00ExecutionStageIds"]} for item in handover_items
        ],
        "currentStatus": "IMPLEMENTATION_REGISTERED_ACCEPTANCE_NOT_RUN",
    }
    graph = {
        "schemaVersion": 1,
        "stage": "R07",
        "resultVersion": "lingxi.xtask.verify-stage.v1",
        "defaultTimeoutSecs": 1800,
        "commands": commands,
        "scenarios": scenarios,
        "supplementalLeafScenarios": leaf_rows,
        "extensionBoundaries": [
            {
                "featureId": fid,
                "requirement": "REQUIRED",
                "commandRefs": ["r07_t11_dev_entry_and_unload"],
                "note": "T11 扩展边界真实测试证据；缺证据即 FAIL，不并入补充叶集合。",
            } for fid in EXTENSION_IDS
        ],
        "handoverItems": [
            {
                "id": item["id"],
                "featureId": item["featureId"],
                "executionStageIds": item["executionStageIds"],
                "originalTaskIds": item["originalTaskIds"],
                "assertionCount": item["assertionCount"],
                "note": "R04 规划继承的 35 项移交（权威来源 artifacts/rust-tauri/R07/T12/35-handoff-source-resolved-20261018.json）；已含 408 内不重复计数，验收随本叶逐断言证据闭合。",
            } for item in handover35
        ],
        "priorStageDeferred": [
            {
                "id": item["id"],
                "sourceStages": item["sourceStages"],
                "r00ExecutionStageIds": item["r00ExecutionStageIds"],
                "r07LeafPresent": item["r07LeafPresent"],
                "note": "R02—R06 地图声明递延到 R07 的叶；与 35 项不重叠的另一类移交来源，验收同样随本叶闭合。",
            } for item in handover_items
        ],
        "inputDigestSha256": sha(inputs),
    }
    # T11 套件同时承载 A21/A22 与两个扩展边界；登记一次，多处引用。
    commands.setdefault("r07_t11_dev_entry_and_unload",
                        command_for_suite("r07_t11_dev_entry_and_unload"))

    out_map = "--check" not in sys.argv
    out_dir = (ROOT / MAP_PATH).parent
    if "--out" in sys.argv:
        out_dir = Path(sys.argv[sys.argv.index("--out") + 1])
    if out_map:
        (out_dir / INPUT_PATH.name).write_text(json.dumps(inputs, ensure_ascii=False, indent=2) + "\n")
        (out_dir / MAP_PATH.name).write_text(json.dumps(graph, ensure_ascii=False, indent=2) + "\n")
        print(f"R07 map written: {len(commands)} commands, {len(scenarios)} scenarios, "
              f"{len(leaf_rows)} leaves, {len(graph['extensionBoundaries'])} extensions, "
              f"{len(handover_items)} handovers")
    else:
        digest = sha(inputs)
        current = json.loads((ROOT / MAP_PATH).read_text(encoding="utf-8"))
        ok = current.get("inputDigestSha256") == digest
        if "--evidence" in sys.argv:
            receipt_dir = Path(sys.argv[sys.argv.index("--evidence") + 1])
            receipt_dir.mkdir(parents=True, exist_ok=True)
            (receipt_dir / "inputs-check.json").write_text(json.dumps({
                "schema": "lingxi.r07.inputs-check.v1",
                "digestMatch": ok,
                "expectedDigestSha256": digest,
                "mapDigestSha256": current.get("inputDigestSha256"),
            }, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        print("inputs digest " + ("match" if ok else "MISMATCH"))
        return 0 if ok else 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
