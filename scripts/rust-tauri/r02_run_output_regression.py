#!/usr/bin/env python3
"""调用生产发现器的真实 fd 回归：不用手写理想排除清单替代发现结果。"""
import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone

REPO = Path(__file__).resolve().parents[2]
GATE = REPO / "scripts/rust-tauri/r02_t08_legacy_entry_regression.sh"
DISCOVERY = REPO / "scripts/rust-tauri/run_output_sinks.py"


def functions(discovery):
    source = GATE.read_text()
    binder = source[source.index("bind_worktree() {"):source.index("\n# The gate's own evidence subtree")]
    discover = source[source.index("discover_run_output_sinks() {"):source.index("\n# validate_declared_run_root")]
    discover = discover.replace('"$MAIN_REPO/scripts/rust-tauri/run_output_sinks.py"', '"' + str(discovery) + '"')
    validation = source[source.index("validate_run_output_unit() {"):source.index("\n# discover_run_output_sinks:")]
    declared = source[source.index("validate_declared_run_root() {"):source.index("\n# Assemble the exclusion-unit set")]
    return binder + "\n" + discover + "\n" + validation + "\n" + declared


def git(root, *args):
    subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True)


def driver():
    # 本进程持有父 stdout，下一进程持有 child stdout/stderr，扫描器能实际向上追踪。
    lib, root, evidence, mode = map(Path, sys.argv[2:6])
    run = root / "artifacts/rust-tauri/R05/run001"
    child = run / "child"
    code = r'''
set -euo pipefail
source "$1"
MAIN_REPO="$2"
EVIDENCE_DIR="$3"
( discover_run_output_sinks ) > "$3/sinks.txt"
sed -n 's/^DIR //p; s/^FILE //p' "$3/sinks.txt" > "$3/units.txt"
bind_worktree "$2" "$3/before.tsv" "$(cat "$3/units.txt")"
cp -R "$2" "$3/candidate-copy"
bind_worktree "$3/candidate-copy" "$3/copy.tsv" "$(cat "$3/units.txt")"
cmp "$3/before.tsv" "$3/copy.tsv"
rm -rf -- "$3/candidate-copy"
if [ "$4" = yes ]; then printf growth >> "$2/artifacts/rust-tauri/R05/run001/stdout.log"; fi
printf growth >> "$2/artifacts/rust-tauri/R05/run001/child/stdout.log"
printf growth >> "$2/artifacts/rust-tauri/R05/run001/child/stderr.log"
bind_worktree "$2" "$3/log-growth.tsv" "$(cat "$3/units.txt")"
'''
    if mode.name == "old":
        code += r'''
printf changed > "$2/artifacts/rust-tauri/R05/run001/child/old-evidence.json"
bind_worktree "$2" "$3/after.tsv" "$(cat "$3/units.txt")"
'''
    else:
        code += r'''
printf new > "$2/source-new.txt"
bind_worktree "$2" "$3/after.tsv" "$(cat "$3/units.txt")"
'''
    with (child / "stdout.log").open("wb") as stdout, (child / "stderr.log").open("wb") as stderr:
        result = subprocess.run(["bash", "-c", code, "bash", str(lib), str(root), str(evidence), sys.argv[6]], stdout=stdout, stderr=stderr)
    raise SystemExit(result.returncode)


