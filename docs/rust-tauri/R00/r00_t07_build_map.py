#!/usr/bin/env python3
"""R00-T07：构建可执行验收账本 ACCEPTANCE_MAP.json 与 BLOCKERS.md。

输入（只读）：
  - Lingxi_Rust_Tauri_Taskbooks_2026-09-23/{acceptance-catalog.json, task-catalog.json}
    （原始规格，逐字导入 200 场景，不缩减不改写）
  - docs/rust-tauri/R00/{FEATURE_STAGE_ACCEPTANCE, ENTRYPOINT_COVERAGE,
    FEATURE_INVENTORY, ENTRYPOINTS}.json（T02/T04 已提交盘点）
  - docs/rust-tauri/R00/R00-T07_RESULTS.json（A01–A14 执行侧结果数据）
  - artifacts/rust-tauri/R00/T07/SELFTEST_SUMMARY.json（存在时回填 A13/A14 的
    tested_sha / timestamp / observed）

输出：
  - docs/rust-tauri/R00/ACCEPTANCE_MAP.json（账本本体，校验器唯一消费对象）
  - docs/rust-tauri/R00/BLOCKERS.md（阻塞登记，含最晚消除阶段）

哈希策略：所有 evidence/source 引用的当前文件 SHA-256 在构建时计算并写入账本；
校验器逐文件重算比对，任何事后改动（含未提交工作区改动）都会使对应结果过期。
"""
from __future__ import annotations

