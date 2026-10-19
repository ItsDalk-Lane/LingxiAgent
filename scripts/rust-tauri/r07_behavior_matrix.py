#!/usr/bin/env python3
"""R07-A23 真实行为矩阵双向映射生产者：要求→入口/存储/测试/结果，以及
实际注册路由→要求或有据边界。缺口必须显式标 BLOCKED，不得静默留白。
报告完整且缺口全部显式登记时退出 0；生产者自身失败或出现未标缺口退出 1。"""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path

ROOT = Path(os.environ.get("LINGXI_ROOT", Path(__file__).resolve().parents[2]))
SERVICE_SRC = ROOT / "rust/crates/lingxi-service/src"

# 基础设施面：不对应功能叶，但属有据边界（握手/传输/静态托管/健康）。
INFRASTRUCTURE_PREFIXES = (
    "/lingxi/v1/health", "/lingxi/v1/ws", "/lingxi/v1/ws-ticket",
    "/lingxi/v1/devices/", "/lingxi/v1/auth", "/mobile", "/desktop",
    "/lingxi/v1/server-info",
)


def out_path(argv: list[str]) -> Path:
    if "--out" not in argv:
        raise SystemExit("--out <dir> required")
    target = Path(argv[argv.index("--out") + 1])
    target.mkdir(parents=True, exist_ok=True)
    return target


def rust_routes() -> dict[str, str]:
    """静态提取服务注册路由及其注册来源文件；来源只限真实源码，不抄清单文档。
    cfg(test) 段是测试夹具服务器（如 background_tasks 的供应商桩），剥掉后扫描。"""
    routes: dict[str, str] = {}
    pattern = re.compile(r'\.route\("(/[^"]*)"')
    for path in SERVICE_SRC.glob("*.rs"):
        text = path.read_text(encoding="utf-8")
        cut = text.find("#[cfg(test)]")
        if cut != -1:
            text = text[:cut]
        for match in pattern.finditer(text):
            routes.setdefault(match.group(1), path.name)
    return dict(sorted(routes.items()))


# 注册来源模块 → 责任任务（模块名即任务面；lib.rs 为跨域主干，单独列出）。
MODULE_TASK = {
    "bridge_routes.rs": "R07-T02", "bridge_session_routes.rs": "R07-T02",
    "browser_routes.rs": "R07-T04", "office_pdf.rs": "R07-T05",
    "beautify_run.rs": "R07-T05", "beautify_generation.rs": "R07-T05",
    "desk_files.rs": "R07-T05", "media_admin.rs": "R07-T06",
    "media_tasks.rs": "R07-T06", "workbench.rs": "R07-T07",
    "workflow.rs": "R07-T07", "git_panel.rs": "R07-T07",
    "mcp_settings.rs": "R07-T08", "mcp_session_permissions.rs": "R07-T08",
    "provider_routes.rs": "R07-T08", "settings_configuration.rs": "R07-T08",
    "client_context.rs": "R07-T09", "observability.rs": "R07-T10",
    "management.rs": "R07-T08", "conversation_routes.rs": "R07-T03",
    "office_routes.rs": "R07-T05", "diary.rs": "R07-T07",
    "mcp_apps.rs": "R07-T08", "mcp_oauth.rs": "R07-T08",
    "agent_deletions.rs": "R07-T08", "agents_admin.rs": "R07-T08",
    "background_tasks.rs": "R07-T01", "automation.rs": "R07-T01",
    "knowledge.rs": "R06(知识面属 R06 责任，R07 不重认领)",
    "media_generate.rs": "R07-T06", "media_routes.rs": "R07-T06",
    "todos.rs": "R06(待办面板属 R06 责任，R07 不重认领)",
}

# lib.rs 主干路由按路径前缀归因（跨阶段责任如实标注，不冒充 R07 所有）。
PREFIX_TASK = {
    "/lingxi/v1/bridge/": "R07-T02", "/lingxi/v1/browser-host": "R07-T04",
    "/lingxi/v1/desk/": "R07-T05", "/lingxi/v1/checkpoints": "R07-T07",
    "/lingxi/v1/sessions/todos": "R06(待办面板)",
    "/lingxi/v1/sessions": "R06(会话管理主干)",
    "/lingxi/v1/session-projects": "R06(会话项目目录)",
    "/lingxi/v1/models": "R06(模型管理主干)",
    "/lingxi/v1/skills": "R06(技能主干)",
    "/lingxi/v1/fs/": "R06(文件读写)",
    "/lingxi/v1/resource-io/": "R06(资源 IO)",
    "/lingxi/v1/search/": "R06(搜索)",
    "/lingxi/v1/me": "基础设施(身份握手)",
    "/lingxi/v1/app-events": "R06(事件总线)",
}


