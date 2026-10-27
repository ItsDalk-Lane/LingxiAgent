#!/usr/bin/env python3
"""R00-T08｜R00-A16 基线交接可独立复查抽查器（结构校验模式）。

模拟「另一执行者只拿到交接与仓库」的复核：全部输入只来自 R00_HANDOFF.json 的
spot_checks 段与仓库本身，不依赖原执行者记忆。三个抽查点：
  1. feature：叶子功能 F-ID -> 盘点条目映射 -> 源码锚点行存在 -> 账本补充场景/任务链；
  2. data_chain：STORES 存储条目 -> 读写方文件存在 -> 夹具文件与固定 SHA-256 一致；
  3. test：交接记录的重跑命令证据日志存在、SHA-256 与交接固定值一致、且日志内容
     与预期（退出码/文件数/用例数）一致。真实重跑命令本身记录在交接文件的
     spot_checks.test.rerun_command（含环境变量），由独立验收者在其 shell 直接执行。

本脚本不执行任何外部进程；退出码：0=三个抽查点全部通过；1=任一失败；2=用法错误。
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
DEFAULT_HANDOFF = Path("docs/rust-tauri/R00/R00_HANDOFF.json")


def sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def load_repo_json(rel: str) -> dict:
    p = (REPO_ROOT / rel).resolve()
    if not p.is_relative_to(REPO_ROOT) or not p.is_file():
        raise SystemExit(f"repo path missing or outside repo: {rel}")
    return json.loads(p.read_text(encoding="utf-8"))


def check_feature(spec: dict) -> str:
    inv = load_repo_json(spec["inventory"])
    entry = next((f for f in inv["features"] if f["feature_id"] == spec["feature_id"]), None)
    assert entry is not None, f"feature {spec['feature_id']} not in inventory"
    for k, v in spec["expected_mapping"].items():
        assert entry.get(k) == v, f"feature mapping {k}: {entry.get(k)!r} != {v!r}"
    anchor = spec["source_anchor"]
    lines = (REPO_ROOT / anchor["file"]).read_text(encoding="utf-8").splitlines()
    actual = lines[anchor["line"] - 1]
    assert anchor["contains"] in actual, \
        f"anchor {anchor['file']}:{anchor['line']} = {actual!r} lacks {anchor['contains']!r}"
    ledger = load_repo_json(spec["ledger"])
    scen = ledger["scenarios"].get(spec["supplemental_acceptance_id"])
    assert scen is not None, f"scenario {spec['supplemental_acceptance_id']} missing in ledger"
    assert scen["kind"] == "supplemental" and scen["feature_id"] == spec["feature_id"], "scenario link mismatch"
    assert sorted(scen["task_ids"]) == sorted(spec["expected_mapping"]["task_ids"]), "scenario task link mismatch"
    fidx = ledger["features_index"].get(spec["feature_id"])
    assert fidx is not None and spec["supplemental_acceptance_id"] in fidx["supplemental_scenario_ids"], \
        "features_index backlink missing"
    return f"feature {spec['feature_id']} -> {anchor['file']}:{anchor['line']} -> {spec['supplemental_acceptance_id']} -> {scen['task_ids']}"


def check_data_chain(spec: dict) -> str:
    stores = load_repo_json(spec["stores"])
    entry = next((s for s in stores["stores"] if s["id"] == spec["store_id"]), None)
    assert entry is not None, f"store {spec['store_id']} not in STORES"
    for rel in spec["site_rule_files"]:
        assert (REPO_ROOT / rel).is_file(), f"site rule file missing: {rel}"
    for rel, want in spec["fixture_sha256"].items():
        p = (REPO_ROOT / rel).resolve()
        assert p.is_relative_to(REPO_ROOT) and p.is_file(), f"fixture missing: {rel}"
        got = sha256_file(p)
        assert got == want, f"fixture {rel} sha256 {got} != pinned {want}"
    manifest = load_repo_json(spec["fixture_manifest"])
    fx = next((f for f in manifest["fixtures"] if f["fixture_id"] == spec["manifest_fixture_id"]), None)
    assert fx is not None, f"fixture_id {spec['manifest_fixture_id']} not in manifest"
    assert fx["input"] == spec["expected_manifest_input"] and fx["expected"] == spec["expected_manifest_expected"], \
        "manifest input/expected path drift"
    return f"store {spec['store_id']} ({len(spec['site_rule_files'])} site files) -> fixture {spec['manifest_fixture_id']} digests match"


def check_test(spec: dict) -> str:
    for rel, want in spec["evidence_log_sha256"].items():
        p = (REPO_ROOT / rel).resolve()
        assert p.is_relative_to(REPO_ROOT) and p.is_file(), f"evidence log missing: {rel}"
        got = sha256_file(p)
        assert got == want, f"evidence log {rel} sha256 {got} != pinned {want}"
        text = p.read_text(encoding="utf-8")
        m_files = re.search(r"Test Files\s+(\d+) passed", text)
        m_tests = re.search(r"Tests\s+(\d+) passed", text)
        assert m_files and int(m_files.group(1)) == spec["expected_test_files_passed"], \
            f"{rel}: test files passed mismatch"
        assert m_tests and int(m_tests.group(1)) == spec["expected_tests_passed"], \
            f"{rel}: tests passed mismatch"
        assert spec["expected_exit_marker"] in text, f"{rel}: exit marker missing"
    return (f"test evidence {len(spec['evidence_log_sha256'])} log(s) digest+content verified; "
            f"rerun_command='{spec['rerun_command']}'")


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--handoff", default=str(DEFAULT_HANDOFF))
    args = ap.parse_args(argv[1:])
    handoff_path = (REPO_ROOT / args.handoff).resolve()
    if not handoff_path.is_relative_to(REPO_ROOT) or not handoff_path.is_file():
        print(f"handoff missing or outside repo: {args.handoff}", file=sys.stderr)
        return 2
    handoff = json.loads(handoff_path.read_text(encoding="utf-8"))
    spots = handoff["spot_checks"]
    failed = 0
    for name, fn in (("feature", check_feature), ("data_chain", check_data_chain), ("test", check_test)):
        try:
            print(f"A16-{name}: PASS ({fn(spots[name])})")
        except AssertionError as e:
            print(f"A16-{name}: FAIL {e}")
            failed += 1
        except Exception as e:  # noqa: BLE001 — 抽查器需把任意复核错误转为非零退出
            print(f"A16-{name}: ERROR {type(e).__name__}: {e}")
            failed += 1
    if failed:
        print(f"A16-SPOTCHECK-FAILED failures={failed}")
        return 1
    print(f"A16-SPOTCHECK-PASSED handoff_sha256={sha256_file(handoff_path)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
