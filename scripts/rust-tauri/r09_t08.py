#!/usr/bin/env python3
"""R09-T08 证据生产者：Tauri 替壳关卡与交接（R09-A15 / R09-A16）。

两套件（由 `r09_suites.py --suite <name>` 分派；阶段图 R09.json 已注册）：

  - `r09_t08_shell_parity`（A15｜同内核同前端对照成立）
      固定候选因子（同一 Rust 服务 / 同一 renderer 摘要 / 同一 R08 v3Base 夹具 /
      同一 protocol-data epoch），从**既有真实证据**聚合 Electron（R08 基线）与
      Tauri（R09 候选）的宿主面逐项对照：T01 同窗 DOM/服务身份、T02 supervisor
      identity/health/退出语义，以及 T03 inventory 50 能力 / 71 命令 / 46 共享叶。
      差异只允许登记在 `RECORDED_APPROVALS`（basis = 文档路径 + 可校验引文）；
      无 GUI/运行时证据的项记 `pending`/`blocked`，绝不当 PASS。
      输出：cases.json / shell-parity-cases.json / parity-matrix.json /
      SHELL_PARITY_RESULTS.<suite>.json（草稿；Lead 汇总成 docs 最终文件）。

  - `r09_t08_host_completeness`（A16｜宿主功能无占位）
      遍历 R00 全部原生动作入口（`docs/rust-tauri/R00/ENTRYPOINTS.json`）+
      T03 inventory 50 能力 + T06 71 命令四集合 + `R09_LEAF_LEDGER.json` 74 叶：
      每项核对真实实现入口（模块/命令/插件/窗口）、真实消费者、Tauri
      capability/permission、证据命令、平台状态；扫描
      `NOT_IMPLEMENTED`/`todo!`/空实现/无理由 no-op/UI 入口未接线（隐藏按钮），
      未登记 blocked/deferred 的入口缺口一律 FAIL。
      输出：cases.json / host-cases.json / host-matrix.json / placeholder-scan.json。

证据纪律（与 R09 其它生产者一致）：
  - 只读：不启动 Tauri/Electron GUI，不执行 desktop/src-tauri 构建，不写冻结候选；
  - 上游证据优先取**同一次正式门**产物（`{EVIDENCE}/SUITES/<suite>/`）；dev 自检
    时回退到 `artifacts/rust-tauri/R09/dev/**` 的最终目录，并把来源写进产物
    （`sourceKind=in-gate|dev-fallback`），不把 dev 证据冒充当轮门证据；
  - 所有结论来自真实文件字节/真实执行 receipt；pending/blocked 记 ok=false 或
    显式登记；Producer 内建真实 `~/.lingxi` 前后摘要断言。

用法：
  python3 -B scripts/rust-tauri/r09_t08.py --suite r09_t08_shell_parity \
      --evidence artifacts/rust-tauri/R09/dev/t08/01-shell-parity
  python3 -B scripts/rust-tauri/r09_t08.py --suite r09_t08_host_completeness \
      --evidence artifacts/rust-tauri/R09/dev/t08/02-host-completeness
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
import traceback
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[1]
sys.path.insert(0, str(SCRIPTS))

from r09_producer import (  # noqa: E402
    Producer,
    sha256_bytes,
    sha256_file,
    sha256_tree,
    tree_digest,
)

DOCS09 = ROOT / "docs/rust-tauri/R09"
DOCS08 = ROOT / "docs/rust-tauri/R08"
R08_GATE = ROOT / "artifacts/rust-tauri/R08/verify-r08-closeout-08"
DEV09 = ROOT / "artifacts/rust-tauri/R09/dev"
TAURI_SRC = ROOT / "desktop/src-tauri/src"
TAURI_CRATE = ROOT / "desktop/src-tauri"
RENDERER_DIST = ROOT / "desktop/dist-renderer"

# ────────────────────────────────────────────────────────────────────────────
# 固定候选（T06 candidate-handoff-02 冻结；T08 只读复算，不重建）
# ────────────────────────────────────────────────────────────────────────────
FROZEN = {
    "deliveryRelease": {
        "appBundle": "desktop/src-tauri/target/release/bundle/macos/Lingxi.app",
        "files": {
            "Contents/Info.plist": "da9ebff0b848d9ff1e3e6e5ac545c7270904ceb2a65677095cbb3e400e01919a",
            "Contents/MacOS/lingxi-resource-desktop": "17523a17da7dcf0bd26df5bb952e3303cd7508527de3acbd6fe9ebdd87e16203",
            "Contents/MacOS/lingxi-service": "55d6edb15cb9e7e075c2845c75a2eaabdd8de463992afdbde2959361cc88a26d",
            "Contents/Resources/Lingxi.icns": "0951f493d51b3537f2b5826d45e117bba1ead1802c38658e2c0ebb1d7706b4d3",
            "Contents/Resources/rust-service/build.json": "40085e84a6bed3f2f4ad9f64d725ba91b19e6518886007f4d01dd9e21f23d353",
            "Contents/_CodeSignature/CodeResources": "09489029ea2e31c170a9f956fcef0f4021d8a41f881939b2c6384b0c55f019d9",
        },
        "appTreeSha256": "2dc38bc36c29fd2c28e9576414dc028bab1b678c1b18557bfe0654d600fe0d0c",
        "hostExeSha256": "17523a17da7dcf0bd26df5bb952e3303cd7508527de3acbd6fe9ebdd87e16203",
        "sidecarSha256": "55d6edb15cb9e7e075c2845c75a2eaabdd8de463992afdbde2959361cc88a26d",
        "buildJsonSha256": "40085e84a6bed3f2f4ad9f64d725ba91b19e6518886007f4d01dd9e21f23d353",
        "features": ["r09-host"],
        "evidenceBuild": False,
        "releaseReceipt": "desktop/src-tauri/.tauri-build/build-receipt-release.json",
        "releaseReceiptSha256": "3b78a519599bda87b89d75dd17b984e74c1890bab67e0feacac4207581f69b22",
        "serviceContentSha256": "73471fbf2f561e9c8003dbe4b9fdd8aea1a2e953624ae716a4a058ec5383eaa0",
        "protocol": {"serverKind": "lingxi-service", "wireProtocolMin": 1,
                     "wireProtocolMax": 1, "dataEpoch": 2},
    },
    "evidenceBuild": {
        "executable": "desktop/src-tauri/target/debug/lingxi-resource-desktop",
        "executableSha256": "c3622805999f1a8871dc8faa1ffebd42ab7a0bd3a61501c8721f1fb0160ba071",
        "features": ["e2e-test", "r09-host", "r09-test-hooks"],
        "evidenceBuild": True,
        "receipt": "desktop/src-tauri/.tauri-build/build-receipt-debug.json",
        "receiptSha256": "78a588ce504f876cffbee38b75208676ce2f56d67badaafc55eb60d6f245263d",
        "wrapperMainExeSha256": "8fe7733e23cdadd62b31f2ed99197efd23f47fe0979f35f4145a31f168685573",
    },
    "renderer": {
        "path": "desktop/dist-renderer",
        "digest": "d09359bd5bc87071c7e4cfc7fffb99553bc39b1345dd83946dcd14c51f244f31",
        "fileCount": 644,
        "themeBundleSha256": "455d963912bbca2aea3137660ed771657c68cf629d5b13ff50712b37efbbae73",
    },
    "fixtures": {
        "v3Base": "80a8d2c2640e2b2974b299a10f36679bc19daf0911e51f82f0c43399cb32feaa",
        "v2Bulk": "40f42dc1fdb7dc026ee1ce6444914a02a085c12c53b7598e70fc453fdb388c9e",
        "source": "R08 a16-versions fixtureAnchors / T07 fixtures deterministic case",
    },
    "r08Baseline": {
        "gate": "artifacts/rust-tauri/R08/verify-r08-closeout-08/verify-stage-result.json",
        "testedSha": "aa99d2ec1dae7f0aef5971bab049734f22fcf7d8",
        "a16Versions": "artifacts/rust-tauri/R08/verify-r08-closeout-08/SUITES/r08_a16_versions/a16-versions.json",
        "rendererDist": "3e5339cc92f9f29a30846a101b3e4636d3b4e2d7f896980c57b8f31b129a89ef",
        "rustBinary": "b855e5d638aea6605c1b2d634377f230e1b6118e71522eb98a8be841ed10faa0",
        "contractVersions": "6d86934755a23e4f35a6dbd68957ff3d302a3ce5f6d61d40066efd68f33a6f96",
        "abParityCases": "artifacts/rust-tauri/R08/verify-r08-closeout-08/SUITES/r08_t07_ab/ab-parity-cases.json",
        "abOutputLog": "artifacts/rust-tauri/R08/verify-r08-closeout-08/SUITES/r08_t07_ab/output.log",
        "abExpected": {"tasks": 71, "parity": 68, "recorded": 3, "differs": 0},
    },
}

# ────────────────────────────────────────────────────────────────────────────
# v2 候选交接（T08 输入动态化）：优先 candidate-handoff-v2（或其 freeze 目录），
# 回退 v1 冻结值；T08 只读消费，不构建、不启动 GUI。
# ────────────────────────────────────────────────────────────────────────────
CANDIDATE_META = {
    "candidateVersion": "v1",
    "source": "static FROZEN（T06 candidate-handoff-02 冻结值）",
    "handoff": "artifacts/rust-tauri/R09/dev/t06/candidate-handoff-02/candidate-handoff.json",
    "gitHead": "aac8cb9fe6bb7bfa7a41d75528221136bfe55e50",
    "rendererDigest": "d09359bd5bc87071c7e4cfc7fffb99553bc39b1345dd83946dcd14c51f244f31",
    "serviceContentSha256": "73471fbf2f561e9c8003dbe4b9fdd8aea1a2e953624ae716a4a058ec5383eaa0",
}


def _read_json_path(path: Path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def _relpath(path: Path) -> str:
    try:
        return str(path.resolve().relative_to(ROOT.resolve()))
    except ValueError:
        return str(path)


def _apply_handoff_payload(raw: dict, source_path: Path) -> None:
    """把 candidate-handoff / docs HANDOFF frozenCandidate 归一化映射到 FROZEN。"""
    delivery = raw.get("delivery") or {}
    evidence = raw.get("g1Evidence") or raw.get("evidenceBuild") or {}
    freeze = raw.get("freeze") or {}
    raw_files = delivery.get("files") or freeze.get("files") or {}
    files = {
        name: (value.get("sha256") if isinstance(value, dict) else value)
        for name, value in raw_files.items()
    }
    app_rel = delivery.get("app") or delivery.get("appBundle") or FROZEN["deliveryRelease"]["appBundle"]
    app_path = ROOT / app_rel
    build_json = app_path / "Contents/Resources/rust-service/build.json"
    protocol = FROZEN["deliveryRelease"]["protocol"]
    service_content = FROZEN["deliveryRelease"]["serviceContentSha256"]
    if build_json.is_file():
        try:
            build = json.loads(build_json.read_text(encoding="utf-8"))
            protocol = build.get("protocol") or protocol
            service_content = build.get("contentSha256") or service_content
        except (OSError, ValueError):
            pass
    host_exe = (files.get("Contents/MacOS/lingxi-resource-desktop")
                or delivery.get("hostExeSha256") or delivery.get("executableSha256"))
    sidecar = files.get("Contents/MacOS/lingxi-service") or delivery.get("sidecarSha256")
    build_json_sha = (files.get("Contents/Resources/rust-service/build.json")
                      or delivery.get("buildJsonSha256"))
    FROZEN["deliveryRelease"].update({
        "appBundle": str(app_rel),
        # 保留 v1 已知文件期望，再用 v2 handoff 覆盖已有字段（docs handoff 可能只给 4 个文件）
        "files": {**FROZEN["deliveryRelease"].get("files", {}), **{k: v for k, v in files.items() if v}},
        "appTreeSha256": delivery.get("appTreeSha256") or freeze.get("appTreeSha256")
                         or FROZEN["deliveryRelease"].get("appTreeSha256"),
        "hostExeSha256": host_exe or FROZEN["deliveryRelease"].get("hostExeSha256"),
        "sidecarSha256": sidecar or FROZEN["deliveryRelease"].get("sidecarSha256"),
        "buildJsonSha256": build_json_sha or FROZEN["deliveryRelease"].get("buildJsonSha256"),
        "features": delivery.get("featureList") or delivery.get("features")
                    or FROZEN["deliveryRelease"].get("features") or ["r09-host"],
        "evidenceBuild": False,
        "releaseReceipt": delivery.get("receipt") or FROZEN["deliveryRelease"].get("releaseReceipt"),
        "releaseReceiptSha256": delivery.get("receiptSha256")
                                or FROZEN["deliveryRelease"].get("releaseReceiptSha256"),
        "serviceContentSha256": service_content,
        "protocol": protocol,
    })
    FROZEN["evidenceBuild"].update({
        "executable": evidence.get("executable") or FROZEN["evidenceBuild"]["executable"],
        "executableSha256": evidence.get("executableSha256") or FROZEN["evidenceBuild"]["executableSha256"],
        "features": evidence.get("featureList") or evidence.get("features") or FROZEN["evidenceBuild"]["features"],
        "evidenceBuild": bool(evidence.get("evidenceBuild", True)),
        "receipt": evidence.get("receipt") or FROZEN["evidenceBuild"]["receipt"],
        "receiptSha256": evidence.get("receiptSha256") or FROZEN["evidenceBuild"].get("receiptSha256"),
        "wrapperMainExeSha256": evidence.get("wrapperMainExeSha256"),
    })
    renderer_digest = (delivery.get("rendererDigest") or raw.get("rendererDigest")
                       or (raw.get("renderer") or {}).get("rendererDigest")
                       or FROZEN["renderer"]["digest"])
    theme_sha = (delivery.get("themeBundleSha256") or (raw.get("renderer") or {}).get("themeBundleSha256")
                 or FROZEN["renderer"]["themeBundleSha256"])
    file_count = delivery.get("rendererFileCount") or (raw.get("renderer") or {}).get("fileCount")
    FROZEN["renderer"].update({
        "digest": renderer_digest,
        "themeBundleSha256": theme_sha,
        "fileCount": int(file_count) if file_count else None,
    })
    version_match = re.search(r"candidate-handoff-(v\d+(?:\.\d+)?)", source_path.as_posix())
    CANDIDATE_META.update({
        "candidateVersion": raw.get("candidateVersion")
                            or (version_match.group(1) if version_match
                                else ("v2" if "v2" in source_path.as_posix() else "v1")),
        "source": _relpath(source_path),
        "handoff": _relpath(source_path),
        "gitHead": raw.get("gitHead"),
        "frozenAt": freeze.get("frozenAt") or raw.get("frozenAt"),
        "rendererDigest": renderer_digest,
        "serviceContentSha256": service_content,
        "deliveryAppTreeSha256": FROZEN["deliveryRelease"].get("appTreeSha256"),
        "evidenceBuildSha256": FROZEN["evidenceBuild"].get("executableSha256"),
    })


def _docs_handoff_payload(path: Path) -> dict | None:
    """把 docs/R09_HANDOFF.json.frozenCandidate 归一成 candidate-handoff 形状。"""
    raw = _read_json_path(path)
    if not isinstance(raw, dict):
        return None
    frozen = raw.get("frozenCandidate") or {}
    if not frozen:
        return None
    delivery = frozen.get("deliveryRelease") or {}
    evidence = frozen.get("evidenceBuildG1") or frozen.get("evidenceBuild") or {}
    renderer = frozen.get("renderer") or {}
    # docs HANDOFF 只记关键哈希；同版本 candidate-handoff 若存在，补齐 per-file/receipt 明细
    supplement_path = DEV09 / "t06/candidate-handoff-v2/candidate-handoff.json"
    supplement = _read_json_path(supplement_path) if supplement_path.is_file() else None
    if isinstance(supplement, dict):
        supp_delivery = supplement.get("delivery") or {}
        same_version = (supplement.get("candidateVersion") or "v2") == (frozen.get("candidateVersion") or "v2")
        supp_tree = supp_delivery.get("appTreeSha256") or (supplement.get("freeze") or {}).get("appTreeSha256")
        if same_version and supp_tree and supp_tree == delivery.get("appTreeSha256"):
            merged = dict(supp_delivery)
            merged.update({k: v for k, v in delivery.items() if v})
            delivery = merged
            evidence = {**supplement.get("g1Evidence", {}), **{k: v for k, v in evidence.items() if v}}
    return {
        "candidateVersion": frozen.get("candidateVersion"),
        "generatedAt": raw.get("generatedAt"),
        "gitHead": raw.get("gitHead"),
        "delivery": {
            "app": delivery.get("appBundle") or delivery.get("app"),
            "files": delivery.get("files"),
            "appTreeSha256": delivery.get("appTreeSha256"),
            "hostExeSha256": delivery.get("hostExeSha256"),
            "sidecarSha256": delivery.get("sidecarSha256"),
            "buildJsonSha256": delivery.get("buildJsonSha256"),
            "featureList": delivery.get("features") or delivery.get("featureList"),
            "receipt": delivery.get("receipt"),
            "receiptSha256": delivery.get("receiptSha256"),
            "rendererDigest": renderer.get("rendererDigest") or delivery.get("rendererDigest"),
            "themeBundleSha256": renderer.get("themeBundleSha256") or delivery.get("themeBundleSha256"),
        },
        "g1Evidence": evidence,
        "freeze": {"frozenAt": frozen.get("frozenAt")},
    }


def _apply_candidate_handoff(path: Path) -> None:
    raw = _read_json_path(path)
    if not isinstance(raw, dict):
        return
    delivery = raw.get("delivery") or {}
    if not (delivery.get("files") or delivery.get("appTreeSha256") or delivery.get("appTreeSha256")):
        return
    _apply_handoff_payload(raw, path)


def _load_candidate_handoff() -> None:
    # 优先级：显式 env → 最新 candidate-handoff-vN（v3+ 刚冻结时先于 docs 生效）→ docs HANDOFF → v1。
    # 若 docs 与 candidate-handoff 同版本，两者值一致；docs 由 Lead 汇总为最终权威。
    handoff_dirs = sorted(
        (path for path in (DEV09 / "t06").glob("candidate-handoff-v*") if path.is_dir()),
        key=lambda path: path.name,
        reverse=True,
    )
    candidates = [os.environ.get("LINGXI_R09_T08_HANDOFF")]
    candidates.extend(str(path / "candidate-handoff.json") for path in handoff_dirs)
    candidates.append(str(DOCS09 / "R09_HANDOFF.json"))
    candidates.append(str(DEV09 / "t06/candidate-handoff-02/candidate-handoff.json"))
    for candidate in candidates:
        if not candidate:
            continue
        path = Path(candidate)
        if not path.is_file():
            continue
        raw = _docs_handoff_payload(path) if path.name == "R09_HANDOFF.json" else _read_json_path(path)
        if not isinstance(raw, dict):
            continue
        _apply_handoff_payload(raw, path)
        return


_load_candidate_handoff()


# A15 RECORDED_APPROVALS：**只登记「差异来自宿主且已由文档裁决/登记」的项**。
# 每项必须给出 basis（repo 相对路径 + 该文件里逐字存在的引文），否则 suite FAIL。
RECORDED_APPROVALS = [
    {
        "id": "R09-ADJ-HOST-01",
        "diff": "主页面来源：Electron=file://<repo>/desktop/dist-renderer/index.html ｜ Tauri=tauri://localhost（嵌入式自定义协议）",
        "affected": ["host:main-window", "host:app-ready", "host:server-connection-info"],
        "basisPath": "docs/rust-tauri/R09/IMPLEMENTATION_PLAN.md",
        "basisQuote": "Tauri webview 无法获得 loopback http 来源",
        "note": "D2（同一 React 产物 dist-renderer）+ D6（精确 origin 接缝）；T01 证据两侧标题/DOM/服务身份一致，差异仅为页面来源。",
    },
    {
        "id": "R09-ADJ-HOST-02",
        "diff": "托盘菜单：Electron 第 4 项「修复组件…」在 Tauri 无等价实现，按 Lead 裁决移除（4 项菜单：show/settings/—/quit），不保留 no-op",
        "affected": ["host:tray"],
        "basisPath": "docs/rust-tauri/R09/BLOCKED.md",
        "basisQuote": "托盘「修复组件…」原能力在 Tauri 无等价实现",
        "note": "T03-REPAIR-02 登记；菜单项移除而非隐藏死按钮。",
    },
    {
        "id": "R09-ADJ-HOST-03",
        "diff": "更新链（GitHub release 检查 / 自动更新 / OTA 列车）R09 未实现：Tauri 命令面 deferred，UI 显式无更新状态",
        "affected": ["host:update-github-check", "host:update-auto", "host:update-train-ota"],
        "basisPath": "docs/rust-tauri/R09/T06_PROGRESS.md",
        "basisQuote": "更新链（GitHub check/自动更新/OTA）属 R10 验收范围",
        "note": "T06 policy deferredCommands=R10-update-chain；11 条命令不注册/不授予（gapLedger=0）。",
    },
    {
        "id": "R09-ADJ-HOST-04",
        "diff": "页截图捕获（screenshot_render）R09 未实现：Tauri/Wry 无稳定捕获 API，命令 deferred",
        "affected": ["host:screenshot-render"],
        "basisPath": "docs/rust-tauri/R09/T06_PROGRESS.md",
        "basisQuote": "Tauri/Wry 无稳定页面捕获 API（T04 OQ-04 BLOCKED）",
        "note": "T04-SHOT-01；策略层保留但无命令、不授予。",
    },
    {
        "id": "R09-ADJ-HOST-05",
        "diff": "卡片帧导航 guard：Tauri/Wry 无 will-frame-navigate 等价，flag 保持 false，HTTP 卡片降级为仅同源（显式降级，不假安全）",
        "affected": ["host:card-frame-navigation-guard"],
        "basisPath": "docs/rust-tauri/R09/IMPLEMENTATION_PLAN.md",
        "basisQuote": "导航 guard 保持 flag=false 并在卡片层显式降级",
        "note": "OQ-07；T03_T04_PROGRESS §3 与 tauri-host-capabilities 理由同源。",
    },
    {
        "id": "R09-ADJ-HOST-06",
        "diff": "拖入文件路径：Electron preload getFilePath(webUtils.getPathForFile)；Tauri 无等价，File 对象不暴露路径，改走原生 drop 适配层（file_drop 校验绝对路径）",
        "affected": ["host:file-drop-paths"],
        "basisPath": "docs/rust-tauri/R09/T03_HOST_CAPABILITY_INVENTORY.md",
        "basisQuote": "Tauri 无 webUtils 等价",
        "note": "R-06 同类平台限制；仅宿主适配，不改业务语义。",
    },
    {
        "id": "R09-ADJ-HOST-07",
        "diff": "onboarding_complete：Electron(Rust 服务模式) 与 Tauri 均显式 NOT_IMPLEMENTED（服务无 setup-complete 等价路由）——两侧行为一致，非换壳引入的差异",
        "affected": ["host:onboarding"],
        "basisPath": "docs/rust-tauri/R09/T03_T04_PROGRESS.md",
        "basisQuote": "Rust 服务模式 onboarding completion 现役 Electron 即显式 throw",
        "note": "T03-ONBOARD-01；登记性质为「与现役一致的显式失败」，不冒充能力。",
    },
    {
        "id": "R09-ADJ-HOST-08",
        "diff": "文件能力（DOCX/XLSX 预览、workspace watch、可观测导出）R09 未实现：命令面 deferred（T03-FILE-01），UI 调用显式受限",
        "affected": ["host:docx-xlsx-preview", "host:observability-export", "host:file-watch"],
        "basisPath": "docs/rust-tauri/R09/T06_PROGRESS.md",
        "basisQuote": "T03 下一轮文件能力（docx/xlsx 预览、workspace watch、可观测导出）尚未实现",
        "note": "T06 policy deferredCommands=T03-FILE-01（8 条命令）；不提前授予/不登记虚假能力。",
    },
]

# 同一次门内的上游证据映射：suite -> dev 最终目录（dev 自检回退来源）。
UPSTREAM = {
    "r09_t01_react_parity": {"dev": "artifacts/rust-tauri/R09/dev/t01/run-08", "need": "PASS"},
    "r09_t01_release_offline": {"dev": "artifacts/rust-tauri/R09/dev/t01/run-release-03", "need": "PASS"},
    "r09_t02_supervisor_ready": {"dev": "artifacts/rust-tauri/R09/dev/t02/phase2-supervisor-03", "need": "PASS"},
    "r09_t02_process_ownership": {"dev": "artifacts/rust-tauri/R09/dev/t02/phase2-ownership-02", "need": "PASS"},
    "r09_t03_native_matrix": {"dev": "artifacts/rust-tauri/R09/dev/t06/t03t04-rerun-02/r09_t03_native_matrix", "need": "ANY"},
    "r09_t03_ipc_negative": {"dev": "artifacts/rust-tauri/R09/dev/t06/t03t04-rerun-02/r09_t03_ipc_negative", "need": "ANY"},
    "r09_t04_permission_matrix": {"dev": "artifacts/rust-tauri/R09/dev/t06/t03t04-rerun-02/r09_t04_permission_matrix", "need": "ANY"},
    "r09_t04_remote_boundary": {"dev": "artifacts/rust-tauri/R09/dev/t06/t03t04-rerun-02/r09_t04_remote_boundary", "need": "ANY"},
    "r09_t05_browser_pdf": {"dev": "artifacts/rust-tauri/R09/dev/t05/01-a09-per-assertion-final2", "need": "PASS"},
    "r09_t05_untrusted_preview": {"dev": "artifacts/rust-tauri/R09/dev/t05/02-a10-preview-final", "need": "PASS"},
    "r09_t06_capability_matrix": {"dev": "artifacts/rust-tauri/R09/dev/t06/v32-capability-matrix-01", "need": "ANY"},
    "r09_t06_release_attack_surface": {"dev": "artifacts/rust-tauri/R09/dev/t06/v32-release-suite-01", "need": "PASS"},
    "r09_t07_native_e2e": {"dev": "artifacts/rust-tauri/R09/dev/t07/g1-hidden-v32-02", "need": "PASS"},
    "r09_t07_release_smoke": {"dev": "artifacts/rust-tauri/R09/dev/t07/static-03-release_smoke", "need": "ANY"},
    # 静态辅助证据（非门内命令，作为 T06 审计/扫描的最终开发证据）
    "audit_capability": {"dev": "artifacts/rust-tauri/R09/dev/t06/v32-audit-01", "need": "PASS"},
    "release_scan": {"dev": "artifacts/rust-tauri/R09/dev/t06/v32-scan-01", "need": "PASS"},
}

FORBIDDEN_TAURI_WRITES = (
    "desktop/src-tauri/",  # 冻结：只读
)


# ────────────────────────────────────────────────────────────────────────────
# 通用读取/解析工具
# ────────────────────────────────────────────────────────────────────────────
def rel(path: Path) -> str:
    try:
        return str(Path(path).resolve().relative_to(ROOT.resolve()))
    except ValueError:
        return str(path)


def read_json(path: Path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def read_text(path: Path) -> str:
    return Path(path).read_text(encoding="utf-8")


def parse_block_keys(src: str, marker: str) -> list[str]:
    """解析 TS 源里 `const <x> = { ... }` 顶层键（用于读宿主实现/理由表）。"""
    start = src.find(marker)
    if start < 0:
        return []
    brace = src.find("{", start)
    if brace < 0:
        return []
    depth = 0
    keys: list[str] = []
    pattern = re.compile(r"[{}]|(?:^|\n)( {4})([A-Za-z_][A-Za-z0-9_]*)\s*:")
    for match in pattern.finditer(src[brace:]):
        token = match.group(0)
        if token == "{":
            depth += 1
        elif token == "}":
            depth -= 1
            if depth == 0:
                break
        elif match.group(2) and depth == 1:
            keys.append(match.group(2))
    return keys


def parse_set_literals(src: str, marker: str) -> list[str]:
    start = src.find(marker)
    if start < 0:
        return []
    end = src.find("])", start)
    block = src[start:end if end > 0 else start + 2000]
    return re.findall(r"'([A-Za-z_][A-Za-z0-9_]*)'", block)


def parse_block_map(src: str, marker: str) -> dict[str, str]:
    """解析 `const x: Record<string, string> = { key: '...', ... }` 的 key→首个字符串。"""
    start = src.find(marker)
    if start < 0:
        return {}
    end = src.find("\n};", start)
    block = src[start: end if end > 0 else start + 20000]
    out: dict[str, str] = {}
    for match in re.finditer(r"^\s{2,4}([A-Za-z_$][A-Za-z0-9_$]*):\s*'((?:[^'\\]|\\.)*)'", block, re.M):
        out[match.group(1)] = match.group(2)
    return out


def cfg_test_ranges(src: str) -> list[tuple[int, int]]:
    ranges = []
    for match in re.finditer(r"#\[cfg\(test\)\]", src):
        start = match.start()
        brace = src.find("{", start)
        if brace < 0:
            continue
        depth = 0
        end = len(src)
        for i in range(brace, len(src)):
            if src[i] == "{":
                depth += 1
            elif src[i] == "}":
                depth -= 1
                if depth == 0:
                    end = i + 1
                    break
        ranges.append((start, end))
    return ranges


def in_ranges(pos: int, ranges: list[tuple[int, int]]) -> bool:
    return any(start <= pos < end for start, end in ranges)


def fn_body_spans(src: str, name: str) -> list[str]:
    bodies = []
    for match in re.finditer(r"fn\s+" + re.escape(name) + r"\s*\(", src):
        brace = src.find("{", match.start())
        if brace < 0:
            continue
        depth = 0
        end = len(src)
        for i in range(brace, len(src)):
            if src[i] == "{":
                depth += 1
            elif src[i] == "}":
                depth -= 1
                if depth == 0:
                    end = i + 1
                    break
        bodies.append(src[brace:end])
    return bodies


class EvidenceError(RuntimeError):
    pass


# 交付 release bundle 的规范 6 文件（app tree 摘要口径 = tauri-build.mjs sha256Tree）
DELIVERY_FILE_PATHS = [
    "Contents/Info.plist",
    "Contents/MacOS/lingxi-resource-desktop",
    "Contents/MacOS/lingxi-service",
    "Contents/Resources/Lingxi.icns",
    "Contents/Resources/rust-service/build.json",
    "Contents/_CodeSignature/CodeResources",
]


def _iso_epoch(text: str | None) -> int | None:
    """解析 `2026-10-26T23:24:04+0800` / `2026-10-26 23:24:04` 等为 epoch ms（失败返回 None）。"""
    if not text:
        return None
    match = re.search(r"(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2})(?::(\d{2}))?", text)
    if not match:
        return None
    import datetime
    year, month, day, hour, minute, second = (
        int(match.group(1)), int(match.group(2)), int(match.group(3)),
        int(match.group(4)), int(match.group(5)), int(match.group(6) or 0))
    try:
        # 冻结时间均为 +0800（本机时区）；用固定偏移避免平台差异
        tz = datetime.timezone(datetime.timedelta(hours=8))
        return int(datetime.datetime(year, month, day, hour, minute, second, tzinfo=tz).timestamp() * 1000)
    except ValueError:
        return None


def _freeze_epoch_ms() -> int | None:
    """冻结时刻：当前生效 candidate-handoff（vN）freeze.frozenAt → CANDIDATE_META → docs → 回执。"""
    source = CANDIDATE_META.get("handoff") or CANDIDATE_META.get("source")
    if source:
        path = ROOT / source if not Path(source).is_absolute() else Path(source)
        raw = _read_json_path(path)
        if isinstance(raw, dict):
            epoch = _iso_epoch(((raw.get("freeze") or {}).get("frozenAt")))
            if epoch:
                return epoch
            frozen = raw.get("frozenCandidate") or {}
            epoch = _iso_epoch(frozen.get("frozenAt"))
            if epoch:
                return epoch
    epoch = _iso_epoch(CANDIDATE_META.get("frozenAt"))
    if epoch:
        return epoch
    handoff_dirs = sorted(
        (path for path in (DEV09 / "t06").glob("candidate-handoff-v*") if path.is_dir()),
        key=lambda path: path.name, reverse=True)
    for directory in handoff_dirs:
        raw = _read_json_path(directory / "candidate-handoff.json")
        if isinstance(raw, dict):
            epoch = _iso_epoch(((raw.get("freeze") or {}).get("frozenAt")))
            if epoch:
                return epoch
    receipt_path = FROZEN["deliveryRelease"].get("releaseReceipt")
    if receipt_path:
        receipt = _read_json_path(ROOT / receipt_path)
        if isinstance(receipt, dict):
            for key in ("refreshedAtUnixMs", "builtAtUnixMs"):
                if isinstance(receipt.get(key), int):
                    return receipt[key]
    return None


def _handoff_dependency_locks() -> dict:
    """docs HANDOFF（或 v2 handoff）记录的依赖锁 sha，用于区分「mtime 触碰」与真实内容漂移。"""
    docs = _read_json_path(DOCS09 / "R09_HANDOFF.json")
    if isinstance(docs, dict) and isinstance(docs.get("dependencyLocks"), dict):
        return docs["dependencyLocks"]
    handoff = DEV09 / "t06/candidate-handoff-v2/candidate-handoff.json"
    raw = _read_json_path(handoff) if handoff.is_file() else None
    if isinstance(raw, dict) and isinstance(raw.get("dependencyLocks"), dict):
        return raw["dependencyLocks"]
    return {}


def _crate_source_snapshot() -> tuple[dict, dict]:
    """返回 (全树文件→sha, 非构建生成文件→sha)。

    生成物=permissions/autogenerated、capabilities-test、*.generated.conf、build.rs（wrapper 会 touch）。
    wrapper 的 crateSourceDigest 含这些生成物，跨 release/debug 构建不相等，不能当冻结指纹。
    """
    entries = sha256_tree(TAURI_CRATE, skip_dirs=("target", "gen", "binaries", "resources", ".tauri-build"))
    generated_prefixes = ("permissions/autogenerated/", "capabilities-test/")
    generated_names = {"build.rs", "tauri.debug.generated.conf.json", "tauri.release.generated.conf.json"}
    stable = {name: digest for name, digest in entries.items()
              if name not in generated_names and not name.startswith(generated_prefixes)}
    return entries, stable


def check_candidate_drift(producer: Producer, fixed: dict) -> dict:
    """源码漂移检测：冻结规则=任何 desktop/src-tauri 写/构建都会使 T01/T02/T06 证据失效。

    注意：wrapper 的 crateSourceDigest 含构建生成物（permissions/autogenerated、
    *.generated.conf、被 touch 的 build.rs），跨 release/debug 构建不相等，不能当冻结
    指纹。这里用**非生成源文件**做漂移判定：
      - 任一非生成源文件 mtime 晚于对应构建回执 builtAt → 构建后又被改（漂移）；
      - 记录非生成源文件集合摘要供人工复算（sourceFingerprint）。
    """
    import time
    entries, stable = _crate_source_snapshot()
    fingerprint = tree_digest(stable)
    drift: dict = {"crateSource": {"sourceFileCount": len(stable), "sourceFingerprint": fingerprint,
                                   "generatedFileCount": len(entries) - len(stable)},
                   "renderer": {}, "freezeEpochMs": _freeze_epoch_ms()}
    latest_receipt_epoch = None
    for key, receipt_rel in (("release", FROZEN["deliveryRelease"].get("releaseReceipt")),
                             ("evidence", FROZEN["evidenceBuild"].get("receipt"))):
        record = {"sourceFingerprint": fingerprint, "receipt": receipt_rel}
        receipt = _read_json_path(ROOT / receipt_rel) if receipt_rel else None
        if not isinstance(receipt, dict):
            record.update({"builtAtUnixMs": None, "filesNewerThanReceipt": None})
            producer.check(f"candidate::no-source-drift-after-{key}-build", False,
                           detail={"reason": "receipt-missing", "receipt": receipt_rel})
        else:
            inputs = receipt.get("inputs") or {}
            built_at = receipt.get("refreshedAtUnixMs") or receipt.get("builtAtUnixMs")
            newer = []
            if isinstance(built_at, int):
                built_epoch = built_at / 1000.0
                latest_receipt_epoch = max(latest_receipt_epoch or 0, built_at)
                lock_expectations = _handoff_dependency_locks()
                tauri_lock_key = "desktop/src-tauri/Cargo.lock"
                tauri_lock_expected = lock_expectations.get(tauri_lock_key)
                mtime_only = []
                for name in stable:
                    path = TAURI_CRATE / name
                    try:
                        if path.stat().st_mtime > built_epoch:
                            if name == "Cargo.lock" and tauri_lock_expected:
                                if sha256_file(path) == tauri_lock_expected:
                                    mtime_only.append(name)
                                    continue
                            newer.append(name)
                    except OSError:
                        newer.append(name + " (missing)")
                record.update({"mtimeOnlyTouches": mtime_only})
            record.update({"builtAtUnixMs": built_at,
                           "receiptCrateDigest": inputs.get("crateSourceDigest"),
                           "receiptCrateFileCount": inputs.get("crateSourceFileCount"),
                           "filesNewerThanReceipt": sorted(newer)[:20]})
            producer.check(f"candidate::no-source-drift-after-{key}-build", not newer,
                           detail={"newer": sorted(newer)[:20],
                                   "note": "非生成的 desktop/src-tauri 源文件晚于该构建 → 需在最终候选重建后重跑证据"})
        drift["crateSource"][key] = record
    # renderer 源码在 renderer 构建之后是否又被改动（v2 构建期为 build-once-bind-once）
    if RENDERER_DIST.is_dir():
        dist_files = [path for path in RENDERER_DIST.rglob("*") if path.is_file()]
        build_time = max((path.stat().st_mtime for path in dist_files), default=0)
        newer = []
        for root in (ROOT / "desktop/src", ROOT / "package.json"):
            candidates = [root] if root.is_file() else [p for p in root.rglob("*") if p.is_file()]
            for path in candidates:
                if path.suffix not in (".ts", ".tsx", ".js", ".jsx", ".json", ".html", ".css"):
                    continue
                if "/__tests__/" in str(path) or str(path).endswith((".test.ts", ".test.tsx")):
                    continue
                try:
                    if path.stat().st_mtime > build_time:
                        newer.append(_relpath(path))
                except OSError:
                    pass
        drift["renderer"] = {"distNewestMtime": build_time, "sourcesNewerThanDist": sorted(newer)[:20]}
        producer.check("candidate::renderer-source-stable-since-build", not newer,
                       detail={"newer": sorted(newer)[:20],
                               "note": "renderer 源码晚于 dist 构建 → 需重建 renderer 并重绑证据"})
    # 冻结后产物是否又被改写
    freeze_epoch = drift.get("freezeEpochMs")
    if freeze_epoch:
        touched = []
        for name in DELIVERY_FILE_PATHS:
            path = ROOT / FROZEN["deliveryRelease"]["appBundle"] / name
            try:
                if path.stat().st_mtime * 1000 > freeze_epoch:
                    touched.append(name)
            except OSError:
                touched.append(name + " (missing)")
        drift["artifactsTouchedAfterFreeze"] = touched
        producer.check("candidate::artifacts-stable-since-freeze", not touched,
                       detail={"freezeEpochMs": freeze_epoch, "touched": touched})
    else:
        drift["artifactsTouchedAfterFreeze"] = None
        producer.check("candidate::freeze-time-recorded", False,
                       detail={"note": "无法解析冻结时间（handoff freeze.frozenAt 缺失），无法判定产物冻结后是否被改写"})
    fixed["sourceDrift"] = drift
    return drift


def check_evidence_candidate_binding(producer: Producer, book: EvidenceBook) -> None:
    """上游证据是否绑定当前候选：receipt 记录 rendererDigest 时必须相等；
    dev 回退证据必须晚于候选冻结时间（否则是旧候选证据，不得当 v2 证据）。"""
    active_renderer = FROZEN["renderer"]["digest"]
    freeze_epoch = _freeze_epoch_ms()
    for suite, entry in book.books.items():
        receipt = entry.get("receipt") or {}
        renderer_digest = receipt.get("rendererDigest") or (receipt.get("inputs") or {}).get("rendererDigest")
        finished = receipt.get("finishedAtUnixMs")
        if entry["sourceKind"] == "in-gate":
            ok = True
            detail = {"sourceKind": "in-gate", "rendererDigest": renderer_digest,
                      "note": "同一次正式门运行，绑定即门候选"}
        elif renderer_digest:
            ok = renderer_digest == active_renderer
            detail = {"sourceKind": "dev-fallback", "receiptRendererDigest": renderer_digest,
                      "activeRendererDigest": active_renderer}
        elif freeze_epoch and isinstance(finished, int):
            # 冻结窗口容差：同轮构建/静态门可能在 handoff 写入前 1-2 分钟完成，仍属本候选
            tolerance_ms = 300_000
            ok = finished >= (freeze_epoch - tolerance_ms)
            detail = {"sourceKind": "dev-fallback", "finishedAtUnixMs": finished,
                      "freezeEpochMs": freeze_epoch, "toleranceMs": tolerance_ms,
                      "note": "证据产生于候选冻结窗口内" if ok else "证据早于本候选冻结窗口 → 旧候选证据（需重跑）"}
        else:
            ok = True
            detail = {"sourceKind": "dev-fallback", "binding": "unbound-receipt",
                      "note": "receipt 无 rendererDigest/finishedAt，无法绑定；按不冒充记录"}
        producer.check(f"evidence::candidate-binding::{suite}", ok, detail=detail)


# v3.2 T07 hidden 批次（H2/H3）以 T02 套件名/案例名产出证据；dev 自检时并入 T02 suite。
HIDDEN_SUPPLEMENTS = {
    "r09_t02_supervisor_ready": [
        "artifacts/rust-tauri/R09/dev/t07/h3-supervisor-01",
        "artifacts/rust-tauri/R09/dev/t07/h2-supervisor-02",
    ],
    "r09_t02_process_ownership": [
        "artifacts/rust-tauri/R09/dev/t07/h3-ownership-01",
        "artifacts/rust-tauri/R09/dev/t07/h2-ownership-02",
    ],
}


AUTO_LATEST_ROOTS = {
    "r09_t01_react_parity": ["artifacts/rust-tauri/R09/dev/t01"],
    "r09_t01_release_offline": ["artifacts/rust-tauri/R09/dev/t01"],
    "r09_t02_supervisor_ready": ["artifacts/rust-tauri/R09/dev/t02"],
    "r09_t02_process_ownership": ["artifacts/rust-tauri/R09/dev/t02"],
    "r09_t03_native_matrix": ["artifacts/rust-tauri/R09/dev/t06", "artifacts/rust-tauri/R09/dev/t03"],
    "r09_t03_ipc_negative": ["artifacts/rust-tauri/R09/dev/t06", "artifacts/rust-tauri/R09/dev/t03"],
    "r09_t04_permission_matrix": ["artifacts/rust-tauri/R09/dev/t06", "artifacts/rust-tauri/R09/dev/t04"],
    "r09_t04_remote_boundary": ["artifacts/rust-tauri/R09/dev/t06", "artifacts/rust-tauri/R09/dev/t04"],
    "r09_t05_browser_pdf": ["artifacts/rust-tauri/R09/dev/t05"],
    "r09_t05_untrusted_preview": ["artifacts/rust-tauri/R09/dev/t05"],
    "r09_t07_native_e2e": ["artifacts/rust-tauri/R09/dev/t07"],
    "r09_t07_release_smoke": ["artifacts/rust-tauri/R09/dev/t07"],
}


def auto_latest_dev_dir(suite: str) -> Path | None:
    """v3+ 重跑辅助：在 dev 证据根里挑「suite 匹配、最新且满足 need 状态」的目录。

    用于 LINGXI_R09_T08_AUTO_LATEST=1 时的 dev 自检重跑；正式门仍用同门 SUITES 证据。
    只认 result.json 内 suite 字段与请求一致的目录（防止把同任务的其它套件误当证据）；
    PASS 需求只取 PASS，ANY 取最新（含诚实红）。
    """
    spec = UPSTREAM.get(suite) or {}
    roots = AUTO_LATEST_ROOTS.get(suite) or []
    found = []
    for root_rel in roots:
        root = ROOT / root_rel
        if not root.is_dir():
            continue
        for receipt_path in root.rglob("result.json"):
            try:
                receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                continue
            if receipt.get("suite") != suite:
                continue
            status = receipt.get("status")
            if spec.get("need") == "PASS" and status != "PASS":
                continue
            finished = receipt.get("finishedAtUnixMs")
            directory = receipt_path.parent
            if not isinstance(finished, int):
                finished = int(directory.stat().st_mtime * 1000)
            found.append((finished, directory, status))
    if not found:
        return None
    found.sort(key=lambda item: item[0], reverse=True)
    return found[0][1]


class EvidenceBook:
    """上游证据解析：同门优先（{EVIDENCE}/SUITES/<suite>），dev 回退显式标记。"""

    def __init__(self, evidence_dir: Path):
        self.evidence = Path(evidence_dir)
        self.suite_root = self.evidence.parent
        self.books: dict[str, dict] = {}

    def load(self, producer: Producer, suite: str, cases_file: str = "cases.json") -> dict:
        if suite in self.books:
            return self.books[suite]
        spec = UPSTREAM.get(suite)
        if not spec:
            raise EvidenceError(f"未登记的上游证据 {suite}")
        gate_dir = self.suite_root / suite
        if (gate_dir / "result.json").is_file():
            source_kind = "in-gate"
            source_dir = gate_dir
        else:
            source_kind = "dev-fallback"
            source_dir = ROOT / spec["dev"]
            candidates = []
            for supplement_rel in HIDDEN_SUPPLEMENTS.get(suite, []):
                supplement = ROOT / supplement_rel
                if (supplement / "result.json").is_file():
                    candidates.append(supplement)
            if os.environ.get("LINGXI_R09_T08_AUTO_LATEST") == "1":
                latest = auto_latest_dev_dir(suite)
                if latest is not None:
                    source_dir = latest
            candidates.append(source_dir)
            # 选第一个 PASS 候选（优先 T07 hidden 的新候选证据），其余作为 caseMap 合并补充
            chosen = None
            merged_cases = {}
            supplements = []
            for candidate in candidates:
                receipt_path = candidate / "result.json"
                try:
                    candidate_receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
                except (OSError, ValueError):
                    continue
                supplements.append({"dir": rel(candidate), "status": candidate_receipt.get("status"),
                                    "finishedAtUnixMs": candidate_receipt.get("finishedAtUnixMs")})
                case_path = candidate / cases_file
                if case_path.is_file():
                    try:
                        payload = json.loads(case_path.read_text(encoding="utf-8"))
                    except (OSError, ValueError):
                        payload = {}
                    for case in payload.get("cases", []):
                        name = case.get("case")
                        if isinstance(name, str):
                            merged_cases[name] = merged_cases.get(name, True) and bool(case.get("ok"))
                if chosen is None and candidate_receipt.get("status") == "PASS":
                    chosen = candidate
            source_dir = chosen or source_dir
        receipt = None
        cases = None
        if (source_dir / "result.json").is_file():
            receipt = read_json(source_dir / "result.json")
        if (source_dir / cases_file).is_file():
            cases = read_json(source_dir / cases_file)
        case_map = {}
        if cases:
            for case in cases.get("cases", []):
                name = case.get("case")
                if isinstance(name, str):
                    case_map[name] = bool(case.get("ok"))
        if source_kind == "dev-fallback":
            for name, ok in merged_cases.items():
                case_map[name] = case_map.get(name, True) and ok
        book = {
            "suite": suite,
            "sourceKind": source_kind,
            "dir": rel(source_dir),
            "exists": source_dir.is_dir(),
            "receipt": receipt,
            "status": (receipt or {}).get("status"),
            "exitCode": (receipt or {}).get("exitCode"),
            "caseCount": (receipt or {}).get("caseCount"),
            "casesSha256": (receipt or {}).get("casesSha256"),
            "caseMap": case_map,
            "need": spec["need"],
            "supplements": supplements if source_kind == "dev-fallback" else [],
        }
        self.books[suite] = book
        producer.check(
            f"evidence::source::{suite}",
            book["exists"] and receipt is not None,
            detail={"sourceKind": source_kind, "dir": book["dir"],
                    "status": book["status"], "need": spec["need"]},
        )
        return book

    def case_ok(self, suite: str, case: str) -> bool:
        return bool(self.books.get(suite, {}).get("caseMap", {}).get(case))

    def require_cases(self, producer: Producer, suite: str, cases: list[str],
                      label_prefix: str) -> dict:
        book = self.books[suite]
        missing = [name for name in cases if name not in book["caseMap"]]
        failed = [name for name in cases if book["caseMap"].get(name) is False]
        producer.check(
            f"evidence::{label_prefix}::{suite}",
            not missing and not failed,
            detail={"cases": len(cases), "missing": missing[:8], "failed": failed[:8]},
        )
        return {"suite": suite, "cases": cases, "missing": missing, "failed": failed}

    # ── 上游 PASS/状态判定（不允许把失败上游当通过）────────────────────
    def is_pass(self, suite: str) -> bool:
        book = self.books[suite]
        receipt = book.get("receipt") or {}
        return receipt.get("status") == "PASS" and receipt.get("exitCode") == 0

    def require_upstream_pass(self, producer: Producer, suites: list[str], label: str) -> bool:
        bad = [suite for suite in suites if not self.is_pass(suite)]
        producer.check(
            f"evidence::upstream-pass::{label}",
            not bad,
            detail={"required": suites, "notPass": [{"suite": s, "status": self.books[s]["status"]} for s in bad]},
        )
        return not bad

    def provenance(self) -> dict:
        return {suite: {"sourceKind": book["sourceKind"], "dir": book["dir"],
                        "status": book["status"], "exitCode": book["exitCode"]}
                for suite, book in self.books.items()}


# ════════════════════════════════════════════════════════════════════════════
# A15：固定因子的只读复算
# ════════════════════════════════════════════════════════════════════════════
def verify_fixed_factors(producer: Producer) -> dict:
    fixed = {"verifiedAtUnixMs": None, "candidate": CANDIDATE_META, "renderer": {}, "delivery": {}, "evidence": {},
             "fixtures": {}, "r08Baseline": {}, "protocol": {}}
    import time
    fixed["verifiedAtUnixMs"] = int(time.time() * 1000)

    # 1) 交付 release .app 树与逐文件摘要
    app = ROOT / FROZEN["deliveryRelease"]["appBundle"]
    entries = {}
    for name in FROZEN["deliveryRelease"]["files"]:
        path = app / name
        producer.check(f"fixed::delivery-file::{name}", path.is_file(), detail={"path": rel(path)})
        if path.is_file():
            entries[name] = sha256_file(path)
            producer.check(
                f"fixed::delivery-sha::{name}",
                entries[name] == FROZEN["deliveryRelease"]["files"][name],
                detail={"expected": FROZEN["deliveryRelease"]["files"][name], "actual": entries[name]},
            )
    app_tree = tree_digest(entries) if entries else ""
    producer.check("fixed::delivery-app-tree-sha256",
                   app_tree == FROZEN["deliveryRelease"]["appTreeSha256"],
                   detail={"expected": FROZEN["deliveryRelease"]["appTreeSha256"], "actual": app_tree})
    fixed["delivery"] = {
        "appBundle": FROZEN["deliveryRelease"]["appBundle"],
        "appTreeSha256": app_tree,
        "hostExeSha256": entries.get("Contents/MacOS/lingxi-resource-desktop"),
        "sidecarSha256": entries.get("Contents/MacOS/lingxi-service"),
        "buildJsonSha256": entries.get("Contents/Resources/rust-service/build.json"),
        "infoPlistSha256": entries.get("Contents/Info.plist"),
        "features": FROZEN["deliveryRelease"]["features"],
        "protocol": FROZEN["deliveryRelease"]["protocol"],
    }

    # 2) 构建回执（release / evidence）
    for key in ("deliveryRelease", "evidenceBuild"):
        spec = FROZEN[key]
        receipt_rel = spec.get("releaseReceipt") or spec["receipt"]
        receipt_path = ROOT / receipt_rel
        exists = receipt_path.is_file()
        producer.check(f"fixed::receipt::{key}", exists, detail={"path": rel(receipt_path)})
        if not exists:
            continue
        actual = sha256_file(receipt_path)
        expected_receipt = spec.get("receiptSha256") or spec.get("releaseReceiptSha256")
        producer.check(f"fixed::receipt-sha::{key}", actual == expected_receipt,
                       detail={"expected": expected_receipt, "actual": actual})
        receipt = read_json(receipt_path)
        inputs = receipt.get("inputs") or {}
        features = receipt.get("features") or inputs.get("features") or []
        producer.check(f"fixed::receipt-features::{key}", list(features) == spec["features"],
                       detail={"expected": spec["features"], "actual": features})
        producer.check(f"fixed::receipt-evidence-build::{key}",
                       bool(receipt.get("evidenceBuild")) == spec["evidenceBuild"],
                       detail={"actual": receipt.get("evidenceBuild")})
        if key == "deliveryRelease":
            producer.check("fixed::release-artifacts-recorded",
                           receipt.get("artifacts", {}).get("executableSha256") == spec["hostExeSha256"]
                           and receipt.get("artifacts", {}).get("appBundleExists") is True,
                           detail=receipt.get("artifacts"))
            producer.check("fixed::release-renderer-bound",
                           inputs.get("rendererDigest") == FROZEN["renderer"]["digest"],
                           detail={"rendererDigest": inputs.get("rendererDigest")})
            producer.check("fixed::release-theme-bound",
                           inputs.get("themeBundleSha256") == FROZEN["renderer"]["themeBundleSha256"],
                           detail={"themeBundleSha256": inputs.get("themeBundleSha256")})
            fixed["delivery"]["releaseReceipt"] = rel(receipt_path)
            fixed["delivery"]["releaseReceiptSha256"] = actual
        else:
            fixed["evidence"] = {
                "executable": spec["executable"],
                "executableSha256": spec["executableSha256"],
                "receipt": rel(receipt_path),
                "receiptSha256": actual,
                "features": list(features),
                "wrapperMainExeSha256": spec["wrapperMainExeSha256"],
            }
            exe = ROOT / spec["executable"]
            producer.check("fixed::evidence-exe-exists", exe.is_file(), detail={"path": spec["executable"]})
            if exe.is_file():
                actual_exe = sha256_file(exe)
                producer.check("fixed::evidence-exe-sha", actual_exe == spec["executableSha256"],
                               detail={"expected": spec["executableSha256"], "actual": actual_exe})
                fixed["evidence"]["executableSha256Actual"] = actual_exe
            producer.check("fixed::evidence-renderer-bound",
                           inputs.get("rendererDigest") == FROZEN["renderer"]["digest"],
                           detail={"rendererDigest": inputs.get("rendererDigest")})
            producer.check("fixed::same-renderer-candidate",
                           inputs.get("rendererDigest") == FROZEN["renderer"]["digest"]
                           and inputs.get("themeBundleSha256") == FROZEN["renderer"]["themeBundleSha256"],
                           detail={"note": "delivery release 与 G1 evidence 构建绑定同一 renderer/theme 摘要"})

    # 3) renderer 摘要（磁盘树 == 回执摘要；build-once-bind-once 口径）
    if RENDERER_DIST.is_dir():
        renderer_entries = sha256_tree(RENDERER_DIST)
        digest = tree_digest(renderer_entries)
        producer.check("fixed::renderer-tree-digest",
                       digest == FROZEN["renderer"]["digest"],
                       detail={"expected": FROZEN["renderer"]["digest"], "actual": digest,
                               "files": len(renderer_entries)})
        expected_file_count = FROZEN["renderer"].get("fileCount")
        if expected_file_count is not None:
            producer.check("fixed::renderer-file-count",
                           len(renderer_entries) == expected_file_count,
                           detail={"expected": expected_file_count, "actual": len(renderer_entries)})
        else:
            producer.check("fixed::renderer-file-count-recorded",
                           len(renderer_entries) > 0,
                           detail={"actual": len(renderer_entries),
                                   "note": "v2 handoff 未提供 fileCount，按磁盘实测记录（build-once-bind-once）"})
        theme = RENDERER_DIST / "lib/theme.js"
        producer.check("fixed::renderer-theme-present", theme.is_file(), detail={"path": rel(theme)})
        if theme.is_file():
            actual_theme = sha256_file(theme)
            producer.check("fixed::renderer-theme-sha",
                           actual_theme == FROZEN["renderer"]["themeBundleSha256"],
                           detail={"expected": FROZEN["renderer"]["themeBundleSha256"], "actual": actual_theme})
        fixed["renderer"] = {"path": FROZEN["renderer"]["path"], "digest": digest,
                             "fileCount": len(renderer_entries),
                             "themeBundleSha256": FROZEN["renderer"]["themeBundleSha256"]}
    else:
        producer.check("fixed::renderer-dist-present", False, detail={"path": rel(RENDERER_DIST)})

    # 4) build.json 协议/摘要（sidecar 签名后 sha 变化，contentSha 复算一致）
    build_json_path = app / "Contents/Resources/rust-service/build.json"
    if build_json_path.is_file():
        build_json = read_json(build_json_path)
        protocol = build_json.get("protocol") or {}
        expected = FROZEN["deliveryRelease"]["protocol"]
        producer.check("fixed::service-protocol",
                       protocol.get("serverKind") == expected["serverKind"]
                       and protocol.get("wireProtocolMin") == expected["wireProtocolMin"]
                       and protocol.get("wireProtocolMax") == expected["wireProtocolMax"]
                       and protocol.get("dataEpoch") == expected["dataEpoch"],
                       detail={"actual": protocol, "expected": expected})
        sidecar = app / "Contents/MacOS/lingxi-service"
        if sidecar.is_file():
            content = sha256_file(sidecar)
            fixed_content = build_json.get("contentSha256") or build_json.get("sha256")
            producer.check("fixed::service-content-digest",
                           content == FROZEN["deliveryRelease"]["serviceContentSha256"]
                           or fixed_content == FROZEN["deliveryRelease"]["serviceContentSha256"],
                           detail={"sha256": content, "contentSha256": build_json.get("contentSha256")})
        fixed["protocol"] = protocol
        fixed["delivery"]["buildJson"] = build_json

    # 5) 夹具锚点（R08 a16-versions + T07 实测 case）
    producer.check("fixed::candidate-handoff-provenance",
                   bool(CANDIDATE_META.get("source")),
                   detail=CANDIDATE_META)
    a16 = read_json(ROOT / FROZEN["r08Baseline"]["a16Versions"])
    anchors = a16.get("fixtureAnchors") or {}
    producer.check("fixed::fixture-v3-anchor", anchors.get("v3Base") == FROZEN["fixtures"]["v3Base"],
                   detail={"actual": anchors.get("v3Base")})
    fixed["fixtures"] = {"v3Base": anchors.get("v3Base"), "v2Bulk": anchors.get("v2Bulk")}
    fixed["r08Baseline"] = {key: a16.get(key) for key in
                            ("rendererDist", "rustBinary", "contractVersions", "gitHead", "node", "packageVersion")}
    producer.check("fixed::r08-renderer-baseline",
                   a16.get("rendererDist") == FROZEN["r08Baseline"]["rendererDist"],
                   detail={"actual": a16.get("rendererDist")})
    producer.check("fixed::r08-rust-binary-baseline",
                   a16.get("rustBinary") == FROZEN["r08Baseline"]["rustBinary"],
                   detail={"actual": a16.get("rustBinary")})

    # 6) R08 正式门回执（Electron 宿主对照基线）
    gate_receipt = read_json(ROOT / FROZEN["r08Baseline"]["gate"])
    producer.check("fixed::r08-gate-overall-pass", gate_receipt.get("overall") == "PASS",
                   detail={"overall": gate_receipt.get("overall")})
    producer.check("fixed::r08-gate-tested-sha",
                   gate_receipt.get("testedSha") == FROZEN["r08Baseline"]["testedSha"],
                   detail={"expected": FROZEN["r08Baseline"]["testedSha"],
                           "actual": gate_receipt.get("testedSha")})
    binding = gate_receipt.get("candidateSourceBinding") or {}
    producer.check("fixed::r08-candidate-binding-stable",
                   (binding.get("before") or {}).get("digestSha256") == (binding.get("after") or {}).get("digestSha256")
                   and bool((binding.get("before") or {}).get("digestSha256")),
                   detail={"before": (binding.get("before") or {}).get("digestSha256"),
                           "after": (binding.get("after") or {}).get("digestSha256")})
    fixed["r08Baseline"]["gateOverall"] = gate_receipt.get("overall")
    fixed["r08Baseline"]["testedSha"] = gate_receipt.get("testedSha")
    fixed["r08Baseline"]["candidateSourceDigest"] = (binding.get("before") or {}).get("digestSha256")

    # 7) R08 A/B（Electron host + Rust service）任务级结果
    ab = read_json(ROOT / FROZEN["r08Baseline"]["abParityCases"])
    ab_cases = ab.get("cases") or []
    producer.check("fixed::r08-ab-parity-cases-nonempty",
                   len(ab_cases) >= 60 and all(case.get("ok") for case in ab_cases),
                   detail={"count": len(ab_cases),
                           "failed": [c.get("case") for c in ab_cases if not c.get("ok")][:8]})
    ab_log_path = ROOT / FROZEN["r08Baseline"]["abOutputLog"]
    ab_line = ""
    if ab_log_path.is_file():
        for line in read_text(ab_log_path).splitlines():
            if "AB_RESULT" in line:
                ab_line = line.strip()
    expected_ab = FROZEN["r08Baseline"]["abExpected"]
    parsed_ab = {key: int(re.search(key + r"=(\d+)", ab_line).group(1))
                 for key in ("tasks", "parity", "recorded", "differs")
                 if re.search(key + r"=(\d+)", ab_line)} if ab_line else {}
    producer.check("fixed::r08-ab-verdict",
                   parsed_ab.get("tasks") == expected_ab["tasks"]
                   and parsed_ab.get("parity") == expected_ab["parity"]
                   and parsed_ab.get("recorded") == expected_ab["recorded"]
                   and parsed_ab.get("differs") == expected_ab["differs"],
                   detail={"expected": expected_ab, "actual": parsed_ab, "line": ab_line[:200]})
    fixed["r08Baseline"]["ab"] = parsed_ab
    fixed["r08Baseline"]["abRecordedApprovals"] = list((ab.get("recordedApprovals") or {}).keys())
    return fixed


# ════════════════════════════════════════════════════════════════════════════
# 共享数据装载（A15/A16 共用；全部来自既有机器可读账与真实源码字节）
# ════════════════════════════════════════════════════════════════════════════
def load_sources() -> dict:
    sources = {
        "inventory": read_json(DOCS09 / "T03_HOST_CAPABILITY_INVENTORY.json"),
        "inventoryMd": read_text(DOCS09 / "T03_HOST_CAPABILITY_INVENTORY.md"),
        "t03Plan": read_json(DOCS09 / "T03_LEAF_CASE_PLAN.json"),
        "t07Mapping": read_json(DOCS09 / "T07_LEAF_MAPPING.json"),
        "ledger": read_json(DOCS09 / "R09_LEAF_LEDGER.json"),
        "policy": read_json(DOCS09 / "R09_COMMAND_POLICY.json"),
        "handoff": read_json(DOCS09 / "R09_HANDOFF.json"),
        "guiBatch": read_json(DOCS09 / "T06_GUI_BATCH_NEEDS.json"),
        "blockedMd": read_text(DOCS09 / "BLOCKED.md"),
        "t06ProgressMd": read_text(DOCS09 / "T06_PROGRESS.md"),
        "t03t04ProgressMd": read_text(DOCS09 / "T03_T04_PROGRESS.md"),
        "t01t02ProgressMd": read_text(DOCS09 / "T01_T02_PROGRESS.md"),
        "t05ProgressMd": read_text(DOCS09 / "T05_PROGRESS.md"),
        "t07ProgressMd": read_text(DOCS09 / "T07_PROGRESS.md"),
        "implementationPlanMd": read_text(DOCS09 / "IMPLEMENTATION_PLAN.md"),
        "progressMd": read_text(DOCS09 / "PROGRESS.md"),
        "reportMd": read_text(DOCS09 / "R09_REPORT.md"),
        "entrypoints": read_json(ROOT / "docs/rust-tauri/R00/ENTRYPOINTS.json"),
        "hostTs": read_text(ROOT / "desktop/src/react/host/tauri-desktop-host.ts"),
        "capFragmentTs": read_text(ROOT / "desktop/src/react/host/tauri-host-capabilities.ts"),
        "platformTypesTs": read_text(ROOT / "desktop/src/react/types.ts"),
        "tauriBaseConf": read_text(ROOT / "desktop/src-tauri/tauri.conf.json"),
        "tauriReleaseConf": read_text(ROOT / "desktop/src-tauri/tauri.release.conf.json"),
    }
    return sources


def scene_by_id(sources: dict) -> dict:
    inventory = sources["inventory"]
    groups = inventory["coverageSummary"]["groups"]
    cap_group = {}
    for group, spec in groups.items():
        for cap in spec.get("capabilities", []):
            cap_group[cap] = group
    return {
        "capabilities": {cap["id"]: cap for cap in inventory["capabilities"]},
        "capGroup": cap_group,
        "hostInternal": set(inventory["coverageSummary"]["hostInternalCapabilities"]),
        "noConsumerMembers": set(inventory["coverageSummary"].get("apiMembersWithoutProductionConsumer") or []),
        "tasks": {leaf["id"]: leaf for leaf in sources["ledger"]["leaves"]},
        "t03Leaves": {leaf["leafId"]: leaf for leaf in sources["t03Plan"]["leaves"]},
        "t07Leaves": {leaf["leafId"]: leaf for leaf in sources["t07Mapping"]["leaves"]},
        "policyWindows": sources["policy"]["windows"],
        "deferredCommands": deferred_groups(sources),
        "droppedCommands": sources["policy"]["droppedCommands"],
        "releaseDenied": sources["policy"]["releaseDeniedCommands"],
        "devOnly": sources["policy"]["devOnlyCommands"],
    }


# 能力 → 原生实现模块（相对 desktop/src-tauri/src/）；空列表 = 无原生模块（政策/延后/缺口）
NATIVE_MODULES = {
    "host:single-instance": ["native/single_instance.rs"],
    "host:main-window": ["native/window_lifecycle.rs", "native/window_labels.rs", "host.rs"],
    "host:window-state": ["native/window_state.rs"],
    "host:window-controls": ["native/window_controls.rs"],
    "host:app-lifecycle-quit": ["native/window_lifecycle.rs"],
    "host:app-ready": ["native/app_ready.rs", "host.rs"],
    "host:theme-sync": ["native/theme_sync.rs"],
    "host:i18n-main": ["native/i18n.rs"],
    "host:reload-restart": ["native/reload_restart.rs"],
    "host:tray": ["native/tray.rs"],
    "host:global-shortcut": ["native/global_shortcut.rs"],
    "host:notification": ["native/notification.rs", "native/notification_center_macos.rs"],
    "host:keep-awake": ["native/keep_awake.rs"],
    "host:auto-launch": ["native/auto_launch.rs"],
    "host:file-dialogs": ["native/file_dialogs.rs"],
    "host:clipboard-edit-command": ["native/clipboard_edit.rs"],
    "host:drag-out": ["native/drag_out.rs"],
    "host:file-drop-paths": ["native/file_drop.rs"],
    "host:external-open": ["native/external_open.rs"],
    "host:trash": ["native/trash.rs"],
    "host:settings-window": ["native/settings_window.rs"],
    "host:settings-broadcast": ["native/settings_broadcast.rs"],
    "host:quick-chat-window": ["native/quick_chat.rs"],
    "host:quick-chat-session-handoff": ["native/quick_chat_handoff.rs"],
    "host:app-info-platform": ["native/app_info.rs"],
    "host:splash": ["native/splash.rs", "host.rs"],
    "host:onboarding": ["native/onboarding.rs"],
    "host:speech-permission": ["native/speech.rs", "native/permissions.rs"],
    "host:file-text-io": ["files.rs"],
    "host:file-watch": ["files.rs"],
    "host:file-url-asset": [],
    "host:detached-viewer": ["files.rs", "resource_bridge.rs"],
    "host:skill-viewer": ["skills.rs"],
    "host:browser-viewer": ["browser_viewer.rs"],
    "host:screenshot-render": ["native/screenshot.rs"],
    "host:server-connection-info": ["commands.rs", "service/supervisor.rs"],
    "host:card-frame-navigation-guard": [],
    "host:context-menu-policy": [],
    "host:docx-xlsx-preview": [],
    "host:observability-export": [],
    "host:update-github-check": [],
    "host:update-auto": [],
    "host:update-train-ota": [],
    "security:capability-sets": [],
    "security:csp": [],
    "security:command-acl": [],
    "security:release-no-test-control": [],
    "security:remote-content-isolation": [],
    "security:no-arbitrary-shell": [],
    "security:token-exposure": [],
}

# 命令 → 能力（71 条注册命令面；A15 命令行 + A16 命令行共用）
COMMAND_CAPABILITY = {
    "open_skill_viewer": "host:skill-viewer",
    "skill_viewer_load": "host:skill-viewer",
    "list_skill_files": "host:skill-viewer",
    "read_skill_file": "host:skill-viewer",
    "skill_viewer_close": "host:skill-viewer",
    "select_files": "host:file-dialogs",
    "select_folder": "host:file-dialogs",
    "select_skill": "host:file-dialogs",
    "read_file": "host:file-text-io",
    "read_file_snapshot": "host:file-text-io",
    "read_file_base64": "host:file-text-io",
    "file_info": "host:file-text-io",
    "write_file": "host:file-text-io",
    "write_file_binary": "host:file-text-io",
    "copy_file": "host:file-text-io",
    "write_file_if_unchanged": "host:file-text-io",
    "open_file": "host:external-open",
    "open_folder": "host:external-open",
    "show_in_finder": "host:external-open",
    "open_external": "host:external-open",
    "trash_item": "host:trash",
    "run_edit_command": "host:clipboard-edit-command",
    "watch_file": "host:file-watch",
    "unwatch_file": "host:file-watch",
    "watch_workspace": "host:file-watch",
    "unwatch_workspace": "host:file-watch",
    "resource_subscribe": "host:detached-viewer",
    "resource_events": "host:detached-viewer",
    "resource_unsubscribe": "host:detached-viewer",
    "spawn_viewer": "host:detached-viewer",
    "viewer_request_load": "host:detached-viewer",
    "viewer_close": "host:detached-viewer",
    "get_server_connection_info": "host:server-connection-info",
    "get_server_port": "host:server-connection-info",
    "get_server_token": "host:server-connection-info",
    "get_service_status": "host:server-connection-info",
    "get_platform": "host:app-info-platform",
    "get_app_version": "host:app-info-platform",
    "app_ready": "host:app-ready",
    "window_minimize": "host:window-controls",
    "window_maximize": "host:window-controls",
    "window_close": "host:window-controls",
    "window_is_maximized": "host:window-controls",
    "window_theme_changed": "host:theme-sync",
    "reload_main_window": "host:reload-restart",
    "restart_app": "host:reload-restart",
    "open_settings": "host:settings-window",
    "settings_changed": "host:settings-broadcast",
    "show_notification": "host:notification",
    "start_drag": "host:drag-out",
    "get_keep_awake_status": "host:keep-awake",
    "set_keep_awake_enabled": "host:keep-awake",
    "get_auto_launch_status": "host:auto-launch",
    "set_auto_launch_enabled": "host:auto-launch",
    "keybindings_reload_global": "host:global-shortcut",
    "quick_chat_reload_shortcut": "host:global-shortcut",
    "keybindings_test_register": "host:global-shortcut",
    "quick_chat_shortcut_status": "host:global-shortcut",
    "quick_chat_show": "host:quick-chat-window",
    "quick_chat_hide": "host:quick-chat-window",
    "quick_chat_resize": "host:quick-chat-window",
    "quick_chat_open_session": "host:quick-chat-session-handoff",
    "quick_chat_open_session_ack": "host:quick-chat-session-handoff",
    "get_splash_info": "host:splash",
    "get_avatar_path": "host:splash",
    "onboarding_complete": "host:onboarding",
    "speech_permission_status": "host:speech-permission",
    "speech_permission_request": "host:speech-permission",
    "set_browser_connection": "host:browser-viewer",
    "get_browser_viewer_context": "host:browser-viewer",
    "open_browser_viewer": "host:browser-viewer",
    "rust_browser_request": "host:browser-viewer",
    "close_browser_viewer": "host:browser-viewer",
}

# 能力 → 证据（suite + case 前缀/显式 case；文档作为辅助）
CAP_EVIDENCE = {
    "host:single-instance": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::single-instance."]},
    "host:main-window": {"suites": ["r09_t03_native_matrix", "r09_t01_react_parity"],
                         "prefixes": ["leaf::window-lifecycle."],
                         "cases": ["a01::dom-snapshot-parity", "a01::tauri-loads-embedded-page"]},
    "host:window-state": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::window-state."]},
    "host:window-controls": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::window-controls."]},
    "host:app-lifecycle-quit": {"suites": ["r09_t03_native_matrix", "r09_t02_process_ownership",
                                            "r09_t07_native_e2e"],
                                 "prefixes": ["leaf::window-lifecycle."],
                                 "cases": ["t02::no-orphan-owned-services", "t02::exit-owned-tree-only",
                                           "t07::native-e2e.graceful-exit"]},
    "host:app-ready": {"suites": ["r09_t03_native_matrix", "r09_t01_react_parity"],
                       "prefixes": ["leaf::app-ready."],
                       "cases": ["a01::tauri-boots", "a01::tauri-graceful-exit"]},
    "host:theme-sync": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::theme-sync."]},
    "host:i18n-main": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::i18n."]},
    "host:reload-restart": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::reload-restart."]},
    "host:tray": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::tray."]},
    "host:global-shortcut": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::global-shortcut."]},
    "host:notification": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::notification."]},
    "host:keep-awake": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::keep-awake."]},
    "host:auto-launch": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::auto-launch."]},
    "host:file-dialogs": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::file-dialogs."]},
    "host:clipboard-edit-command": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::clipboard-edit."]},
    "host:drag-out": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::drag-out."]},
    "host:file-drop-paths": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::file-drop."]},
    "host:external-open": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::external-open."]},
    "host:trash": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::trash."]},
    "host:settings-window": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::settings-window."]},
    "host:settings-broadcast": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::settings-broadcast."]},
    "host:quick-chat-window": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::quick-chat."]},
    "host:quick-chat-session-handoff": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::quick-chat-handoff."]},
    "host:app-info-platform": {"suites": ["r09_t03_native_matrix", "r09_t01_react_parity"],
                               "prefixes": ["leaf::app-info."],
                               "cases": ["a01::dom-snapshot-parity"]},
    "host:splash": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::splash."]},
    "host:onboarding": {"suites": ["r09_t03_native_matrix"], "prefixes": ["leaf::onboarding."]},
    "host:speech-permission": {"suites": ["r09_t04_permission_matrix"], "prefixes": ["leaf::speech."]},
    "host:file-text-io": {"suites": ["audit_capability"],
                          "cases": ["caps::invoke-handler-equals-build-manifest", "caps::each-command-has-permission"]},
    "host:file-watch": {"suites": ["audit_capability"],
                        "cases": ["caps::invoke-handler-equals-build-manifest", "caps::deferred-commands-not-granted"]},
    "host:detached-viewer": {"suites": ["audit_capability"],
                             "cases": ["caps::invoke-handler-equals-build-manifest", "caps::each-command-has-permission"]},
    "host:skill-viewer": {"suites": ["audit_capability"],
                          "cases": ["caps::invoke-handler-equals-build-manifest", "caps::each-command-has-permission"]},
    "host:browser-viewer": {"suites": ["r09_t05_browser_pdf"], "prefixes": ["leaf:R00-T02-LA-"],
                            "note": "T05 服务/Chrome 侧 18/19 叶；Tauri browser-viewer 窗口与 3 叶 blocked"},
    "host:screenshot-render": {"suites": ["audit_capability"], "cases": ["caps::deferred-commands-not-granted"]},
    "host:server-connection-info": {"suites": ["r09_t02_supervisor_ready", "r09_t02_process_ownership",
                                               "r09_t01_react_parity"],
                                    "cases": ["t02::ready-handshake-real", "t02::manifest-protocol-match",
                                              "a01::service-identity-parity",
                                              "t02::no-orphan-owned-services"]},
    "host:card-frame-navigation-guard": {"suites": [], "cases": []},
    "host:docx-xlsx-preview": {"suites": [], "cases": []},
    "host:observability-export": {"suites": [], "cases": []},
    "host:update-github-check": {"suites": [], "cases": []},
    "host:update-auto": {"suites": [], "cases": []},
    "host:update-train-ota": {"suites": [], "cases": []},
    "security:capability-sets": {"suites": ["audit_capability"],
                                 "cases": ["caps::window-union-equals-policy::main",
                                           "caps::no-dangerous-plugin-permissions",
                                           "caps::window-stacking::main"]},
    "security:csp": {"suites": ["release_scan"],
                     "cases": ["release::csp-directives-hardened", "release::csp-no-forbidden-tokens"]},
    "security:command-acl": {"suites": ["audit_capability"],
                             "cases": ["caps::invoke-handler-equals-build-manifest",
                                       "caps::each-command-has-permission",
                                       "caps::each-permission-command-registered",
                                       "caps::acl-manifest-covers-registered"]},
    "security:release-no-test-control": {"suites": ["audit_capability", "release_scan"],
                                         "cases": ["caps::test-hooks-feature-declared",
                                                   "caps::delivery-release-excludes-test-hooks",
                                                   "caps::delivery-release-binary-no-test-env",
                                                   "release::no-test-control-env-in-artifacts"]},
    "security:remote-content-isolation": {"suites": ["release_scan"],
                                          "cases": ["release::no-remote-domain-ipc-access",
                                                    "release::capabilities-no-generic-shell-http-fs"]},
    "security:no-arbitrary-shell": {"suites": ["release_scan"],
                                    "cases": ["release::no-shell-plugin-dependency",
                                              "release::process-spawn-sites-match-ledger",
                                              "release::no-renderer-controllable-shell-binary"]},
    "security:token-exposure": {"suites": ["audit_capability"],
                                "cases": ["caps::protected-commands-window-scoped"]},
    "host:file-url-asset": {"suites": ["r09_t07_native_e2e"],
                             "cases": ["t07::native-e2e.file-url-asset"],
                             "prefixes": []},
    "host:context-menu-policy": {"suites": ["r09_t07_native_e2e"],
                                  "cases": ["t07::native-e2e.context-menu-policy"],
                                  "prefixes": []},
}

# 已登记 deferred（cap -> (登记组, 登记文件, 引文)）；A15 记为 recorded，A16 记为 deferred_registered
DEFERRED_CAPS = {
    "host:update-github-check": ("R10-update-chain", "docs/rust-tauri/R09/T06_PROGRESS.md",
                                 "更新链（GitHub check/自动更新/OTA）属 R10 验收范围"),
    "host:update-auto": ("R10-update-chain", "docs/rust-tauri/R09/T06_PROGRESS.md",
                         "更新链（GitHub check/自动更新/OTA）属 R10 验收范围"),
    "host:update-train-ota": ("R10-update-chain", "docs/rust-tauri/R09/T06_PROGRESS.md",
                              "更新链（GitHub check/自动更新/OTA）属 R10 验收范围"),
    "host:screenshot-render": ("T04-screenshot", "docs/rust-tauri/R09/T06_PROGRESS.md",
                               "Tauri/Wry 无稳定页面捕获 API（T04 OQ-04 BLOCKED）"),
    "host:docx-xlsx-preview": ("T03-FILE-01", "docs/rust-tauri/R09/T06_PROGRESS.md",
                               "T03 下一轮文件能力（docx/xlsx 预览、workspace watch、可观测导出）尚未实现"),
    "host:observability-export": ("T03-FILE-01", "docs/rust-tauri/R09/T06_PROGRESS.md",
                                  "T03 下一轮文件能力（docx/xlsx 预览、workspace watch、可观测导出）尚未实现"),
}

# 已登记 blocked（cap -> {blockerId, basis paths, quote, kind}）
BLOCKED_CAPS = {
    "host:notification": {
        "blockerId": "T03-NOTIFY-02", "kind": "system-gui",
        "basis": [("docs/rust-tauri/R09/BLOCKED.md", "通知点击聚焦")],
    },
    "host:auto-launch": {
        "blockerId": "BLOCKED-SIGNING-IDENTITY", "kind": "signing",
        "basis": [("docs/rust-tauri/R09/BLOCKED.md", "无 Apple Developer 正式签名/公证身份")],
    },
    "host:speech-permission": {
        "blockerId": "BLOCKED-SYSTEM-PERMISSIONS", "kind": "tcc",
        "basis": [("docs/rust-tauri/R09/BLOCKED.md", "macOS 系统级权限")],
    },
    "host:onboarding": {
        "blockerId": "T06-ONBOARD-01", "kind": "service-gap",
        "basis": [("docs/rust-tauri/R09/BLOCKED.md", "onboarding_complete` 返回 NOT_IMPLEMENTED")],
    },
}

# A15/A16：判定为「已登记设计边界（显式降级，不得假安全）」的能力
DESIGN_BOUNDARY_CAPS = {
    "host:card-frame-navigation-guard": ("R09-ADJ-HOST-05", "docs/rust-tauri/R09/IMPLEMENTATION_PLAN.md",
                                         "导航 guard 保持 flag=false 并在卡片层显式降级"),
}

# A15：命中这些能力的「有意外的宿主差异」批准 id（不登记 = 未批准差异）
CAP_APPROVAL = {
    "host:main-window": "R09-ADJ-HOST-01",
    "host:app-ready": "R09-ADJ-HOST-01",
    "host:tray": "R09-ADJ-HOST-02",
    "host:update-github-check": "R09-ADJ-HOST-03",
    "host:update-auto": "R09-ADJ-HOST-03",
    "host:update-train-ota": "R09-ADJ-HOST-03",
    "host:screenshot-render": "R09-ADJ-HOST-04",
    "host:docx-xlsx-preview": "R09-ADJ-HOST-08",
    "host:observability-export": "R09-ADJ-HOST-08",
    "host:card-frame-navigation-guard": "R09-ADJ-HOST-05",
    "host:file-drop-paths": "R09-ADJ-HOST-06",
    "host:onboarding": "R09-ADJ-HOST-07",
}

# T03/T07 共享 46 叶中「宿主应实现」的 30 叶（其余 16 叶 = SERVER_REACT，非宿主面）
HOST_LEAF_CLASSES = {"HOST_NATIVE", "HOST_BRIDGE"}


def load_capability_model(producer: Producer, sources: dict) -> dict:
    """构建 50 能力模型：实现入口/权限/消费者/证据/阻塞/平台 + 静态校验。"""
    scene = scene_by_id(sources)
    inventory = sources["inventory"]
    caps = scene["capabilities"]
    model = {}
    producer.check("inventory::capability-count-50", len(caps) == 50, detail={"count": len(caps)})
    union_groups = set()
    for group in scene["capGroup"].values():
        union_groups.add(group)
    producer.check("inventory::group-union-covers-all",
                   len(scene["capGroup"]) == 50,
                   detail={"mapped": len(scene["capGroup"]), "groups": sorted(union_groups)})

    for cap_id, cap in caps.items():
        modules = NATIVE_MODULES.get(cap_id, [])
        module_paths = [TAURI_SRC / module for module in modules]
        modules_ok = all(path.is_file() for path in module_paths)
        commands = [cmd for cmd, owner in COMMAND_CAPABILITY.items() if owner == cap_id]
        record = {
            "id": cap_id,
            "group": scene["capGroup"].get(cap_id),
            "electronImpl": cap.get("electronImpl"),
            "tauriTarget": cap.get("tauriTarget"),
            "realConsumers": cap.get("realConsumers") or [],
            "permissionScope": cap.get("permissionScope"),
            "platformNotes": cap.get("platformNotes"),
            "apiMembers": cap.get("apiMembers") or [],
            "r09Leaves": cap.get("r09Leaves") or [],
            "modules": modules,
            "modulesPresent": modules_ok,
            "commands": sorted(commands),
            "hostInternal": cap_id in scene["hostInternal"],
            "kind": "native-module",
            "evidence": [],
            "status": "pending",
            "runtimeStatus": None,
            "blockedRegistration": None,
            "deferredRegistration": None,
            "approvalId": None,
            "notes": [],
        }
        # 证据：静态支持 case + 文档
        evidence_spec = CAP_EVIDENCE.get(cap_id, {"suites": [], "cases": []})
        record["evidenceSuites"] = evidence_spec.get("suites", [])
        record["evidencePrefixes"] = evidence_spec.get("prefixes", [])
        record["evidenceCases"] = evidence_spec.get("cases", [])
        if cap_id in DEFERRED_CAPS:
            group_id, basis_path, quote = DEFERRED_CAPS[cap_id]
            record["kind"] = "deferred"
            record["deferredRegistration"] = {"group": group_id, "basisPath": basis_path, "basisQuote": quote}
            record["status"] = "deferred_registered"
        elif cap_id in BLOCKED_CAPS:
            blocked = BLOCKED_CAPS[cap_id]
            record["blockedRegistration"] = blocked
            record["kind"] = "blocked"
            record["status"] = "blocked_registered"
        elif cap_id in DESIGN_BOUNDARY_CAPS:
            approval_id, basis_path, quote = DESIGN_BOUNDARY_CAPS[cap_id]
            record["approvalId"] = approval_id
            record["kind"] = "design-boundary"
            record["status"] = "recorded_design_boundary"
        elif cap_id.startswith("security:"):
            record["kind"] = "security-policy"
            record["status"] = "security_static_verified"
            record["runtimeStatus"] = "blocked_registered_by_T06"
        elif cap_id == "host:file-url-asset":
            # v3.1+：配置+renderer 接线已存在（convertFileSrc/assetProtocol scope/CSP），运行期证据来自 T07
            record["kind"] = "renderer+config"
            record["status"] = "pending"
            record["notes"].append("assetProtocol scope（home 三档+$TEMP）+ CSP asset + convertFileSrc；运行期证据=t07::native-e2e.file-url-asset")
        elif cap_id == "host:context-menu-policy":
            # v3.2 起：renderer 等价实现（非可编辑区 preventDefault；编辑区保留
            # React InputContextMenu），T07 G1 采用真实 DOM contextmenu 断言。
            record["kind"] = "renderer-policy"
            record["status"] = "pending"
            record["notes"].append("renderer 等价实现已接线；运行期证据=t07::native-e2e.context-menu-policy（门内 T07 PASS 含该 case）")
        elif cap_id in ("host:file-text-io", "host:detached-viewer", "host:skill-viewer"):
            record["kind"] = "r06-command"
            record["status"] = "pending"
        elif cap_id == "host:file-watch":
            record["kind"] = "r06-command+deferred"
            record["status"] = "pending"
            record["notes"].append("watch_file/unwatch_file 已注册；watchWorkspace/unwatchWorkspace 属 T03-FILE-01 deferred")
        else:
            record["kind"] = "native-module"
            record["status"] = "pending"
        model[cap_id] = record
    return model


def deferred_groups(sources: dict) -> dict[str, dict]:
    """v2 policy: deferredCommands = {group: {commands, reason, owner}}；兼容 list 形态。"""
    raw = sources["policy"].get("deferredCommands") or {}
    groups = {}
    for name, value in raw.items():
        if isinstance(value, dict):
            groups[name] = {"commands": list(value.get("commands") or []),
                            "reason": value.get("reason"), "owner": value.get("owner")}
        elif isinstance(value, list):
            groups[name] = {"commands": list(value), "reason": None, "owner": None}
    return groups


def deferred_command_names(sources: dict) -> list[str]:
    return sorted(cmd for group in deferred_groups(sources).values() for cmd in group["commands"])


def policy_command_sets(sources: dict) -> dict:
    policy = sources["policy"]
    windows = {}
    for label, spec in (policy.get("windows") or {}).items():
        windows[label] = sorted(spec.get("allowedCommands") or [])
    surface = sorted({cmd for commands in windows.values() for cmd in commands})
    return {"windows": windows, "surface": surface}


def parse_tauri_host(sources: dict) -> dict:
    """解析 Tauri DesktopHost/能力片段的真实接线面（静态 + 文档登记）。"""
    host = sources["hostTs"]
    fragment = sources["capFragmentTs"]
    types = sources["platformTypesTs"]
    implemented = parse_block_keys(host, "const implemented: Partial<PlatformApi> = {")
    noops = parse_block_keys(host, "const noops: Partial<PlatformApi> = {")
    noop_reasons = parse_block_keys(host, "const noopReasons: Record<string, string> = {")
    absent = parse_set_literals(host, "const absentMembers = new Set([")
    fragment_members = parse_block_keys(fragment, "const api: Partial<PlatformApi> = {")
    fragment_reasons = parse_block_keys(fragment, "const TAURI_CAPABILITY_REASONS: Readonly<Record<string, string>> = {")
    fragment_merged = ("from './tauri-host-capabilities'" in host
                       or "tauri-host-capabilities" in host.split("export interface TauriGlobal")[0])
    # 条件注入成员：`...(deps.getFilePath ? { getFilePath: ... } : {})` 等静态键解析会漏，单列记录
    conditional_members = set()
    for conditional_match in re.finditer(r"\.\.\.\(\s*deps\.([A-Za-z_][A-Za-z0-9_]*)\s*\?", fragment):
        conditional_members.add(conditional_match.group(1))
    real_members = sorted(set(implemented) | (set(fragment_members) if fragment_merged else set())
                          | (conditional_members if fragment_merged else set()))
    effective_wired = sorted(set(real_members) | set(noops))
    fragment_reason_map = parse_block_map(fragment, "const TAURI_CAPABILITY_REASONS: Readonly<Record<string, string>> = {")
    noop_reason_map = parse_block_map(host, "const noopReasons: Record<string, string> = {")
    fragment_reasons = sorted(fragment_reason_map)
    platform_members = set()
    start = types.find("export interface PlatformApi {")
    if start >= 0:
        segment = types[start:]
        end = segment.find("\n}")
        segment = segment[: end if end > 0 else 40000]
        for match in re.finditer(r"^\s{2}([A-Za-z_][A-Za-z0-9_]*)\??\s*[:(]", segment, re.M):
            platform_members.add(match.group(1))
    # 渲染层真实消费（排除宿主/类型/测试）
    used = {}
    for path in (ROOT / "desktop/src").rglob("*"):
        if not path.is_file() or path.suffix not in (".ts", ".tsx"):
            continue
        name = str(path)
        if "/react/host/" in name or "/__tests__/" in name or name.endswith("types.ts"):
            continue
        try:
            text = read_text(path)
        except OSError:
            continue
        for member in platform_members:
            if re.search(r"[.?]\s*" + re.escape(member) + r"\s*\(", text):
                used.setdefault(member, []).append(rel(path))
    return {
        "implemented": sorted(implemented),
        "realMembers": real_members,
        "conditionalMembers": sorted(conditional_members),
        "fragmentReasonMap": fragment_reason_map,
        "noopReasonMap": noop_reason_map,
        "getFileUrlConvert": "convertFileSrc" in host and "getFileUrl" in host,
        "contextMenuPolicyInstalled": "installTauriContextMenuPolicy" in host,
        "noops": sorted(noops),
        "noopReasons": sorted(noop_reasons),
        "absentMembers": sorted(absent),
        "fragmentMembers": sorted(fragment_members),
        "fragmentReasons": fragment_reasons,
        "fragmentMerged": fragment_merged,
        "platformMembers": sorted(platform_members),
        "rendererUsed": {member: sorted(set(paths)) for member, paths in used.items()},
        "wired": effective_wired,
    }


def member_status(member: str, host: dict) -> str:
    if member in host["realMembers"]:
        return "wired"
    if member in host["noops"]:
        return "explicit-noop"
    if member in host["absentMembers"]:
        return "absent-with-reason"
    if member in host["fragmentReasons"] or member in host["fragmentReasonMap"]:
        return "restricted-with-reason"
    return "unwired"


# ════════════════════════════════════════════════════════════════════════════
# R09-A15｜r09_t08_shell_parity
# ════════════════════════════════════════════════════════════════════════════
A15_LEAF_TASK = "R09-T08"


def _check_approval_bases(producer: Producer) -> dict:
    approvals = {}
    for approval in RECORDED_APPROVALS:
        path = ROOT / approval["basisPath"]
        exists = path.is_file()
        quote_ok = False
        if exists:
            quote_ok = approval["basisQuote"] in read_text(path)
        producer.check(
            f"approval::basis::{approval['id']}",
            exists and quote_ok,
            detail={"path": approval["basisPath"], "quote": approval["basisQuote"], "exists": exists,
                    "quoteFound": quote_ok},
        )
        approvals[approval["id"]] = approval
    producer.check("approval::unique-ids",
                   len({a["id"] for a in RECORDED_APPROVALS}) == len(RECORDED_APPROVALS),
                   detail={"count": len(RECORDED_APPROVALS)})
    return approvals


def _measured_a15_rows(producer: Producer, book: EvidenceBook, sources: dict) -> list[dict]:
    """T01/T02/T05/T07 已实测项 → A/B parity 行（每行绑定真实 evidence case）。"""
    rows = []
    t01 = ["r09_t01_react_parity"]
    t02 = ["r09_t02_supervisor_ready", "r09_t02_process_ownership"]

    t01_cases = {
        "renderer-identity": ["a01::renderer-dist-digest-recorded", "a01::tauri-build-uses-same-renderer",
                              "a01::no-second-react-entry"],
        "boot-dom-projection": ["a01::dom-snapshot-parity", "a01::electron-boots", "a01::tauri-boots"],
        "page-embedding": ["a01::tauri-loads-embedded-page", "a01::renderer-has-no-resource-ui"],
        "service-identity": ["a01::service-identity-parity", "a01::electron-service-identity"],
        "exit-and-isolation": ["a01::electron-exit", "a01::tauri-graceful-exit", "a01::isolated-homes"],
    }
    for suite in t01:
        book.load(producer, suite)
    for row_id, cases in t01_cases.items():
        book.require_cases(producer, "r09_t01_react_parity", cases, f"a15-{row_id}")
        all_ok = all(book.case_ok("r09_t01_react_parity", case) for case in cases)
        rows.append({
            "id": f"measured::t01::{row_id}",
            "surface": "same-window A/B (Electron vs Tauri, 同一次构建)",
            "aSide": {"kind": "t01-same-window-electron", "refs": ["artifacts/rust-tauri/R09/dev/t01/run-08"]},
            "bSide": {"kind": "t01-same-window-tauri", "suite": "r09_t01_react_parity", "cases": cases},
            "status": "parity" if all_ok else "differs",
            "basis": None,
        })

    t02_cases = {
        "ready-handshake": ["t02::ready-handshake-real", "t02::no-fixed-sleep-handshake"],
        "identity-health": ["t02::manifest-protocol-match", "t02::debug-exit-graceful",
                            "t02::packaged-exit-graceful"],
        "protocol-digest-identity-gate": ["t02::manifest-protocol-mismatch-rejected",
                                          "t02::manifest-digest-mismatch-rejected",
                                          "t02::runtime-identity-mismatch-rejected",
                                          "t02::injected-protocol-no-service-spawn",
                                          "t02::injected-digest-no-service-spawn"],
        "exit-ownership": ["t02::no-orphan-owned-services", "t02::exit-owned-tree-only",
                           "t02::reused-is-detached", "t02::remote-mode-zero-touch",
                           "t02::packaged-refuses-remote", "t02::pid-reuse-safe"],
        "error-surface": ["t02::error-surfaced-records",
                          "t02::no-node-fallback-in-release-binary"],
    }
    for suite in t02:
        book.load(producer, suite)
    for row_id, cases in t02_cases.items():
        # 逐 case 分派到实际所属 suite（ownership 套件与 supervisor 套件都可能在运行期变化）
        per_case = {}
        for case in cases:
            for suite in t02:
                if case in book.books[suite]["caseMap"]:
                    per_case[case] = suite
                    break
        missing = [case for case in cases if case not in per_case]
        failed = [case for case, suite in per_case.items() if not book.case_ok(suite, case)]
        producer.check(f"evidence::a15-{row_id}", not missing and not failed,
                       detail={"cases": cases, "missing": missing, "failed": failed})
        rows.append({
            "id": f"measured::t02::{row_id}",
            "surface": "Rust service 监督/握手/退出语义（Tauri 宿主实测；Electron 基线=R08 已验证链）",
            "aSide": {"kind": "r08-electron-baseline-receipt",
                      "refs": [FROZEN["r08Baseline"]["gate"], "docs/rust-tauri/R08/R08_HANDOFF.json"]},
            "bSide": {"kind": "r09-tauri-supervisor", "suites": t02, "cases": cases},
            "status": "parity" if not missing and not failed else "differs",
            "basis": None,
        })

    # T05 旧链（Electron 时代冻结基线）vs Rust 链（服务侧真实对照）
    book.load(producer, "r09_t05_browser_pdf")
    t05_dir = Path(book.books["r09_t05_browser_pdf"]["dir"])
    compare_path = (t05_dir if t05_dir.is_absolute() else ROOT / t05_dir) / "a09-compare.json"
    compare = read_json(compare_path) if compare_path.is_file() else {}
    compare_checks = compare.get("checks") or []
    failed_checks = [check for check in compare_checks if not check.get("pass")]
    producer.check("evidence::a15-t05-old-vs-rust",
                   book.is_pass("r09_t05_browser_pdf") and bool(compare_checks) and not failed_checks,
                   detail={"checks": len(compare_checks), "failed": failed_checks[:5],
                           "allPass": compare.get("all_pass")})
    rows.append({
        "id": "measured::t05::old-chain-vs-rust-chain",
        "surface": "旧链（Electron 时代冻结基线）vs 服务侧 Rust 链：PDF 页数/文本/表格/视觉网格逐项",
        "aSide": {"kind": "r01-t05-frozen-old-chain", "refs": [rel(compare_path)]},
        "bSide": {"kind": "r09-rust-service-chain", "suite": "r09_t05_browser_pdf",
                  "cases": [], "note": "证据=同目录 a09-compare.json（35 项逐项 pass、PDF 74 页/文本 sha/表格/视觉网格）"},
        "status": "parity" if compare_checks and not failed_checks and compare.get("all_pass") else "differs",
        "basis": None,
    })
    book.load(producer, "r09_t05_untrusted_preview")
    producer.check("evidence::a15-t05-untrusted-preview-pass",
                   book.is_pass("r09_t05_untrusted_preview"),
                   detail={"status": book.books["r09_t05_untrusted_preview"]["status"]})
    rows.append({
        "id": "measured::t05::untrusted-preview-zero-host-access",
        "surface": "恶意预览内容无宿主权限（真实服务+真实 Chrome；Tauri 真机 IPC 矩阵属 T06 runtime blocked）",
        "aSide": {"kind": "r08-electron-baseline-receipt",
                  "refs": [FROZEN["r08Baseline"]["gate"]]},
        "bSide": {"kind": "r09-untrusted-preview", "suite": "r09_t05_untrusted_preview", "cases": []},
        "status": "parity" if book.is_pass("r09_t05_untrusted_preview") else "differs",
        "basis": None,
    })

    # T07 G1-hidden：Tauri 候选 app 真实启动（hidden+sink）——宿主壳加载/优雅退出
    book.load(producer, "r09_t07_native_e2e")
    t07_cases = ["t07::native-e2e.shell", "t07::native-e2e.graceful-exit",
                 "t07::native-e2e.no-process-residual", "t07::fixtures.deterministic-v3-anchor"]
    present = [case for case in t07_cases if case in book.books["r09_t07_native_e2e"]["caseMap"]]
    ok = len(present) == len(t07_cases) and all(book.case_ok("r09_t07_native_e2e", case) for case in t07_cases)
    producer.check("evidence::a15-t07-g1-hidden", ok,
                   detail={"cases": t07_cases,
                           "missing": [case for case in t07_cases if case not in present]})
    rows.append({
        "id": "measured::t07::g1-hidden-shell",
        "surface": "Tauri evidence 候选真实启动（hidden+sink）：tauri://localhost 加载、优雅退出、零残留、夹具锚点",
        "aSide": {"kind": "t01-same-window-electron",
                  "refs": ["artifacts/rust-tauri/R09/dev/t01/run-08"]},
        "bSide": {"kind": "r09-tauri-hidden-shell", "suite": "r09_t07_native_e2e", "cases": t07_cases},
        "status": "parity" if ok else "differs",
        "basis": None,
    })
    return rows


def build_a15_matrix(producer: Producer, book: EvidenceBook, sources: dict,
                     fixed: dict) -> dict:
    """A15 parity matrix：measured + 50 能力 + 71 命令 + 46 共享叶 + approvals。"""
    approvals = _check_approval_bases(producer)
    scene = scene_by_id(sources)
    model = load_capability_model(producer, sources)
    verify_capability_evidence(producer, book, model)
    rows = _measured_a15_rows(producer, book, sources)

    # 能力行：行为状态由证据成熟度决定（不冒充）
    parity_caps = {
        "host:main-window": "measured::t01::boot-dom-projection",
        "host:app-ready": "measured::t01::boot-dom-projection",
        "host:server-connection-info": "measured::t01::service-identity",
        "host:app-info-platform": "measured::t01::boot-dom-projection",
        "host:app-lifecycle-quit": "measured::t02::exit-ownership",
        "security:release-no-test-control": "measured::t01::renderer-identity",
    }
    pending_reasons = {
        "host:file-url-asset": "plugin/asset protocol 运行期接线缺失（见 A16 缺口）",
        "host:context-menu-policy": "R09 未实现（见 A16 缺口）",
    }
    cap_rows = []
    for cap_id in sorted(model):
        cap = model[cap_id]
        if cap_id in parity_caps:
            status = "parity"
            note = f"measured={parity_caps[cap_id]}"
        elif cap_id == "host:onboarding":
            status = "recorded"
            note = "RECORDED_APPROVALS=R09-ADJ-HOST-07（两侧一致显式 NOT_IMPLEMENTED；非换壳差异）"
        elif cap_id in CAP_APPROVAL and cap["status"] in ("deferred_registered", "recorded_design_boundary"):
            status = "recorded"
            note = f"RECORDED_APPROVALS={CAP_APPROVAL[cap_id]}"
        elif cap_id in BLOCKED_CAPS:
            status = "blocked"
            note = f"BLOCKED={BLOCKED_CAPS[cap_id]['blockerId']}"
        else:
            status = "pending"
            note = pending_reasons.get(cap_id, "Tauri 行为证据未覆盖（静态/单测支持不算 A15 实测）")
        evidence = {
            "suites": cap["evidenceSuites"],
            "prefixes": cap["evidencePrefixes"],
            "cases": cap["evidenceCases"],
            "deferred": cap["deferredRegistration"],
            "blocked": cap["blockedRegistration"],
            "approvalId": CAP_APPROVAL.get(cap_id) if status == "recorded" else None,
        }
        cap_rows.append({
            "id": f"capability::{cap_id}",
            "kind": "capability",
            "capability": cap_id,
            "group": cap["group"],
            "electronReference": cap["electronImpl"],
            "tauriImplementation": {"modules": cap["modules"], "commands": cap["commands"],
                                    "kind": cap["kind"], "hostInternal": cap["hostInternal"]},
            "consumer": {"inventory": cap["realConsumers"]},
            "evidence": evidence,
            "status": status,
            "note": note,
        })
    producer.check("matrix::capability-rows-50", len(cap_rows) == 50, detail={"rows": len(cap_rows)})
    for row in cap_rows:
        producer.check(f"parity::capability::{row['capability']}",
                       row["status"] in ("parity", "recorded", "blocked", "excluded_non_host"),
                       detail={"status": row["status"], "note": row["note"]})

    # 命令行：注册面逐条（71）；行为状态继承所属能力
    command_sets = parse_command_registration(producer, sources)
    policy = policy_command_sets(sources)
    cap_status = {row["capability"]: row["status"] for row in cap_rows}
    command_rows = []
    for command in policy["surface"]:
        owner = COMMAND_CAPABILITY.get(command)
        windows = sorted(label for label, commands in policy["windows"].items() if command in commands)
        impl_files = command_impl_files(command)
        surface_ok = (command in command_sets["release"]
                      and owner in model
                      and bool(impl_files))
        behavior = cap_status.get(owner, "pending")
        command_rows.append({
            "id": f"command::{command}",
            "kind": "command",
            "command": command,
            "ownerCapability": owner,
            "windows": windows,
            "implementationFiles": impl_files,
            "surfaceVerified": surface_ok,
            "status": behavior,
            "note": "行为状态继承所属能力；该行只声明注册面/实现符号核对",
        })
        producer.check(f"parity::command::surface::{command}", surface_ok,
                       detail={"owner": owner, "windows": windows, "files": impl_files})
    producer.check("matrix::command-rows-match-policy",
                   len(command_rows) == len(policy["surface"]),
                   detail={"rows": len(command_rows), "policySurface": len(policy["surface"])})
    unassigned = [cmd for cmd in policy["surface"] if cmd not in COMMAND_CAPABILITY]
    producer.check("matrix::commands-assigned-to-capability", not unassigned,
                   detail={"unassigned": unassigned})

    # 46 共享叶行：30 宿主 + 16 SERVER_REACT
    leaf_rows = []
    for leaf_id, mapping in sorted(scene["t07Leaves"].items()):
        host_class = mapping.get("hostClass")
        leaf = scene["tasks"].get(leaf_id) or {}
        plan = scene["t03Leaves"].get(leaf_id) or {}
        blocked_assertions = [item for item in (sources["t03Plan"].get("blockedAssertions") or [])
                              if item.get("leafId") == leaf_id]
        manual_assertions = [item for item in (sources["t03Plan"].get("manualOnlyAssertions") or [])
                             if item.get("leafId") == leaf_id]
        if host_class not in HOST_LEAF_CLASSES:
            status = "excluded_non_host"
            note = "SERVER_REACT：实现/证据归 server/React，不属宿主面（T03_T04_PROGRESS §9.1）"
        elif blocked_assertions:
            status = "blocked"
            note = "T03 计划中该叶存在 blocked 断言（onboarding 服务路由/DOCX/XLSX 命令缺口，已登记 T03_T04_PROGRESS）；真实动作证据 pending"
        else:
            status = "pending"
            note = "T03 GUI 真实动作证据 pending（plannedActionCase 未执行；不得用逻辑单测冒充）"
        planned_cases = [assertion.get("plannedActionCase") for assertion in plan.get("assertions", [])
                         if assertion.get("plannedActionCase")]
        covered = sum(1 for case in planned_cases if case in book.books.get("r09_t03_native_matrix", {}).get("caseMap", {}))
        leaf_rows.append({
            "id": f"leaf::{leaf_id}",
            "kind": "shared-leaf",
            "leafId": leaf_id,
            "hostClass": host_class,
            "classification": mapping.get("classification"),
            "assertionCount": mapping.get("assertionCount"),
            "plannedSuite": mapping.get("plannedSuite"),
            "plannedActionCases": planned_cases,
            "plannedCasesObserved": covered,
            "blockedAssertions": [item.get("case") for item in blocked_assertions],
            "manualAssertions": [item.get("case") for item in manual_assertions],
            "electronReference": mapping.get("hostClassJudgement"),
            "tauriEvidence": {"suite": mapping.get("plannedSuite"),
                             "observedActionCases": covered},
            "status": status,
            "note": note,
        })
        producer.check(f"parity::leaf::{leaf_id}", status in ("parity", "recorded", "blocked", "excluded_non_host"),
                       detail={"status": status, "note": note})
    producer.check("matrix::leaf-rows-46", len(leaf_rows) == 46, detail={"rows": len(leaf_rows)})
    producer.check("matrix::host-leaf-classification",
                   sum(1 for row in leaf_rows if row["hostClass"] in HOST_LEAF_CLASSES) == 30
                   and sum(1 for row in leaf_rows if row["hostClass"] not in HOST_LEAF_CLASSES) == 16,
                   detail={"host": sum(1 for row in leaf_rows if row["hostClass"] in HOST_LEAF_CLASSES)})

    # 汇总统计
    def count(rows: list[dict], key: str = "status") -> dict:
        summary = {}
        for row in rows:
            summary[row[key]] = summary.get(row[key], 0) + 1
        return summary

    measured = [row for row in rows if row["id"].startswith("measured::")]
    measured_summary = count(measured)
    capability_summary = count(cap_rows)
    leaf_summary = count(leaf_rows)
    unapproved = []
    for row in measured:
        if row["status"] == "differs":
            unapproved.append({"id": row["id"], "reason": "measured evidence failed/unavailable"})
    for row in cap_rows:
        if row["status"] == "differs":
            unapproved.append({"id": row["id"], "reason": "capability evidence mismatch"})
    # 未批准差异：pending 项不是「差异」，但必须如实列出（A15 未成立的原因）
    pending = ([row["id"] for row in cap_rows if row["status"] == "pending"]
               + [row["id"] for row in leaf_rows if row["status"] == "pending"]
               + [row["id"] for row in measured if row["status"] == "pending"])
    blocked = ([row["id"] for row in cap_rows if row["status"] == "blocked"]
               + [row["id"] for row in leaf_rows if row["status"] == "blocked"])
    recorded = [row["id"] for row in cap_rows if row["status"] == "recorded"]
    producer.check("a15::no-unapproved-differences", not unapproved,
                   detail={"unapproved": unapproved})
    producer.check("a15::parity-not-yet-complete", not pending,
                   detail={"pendingCount": len(pending), "sample": pending[:20],
                           "note": "pending = 尚无 Tauri 真实行为证据；不得写 PASS"})
    producer.check("a15::blocked-items-registered",
                   all(row.get("evidence", {}).get("blocked") or row.get("blockedAssertions") is not None
                       for row in cap_rows + leaf_rows if row["status"] == "blocked"),
                   detail={"blocked": blocked})
    producer.check("a15::recorded-approvals-have-basis",
                   all(approvals.get(approval_id) for approval_id in CAP_APPROVAL.values()),
                   detail={"approvals": sorted({v for k, v in CAP_APPROVAL.items()})})

    matrix = {
        "schema": "lingxi.r09.shell-parity-matrix.v1",
        "stage": "R09",
        "acceptance": "R09-A15",
        "suite": "r09_t08_shell_parity",
        "fixedFactors": fixed,
        "recordedApprovals": RECORDED_APPROVALS,
        "measuredRows": measured,
        "capabilityRows": cap_rows,
        "commandRows": command_rows,
        "leafRows": leaf_rows,
        "summary": {
            "measured": measured_summary,
            "capabilities": capability_summary,
            "leaves": leaf_summary,
            "recorded": len(recorded),
            "blocked": len(blocked),
            "pending": len(pending),
            "unapprovedDifferences": unapproved,
        },
    }
    return matrix


def _a15_fill_shell_parity_results(matrix: dict, fixed: dict) -> dict:
    """SHELL_PARITY_RESULTS.json 草稿内容（Lead 汇总 docs 最终文件的建议填充）。"""
    summary = matrix["summary"]
    task_total = summary["capabilities"].get("parity", 0) + summary["capabilities"].get("recorded", 0) \
        + summary["capabilities"].get("pending", 0) + summary["capabilities"].get("blocked", 0)
    return {
        "schema": "lingxi.r09.shell-parity.v1",
        "stage": "R09",
        "status": "DRAFT_T08_%s_SELFCHECK_PENDING_GUI_EVIDENCE" % (
            str(CANDIDATE_META.get("candidateVersion") or "UNKNOWN").replace(".", "").upper()),
        "generatedAt": None,
        "candidate": CANDIDATE_META,
        "note": ("T08 生产者草稿：由 r09_t08_shell_parity 聚合既有证据生成；"
                 "pending/blocked 未完成前不得引用为对照成立。Lead 汇总后覆盖 docs/rust-tauri/R09/SHELL_PARITY_RESULTS.json。"),
        "methodology": {
            "variableUnderTest": "desktop host only (Electron host vs Tauri host)",
            "heldConstant": [
                "same Rust service (sidecar sha256 + build.json protocol/dataEpoch; T01 两壳注入同一 LINGXI_SERVICE_BIN)",
                "same React renderer build (build-once-bind-once rendererDigest/themeBundleSha256)",
                "same deterministic fixtures (R08 v3Base anchor; T07 实测 case)",
                "same protocol/data epoch (wire 1..=1, data epoch 2)",
            ],
            "aSide": "R08 verified Electron host + Rust service (verify-r08-closeout-08, overall PASS)",
            "bSide": "R09 frozen Tauri candidate + same Rust service/renderer",
            "harness": "scripts/rust-tauri/r09_t08.py --suite r09_t08_shell_parity",
            "recordedApprovalsRule": "Any host difference must be registered with a documented basis; unapproved differences fail A15",
        },
        "fixedHashes": {
            "rustServiceSha256": fixed["delivery"].get("sidecarSha256"),
            "rustServiceContentSha256": FROZEN["deliveryRelease"]["serviceContentSha256"],
            "rendererDigest": fixed["renderer"].get("digest"),
            "themeBundleSha256": fixed["renderer"].get("themeBundleSha256"),
            "fixturesManifestSha256": FROZEN["fixtures"]["v3Base"],
            "electronBaselineReceipt": {
                "path": FROZEN["r08Baseline"]["gate"],
                "testedSha": fixed["r08Baseline"].get("testedSha"),
                "candidateSourceDigest": fixed["r08Baseline"].get("candidateSourceDigest"),
                "ab": fixed["r08Baseline"].get("ab"),
            },
            "tauriCandidateReceipt": {
                "deliveryAppTreeSha256": fixed["delivery"].get("appTreeSha256"),
                "hostExeSha256": fixed["delivery"].get("hostExeSha256"),
                "evidenceBuildSha256": fixed["evidence"].get("executableSha256Actual"),
                "featuresDelivery": fixed["delivery"].get("features"),
                "featuresEvidence": fixed["evidence"].get("features"),
            },
        },
        "tasks": [
            {
                "taskId": "R09-A15",
                "aSide": "R08 Electron host baseline (closeout-08 PASS, 71 A/B tasks = 68 parity + 3 recorded)",
                "bSide": "R09 Tauri frozen candidate",
                "verdict": "PARTIAL",
                "measuredParityRows": summary["measured"].get("parity", 0),
                "capabilityParity": summary["capabilities"].get("parity", 0),
                "capabilityRecorded": summary["capabilities"].get("recorded", 0),
                "capabilityPending": summary["capabilities"].get("pending", 0),
                "capabilityBlocked": summary["capabilities"].get("blocked", 0),
                "leafParity": summary["leaves"].get("parity", 0),
                "leafExcludedNonHost": summary["leaves"].get("excluded_non_host", 0),
                "leafPending": summary["leaves"].get("pending", 0),
                "leafBlocked": summary["leaves"].get("blocked", 0),
            }
        ],
        "summary": {
            "totalCapabilities": task_total,
            "parity": summary["capabilities"].get("parity", 0),
            "recorded": summary["capabilities"].get("recorded", 0),
            "differs": len(summary["unapprovedDifferences"]),
            "pending": summary["pending"],
            "blocked": summary["blocked"],
            "hostPlaceholders": "由 r09_t08_host_completeness 产出",
            "leafCoverage": {"total": 74, "bound": 15, "pending": 59},
        },
        "leafParity": [
            {"leafId": row["leafId"], "status": row["status"], "plannedSuite": row["plannedSuite"]}
            for row in matrix["leafRows"]
        ],
        "unapprovedDifferences": summary["unapprovedDifferences"],
        "pendingBeforePASS": [
            "T01/T02/T05/T07 v2 候选证据重跑（candidate-binding 红项；正式门 in-gate 自动满足）",
            "T03/T04/T07 hidden+visible GUI 真实动作批次（19 能力 + T03 26 叶）",
            "T06 运行期 ACL hidden 批次（security:* 行为对照）",
            "T07 release smoke（交付 release 启动/连接/文件任务）",
            "file-url-asset T07 运行期证据（asset scope 命中 + 内容校验）",
        ],
        "sourceDrift": fixed.get("sourceDrift"),
        "evidence": "artifacts/rust-tauri/R09/dev/t08/<run>/ (shell-parity-cases.json / parity-matrix.json)",
    }


# ════════════════════════════════════════════════════════════════════════════
# R09-A16｜r09_t08_host_completeness
# ════════════════════════════════════════════════════════════════════════════
KBD_RE = re.compile(r"allow-([a-z0-9-]+)")

# 非宿主 R00 入口类别的归属理由（Tauri 宿主不是 owner；不得当宿主占位）
NON_HOST_ENTRY_REASONS = {
    "http_ws_server": "Rust lingxi-service HTTP/WS 服务（R07/R08 已迁移）；Tauri 宿主只做 sidecar 监督与连接信息注入",
    "cli": "hana CLI（server-first 客户端）；非桌面宿主入口",
    "bridge": "外部 IM 桥接在 lingxi-service 内运行；桌面宿主不持有 bridge 适配器",
    "schedulers": "心跳/cron/loop 调度器在 lingxi-service 内部；桌面宿主无调度入口",
    "channels_dm": "频道/DM 轮次在 lingxi-service hub 内；桌面宿主只呈现事件",
    "subagents": "子代理工具在服务侧 agent 运行时；桌面宿主无独立入口",
    "plugins_mcp": "插件/MCP 在 lingxi-service 内；桌面宿主只消费服务 API",
    "tool_boundary": "工具执行边界在服务侧 ToolInvocationGateway；桌面宿主无工具执行入口",
    "renderer_clients": "同一 React 产物（T01 已验证两壳同构建）；非独立宿主入口",
}

# R00 桌面原生入口 → Tauri 等价实现/登记
DESKTOP_ENTRY_MAP = {
    "EP-DESK-01": {"status": "wired", "tauri": ["desktop/src-tauri/src/main.rs", "desktop/src-tauri/src/host.rs"],
                   "evidence": {"suites": ["r09_t01_react_parity", "r09_t02_supervisor_ready"],
                                "cases": ["a01::tauri-boots", "t02::ready-handshake-real"]}},
    "EP-DESK-02": {"status": "wired", "tauri": ["desktop/src-tauri/src/service/supervisor.rs"],
                   "evidence": {"suites": ["r09_t02_supervisor_ready", "r09_t02_process_ownership"],
                                "cases": ["t02::ready-handshake-real", "t02::no-orphan-owned-services"]},
                   "note": "Rust service 取代 Node server（R08 A/B 已证内核替换；非 Tauri 引入差异）"},
    "EP-DESK-03": {"status": "recorded", "tauri": ["desktop/src-tauri/src/host.rs", "desktop/dist-renderer"],
                   "evidence": {"suites": ["r09_t01_react_parity"],
                                "cases": ["a01::dom-snapshot-parity", "a01::tauri-loads-embedded-page"]},
                   "approvalId": "R09-ADJ-HOST-01",
                   "note": "页面来源 file://→tauri://localhost（RECORDED_APPROVALS）"},
    "EP-DESK-04": {"status": "pending_gui", "tauri": ["desktop/src-tauri/src/native/window_labels.rs",
                                                      "desktop/src-tauri/src/native/settings_window.rs",
                                                      "desktop/src-tauri/src/native/quick_chat.rs",
                                                      "desktop/src-tauri/src/native/splash.rs",
                                                      "desktop/src-tauri/src/browser_viewer.rs",
                                                      "desktop/src-tauri/src/resource_bridge.rs"],
                   "evidence": {"docs": ["docs/rust-tauri/R09/T06_GUI_BATCH_NEEDS.json",
                                        "docs/rust-tauri/R09/T07_PROGRESS.md"],
                                "note": "window-open/真实窗口证据在统一 visible GUI 批次"},
                   "registration": "T06_GUI_BATCH_NEEDS / T07_PROGRESS §9-10 visible batch"},
    "EP-DESK-05": {"status": "blocked", "tauri": ["desktop/src-tauri/src/native/tray.rs",
                                                  "desktop/src-tauri/src/native/global_shortcut.rs",
                                                  "desktop/src-tauri/src/native/notification.rs",
                                                  "desktop/src-tauri/src/native/auto_launch.rs",
                                                  "desktop/src-tauri/src/native/keep_awake.rs"],
                   "evidence": {"docs": ["docs/rust-tauri/R09/BLOCKED.md",
                                        "docs/rust-tauri/R09/T06_GUI_BATCH_NEEDS.json"]},
                   "registration": "BLOCKED.md T03-REPAIR-02/T03-NOTIFY-02 + T06_GUI_BATCH_NEEDS",
                   "note": "真实系统动作需 visible GUI batch；「修复组件…」按裁决移除，通知点击待验证"},
    "EP-DESK-06": {"status": "deferred", "tauri": ["docs/rust-tauri/R09/R09_COMMAND_POLICY.json"],
                   "evidence": {"docs": ["docs/rust-tauri/R09/T06_PROGRESS.md"]},
                   "registration": "T06 policy deferredCommands=R10-update-chain"},
    "EP-DESK-07": {"status": "blocked", "tauri": ["rust/crates/lingxi-service（PDF/ASR/computer-use 服务侧）",
                                                  "desktop/src-tauri/src/native/speech.rs",
                                                  "desktop/src-tauri/src/native/permissions.rs"],
                   "evidence": {"docs": ["docs/rust-tauri/R09/BLOCKED.md",
                                        "docs/rust-tauri/R09/T03_T04_PROGRESS.md"]},
                   "registration": "BLOCKED-SYSTEM-PERMISSIONS / BLOCKED-REAL-PROVIDER / T04-HELPER-01",
                   "note": "PDF helper 服务侧已由 T05 PASS；Speech/TCC/computer-use helper 受阻塞登记"},
}


def command_impl_files(command: str) -> list[str]:
    files = []
    for path in TAURI_SRC.rglob("*.rs"):
        try:
            text = read_text(path)
        except OSError:
            continue
        if re.search(r"fn\s+" + re.escape(command) + r"\s*\(", text):
            files.append(rel(path))
    return sorted(set(files))


def parse_command_registration(producer: Producer, sources: dict) -> dict:
    """独立解析 71 命令四集合：build.rs / invoke_handler / permissions / capabilities。"""
    build_rs = read_text(TAURI_CRATE / "build.rs")
    build_block = build_rs[build_rs.find("let mut commands: Vec<&str> = vec!["):]
    build_block = build_block[: build_block.find("];") + 2]
    build_commands = set(re.findall(r'"([a-z0-9_]+)"', build_block))
    extend_match = re.search(r"commands\.extend\(\[(.*?)\]\)", build_rs, re.S)
    debug_extend = set(re.findall(r'"([a-z0-9_]+)"', extend_match.group(1))) if extend_match else set()

    lib_rs = read_text(TAURI_SRC / "lib.rs")
    handler_blocks = []
    for match in re.finditer(r"generate_handler!\[", lib_rs):
        start = match.end()
        end = lib_rs.find("]", start)
        body = lib_rs[start:end]
        names = set(re.findall(r"\b([a-z][a-z0-9_]*)\b", body))
        handler_blocks.append(names)
    handler_release = handler_blocks[0] if handler_blocks else set()
    handler_with_hooks = handler_blocks[1] if len(handler_blocks) > 1 else set()

    permission_commands = set()
    kebab_to_command = {}
    for path in sorted((TAURI_CRATE / "permissions/autogenerated").glob("*.toml")):
        text = read_text(path)
        identifier = re.search(r'identifier\s*=\s*"allow-([a-z0-9-]+)"', text)
        allow = re.search(r'commands\.allow\s*=\s*\["([a-z0-9_]+)"\]', text)
        if allow:
            permission_commands.add(allow.group(1))
        if identifier:
            kebab_to_command[identifier.group(1)] = allow.group(1) if allow else None
    debug_permission_files = sorted(
        path.name for path in (TAURI_CRATE / "permissions/autogenerated").glob("debug_*.toml"))

    capability_grants = {}
    unresolved = []
    referenced = set()
    for path in sorted((TAURI_CRATE / "capabilities").glob("*.json")):
        payload = read_json(path)
        windows = payload.get("windows") or []
        grants = set()
        for permission in payload.get("permissions") or []:
            match = KBD_RE.fullmatch(permission)
            if not match:
                continue
            referenced.add(permission)
            command = kebab_to_command.get(match.group(1))
            if command is None:
                unresolved.append({"file": rel(path), "permission": permission})
            else:
                grants.add(command)
        capability_grants[rel(path)] = {"windows": windows, "commands": sorted(grants)}
    all_capability_grants = set()
    for spec in capability_grants.values():
        all_capability_grants.update(spec["commands"])

    dev_capability_grants = set()
    dev_capability_path = TAURI_CRATE / "capabilities-test/r09-test-hooks.json"
    if dev_capability_path.is_file():
        payload = read_json(dev_capability_path)
        for permission in payload.get("permissions") or []:
            match = KBD_RE.fullmatch(permission)
            if match and kebab_to_command.get(match.group(1)):
                dev_capability_grants.add(kebab_to_command[match.group(1)])

    release = set(build_commands)
    debug_commands = set(debug_extend)
    result = {
        "buildCommands": sorted(build_commands),
        "handlerRelease": sorted(handler_release),
        "handlerWithHooks": sorted(handler_with_hooks),
        "permissionCommands": sorted(permission_commands),
        "capabilityGrants": capability_grants,
        "capabilitySurface": sorted(all_capability_grants),
        "devCapabilityGrants": sorted(dev_capability_grants),
        "debugPermissionFiles": debug_permission_files,
        "unresolvedPermissions": unresolved,
        "release": sorted(release),
        "debugOnly": sorted(debug_commands),
        "debugExtendSource": sorted(debug_extend),
    }
    policy = policy_command_sets(sources)
    expected = set(policy["surface"])
    producer.check("commands::build-manifest-nonzero", len(build_commands) >= 71,
                   detail={"count": len(build_commands)})
    producer.check("commands::invoke-handler-equals-build-manifest",
                   set(build_commands) == set(handler_release)
                   and set(handler_with_hooks) == set(handler_release) | set(debug_commands),
                   detail={"build": len(build_commands), "handler": len(handler_release),
                           "handlerWithHooks": len(handler_with_hooks),
                           "debugOnly": sorted(debug_commands)})
    producer.check("commands::release-set-equals-policy-surface",
                   set(release) == expected,
                   detail={"release": len(release), "policy": len(expected),
                           "onlyRelease": sorted(set(release) - expected)[:8],
                           "onlyPolicy": sorted(expected - set(release))[:8]})
    producer.check("commands::permissions-files-cover-registered",
                   expected <= permission_commands,
                   detail={"missing": sorted(expected - permission_commands)[:8]})
    producer.check("commands::capabilities-grants-resolve",
                   not unresolved and set(all_capability_grants) == expected,
                   detail={"unresolved": unresolved[:8],
                           "onlyGranted": sorted(set(all_capability_grants) - expected)[:8],
                           "missingGrants": sorted(expected - set(all_capability_grants))[:8]})
    producer.check("commands::debug-commands-gated",
                   set(debug_commands) <= {"debug_open_onboarding", "debug_open_onboarding_preview",
                                            "debug_asset_probe"}
                   and set(debug_commands).isdisjoint(all_capability_grants)
                   and set(debug_commands) <= set(handler_with_hooks),
                   detail={"debugOnly": sorted(debug_commands),
                           "devCapabilityGrants": sorted(dev_capability_grants)})
    producer.check("commands::all-have-impl-symbol",
                   all(command_impl_files(command) for command in expected),
                   detail={"missing": [cmd for cmd in expected if not command_impl_files(cmd)][:8]})
    return result


def resolve_static_case(book: EvidenceBook, case_name: str) -> tuple[bool | None, str | None]:
    """T06 静态证据：优先同门 capability_matrix（`static::caps::` 前缀），否则最终 dev audit。"""
    matrix = book.books.get("r09_t06_capability_matrix")
    if matrix:
        for name in (case_name, "static::" + case_name):
            if name in matrix["caseMap"]:
                return matrix["caseMap"][name], f"r09_t06_capability_matrix/{name}"
    audit = book.books.get("audit_capability")
    if audit and case_name in audit["caseMap"]:
        return audit["caseMap"][case_name], f"audit_capability/{case_name}"
    return None, None


def resolve_release_case(book: EvidenceBook, case_name: str) -> tuple[bool | None, str | None]:
    surface = book.books.get("r09_t06_release_attack_surface")
    if surface and case_name in surface["caseMap"]:
        return surface["caseMap"][case_name], f"r09_t06_release_attack_surface/{case_name}"
    scan = book.books.get("release_scan")
    if scan and case_name in scan["caseMap"]:
        return scan["caseMap"][case_name], f"release_scan/{case_name}"
    return None, None


def load_supporting_books(producer: Producer, book: EvidenceBook) -> None:
    for suite in UPSTREAM:
        if suite in ("audit_capability", "release_scan"):
            continue
        book.load(producer, suite)
    for suite in ("audit_capability", "release_scan"):
        book.load(producer, suite)


def verify_capability_evidence(producer: Producer, book: EvidenceBook,
                               model: dict) -> dict:
    """逐能力核对已存在的证据 case/前缀（只读、不改状态）。

    - 前缀：在声明 suite 的 caseMap 里匹配；任一命中 case 非 ok 即失败；
    - 显式 case：caps::/release:: 走 T06 静态证据解析（同门优先），其余在声明
      suite 集合内任一出现即可；缺失/失败即该能力证据不成立。
    """
    evidence_summary = {}
    for cap_id, cap in model.items():
        spec_suites = list(cap["evidenceSuites"])
        prefixes = list(cap["evidencePrefixes"])
        case_names = list(cap["evidenceCases"])
        missing = []
        failed = []
        matched_prefix_cases = 0
        case_maps = {}
        for suite in spec_suites:
            if suite not in book.books:
                book.load(producer, suite)
            case_maps[suite] = book.books[suite]["caseMap"]
        for suite, case_map in case_maps.items():
            for prefix in prefixes:
                matches = [name for name in case_map if name.startswith(prefix)]
                matched_prefix_cases += len(matches)
                failed.extend(name for name in matches if not case_map[name])
        for name in case_names:
            if name.startswith("caps::"):
                ok, resolved = resolve_static_case(book, name)
            elif name.startswith("release::"):
                ok, resolved = resolve_release_case(book, name)
            else:
                ok, resolved = None, None
                for suite, case_map in case_maps.items():
                    if name in case_map:
                        ok, resolved = case_map[name], f"{suite}/{name}"
                        break
            if ok is None:
                missing.append({"case": name})
            elif not ok:
                failed.append(resolved or name)
        if spec_suites and prefixes and matched_prefix_cases == 0:
            missing.append({"prefixes": prefixes, "suites": spec_suites})
        checks = bool(spec_suites or case_names)
        passed = checks and not missing and not failed
        if cap["status"] in ("deferred_registered", "blocked_registered", "recorded_design_boundary"):
            passed = True
        if cap["kind"] == "missing":
            passed = False
        cap["evidenceChecks"] = len(prefixes) + len(case_names)
        cap["evidencePassed"] = passed if checks else None
        evidence_summary[cap_id] = {
            "suites": spec_suites, "prefixes": prefixes, "cases": case_names,
            "matchedPrefixCases": matched_prefix_cases,
            "missing": missing, "failed": failed, "passed": cap["evidencePassed"],
        }
        producer.check(f"evidence::capability::{cap_id}", bool(passed),
                       detail=evidence_summary[cap_id])
    return evidence_summary


# A16：已有「真实调用结果」证据的能力（不是纯逻辑单测/静态策略）。
# 其余能力的真实动作证据在 GUI clearance 批次（T06_GUI_BATCH_NEEDS / T07 §9-10）。
REAL_BEHAVIOR_CAPS = {
    "host:main-window": "T01 同窗 Electron/Tauri 启动 + DOM 投影 + T07 G1 hidden 启动",
    "host:app-ready": "T01 a01::tauri-boots / T07 G1 app-ready + graceful-exit",
    "host:server-connection-info": "T01 service-identity parity + T02 真实 supervisor 握手/健康/退出",
    "host:app-info-platform": "T01 同窗 platform/title 投影一致",
    "host:app-lifecycle-quit": "T02 退出所有权/no-orphan（真实进程树）+ T07 G1 graceful-exit/零残留",
}


def compute_capability_completeness(model: dict, host: dict, sources: dict) -> None:
    """给每个能力算：实现入口/真实消费者/capability 权限/渲染层接线/缺口分类。"""
    scene = scene_by_id(sources)
    for cap_id, cap in model.items():
        # 实现入口：原生模块文件存在；无模块的能力由 deferred/blocked/security 分支单独判定
        impl_ok = cap["modulesPresent"] if cap["modules"] else cap["kind"] != "native-module"
        # capability/permission：注册命令的窗口授予 + 插件权限基线（静态由 T06 audit 覆盖）
        window_permissions = []
        for label, commands in scene["policyWindows"].items():
            granted = [cmd for cmd in cap["commands"] if cmd in (commands.get("allowedCommands") or [])]
            if granted:
                window_permissions.append({"window": label, "commands": sorted(granted)})
        # 渲染层接线
        members = [member.replace(" (preload-only)", "") for member in cap["apiMembers"]]
        member_states = {member: member_status(member, host) for member in members}
        non_wired = {member: state for member, state in member_states.items()
                     if state not in ("wired", "explicit-noop", "absent-with-reason")}
        if cap["hostInternal"] or not members:
            renderer_status = "host-internal"
        elif not non_wired:
            renderer_status = "wired"
        elif all(state in ("fragment-only", "fragment-reason-only", "restricted-with-reason")
                 for state in non_wired.values()):
            # 全部未接线成员都有显式理由（fragment capabilityReasons）→ 显式受限（已登记），非静默缺失
            if all(member in host["fragmentReasonMap"] for member in non_wired):
                renderer_status = "restricted-registered"
            elif not host["fragmentMerged"]:
                renderer_status = "fragment-not-merged"
            else:
                renderer_status = "unwired"
        else:
            renderer_status = "unwired"
        cap["implementationOk"] = impl_ok
        cap["windowPermissions"] = window_permissions
        cap["rendererMembers"] = member_states
        cap["rendererStatus"] = renderer_status
        cap["gaps"] = []
        if not impl_ok:
            cap["gaps"].append("implementation-missing")
        if renderer_status in ("fragment-not-merged", "unwired"):
            cap["gaps"].append(renderer_status)
        elif renderer_status == "restricted-registered":
            cap["gaps"].append("restricted-registered (explicit capabilityReason; 运行期证据仍 pending)")
        if cap["kind"] == "missing":
            cap["gaps"].append("capability-unimplemented")
        # A16 判定：真实实现 + 真实消费者/接线 + 真实调用结果证据（或已登记 blocked/deferred/设计边界）
        registration_ok = cap["status"] in ("deferred_registered", "blocked_registered",
                                            "recorded_design_boundary")
        real_evidence = cap_id in REAL_BEHAVIOR_CAPS
        if real_evidence:
            cap["evidenceMaturity"] = "real-behavior"
        elif cap["kind"] == "security-policy":
            cap["evidenceMaturity"] = "static-policy"
        elif registration_ok:
            cap["evidenceMaturity"] = "registered"
        elif cap.get("evidencePassed"):
            cap["evidenceMaturity"] = "support-cases-only"
        else:
            cap["evidenceMaturity"] = "none"
        cap["realBehaviorEvidence"] = REAL_BEHAVIOR_CAPS.get(cap_id)
        if cap["kind"] == "security-policy":
            cap["a16Ok"] = bool(cap.get("evidencePassed")) and impl_ok
        elif registration_ok:
            cap["a16Ok"] = impl_ok
        elif cap["status"] == "missing_unregistered":
            cap["a16Ok"] = False
        else:
            cap["a16Ok"] = (impl_ok and renderer_status in ("wired", "host-internal", "restricted-registered")
                            and real_evidence)



# ── T08 v2：PlatformApi 矩阵 + context-menu 真实行为断言 ───────────────────

def _asset_scope_entries(sources: dict) -> list[str]:
    """从 base/release tauri conf 收集 assetProtocol scope 条目（v2 patch 是否已落地）。"""
    entries: list[str] = []
    for key in ("tauriBaseConf", "tauriReleaseConf"):
        text = sources.get(key) or ""
        position = text.find('"assetProtocol"')
        if position < 0:
            continue
        block = text[position: position + 2000]
        match = re.search(r'"scope"\s*:\s*\[(.*?)\]', block, re.S)
        if not match:
            continue
        entries.extend(value for value in re.findall(r'"([^"]+)"', match.group(1)) if value)
    return sorted(set(entries))


def _platform_api_rows(host: dict) -> list[dict]:
    rows = []
    for member in host["platformMembers"]:
        status = member_status(member, host)
        reason = None
        if status == "restricted-with-reason":
            reason = host["fragmentReasonMap"].get(member)
        elif status == "absent-with-reason":
            reason = host["noopReasonMap"].get(member)
        if status == "wired":
            classification = "real-executable"
        elif status == "explicit-noop":
            classification = "explicit-noop"
        elif status == "absent-with-reason":
            classification = "absent-optional-reasoned"
        else:
            classification = "restricted-reasoned"
        rows.append({
            "member": member,
            "status": status,
            "classification": classification,
            "realExecutable": status == "wired",
            "conditionalInjection": member in set(host.get("conditionalMembers") or []),
            "capabilityReason": reason,
        })
    return rows


def run_context_menu_behavior(producer: Producer) -> dict:
    """真实行为断言：jsdom 分发 contextmenu，验证非可编辑拦截/可编辑保留。"""
    command = [
        "npx", "vitest", "run",
        "desktop/src/react/__tests__/host/r09-t01-tauri-desktop-host.test.tsx",
        "-t", "右键策略",
    ]
    try:
        result = subprocess.run(command, cwd=str(ROOT), capture_output=True, text=True, timeout=300)
        log = (result.stdout or "") + (result.stderr or "")
        (producer.evidence / "context-menu-behavior.log").write_text(log, encoding="utf-8")
        ok = result.returncode == 0 and bool(re.search(r"Tests\s+1 passed", log))
        producer.check("a16::context-menu-policy.real-behavior", ok, detail={
            "command": " ".join(command), "exit": result.returncode,
            "assertion": "非可编辑区 contextmenu.defaultPrevented=true；textarea/可编辑区 defaultPrevented=false",
            "evidence": "context-menu-behavior.log",
        })
        return {"ok": ok, "exit": result.returncode, "log": "context-menu-behavior.log", "command": command}
    except (OSError, subprocess.TimeoutExpired) as error:
        producer.check("a16::context-menu-policy.real-behavior", False,
                       detail={"error": f"{type(error).__name__}: {error}"})
        return {"ok": False, "error": f"{type(error).__name__}: {error}", "command": command}


def override_renderer_only_caps(model: dict, host: dict, sources: dict, context_ok: bool) -> None:
    """host:file-url-asset / host:context-menu-policy 是 renderer+配置能力，非 native 模块。"""
    asset_scope = _asset_scope_entries(sources)
    file_url = model.get("host:file-url-asset")
    if file_url is not None:
        file_url["kind"] = "renderer+config"
        if host.get("getFileUrlConvert") and asset_scope:
            file_url["status"] = "wired_config"
            file_url["notes"] = [f"convertFileSrc 已接线；asset scope={asset_scope}",
                                 "运行期证据：T07 G1 file-url-asset（scope 命中 + 内容校验）"]
            file_url["gaps"] = [] if file_url.get("evidencePassed") else [
                "asset-scope-runtime-evidence-pending"]
            file_url["a16Ok"] = bool(file_url.get("evidencePassed"))
            file_url["evidenceMaturity"] = "real-behavior" if file_url.get("evidencePassed") else "wired-static"
        else:
            file_url["status"] = "renderer_wired_scope_pending"
            file_url["notes"] = ["convertFileSrc 已接线" if host.get("getFileUrlConvert") else "convertFileSrc 未接线",
                                 f"assetProtocol scope 仍为空（{asset_scope}）→ 需 T06 v2 patch（HOST_V2_PATCHES §1）"]
            file_url["a16Ok"] = False
            file_url["evidenceMaturity"] = "none"
        file_url["realBehaviorEvidence"] = {"planned": "T07 G1：convertFileSrc→fetch/ <img> onload（scope 内文件）+ 越界路径拒绝"}
    context = model.get("host:context-menu-policy")
    if context is not None:
        context["kind"] = "renderer-policy"
        context["status"] = "wired_renderer_equivalent" if host.get("contextMenuPolicyInstalled") else "missing_unregistered"
        context["evidenceMaturity"] = "real-behavior" if context_ok else "none"
        context["evidencePassed"] = context_ok
        context["realBehaviorEvidence"] = {"case": "a16::context-menu-policy.real-behavior",
                                            "assertion": "renderer contextmenu preventDefault 行为（Electron main.cjs:490-507 等价）",
                                            "note": "jsdom 真实事件断言；无需可见窗口；release devtools 残留见 HOST_V2_PATCHES §2"}
        context["a16Ok"] = bool(context_ok) and host.get("contextMenuPolicyInstalled")
        context["notes"] = ["renderer 层等价实现 installTauriContextMenuPolicy()；Rust release devtools/原生菜单残留见 patch 清单"]


def build_a16_matrix(producer: Producer, book: EvidenceBook, sources: dict,
                     fixed: dict) -> tuple[dict, dict]:
    scene = scene_by_id(sources)
    host = parse_tauri_host(sources)
    model = load_capability_model(producer, sources)
    load_supporting_books(producer, book)
    verify_capability_evidence(producer, book, model)
    compute_capability_completeness(model, host, sources)
    command_reg = parse_command_registration(producer, sources)
    policy = policy_command_sets(sources)

    # ── 渲染层接线静态事实（A16 隐藏按钮扫描基础）──────────────────────
    producer.check("bridge::fragment-file-present",
                   bool(sources["capFragmentTs"].strip()),
                   detail={"fragmentMembers": len(host["fragmentMembers"])})
    producer.check("bridge::tauri-capability-fragment-merged",
                   host["fragmentMerged"],
                   detail={"merged": host["fragmentMerged"],
                           "fragmentMembers": len(host["fragmentMembers"]),
                           "note": "T08 修复后 tauri-desktop-host.ts 正式 import/合并片段；覆盖成员真实 invoke/listen"})
    wired_members = set(host["wired"]) | set(host["absentMembers"]) | set(host["fragmentReasons"])
    used = set(host["rendererUsed"])
    unwired_used = sorted(member for member in used if member not in wired_members)
    producer.check("bridge::ui-members-wired", not unwired_used,
                   detail={"unwiredUsedMembers": unwired_used[:30],
                           "totalUnwiredUsed": len(unwired_used),
                           "note": "渲染层真实消费的 PlatformApi 成员在 Tauri 宿主未接线（调用即 HostCapabilityError）"})

    # ── PlatformApi 成员矩阵（48 真实 / 1 no-op / 6 可选缺失 / 54 受限+理由）──
    api_rows = _platform_api_rows(host)
    real_rows = [row for row in api_rows if row["classification"] == "real-executable"]
    noop_rows = [row for row in api_rows if row["classification"] == "explicit-noop"]
    absent_rows = [row for row in api_rows if row["classification"] == "absent-optional-reasoned"]
    restricted_rows = [row for row in api_rows if row["classification"] == "restricted-reasoned"]
    silent_rows = [row for row in api_rows if row["classification"] == "restricted-reasoned" and not row["capabilityReason"]]
    conditional_rows = [row for row in api_rows if row.get("conditionalInjection")]
    # 不硬编码覆盖数（v3/T06 patch 后真实上限会继续上升）；只校验结构不变量 + v2 下限。
    producer.check("bridge::platform-api-real-executable",
                   len(real_rows) >= 48,
                   detail={"count": len(real_rows), "conditional": len(conditional_rows),
                           "members": [row["member"] for row in real_rows]})
    producer.check("bridge::platform-api-restricted-reasoned",
                   bool(restricted_rows) and all(row["capabilityReason"] for row in restricted_rows),
                   detail={"count": len(restricted_rows), "missingReason": [row["member"] for row in silent_rows],
                           "sample": [{"member": row["member"], "reason": row["capabilityReason"]}
                                      for row in restricted_rows[:8]]})
    producer.check("bridge::platform-api-no-silent-undefined",
                   not silent_rows and len(real_rows) + len(noop_rows) + len(absent_rows) + len(restricted_rows) == len(api_rows),
                   detail={"total": len(api_rows), "silent": [row["member"] for row in silent_rows],
                           "classifications": {"real": len(real_rows), "noop": len(noop_rows),
                                               "absentOptional": len(absent_rows),
                                               "restrictedReasoned": len(restricted_rows)},
                           "note": "显式受限成员不计入「真实可执行」；matrix 内 realExecutable=false 且附 reason"})
    context_menu = run_context_menu_behavior(producer)
    override_renderer_only_caps(model, host, sources, context_menu["ok"])
    host_core_expected = {"realExecutable": 76, "explicitNoop": 0,
                          "absentOptionalWithReason": 6, "restrictedWithReason": 27,
                          "silentUndefined": 0, "source": "HOST_V2_PATCHES §8.3 / Lead v3.1 clearance"}
    actual_summary = {"totalMembers": len(api_rows), "realExecutable": len(real_rows),
                      "conditionalInjection": len([row for row in api_rows if row.get("conditionalInjection")]),
                      "explicitNoop": len(noop_rows), "absentOptionalWithReason": len(absent_rows),
                      "restrictedWithReason": len(restricted_rows), "silentUndefined": len(silent_rows)}
    expectation_delta = {key: actual_summary[key] - value
                         for key, value in host_core_expected.items()
                         if key in actual_summary and isinstance(value, int)
                         and actual_summary[key] != value}
    platform_api_block = {
        "schema": "lingxi.r09.platform-api-matrix.v1",
        "hostCoreExpected": host_core_expected,
        "expectationDelta": expectation_delta,
        "source": ["desktop/src/react/types.ts", "desktop/src/react/host/tauri-desktop-host.ts",
                   "desktop/src/react/host/tauri-host-capabilities.ts"],
        "summary": {"totalMembers": len(api_rows), "realExecutable": len(real_rows),
                    "conditionalInjection": len([row for row in api_rows if row.get("conditionalInjection")]),
                    "explicitNoop": len(noop_rows), "absentOptionalWithReason": len(absent_rows),
                    "restrictedWithReason": len(restricted_rows), "silentUndefined": len(silent_rows)},
        "members": api_rows,
        "fragmentMerged": host["fragmentMerged"],
        "getFileUrlConvertFileSrc": host["getFileUrlConvert"],
        "contextMenuPolicy": {
            "rendererEquivalent": host["contextMenuPolicyInstalled"],
            "behaviorEvidence": context_menu,
            "rustResidual": "HOST_V2_PATCHES.md §2（release devtools=false / 无原生右键菜单）",
        },
    }

    # ── 73/71 命令矩阵 ────────────────────────────────────────────────
    command_rows = []
    for command in policy["surface"]:
        owner = COMMAND_CAPABILITY.get(command)
        windows = sorted(label for label, commands in policy["windows"].items() if command in commands)
        files = command_impl_files(command)
        row = {
            "command": command,
            "ownerCapability": owner,
            "windows": windows,
            "implementationFiles": files,
            "registeredIn": {
                "buildManifest": command in set(command_reg["buildCommands"]),
                "invokeHandler": command in set(command_reg["handlerRelease"]),
                "permissions": command in set(command_reg["permissionCommands"]),
                "capabilities": command in set(command_reg["capabilitySurface"]),
            },
            "runtimeAcl": "blocked_registered_by_T06",
            "evidence": ["r09_t06_capability_matrix (static PASS) / 运行期 ACL blocked"],
        }
        command_rows.append(row)
        producer.check(f"host::command::{command}",
                       row["registeredIn"]["buildManifest"] and row["registeredIn"]["invokeHandler"]
                       and row["registeredIn"]["permissions"] and row["registeredIn"]["capabilities"]
                       and bool(files) and owner is not None,
                       detail=row)
    producer.check("host::command-count-match-policy",
                   len(command_rows) == len(policy["surface"]),
                   detail={"rows": len(command_rows), "policySurface": len(policy["surface"])})

    # ── R00 入口遍历（37）───────────────────────────────────────────
    entry_rows = []
    for entry in sources["entrypoints"]["entries"]:
        category = entry.get("category")
        row = {
            "id": entry["id"],
            "category": category,
            "title": entry.get("title"),
            "r00Status": entry.get("status"),
            "hostRelevant": category in ("desktop_shell", "renderer_clients"),
        }
        if entry["id"] in DESKTOP_ENTRY_MAP:
            row.update(DESKTOP_ENTRY_MAP[entry["id"]])
        elif category == "desktop_shell":
            row["status"] = "missing_unregistered"
            row["reason"] = "R00 桌面入口未映射到 Tauri 等价实现"
        else:
            row["status"] = "non-host"
            row["reason"] = NON_HOST_ENTRY_REASONS.get(category, "非桌面宿主入口")
        entry_rows.append(row)
        valid = row["status"] in ("wired", "recorded", "pending_gui", "blocked", "deferred", "non-host")
        producer.check(f"host::entrypoint::{entry['id']}", valid,
                       detail={"status": row.get("status"), "reason": row.get("reason") or row.get("note")})
    producer.check("host::entrypoints-traversed-37", len(entry_rows) == 37, detail={"count": len(entry_rows)})
    desktop_entries = [row for row in entry_rows if row["hostRelevant"]]
    producer.check("host::desktop-entrypoints-mapped",
                   all(row.get("status") not in (None, "missing_unregistered") for row in desktop_entries),
                   detail={"entries": [{"id": row["id"], "status": row.get("status")} for row in desktop_entries]})

    # ── 74 叶矩阵 ─────────────────────────────────────────────────────
    leaf_rows = build_leaf_matrix(producer, book, sources)
    leaf_summary = {}
    for row in leaf_rows:
        leaf_summary[row["status"]] = leaf_summary.get(row["status"], 0) + 1
    producer.check("host::leaf-matrix-74", len(leaf_rows) == 74, detail={"count": len(leaf_rows)})
    producer.check("host::leaf-bound-count-15",
                   leaf_summary.get("bound", 0) == 15,
                   detail={"summary": leaf_summary, "ledger": sources["ledger"]["counts"]})
    producer.check("host::leaf-bound-cases-verified",
                   all(row["boundCasesVerified"] for row in leaf_rows if row["status"] == "bound"),
                   detail={"bad": [row["leafId"] for row in leaf_rows
                                   if row["status"] == "bound" and not row["boundCasesVerified"]]})
    producer.check("host::leaf-no-overclaim",
                   all(row["status"] != "bound" or row["boundCasesVerified"] for row in leaf_rows),
                   detail={"note": "只有 ledger BOUND 且逐断言 case 在证据中真实 PASS 的叶才算 bound"})
    registration_gaps = [row["leafId"] for row in leaf_rows
                         if row["status"] == "blocked" and not row.get("blockedRegistration")]
    producer.check("host::blocked-leaves-registered", not registration_gaps,
                   detail={"unregistered": registration_gaps})
    platform_blocked = set(sources["ledger"]["platformAvailability"])
    producer.check("host::platform-matrix-recorded",
                   sources["ledger"]["platformAvailability"].get("macos-arm64") == "AVAILABLE"
                   and all(sources["ledger"]["platformAvailability"].get(key) == "BLOCKED_PLATFORM"
                           for key in ("macos-x64", "windows-x64", "linux-x64")),
                   detail=sources["ledger"]["platformAvailability"])

    # ── 占位/空实现/隐藏按钮扫描 ─────────────────────────────────────
    scan = run_placeholder_scan(producer, sources, host, model, command_reg)
    producer.check("host::no-unregistered-placement-gaps",
                   not scan["unregisteredGaps"],
                   detail={"unregisteredGaps": scan["unregisteredGaps"]})

    # ── 能力行输出 + A16 判定 ────────────────────────────────────────
    capability_rows = []
    for cap_id in sorted(model):
        cap = model[cap_id]
        capability_rows.append({
            "id": cap_id,
            "group": cap["group"],
            "kind": cap["kind"],
            "electronImpl": cap["electronImpl"],
            "tauriTarget": cap["tauriTarget"],
            "implementation": {
                "modules": cap["modules"],
                "modulesPresent": cap["modulesPresent"],
                "commands": cap["commands"],
                "realCommandsRegistered": [
                    cmd for cmd in cap["commands"] if cmd in set(command_reg["release"])],
            },
            "realConsumer": {
                "inventory": cap["realConsumers"],
                "rendererApiMembers": cap["rendererMembers"],
                "rendererStatus": cap["rendererStatus"],
            },
            "permission": {
                "scope": cap["permissionScope"],
                "windowGrants": cap["windowPermissions"],
                "auditEvidence": "r09_t06_capability_matrix static caps::* / audit-05-freeze PASS",
            },
            "evidence": {
                "suites": cap["evidenceSuites"],
                "prefixes": cap["evidencePrefixes"],
                "cases": cap["evidenceCases"],
                "passed": cap.get("evidencePassed"),
                "checks": cap.get("evidenceChecks"),
            },
            "platform": {
                "macos-arm64": "LOCAL_VERIFIABLE",
                "macos-x64": "BLOCKED_PLATFORM",
                "windows-x64": "BLOCKED_PLATFORM",
                "linux-x64": "BLOCKED_PLATFORM",
            },
            "deferredRegistration": cap["deferredRegistration"],
            "blockedRegistration": cap["blockedRegistration"],
            "approvalId": cap["approvalId"],
            "evidenceMaturity": cap["evidenceMaturity"],
            "realBehaviorEvidence": cap["realBehaviorEvidence"],
            "gaps": cap["gaps"],
            "a16Ok": cap["a16Ok"],
            "notes": cap["notes"],
        })
        producer.check(f"host::capability::{cap_id}", cap["a16Ok"],
                       detail={"status": cap["status"], "kind": cap["kind"],
                               "rendererStatus": cap["rendererStatus"],
                               "implementationOk": cap["implementationOk"],
                               "evidencePassed": cap.get("evidencePassed"),
                               "gaps": cap["gaps"]})

    # ── 汇总 + matrix 文件 ───────────────────────────────────────────
    a16_failures = [row["id"] for row in capability_rows if not row["a16Ok"]]
    deferred_commands = deferred_command_names(sources)
    matrix = {
        "schema": "lingxi.r09.host-matrix.v1",
        "stage": "R09",
        "acceptance": "R09-A16",
        "suite": "r09_t08_host_completeness",
        "generatedAtUnixMs": None,
        "fixedFactors": fixed,
        "candidate": CANDIDATE_META,
        "sourceDrift": fixed.get("sourceDrift"),
        "sources": {
            "capabilityInventory": "docs/rust-tauri/R09/T03_HOST_CAPABILITY_INVENTORY.json (50 能力)",
            "commandPolicy": "docs/rust-tauri/R09/R09_COMMAND_POLICY.json (71 命令 / 8 窗口)",
            "leafLedger": "docs/rust-tauri/R09/R09_LEAF_LEDGER.json (74 叶 / 168 断言)",
            "r00Entrypoints": "docs/rust-tauri/R00/ENTRYPOINTS.json (37 入口)",
            "t03LeafPlan": "docs/rust-tauri/R09/T03_LEAF_CASE_PLAN.json (30 宿主叶 / 60 断言)",
            "t07LeafMapping": "docs/rust-tauri/R09/T07_LEAF_MAPPING.json (46 共享叶分类)",
            "guiBatch": "docs/rust-tauri/R09/T06_GUI_BATCH_NEEDS.json (25 项 GUI 缺口)",
        },
        "capabilities": capability_rows,
        "platformApi": platform_api_block,
        "commands": command_rows,
        "entrypoints": entry_rows,
        "leaves": leaf_rows,
        "placeholderScan": scan,
        "platformMatrix": sources["inventory"]["platformMatrix"],
        "summary": {
            "capabilities": {"total": len(capability_rows),
                             "a16Ok": len(capability_rows) - len(a16_failures),
                             "failed": a16_failures},
            "commands": {"total": len(command_rows),
                         "fourSetRegistered": sum(
                             1 for row in command_rows if all(row["registeredIn"].values())),
                         "runtimeAcl": "blocked_registered_by_T06"},
            "entrypoints": {"total": len(entry_rows),
                            "hostRelevant": len(desktop_entries),
                            "nonHost": len(entry_rows) - len(desktop_entries)},
            "leaves": {"total": len(leaf_rows), "byStatus": leaf_summary,
                       "originalAssertions": sources["ledger"]["counts"]["originalAssertions"]},
            "placeholders": {
                "unregisteredGaps": scan["unregisteredGaps"],
                "notImplementedRegistered": scan["notImplementedRegistered"],
                "notImplementedUnregistered": scan["notImplementedUnregistered"],
                "failClosed": scan["failClosed"],
                "deferredCommands": deferred_commands,
                "uiDeadEntrypoints": scan["uiDeadEntrypoints"],
            },
            "platform": sources["ledger"]["platformAvailability"],
        },
    }
    return matrix, scan


# ────────────────────────────────────────────────────────────────────────────
# 74 叶矩阵（A16 逐叶状态；不把未绑定叶写成 PASS）
# ────────────────────────────────────────────────────────────────────────────
def build_leaf_matrix(producer: Producer, book: EvidenceBook, sources: dict) -> list[dict]:
    scene = scene_by_id(sources)
    t03_plan = sources["t03Plan"]
    blocked_assertions_by_leaf = {}
    for item in t03_plan.get("blockedAssertions") or []:
        blocked_assertions_by_leaf.setdefault(item["leafId"], []).append(item)
    manual_assertions_by_leaf = {}
    for item in t03_plan.get("manualOnlyAssertions") or []:
        manual_assertions_by_leaf.setdefault(item["leafId"], []).append(item)
    t03_plan_leaves = scene["t03Leaves"]
    t07_mapping = scene["t07Leaves"]

    # T04 绑定计划（唯一机器源在 r09_t04.py；只读 import 常量，不执行生产者逻辑）
    import r09_t04 as t04_module  # noqa: E402
    t04_plan = t04_module.T04_BINDING_PLAN

    t05_book = book.books["r09_t05_browser_pdf"]
    t04_book = book.books["r09_t04_permission_matrix"]
    t03_book = book.books["r09_t03_native_matrix"]

    rows = []
    for leaf in sources["ledger"]["leaves"]:
        leaf_id = leaf["id"]
        planned_suite = leaf.get("plannedSuite")
        row = {
            "leafId": leaf_id,
            "featureId": leaf.get("featureId"),
            "taskIds": leaf.get("taskIds"),
            "executionTaskIds": leaf.get("executionTaskIds"),
            "assertionCount": len(leaf.get("assertions") or []),
            "originalAssertions": [{"index": item.get("index"), "text": item.get("text"),
                                    "caseStatus": item.get("caseStatus"), "cases": item.get("cases")}
                                   for item in (leaf.get("assertions") or [])],
            "implementationEntry": leaf.get("implementationEntry"),
            "realConsumer": leaf.get("realConsumer"),
            "plannedSuite": planned_suite,
            "platformScope": leaf.get("platformScope"),
            "platformStatus": leaf.get("platformStatus"),
            "status": None,
            "boundCases": [],
            "boundCasesVerified": False,
            "blockedRegistration": None,
            "manualSteps": None,
            "note": None,
        }
        if leaf.get("status") == "EVIDENCED":
            bound_cases = [case for assertion in (leaf.get("assertions") or [])
                           for case in (assertion.get("cases") or [])]
            row["boundCases"] = bound_cases
            row["boundCasesVerified"] = bool(bound_cases) and all(
                t05_book["caseMap"].get(case) is True for case in bound_cases)
            row["status"] = "bound"
            row["note"] = "ledger EVIDENCED；逐断言 case 在 T05 证据中 PASS"
        elif planned_suite == "r09_t05_browser_pdf":
            row["status"] = "blocked"
            row["blockedRegistration"] = {
                "basis": ["docs/rust-tauri/R09/T05_PROGRESS.md",
                          "docs/rust-tauri/R09/T07_LEAF_MAPPING.json"],
                "reason": "viewer/设置页/文件预览宿主接线（T03/T06/T07），服务侧断言以外无宿主证据",
            }
            row["note"] = "T05_BUILDING：4 叶 blocked（T05_PROGRESS §6 逐叶 BLOCKED）"
        elif planned_suite == "r09_t04_permission_matrix":
            plan = t04_plan.get(leaf_id) or {}
            status = plan.get("status")
            if status == "bindable":
                cases = [case for group in plan.get("assertions", []) for case in group]
                row["boundCases"] = cases
                row["boundCasesVerified"] = bool(cases) and all(
                    t04_book["caseMap"].get(case) is True for case in cases)
                row["status"] = "evidence_green_unbound"
                row["note"] = "T04 producer case 全 PASS，但 Lead 尚未登记 FULL_BINDINGS（不得算 bound）"
            elif status == "manual":
                row["status"] = "manual_pending"
                row["manualSteps"] = plan.get("manualSteps")
                row["blockedRegistration"] = {"basis": ["docs/rust-tauri/R09/BLOCKED.md"],
                                              "reason": "BLOCKED-SYSTEM-PERMISSIONS"}
            else:
                row["status"] = "blocked"
                row["blockedRegistration"] = {
                    "basis": ["docs/rust-tauri/R09/T03_T04_PROGRESS.md", "docs/rust-tauri/R09/BLOCKED.md"],
                    "reason": plan.get("blockedReason"),
                }
            row["note"] = f"T04_BINDING_PLAN 状态={status}"
        elif planned_suite == "r09_t03_native_matrix":
            mapping = t07_mapping.get(leaf_id) or {}
            host_class = mapping.get("hostClass")
            row["hostClass"] = host_class
            row["classification"] = mapping.get("classification")
            plan = t03_plan_leaves.get(leaf_id)
            planned_cases = [item.get("plannedActionCase") for item in (plan or {}).get("assertions", [])
                             if item.get("plannedActionCase")]
            row["plannedActionCases"] = planned_cases
            row["plannedCasesObserved"] = sum(1 for case in planned_cases
                                              if t03_book["caseMap"].get(case) is True)
            if host_class not in HOST_LEAF_CLASSES:
                row["status"] = "excluded_non_host"
                row["note"] = "SERVER_REACT（16 叶）：宿主无新增命令；T07 原生 WebView E2E 覆盖"
            elif blocked_assertions_by_leaf.get(leaf_id):
                row["status"] = "blocked"
                row["blockedRegistration"] = {
                    "basis": ["docs/rust-tauri/R09/T03_T04_PROGRESS.md",
                              "docs/rust-tauri/R09/T03_LEAF_CASE_PLAN.json"],
                    "reason": "; ".join(item.get("reason") for item in blocked_assertions_by_leaf[leaf_id]),
                }
                row["note"] = "T03 blocked 断言（服务路由/命令缺口），已登记 T03_T04_PROGRESS §6"
            elif manual_assertions_by_leaf.get(leaf_id):
                row["status"] = "manual_pending"
                row["manualSteps"] = [item.get("case") for item in manual_assertions_by_leaf[leaf_id]]
                row["blockedRegistration"] = {
                    "basis": ["docs/rust-tauri/R09/T06_GUI_BATCH_NEEDS.json",
                              "docs/rust-tauri/R09/T03_LEAF_CASE_PLAN.json"],
                    "reason": "manual-observation 证据槽（GUI/系统动作）",
                }
                row["note"] = "计划含人工断言；其余断言已实现但真实动作证据 pending"
            else:
                row["status"] = "pending_gui"
                row["blockedRegistration"] = {
                    "basis": ["docs/rust-tauri/R09/T06_GUI_BATCH_NEEDS.json",
                              "docs/rust-tauri/R09/T07_PROGRESS.md"],
                    "reason": "plannedActionCase 需 GUI 真实动作（hidden/visible clearance 批次）",
                }
                row["note"] = "plannedActionCase 未执行（逻辑单测不算）；A05/A16 真实动作证据 pending"
        else:
            row["status"] = "unclassified"
            row["note"] = f"未知 plannedSuite={planned_suite}"
        rows.append(row)
    return rows


# ────────────────────────────────────────────────────────────────────────────
# 占位/空实现/隐藏按钮扫描
# ────────────────────────────────────────────────────────────────────────────
def run_placeholder_scan(producer: Producer, sources: dict, host: dict, model: dict,
                         command_reg: dict) -> dict:
    not_implemented_occurrences = []
    for path in sorted(TAURI_SRC.rglob("*.rs")):
        text = read_text(path)
        test_ranges = cfg_test_ranges(text)
        for match in re.finditer(r"NOT_IMPLEMENTED|todo!\(|unimplemented!\(", text):
            if in_ranges(match.start(), test_ranges):
                continue
            line_no = text.count("\n", 0, match.start()) + 1
            line = text.splitlines()[line_no - 1].strip()
            not_implemented_occurrences.append({
                "file": rel(path), "line": line_no, "token": match.group(0), "text": line[:220],
            })
    registered = []
    unregistered = []
    fail_closed = []
    for occurrence in not_implemented_occurrences:
        file = occurrence["file"]
        token = occurrence["token"]
        if file.endswith("native/errors.rs"):
            occurrence["category"] = "constant-definition"
            registered.append(occurrence)
        elif file.endswith("native/ipc_negative_matrix.rs"):
            occurrence["category"] = "negative-test-matrix"
            registered.append(occurrence)
        elif file.endswith("native/tray.rs"):
            occurrence["category"] = "fail-closed (icon missing → explicit deny, no silent continue)"
            fail_closed.append(occurrence)
        elif file.endswith("native/screenshot.rs"):
            check = ("T04-screenshot" in sources["t06ProgressMd"]
                     and "screenshot_render" in sources["t06ProgressMd"])
            occurrence["category"] = "deferred-command-policy (T04 OQ-04)"
            (registered if check else unregistered).append(occurrence)
        elif file.endswith("native/onboarding.rs"):
            registered_blocked = ("T06-ONBOARD-01" in sources["blockedMd"]
                                  or "T03-ONBOARD-01" in sources["blockedMd"])
            registered_progress = (("T06-ONBOARD-01" in sources["t06ProgressMd"])
                                   or ("T03-ONBOARD-01" in sources["t06ProgressMd"]))
            check = registered_blocked or registered_progress
            occurrence["category"] = ("registered-blocked (T06-ONBOARD-01，与 Electron Rust 模式一致)"
                                      if check else
                                      "unregistered-in-required-lists (仅在 T03_T04_PROGRESS §6)")
            occurrence["registrationChecked"] = {
                "BLOCKED.md": registered_blocked,
                "T06_PROGRESS.md": registered_progress,
            }
            (registered if check else unregistered).append(occurrence)
        else:
            occurrence["category"] = "unclassified"
            unregistered.append(occurrence)
        _ = token

    # 空实现：命令函数体 trivial 扫描
    trivial_commands = []
    for command in command_reg["release"]:
        bodies = []
        for path in TAURI_SRC.rglob("*.rs"):
            text = read_text(path)
            test_ranges = cfg_test_ranges(text)
            for match in re.finditer(r"fn\s+" + re.escape(command) + r"\s*\(", text):
                if in_ranges(match.start(), test_ranges):
                    continue
                bodies.append(text[match.start(): match.start() + 2000])
        if not bodies:
            trivial_commands.append({"command": command, "reason": "no-fn"})
            continue
        for body in bodies:
            brace = body.find("{")
            inner = body[brace + 1:] if brace >= 0 else body
            stripped = re.sub(r"\s+", " ", inner).strip()
            if stripped in ("}", "Ok(())}", "Ok(()) }"):
                trivial_commands.append({"command": command, "reason": "empty-body"})
    producer.check("scan::no-trivial-command-bodies", not trivial_commands,
                   detail={"trivial": trivial_commands[:10]})

    # 宿主显式 no-op / 缺失成员必须有理由（不得静默降级）
    noop_without_reason = [name for name in host["noops"] if name not in host["noopReasons"]]
    absent_without_reason = [name for name in host["absentMembers"] if name not in host["noopReasons"]]
    producer.check("scan::host-noops-have-reasons", not noop_without_reason,
                   detail={"missingReason": noop_without_reason})
    producer.check("scan::absent-members-have-reasons", not absent_without_reason,
                   detail={"missingReason": absent_without_reason})

    # 隐藏按钮：渲染层消费但 Tauri 未接线/无理由的成员
    wired = set(host["wired"]) | set(host["absentMembers"])
    ui_dead = []
    for member, files in sorted(host["rendererUsed"].items()):
        state = member_status(member, host)
        if state in ("fragment-only", "unwired", "fragment-reason-only"):
            owner_caps = [cap_id for cap_id, cap in model.items()
                          if member in [m.replace(" (preload-only)", "") for m in cap["apiMembers"]]]
            ui_dead.append({
                "member": member,
                "state": state,
                "rendererFiles": files[:8],
                "ownerCapabilities": owner_caps,
                "registeredBlocked": False,
            })
    _ = wired
    producer.check("scan::ui-entrypoints-wired", not ui_dead,
                   detail={"deadEntrypoints": len(ui_dead),
                           "sample": [{"member": item["member"], "state": item["state"],
                                       "files": item["rendererFiles"][:3]} for item in ui_dead[:12]]})

    # 已登记 deferred 命令不得被 grant / 不得出现在 release 命令面
    deferred_commands = deferred_command_names(sources)
    deferred_granted = sorted(set(deferred_commands) & set(command_reg["capabilitySurface"]))
    deferred_in_release = sorted(set(deferred_commands) & set(command_reg["release"]))
    producer.check("scan::deferred-commands-not-granted",
                   not deferred_granted and not deferred_in_release,
                   detail={"deferred": len(deferred_commands), "granted": deferred_granted,
                           "inRelease": deferred_in_release})

    # 隐藏按钮接线是否有 capabilityReason 消费（桌面 renderer 不使用 → UI 不会降级显示）
    capability_reason_consumers = []
    for path in (ROOT / "desktop/src").rglob("*.ts*"):
        if not path.is_file() or "/react/host/" in str(path) or "/__tests__/" in str(path):
            continue
        text = read_text(path)
        if "capabilityReason" in text:
            capability_reason_consumers.append(rel(path))
    producer.check("scan::ui-capability-reason-consumers",
                   bool(capability_reason_consumers),
                   detail={"consumers": capability_reason_consumers,
                           "note": "桌面 renderer 未消费 capabilityReason；受限成员只能运行期抛错"})

    unregistered_gaps = []
    for occurrence in unregistered:
        unregistered_gaps.append({
            "kind": "NOT_IMPLEMENTED-unregistered",
            "file": occurrence["file"], "line": occurrence["line"],
            "category": occurrence["category"],
            "requiredRegistration": "BLOCKED.md / T06_PROGRESS deferred·blocked entry",
        })
    for row_member in ui_dead:
        if not row_member["registeredBlocked"]:
            unregistered_gaps.append({
                "kind": "ui-entrypoint-not-wired",
                "member": row_member["member"],
                "state": row_member["state"],
                "ownerCapabilities": row_member["ownerCapabilities"],
                "requiredRegistration": "BLOCKED.md / T06_PROGRESS（或合并能力片段并重建候选）",
            })
    for cap_id, cap in model.items():
        if cap["status"] == "missing_unregistered":
            unregistered_gaps.append({
                "kind": "capability-unimplemented-unregistered",
                "capability": cap_id,
                "requiredRegistration": "BLOCKED.md / T06_PROGRESS deferred·blocked entry",
            })

    return {
        "scannedAtUnixMs": None,
        "notImplementedOccurrences": not_implemented_occurrences,
        "notImplementedRegistered": registered,
        "notImplementedUnregistered": unregistered,
        "failClosed": fail_closed,
        "trivialCommands": trivial_commands,
        "noopWithoutReason": noop_without_reason,
        "absentWithoutReason": absent_without_reason,
        "uiDeadEntrypoints": ui_dead,
        "capabilityReasonConsumers": capability_reason_consumers,
        "deferredCommands": deferred_commands,
        "unregisteredGaps": unregistered_gaps,
        "summary": {
            "notImplemented": len(not_implemented_occurrences),
            "registered": len(registered),
            "unregistered": len(unregistered),
            "failClosed": len(fail_closed),
            "uiDeadEntrypoints": len(ui_dead),
            "unregisteredGaps": len(unregistered_gaps),
        },
    }


# ════════════════════════════════════════════════════════════════════════════
# 套件执行入口
# ════════════════════════════════════════════════════════════════════════════
def _write_json(path: Path, payload) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")


def _write_cases_copy(producer: Producer, filename: str) -> None:
    payload = {
        "schema": "lingxi.leaf-case-results.v1",
        "suite": producer.suite,
        "cases": producer.cases,
    }
    _write_json(producer.evidence / filename, payload)


def run_shell_parity(suite: str, evidence: Path) -> int:
    producer = Producer(suite, evidence)
    try:
        import time
        sources = load_sources()
        fixed = verify_fixed_factors(producer)
        book = EvidenceBook(evidence)
        # 实测 A/B 行的上游证据
        for upstream in ("r09_t01_react_parity", "r09_t02_supervisor_ready",
                         "r09_t02_process_ownership", "r09_t05_browser_pdf",
                         "r09_t05_untrusted_preview", "r09_t07_native_e2e"):
            book.load(producer, upstream)
        load_supporting_books(producer, book)
        drift = check_candidate_drift(producer, fixed)
        check_evidence_candidate_binding(producer, book)
        matrix = build_a15_matrix(producer, book, sources, fixed)
        book.require_upstream_pass(
            producer,
            ["r09_t01_react_parity", "r09_t02_supervisor_ready", "r09_t02_process_ownership",
             "r09_t05_browser_pdf", "r09_t05_untrusted_preview", "r09_t07_native_e2e"],
            "a15-measured",
        )
        # 差异登记：矩阵里出现 recorded 的行必须引用已知批准
        recorded_rows = [row for row in matrix["capabilityRows"] if row["status"] == "recorded"]
        unknown_recorded = [row["id"] for row in recorded_rows
                            if row["evidence"].get("approvalId") not in {a["id"] for a in RECORDED_APPROVALS}]
        producer.check("a15::recorded-rows-reference-approvals", not unknown_recorded,
                       detail={"unknown": unknown_recorded})
        results = _a15_fill_shell_parity_results(matrix, fixed)
        results["generatedAt"] = time.strftime("%Y-%m-%dT%H:%M:%S%z")
        results["provenance"] = {suite_name: book.books[suite_name]["sourceKind"]
                                 for suite_name in book.books}
        _write_json(evidence / "parity-matrix.json", matrix)
        _write_json(evidence / f"SHELL_PARITY_RESULTS.{suite}.json", results)
        producer.assert_real_home_untouched()
        _write_cases_copy(producer, "shell-parity-cases.json")
        summary = matrix["summary"]
        extra = {
            "outputs": {
                "matrix": "parity-matrix.json",
                "shellParityResultsDraft": f"SHELL_PARITY_RESULTS.{suite}.json",
                "casesCopy": "shell-parity-cases.json",
            },
            "fixedHashes": results["fixedHashes"],
            "paritySummary": {
                "measured": summary["measured"],
                "capabilities": summary["capabilities"],
                "leaves": summary["leaves"],
                "pendingCount": summary["pending"],
                "blockedCount": summary["blocked"],
                "unapprovedDifferences": summary["unapprovedDifferences"],
            },
            "evidenceProvenance": book.provenance(),
            "candidate": CANDIDATE_META,
            "sourceDrift": drift,
            "note": "A15 在 pending/blocked 清零前不得宣称对照成立；本回执如实红。",
        }
        return producer.finish(status="PASS", extra=extra)
    except Exception as exc:  # noqa: BLE001 - 生产者失败必须写回执，不能静默
        traceback.print_exc()
        return producer.fail(f"{type(exc).__name__}: {exc}")


def run_host_completeness(suite: str, evidence: Path) -> int:
    producer = Producer(suite, evidence)
    try:
        import time
        sources = load_sources()
        fixed = verify_fixed_factors(producer)
        book = EvidenceBook(evidence)
        load_supporting_books(producer, book)
        drift = check_candidate_drift(producer, fixed)
        check_evidence_candidate_binding(producer, book)
        matrix, scan = build_a16_matrix(producer, book, sources, fixed)
        matrix["generatedAtUnixMs"] = int(time.time() * 1000)
        scan["scannedAtUnixMs"] = int(time.time() * 1000)
        _write_json(evidence / "host-matrix.json", matrix)
        _write_json(evidence / "placeholder-scan.json", scan)
        producer.assert_real_home_untouched()
        _write_cases_copy(producer, "host-cases.json")
        summary = matrix["summary"]
        extra = {
            "outputs": {
                "hostMatrix": "host-matrix.json",
                "placeholderScan": "placeholder-scan.json",
                "casesCopy": "host-cases.json",
            },
            "hostSummary": summary,
            "evidenceProvenance": book.provenance(),
            "candidate": CANDIDATE_META,
            "sourceDrift": drift,
            "note": "A16 真实缺口必须保持红；能力/命令/入口/叶级明细见 host-matrix.json。",
        }
        return producer.finish(status="PASS", extra=extra)
    except Exception as exc:  # noqa: BLE001
        traceback.print_exc()
        return producer.fail(f"{type(exc).__name__}: {exc}")


SUITES = {
    "r09_t08_shell_parity": run_shell_parity,
    "r09_t08_host_completeness": run_host_completeness,
}


def run_suite(suite: str, evidence: Path) -> int:
    handler = SUITES.get(suite)
    if handler is None:
        producer = Producer(suite, evidence)
        return producer.fail(f"套件 {suite} 不属于 R09-T08 生产者 {sorted(SUITES)}")
    return handler(suite, Path(evidence))


def main() -> int:
    argv = sys.argv
    suite = argv[argv.index("--suite") + 1] if "--suite" in argv else None
    evidence = argv[argv.index("--evidence") + 1] if "--evidence" in argv else None
    if not suite or not evidence:
        print("usage: r09_t08.py --suite <name> --evidence <dir>", file=sys.stderr)
        return 64
    return run_suite(suite, Path(evidence))


if __name__ == "__main__":
    sys.exit(main())

