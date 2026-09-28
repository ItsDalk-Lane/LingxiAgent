#!/usr/bin/env python3
"""在隔离的一次性仓库复现 A16 的基线污染；绝不写入主仓库。"""

import datetime as dt
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import tempfile


ROOT = pathlib.Path(__file__).resolve().parent
MAIN = next(
    parent for parent in ROOT.parents
    if (parent / "scripts/rust-tauri/r02_t08_legacy_entry_regression.sh").is_file()
)
SCRIPT = MAIN / "scripts/rust-tauri/r02_t08_legacy_entry_regression.sh"
ROOT.mkdir(parents=True, exist_ok=True)
TRACE = ROOT / "command-trace.jsonl"
if TRACE.exists():
    raise SystemExit("证据已存在：拒绝覆盖旧运行")


def now():
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec="milliseconds")


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


count = 0


def run(args, cwd, expected=(0,)):
    global count
    count += 1
    start = now()
    env = dict(os.environ)
    env["GIT_CONFIG_NOSYSTEM"] = "1"
    env["GIT_CONFIG_GLOBAL"] = "/dev/null"
    proc = subprocess.run(args, cwd=cwd, env=env, capture_output=True, text=True)
    end = now()
    stem = f"{count:02d}"
    stdout_path = ROOT / f"{stem}.stdout.log"
    stderr_path = ROOT / f"{stem}.stderr.log"
    stdout_path.write_text(proc.stdout)
    stderr_path.write_text(proc.stderr)
    row = {
        "number": count,
        "command": args,
        "cwd": str(cwd),
        "start_utc": start,
        "end_utc": end,
        "exit_code": proc.returncode,
        "expected_exit_codes": list(expected),
        "stdout_log": stdout_path.name,
        "stderr_log": stderr_path.name,
        "stdout_sha256": sha(stdout_path),
        "stderr_sha256": sha(stderr_path),
    }
    with TRACE.open("a") as handle:
        handle.write(json.dumps(row, ensure_ascii=False) + "\n")
    if proc.returncode not in expected:
        raise SystemExit(f"命令 {count} 失败：{args}; exit={proc.returncode}; 详情见 {stem}.*.log")
    return proc.stdout.rstrip("\n")


started = now()
temp_root = pathlib.Path(tempfile.mkdtemp(prefix="r02-a16-git-fixture-"))
candidate = temp_root / "candidate"
old = temp_root / "old-copy"
fresh = temp_root / "fresh-base"
candidate.mkdir()
(ROOT / "fixture-root.txt").write_text(str(temp_root) + "\n")
script_sha_before = sha(SCRIPT)
main_head = run(["git", "rev-parse", "HEAD"], MAIN)
run(["git", "init", "-q", "--initial-branch=main"], candidate)
(candidate / ".gitignore").write_text("node_modules/\n")
(candidate / "src").mkdir()
(candidate / "src/tracked.rs").write_text("baseline-v1\n")
run(["git", "add", ".gitignore", "src/tracked.rs"], candidate)
run(["git", "-c", "user.name=R02 Fixture", "-c", "user.email=r02-fixture@example.invalid", "commit", "-m", "baseline"], candidate)
base_sha = run(["git", "rev-parse", "HEAD"], candidate)
(candidate / "src/tracked.rs").write_text("committed-v2\n")
run(["git", "add", "src/tracked.rs"], candidate)
run(["git", "-c", "user.name=R02 Fixture", "-c", "user.email=r02-fixture@example.invalid", "commit", "-m", "candidate"], candidate)
candidate_sha = run(["git", "rev-parse", "HEAD"], candidate)
(candidate / "src/tracked.rs").write_text("uncommitted-v3\n")
(candidate / "src/new_candidate.rs").write_text("untracked-candidate-only\n")
(candidate / "node_modules").mkdir()
(candidate / "node_modules/ignored.bin").write_text("dependency\n")
candidate_tracked_sha = sha(candidate / "src/tracked.rs")
candidate_untracked_sha = sha(candidate / "src/new_candidate.rs")
candidate_status = run(["git", "status", "--porcelain", "--untracked-files=all"], candidate)
if " M src/tracked.rs" not in candidate_status or "?? src/new_candidate.rs" not in candidate_status:
    raise SystemExit("候选夹具缺少预期的已修改与未跟踪路径")

