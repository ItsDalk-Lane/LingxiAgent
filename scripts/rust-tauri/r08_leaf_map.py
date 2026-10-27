#!/usr/bin/env python3
"""从原验收账生成 R08 准备地图；登记责任不代表实现或验收通过。"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import subprocess


ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / "docs/rust-tauri/R08/R08_LEAF_MAP.json"
SOURCES = {
    "acceptanceMap": "docs/rust-tauri/R00/ACCEPTANCE_MAP.json",
    "featureStageAcceptance": "docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json",
    "features": "docs/rust-tauri/R00/FEATURE_INVENTORY.json",
    "entrypoints": "docs/rust-tauri/R00/ENTRYPOINTS.json",
    "stores": "docs/rust-tauri/R00/STORES.json",
    "apiCompatibility": "docs/rust-tauri/R01/API_COMPAT_MATRIX.json",
    "dataCompatibility": "docs/rust-tauri/R01/DATA_COMPATIBILITY_MATRIX.json",
    "rustStorageRegistry": "docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json",
    "r07Bindings": "docs/rust-tauri/R07/R07_LEAF_BINDINGS.json",
    "r07Handoff": "docs/rust-tauri/R07/R07_HANDOFF.json",
    "taskCatalog": "Lingxi_Rust_Tauri_Taskbooks_2026-09-23/task-catalog.json",
    "acceptanceCatalog": "Lingxi_Rust_Tauri_Taskbooks_2026-09-23/acceptance-catalog.json",
}

# 这些是已存在、可供接线调查的测试；不声称它们证明 R08 客户端结果。
CANDIDATES = {
    "D01": [
        "rust/crates/lingxi-service/tests/r06_chat_wire.rs",
        "desktop/src/react/__tests__/services/rust-chat-history.test.ts",
        "desktop/src/react/__tests__/stores/chat-slice.test.ts",
    ],
    "D02": [
        "rust/crates/lingxi-service/tests/r06_original_session_proofs.rs",
        "rust/crates/lingxi-service/tests/r06_t04_history.rs",
        "desktop/src/react/__tests__/stores/session-actions.test.ts",
    ],
    "D03": [
        "rust/crates/lingxi-service/tests/r07_t08_mcp_settings.rs",
        "desktop/src/react/__tests__/services/r07-mcp-oauth-real-service.test.tsx",
        "desktop/src/react/__tests__/components/rust-mcp-session-permissions.test.tsx",
    ],
    "D04": [
        "rust/crates/lingxi-service/tests/r07_t08_mcp_session_permissions.rs",
        "desktop/src/react/__tests__/services/server-connection.test.ts",
        "desktop/src/react/__tests__/components/rust-mcp-session-permissions.test.tsx",
    ],
    "D13": [
        "desktop/src/react/__tests__/services/terminal-client.test.ts",
        "desktop/src/react/__tests__/services/terminal-ws-message-handler.test.ts",
        "desktop/src/react/__tests__/services/terminal-output-stream.test.ts",
    ],
    "D20": ["tests/cli-rust-service.test.ts"],
}
OWNERS = {
    "D01": ["rust/crates/lingxi-service/src/ws.rs", "rust/crates/lingxi-service/src/runs.rs", "desktop/src/react/services/chat-client.ts", "desktop/src/react/services/rust-chat-history.ts"],
    "D02": ["rust/crates/lingxi-service/src/conversations.rs", "rust/crates/lingxi-service/src/history.rs", "rust/crates/lingxi-service/src/session_tree.rs", "desktop/src/react/services/rust-chat-history.ts"],
    "D03": ["rust/crates/lingxi-service/src/mcp_http.rs", "rust/crates/lingxi-service/src/mcp_settings.rs", "desktop/src/react/services/mcp-session-permissions.ts"],
    "D04": ["rust/crates/lingxi-service/src/approval.rs", "rust/crates/lingxi-service/src/mcp_session_permissions.rs", "desktop/src/react/services/server-connection.ts"],
    "D13": ["rust/crates/lingxi-service/src/ws.rs", "rust/crates/lingxi-service/src/exectools.rs", "desktop/src/react/services/terminal-client.ts"],
    "D20": ["cli/entry.ts", "rust/crates/lingxi-service/src/epoch.rs"],
}


def load(name):
    return json.loads((ROOT / SOURCES[name]).read_text())


def sha(path):
    return hashlib.sha256((ROOT / path).read_bytes()).hexdigest()


def test_inventory(path):
    source = (ROOT / path).read_text()
    if path.endswith(".rs"):
        names = re.findall(r"#\[(?:tokio::)?test(?:\([^\]]*\))?\]\s*(?:async\s+)?fn\s+(\w+)", source)
    else:
        names = re.findall(r"\b(?:it|test)\(\s*['\"]([^'\"\n]+)['\"]", source)
        names += re.findall(r"\b(?:it|test)\.skipIf\([^\n]*\)\(\s*['\"]([^'\"\n]+)['\"]", source)
    assert names, f"候选测试无具名测试，必须先人工核对：{path}"
    return {"path": path, "sha256": sha(path), "testNames": names,
            "conditionalExecutionLines": [line.strip() for line in source.splitlines()
                                          if re.search(r"\b(?:it|test)\.(?:skip|skipIf|runIf)|#\[ignore", line)],
            "relation": "调查候选，尚未逐断言证明 R08 覆盖", "executedForR08": False}


def generate():
    original = load("acceptanceMap")["scenarios"]
    scenarios = {key: value for key, value in original.items()
                 if value.get("requirement") == "REQUIRED_SUPPLEMENTAL"
                 and "R08" in value.get("execution_stage_ids", [])}
    specs = {row["id"]: row for row in load("featureStageAcceptance")["supplemental_scenarios"]
             if "R08" in row.get("execution_stage_ids", [])}
    features = {row["feature_id"]: row for row in load("features")["features"]}
    bindings = load("r07Bindings")["leaves"]
    base = [row for row in load("acceptanceCatalog")["scenarios"] if row["stage_id"] == "R08"]
    tasks = [row for row in load("taskCatalog")["tasks"] if row["stage_id"] == "R08"]
    later = {key for key, value in bindings.items() if value.get("laterShare", "").startswith("R08 ")}
    assert len(tasks) == 8 and len(base) == 16
    assert set(specs) == set(scenarios) and len(scenarios) == 126
    assert len(later) == 27 and later <= set(scenarios)
    leaves = []
    for leaf_id, scenario in scenarios.items():
        spec, feature = specs[leaf_id], features[scenario["feature_id"]]
        assert scenario["then"] == spec["then"]
        assert feature["feature_id"] == spec["feature_id"]
        explicit = [task for task in spec["execution_task_ids"] if task.startswith("R08-")]
        primary = explicit[0] if explicit else "R08-T07"
        secondary = ["R08-T01"]
        if primary == "R08-T07":
            secondary.append("R08-T02")
        if feature["parent_domain"] in ("D01", "D02", "D20"):
            secondary.extend(["R08-T04", "R08-T06"])
        background = bindings.get(leaf_id)
        cases = []
        for index, assertion in enumerate(spec["assertions"]):
            case_id = f"r08_leaf::{leaf_id}::assertion_{index:03d}"
            cases.append({
                "assertionIndex": index,
                "originalAssertion": assertion,
                "requiredTestId": case_id,
                "implementationStatus": "NOT_STARTED",
                "bindingStatus": "UNBOUND",
                "executionStatus": "NOT_STARTED",
                "evidence": [],
                "requiredEvidence": [
                    "同一 React 构建的旧/新内核入口操作与用户可见结果",
                    "真实 Rust 服务/持久化及副作用或拒绝结果",
                    "命令、具名测试、原始退出码、fixture/产物摘要、候选摘要与替身边界",
                ],
            })
        leaves.append({
            "id": leaf_id,
            "featureId": feature["feature_id"],
            "title": feature["title"],
            "domainId": feature["parent_domain"],
            "originalScenario": spec,
            "originalFeature": feature,
            "stageResponsibility": {
                "originalExecutionStageIds": spec["execution_stage_ids"],
                "originalExecutionTaskIds": spec["execution_task_ids"],
                "primaryR08Task": primary,
                "supportingR08Tasks": [item for item in dict.fromkeys(secondary) if item != primary],
                "assignmentBasis": "沿用原 R08 Task" if explicit else "原账已追加 R08 阶段；本地图将该阶段的客户端/协议验收接到 R08-T07，未改原 task_ids",
                "r08Scope": background["laterShare"] if leaf_id in later else "R08 负责现有客户端真实入口、兼容及副本往返；其他阶段份额继续由原阶段负责",
                "otherStageScopesPreserved": [stage for stage in spec["execution_stage_ids"] if stage != "R08"],
                "receivedR07LaterShare": leaf_id in later,
            },
            "entrypointChain": {
                "entryIds": feature["current_entrypoints"],
                "legacyOwners": feature["current_owners"],
                "sourceAnchors": feature["source_ref"],
                "candidateRustAndClientOwners": OWNERS[feature["parent_domain"]],
                "chainStatus": "SOURCE_CANDIDATES_NOT_RUNTIME_VERIFIED",
            },
            "executionAndStorage": {
                "targetOwner": feature["target_owner"],
                "legacyStores": feature["current_stores"],
                "dataPlanRef": "storePlans；具体路径/ID/引用在副本迁移核验时绑定，不凭同名推定兼容",
                "authority": "Rust 唯一裁决权限、Run 及消息状态；兼容层只转换；导入后历史不得双读双写",
            },
            "userResult": {"action": feature["user_action"], "visibleResult": feature["visible_result"], "assertions": spec["assertions"]},
            "assertionCases": cases,
            "existingTestCandidates": CANDIDATES[feature["parent_domain"]],
            "antecedentR07Binding": background,
            "antecedentEvidenceIsR08Pass": False,
            "status": "NOT_STARTED",
        })
    inventories = {path: test_inventory(path) for paths in CANDIDATES.values() for path in paths}
    for paths in OWNERS.values():
        assert all((ROOT / path).is_file() for path in paths)
    source_binding = {key: {"path": path, "sha256": sha(path)} for key, path in SOURCES.items()}
    domain_counts = Counter(leaf["domainId"] for leaf in leaves)
    task_counts = Counter(leaf["stageResponsibility"]["primaryR08Task"] for leaf in leaves)
    for task in tasks:
        task["status"] = "NOT_STARTED"
        task["supplementalLeafIds"] = [leaf["id"] for leaf in leaves if leaf["stageResponsibility"]["primaryR08Task"] == task["id"]]
    return {
        "schemaVersion": 1, "stage": "R08",
        "basis": "R00/ACCEPTANCE_MAP.json execution_stage_ids 含 R08；scenarios 完整镜像原账。原断言来自 FEATURE_STAGE_ACCEPTANCE；登记不是执行证据。",
        "sourceSha": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "sourceBindings": source_binding,
        "count": len(scenarios),
        "counts": {"tasks": len(tasks), "baseScenarios": len(base), "supplementalLeaves": len(leaves),
                   "originalAssertionEntries": sum(len(leaf["assertionCases"]) for leaf in leaves),
                   "includedR07LaterShare": len(later), "withOriginalR08Task": sum(bool([t for t in specs[k]["execution_task_ids"] if t.startswith("R08-")]) for k in scenarios),
                   "stageAddedWithoutR08Task": sum(not any(t.startswith("R08-") for t in specs[k]["execution_task_ids"]) for k in scenarios),
                   "byDomain": dict(sorted(domain_counts.items())), "byPrimaryR08Task": dict(sorted(task_counts.items()))},
        "status": "NOT_STARTED",
        "tasks": tasks,
        "baseScenarios": base,
        "scenarios": scenarios,
        "leaves": leaves,
        "existingTestInventory": inventories,
        "storePlans": {"legacyStores": load("dataCompatibility")["stores"],
                       "additionalRustPersistencePoints": load("rustStorageRegistry")["new_persistence_points"],
                       "status": "PLANNED_NOT_MIGRATED", "rules": [
                           "每类按 preserve/switch/rebuild/shell-local/epoch 原策略处理，不能只迁 runs.db",
                           "共用停写点逐库 checkpoint/一致备份，再文件快照与总 manifest；不复制活跃 .db 单文件",
                           "旧根只读，未知/opaque/签名块归档不直显；不足 Run 边界标 legacy_unknown",
                           "回滚恢复切换前快照，新写入保留可再导入包；再次前滚不重放外部操作",
                       ]},
        "additionalObligations": [{
            "id": "R08-X-BROWSER-HOST", "taskIds": ["R08-T03", "R08-T07"],
            "basis": "用户本轮生效裁决：沿用 R07 旧浏览器宿主退役，补替代宿主覆盖；不是第127条原补充叶",
            "requirements": ["替代宿主的真实可见交互/接管/登录/DOM/tab/截图/下载/重启恢复", "经 Rust 同一身份与授权；不恢复退役 WebContentsView", "现有 React 入口和会话浏览器联动可用"],
            "status": "NOT_STARTED", "testBindings": [], "evidence": [],
        }],
        "gateReadiness": {"registeredInXtask": False, "reason": "准备地图尚无 R08 证据生产者与 runner；不能以此文件或旧 R07 PASS 放行", "allAssertionCasesInitiallyUnbound": True},
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="仅验证生成结果与已存地图逐字一致")
    args = parser.parse_args()
    text = json.dumps(generate(), ensure_ascii=False, indent=2) + "\n"
    if args.check:
        if not DEST.is_file() or DEST.read_text() != text:
            raise SystemExit("R08_LEAF_MAP_STALE")
        print("R08_LEAF_MAP_PREPARATION_VALID tasks=8 base=16 leaves=126 assertions=281 laterShare=27 runtimeStatus=NOT_STARTED")
    else:
        DEST.parent.mkdir(parents=True, exist_ok=True)
        DEST.write_text(text)
        print("R08_LEAF_MAP_GENERATED tasks=8 base=16 leaves=126 assertions=281 laterShare=27 runtimeStatus=NOT_STARTED")


if __name__ == "__main__":
    main()
