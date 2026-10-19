#!/usr/bin/env python3
"""绑定未提交候选、实际命令和原始退出结果，不改写开发失败记录。"""
import argparse
import datetime
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[5]
EVIDENCE = Path(__file__).resolve().parents[1]


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def rows(paths):
    return [{"path": p, "sha256": digest((ROOT / p).read_bytes()),
             "bytes": (ROOT / p).stat().st_size} for p in sorted(paths)]


def fingerprint(files):
    return digest(json.dumps(files, ensure_ascii=False, separators=(",", ":")).encode())


def freeze(name):
    if (EVIDENCE / f"SOURCE_MANIFEST_{name}.json").exists():
        raise SystemExit("该候选已有证据；使用新编号保留历史")
    # T05 尚未注册的开发文件不属于本次可执行候选；之后另行冻结并验证。
    excluded = ("rust/crates/lingxi-service/src/office_",
                "rust/crates/lingxi-service/src/media",
                "rust/crates/lingxi-service/src/resource_artifacts.rs",
                "rust/crates/lingxi-protocol/src/media.rs",
                "rust/crates/lingxi-adapters/src/storage/media.rs",
                "rust/crates/lingxi-adapters/src/browser/pdf.rs",
                "rust/crates/lingxi-adapters/tests/r07_media_store/",
                "rust/crates/lingxi-service/tests/r07_office",
                "rust/crates/lingxi-service/tests/r07_media")
    prior = json.loads((EVIDENCE.parent / "T03/SOURCE_MANIFEST_FINAL.json").read_text())
    paths = {f["path"] for f in prior["files"]}
    for argv in (["git", "diff", "--name-only", "-z", "HEAD"],
                 ["git", "ls-files", "--others", "--exclude-standard", "-z"]):
        for item in subprocess.check_output(argv, cwd=ROOT).decode().split("\0"):
            if not item or item.startswith(excluded):
                continue
            if item.startswith(("rust/", "contracts/", "desktop/", "lib/", "scripts/", "tests/", "plugins/")):
                paths.add(item)
    paths.add("docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json")
    files = rows(p for p in paths if (ROOT / p).is_file())
    manifest = {"stage": "R07-T04", "candidate": name,
                "baseCommit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip(),
                "frozenAt": now(), "workingTreeCodeFingerprint": fingerprint(files),
                "excludedUnregisteredT05Sources": list(excluded), "files": files}
    archive = EVIDENCE / f"source-{name}.tar.gz"
    with archive.open("wb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode="w") as tar:
            for row in files:
                data = (ROOT / row["path"]).read_bytes()
                info = tarfile.TarInfo(row["path"])
                info.size = len(data)
                info.mode = 0o644
                tar.addfile(info, io.BytesIO(data))
    manifest["archive"] = {"path": archive.name, "sha256": digest(archive.read_bytes())}
    save(EVIDENCE / f"SOURCE_MANIFEST_{name}.json", manifest)
    print(json.dumps({"candidate": name, "files": len(files), "fingerprint": manifest["workingTreeCodeFingerprint"]}))


def run(name, label, command):
    manifest = json.loads((EVIDENCE / f"SOURCE_MANIFEST_{name}.json").read_text())
    paths = [f["path"] for f in manifest["files"]]
    before = fingerprint(rows(paths))
    if before != manifest["workingTreeCodeFingerprint"]:
        raise SystemExit("候选源码已变化；先建立新的候选，不借用旧版本结果")
    env = os.environ.copy()
    supplied = {"CARGO_TARGET_DIR": "/private/tmp/lingxi-r07-build", "CARGO_BUILD_JOBS": "4",
                "LINGXI_HOME": "/private/tmp/lingxi-r07-tests/home", "TMPDIR": "/private/tmp/lingxi-r07-tests/tmp"}
    env.update(supplied)
    log = EVIDENCE / f"{name}-{label}.log"
    if log.exists() or (EVIDENCE / f"{name}-{label}.json").exists():
        raise SystemExit("该次检查已有证据；使用新名称保留历史")
    result = {"candidate": name, "name": label, "command": command,
              "cwd": str(ROOT), "environment": supplied, "startedAt": now(),
              "sourceFingerprint": before, "log": log.name}
    with log.open("wb") as stream:
        completed = subprocess.run(command, cwd=ROOT, env=env, stdout=stream, stderr=subprocess.STDOUT)
    result.update({"endedAt": now(), "exitCode": completed.returncode,
                   "sourceFingerprintAfter": fingerprint(rows(paths)), "logSha256": digest(log.read_bytes())})
    result["sourceUnchanged"] = before == result["sourceFingerprintAfter"]
    save(EVIDENCE / f"{name}-{label}.json", result)
    print(json.dumps(result, ensure_ascii=False))
    raise SystemExit(completed.returncode if result["sourceUnchanged"] else 99)


parser = argparse.ArgumentParser()
parser.add_argument("mode", choices=("freeze", "run"))
parser.add_argument("candidate")
parser.add_argument("label", nargs="?")
parser.add_argument("command", nargs=argparse.REMAINDER)
args = parser.parse_args()
if args.mode == "freeze":
    freeze(args.candidate)
else:
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not args.label or not command:
        parser.error("run需要检查名称和实际命令")
    run(args.candidate, args.label, command)