# 历史旧做法：脏候选整体拷贝，再强制切换基线。
shutil.copytree(candidate, old)
old_untracked_before = sha(old / "src/new_candidate.rs")
run(["git", "checkout", "-f", base_sha], old)
old_status_after_checkout = run(["git", "status", "--porcelain", "--untracked-files=all"], old)
if not (old / "src/new_candidate.rs").exists() or "?? src/new_candidate.rs" not in old_status_after_checkout:
    raise SystemExit("旧做法污染复现失败：未跟踪候选文件未滞留")
if (old / "src/tracked.rs").read_text() != "baseline-v1\n":
    raise SystemExit("旧做法覆盖复现失败：已修改候选文件未被强制回退")
old_untracked_after_checkout = sha(old / "src/new_candidate.rs")
run(["git", "clean", "-fd"], old)
old_status_after_clean = run(["git", "status", "--porcelain", "--untracked-files=all"], old)
if (old / "src/new_candidate.rs").exists() or old_status_after_clean:
    raise SystemExit("旧做法清理复现失败：隔离仓库的未跟踪文件仍在")

# 现行构造：仅克隆隔离仓库自己的 Git 对象库，空工作树普通 checkout。
fresh.mkdir()
shutil.copytree(candidate / ".git", fresh / ".git")
(fresh / ".git/index").unlink(missing_ok=True)
run(["git", "checkout", "--detach", base_sha], fresh)
(fresh / "node_modules").mkdir()
shutil.copy2(candidate / "node_modules/ignored.bin", fresh / "node_modules/ignored.bin")
fresh_head = run(["git", "rev-parse", "HEAD"], fresh)
run(["git", "diff", "--quiet", base_sha, "--"], fresh)
run(["git", "diff", "--cached", "--quiet"], fresh)
fresh_status = run(["git", "status", "--porcelain", "--untracked-files=all"], fresh)
fresh_baseline_content = run(["git", "show", f"{base_sha}:src/tracked.rs"], fresh)
if fresh_head != base_sha or fresh_status or (fresh / "src/new_candidate.rs").exists():
    raise SystemExit("现行构造纯度断言失败")
if fresh_baseline_content + "\n" != (fresh / "src/tracked.rs").read_text():
    raise SystemExit("现行构造内容不等于提交的历史内容")
if sha(SCRIPT) != script_sha_before:
    raise SystemExit("验证期间主仓库 A16 脚本内容改变，不能绑定本轮结论")

result = {
    "scope": "isolated Git fixture only; not R02-A16 acceptance",
    "started_utc": started,
    "ended_utc": now(),
    "main_repo_read_only": str(MAIN),
    "main_head": main_head,
    "a16_script_sha256": script_sha_before,
    "fixture_root": str(temp_root),
    "base_sha": base_sha,
    "candidate_sha": candidate_sha,
    "candidate_status": candidate_status.splitlines(),
    "candidate_tracked_sha256": candidate_tracked_sha,
    "candidate_untracked_sha256": candidate_untracked_sha,
    "old_copy_untracked_sha256_before": old_untracked_before,
    "old_copy_untracked_sha256_after_force_checkout": old_untracked_after_checkout,
    "old_copy_status_after_force_checkout": old_status_after_checkout.splitlines(),
    "old_copy_status_after_clean": old_status_after_clean.splitlines(),
    "fresh_base_head": fresh_head,
    "fresh_base_status": fresh_status.splitlines(),
    "fresh_base_tracked_sha256": sha(fresh / "src/tracked.rs"),
    "outcome": "PASS fixture: old force checkout retains untracked candidate; clean deletes it; fresh construction is pristine",
    "command_count": count,
}
(ROOT / "fixture-result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
print(json.dumps(result, ensure_ascii=False, indent=2))
