#!/usr/bin/env python3
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile

BASE = "89bc0b64bf0a9b84ef3532efaa66c23213affb70"
ROOT = pathlib.Path(__file__).resolve().parents[3]
OUT = ROOT / "artifacts/f1-f12-repair/round2"
PATCH = OUT / "patches/89bc0b64-to-r01-r10-source.patch"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def keep(relative: str) -> bool:
    if relative.startswith("artifacts/f1-f12-repair/") and not relative.endswith(".py"):
        return False
    return not relative.endswith(".patch")


def stage_current(index_name: str) -> None:
    """BASE + 当前树（artifacts 只保留 *.py）→ 临时 index。补丁与校验共用。"""
    env = os.environ.copy()
    env["GIT_INDEX_FILE"] = index_name
    subprocess.run(["git", "read-tree", BASE], cwd=ROOT, env=env, check=True)
    subprocess.run(["git", "add", "-A", "--", "."], cwd=ROOT, env=env, check=True)
    subprocess.run(
        ["git", "reset", "-q", BASE, "--", "artifacts/f1-f12-repair"],
        cwd=ROOT, env=env, check=True,
    )
    python_files = sorted(
        str(path.relative_to(ROOT))
        for path in (ROOT / "artifacts/f1-f12-repair").rglob("*.py")
        if path.is_file()
    )
    if python_files:
        subprocess.run(["git", "add", "--", *python_files], cwd=ROOT, env=env, check=True)


def manifest_from_index(index_name: str) -> bytes:
    """从临时 index 的 blob（git 归一化字节）计算源码 manifest。

    不再读工作树字节：Windows autocrlf 会把工作树字节与 blob 字节拆开
    （CRLF/LF），导致 current 与 replay 天然不一致（R10-09 Windows CI 实测）。
    两侧统一走 index blob 后，行尾归一化由 git 同一套规则处理。
    """
    env = os.environ.copy()
    env["GIT_INDEX_FILE"] = index_name
    listing = subprocess.check_output(["git", "ls-files", "-s", "-z"], cwd=ROOT, env=env)
    entries = []
    for record in listing.decode().split("\0"):
        if not record:
            continue
        meta, relative = record.split("\t", 1)
        _mode, blob_sha, _stage = meta.split(" ")
        if keep(relative):
            entries.append((relative, blob_sha))
    entries.sort()
    batch_input = "\n".join(sha for _rel, sha in entries) + "\n"
    batch = subprocess.run(
        ["git", "cat-file", "--batch"], cwd=ROOT, env=env,
        input=batch_input.encode(), stdout=subprocess.PIPE,
    )
    if batch.returncode != 0:
        raise RuntimeError("cat-file --batch failed while building index manifest")
    rows = []
    offset = 0
    stdout = batch.stdout
    for relative, blob_sha in entries:
        header_end = stdout.index(0x0A, offset)
        header = stdout[offset:header_end].decode()
        if not header.startswith(f"{blob_sha} blob "):
            raise RuntimeError(f"cat-file record mismatch for {relative}: {header}")
        size = int(header.split(" ")[2])
        start = header_end + 1
        end = start + size
        blob = stdout[start:end]
        rows.append({"path": relative, "bytes": len(blob), "sha256": sha256(blob)})
        offset = end + 1
    manifest = {
        "sourceIdentity": {"kind": "worktree", "base": BASE},
        "exclusions": [
            "artifacts/f1-f12-repair/** except *.py (generated evidence, reports, delivery files and synthetic test home)",
            "*.patch (delivery patches)",
            "git ignored generated files; git ls-files --cached --others --exclude-standard",
        ],
        "files": rows,
    }
    return (json.dumps(manifest, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()


def temp_index(prefix: str) -> str:
    fd, name = tempfile.mkstemp(prefix=prefix)
    os.close(fd)
    os.unlink(name)
    return name


def build_patch() -> bytes:
    PATCH.parent.mkdir(parents=True, exist_ok=True)
    index_name = temp_index("lingxi-r01-r10-index-")
    try:
        stage_current(index_name)
        env = os.environ.copy()
        env["GIT_INDEX_FILE"] = index_name
        content = subprocess.check_output(
            ["git", "diff", "--cached", "--binary", "--full-index", BASE, "--"],
            cwd=ROOT, env=env,
        )
    finally:
        pathlib.Path(index_name).unlink(missing_ok=True)
    if not content:
        raise RuntimeError("delivery source patch is empty")
    PATCH.write_bytes(content)
    return content


def replay_and_verify(patch: bytes) -> dict:
    current_index = temp_index("lingxi-r01-r10-current-")
    replay_index = temp_index("lingxi-r01-r10-replay-")
    try:
        stage_current(current_index)
        current = manifest_from_index(current_index)

        replay_env = os.environ.copy()
        replay_env["GIT_INDEX_FILE"] = replay_index
        subprocess.run(["git", "read-tree", BASE], cwd=ROOT, env=replay_env, check=True)
        apply = subprocess.run(
            ["git", "apply", "--cached", "--binary", "--whitespace=nowarn", "-"],
            cwd=ROOT, env=replay_env, input=patch,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        )
        if apply.returncode != 0:
            raise RuntimeError(f"patch replay failed: {apply.stdout.decode(errors='replace')[:2000]}")
        replayed = manifest_from_index(replay_index)
        if replayed != current:
            raise RuntimeError(
                f"replayed source manifest mismatch: current={sha256(current)} replay={sha256(replayed)}"
            )
        return {
            "base": BASE,
            "patch": str(PATCH.relative_to(ROOT)),
            "patchBytes": len(patch),
            "patchSha256": sha256(patch),
            "sourceManifestHash": sha256(current),
            "replayedSourceManifestHash": sha256(replayed),
            "result": "VERIFIED",
        }
    finally:
        pathlib.Path(current_index).unlink(missing_ok=True)
        pathlib.Path(replay_index).unlink(missing_ok=True)


if __name__ == "__main__":
    patch = build_patch()
    print(json.dumps(replay_and_verify(patch), ensure_ascii=False, sort_keys=True))
