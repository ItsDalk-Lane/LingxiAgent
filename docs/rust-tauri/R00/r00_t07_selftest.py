#!/usr/bin/env python3
"""R00-T07：A13/A14 负向与增删反例自测（在隔离副本上执行，不触碰真实仓库文件）。

用法：
  python3 -B docs/rust-tauri/R00/r00_t07_selftest.py [--suite a13|a14|all] [--keep-temp]

每个变体：从真实仓库复制账本+伴生输入+全部被引用证据/源码/测试/lockfile/任务书到
临时隔离根 → 施加单一篡改 → 在隔离根上运行账本校验器（进程内调用
r00_t07_validate_ledger.py 的 main()，等效 argv：--root <隔离根> --git-repo <真实仓库>；
退出码即校验器返回值）→ 断言退出码与错误行（精确到 ID 与字段）→ 逐变体日志与汇总
落盘 artifacts/rust-tauri/R00/T07/。全变体符合预期时本脚本退出码 0。

A13 规格映射：账本某场景 ledger_status 手改为 PASS 且删除日志 → 校验器非零退出、
精确指出场景 ID 与缺失字段（canonical 变体 + 同根因变体 v1–v9b）。
A14 规格映射：入口盘点增加一个真实形态生产入口但不映射任务/场景 → 检查失败；
补齐 feature→scenario→task 映射后 → 通过（v1–v4）。
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import io
import json
import shutil
import sys
import tempfile
from contextlib import redirect_stdout
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
VALIDATOR = HERE / "r00_t07_validate_ledger.py"
R00 = ROOT / "docs/rust-tauri/R00"
ART = ROOT / "artifacts/rust-tauri/R00/T07"
TASKBOOK_REL = "Lingxi_Rust_Tauri_Taskbooks_2026-09-23"

MAP_REL = "docs/rust-tauri/R00/ACCEPTANCE_MAP.json"


def sha256_file(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


def now_utc() -> str:
    return datetime.now(timezone.utc).isoformat()


def resolve_git_head(repo: Path) -> str:
    """纯 Python 解析 HEAD（读 .git/HEAD 与 refs；不执行任何子进程）。"""
    git = repo / ".git"
    if git.is_file():  # worktree 指针
        target = Path(git.read_text(encoding="utf-8").strip().split("gitdir:", 1)[-1].strip())
        if not target.is_absolute():
            target = repo / target
    else:
        target = git
    head_text = (target / "HEAD").read_text(encoding="utf-8").strip()
    if head_text.startswith("ref:"):
        ref = head_text.split("ref:", 1)[1].strip()
        ref_file = target / ref
        if ref_file.is_file():
            return ref_file.read_text(encoding="utf-8").strip()
        packed = (target / "packed-refs").read_text(encoding="utf-8")
        for line in packed.splitlines():
            if line.endswith(" " + ref):
                return line.split()[0]
        raise RuntimeError("cannot resolve git ref " + ref)
    return head_text


def load_validator_module():
    spec = importlib.util.spec_from_file_location("r00_t07_validate_ledger", VALIDATOR)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


VALIDATOR_MODULE = None


def run_validator(fake_root: Path, real_repo: Path):
    """进程内运行校验器；返回 (exit_code, stdout, stderr='')。

    等效命令行：python3 -B r00_t07_validate_ledger.py --root FAKE --git-repo REAL。
    校验器内部对错误输出使用 print（stdout）；此处捕获为字符串。
    """
    global VALIDATOR_MODULE
    if VALIDATOR_MODULE is None:
        VALIDATOR_MODULE = load_validator_module()
    VALIDATOR_MODULE.errors.clear()
    VALIDATOR_MODULE.checks = 0
    old_argv = sys.argv
    sys.argv = ["r00_t07_validate_ledger.py",
                "--root", str(fake_root), "--git-repo", str(real_repo)]
    buf = io.StringIO()
    try:
        with redirect_stdout(buf):
            code = VALIDATOR_MODULE.main()
    finally:
        sys.argv = old_argv
    return code, buf.getvalue(), ""


class FakeRoot:
    """隔离副本：只复制校验器会读到的文件。"""

    def __init__(self, name: str, files: list):
        self.dir = Path(tempfile.mkdtemp(prefix="r00t07-" + name + "-"))
        self.name = name
        self.copied = 0
        for rel in sorted(set(files)):
            src = ROOT / rel
            if not src.is_file():
                raise RuntimeError("selftest setup: missing source file " + rel)
            dst = self.dir / rel
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dst)
            self.copied += 1

    def path(self, rel: str) -> Path:
        return self.dir / rel

    def load_json(self, rel: str):
        return json.loads(self.path(rel).read_text(encoding="utf-8"))

    def write_json(self, rel: str, obj) -> None:
        self.path(rel).write_text(
            json.dumps(obj, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")

    def check(self, real_repo: Path):
        return run_validator(self.dir, real_repo)


def files_needed_for_fake_root() -> list:
    amap = json.loads((ROOT / MAP_REL).read_text(encoding="utf-8"))
    files = [MAP_REL,
             "docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json",
             "docs/rust-tauri/R00/ENTRYPOINT_COVERAGE.json",
             "docs/rust-tauri/R00/FEATURE_INVENTORY.json",
             "docs/rust-tauri/R00/ENTRYPOINTS.json",
             "package-lock.json",
             TASKBOOK_REL + "/acceptance-catalog.json",
             TASKBOOK_REL + "/task-catalog.json"]
    for r in amap["results"]:
        files.extend(ev["path"] for ev in r.get("evidence", []))
        files.extend((r.get("source_digests") or {}).keys())
    for t in amap["tests"]:
        files.append(t["path"])
    for rel in files:
        if not (ROOT / rel).is_file():
            raise RuntimeError("map references missing file for fake root: " + rel)
    return files


def first_supplemental(amap) -> str:
    return sorted(sid for sid, sc in amap["scenarios"].items()
                  if sc.get("kind") == "supplemental")[0]


# ── A13 变体 ────────────────────────────────────────────────────────────────

def tamper_canonical(fr: FakeRoot):
    """规格正例：场景状态手改为 PASS 且删除日志。"""
    amap = fr.load_json(MAP_REL)
    sid = first_supplemental(amap)
    amap["scenarios"][sid]["ledger_status"] = "PASS"
    fr.write_json(MAP_REL, amap)
    deleted = "artifacts/rust-tauri/R00/T01/startup-probe.json"
    fr.path(deleted).unlink()
    return sid, deleted


def tamper_v1(fr: FakeRoot):
    amap = fr.load_json(MAP_REL)
    amap["scenarios"]["R01-A01"]["ledger_status"] = "PASS"
    fr.write_json(MAP_REL, amap)


def tamper_v2(fr: FakeRoot):
    fr.path("artifacts/rust-tauri/R00/T01/startup-probe.json").unlink()


def tamper_v3(fr: FakeRoot):
    p = fr.path("artifacts/rust-tauri/R00/T05/REPLAY_SUMMARY.json")
    p.write_bytes(p.read_bytes() + b"\n")


def tamper_v4(fr: FakeRoot):
    amap = fr.load_json(MAP_REL)
    for r in amap["results"]:
        if r["result_id"] == "RES-R00-A03":
            del r["tested_sha"]
        if r["result_id"] == "RES-R00-A07":
            del r["exit_code"]
    fr.write_json(MAP_REL, amap)


def tamper_v5(fr: FakeRoot):
    p = fr.path("scripts/rust-tauri/r00-t05-replay.mjs")
    p.write_text(p.read_text(encoding="utf-8") + "\n// tampered\n", encoding="utf-8")


def tamper_v6(fr: FakeRoot):
    amap = fr.load_json(MAP_REL)
    for r in amap["results"]:
        if r["result_id"] == "RES-R00-A10":
            r["exit_code"] = 1
    fr.write_json(MAP_REL, amap)


def tamper_v7(fr: FakeRoot):
    p = fr.path(MAP_REL)
    text = p.read_text(encoding="utf-8")
    needle = '"R00-A01": {'
    dup = ('"R00-A01": {"kind": "base", "ledger_status": "NOT_STARTED", "result_ids": []},\n'
           '    "R00-A01": {')
    if needle not in text:
        raise RuntimeError("v7 needle not found")
    p.write_text(text.replace(needle, dup, 1), encoding="utf-8")


def tamper_v8(fr: FakeRoot):
    amap = fr.load_json(MAP_REL)
    del amap["scenarios"]["R00-A03"]
    fr.write_json(MAP_REL, amap)


def tamper_v9(fr: FakeRoot):
    amap = fr.load_json(MAP_REL)
    amap["scenarios"]["R00-A04"]["spec"]["then"] = \
        amap["scenarios"]["R00-A04"]["spec"]["then"] + "（被篡改）"
    fr.write_json(MAP_REL, amap)


def _set_result_status(fr: FakeRoot, rid: str, status: str):
    amap = fr.load_json(MAP_REL)
    for r in amap["results"]:
        if r["result_id"] == rid:
            r["status"] = status
    fr.write_json(MAP_REL, amap)


def tamper_v10(fr: FakeRoot):
    """R1 反例①+③：场景保持 PASS，绑定结果 status 改为 FAIL（exit_code 仍 0）。
    应同时触发 STATUS-RESULT-CONFLICT 与 EXIT-CODE-CONFLICT（同一篡改的两面）。"""
    _set_result_status(fr, "RES-R00-A09", "FAIL")


def tamper_v11(fr: FakeRoot):
    """R1 反例②：result.status 写入非法枚举值 MAYBE_PASSED。"""
    _set_result_status(fr, "RES-R00-A09", "MAYBE_PASSED")


def tamper_v12(fr: FakeRoot):
    """R1 反例③的规则隔离态：场景与结果一致地改为 FAIL（无场景↔结果矛盾），
    但 exit_code 仍为 0——仅 EXIT-CODE-CONFLICT（FAIL 分支）应拒绝。"""
    amap = fr.load_json(MAP_REL)
    amap["scenarios"]["R00-A09"]["ledger_status"] = "FAIL"
    fr.write_json(MAP_REL, amap)
    _set_result_status(fr, "RES-R00-A09", "FAIL")


# ── A14 变体 ────────────────────────────────────────────────────────────────

NEW_ENTRY = "cli:lingxi-doctor"
NEW_FEATURE = "F-D99-CLI-DOCTOR"
NEW_SCENARIO = "R00-T07-LA-A14SIM-CLIDOCTOR"


def _add_entry_to_coverage(fr: FakeRoot):
    cov = fr.load_json("docs/rust-tauri/R00/ENTRYPOINT_COVERAGE.json")
    cov["registrations"].append({
        "entry_id": NEW_ENTRY, "feature_id": NEW_FEATURE,
        "feature_ids": [NEW_FEATURE],
        "source_ref": "cli/index.js:1"})
    fr.write_json("docs/rust-tauri/R00/ENTRYPOINT_COVERAGE.json", cov)


def _add_feature_to_inventory(fr: FakeRoot):
    inv = fr.load_json("docs/rust-tauri/R00/FEATURE_INVENTORY.json")
    inv["features"].append({
        "feature_id": NEW_FEATURE, "entry_id": NEW_ENTRY, "kind": "cli",
        "parent_domain": "D20", "title": "CLI 体检命令（自测模拟新增入口）",
        "classification": "保留", "user_action": "运行 lingxi doctor",
        "visible_result": "输出环境体检结果", "current_entrypoints": [NEW_ENTRY],
        "current_owners": ["cli"], "current_stores": [], "source_ref": "cli/index.js:1",
        "reachability": "production", "target_owner": "Rust service + CLI"})
    fr.write_json("docs/rust-tauri/R00/FEATURE_INVENTORY.json", inv)


def tamper_a14_v1(fr: FakeRoot):
    _add_entry_to_coverage(fr)
    _add_feature_to_inventory(fr)


def tamper_a14_v2(fr: FakeRoot):
    tamper_a14_v1(fr)
    amap = fr.load_json(MAP_REL)
    amap["features_index"][NEW_FEATURE] = {
        "title": "CLI 体检命令（自测模拟新增入口）", "parent_domain": "D20",
        "classification": "保留", "kind": "production", "stage_ids": ["R07"],
        "task_ids": [], "acceptance_ids": [], "supplemental_scenario_ids": [],
        "entrypoints": [NEW_ENTRY]}
    fr.write_json(MAP_REL, amap)


def tamper_a14_v3(fr: FakeRoot):
    tamper_a14_v2(fr)
    amap = fr.load_json(MAP_REL)
    amap["scenarios"][NEW_SCENARIO] = {
        "kind": "t07_added", "stage_id": "R00",
        "execution_stage_ids": ["R07"], "task_ids": ["R07-T08"],
        "requirement": "REQUIRED_ADDED",
        "name": "A14 补齐映射模拟场景：CLI doctor",
        "given": "新入口 cli:lingxi-doctor 已登记", "when": "通过 CLI 运行 doctor",
        "then": "输出环境体检结果且经统一服务边界",
        "formalization_task_id": "R00-T07",
        "ledger_status": "NOT_STARTED", "result_ids": [],
        "feature_ids": [NEW_FEATURE], "test_ids": []}
    amap["features_index"][NEW_FEATURE]["task_ids"] = ["R07-T08"]
    amap["features_index"][NEW_FEATURE]["supplemental_scenario_ids"] = [NEW_SCENARIO]
    amap["entrypoint_index"][NEW_ENTRY] = {
        "feature_ids": [NEW_FEATURE], "source_refs": ["cli/index.js:1"],
        "scenario_ids": [NEW_SCENARIO]}
    amap["tasks"]["R07-T08"]["scenario_ids"] = sorted(
        set(amap["tasks"]["R07-T08"]["scenario_ids"]) | {NEW_SCENARIO})
    c = amap["counts"]
    c["scenarios_total"] += 1
    c["t07_added_scenarios"] += 1
    c["features_total"] += 1
    c["features_production"] += 1
    c["entrypoints_registered"] += 1
    # 获准路径模拟：盘点文件更新后，以其为证据/受监控源码的历史结果须重新绑定摘要
    # （等效于重跑 r00_t02_inventory.py 扫描与 T07 自测后重建账本）；只改映射不改摘要
    # 会被 EVIDENCE-HASH-MISMATCH / STALE-SOURCE 拒绝（见 v1/v2 的失败输出）。
    touched_files = ["docs/rust-tauri/R00/ENTRYPOINT_COVERAGE.json",
                     "docs/rust-tauri/R00/FEATURE_INVENTORY.json"]
    for rid in ("RES-R00-A03", "RES-R00-A04", "RES-R00-A13", "RES-R00-A14"):
        for r in amap["results"]:
            if r["result_id"] == rid:
                for ev in r["evidence"]:
                    if ev["path"] in touched_files:
                        ev["sha256"] = sha256_file(fr.path(ev["path"]))
                for sp in touched_files:
                    if sp in (r.get("source_digests") or {}):
                        r["source_digests"][sp] = sha256_file(fr.path(sp))
    fr.write_json(MAP_REL, amap)


def tamper_a14_v4(fr: FakeRoot):
    amap = fr.load_json(MAP_REL)
    amap["entrypoint_index"]["cli:ghost"] = {
        "feature_ids": ["F-D17-BRIDGE_ADAPTER-BRIDGE-DINGTALK-244169"],
        "source_refs": ["lib/bridge/bridge-manager.ts:129"],
        "scenario_ids": amap["entrypoint_index"]["bridge:dingtalk"]["scenario_ids"]}
    amap["counts"]["entrypoints_registered"] += 1
    fr.write_json(MAP_REL, amap)


# ── 变体注册表 ──────────────────────────────────────────────────────────────

def build_variants() -> dict:
    a13 = {
        "canonical": (tamper_canonical, 1, [
            "LEDGER-ERROR STATUS-PASS-WITHOUT-RESULT scenario=",
            "field 'result_ids'",
            "LEDGER-ERROR EVIDENCE-MISSING-FILE result=RES-R00-A01",
            "artifacts/rust-tauri/R00/T01/startup-probe.json",
        ], "场景手改 PASS 且删除日志：非零退出并精确指出场景 ID、结果 ID、缺失字段与证据路径"),
        "v1-flip-no-result": (tamper_v1, 1, [
            "LEDGER-ERROR STATUS-PASS-WITHOUT-RESULT scenario=R01-A01",
            "field 'result_ids'",
        ], "NOT_STARTED 场景手改 PASS 无结果记录"),
        "v2-evidence-deleted": (tamper_v2, 1, [
            "LEDGER-ERROR EVIDENCE-MISSING-FILE result=RES-R00-A01",
            "artifacts/rust-tauri/R00/T01/startup-probe.json",
        ], "删除已 PASS 结果引用的证据文件"),
        "v3-evidence-tampered": (tamper_v3, 1, [
            "LEDGER-ERROR EVIDENCE-HASH-MISMATCH result=RES-R00-A09",
            "artifacts/rust-tauri/R00/T05/REPLAY_SUMMARY.json",
        ], "篡改证据文件内容（哈希不符）"),
        "v4-missing-field": (tamper_v4, 1, [
            "LEDGER-ERROR RESULT-MISSING-FIELD result=RES-R00-A03 field 'tested_sha'",
            "LEDGER-ERROR RESULT-MISSING-FIELD result=RES-R00-A07 field 'exit_code'",
        ], "删除结果关键字段（tested_sha / exit_code）"),
        "v5-stale-source": (tamper_v5, 1, [
            "LEDGER-ERROR STALE-SOURCE result=RES-R00-A09",
            "scripts/rust-tauri/r00-t05-replay.mjs",
        ], "结果监控的源码在运行后被修改（结果过期）"),
        "v6-exitcode-conflict": (tamper_v6, 1, [
            "LEDGER-ERROR EXIT-CODE-CONFLICT result=RES-R00-A10",
        ], "exit_code=1 却保持 status=PASS（跳过/失败冒充通过）"),
        "v7-duplicate-id": (tamper_v7, 1, [
            "LEDGER-ERROR NO-DUPLICATE-ID",
        ], "账本 JSON 重复场景键"),
        "v8-task-ref-missing": (tamper_v8, 1, [
            "LEDGER-ERROR TASK-SCENARIO-BIJCTION task=R00-T02",
            "acceptance 'R00-A03' missing",
        ], "任务声明的验收场景从账本消失"),
        "v9-spec-tamper-strict": (tamper_v9, 1, [
            "LEDGER-ERROR SPEC-FIDELITY scenario=R00-A04",
            "spec.then",
        ], "篡改场景规格文本（任务书严格模式，字段级定位）"),
        "v10-scenario-result-conflict": (tamper_v10, 1, [
            "LEDGER-ERROR STATUS-RESULT-CONFLICT scenario=R00-A09 result=RES-R00-A09",
            "field 'status' is 'FAIL'",
            "LEDGER-ERROR EXIT-CODE-CONFLICT result=RES-R00-A09",
            "status=FAIL but raw exit_code=0",
        ], "场景 PASS 但绑定结果 status=FAIL（R1 反例①/③状态：状态脱节与零退出同报）"),
        "v11-result-status-enum": (tamper_v11, 1, [
            "LEDGER-ERROR RESULT-STATUS-ENUM result=RES-R00-A09",
            "'MAYBE_PASSED'",
            "LEDGER-ERROR STATUS-RESULT-CONFLICT scenario=R00-A09 result=RES-R00-A09",
        ], "result.status 写入非法枚举 MAYBE_PASSED（R1 反例②）"),
        "v12-fail-exit-zero": (tamper_v12, 1, [
            "LEDGER-ERROR EXIT-CODE-CONFLICT result=RES-R00-A09",
            "status=FAIL but raw exit_code=0",
        ], "场景与结果一致 FAIL 但 exit_code=0（R1 反例③规则隔离：仅退出码矛盾）"),
    }
    a14 = {
        "v1-entry-unmapped": (tamper_a14_v1, 1, [
            "LEDGER-ERROR ENTRY-COVERAGE-BIDIR entry=" + NEW_ENTRY,
            "not mapped in ledger",
            "LEDGER-ERROR FEATURE-INVENTORY-BIDIR features_index",
            NEW_FEATURE,
        ], "入口盘点新增真实形态入口与功能、账本未映射 → 失败"),
        "v2-feature-no-scenario": (tamper_a14_v2, 1, [
            "LEDGER-ERROR ORPHAN-REQUIREMENT feature=" + NEW_FEATURE,
            "no supplemental scenario",
            "LEDGER-ERROR ENTRY-COVERAGE-BIDIR entry=" + NEW_ENTRY,
        ], "新功能入账本 feature 索引但无场景/任务映射 → 失败"),
        "v3-mapped-passes": (tamper_a14_v3, 0, [
            "LEDGER_VALID",
        ], "补齐 feature→scenario→task 完整映射 → 通过"),
        "v4-orphan-mapping": (tamper_a14_v4, 1, [
            "LEDGER-ERROR ENTRY-COVERAGE-BIDIR entrypoint_index",
            "cli:ghost",
        ], "账本映射盘点不存在的入口（孤立映射）→ 失败"),
    }
    return {"a13": a13, "a14": a14}


def write_variant_log(art_dir: Path, log_rel: str, header: str, out: str, errout: str) -> None:
    (art_dir / log_rel).write_text(
        header + "# --- stdout ---\n" + out +
        "# --- stderr ---\n" + (errout or "(empty)") + "\n", encoding="utf-8")


def main() -> int:
    ap = argparse.ArgumentParser(description="R00-T07 ledger selftest (A13/A14)")
    ap.add_argument("--suite", choices=["a13", "a14", "all"], default="all")
    ap.add_argument("--keep-temp", action="store_true")
    ap.add_argument("--art-dir", default=None,
                    help="自测证据输出目录（默认 artifacts/rust-tauri/R00/T07；"
                         "确认重跑时用独立目录避免覆盖已嵌入账本的证据）")
    args = ap.parse_args()

    art_dir = Path(args.art_dir) if args.art_dir else ART
    art_dir.mkdir(parents=True, exist_ok=True)
    # 本目录为 T07 自测专属：清理上一轮自测输出，保证落盘证据只反映本轮
    for pattern in ("a13-*.log", "a14-*.log", "positive-control.log",
                    "SELFTEST_SUMMARY.json", "a13-canonical-detail.json",
                    "a14-canonical-detail.json", "selftest-confirm-rerun.log",
                    "CONFIRM_RERUN.json"):
        for stale in art_dir.glob(pattern):
            stale.unlink()
    base_files = files_needed_for_fake_root()
    head = resolve_git_head(ROOT)
    variants = build_variants()
    suites_to_run = ["a13", "a14"] if args.suite == "all" else [args.suite]

    summary = {
        "generated_at": now_utc(),
        "validated_repo_head": head,
        "validator_sha256": sha256_file(VALIDATOR),
        "validator_path": str(VALIDATOR.relative_to(ROOT)),
        "invocation_mode": "in-process import of r00_t07_validate_ledger.main() "
                           "(equivalent argv: --root <isolated copy> --git-repo <real repo>)",
        "fake_root_file_count": len(set(base_files)),
        "temp_roots_created": 0,
        "temp_roots_removed": 0,
        "suites": {},
        "all_passed": False,
    }

    canonical_sid = canonical_deleted = None
    all_ok = True
    for suite_name in suites_to_run:
        suite = {"started_utc": now_utc(), "variants": []}
        for vid, (tamper, expect_exit, substrs, desc) in variants[suite_name].items():
            fr = FakeRoot(suite_name + "-" + vid, base_files)
            summary["temp_roots_created"] += 1
            try:
                extra_note = ""
                if vid == "canonical":
                    canonical_sid, canonical_deleted = tamper(fr)
                    extra_note = "flipped scenario " + canonical_sid + "; deleted " + canonical_deleted
                else:
                    tamper(fr)
                code, out, errout = fr.check(ROOT)
                missing = [s for s in substrs if s not in out]
                ok = (code == expect_exit) and not missing
                log_rel = suite_name + "-" + vid + ".log"
                header = ("# variant " + suite_name + "/" + vid + ": " + desc + "\n"
                          "# tamper: " + (extra_note or tamper.__name__) + "\n"
                          "# validator: r00_t07_validate_ledger.py --root <isolated:" + fr.name
                          + "> --git-repo <real repo>\n"
                          "# expect_exit=" + str(expect_exit) + " actual_exit=" + str(code)
                          + " substr_matched=" + str(len(substrs) - len(missing))
                          + "/" + str(len(substrs)) + "\n")
                write_variant_log(art_dir, log_rel, header, out, errout)
                suite["variants"].append({
                    "variant_id": vid, "description": desc,
                    "expect_exit": expect_exit, "actual_exit": code,
                    "expected_substrings": substrs, "missing_substrings": missing,
                    "passed": ok, "log": log_rel,
                    "fake_root": str(fr.dir)})
                if vid == "canonical":
                    (art_dir / (suite_name + "-canonical-detail.json")).write_text(
                        json.dumps({
                            "flipped_scenario": canonical_sid,
                            "deleted_evidence": canonical_deleted,
                            "validator_exit": code,
                            "missing_substrings": missing,
                            "log": log_rel}, ensure_ascii=False, indent=1) + "\n",
                        encoding="utf-8")
                all_ok = all_ok and ok
            finally:
                if not args.keep_temp:
                    shutil.rmtree(fr.dir, ignore_errors=True)
                    summary["temp_roots_removed"] += 1
        suite["ended_utc"] = now_utc()
        suite["variants_total"] = len(suite["variants"])
        suite["variants_passed"] = sum(1 for v in suite["variants"] if v["passed"])
        summary["suites"][suite_name] = suite

    # 无任务书目录时的内嵌快照保真回退（A13 同根因补充变体 v9b）
    if "a13" in suites_to_run:
        fr = FakeRoot("a13-v9b-digest-fallback", base_files)
        summary["temp_roots_created"] += 1
        try:
            shutil.rmtree(fr.path(TASKBOOK_REL))
            tamper_v9(fr)
            code, out, errout = fr.check(ROOT)
            substrs = ["LEDGER-ERROR SPEC-FIDELITY scenario=R00-A04", "tampered"]
            missing = [s for s in substrs if s not in out]
            ok = code == 1 and not missing
            header = ("# variant a13/v9b-digest-fallback: 无任务书目录时用内嵌 spec_digests "
                      "快照检测规格篡改\n# expect_exit=1 actual_exit=" + str(code) + "\n")
            write_variant_log(art_dir, "a13-v9b-digest-fallback.log", header, out, errout)
            summary["suites"]["a13"]["variants"].append({
                "variant_id": "v9b-digest-fallback",
                "description": "篡改规格文本（内嵌快照回退模式，无任务书目录）",
                "expect_exit": 1, "actual_exit": code,
                "expected_substrings": substrs, "missing_substrings": missing,
                "passed": ok, "log": "a13-v9b-digest-fallback.log",
                "fake_root": "(removed)"})
            summary["suites"]["a13"]["variants_total"] += 1
            if ok:
                summary["suites"]["a13"]["variants_passed"] += 1
            all_ok = all_ok and ok
        finally:
            if not args.keep_temp:
                shutil.rmtree(fr.dir, ignore_errors=True)
                summary["temp_roots_removed"] += 1

    # 正面对照：未篡改隔离副本必须 LEDGER_VALID（防校验器"永远失败"取巧）
    fr = FakeRoot("positive-control", base_files)
    summary["temp_roots_created"] += 1
    try:
        code, out, errout = fr.check(ROOT)
        ok = code == 0 and "LEDGER_VALID" in out
        header = ("# positive control: 未篡改隔离副本必须通过\n"
                  "# expect_exit=0 actual_exit=" + str(code) + "\n")
        write_variant_log(art_dir, "positive-control.log", header, out, errout)
        summary["positive_control"] = {"expect_exit": 0, "actual_exit": code,
                                       "passed": ok, "log": "positive-control.log"}
        all_ok = all_ok and ok
    finally:
        if not args.keep_temp:
            shutil.rmtree(fr.dir, ignore_errors=True)
            summary["temp_roots_removed"] += 1

    # 规范名别名日志：与 R00-T07_RESULTS.json 引用的证据路径一致
    if "a13" in suites_to_run and (art_dir / "a13-canonical.log").is_file():
        (art_dir / "a13-negative-flip-status.log").write_text(
            (art_dir / "a13-canonical.log").read_text(encoding="utf-8"), encoding="utf-8")
    if "a14" in suites_to_run and (art_dir / "a14-v1-entry-unmapped.log").is_file():
        parts = []
        for src in ("a14-v1-entry-unmapped.log", "a14-v3-mapped-passes.log"):
            p = ART / src
            if p.is_file():
                parts.append(p.read_text(encoding="utf-8"))
        (art_dir / "a14-entry-add-remove.log").write_text(
            "# A14 增删反例：先未映射（失败），补齐映射后（通过）\n" +
            "\n".join(parts), encoding="utf-8")

    summary["all_passed"] = all_ok
    (art_dir / "SELFTEST_SUMMARY.json").write_text(
        json.dumps(summary, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")

    total = sum(s["variants_total"] for s in summary["suites"].values())
    passed = sum(s["variants_passed"] for s in summary["suites"].values())
    print("SELFTEST " + ("PASS" if all_ok else "FAIL") +
          " variants=" + str(passed) + "/" + str(total) +
          " (+positive-control " + ("ok" if summary["positive_control"]["passed"] else "FAILED") + ")" +
          " temp_removed=" + str(summary["temp_roots_removed"]) + "/" + str(summary["temp_roots_created"]))
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main())
