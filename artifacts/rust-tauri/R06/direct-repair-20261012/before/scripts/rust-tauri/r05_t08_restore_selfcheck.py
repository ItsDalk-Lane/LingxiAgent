#!/usr/bin/env python3
"""F49 永久回归：实际执行生产 shell 函数，所有写入仅在新隔离夹具。"""

import argparse
import datetime
import hashlib
import json
import os
import pathlib
import re
import shutil
import shlex
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
GATE = ROOT / "scripts/rust-tauri/r05_t08_negative_gate.sh"
KERNEL = "rust/crates/lingxi-kernel/src/lib.rs"


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--evidence", type=pathlib.Path)
    parser.add_argument("--production-sync", action="store_true")
    args = parser.parse_args()
    source = GATE.read_text()
    section = source[source.index("snapshot_pristine() {"):source.index("\nrecord_case()")]
    # 删除顶层调用，保留原声明和原函数，不改函数内任何行为。
    functions = section.replace('for f in "${MUTATED_FILES[@]}"; do\n  snapshot_pristine "$f"\ndone\n', "")
    registry = re.search(r"MUTATED_FILES=\(\n(.*?)\n\)", source, re.S).group(1).split()
    assert len(registry) == len(set(registry)) == 12
    targets = {
        "N01": ["rust/crates/xtask/src/stage_maps/R05.json"],
        "N02": ["docs/rust-tauri/R05/r05_stage_pins.tsv"],
        "N03": ["docs/rust-tauri/R05/r05_stage_pins.tsv"],
        "N04": ["rust/crates/lingxi-service/src/credentials/mod.rs", "scripts/rust-tauri/r05_t08_stage_suites.sh"],
        "N05": [], "N06": [KERNEL],
        "N07": ["rust/crates/xtask/src/stage_maps/R05.json"],
        "N08": ["rust/crates/lingxi-service/src/lib.rs"],
        "N09": ["rust/crates/lingxi-adapters/src/models/tool_render.rs"],
        "N10": ["rust/crates/lingxi-adapters/src/models/openai_completions.rs"],
        "N11": ["rust/crates/lingxi-adapters/src/models/tool_render.rs"],
        "N12": ["rust/crates/lingxi-service/src/runs.rs"],
        "N13": ["rust/crates/xtask/src/stage_maps/R04.json"],
        "N14": ["rust/crates/xtask/src/stage_maps/R05.json"],
        "N15": ["rust/crates/lingxi-adapters/src/models/credentials.rs"], "N16": [KERNEL],
    }
    assert set(sum(targets.values(), [])) <= set(registry)
    mutation_source = source[source.index("# ── N01:"):] + source[source.index("run_n03() {"):source.index("write_results() {")]
    actual_targets = set(re.findall(r'"(?:\$COPY)?/((?:rust|docs|scripts)/[^"\n]+\.(?:rs|json|tsv|sh))"', mutation_source))
    assert actual_targets == set(sum(targets.values(), [])), "原变异目标变化须更新独立覆盖清单"
    assert 'reset_copy\nwrite_results\nnote "RESULT: every' in source
    results = []
    with tempfile.TemporaryDirectory(prefix="r05-f49-") as temporary:
        ev = args.evidence.resolve() if args.evidence else pathlib.Path(temporary) / "evidence"
        ev.mkdir(parents=True, exist_ok=False)
        copy = ev / "copy"
        pristine = ev / "pristine"
        for relative in registry:
            target = copy / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes((ROOT / relative).read_bytes())
        pristine.mkdir()
        program = ev / "production-functions.sh"
        program.write_text('set -uo pipefail\nCOPY="$1"; PRISTINE="$2"\nfail() { echo "FAIL: $*" >&2; exit 1; }\n' + functions + '\neval "$3"\n')

        def run(name, argv, expected=0, cwd=copy, env=None):
            start = utc()
            proc = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, text=True)
            (ev / (name + ".stdout.log")).write_text(proc.stdout)
            (ev / (name + ".stderr.log")).write_text(proc.stderr)
            row = {"check": name, "argv": argv, "cwd": str(cwd), "startUTC": start,
                   "endUTC": utc(), "exitCode": proc.returncode, "expectedExitCode": expected,
                   "stdoutSha256": sha(ev / (name + ".stdout.log")),
                   "stderrSha256": sha(ev / (name + ".stderr.log"))}
            results.append(row)
            (ev / "commands.json").write_text(json.dumps(results, ensure_ascii=False, indent=2) + "\n")
            assert proc.returncode == expected, (name, proc.stdout, proc.stderr)
            return proc

        def shell(name, command, expected=0):
            return run(name, ["bash", str(program), str(copy), str(pristine), command], expected)

        # 真实 Git 只作用于本测试的新夹具，不使用或复制主库对象。
        run("fixture-git-init", ["git", "init", "-q"])
        run("fixture-git-add", ["git", "add", "."])
        run("fixture-git-commit", ["git", "-c", "user.name=F49 fixture", "-c", "user.email=f49@invalid", "commit", "-qm", "隔离夹具基线"])
        (copy / KERNEL).write_bytes((copy / KERNEL).read_bytes() + "\n// 合法未提交候选改动\n".encode())
        (copy / "legal-dirty.txt").write_bytes(b"legal untracked candidate\x00\xff\n")
        baseline = {f: sha(copy / f) for f in registry + ["legal-dirty.txt"]}
        status_before = run("dirty-status-before", ["git", "status", "--porcelain"]).stdout
        assert " M " + KERNEL in status_before and "?? legal-dirty.txt" in status_before
        shell("snapshot", 'for f in "${MUTATED_FILES[@]}"; do snapshot_pristine "$f"; done')
        assert all(sha(pristine / f) == baseline[f] for f in registry)
        # 从生产脚本提取两条真实追加命令，不手工制造理想恢复结果。
        for case in ("N06", "N16"):
            append = re.search(r"printf '\\n// " + case + r"[^\n]+\n\s*>> \"\$COPY/[^\"]+\"[^\n]*", source).group(0)
            shell(case + "-append", append)
            assert sha(copy / KERNEL) != baseline[KERNEL]
            shell(case + "-restore", "reset_copy")
            assert all(sha(copy / f) == baseline[f] for f in baseline)
        for f in registry:
            (copy / f).write_bytes((copy / f).read_bytes() + b"\nall-targets mutation\x00\xff")
        shell("all-targets-restore", "reset_copy")
        assert all(sha(copy / f) == baseline[f] for f in baseline)
        shell("idempotent-restore", "reset_copy; reset_copy")
        assert all(sha(copy / f) == baseline[f] for f in baseline)
        assert run("dirty-status-after", ["git", "status", "--porcelain"]).stdout == status_before
        rejected = shell("snapshot-refuses-recapture", 'snapshot_pristine "' + KERNEL + '"', 1)
        assert "already exists" in rejected.stderr
        for name, override, marker in [
            ("restore-cp-failure", 'cp() { echo "目标 cp 故障" >&2; return 23; };', "restore copy failed"),
            ("restore-kernel-cp-failure", 'cp() { if [ "$2" = "$COPY/' + KERNEL + '" ]; then return 23; fi; command cp "$@"; };', "restore copy failed"),
            ("restore-cmp-failure", 'cmp() { echo "目标 cmp 故障" >&2; return 2; };', "restore bytes differ"),
            ("restore-cp-false-success", 'cp() { return 0; };', "restore bytes differ"),
        ]:
            (copy / registry[0]).write_bytes(b"mutation before failed restore")
            rejected = shell(name, override + " reset_copy", 1)
            assert marker in rejected.stderr
            shell(name + "-recovered", "reset_copy")
        missing = pristine / KERNEL
        retained = missing.read_bytes()
        missing.unlink()
        rejected = shell("restore-missing-pristine", "reset_copy", 1)
        assert KERNEL in rejected.stderr and "restore copy failed" in rejected.stderr
        missing.write_bytes(retained)
        shell("restore-missing-pristine-recovered", "reset_copy")
        for name, override in [("snapshot-cp-failure", 'cp() { return 23; };'),
                               ("snapshot-cmp-failure", 'cmp() { return 2; };')]:
            new = ev / name
            new.mkdir()
            rejected = shell(name, 'PRISTINE="' + str(new) + '"; ' + override + ' snapshot_pristine "' + KERNEL + '"', 1)
            assert "pristine" in rejected.stderr and KERNEL in rejected.stderr
        # 身份查询执行原赋值语句，目标 Git 异常不能进入正常 note。
        queries = re.findall(r'^(?:SOURCE_HEAD|COPY_BRANCH|COPY_HEAD|COPY_STATUS)=.*$', source, re.M)
        assert len(queries) == 4
        for i, query in enumerate(queries):
            rejected = shell("git-query-failure-" + str(i), 'ROOT="$COPY"; git() { echo "目标 Git 查询故障" >&2; return 2; }; ' + query, 1)
            assert "query failed" in rejected.stderr
        authority = copy / "rust/crates/xtask/src/stage_maps/R02.json"
        authority.parent.mkdir(parents=True, exist_ok=True)
        authority.write_bytes((ROOT / authority.relative_to(copy)).read_bytes())
        run_map = json.loads(authority.read_text())
        key = run_map["scenarios"][0]["commandRefs"][0]
        expected_signal = f'xtask: verify-stage {run_map["stage"]} [{" ".join(run_map["commands"][key]["argv"])}] > {ev / "run-evidence" / key}'
        signal = shell("authority-signal", 'n06_start_signal "' + str(ev / "run-evidence") + '"').stdout.strip()
        assert signal == expected_signal
        authority.write_text("invalid json")
        assert shell("authority-invalid-query", 'n06_start_signal "' + str(ev / "run-evidence") + '" || fail "N06 authority query failed"', 1).returncode == 1
        authority.write_bytes((ROOT / authority.relative_to(copy)).read_bytes())
        # 正常同步由真实生产 runner 的输出另行集成检查；此处只造非法前提。
        log = ev / "no-start.log"
        log.write_text("evidence root exists; cargo Running is not command start\n")
        (ev / "run-evidence").mkdir()
        live = subprocess.Popen(["sleep", "30"])
        try:
            shell("sync-no-command-start", f'wait_for_n06_start {live.pid} "{log}" "{signal}" 1', 1)
            shell("sync-log-query-failure", f'wait_for_n06_start {live.pid} "{ev / "missing.log"}" "{signal}" 1', 1)
            shell("sync-process-query-failure", f'ps() {{ return 2; }}; wait_for_n06_start {live.pid} "{log}" "{signal}" 1', 1)
        finally:
            live.terminate()
            live.wait()
        shell("sync-already-exited", f'wait_for_n06_start {live.pid} "{log}" "{signal}" 1', 1)
        final = {f: sha(copy / f) for f in baseline}
        assert final == baseline
        if args.production_sync:
            # 只复制源码并建立小型新 Git 库；不复用其他人的 target 或大对象库。
            runner = ev / "runner-copy"
            shutil.copytree(ROOT / "rust", runner / "rust", ignore=shutil.ignore_patterns("target"))
            for relative in ["rust-toolchain.toml", ".gitignore", "scripts/rust-tauri/run_output_sinks.py",
                             "docs/rust-tauri/R00/ACCEPTANCE_MAP.json", "docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json"] + registry:
                target = runner / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes((ROOT / relative).read_bytes())
            release = ev / "release-command"
            controlled = {"schemaVersion": 1, "resultVersion": "lingxi.xtask.verify-stage.v1", "stage": "RX",
                          "defaultTimeoutSecs": 20, "commands": {key: {
                              "argv": ["python3", "-c", "import pathlib,time,sys; p=pathlib.Path(sys.argv[1]); end=time.monotonic()+15; exec('while not p.exists() and time.monotonic()<end:\\n time.sleep(.02)'); assert p.exists(), '控制命令未释放'", str(release)],
                              "timeoutSecs": 20, "evidencePaths": ["{EVIDENCE}/" + key + "/stdout.log"]}},
                          "scenarios": [{"id": "RX-F49-SYNC", "requirement": "REQUIRED", "commandRefs": [key]}]}
            (runner / authority.relative_to(copy)).write_text(json.dumps(controlled, ensure_ascii=False, indent=2) + "\n")
            run("runner-fixture-git-init", ["git", "init", "-q"], cwd=runner)
            run("runner-fixture-git-add", ["git", "add", "."], cwd=runner)
            run("runner-fixture-git-commit", ["git", "-c", "user.name=F49 fixture", "-c", "user.email=f49@invalid", "commit", "-qm", "生产同步隔离夹具"], cwd=runner)
            cargo = str(pathlib.Path.home() / ".cargo/bin/cargo")
            build_env = dict(os.environ, CARGO_NET_OFFLINE="true", CARGO_TARGET_DIR=str(ev / "own-target"))
            run("build-production-runner", [cargo, "build", "--manifest-path", "rust/Cargo.toml", "--locked", "-p", "xtask"], cwd=runner, env=build_env)
            pristine_runner = ev / "runner-pristine"
            pristine_runner.mkdir()
            def runner_shell(name, command, expected=0):
                return run(name, ["bash", str(program), str(runner), str(pristine_runner), command], expected, cwd=runner)
            runner_shell("runner-snapshot", 'for f in "${MUTATED_FILES[@]}"; do snapshot_pristine "$f"; done')
            binary = ev / "own-target/debug/xtask"
            (ev / "runner-binary.json").write_text(json.dumps({"path": str(binary), "sha256": sha(binary),
                "authority": controlled, "boundary": "真实生产 main/verify/Scope/FD/Git；RX 控制命令仅证明同步和 stable 强制失败，不代表 R02 业务门禁。"}, ensure_ascii=False, indent=2) + "\n")
            for name, mutate in [("sync-production-normal", False), ("sync-production-midbyte", True), ("sync-production-restored", False)]:
                release.unlink(missing_ok=True)
                evidence_root = ev / name
                argv = [str(binary), "verify-stage", "R02", "--evidence", str(evidence_root)]
                live_log = ev / (name + ".run.log")
                started = utc()
                with live_log.open("w") as output:
                    gate = subprocess.Popen(argv, cwd=runner, stdout=output, stderr=subprocess.STDOUT)
                    try:
                        real_signal = runner_shell(name + "-authority", f'n06_start_signal "{evidence_root}"').stdout.strip()
                        runner_shell(name + "-wait", f'wait_for_n06_start {gate.pid} "{live_log}" {shlex.quote(real_signal)} 10')
                        assert gate.poll() is None
                        if mutate:
                            runner_shell(name + "-append", re.search(r"printf '\\n// N06[^\n]+\n\s*>> \"\$COPY/[^\"]+\"[^\n]*", source).group(0))
                        release.write_text("释放当前控制命令\n")
                        exit_code = gate.wait(timeout=20)
                    finally:
                        release.write_text("结束控制命令\n")
                        if gate.poll() is None:
                            gate.terminate()
                            gate.wait(timeout=20)
                report = json.loads((evidence_root / "verify-stage-result.json").read_text())
                assert exit_code == (1 if mutate else 0), name
                assert report["overall"] == ("FAIL" if mutate else "PASS"), name
                assert report["candidateSourceBinding"]["stable"] == (not mutate), name
                assert report["runnerSourceBinding"]["status"] == "PASS", name
                checkpoints = report["candidateSourceBinding"]["checkpointAfterEveryCommand"]
                assert len(checkpoints) == 1 and checkpoints[0]["stable"] == (not mutate), name
                assert all(c["status"] == "PASS" and c["exitCode"] == 0 for c in report["commands"]), name
                runner_shell(name + "-refuse-finished", f'wait_for_n06_start {gate.pid} "{live_log}" {shlex.quote(real_signal)} 1', 1)
                results.append({"check": name, "argv": argv, "cwd": str(runner), "startUTC": started,
                                "endUTC": utc(), "exitCode": exit_code, "expectedExitCode": 1 if mutate else 0,
                                "logSha256": sha(live_log), "overall": report["overall"],
                                "stable": report["candidateSourceBinding"]["stable"], "checkpointCount": 1})
                runner_shell(name + "-restore", "reset_copy")
                assert all(sha(runner / f) == sha(pristine_runner / f) for f in registry)
        doc = {"findingId": "F49", "status": "SELF_CHECKED", "checks": len(results),
               "failed": 0, "ignored": 0, "filtered": 0, "results": results,
               "gateSha256": sha(GATE), "selfcheckSha256": sha(pathlib.Path(__file__)),
               "productionSyncExecuted": args.production_sync,
               "mutationTargets": targets, "snapshotRestoreRegistry": registry,
               "preservedBaseline": baseline, "restoredBytes": final,
               "boundary": "原 shell 函数的隔离夹具；不代表默认 16 或完整业务门禁运行；正常同步须实际生产 runner 集成证据。"}
        (ev / "result.json").write_text(json.dumps(doc, ensure_ascii=False, indent=2) + "\n")
        print(json.dumps({"findingId": "F49", "checks": len(results), "failed": 0, "evidence": str(ev)}, ensure_ascii=False))


if __name__ == "__main__":
    main()
