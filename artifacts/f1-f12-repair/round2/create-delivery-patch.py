#!/usr/bin/env python3
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import tempfile

BASE = "89bc0b64bf0a9b84ef3532efaa66c23213affb70"
ROOT = pathlib.Path(__file__).resolve().parents[3]
OUT = ROOT / "artifacts/f1-f12-repair/round2"
PATCH = OUT / "patches/89bc0b64-to-r01-r10-source.patch"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def source_manifest(root: pathlib.Path) -> dict:
    raw = subprocess.check_output(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=root
    )
    rows = []
    for relative in sorted(set(raw.decode().split("\0"))):
        if not relative:
            continue
        if relative.startswith("artifacts/f1-f12-repair/") and not relative.endswith(".py"):
            continue
        if relative.endswith(".patch"):
            continue
        file_path = root / relative
        if file_path.is_file():
            content = file_path.read_bytes()
            rows.append({"path": relative, "bytes": len(content), "sha256": sha256(content)})
    return {
        "sourceIdentity": {"kind": "worktree", "base": BASE},
        "exclusions": [
            "artifacts/f1-f12-repair/** except *.py (generated evidence, reports, delivery files and synthetic test home)",
            "*.patch (delivery patches)",
            "git ignored generated files; git ls-files --cached --others --exclude-standard",
        ],
        "files": rows,
    }


def manifest_bytes(root: pathlib.Path) -> bytes:
    return (json.dumps(source_manifest(root), ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()


def build_patch() -> bytes:
    PATCH.parent.mkdir(parents=True, exist_ok=True)
    index_fd, index_name = tempfile.mkstemp(prefix="lingxi-r01-r10-index-")
    os.close(index_fd)
    os.unlink(index_name)
    env = os.environ.copy()
    env["GIT_INDEX_FILE"] = index_name
    try:
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
    current = manifest_bytes(ROOT)
    temp_parent = pathlib.Path(tempfile.mkdtemp(prefix="lingxi-r01-r10-replay-parent-"))
    replay = temp_parent / "worktree"
    added = False
    try:
        subprocess.run(
            ["git", "worktree", "add", "--detach", str(replay), BASE],
            cwd=ROOT, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        added = True
        subprocess.run(
            ["git", "apply", "--check", "--binary", str(PATCH)],
            cwd=replay, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        subprocess.run(
            ["git", "apply", "--binary", str(PATCH)],
            cwd=replay, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        replayed = manifest_bytes(replay)
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
        if added:
            subprocess.run(
                ["git", "worktree", "remove", "--force", str(replay)],
                cwd=ROOT, check=False, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
        shutil.rmtree(temp_parent, ignore_errors=True)


if __name__ == "__main__":
    patch = build_patch()
    print(json.dumps(replay_and_verify(patch), ensure_ascii=False, sort_keys=True))
