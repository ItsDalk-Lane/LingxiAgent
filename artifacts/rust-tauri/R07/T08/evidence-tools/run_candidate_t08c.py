#!/usr/bin/env python3
"""绑定未提交候选、实际命令和原始退出结果，不改写开发失败记录。"""
import argparse
import datetime
import gzip
import hashlib
import io
import json
import os
import stat
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
    result = []
    for item in sorted(paths):
        path = ROOT / item
        if path.is_symlink():
            target = os.readlink(path)
            result.append({"path": item, "symlink": target, "sha256": digest(target.encode())})
        elif path.is_file():
            data = path.read_bytes()
            result.append({"path": item, "sha256": digest(data), "bytes": len(data),
                           "mode": stat.S_IMODE(path.stat().st_mode)})
        else:
            result.append({"path": item, "deleted": True})
    return result


def source_paths():
    # 每次重新扫描，其他软件新增或修改原清单之外的源文件也会使候选失效。
    prior = json.loads((EVIDENCE.parent / "T07/SOURCE_MANIFEST_t07d.json").read_text())
    paths = {f["path"] for f in prior["files"]}
    for argv in (["git", "diff", "--name-only", "-z", "HEAD"],
                 ["git", "ls-files", "--others", "--exclude-standard", "-z"]):
        for item in subprocess.check_output(argv, cwd=ROOT).decode().split("\0"):
            if item in {"package.json", "package-lock.json", "Cargo.toml", "Cargo.lock"} or item.startswith(("tsconfig", "vite.", "vite-", "vitest.", "eslint.", "electron-builder.")) or item.startswith(("rust/", "contracts/", "desktop/", "lib/", "scripts/", "tests/", "plugins/", "shared/", "core/", "server/", "mobile/", "cli/", "src/", "bin/", "config/", "skills2set/", "vendor/", "build/", "hub/", "bundled-bin/", "spike/", ".github/")):
                paths.add(item)
    paths.update(["docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json",
                  "docs/rust-tauri/R01/API_COMPAT_MATRIX.json",
                  "docs/rust-tauri/R01/PROTOCOL_SPEC.md",
                  "docs/rust-tauri/R01/r01_t01_check_ownership.py",
                  "docs/rust-tauri/R01/r01_t01_build_ownership.py",
                  "docs/rust-tauri/R01/OWNERSHIP_TARGET.json",
                  "docs/rust-tauri/R01/DEPENDENCY_RULES.json",
                  "docs/rust-tauri/R00/FEATURE_INVENTORY.json",
                  "docs/rust-tauri/R00/STORES.json",
                  "docs/rust-tauri/R07/R07_LEAF_MAP.json",
                  "package.json", "package-lock.json", "rust-toolchain.toml",
                  "tsconfig.json", "tsconfig.node.json", "tsconfig.test.json"])
    return paths


def fingerprint(files):
    return digest(json.dumps(files, ensure_ascii=False, separators=(",", ":")).encode())


def verification_tools():
    # 检查包装器同样冻结，防止候选源码不变时检查逻辑被其他编辑器改动。
    names = ["run_gate_group.py", "run_ui_bound.py", "verify_native_picker_binding.py"]
    return {name: digest((EVIDENCE / "evidence-tools" / name).read_bytes()) for name in names}


def freeze(name):
    if (EVIDENCE / f"SOURCE_MANIFEST_{name}.json").exists():
        raise SystemExit("该候选已有证据；使用新编号保留历史")
    files = rows(source_paths())
    manifest = {"stage": "R07-T08", "candidate": name,
                "baseCommit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip(),
                "runnerSha256": digest(Path(__file__).read_bytes()),
                "verificationToolHashes": verification_tools(),
                "gatePlanSha256": digest((EVIDENCE / "GATE_PLAN.json").read_bytes()),
                "frozenAt": now(), "workingTreeCodeFingerprint": fingerprint(files),
                "fingerprintMethod": "sorted-path-sha256-bytes-mode-with-deletions-and-symlink-targets-v1",
                "excludedSources": [], "files": files}
    archive = EVIDENCE / f"source-{name}.tar.gz"
    with archive.open("wb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode="w") as tar:
            for row in files:
                if row.get("deleted"):
                    continue
                info = tarfile.TarInfo(row["path"])
                if "symlink" in row:
                    info.type = tarfile.SYMTYPE
                    info.linkname = row["symlink"]
                    tar.addfile(info)
                else:
                    data = (ROOT / row["path"]).read_bytes()
                    info.size = len(data)
                    info.mode = row["mode"]
                    tar.addfile(info, io.BytesIO(data))
    manifest["archive"] = {"path": archive.name, "sha256": digest(archive.read_bytes())}
    save(EVIDENCE / f"SOURCE_MANIFEST_{name}.json", manifest)
    print(json.dumps({"candidate": name, "files": len(files), "fingerprint": manifest["workingTreeCodeFingerprint"]}))


def run(name, label, command):
    manifest = json.loads((EVIDENCE / f"SOURCE_MANIFEST_{name}.json").read_text())
    if digest(Path(__file__).read_bytes()) != manifest["runnerSha256"]:
        raise SystemExit("候选运行器已变化，拒绝旧版本运行器证据")
    if verification_tools() != manifest["verificationToolHashes"]:
        raise SystemExit("候选检查包装器已变化，必须重新冻结")
    plan_path = EVIDENCE / "GATE_PLAN.json"
    if digest(plan_path.read_bytes()) != manifest["gatePlanSha256"]:
        raise SystemExit("候选检查计划已变化，必须重新冻结")
    planned = json.loads(plan_path.read_text())["gates"].get(label)
    if planned is None or command != planned:
        raise SystemExit("实际命令与冻结检查计划不一致，拒绝借用检查名称")
    before = fingerprint(rows(source_paths()))
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip()
    if before != manifest["workingTreeCodeFingerprint"] or head != manifest["baseCommit"]:
        raise SystemExit("候选源码已变化；先建立新的候选，不借用旧版本结果")
    env = os.environ.copy()
    supplied = {"CARGO_TARGET_DIR": "/private/tmp/lingxi-r07-build", "CARGO_BUILD_JOBS": "4",
                "LINGXI_HOME": "/private/tmp/lingxi-r07-tests/home", "TMPDIR": "/private/tmp/lingxi-r07-tests/tmp", "LINGXI_PRODUCT_DIR": str(ROOT / "lib"), "PATH": "/Users/study_superior/.cargo/bin:" + os.environ["PATH"]}
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
                   "sourceFingerprintAfter": fingerprint(rows(source_paths())), "logSha256": digest(log.read_bytes()),
                   "baseCommitAfter": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip()})
    result["sourceUnchanged"] = before == result["sourceFingerprintAfter"] and result["baseCommitAfter"] == head
    result["verificationToolsUnchanged"] = verification_tools() == manifest["verificationToolHashes"]
    result["gatePlanSha256"] = manifest["gatePlanSha256"]
    if label == "build" and completed.returncode == 0:
        binary = Path(supplied["CARGO_TARGET_DIR"]) / "debug/lingxi-service"
        result["binary"] = {"path": str(binary), "sha256": digest(binary.read_bytes()), "bytes": binary.stat().st_size}
    save(EVIDENCE / f"{name}-{label}.json", result)
    print(json.dumps(result, ensure_ascii=False))
    raise SystemExit(completed.returncode if result["sourceUnchanged"] and result["verificationToolsUnchanged"] else 99)


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