def validator_query_regression(evidence, scratch, discovery):
    """真实生产校验与 Git 索引；只在隔离目录替换出错的外部查询。"""
    case = evidence / "validator-query"
    case.mkdir()
    root = scratch / "validator-query-repo"
    root.mkdir()
    git(root, "init", "-q")
    tracked_unit = "artifacts/rust-tauri/R05/tracked-run"
    fresh_unit = "artifacts/rust-tauri/R05/fresh-run"
    for unit in (tracked_unit, fresh_unit):
        (root / unit).mkdir(parents=True)
    (root / tracked_unit / "tracked-source.rs").write_text("tracked source")
    git(root, "add", tracked_unit + "/tracked-source.rs")
    lib = case / "production-functions.sh"
    lib.write_text(functions(discovery))
    fault_bin = scratch / "validator-query-bin"
    fault_bin.mkdir()
    real_git = shutil.which("git")
    assert real_git, "真实 Git 不可用"
    proxy = fault_bin / "git"
    proxy.write_text("#!/usr/bin/env python3\n" + '''import os, subprocess, sys
mode = os.environ.get("RR3_GIT_QUERY_FAULT", "normal")
if "ls-files" in sys.argv[1:] and mode not in ("normal", "restored"):
    if mode in ("exit2-stderr", "exit0-stderr"):
        sys.stderr.write("controlled ls-files inspection failure\\n")
    if mode == "exit2-stdout":
        sys.stdout.write("misleading-result\\n")
    sys.exit(0 if mode == "exit0-stderr" else 2)
sys.exit(subprocess.call([os.environ["RR3_REAL_GIT"], *sys.argv[1:]]))
''')
    proxy.chmod(0o755)
    records = []
    for validator in ("validate_run_output_unit", "validate_declared_run_root"):
        for kind, unit in (("tracked", tracked_unit), ("fresh", fresh_unit)):
            for mode in ("normal", "exit2-stderr", "exit2-empty", "exit0-stderr", "exit2-stdout", "restored"):
                env = dict(os.environ, RR3_REAL_GIT=real_git, RR3_GIT_QUERY_FAULT=mode)
                # 正常和恢复对照直接调用真实 Git，故障只替换目标查询。
                if mode not in ("normal", "restored"):
                    env["PATH"] = str(fault_bin) + os.pathsep + env["PATH"]
                argv = ["bash", "-c", 'source "$1"; "$2" "$3" "$4"', "bash", str(lib), validator, str(root), unit]
                started = datetime.now(timezone.utc).isoformat()
                result = subprocess.run(argv, env=env, capture_output=True)
                ended = datetime.now(timezone.utc).isoformat()
                stem = f"{validator}-{kind}-{mode}"
                stdout_log, stderr_log = case / (stem + ".stdout.log"), case / (stem + ".stderr.log")
                stdout_log.write_bytes(result.stdout)
                stderr_log.write_bytes(result.stderr)
                fault = mode not in ("normal", "restored")
                expected = 1 if fault or kind == "tracked" else 0
                reason_valid = (b"git ls-files query failed" in result.stderr) if fault else ((b"INDEX-TRACKED" in result.stderr) if kind == "tracked" else result.stderr == b"")
                records.append({"validator": validator, "rootKind": kind, "mode": mode, "argv": argv, "startedAt": started, "endedAt": ended, "exitCode": result.returncode, "expectedExitCode": expected, "reasonValid": reason_valid, "passed": result.returncode == expected and reason_valid, "stdoutLog": str(stdout_log), "stderrLog": str(stderr_log), "stdoutSha256": hashlib.sha256(result.stdout).hexdigest(), "stderrSha256": hashlib.sha256(result.stderr).hexdigest()})
    summary = {"actual": len(records), "passed": sum(row["passed"] for row in records), "ignored": 0, "filtered": 0, "realGit": real_git, "policy": "git ls-files 仅允许 exit 0 且 stderr 为空；非零、缺少诊断的非零、成功码附带错误文本均拒绝", "substituteBoundary": "临时 Git 工作区真实索引；生产函数不替换；仅故障行 PATH 代理 ls-files，其余 Git 调真实程序", "commands": records}
    (case / "results.json").write_text(json.dumps(summary, indent=2, ensure_ascii=False) + "\n")
    assert summary["passed"] == summary["actual"], ("生产 Git 查询异常未显式拒绝", summary["passed"], summary["actual"])
    return {key: summary[key] for key in ("actual", "passed", "ignored", "filtered", "policy")}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--discovery", type=Path, default=DISCOVERY)
    parser.add_argument("--validators-only", action="store_true")
    args = parser.parse_args()
    evidence = Path(args.evidence).resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    records = []
    scratch = Path(tempfile.mkdtemp(prefix="rr3-output-regression-")).resolve()
    try:
        query_regression = validator_query_regression(evidence, scratch, args.discovery.resolve())
        if args.validators_only:
            print(f"PASS production validators: {query_regression['passed']}/{query_regression['actual']} cases")
            return
        for name, old, tracked, parent in [
            ("old-untracked-parent-child", True, False, True),
            ("old-untracked-child-only", True, False, False),
            ("old-tracked-parent-child", True, True, True),
            ("fresh-parent-child", False, False, True),
        ]:
            case = evidence / name
            case.mkdir()
            root = scratch / name
            root.mkdir()
            git(root, "init", "-q")
            # 使用现行忽略规则，真实旧非忽略文件不可漏绑定。
            shutil.copyfile(REPO / ".gitignore", root / ".gitignore")
            git(root, "add", ".gitignore")
            run = root / "artifacts/rust-tauri/R05/run001"
            child = run / "child"
            child.mkdir(parents=True)
            if old:
                (child / "old-evidence.json").write_text("old")
                if tracked:
                    git(root, "add", "artifacts/rust-tauri/R05/run001/child/old-evidence.json")
            lib = case / "production-functions.sh"
            lib.write_text(functions(args.discovery.resolve()))
            parent_sink = run / "stdout.log" if parent else case / "external-parent.log"
            with parent_sink.open("wb") as stdout, (case / "driver-stderr.log").open("wb") as stderr:
                proc = subprocess.run([sys.executable, __file__, "driver", str(lib), str(root), str(case), "old" if old else "new", "yes" if parent else "no"], stdout=stdout, stderr=stderr)
            assert proc.returncode == 0, (name, proc.returncode)
            before = (case / "before.tsv").read_bytes()
            growth = (case / "log-growth.tsv").read_bytes()
            after = (case / "after.tsv").read_bytes()
            assert before == growth, (name, "真实父子日志增长污染绑定")
            assert before != after, (name, "旧证据变更或新增源码被发现器吞掉")
            units = (case / "units.txt").read_text().splitlines()
            if old:
                assert "artifacts/rust-tauri/R05/run001" not in units
                assert "artifacts/rust-tauri/R05/run001/child" not in units
                assert b"old-evidence.json" in before
            # 所有变异都用发现器的实际固定清单，且源端/副本端套用完全相同的清单。
            mutation_script = r'''
set -euo pipefail
source "$1"
for target in source.rs scripts/check.sh config.json stage-map.json; do
  mkdir -p "$(dirname "$2/$target")"
  printf original > "$2/$target"
  git -C "$2" add "$target"
  bind_worktree "$2" "$3/mutation-before.tsv" "$(cat "$3/units.txt")"
  printf changed > "$2/$target"
  bind_worktree "$2" "$3/mutation-after.tsv" "$(cat "$3/units.txt")"
  if cmp -s "$3/mutation-before.tsv" "$3/mutation-after.tsv"; then exit 41; fi
  printf original > "$2/$target"
done
bind_worktree "$2" "$3/delete-before.tsv" "$(cat "$3/units.txt")"
rm "$2/source.rs"
bind_worktree "$2" "$3/delete-after.tsv" "$(cat "$3/units.txt")"
if cmp -s "$3/delete-before.tsv" "$3/delete-after.tsv"; then exit 42; fi
printf original > "$2/source.rs"
bind_worktree "$2" "$3/rename-before.tsv" "$(cat "$3/units.txt")"
mv "$2/source.rs" "$2/renamed.rs"
bind_worktree "$2" "$3/rename-after.tsv" "$(cat "$3/units.txt")"
if cmp -s "$3/rename-before.tsv" "$3/rename-after.tsv"; then exit 43; fi
'''
            mutation = subprocess.run(["bash", "-c", mutation_script, "bash", str(lib), str(root), str(case)], capture_output=True)
            (case / "mutation-stdout.log").write_bytes(mutation.stdout)
            (case / "mutation-stderr.log").write_bytes(mutation.stderr)
            assert mutation.returncode == 0, (name, "source/script/config/stage map/delete/rename", mutation.stderr)
            records.append({"case": name, "exit": proc.returncode, "stableDuringLogGrowth": True, "detectsCandidateChange": True, "sourceCopySymmetric": True, "detectsSourceScriptConfigStageMapDeleteRename": True, "units": units, "beforeSha256": hashlib.sha256(before).hexdigest(), "afterSha256": hashlib.sha256(after).hexdigest()})
        root = scratch / "root-fences"
        root.mkdir()
        git(root, "init", "-q")
        (root / "rust").mkdir()
        (root / "rust/source.rs").write_text("source")
        git(root, "add", "rust/source.rs")
        fresh = root / "artifacts/rust-tauri/R05/fresh-run"
        fresh.mkdir(parents=True)
        (fresh / "stdout.log").write_text("fresh")
        (root / "artifacts/rust-tauri/R05/link-run").symlink_to(fresh, target_is_directory=True)
        lib = evidence / "fence-functions.sh"
        lib.write_text(functions(args.discovery.resolve()))
        fence_records = []
        for unit in ["rust", "artifacts", "artifacts/rust-tauri", "artifacts/rust-tauri/R05", "artifacts/rust-tauri/R05/link-run", "../escape", "artifacts/rust-tauri/R05/missing"]:
            result = subprocess.run(["bash", "-c", 'source "$1"; validate_declared_run_root "$2" "$3"', "bash", str(lib), str(root), unit], capture_output=True)
            assert result.returncode != 0, ("illegal declared root accepted", unit)
            fence_records.append({"unit": unit, "exit": result.returncode, "reason": result.stderr.decode()})
        valid = subprocess.run(["bash", "-c", 'source "$1"; validate_declared_run_root "$2" "$3"', "bash", str(lib), str(root), "artifacts/rust-tauri/R05/fresh-run"], capture_output=True)
        assert valid.returncode == 0
        tracked_sink = fresh / "stdout.log"
        git(root, "add", "artifacts/rust-tauri/R05/fresh-run/stdout.log")
        tracked_receipt = evidence / "tracked-sink.txt"
        tracked_code = r'''source "$1"; MAIN_REPO="$2"; EVIDENCE_DIR="$3"; ( discover_run_output_sinks ) > "$4"; if grep -q '^TRACKED-SINK ' "$4"; then exit 1; fi'''
        with tracked_sink.open("ab") as sink:
            tracked_result = subprocess.run(["bash", "-c", tracked_code, "bash", str(lib), str(root), str(evidence), str(tracked_receipt)], stdout=sink, stderr=subprocess.PIPE)
        assert tracked_result.returncode == 1
        assert "TRACKED-SINK artifacts/rust-tauri/R05/fresh-run/stdout.log" in tracked_receipt.read_text()
        os_failure = {"status": "NOT_APPLICABLE", "platform": platform.system()}
        if platform.system() == "Darwin":
            # 仅负向控制替换系统查询命令；生产发现器不得把故障当空结果。
            broken_bin = scratch / "broken-os-query"
            broken_bin.mkdir()
            lsof = broken_bin / "lsof"
            lsof.write_text("#!/bin/sh\nprintf 'controlled OS query failure\n' >&2\nexit 2\n")
            lsof.chmod(0o755)
            env = dict(os.environ)
            env["PATH"] = str(broken_bin) + os.pathsep + env["PATH"]
            failure = subprocess.run([sys.executable, str(args.discovery.resolve()), str(root), str(evidence), "--files"], env=env, capture_output=True)
            (evidence / "os-query-failure.stderr.log").write_bytes(failure.stderr)
            assert failure.returncode != 0
            assert b"cannot query real OS output descriptors" in failure.stderr
            os_failure = {"status": "PASS", "exit": failure.returncode, "boundary": "仅负例lsof返回错误；发现器未替换"}
        (evidence / "root-fences.json").write_text(json.dumps({"rejections": fence_records, "legalFreshRootExit": valid.returncode}, indent=2) + "\n")
        (evidence / "results.json").write_text(json.dumps({"cases": records, "actual": len(records), "ignored": 0, "filtered": 0, "validatorQueries": query_regression, "illegalRootsRejected": len(fence_records), "trackedSinkRejected": tracked_result.returncode == 1, "osQueryFailure": os_failure, "substituteBoundary": "仅临时Git工作区；发现器、OS fd、binder都是生产入口"}, indent=2) + "\n")
        print(f"PASS real OS fd discovery: {len(records)}/{len(records)} cases")
    finally:
        # 只清理本回归自己创建的唯一临时目录。
        shutil.rmtree(scratch)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "driver":
        driver()
    else:
        main()