def load(path: str):
    return json.loads((ROOT / path).read_text(encoding="utf-8"))


def main() -> int:
    target = out_path(sys.argv)
    routes = rust_routes()
    inventory = load("docs/rust-tauri/R00/FEATURE_INVENTORY.json")["features"]
    acceptance = load("docs/rust-tauri/R00/ACCEPTANCE_MAP.json")["scenarios"]
    parity = load("docs/rust-tauri/R07/FEATURE_PARITY_RESULTS.json")

    r07_features = [f for f in inventory if "R07" in (f.get("stage_id") or [])]
    leaves = {k: v for k, v in acceptance.items()
              if v.get("kind") == "supplemental" and "R07" in (v.get("execution_stage_ids") or [])}
    base = {k: v for k, v in acceptance.items()
            if v.get("kind") == "base" and v.get("stage_id") == "R07"}

    # 叶证据草案（T07—T10）提供测试/结果绑定；缺草案不等于缺实现，如实标注。
    drafts = {}
    for name in ("T07", "T08", "T09", "T10"):
        path = ROOT / f"docs/rust-tauri/R07/{name}_LEAF_EVIDENCE_DRAFT.json"
        if path.is_file():
            data = json.loads(path.read_text(encoding="utf-8"))
            for leaf_id, row in (data.get("leaves") or {}).items():
                drafts.setdefault(leaf_id, []).append(
                    {"task": name, "draftResult": row.get("draftResult"),
                     "proofs": row.get("directCurrentProof") or []})

    feature_by_id = {f["feature_id"]: f for f in r07_features}
    parity_leaves = parity["scenarioResults"]

    leaf_rows = []
    holes = []
    for leaf_id in sorted(leaves):
        leaf = leaves[leaf_id]
        feature = feature_by_id.get(leaf["feature_id"])
        entry = (feature or {}).get("current_entrypoints") or []
        stores = (feature or {}).get("current_stores") or []
        proof = drafts.get(leaf_id, [])
        result = parity_leaves.get(leaf_id, {}).get("status", "NOT_RUN")
        row = {
            "id": leaf_id,
            "featureId": leaf["feature_id"],
            "requirement": leaf["requirement"],
            "entrypoints": entry,
            "stores": stores,
            "testEvidence": proof,
            "currentResult": result,
        }
        gaps = []
        if not entry:
            gaps.append("entry")
        if not proof:
            gaps.append("test")
        if result == "NOT_RUN":
            gaps.append("result")
        if gaps:
            row["status"] = "BLOCKED"
            row["missing"] = gaps
            holes.append(leaf_id)
        else:
            row["status"] = "MAPPED"
        leaf_rows.append(row)

    # 反向：每条注册路由 → 注册来源模块 → 责任任务；基础设施面单列有据边界。
    route_rows = []
    orphan_routes = []
    for route, source in routes.items():
        boundary = next((p for p in INFRASTRUCTURE_PREFIXES if route.startswith(p)), None)
        if boundary:
            row = {"route": route, "source": source,
                   "binding": "INFRASTRUCTURE_BOUNDARY", "note": boundary}
        elif source in MODULE_TASK:
            row = {"route": route, "source": source,
                   "binding": "TASK_MODULE", "taskId": MODULE_TASK[source]}
        else:
            prefix = next((t for p, t in PREFIX_TASK.items() if route.startswith(p)), None)
            if prefix:
                row = {"route": route, "source": source,
                       "binding": "TASK_MODULE", "taskId": prefix}
            else:
                row = {"route": route, "source": source, "binding": "UNMAPPED"}
                orphan_routes.append(route)
        route_rows.append(row)

    report = {
        "schema": "lingxi.r07.bidirectional-map.v1",
        "stage": "R07",
        "generatedBy": "scripts/rust-tauri/r07_behavior_matrix.py",
        "baseScenarios": len(base),
        "leafCount": len(leaf_rows),
        "leaves": leaf_rows,
        "routes": route_rows,
        "holes": {"leavesWithGaps": holes, "unmappedRoutes": orphan_routes},
        "note": "缺口显式 BLOCKED 登记即满足 A23 的矩阵无静默空洞要求；"
                "缺口闭合由各叶逐断言证据推进，不以本报告存在冒充完成。",
    }
    (target / "R07_BIDIRECTIONAL_MAP.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"A23 map: {len(leaf_rows)} leaves ({len(holes)} with explicit gaps), "
          f"{len(routes)} routes ({len(orphan_routes)} unmapped)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
