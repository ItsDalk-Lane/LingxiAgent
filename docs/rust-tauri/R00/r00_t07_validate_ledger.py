#!/usr/bin/env python3
"""R00-T07：验收账本校验器。

用法：
  python3 docs/rust-tauri/R00/r00_t07_validate_ledger.py [--root ROOT] [--git-repo GIT_REPO]

默认 ROOT=仓库根；账本=ROOT/docs/rust-tauri/R00/ACCEPTANCE_MAP.json。
退出码：0=LEDGER_VALID；1=LEDGER_INVALID（错误行精确到 ID/字段/路径）；2=输入错误。

规则族（错误行前缀 LEDGER-ERROR <RULE>，稳定不随实现变化）：
  MAP-INPUT                账本/输入文件缺失或不可解析（含 JSON 重复键检测）
  SPEC-IMPORT-COMPLETE     200 基础场景必须全量导入（对任务书目录做严格比对）
  SPEC-SOURCE-PROVENANCE   账本声明的 acceptance-catalog 来源哈希与实际不符
  SPEC-FIDELITY            场景规格文本/规格摘要与导入快照不一致（篡改规格）
  NO-DUPLICATE-ID          任何层面的重复 ID
  TASK-SCENARIO-BIJCTION   任务声明的验收场景缺失 / 场景引用不存在的任务
  ORPHAN-REQUIREMENT       场景无任务、生产功能无场景、补充场景无功能
  ENTRY-COVERAGE-BIDIR     入口盘点与账本 entrypoint 索引双向差集（A14 核心）
  FEATURE-INVENTORY-BIDIR  功能清单与账本 feature 索引双向差集
  STATUS-PASS-WITHOUT-RESULT  PASS 无结果记录（A13 核心）
  STATUS-NOTSTARTED-WITH-RESULT 未执行状态却挂结果
  STATUS-ENUM              非法状态值
  RESULT-STATUS-ENUM       结果记录 status 非法枚举值
  STATUS-RESULT-CONFLICT   场景 ledger_status=PASS 但绑定结果 status 非 PASS
  RESULT-MISSING-FIELD     结果缺字段/空值（A13：精确到字段名）
  RESULT-BAD-FIELD         字段格式非法（SHA/时间戳/类型）
  COMMIT-PENDING-BASIS     committed_in=null 的待提交结果存在时，basis.head 必须仍是
                           当前 Git HEAD（提交后 HEAD 前移而账本未重建重绑的，拒绝——
                           防止旧基准头+空 committed_in 的过期待提交态冒充当前状态）
  EVIDENCE-MISSING-FILE    证据文件不存在（A13：删日志）
  EVIDENCE-HASH-MISMATCH   证据哈希不符（A13：篡改）
  EXIT-CODE-CONFLICT       exit_code 与 status 冲突（非零冒充 PASS / 零退出冒充 FAIL）
  STALE-SOURCE             结果相对源码变更过期（digest 不符/文件缺失）
  STALE-BRANCH             tested_sha 不在当前 HEAD 历史中
  LOCKFILE-MISMATCH        结果记录的 lockfile 哈希与当前不符（配置过期）
  TESTS-MISSING/HASH       登记测试不存在/哈希不符/引用未知场景
  BLOCKER-REGISTRY         阻塞登记与账本矛盾（含条件授权场景状态）
  COUNTS-MISMATCH          汇总计数与实际不符
  LEDGER-SELF-STATUS       执行者不得自标 PASS/ACCEPTED
  STAGE-PREFIX             场景 ID 前缀与 stage 不一致
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

ALLOWED_LEDGER_STATUS = {
    "NOT_STARTED", "IN_PROGRESS", "PASS", "FAIL", "BLOCKED", "STALE",
    "NOT_APPLICABLE", "SPECIFIED_NOT_EXECUTED", "NOT_RUN_UNAUTHORIZED",
}
# 结果记录 status 合法集（01 通用约束 §6 场景状态集中可作用于执行记录的值
# + task-result.template.json 的执行前占位 NOT_RUN）。NOT_STARTED/IN_PROGRESS
# 是未执行场景的规划态，不允许出现在结果记录中。FAIL/BLOCKED/STALE 保持合法：
# 真实失败/阻塞必须如实记录，只是不能挂在 PASS 场景下（STATUS-RESULT-CONFLICT）。
ALLOWED_RESULT_STATUS = {
    "NOT_RUN", "PASS", "FAIL", "BLOCKED", "STALE", "NOT_APPLICABLE",
}
HEX40 = re.compile(r"^[0-9a-f]{40}$")
ISO_TS = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}")
STAGES = {f"R{i:02d}" for i in range(12)}

RESULT_REQUIRED_FIELDS = [
    "result_id", "acceptance_id", "task_id", "status", "tested_sha",
    "timestamp_utc", "timestamp_basis", "working_tree_digest",
    "working_tree_digest_scope", "platform", "toolchain",
    "dependency_lock_hashes", "command", "exit_code", "expected", "observed",
    "stub_boundary", "source_paths", "source_digests", "evidence",
]
# committed_in 由专用规则管理：仅 tested_sha == basis.head 的当前任务结果允许为 null
# （提交尚待授权），历史结果必须为 40-hex。

errors: list[str] = []
checks = 0


def err(rule: str, target: str, detail: str) -> None:
    errors.append(f"LEDGER-ERROR {rule} {target} {detail}")


def check(cond: bool, rule: str, target: str, detail: str) -> bool:
    global checks
    checks += 1
    if not cond:
        err(rule, target, detail)
    return cond


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def load_strict(path: Path):
    """解析 JSON 并检测任意层级的重复键（json 默认静默覆盖）。"""
    def hook(pairs):
        seen = set()
        for k, _ in pairs:
            if k in seen:
                raise DuplicateKey(f"duplicate key '{k}'")
            seen.add(k)
        return dict(pairs)

    class DuplicateKey(Exception):
        pass

    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=hook)
    except DuplicateKey as e:
        err("NO-DUPLICATE-ID", str(path), f"JSON {e}")
        return None
    except json.JSONDecodeError as e:
        err("MAP-INPUT", str(path), f"JSON parse error: {e}")
        return None


def canonical(obj) -> str:
    return json.dumps(obj, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def main() -> int:
    ap = argparse.ArgumentParser(description="R00-T07 acceptance ledger validator")
    ap.add_argument("--root", default=None, help="仓库根（默认：脚本位置上两级）")
    ap.add_argument("--git-repo", default=None,
                    help="用于 tested_sha 祖先校验的 git 仓库（默认同 --root；"
                         "隔离副本测试时指向真实仓库）")
    args = ap.parse_args()
    root = Path(args.root).resolve() if args.root else Path(__file__).resolve().parent.parents[2]
    git_repo = Path(args.git_repo).resolve() if args.git_repo else root

    r00 = root / "docs/rust-tauri/R00"
    amap_path = r00 / "ACCEPTANCE_MAP.json"
    if not amap_path.is_file():
        print(f"LEDGER-ERROR MAP-INPUT {amap_path} file does not exist", file=sys.stderr)
        return 2
    amap = load_strict(amap_path)
    if amap is None:
        print("\n".join(errors))
        print(f"LEDGER_INVALID errors={len(errors)}")
        return 1

    for key in ("schema_version", "task_id", "basis", "counts", "scenarios", "tasks",
                "features_index", "entrypoint_index", "surfaces", "tests", "results",
                "blockers", "ledger_self_status"):
        check(key in amap, "MAP-INPUT", "ACCEPTANCE_MAP.json", f"missing top-level key '{key}'")
    if errors:
        print("\n".join(errors))
        print(f"LEDGER_INVALID errors={len(errors)}")
        return 1

    scenarios: dict = amap["scenarios"]
    tasks: dict = amap["tasks"]
    features: dict = amap["features_index"]
    entry_index: dict = amap["entrypoint_index"]
    results: list = amap["results"]
    tests: list = amap["tests"]

    # ── 伴生输入（在隔离副本中与账本一起被校验） ─────────────────────────────
    fsa = load_strict(r00 / "FEATURE_STAGE_ACCEPTANCE.json") or {}
    coverage = load_strict(r00 / "ENTRYPOINT_COVERAGE.json") or {}
    inventory = load_strict(r00 / "FEATURE_INVENTORY.json") or {}
    entrypoints_doc = load_strict(r00 / "ENTRYPOINTS.json") or {}
    catalog_path = root / "Lingxi_Rust_Tauri_Taskbooks_2026-09-23/acceptance-catalog.json"
    catalog = load_strict(catalog_path) if catalog_path.is_file() else None

    # ── 1. 规格导入完整性与保真 ─────────────────────────────────────────────
    base = {sid: sc for sid, sc in scenarios.items() if sc.get("kind") == "base"}
    check(len(base) == 200, "SPEC-IMPORT-COMPLETE", "scenarios[kind=base]",
          f"base scenario count {len(base)} != 200 (scope reduction is forbidden)")
    if catalog is not None:
        cat_ids = {s["id"] for s in catalog["scenarios"]}
        missing = sorted(cat_ids - set(base))
        extra = sorted(set(base) - cat_ids)
        check(not missing, "SPEC-IMPORT-COMPLETE", "scenarios[kind=base]",
              f"catalog scenarios missing from ledger: {missing[:8]}")
        check(not extra, "SPEC-IMPORT-COMPLETE", "scenarios[kind=base]",
              f"unknown base scenarios not in catalog: {extra[:8]}")
        cat_by_id = {s["id"]: s for s in catalog["scenarios"]}
        for sid, sc in sorted(base.items()):
            spec = sc.get("spec") or {}
            src = cat_by_id[sid]
            for field in ("name", "given", "when", "then", "evidence", "environment",
                          "requirement", "activation_condition", "task_id", "stage_id"):
                check(spec.get(field) == src.get(field), "SPEC-FIDELITY", f"scenario={sid}",
                      f"field 'spec.{field}' differs from acceptance-catalog.json")
        declared = amap["basis"]["input_sha256"].get("acceptance-catalog.json")
        actual_cat = sha256_file(catalog_path)
        check(declared == actual_cat, "SPEC-SOURCE-PROVENANCE", "basis.input_sha256",
              f"declared acceptance-catalog.json sha256 {declared} != actual {actual_cat} "
              f"(ledger built from a different catalog revision)")
    else:
        # 无任务书目录（如隔离副本）：用账本内嵌 spec_digests 快照做保真校验
        snaps = amap.get("spec_digests") or {}
        check(bool(snaps), "SPEC-IMPORT-COMPLETE", "spec_digests",
              "taskbook dir absent AND ledger has no embedded spec_digests snapshot; "
              "cannot verify spec fidelity")
        for sid, sc in sorted(base.items()):
            if sid in snaps:
                digest = hashlib.sha256(canonical(sc.get("spec")).encode()).hexdigest()
                check(digest == snaps[sid], "SPEC-FIDELITY", f"scenario={sid}",
                      f"embedded spec digest {digest} != snapshot {snaps[sid]} "
                      f"(spec text tampered after import)")
            else:
                err("SPEC-IMPORT-COMPLETE", f"scenario={sid}",
                    "no spec_digests entry (import incomplete)")

    # ── 2. ID 唯一性 ────────────────────────────────────────────────────────
    check(len(scenarios) == len(set(scenarios)), "NO-DUPLICATE-ID", "scenarios",
          "duplicate scenario keys")  # load_strict 已在 JSON 层拦截
    seen_rid = set()
    for r in results:
        rid = r.get("result_id")
        check(rid not in seen_rid, "NO-DUPLICATE-ID", f"result={rid}",
              f"duplicate result_id (also at {rid})" if rid in seen_rid else "")
        seen_rid.add(rid)
    seen_tid = set()
    for t in tests:
        tid = t.get("test_id")
        check(tid not in seen_tid, "NO-DUPLICATE-ID", f"test={tid}", "duplicate test_id")
        seen_tid.add(tid)

    # ── 3. 任务↔场景双向 ────────────────────────────────────────────────────
    for tid, t in sorted(tasks.items()):
        for aid in t.get("acceptance_ids", []):
            check(aid in scenarios, "TASK-SCENARIO-BIJCTION", f"task={tid}",
                  f"declares acceptance '{aid}' missing from ledger scenarios")
        check(bool(t.get("scenario_ids")), "ORPHAN-REQUIREMENT", f"task={tid}",
              "no scenarios linked (reverse index empty)")
    for sid, sc in sorted(scenarios.items()):
        tids = sc.get("task_ids") or []
        check(bool(tids), "ORPHAN-REQUIREMENT", f"scenario={sid}",
              "no task_ids (orphan requirement)")
        for tid in tids:
            check(tid in tasks, "TASK-SCENARIO-BIJCTION", f"scenario={sid}",
                  f"references unknown task '{tid}'")
        stage = sc.get("stage_id", "")
        check(sid.split("-A")[0] == stage if "-A" in sid and sc.get("kind") == "base" else True,
              "STAGE-PREFIX", f"scenario={sid}", f"stage_id '{stage}' does not match ID prefix")

    # ── 4. 功能/入口双向映射（A14 核心） ────────────────────────────────────
    inv_ids = {f["feature_id"] for f in inventory.get("features", [])}
    inv_ids |= {f["feature_id"] for f in inventory.get("classified_nonproduction", [])}
    check(set(features) == inv_ids, "FEATURE-INVENTORY-BIDIR", "features_index",
          f"ledger-only features: {sorted(set(features) - inv_ids)[:6]}; "
          f"inventory-only features: {sorted(inv_ids - set(features))[:6]}")

    reg_entries: dict[str, set] = {}
    for r in coverage.get("registrations", []):
        reg_entries.setdefault(r["entry_id"], set()).update(r.get("feature_ids") or [])
    check(set(entry_index) == set(reg_entries), "ENTRY-COVERAGE-BIDIR", "entrypoint_index",
          f"registered-but-unmapped entries: {sorted(set(reg_entries) - set(entry_index))[:6]}; "
          f"mapped-but-unregistered entries: {sorted(set(entry_index) - set(reg_entries))[:6]}")
    for entry, fids in sorted(reg_entries.items()):
        mapped = set(entry_index.get(entry, {}).get("feature_ids", []))
        check(mapped == set(fids), "ENTRY-COVERAGE-BIDIR", f"entry={entry}",
              f"registration features {sorted(set(fids) - mapped)} not mapped in ledger; "
              f"ledger-only features {sorted(mapped - set(fids))}")
    for entry, rec in sorted(entry_index.items()):
        for fid in rec.get("feature_ids", []):
            check(fid in features, "ENTRY-COVERAGE-BIDIR", f"entry={entry}",
                  f"references unknown feature '{fid}'")
        check(bool(rec.get("scenario_ids")), "ENTRY-COVERAGE-BIDIR", f"entry={entry}",
              "no scenario linked via features (entry lacks acceptance mapping)")

    ep_ids = {e["id"] for e in entrypoints_doc.get("entries", [])}
    check(set(amap.get("surfaces", {})) == ep_ids, "ENTRY-COVERAGE-BIDIR", "surfaces",
          f"ENTRYPOINTS.json surfaces missing from ledger: {sorted(ep_ids - set(amap.get('surfaces', {})))[:6]}; "
          f"ledger-only surfaces: {sorted(set(amap.get('surfaces', {})) - ep_ids)[:6]}")

    for fid, f in sorted(features.items()):
        has_scenario = bool(f.get("supplemental_scenario_ids") or f.get("acceptance_ids"))
        if f.get("kind") == "production":
            check(has_scenario, "ORPHAN-REQUIREMENT", f"feature={fid}",
                  "production feature has no supplemental scenario and no base acceptance")
        for sid in f.get("supplemental_scenario_ids", []):
            check(sid in scenarios, "ORPHAN-REQUIREMENT", f"feature={fid}",
                  f"references missing supplemental scenario '{sid}'")
    for sid, sc in sorted(scenarios.items()):
        if sc.get("kind") == "supplemental":
            fid = sc.get("feature_id")
            check(fid in features, "ORPHAN-REQUIREMENT", f"scenario={sid}",
                  f"references unknown feature '{fid}'")
            for st in sc.get("execution_stage_ids", []):
                check(st in STAGES, "STAGE-PREFIX", f"scenario={sid}",
                      f"unknown execution stage '{st}'")

    fsa_sup = {s["id"] for s in fsa.get("supplemental_scenarios", [])}
    ledger_sup = {sid for sid, sc in scenarios.items() if sc.get("kind") == "supplemental"}
    check(fsa_sup == ledger_sup, "SPEC-IMPORT-COMPLETE", "scenarios[kind=supplemental]",
          f"FSA supplemental missing from ledger: {sorted(fsa_sup - ledger_sup)[:6]}; "
          f"ledger supplemental not in FSA: {sorted(ledger_sup - fsa_sup)[:6]}")

    # ── 5. 状态与结果绑定（A13 核心） ───────────────────────────────────────
    results_by_aid: dict[str, list] = {}
    results_by_id: dict = {r.get("result_id"): r for r in results}
    for r in results:
        results_by_aid.setdefault(r.get("acceptance_id"), []).append(r)
    pass_scenarios = set()
    for sid, sc in sorted(scenarios.items()):
        st = sc.get("ledger_status")
        check(st in ALLOWED_LEDGER_STATUS, "STATUS-ENUM", f"scenario={sid}",
              f"illegal ledger_status '{st}'")
        rids = sc.get("result_ids") or []
        if st == "PASS":
            check(bool(rids), "STATUS-PASS-WITHOUT-RESULT", f"scenario={sid}",
                  f"field 'result_ids' empty: status=PASS requires >=1 result record "
                  f"(no-source PASS is rejected)")
            pass_scenarios.add(sid)
        if st in ("NOT_STARTED", "SPECIFIED_NOT_EXECUTED", "NOT_RUN_UNAUTHORIZED"):
            check(not rids, "STATUS-NOTSTARTED-WITH-RESULT", f"scenario={sid}",
                  f"status={st} but result_ids={rids} (future-stage scenario with result)")
        for rid in rids:
            if not check(any(r.get("result_id") == rid for r in results),
                         "STATUS-PASS-WITHOUT-RESULT", f"scenario={sid}",
                         f"result_ids references unknown result '{rid}'"):
                continue
            rec = results_by_id.get(rid)
            if st == "PASS":
                check(rec.get("status") == "PASS", "STATUS-RESULT-CONFLICT",
                      f"scenario={sid}",
                      f"result={rid} field 'status' is {rec.get('status')!r} but "
                      f"scenario ledger_status=PASS (PASS scenario requires every "
                      f"bound result to be PASS; real FAIL/BLOCKED must not be "
                      f"presented as passed)")
    for r in results:
        aid = r.get("acceptance_id")
        check(aid in scenarios, "STATUS-PASS-WITHOUT-RESULT", f"result={r.get('result_id')}",
              f"acceptance_id '{aid}' is not a ledger scenario (result without requirement)")

    # ── 6. 结果字段完整性（A13：精确字段） ──────────────────────────────────
    head = amap["basis"].get("head", "")
    for r in results:
        rid = r.get("result_id", "<no-result-id>")
        for field in RESULT_REQUIRED_FIELDS:
            v = r.get(field)
            empty = v is None or v == "" or v == [] or v == {}
            check(not empty, "RESULT-MISSING-FIELD", f"result={rid}",
                  f"field '{field}' is missing or empty")
        if r.get("status") is not None:
            check(r.get("status") in ALLOWED_RESULT_STATUS, "RESULT-STATUS-ENUM",
                  f"result={rid}",
                  f"field 'status' value {r.get('status')!r} not in allowed result "
                  f"status set {sorted(ALLOWED_RESULT_STATUS)} (a result may not invent "
                  f"ambiguous outcome values)")
        if r.get("tested_sha") is not None:
            check(bool(HEX40.match(str(r["tested_sha"]))), "RESULT-BAD-FIELD", f"result={rid}",
                  f"field 'tested_sha' value {r.get('tested_sha')!r} is not a 40-hex commit sha")
        if r.get("committed_in") is None:
            check(r.get("tested_sha") == head, "RESULT-BAD-FIELD", f"result={rid}",
                  "field 'committed_in' is null but tested_sha != basis.head "
                  f"(only current-task results at HEAD may be commit-pending; "
                  f"tested_sha={r.get('tested_sha')}, basis.head={head})")
        else:
            check(bool(HEX40.match(str(r["committed_in"]))), "RESULT-BAD-FIELD", f"result={rid}",
                  f"field 'committed_in' value {r.get('committed_in')!r} is not 40-hex")
        check(bool(ISO_TS.match(str(r.get("timestamp_utc", "")))), "RESULT-BAD-FIELD",
              f"result={rid}", f"field 'timestamp_utc' value {r.get('timestamp_utc')!r} not ISO-8601")
        check(isinstance(r.get("exit_code"), int), "RESULT-BAD-FIELD", f"result={rid}",
              f"field 'exit_code' value {r.get('exit_code')!r} is not an integer")
        dl = r.get("dependency_lock_hashes") or {}
        check("package-lock.json" in dl, "RESULT-MISSING-FIELD", f"result={rid}",
              "field 'dependency_lock_hashes.package-lock.json' missing")

    # COMMIT-PENDING-BASIS（窄负例）：仅当存在 committed_in=null 的待提交结果时启用。
    # 该态仅“任务已执行、交付尚待提交”的窗口合法；一旦提交使 HEAD 前移而账本未按获准
    # 路径重建重绑，basis.head 即落后于当前 HEAD——旧基准头+空 committed_in 不得继续
    # 冒充当前待提交态（R00 阶段修复 R1 针对此类假绿增补）。无待提交结果时本规则
    # 休眠，不改变既有 checks 计数与离线行为。
    pending_commit = [r for r in results if r.get("committed_in") is None]
    if pending_commit and HEX40.match(str(head)):
        proc = subprocess.run(["git", "-C", str(git_repo), "rev-parse", "HEAD"],
                              capture_output=True, text=True)
        current_head = proc.stdout.strip() if proc.returncode == 0 else ""
        check(bool(current_head) and head == current_head, "COMMIT-PENDING-BASIS",
              "basis.head",
              f"results {[r.get('result_id') for r in pending_commit][:6]} have "
              f"committed_in=null (commit-pending) but basis.head {head} != current "
              f"git HEAD {current_head or '<unresolvable>'} (stale commit-pending "
              f"state masquerading as current; rebuild via r00_t07_build_map.py to "
              f"bind the real commit)")

    # ── 7. 证据存在与哈希（A13：删日志/篡改） ───────────────────────────────
    for r in results:
        rid = r.get("result_id", "<no-result-id>")
        for ev in r.get("evidence", []):
            p = root / ev["path"]
            if not check(p.is_file(), "EVIDENCE-MISSING-FILE", f"result={rid}",
                         f"evidence file '{ev['path']}' does not exist (deleted log)"):
                continue
            actual = sha256_file(p)
            check(actual == ev.get("sha256"), "EVIDENCE-HASH-MISMATCH", f"result={rid}",
                  f"evidence '{ev['path']}' sha256 {actual} != recorded {ev.get('sha256')}")

    # ── 8. 退出码一致性（跳过/失败冒充 PASS；零退出冒充 FAIL） ──────────────
    for r in results:
        rid = r.get("result_id", "<no-result-id>")
        ec, st = r.get("exit_code"), r.get("status")
        if st == "PASS":
            check(ec == 0, "EXIT-CODE-CONFLICT", f"result={rid}",
                  f"status=PASS but raw exit_code={ec} (skipped/failed run must not be PASS)")
        if st == "FAIL" and isinstance(ec, int):
            check(ec != 0, "EXIT-CODE-CONFLICT", f"result={rid}",
                  f"status=FAIL but raw exit_code={ec} (a zero exit cannot evidence "
                  f"FAIL; the failing signal itself is required)")
        if isinstance(ec, int) and ec != 0:
            check(st in ("FAIL", "BLOCKED"), "EXIT-CODE-CONFLICT", f"result={rid}",
                  f"exit_code={ec} but status={st} (nonzero exit must be FAIL or BLOCKED)")

    # ── 9. 过期检测（源码变更 / 分支历史） ─────────────────────────────────
    for r in results:
        rid = r.get("result_id", "<no-result-id>")
        for sp, digest in (r.get("source_digests") or {}).items():
            check(sp != "docs/rust-tauri/R00/ACCEPTANCE_MAP.json", "STALE-SOURCE",
                  f"result={rid}",
                  f"monitored source '{sp}' is the ledger itself (self-reference forbidden)")
            p = root / sp
            if not check(p.is_file(), "STALE-SOURCE", f"result={rid}",
                         f"monitored source '{sp}' no longer exists"):
                continue
            actual = sha256_file(p)
            check(actual == digest, "STALE-SOURCE", f"result={rid}",
                  f"monitored source '{sp}' changed: sha256 {actual} != recorded {digest} "
                  f"(result is stale relative to source change; re-run required)")
        ts = r.get("tested_sha")
        if ts and HEX40.match(str(ts)):
            proc = subprocess.run(["git", "-C", str(git_repo), "merge-base",
                                   "--is-ancestor", ts, "HEAD"],
                                  capture_output=True)
            check(proc.returncode == 0, "STALE-BRANCH", f"result={rid}",
                  f"tested_sha {ts} is not an ancestor of current HEAD "
                  f"(result from a discarded/rebased line)")

    # ── 10. lockfile 与测试登记 ─────────────────────────────────────────────
    lock = root / "package-lock.json"
    if lock.is_file():
        lock_sha = sha256_file(lock)
        for r in results:
            dl = (r.get("dependency_lock_hashes") or {}).get("package-lock.json")
            check(dl == lock_sha, "LOCKFILE-MISMATCH", f"result={r.get('result_id')}",
                  f"recorded package-lock.json {dl} != current {lock_sha}")
    else:
        check(False, "MAP-INPUT", "package-lock.json", "missing under root")
    for t in tests:
        p = root / t["path"]
        if not check(p.is_file(), "TESTS-MISSING", f"test={t.get('test_id')}",
                     f"declared test '{t['path']}' does not exist"):
            continue
        check(sha256_file(p) == t.get("sha256"), "TESTS-HASH", f"test={t.get('test_id')}",
              f"test '{t['path']}' sha256 differs from recorded")
        for sid in t.get("scenario_ids", []):
            check(sid in scenarios, "TESTS-MISSING", f"test={t.get('test_id')}",
                  f"references unknown scenario '{sid}'")

    # ── 11. 阻塞登记 ────────────────────────────────────────────────────────
    blk = amap.get("blockers") or {}
    all_blk_scen: set[str] = set()
    for group in ("active", "anticipated", "conditional_authorization", "not_run_this_round"):
        for b in blk.get(group, []):
            for sid in b.get("scenario_ids", []):
                check(sid in scenarios, "BLOCKER-REGISTRY", f"blocker={b.get('blocker_id')}",
                      f"references unknown scenario '{sid}'")
                all_blk_scen.add(sid)
    cond = scenarios.get("R11-A14")
    if cond is not None:
        check(cond.get("requirement") != "CONDITIONAL_AUTHORIZATION"
              or cond.get("ledger_status") == "NOT_RUN_UNAUTHORIZED",
              "BLOCKER-REGISTRY", "scenario=R11-A14",
              f"conditional scenario must be NOT_RUN_UNAUTHORIZED when unauthorized, "
              f"got '{cond.get('ledger_status')}'")
    leaked = sorted(all_blk_scen & pass_scenarios)
    check(not leaked, "BLOCKER-REGISTRY", "blockers",
          f"scenarios listed in blockers but ledger_status=PASS: {leaked[:6]}")

    # ── 12. 计数与自评状态 ──────────────────────────────────────────────────
    c = amap.get("counts", {})
    recomputed = {
        "scenarios_total": len(scenarios),
        "tasks": len(tasks),
        "features_total": len(features),
        "entrypoints_registered": len(entry_index),
        "surfaces": len(amap.get("surfaces", {})),
        "tests": len(tests),
        "results": len(results),
        "base_scenarios": sum(1 for s in scenarios.values() if s.get("kind") == "base"),
        "supplemental_scenarios": sum(1 for s in scenarios.values() if s.get("kind") == "supplemental"),
        "t07_added_scenarios": sum(1 for s in scenarios.values() if s.get("kind") == "t07_added"),
        "features_production": sum(1 for f in features.values() if f.get("kind") == "production"),
    }
    for k, v in recomputed.items():
        check(c.get(k) == v, "COUNTS-MISMATCH", f"counts.{k}",
              f"declared {c.get(k)} != actual {v}")
    lss = amap.get("ledger_self_status")
    check(lss in ("READY_FOR_REVIEW", "IN_PROGRESS", "BLOCKED"), "LEDGER-SELF-STATUS",
          "ledger_self_status",
          f"executor may not self-accept; got '{lss}' (PASS/ACCEPTED is reserved for independent review)")

    # ── 输出 ────────────────────────────────────────────────────────────────
    for line in sorted(errors):
        print(line)
    if errors:
        print(f"LEDGER_INVALID errors={len(errors)} checks={checks}")
        return 1
    print(f"LEDGER_VALID checks={checks} scenarios={len(scenarios)} "
          f"results={len(results)} entries={len(entry_index)} "
          f"spec_source={'taskbook-strict' if catalog is not None else 'embedded-snapshot'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
