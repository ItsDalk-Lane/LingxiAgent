#!/usr/bin/env python3
"""R09 地图/证据合同完整性负例（真实调用 `xtask check-map`，非自造宽松通道）。

目的：证明 R09 正式门消费的地图合同不可被以下方式悄悄放宽/缩水：
  缺叶、删整任务份额、编造叶、改写原断言、改写基础场景镜像、孤儿命令、
  引用未注册命令、空绑定场景、逐断言覆盖不完整、命令无产物、源码漂移。

做法：把损坏样本写到隔离临时目录（绝不改仓库地图），逐个喂给与 `verify-stage`
共用 cross-check 的 `xtask check-map`；每例必须非零且带预期的拒绝原因。源码漂移
负例临时新增一个 `xtask/src` 下的 `.json` 文件再删除，并在前后核对源码树摘要，
证明还原成功（非零失败 → 还原通过证据）。最后对原地图再跑一次正例。

输出（供阶段门消费）：
  result.json（lingxi.r09.map-integrity.v1）
  map-integrity-cases.json（lingxi.leaf-case-results.v1）
  output.log
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
ROOT = SCRIPTS.parents[1]
MAP_PATH = ROOT / "rust/crates/xtask/src/stage_maps/R09.json"
XTASK = ROOT / "rust/target/debug/xtask"
ACCEPTANCE_PATH = ROOT / "docs/rust-tauri/R00/ACCEPTANCE_MAP.json"
FEATURE_PATH = ROOT / "docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json"

CASES: list[dict] = []
LOGS: list[str] = []


def case(name: str, ok: bool, detail=None) -> bool:
    CASES.append({"case": name, "ok": bool(ok), "actual": 1 if ok else 0, "detail": detail})
    return bool(ok)


def log(message: str) -> None:
    print(message, flush=True)
    LOGS.append(message)


def sha256(path: Path) -> str:
    import hashlib
    return hashlib.sha256(path.read_bytes()).hexdigest()


def xtask_check_map(map_file: Path | None = None) -> subprocess.CompletedProcess:
    argv = [str(XTASK), "check-map", "R09"]
    if map_file is not None:
        argv += ["--map", str(map_file)]
    process = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True, timeout=120)
    log(f"$ {' '.join(argv)}\n  exit={process.returncode}\n  "
        f"{(process.stdout + process.stderr).strip()[:1500]}")
    return process


def independent_ledger_facts() -> dict:
    """独立从冻结原账提取预期集合（不读阶段图、不复用生成器实现）。"""
    acceptance = json.loads(ACCEPTANCE_PATH.read_text())["scenarios"]
    features = {row["id"]: row
                for row in json.loads(FEATURE_PATH.read_text())["supplemental_scenarios"]}
    base = sorted(k for k, v in acceptance.items()
                  if v.get("kind") == "base" and v.get("stage_id") == "R09")
    leaves = sorted(k for k, v in acceptance.items()
                    if v.get("kind") == "supplemental"
                    and "R09" in (v.get("execution_stage_ids") or []))
    assertions = sum(len(features[lid]["assertions"]) for lid in leaves)
    by_task: dict[str, set] = {}
    for lid in leaves:
        for task in acceptance[lid].get("task_ids") or []:
            by_task.setdefault(task, set()).add(lid)
    return {
        "base": base,
        "leaves": leaves,
        "assertionCount": assertions,
        "t03t07Shared": len(by_task.get("R09-T03", set()) & by_task.get("R09-T07", set())),
        "t04": len(by_task.get("R09-T04", set())),
        "t05": len(by_task.get("R09-T05", set())),
        "features": features,
        "acceptance": acceptance,
    }


def source_tree_digest() -> str:
    import hashlib
    digest = hashlib.sha256()
    root = ROOT / "rust/crates/xtask/src"
    for path in sorted(root.rglob("*")):
        if path.is_file():
            digest.update(str(path.relative_to(root)).encode())
            digest.update(b"\0")
            digest.update(sha256(path).encode())
            digest.update(b"\0")
    return digest.hexdigest()


def mutations(source: dict) -> list[dict]:
    """返回 [{'name','case','expectSubstrings','mutate','expectedExitNot0'}...]。"""
    import copy

    def drop_leaves(ids):
        value = copy.deepcopy(source)
        value["supplementalLeafScenarios"] = [
            leaf for leaf in value["supplementalLeafScenarios"] if leaf["id"] not in ids]
        return value

    def mutate(name, case_name, substrings, builder):
        return {"name": name, "case": case_name, "substrings": substrings,
                "builder": builder}

    def add_unknown_leaf(value):
        extra = copy.deepcopy(value["supplementalLeafScenarios"][0])
        extra["id"] = "R00-T02-LA-PROBE-UNKNOWN-000"
        value["supplementalLeafScenarios"].append(extra)
        return value

    def rewrite_assertion(value):
        leaf = value["supplementalLeafScenarios"][0]
        leaf["r00Assertions"] = [text + "（已被放宽）" for text in leaf["r00Assertions"]]
        return value

    def rewrite_base_mirror(value):
        value["scenarios"][0]["specMirror"]["then"] = "只需启动一次即可"
        return value

    def add_orphan_command(value):
        value["commands"]["r09_probe_orphan"] = {
            "argv": ["true"], "timeoutSecs": 30,
            "evidencePaths": ["{EVIDENCE}/probe-orphan.log"],
        }
        return value

    def drop_referenced_command(value):
        referenced = value["scenarios"][0]["commandRefs"][0]
        value["commands"].pop(referenced)
        return value

    def empty_scenario(value):
        value["scenarios"][0]["commandRefs"] = []
        return value

    def incomplete_contract(value):
        # 取一个已有 full_original_behavior 契约的叶，删除 originalAssertionCases：
        # 逐断言覆盖不完整必须被解析层拒绝（R09 起地图已有契约叶）。
        candidates = [leaf for leaf in value["supplementalLeafScenarios"]
                      if leaf.get("assertionContract") and leaf.get("originalAssertionCases")]
        if not candidates:
            raise RuntimeError("probe cannot build incomplete-contract mutation: no contract leaf")
        leaf = copy.deepcopy(candidates[0])
        leaf.pop("originalAssertionCases", None)
        target = next(i for i, item in enumerate(value["supplementalLeafScenarios"])
                      if item["id"] == leaf["id"])
        value["supplementalLeafScenarios"][target] = leaf
        return value

    def missing_evidence_paths(value):
        first = next(iter(value["commands"]))
        value["commands"][first]["evidencePaths"] = []
        return value

    t04_ids = [leaf["id"] for leaf in source["supplementalLeafScenarios"]
               if "R09-T04" in leaf["r00TaskIds"]]
    t03t07_ids = [leaf["id"] for leaf in source["supplementalLeafScenarios"]
                  if "R09-T03" in leaf["r00TaskIds"] and "R09-T07" in leaf["r00TaskIds"]]
    return [
        mutate("删 1 个补充叶", "map::dropped-leaf-rejected",
               ["drops 1 REQUIRED_SUPPLEMENTAL"], lambda v: drop_leaves([source["supplementalLeafScenarios"][0]["id"]])),
        mutate("删整任务份额（R09-T04 全部 9 叶）", "map::deleted-task-share-rejected",
               ["drops 9 REQUIRED_SUPPLEMENTAL"], lambda v: drop_leaves(t04_ids)),
        mutate("删共享任务组（T03/T07 46 叶）", "map::deleted-shared-task-share-rejected",
               ["drops 46 REQUIRED_SUPPLEMENTAL"], lambda v: drop_leaves(t03t07_ids)),
        mutate("编造账外叶", "map::unknown-leaf-rejected",
               ["unknown to the R00 acceptance ledger"], add_unknown_leaf),
        mutate("改写叶原断言", "map::rewritten-original-assertion-rejected",
               ["r00Assertions"], rewrite_assertion),
        mutate("改写基础场景镜像", "map::base-mirror-drift-rejected",
               ["specMirror drifted"], rewrite_base_mirror),
        mutate("孤儿命令（注册不引用）", "map::orphan-command-rejected",
               ["command(s) that no scenario/leaf references"], add_orphan_command),
        mutate("场景引用未注册命令", "map::missing-command-rejected",
               ["references unknown command"], drop_referenced_command),
        mutate("空绑定场景", "map::empty-scenario-rejected",
               ["commandRefs"], empty_scenario),
        mutate("逐断言覆盖不完整", "map::incomplete-assertion-coverage-rejected",
               ["originalAssertionCases"], incomplete_contract),
        mutate("命令无产物声明（缺产物）", "map::missing-evidence-paths-rejected",
               ["evidencePaths must not be empty"], missing_evidence_paths),
    ]


def main() -> int:
    started = time.time()
    evidence = Path(sys.argv[sys.argv.index("--evidence") + 1]) \
        if "--evidence" in sys.argv else None
    if evidence is None:
        print("usage: r09_map_integrity.py --evidence <dir>", file=sys.stderr)
        return 64
    evidence = evidence if evidence.is_absolute() else ROOT / evidence
    evidence.mkdir(parents=True, exist_ok=True)

    if not XTASK.is_file():
        return finish(evidence, started, "FAIL", "xtask 二进制不存在（阶段门应已构建）")

    facts = independent_ledger_facts()
    source = json.loads(MAP_PATH.read_text())
    map_ids = sorted(leaf["id"] for leaf in source["supplementalLeafScenarios"])
    map_base = sorted(s["id"] for s in source["scenarios"])
    map_assertions = sum(len(leaf["r00Assertions"]) for leaf in source["supplementalLeafScenarios"])

    case("map::base-scenario-set-16-equal-r00",
         map_base == facts["base"] == [f"R09-A{i:02}" for i in range(1, 17)],
         {"map": map_base})
    case("map::deduped-leaf-coverage-74-equal-r00",
         map_ids == facts["leaves"] and len(map_ids) == 74,
         {"count": len(map_ids)})
    case("map::original-assertions-168-verbatim",
         map_assertions == facts["assertionCount"] == 168,
         {"count": map_assertions})
    case("map::task-share-counts",
         facts["t03t07Shared"] == 46 and facts["t04"] == 9 and facts["t05"] == 19,
         {"t03t07": facts["t03t07Shared"], "t04": facts["t04"], "t05": facts["t05"]})

    source_digest_before = source_tree_digest()
    with tempfile.TemporaryDirectory(prefix="lingxi-r09-map-integrity-") as tmp:
        positive = xtask_check_map()
        case("map::contract-valid", positive.returncode == 0,
             {"exit": positive.returncode, "stdout": positive.stdout.strip()[:400]})

        for index, mutation in enumerate(mutations(source), 1):
            # 每个损坏样本都从干净的原图深拷贝出发：一个 builder 绝不能污染
            # 后续样本（否则拒绝原因错位 = 负例假通过）。
            candidate = mutation["builder"](__import__("copy").deepcopy(source))
            fixture = Path(tmp) / f"{index:02}-{mutation['case'].split('::')[1]}.json"
            fixture.write_text(json.dumps(candidate, ensure_ascii=False, indent=2) + "\n",
                               encoding="utf-8")
            process = xtask_check_map(fixture)
            output = process.stdout + process.stderr
            rejected = process.returncode != 0
            message_ok = all(token in output for token in mutation["substrings"])
            case(mutation["case"], rejected and message_ok,
                 {"expectedExitNot0": 0, "actualExit": process.returncode,
                  "expectedSubstrings": mutation["substrings"],
                  "outputTail": output.strip()[-500:],
                  "fixtureSha256": sha256(fixture)})
            log(f"[{'PASS' if rejected and message_ok else 'FAIL'}] {mutation['name']} "
                f"-> exit={process.returncode}")

        # 源码漂移负例：临时新增 xtask/src 下的 .json，runner identity 必须拒绝；
        # 删除后源码树摘要必须还原（非零失败 → 还原通过）。
        drift_file = ROOT / "rust/crates/xtask/src/stage_maps/_r09_map_integrity_drift_probe.json"
        try:
            drift_file.write_text("{}\n", encoding="utf-8")
            drift = xtask_check_map()
            drift_output = drift.stdout + drift.stderr
            case("map::source-drift-rejected",
                 drift.returncode != 0 and "source inventory differs" in drift_output,
                 {"exit": drift.returncode, "outputTail": drift_output.strip()[-500:]})
        finally:
            drift_file.unlink(missing_ok=True)
        case("map::source-tree-restored",
             source_tree_digest() == source_digest_before,
             {"before": source_digest_before, "after": source_tree_digest()})

        restored = xtask_check_map()
        case("map::restored-map-valid", restored.returncode == 0,
             {"exit": restored.returncode})
        log(f"map file sha256={sha256(MAP_PATH)} unchanged={source['stage'] == 'R09'}")

    failed = [row["case"] for row in CASES if not row["ok"]]
    status = "PASS" if not failed else "FAIL"
    return finish(evidence, started, status,
                  "" if not failed else f"failed cases: {failed}")


def finish(evidence: Path, started: float, status: str, detail: str) -> int:
    cases_payload = {"schema": "lingxi.leaf-case-results.v1",
                     "suite": "r09_map_integrity", "cases": CASES}
    (evidence / "map-integrity-cases.json").write_text(
        json.dumps(cases_payload, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    log_text = "\n".join(LOGS) + "\n"
    (evidence / "output.log").write_text(log_text, encoding="utf-8")
    result = {
        "schema": "lingxi.r09.map-integrity.v1",
        "suite": "r09_map_integrity",
        "status": status,
        "detail": detail,
        "caseCount": len(CASES),
        "failedCases": [row["case"] for row in CASES if not row["ok"]],
        "mapSha256": sha256(MAP_PATH),
        "evidence": "map-integrity-cases.json",
        "durationSecs": round(time.time() - started, 3),
    }
    (evidence / "result.json").write_text(
        json.dumps(result, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"r09_map_integrity: status={status} cases={len(CASES)} detail={detail}")
    return 0 if status == "PASS" else 2


if __name__ == "__main__":
    sys.exit(main())