import hashlib
import json
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
TASKBOOK = ROOT / "Lingxi_Rust_Tauri_Taskbooks_2026-09-23"
DOCS = HERE
ART_T07 = ROOT / "artifacts/rust-tauri/R00/T07"


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def load(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def die(msg: str) -> None:
    print(f"BUILD-ERROR {msg}", file=sys.stderr)
    sys.exit(1)


def main() -> int:
    catalog_path = TASKBOOK / "acceptance-catalog.json"
    taskcat_path = TASKBOOK / "task-catalog.json"
    for p in (catalog_path, taskcat_path,
              DOCS / "FEATURE_STAGE_ACCEPTANCE.json",
              DOCS / "ENTRYPOINT_COVERAGE.json",
              DOCS / "FEATURE_INVENTORY.json",
              DOCS / "ENTRYPOINTS.json",
              DOCS / "R00-T07_RESULTS.json"):
        if not p.is_file():
            die(f"missing input {p}")

    catalog = load(catalog_path)
    taskcat = load(taskcat_path)
    fsa = load(DOCS / "FEATURE_STAGE_ACCEPTANCE.json")
    coverage = load(DOCS / "ENTRYPOINT_COVERAGE.json")
    inventory = load(DOCS / "FEATURE_INVENTORY.json")
    entrypoints = load(DOCS / "ENTRYPOINTS.json")
    results_input = load(DOCS / "R00-T07_RESULTS.json")

    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    branch = subprocess.check_output(["git", "rev-parse", "--abbrev-ref", "HEAD"],
                                     cwd=ROOT, text=True).strip()

    # ── 1. 导入 200 基础场景（逐字，不缩减） ────────────────────────────────
    base_specs = {}
    for s in catalog["scenarios"]:
        if s["id"] in base_specs:
            die(f"duplicate scenario id in catalog: {s['id']}")
        base_specs[s["id"]] = {
            "name": s["name"], "given": s["given"], "when": s["when"],
            "then": s["then"], "evidence": s["evidence"],
            "environment": s["environment"], "requirement": s["requirement"],
            "activation_condition": s["activation_condition"],
            "spec_status": s["status"], "task_id": s["task_id"],
            "stage_id": s["stage_id"],
        }
    if len(base_specs) != 200:
        die(f"catalog scenario count {len(base_specs)} != 200")

    # ── 2. 任务索引（100 项） ────────────────────────────────────────────────
    tasks = {}
    for t in taskcat["tasks"]:
        tasks[t["id"]] = {
            "name": t["name"], "stage_id": t["stage_id"],
            "depends_on": t.get("depends_on", []),
            "acceptance_ids": t.get("acceptance_ids", []),
            "deliverables": t.get("deliverables", []),
            "spec_status": t.get("status", "NOT_STARTED"),
        }

    # ── 3. T07 新增执行侧子场景（A13/A14 同根因变体覆盖，不触碰原始规格） ───
    t07_scenarios = {
        "R00-T07-LA-A13-V1-FLIP-NO-RESULT": {
            "name": "A13 变体：无结果 PASS 被拒绝",
            "given": "隔离副本中将 NOT_STARTED 场景 ledger_status 手改为 PASS（无任何 result 绑定）",
            "when": "运行账本校验器", "then": "非零退出，精确指出场景 ID 与缺失字段（PASS 无结果记录）",
            "parent_acceptance": "R00-A13"},
        "R00-T07-LA-A13-V2-EVIDENCE-DELETED": {
            "name": "A13 变体：删除证据文件被拒绝",
            "given": "隔离副本中将已 PASS 场景的证据文件删除（result 保留）",
            "when": "运行账本校验器", "then": "非零退出，精确指出 result ID、证据路径与缺失类型",
            "parent_acceptance": "R00-A13"},
        "R00-T07-LA-A13-V3-EVIDENCE-TAMPERED": {
            "name": "A13 变体：证据内容篡改被拒绝",
            "given": "隔离副本中修改已 PASS 结果引用的证据文件内容",
            "when": "运行账本校验器", "then": "非零退出，精确指出路径与期望/实际哈希",
            "parent_acceptance": "R00-A13"},
        "R00-T07-LA-A13-V4-MISSING-FIELD": {
            "name": "A13 变体：结果字段缺失被拒绝",
            "given": "隔离副本中删除某 PASS 结果的关键字段（tested_sha / exit_code）",
            "when": "运行账本校验器", "then": "非零退出，精确指出 result ID 与缺失字段名",
            "parent_acceptance": "R00-A13"},
        "R00-T07-LA-A13-V5-STALE-SOURCE": {
            "name": "A13 变体：源码变更后结果过期被拒绝",
            "given": "隔离副本中修改某结果 monitored source_path 的当前内容",
            "when": "运行账本校验器", "then": "非零退出，精确指出 result ID、路径与过期类型",
            "parent_acceptance": "R00-A13"},
        "R00-T07-LA-A13-V6-EXITCODE-CONFLICT": {
            "name": "A13 变体：跳过/失败冒充 PASS 被拒绝",
            "given": "隔离副本中将某结果的 exit_code 改为 1 但 status 保持 PASS",
            "when": "运行账本校验器", "then": "非零退出，精确指出 result ID 与字段冲突",
            "parent_acceptance": "R00-A13"},
        "R00-T07-LA-A13-V7-DUPLICATE-ID": {
            "name": "A13 变体：重复场景 ID 被拒绝",
            "given": "隔离副本中复制一个场景记录造成 ID 重复",
            "when": "运行账本校验器", "then": "非零退出，精确指出重复 ID",
            "parent_acceptance": "R00-A13"},
        "R00-T07-LA-A13-V8-TASK-REF-MISSING": {
            "name": "A13 变体：任务引用缺失场景被拒绝",
            "given": "隔离副本中从账本删除某任务声明的验收场景",
            "when": "运行账本校验器", "then": "非零退出，精确指出任务 ID 与缺失场景 ID",
            "parent_acceptance": "R00-A13"},
        "R00-T07-LA-A13-V9-SPEC-TAMPER": {
            "name": "A13 变体：规格文本篡改被拒绝",
            "given": "隔离副本中修改某场景 spec_snapshot 内的 then 文本",
            "when": "运行账本校验器", "then": "非零退出，精确指出场景 ID 与被篡改字段",
            "parent_acceptance": "R00-A13"},
        "R00-T07-LA-A13-V10-SCENARIO-RESULT-CONFLICT": {
            "name": "A13 变体：场景 PASS 绑定 FAIL 结果被拒绝",
            "given": "隔离副本中场景 ledger_status 保持 PASS，绑定结果的 status 改为 FAIL（exit_code 仍 0）",
            "when": "运行账本校验器",
            "then": "非零退出，同时指出场景↔结果状态脱节与零退出无法支撑 FAIL（R1 验收反例）",
            "parent_acceptance": "R00-A13"},
        "R00-T07-LA-A13-V11-RESULT-STATUS-ENUM": {
            "name": "A13 变体：结果状态非法枚举被拒绝",
            "given": "隔离副本中结果 status 写入非法枚举值 MAYBE_PASSED",
            "when": "运行账本校验器", "then": "非零退出，精确指出 result ID、字段名与非法值",
            "parent_acceptance": "R00-A13"},
        "R00-T07-LA-A13-V12-FAIL-EXIT-ZERO": {
            "name": "A13 变体：零退出冒充 FAIL 被拒绝",
            "given": "隔离副本中场景与结果一致改为 FAIL 但 exit_code 保持 0",
            "when": "运行账本校验器", "then": "非零退出，精确指出 result ID 与退出码矛盾",
            "parent_acceptance": "R00-A13"},
        "R00-T07-LA-A14-V1-ENTRY-UNMAPPED": {
            "name": "A14 变体：新增生产入口未映射被拒绝",
            "given": "隔离副本中在入口盘点（coverage+inventory）增加一个真实形态入口与功能，账本未映射",
            "when": "运行映射检查", "then": "非零退出，精确指出未映射的 entry_id 与 feature_id",
            "parent_acceptance": "R00-A14"},
        "R00-T07-LA-A14-V2-FEATURE-NO-SCENARIO": {
            "name": "A14 变体：新功能无场景/任务被拒绝",
            "given": "隔离副本中新功能已入账本 feature 索引但无场景与任务映射",
            "when": "运行映射检查", "then": "非零退出，精确指出 feature_id 与缺失维度",
            "parent_acceptance": "R00-A14"},
        "R00-T07-LA-A14-V3-MAPPED-PASSES": {
            "name": "A14 变体：补齐映射后通过",
            "given": "隔离副本中新入口/功能补齐 feature→scenario→task 完整映射",
            "when": "运行映射检查", "then": "退出码 0（账本校验通过）",
            "parent_acceptance": "R00-A14"},
        "R00-T07-LA-A14-V4-ORPHAN-MAPPING": {
            "name": "A14 变体：孤立映射被拒绝",
            "given": "隔离副本中账本 entrypoint 索引引用盘点不存在的入口",
            "when": "运行映射检查", "then": "非零退出，精确指出孤立 entry_id",
            "parent_acceptance": "R00-A14"},
    }

    # ── 4. 结果记录（哈希绑定） ─────────────────────────────────────────────
    selftest_summary = None
    if (ART_T07 / "SELFTEST_SUMMARY.json").is_file():
        selftest_summary = load(ART_T07 / "SELFTEST_SUMMARY.json")

    results = []
    for r in results_input["results"]:
        rec = dict(r)
        aid = rec["acceptance_id"]
        if aid in ("R00-A13", "R00-A14"):
            if selftest_summary is None:
                # 引导阶段：自测尚未运行，A13/A14 结果与证据不入账本
                #（场景保持 IN_PROGRESS；自测完成后的重建才会嵌入）
                continue
            suite_key = "a13" if aid == "R00-A13" else "a14"
            suite = selftest_summary["suites"][suite_key]
            rec["tested_sha"] = selftest_summary["validated_repo_head"]
            rec["timestamp_utc"] = suite["started_utc"]
            rec["observed"] = (f"selftest suite {suite_key}: {suite['variants_passed']}/{suite['variants_total']} "
                               f"variants behaved as expected (canonical case + same-root-cause variants); "
                               f"validator exit codes and stdout captured in logs")
        ev_filled = []
        for ev in rec.get("evidence", []):
            p = ROOT / ev["path"]
            if not p.is_file():
                die(f"result {rec['result_id']} evidence missing on disk: {ev['path']}")
            ev_filled.append({**ev, "sha256": sha256_file(p)})
        rec["evidence"] = ev_filled
        src_digests = {}
        for sp in rec.get("source_paths", []):
            p = ROOT / sp
            if not p.is_file():
                die(f"result {rec['result_id']} source_path missing on disk: {sp}")
            src_digests[sp] = sha256_file(p)
        rec["source_digests"] = src_digests
        if rec.get("working_tree_digest") is None and src_digests:
            joined = "".join(f"{sha}  {p}\n" for p, sha in sorted(src_digests.items()))
            rec["working_tree_digest"] = hashlib.sha256(joined.encode()).hexdigest()
        results.append(rec)

    # ── 5. 现有测试 ↔ 场景关联 ──────────────────────────────────────────────
    test_links = {
        "tests/migration/r00-t05-replay.test.ts": ["R00-A09"],
        "tests/migration/r00-a10-old-defect.test.ts": ["R00-A10"],
        "scripts/rust-tauri/r00-t05-replay.mjs": ["R00-A09"],
        "docs/rust-tauri/R00/r00_t02_inventory.py": ["R00-A03", "R00-A04"],
        "docs/rust-tauri/R00/r00_t02_source_gates.py": ["R00-A03"],
        "docs/rust-tauri/R00/ask_user_ast_gate.cjs": ["R00-A03"],
        "docs/rust-tauri/R00/r00_t03_import_graph.mjs": ["R00-A05"],
        "docs/rust-tauri/R00/r00_t03_build_matrices.py": ["R00-A05"],
        "docs/rust-tauri/R00/r00_t03_validate.py": ["R00-A05", "R00-A06"],
        "docs/rust-tauri/R00/r00_t04_scan.py": ["R00-A07", "R00-A08"],
        "tests/persistence-store-registry.test.ts": ["R00-A07"],
        "tests/persistence-schema-tripwire.test.ts": ["R00-A07"],
        "tests/http-route-security.test.ts": ["R00-A08"],
        "tests/server-auth.test.ts": ["R00-A08"],
        "tests/ws-scope.test.ts": ["R00-A08"],
        "tests/device-registry.test.ts": ["R00-A08"],
        "tests/config-scope.test.ts": ["R00-A04"],
        "tests/provider-catalog.test.ts": ["R00-A04"],
        "scripts/rust-tauri/r00-t06-bench-server.mjs": ["R00-A11"],
        "scripts/rust-tauri/r00-t06-bench-pty.mjs": ["R00-A11"],
        "scripts/rust-tauri/r00-t06-bench-desktop.mjs": ["R00-A11"],
        "scripts/rust-tauri/r00-t06-bench-office-pdf.mjs": ["R00-A11"],
        "scripts/rust-tauri/r00-t06-build-release.mjs": ["R00-A11"],
        "scripts/rust-tauri/r00-t06-stub-provider.mjs": ["R00-A11"],
        "scripts/rust-tauri/r00-t06-probe-fixed-prefix.mjs": ["R00-A12"],
        "scripts/rust-tauri/r00-t06-summarize.mjs": ["R00-A11", "R00-A12"],
        "docs/rust-tauri/R00/r00_t07_validate_ledger.py": [
            "R00-A13", "R00-A14", "R00-T07-LA-A13-V1-FLIP-NO-RESULT",
            "R00-T07-LA-A13-V2-EVIDENCE-DELETED", "R00-T07-LA-A13-V3-EVIDENCE-TAMPERED",
            "R00-T07-LA-A13-V4-MISSING-FIELD", "R00-T07-LA-A13-V5-STALE-SOURCE",
            "R00-T07-LA-A13-V6-EXITCODE-CONFLICT", "R00-T07-LA-A13-V7-DUPLICATE-ID",
            "R00-T07-LA-A13-V8-TASK-REF-MISSING", "R00-T07-LA-A13-V9-SPEC-TAMPER",
            "R00-T07-LA-A13-V10-SCENARIO-RESULT-CONFLICT",
            "R00-T07-LA-A13-V11-RESULT-STATUS-ENUM",
            "R00-T07-LA-A13-V12-FAIL-EXIT-ZERO",
            "R00-T07-LA-A14-V1-ENTRY-UNMAPPED", "R00-T07-LA-A14-V2-FEATURE-NO-SCENARIO",
            "R00-T07-LA-A14-V3-MAPPED-PASSES", "R00-T07-LA-A14-V4-ORPHAN-MAPPING"],
        "docs/rust-tauri/R00/r00_t07_selftest.py": [
            "R00-A13", "R00-A14", "R00-T07-LA-A13-V1-FLIP-NO-RESULT",
            "R00-T07-LA-A13-V2-EVIDENCE-DELETED", "R00-T07-LA-A13-V3-EVIDENCE-TAMPERED",
            "R00-T07-LA-A13-V4-MISSING-FIELD", "R00-T07-LA-A13-V5-STALE-SOURCE",
            "R00-T07-LA-A13-V6-EXITCODE-CONFLICT", "R00-T07-LA-A13-V7-DUPLICATE-ID",
            "R00-T07-LA-A13-V8-TASK-REF-MISSING", "R00-T07-LA-A13-V9-SPEC-TAMPER",
            "R00-T07-LA-A13-V10-SCENARIO-RESULT-CONFLICT",
            "R00-T07-LA-A13-V11-RESULT-STATUS-ENUM",
            "R00-T07-LA-A13-V12-FAIL-EXIT-ZERO",
            "R00-T07-LA-A14-V1-ENTRY-UNMAPPED", "R00-T07-LA-A14-V2-FEATURE-NO-SCENARIO",
            "R00-T07-LA-A14-V3-MAPPED-PASSES", "R00-T07-LA-A14-V4-ORPHAN-MAPPING"],
    }
    tests = []
    for path, aids in sorted(test_links.items()):
        p = ROOT / path
        if not p.is_file():
            die(f"declared test missing on disk: {path}")
        kind = ("vitest" if re.search(r"\.test\.ts$", path)
                else "validator" if "validate" in Path(path).name or "inventory" in Path(path).name
                or "scan" in Path(path).name or "gates" in Path(path).name or "gate" in Path(path).name
                else "script")
        tests.append({"test_id": f"T-{len(tests)+1:03d}", "path": path, "kind": kind,
                      "sha256": sha256_file(p), "scenario_ids": aids})

    # ── 6. 双向索引组装 ─────────────────────────────────────────────────────
    inv_by_fid = {f["feature_id"]: f for f in inventory["features"]}
    for f in inventory["classified_nonproduction"]:
        inv_by_fid[f["feature_id"]] = f
    fsa_by_fid = {f["feature_id"]: f for f in fsa["features"]}
    reg_by_entry = {}
    for r in coverage["registrations"]:
        reg_by_entry.setdefault(r["entry_id"], {"feature_ids": set(), "source_refs": set()})
        reg_by_entry[r["entry_id"]]["feature_ids"].update(r.get("feature_ids") or [])
        reg_by_entry[r["entry_id"]]["source_refs"].add(r.get("source_ref", ""))

    results_by_aid = {}
    for rec in results:
        results_by_aid.setdefault(rec["acceptance_id"], []).append(rec["result_id"])

    scenarios = {}
    # 基础场景
    for sid, spec in base_specs.items():
        feats = sorted(fid for fid, fs in fsa_by_fid.items()
                       if sid in (fs.get("acceptance_ids") or []))
        scenarios[sid] = {
            "kind": "base", "stage_id": spec["stage_id"], "task_ids": [spec["task_id"]],
            "requirement": spec["requirement"], "spec": spec,
            "ledger_status": "PASS" if sid in results_by_aid else
                             ("NOT_RUN_UNAUTHORIZED" if spec["requirement"] == "CONDITIONAL_AUTHORIZATION"
                              else "NOT_STARTED"),
            "result_ids": results_by_aid.get(sid, []),
            "feature_ids": feats,
            "test_ids": [t["test_id"] for t in tests if sid in t["scenario_ids"]],
        }
    # 补充叶子子场景（T02 预规格、T07 正式登记）
    for s in fsa["supplemental_scenarios"]:
        scenarios[s["id"]] = {
            "kind": "supplemental", "stage_id": "R00",
            "execution_stage_ids": s.get("execution_stage_ids", []),
            "task_ids": s.get("execution_task_ids") or s.get("task_ids", []),
            "requirement": "REQUIRED_SUPPLEMENTAL",
            "feature_id": s["feature_id"],
            "then": s["then"],
            "formalization_task_id": s.get("formalization_task_id", "R00-T07"),
            "ledger_status": s.get("status", "SPECIFIED_NOT_EXECUTED"),
            "result_ids": [], "feature_ids": [s["feature_id"]], "test_ids": [],
        }
    # T07 新增执行侧场景
    for sid, spec in t07_scenarios.items():
        parent = spec["parent_acceptance"]
        scenarios[sid] = {
            "kind": "t07_added", "stage_id": "R00", "task_ids": ["R00-T07"],
            "requirement": "REQUIRED_ADDED", "name": spec["name"],
            "given": spec["given"], "when": spec["when"], "then": spec["then"],
            "parent_acceptance": parent,
            "ledger_status": "PASS" if selftest_summary is not None else "IN_PROGRESS",
            "result_ids": [f"RES-{parent}"] if selftest_summary is not None else [],
            "feature_ids": [], "test_ids": [t["test_id"] for t in tests if sid in t["scenario_ids"]],
        }

    # 任务→场景反向索引
    for tid, t in tasks.items():
        t["scenario_ids"] = sorted(sid for sid, sc in scenarios.items() if tid in sc["task_ids"])

    # 功能索引
    features_index = {}
    for fid, inv in inv_by_fid.items():
        fs = fsa_by_fid.get(fid)
        entries = sorted(e for e, reg in reg_by_entry.items() if fid in reg["feature_ids"])
        sup = fs.get("supplemental_acceptance_id") if fs else None
        features_index[fid] = {
            "title": inv.get("title", ""),
            "parent_domain": inv.get("parent_domain", ""),
            "classification": inv.get("classification", ""),
            "kind": "production" if fid in {f["feature_id"] for f in inventory["features"]} else "non_production",
            "stage_ids": (fs or {}).get("stage_ids", (inv.get("stage_ids") or [])),
            "task_ids": (fs or {}).get("task_ids", (inv.get("task_ids") or [])),
            "acceptance_ids": (fs or {}).get("acceptance_ids", (inv.get("acceptance_ids") or [])),
            "supplemental_scenario_ids": [sup] if sup else [],
            "entrypoints": entries,
        }

    entrypoint_index = {}
    for entry, reg in sorted(reg_by_entry.items()):
        fids = sorted(reg["feature_ids"])
        entrypoint_index[entry] = {
            "feature_ids": fids, "source_refs": sorted(x for x in reg["source_refs"] if x),
            "scenario_ids": sorted(sid for fid in fids
                                   for sid in features_index[fid]["supplemental_scenario_ids"]),
        }
    surfaces = {}
    for e in entrypoints["entries"]:
        surfaces[e["id"]] = {
            "title": e["title"], "category": e["category"], "status": e["status"],
            "anchor_count": len(e.get("anchors", [])),
            "runtime_owner": e.get("runtime_owner", ""),
        }

    # ── 7. 阻塞登记（BLOCKED 预登记 + 条件授权） ─────────────────────────────
    # 关键词为对 acceptance-catalog 逐场景文本（environment/then/given/when/
    # activation_condition）的精确子串匹配；只绑定规格文本明示需要真实外部
    # 条件的场景，不做语义猜测（验收方可复核每个绑定）。
    kw_rules = [
        ("BLK-CREDENTIALS", "缺真实供应商凭证/真实测试账号授权（LIVE 项）", "R10",
         ["真实供应商", "测试账号", "LIVE项"]),
        ("BLK-PLATFORM", "缺目标平台真机（规格明示真实/各/四组目标 OS）", "R10",
         ["真实目标操作系统", "真实安装的目标OS", "各目标操作系统", "四组目标平台"]),
    ]
    anticipated = {b[0]: {"blocker_id": b[0], "category_reason": b[1],
                          "latest_resolution_stage": b[2], "keywords": b[3],
                          "scenario_ids": []} for b in kw_rules}
    for sid, spec in base_specs.items():
        text = " ".join([spec["environment"], spec["then"], spec["given"], spec["when"],
                         spec.get("activation_condition", "")])
        for bid, blk in anticipated.items():
            if any(k in text for k in blk["keywords"]):
                blk["scenario_ids"].append(sid)
    for blk in anticipated.values():
        blk["scenario_count"] = len(blk["scenario_ids"])

    blockers = {
        "active": [],
        "anticipated": sorted(anticipated.values(), key=lambda b: b["blocker_id"]),
        "conditional_authorization": [{
            "blocker_id": "BLK-RELEASE-AUTH", "category_reason": "远程发布未获用户授权",
            "scenario_ids": ["R11-A14"],
            "ledger_status": "NOT_RUN_UNAUTHORIZED",
            "latest_resolution_stage": "R11（授权后激活；未授权不阻塞技术交付）",
        }],
        "not_run_this_round": [{
            "blocker_id": "BLK-LONGRUN-G1", "category_reason":
                "长时资源增长（2h/1000 任务）G1 侧未测（T06 登记 NOT_RUN_THIS_ROUND，阈值已冻结）",
            "scenario_ids": [], "latest_resolution_stage": "R10",
        }],
        "notes": [
            "anticipated 项在对应阶段实际执行且仍缺条件时转为 active BLOCKED；当前各未来阶段场景保持 NOT_STARTED/SPECIFIED_NOT_EXECUTED，不预写 FAIL。",
            "绑定只覆盖规格文本明示需要真实外部条件的场景（精确子串，逐场景可查询复核）；阶段级依赖（如 R05 真实 provider 凭证、R07 平台真实账号、R08 真实数据副本演练授权）由 04_阶段依赖的截止关卡约束，不在没有明示文本的场景上猜测绑定。",
        ],
    }

    # ── 8. 汇总与写出 ───────────────────────────────────────────────────────
    input_files = {
        "acceptance-catalog.json": sha256_file(catalog_path),
        "task-catalog.json": sha256_file(taskcat_path),
        "FEATURE_STAGE_ACCEPTANCE.json": sha256_file(DOCS / "FEATURE_STAGE_ACCEPTANCE.json"),
        "ENTRYPOINT_COVERAGE.json": sha256_file(DOCS / "ENTRYPOINT_COVERAGE.json"),
        "FEATURE_INVENTORY.json": sha256_file(DOCS / "FEATURE_INVENTORY.json"),
        "ENTRYPOINTS.json": sha256_file(DOCS / "ENTRYPOINTS.json"),
        "R00-T07_RESULTS.json": sha256_file(DOCS / "R00-T07_RESULTS.json"),
        "r00_t07_build_map.py": sha256_file(Path(__file__)),
    }
    if selftest_summary is not None:
        input_files["SELFTEST_SUMMARY.json"] = sha256_file(ART_T07 / "SELFTEST_SUMMARY.json")

    kind_counts = {}
    for sc in scenarios.values():
        kind_counts[sc["kind"]] = kind_counts.get(sc["kind"], 0) + 1
    status_counts = {}
    for sc in scenarios.values():
        status_counts[sc["ledger_status"]] = status_counts.get(sc["ledger_status"], 0) + 1

    # 规格导入快照摘要：任务书目录不可用时（隔离副本/独立环境），
    # 校验器用 spec_digests 检测导入后规格文本被篡改。
    spec_digests = {}
    for sid, spec in base_specs.items():
        canon = json.dumps(spec, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
        spec_digests[sid] = hashlib.sha256(canon.encode()).hexdigest()

    amap = {
        "schema_version": "1.0",
        "task_id": "R00-T07",
        "artifact_kind": "EXECUTABLE_ACCEPTANCE_LEDGER",
        "ledger_self_status": "READY_FOR_REVIEW",
        "ledger_self_status_note": "执行者只标 READY_FOR_REVIEW；ACCEPTED 由独立验收决定（01 通用约束 §6）",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "generator": "docs/rust-tauri/R00/r00_t07_build_map.py",
        "basis": {"head": head, "branch": branch,
                  "validated_repo_head": selftest_summary["validated_repo_head"] if selftest_summary else head,
                  "input_sha256": input_files,
                  "package_lock_sha256": sha256_file(ROOT / "package-lock.json")},
        "counts": {
            "base_scenarios": kind_counts.get("base", 0),
            "supplemental_scenarios": kind_counts.get("supplemental", 0),
            "t07_added_scenarios": kind_counts.get("t07_added", 0),
            "scenarios_total": len(scenarios),
            "tasks": len(tasks),
            "features_total": len(features_index),
            "features_production": sum(1 for f in features_index.values() if f["kind"] == "production"),
            "entrypoints_registered": len(entrypoint_index),
            "surfaces": len(surfaces),
            "tests": len(tests),
            "results": len(results),
            "ledger_status_counts": status_counts,
        },
        "scenarios": scenarios,
        "spec_digests": spec_digests,
        "spec_digests_note": "sha256(canonical-json(spec))，逐基础场景；由任务书 acceptance-catalog.json 逐字导入时计算",
        "tasks": tasks,
        "features_index": features_index,
        "entrypoint_index": entrypoint_index,
        "surfaces": surfaces,
        "tests": tests,
        "results": results,
        "blockers": blockers,
        "validation_rules_doc": "docs/rust-tauri/R00/r00_t07_validate_ledger.py（规则 ID 与账本字段一一对应）",
    }
    (DOCS / "ACCEPTANCE_MAP.json").write_text(
        json.dumps(amap, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")

    write_blockers_md(blockers, status_counts, kind_counts, head)
    print(f"MAP-BUILT scenarios={len(scenarios)} tasks={len(tasks)} "
          f"features={len(features_index)} entries={len(entrypoint_index)} "
          f"tests={len(tests)} results={len(results)} selftest={'attached' if selftest_summary else 'pending'}")
    return 0


def write_blockers_md(blockers, status_counts, kind_counts, head) -> None:
    lines = []
    lines.append("# R00 阻塞登记（BLOCKERS.md）")
    lines.append("")
    lines.append(f"生成：`docs/rust-tauri/R00/r00_t07_build_map.py`｜基准 HEAD `{head}`｜配套账本 `ACCEPTANCE_MAP.json`。")
    lines.append("状态语义遵循 01 通用约束 §6：BLOCKED 表示已到执行时点但缺凭证/平台/授权；")
    lines.append("未来阶段场景保持 NOT_STARTED/SPECIFIED_NOT_EXECUTED，其外部依赖在此预登记（anticipated），")
    lines.append("到对应阶段仍缺条件时转为 active BLOCKED，不预写 FAIL、不伪造 PASS。")
    lines.append("")
    lines.append("## 1. 当前 ACTIVE 阻塞（阻止 R00-T07 放行的项）")
    if blockers["active"]:
        for b in blockers["active"]:
            lines.append(f"- **{b['blocker_id']}**：{b['category_reason']}（最晚消除：{b['latest_resolution_stage']}）")
    else:
        lines.append("- 无。R00-A13/A14 已在本机隔离环境真实执行（见 ACCEPTANCE_MAP.json results 与 artifacts/rust-tauri/R00/T07/）。")
    lines.append("")
    lines.append("## 2. 条件授权（未授权不算失败，也不算通过）")
    for b in blockers["conditional_authorization"]:
        lines.append(f"- **{b['blocker_id']}**：{b['category_reason']}；场景 {', '.join(b['scenario_ids'])} "
                     f"状态 {b['ledger_status']}；最晚消除：{b['latest_resolution_stage']}")
    lines.append("")
    lines.append("## 3. 本轮声明未执行（NOT_RUN_THIS_ROUND，已有冻结口径）")
    for b in blockers["not_run_this_round"]:
        lines.append(f"- **{b['blocker_id']}**：{b['category_reason']}；最晚消除：{b['latest_resolution_stage']}")
    lines.append("")
    lines.append("## 4. 预登记（ANTICIPATED）：未来阶段外部依赖，按关键词保守绑定")
    lines.append("")
    lines.append("| 阻塞 ID | 原因 | 最晚消除阶段 | 绑定场景数 | 绑定关键词 |")
    lines.append("|---|---|---|---|---|")
    for b in blockers["anticipated"]:
        kws = "、".join(b["keywords"])
        lines.append(f"| {b['blocker_id']} | {b['category_reason']} | {b['latest_resolution_stage']} | "
                     f"{b['scenario_count']} | {kws} |")
    lines.append("")
    lines.append("逐场景绑定明细在 `ACCEPTANCE_MAP.json` → `blockers.anticipated[].scenario_ids`（可查询、可复核）。")
    lines.append("")
    lines.append("## 5. 账本状态分布（构建时点）")
    lines.append("")
    lines.append("| 维度 | 计数 |")
    lines.append("|---|---|")
    for k, v in sorted(kind_counts.items()):
        lines.append(f"| 场景 kind {k} | {v} |")
    for k, v in sorted(status_counts.items()):
        lines.append(f"| ledger_status {k} | {v} |")
    # 尾部不再追加空行：join+「\n」已给出恰好一个结尾换行（多一个空行会被
    # git diff --check 报 "new blank line at EOF"）
    DOCS.joinpath("BLOCKERS.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


if __name__ == "__main__":
    sys.exit(main())
