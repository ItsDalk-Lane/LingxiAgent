#!/usr/bin/env python3
"""R09-T06 证据生产者：权限/CSP/自定义命令收口（A11）与生产无测试控制面（A12）。

套件（由 `r09_suites.py --suite <name>` 分派；阶段门内真实执行）：
  - `r09_t06_capability_matrix`：A11 静态半边（真实执行 Lead 的
    `r09_capability_audit.py`，逐 case 转写）+ 运行期逐窗口 invoke 负向矩阵的
    **计划生成**（窗口 × 命令 × 期望 allow/deny 写入 runtime-matrix-plan.json）。
    运行期矩阵需要 GUI/wdio（Lead 明确 cleared 前禁止启动 Tauri app），因此全部
    记 `blocked::*` 且套件 exit 2（门真红），不伪造通过。
  - `r09_t06_release_attack_surface`：A12 真实扫描当前 release `.app`
    （`r09_release_attack_surface.py`：wdio/dev 端口/devtools/测试 env、sidecar
    固定二进制与 build.json 摘要、CSP、ACL 命令面），逐 case 转写。

证据纪律：所有 case 来自真实命令退出码 / 真实产物字节；blocked 项 ok=false；
Producer 内建真实 `~/.lingxi` 前后摘要断言；不启动任何 Tauri GUI/安装包。

用法：
  python3 -B scripts/rust-tauri/r09_t06.py --suite r09_t06_capability_matrix \
      --evidence artifacts/rust-tauri/R09/dev/t06/capability-matrix-01
  python3 -B scripts/rust-tauri/r09_t06.py --suite r09_t06_release_attack_surface \
      --evidence artifacts/rust-tauri/R09/dev/t06/release-surface-01
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[1]
sys.path.insert(0, str(SCRIPTS))

from r09_producer import Producer, sha256_file  # noqa: E402

POLICY_PATH = ROOT / "docs/rust-tauri/R09/R09_COMMAND_POLICY.json"
AUDIT_SCRIPT = ROOT / "scripts/rust-tauri/r09_capability_audit.py"
SCAN_SCRIPT = ROOT / "scripts/rust-tauri/r09_release_attack_surface.py"
DEFAULT_APP = ROOT / "desktop/src-tauri/target/release/bundle/macos/Lingxi.app"

# 运行期矩阵的“不可信/远程内容”断言组：远程网页 webview 不加入任何 capability。
REMOTE_CONTENT_CASES = [
    ("runtime.remote-webview-zero-acl",
     "远程内容 webview 调用任一自定义命令必须 100% 拒绝（不加入任何 capability）"),
    ("runtime.token-not-in-url-or-dom",
     "viewer/browser-viewer 拿到的上下文不含服务 token；token 不出现在 URL/query/DOM"),
    ("runtime.csp-blocked-remote-script",
     "注入远程 <script>/unsafe-eval/外链 frame 的预览页必须被 CSP 阻止（无 violation 放行）"),
    ("runtime.canary-command-release-absent",
     "e2e canary 命令只在 debug/e2e-test 构建存在；release 调用返回 command not found"),
    ("runtime.debug-command-release-denied",
     "debug_open_onboarding(_preview) 在 release 任何窗口均不可调用"),
]


def read_json(path: Path) -> dict | None:
    try:
        return json.loads(path.read_text(encoding="utf-8")) if path.is_file() else None
    except json.JSONDecodeError:
        return None


def build_runtime_matrix_plan(policy: dict) -> dict:
    """由意图策略生成逐窗口运行期矩阵（GUI 放行后由 wdio 执行）。

    负向集合 = 全局命令面 − 该窗口 allowedCommands：任何窗口能调用白名单外的
    命令都说明 capability 叠加扩权。正路径 = allowedCommands 全部可调用（不含
    debug/dev-only 命令——那些在 release 应为 not found）。
    """
    surface: set[str] = set()
    for spec in (policy.get("windows") or {}).values():
        surface.update(spec.get("allowedCommands") or [])
    surface.update(policy.get("devOnlyCommands") or [])
    windows = (policy.get("windows") or {})
    negative = []
    positive = []
    for label, spec in sorted(windows.items()):
        allowed = set(spec.get("allowedCommands") or [])
        for command in sorted(allowed):
            positive.append({"window": label, "command": command, "expect": "allow"})
        for command in sorted(surface - allowed):
            negative.append({"window": label, "command": command, "expect": "deny",
                             "reason": "not in window allowedCommands (ACL must reject)"})
    per_window_negative = {
        label: len([row for row in negative if row["window"] == label]) for label in windows
    }
    return {
        "schema": "lingxi.r09.t06-runtime-matrix-plan.v1",
        "generatedFrom": "docs/rust-tauri/R09/R09_COMMAND_POLICY.json",
        "harness": {
            "build": "cargo build --features r09-host,e2e-test（wdio webdriver 插件）",
            "driver": "tauri-plugin-wdio-webdriver + WebDriver 客户端按窗口 label 切换",
            "assertion": "invoke 抛 ACL deny；页面自报与驱动端互证；宿主诊断/事件记录无越权命令",
            "blockedBy": "BLOCKED-GUI-SMOKE：Lead 明确 cleared 前禁止启动 Tauri GUI/安装包",
        },
        "positiveCount": len(positive),
        "negativeCount": len(negative),
        "negativeCountByWindow": per_window_negative,
        "positiveMatrix": positive,
        "negativeMatrix": negative,
    }


def build_candidates() -> dict:
    """记录 evidence（含 hooks）与交付 release 两类构建候选，供 T07 G1/G2 只读消费。"""
    candidates = {}
    evidence_receipt = ROOT / "artifacts/rust-tauri/R09/dev/t06/build-evidence-01/tauri-build-debug.json"
    delivery_receipt = ROOT / "desktop/src-tauri/.tauri-build/build-receipt-release.json"
    for key, path in (("evidence", evidence_receipt), ("delivery", delivery_receipt)):
        if not path.is_file():
            candidates[key] = {"receipt": str(path.relative_to(ROOT)), "present": False}
            continue
        receipt = json.loads(path.read_text(encoding="utf-8"))
        features = ((receipt.get("inputs") or {}).get("features")) or []
        artifacts = receipt.get("artifacts") or {}
        candidates[key] = {
            "receipt": str(path.relative_to(ROOT)),
            "present": True,
            "mode": receipt.get("mode"),
            "evidenceBuild": receipt.get("evidenceBuild"),
            "features": features,
            "executable": artifacts.get("executable"),
            "executableSha256": artifacts.get("executableSha256"),
            "appBundle": artifacts.get("appBundle"),
            "rendererDigest": (receipt.get("inputs") or {}).get("rendererDigest"),
            "themeBundleSha256": (receipt.get("inputs") or {}).get("themeBundleSha256"),
        }
    return candidates


def run_capability_matrix(producer: Producer) -> int:
    policy = read_json(POLICY_PATH)
    producer.check("static::policy-file-present", policy is not None,
                   detail={"path": str(POLICY_PATH.relative_to(ROOT))})
    audit_dir = producer.evidence / "capability-audit"
    step = producer.run_step(
        "static::capability-audit",
        ["python3", "-B", str(AUDIT_SCRIPT.relative_to(ROOT)),
         "--evidence", str(audit_dir)],
        timeout=600, expect_zero=False,
    )
    audit = read_json(audit_dir / "capability-audit.json")
    producer.check("static::audit-ran", audit is not None,
                   detail={"exitCode": step.returncode})
    candidates = build_candidates()
    evidence = candidates.get("evidence") or {}
    delivery = candidates.get("delivery") or {}
    producer.check("build::evidence-receipt-present", bool(evidence.get("present")),
                   detail=evidence)
    producer.check("build::evidence-build-marked-test-hooks",
                   bool(evidence.get("evidenceBuild"))
                   and "r09-test-hooks" in (evidence.get("features") or []),
                   detail={"features": evidence.get("features"),
                           "evidenceBuild": evidence.get("evidenceBuild")})
    producer.check("build::delivery-receipt-present", bool(delivery.get("present")),
                   detail=delivery)
    producer.check("build::delivery-release-no-test-hooks",
                   delivery.get("present") is True
                   and delivery.get("evidenceBuild") is False
                   and "r09-test-hooks" not in (delivery.get("features") or []),
                   detail={"features": delivery.get("features"),
                           "evidenceBuild": delivery.get("evidenceBuild")})
    if audit is not None:
        for row in audit.get("cases") or []:
            producer.check(f"static::{row['case']}", row.get("ok"),
                           1 if row.get("ok") else 0, row.get("detail"))
        gap = audit.get("gapLedger") or {}
        producer.check("static::gap-ledger-empty", (gap.get("remaining") or 0) == 0,
                       detail={"remaining": gap.get("remaining"),
                               "pendingImplementation": gap.get("actualPendingImplementation"),
                               "pendingWindowGrants": gap.get("actualPendingWindowGrants")},
                       )
        producer.check("static::audit-exit-code-zero", step.returncode == 0,
                       detail={"exitCode": step.returncode,
                               "status": audit.get("status"),
                               "failedCases": audit.get("failedCases")})
        (producer.evidence / "runtime-matrix-plan.json").write_text(
            json.dumps(build_runtime_matrix_plan(policy), ensure_ascii=False, indent=1) + "\n",
            encoding="utf-8")
        plan = build_runtime_matrix_plan(policy)
        producer.check("static::runtime-matrix-plan-generated",
                       plan["positiveCount"] > 0 and plan["negativeCount"] > 0,
                       detail={"positive": plan["positiveCount"],
                               "negative": plan["negativeCount"]})
        # 运行期矩阵：逐窗口 aggregated blocked；GUI 未被 Lead cleared 前禁止启动。
        blocked = [
            (f"runtime.{label}.negative-matrix",
             f"{label}: {plan['negativeCountByWindow'].get(label, 0)} 条白名单外命令必须全部被 ACL 拒绝") for label in
            sorted((policy.get("windows") or {}).keys())
        ] + [
            (f"runtime.{label}.positive-matrix",
             f"{label}: allowedCommands 正路径（真实 renderer 调用 + 驱动互证）") for label in
            sorted((policy.get("windows") or {}).keys())
        ] + [(case, reason) for case, reason in REMOTE_CONTENT_CASES]
        for case_name, reason in blocked:
            producer.check(f"blocked::{case_name}", False,
                           detail={"blocked": True, "reason": reason,
                                   "unblock": "Lead 明确 cleared GUI 后由 T07 原生 E2E/wdio 执行"})
        (producer.evidence / "static-summary.json").write_text(json.dumps({
            "schema": "lingxi.r09.t06-static-summary.v1",
            "buildCandidates": candidates,
            "policySha256": sha256_file(POLICY_PATH),
            "auditStatus": audit.get("status"),
            "auditExitCode": step.returncode,
            "gapRemaining": gap.get("remaining"),
            "windowComparison": audit.get("windowComparison"),
            "blockedRuntimeGroups": len(blocked),
        }, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        producer.assert_real_home_untouched()
        failed_cases = [row["case"] for row in producer.cases if not row["ok"]]
        blocked_cases = [{"case": row["case"], "reason": (row.get("detail") or {}).get("reason")}
                         for row in producer.cases
                         if row["case"].startswith("blocked::")]
        return producer.finish(status="FAIL", extra={
            "failedCases": failed_cases,
            "blockedCases": blocked_cases,
            "blockedCaseCount": len(blocked_cases),
            "blockedKind": "BLOCKED_GUI_SMOKE",
            "blockedReason": "运行期逐窗口 invoke 负向/正路径矩阵需要 Tauri GUI + wdio；"
                             "Lead 明确 cleared 前禁止启动 Tauri app/安装包",
            "policySha256": sha256_file(POLICY_PATH),
            "auditStatus": audit.get("status"),
            "auditExitCode": step.returncode,
            "gapRemaining": gap.get("remaining"),
            "runtimeMatrixPlan": "runtime-matrix-plan.json",
            "note": "静态半边已真实执行；套件因运行期矩阵 blocked 保持 exit 2（门真红）",
        })
    producer.assert_real_home_untouched()
    return producer.fail("capability audit 未产出 capability-audit.json")


def run_release_attack_surface(producer: Producer) -> int:
    scan_dir = producer.evidence / "release-scan"
    argv = ["python3", "-B", str(SCAN_SCRIPT.relative_to(ROOT)),
            "--evidence", str(scan_dir)]
    if not DEFAULT_APP.is_dir():
        argv.extend(["--app", str(DEFAULT_APP)])
    step = producer.run_step("release::attack-surface-scan", argv,
                             timeout=600, expect_zero=False)
    scan_result = read_json(scan_dir / "result.json")
    scan_cases = read_json(scan_dir / "cases.json")
    producer.check("release::scan-ran", scan_result is not None,
                   detail={"exitCode": step.returncode})
    if scan_cases is not None:
        for row in scan_cases.get("cases") or []:
            producer.check(row["case"], row.get("ok"), 1 if row.get("ok") else 0,
                           row.get("detail"))
    if scan_result is not None:
        producer.check("release::scan-status", scan_result.get("status") == "PASS",
                       detail={"status": scan_result.get("status"),
                               "failedCases": scan_result.get("failedCases")})
        producer.check("release::artifact-stable-during-scan",
                       not scan_result.get("artifactChangedDuringScan"),
                       detail={"artifactChangedDuringScan":
                               scan_result.get("artifactChangedDuringScan")})
    producer.assert_real_home_untouched()
    status = "PASS" if (scan_result or {}).get("status") == "PASS" and step.returncode == 0 \
        else "FAIL"
    return producer.finish(status=status, extra={
        "scannerExitCode": step.returncode,
        "scannerStatus": (scan_result or {}).get("status"),
        "failedCases": (scan_result or {}).get("failedCases"),
        "artifactChangedDuringScan": (scan_result or {}).get("artifactChangedDuringScan"),
        "releaseScanReceipt": "release-scan/result.json",
        "note": "只读扫描产物字节；未启动 app/GUI/安装包",
    })


def run_suite(suite: str, evidence: Path) -> int:
    producer = Producer(suite, evidence)
    try:
        producer.check("isolation::real-home-provided",
                       bool(producer.env().get("LINGXI_HOME")))
        if suite == "r09_t06_capability_matrix":
            return run_capability_matrix(producer)
        if suite == "r09_t06_release_attack_surface":
            return run_release_attack_surface(producer)
        return producer.fail(f"未知 T06 套件: {suite}")
    except Exception as error:  # noqa: BLE001 - 生产者必须真实失败并写回执
        return producer.fail(f"{type(error).__name__}: {error}")


if __name__ == "__main__":
    args = sys.argv[1:]
    if "--suite" not in args or "--evidence" not in args:
        print("usage: r09_t06.py --suite <name> --evidence <dir>", file=sys.stderr)
        sys.exit(64)
    suite_name = args[args.index("--suite") + 1]
    evidence_dir = Path(args[args.index("--evidence") + 1])
    if not evidence_dir.is_absolute():
        evidence_dir = ROOT / evidence_dir
    sys.exit(run_suite(suite_name, evidence_dir))
