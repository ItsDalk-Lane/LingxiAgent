#!/usr/bin/env python3
"""round3（C01-C03）增量补丁生成与重放验证 — 沿用 round2/create-delivery-patch.py 的
temp-index 技法：从干净基线 67dee5d2 read-tree，git add -A 捕获全部非忽略变更
（源码 + 交付证据），diff 出单一补丁；随后用独立的第二个 temp index 重放补丁，
按源码 manifest 的同一排除规则重算摘要，证明重放树与当前树源码逐字节一致。
输出 JSON 摘要（exit 0 = VERIFIED）。
"""
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile

BASE = "67dee5d2de9d3b9fc75ec5ef5c555e93c65b3ccd"
ROOT = pathlib.Path(__file__).resolve().parents[3]
OUT = ROOT / "artifacts/f1-f12-repair/round3-c01-c03"
PATCH = OUT / "patches/67dee5d2-to-round3-c01-c03.patch"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


EXCLUSIONS = [
    "artifacts/f1-f12-repair/** except *.py (generated evidence, reports, delivery files and synthetic test home)",
    "*.patch (delivery patches)",
    "git ignored generated files; git ls-files --cached --others --exclude-standard",
]


def keep(relative: str) -> bool:
    if relative.startswith("artifacts/f1-f12-repair/") and not relative.endswith(".py"):
        return False
    return not relative.endswith(".patch")


def source_manifest(root: pathlib.Path) -> dict:
    raw = subprocess.check_output(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=root
    )
    rows = []
    for relative in sorted(set(raw.decode().split("\0"))):
        if not relative or not keep(relative):
            continue
        file_path = root / relative
        if file_path.is_file():
            content = file_path.read_bytes()
            rows.append({"path": relative, "bytes": len(content), "sha256": sha256(content)})
    return {"sourceIdentity": {"kind": "worktree", "base": BASE}, "exclusions": EXCLUSIONS, "files": rows}


def manifest_bytes(root: pathlib.Path) -> bytes:
    return (json.dumps(source_manifest(root), ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()


def temp_index(prefix: str) -> str:
    fd, name = tempfile.mkstemp(prefix=prefix)
    os.close(fd)
    os.unlink(name)
    return name


def replay_manifest(patch: bytes) -> bytes:
    """BASE + 补丁 → 独立 temp index → 按排除规则重算源码 manifest 字节。"""
    index = temp_index("lingxi-round3-replay-")
    env = os.environ.copy()
    env["GIT_INDEX_FILE"] = index
    try:
        subprocess.run(["git", "read-tree", BASE], cwd=ROOT, env=env, check=True)
        apply = subprocess.run(
            ["git", "apply", "--cached", "--whitespace=nowarn", "-"],
            cwd=ROOT, env=env, input=patch,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        )
        if apply.returncode != 0:
            raise SystemExit(f"patch replay failed: {apply.stdout.decode(errors='replace')[:2000]}")
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
            raise SystemExit("replay cat-file failed")
        rows = []
        offset = 0
        stdout = batch.stdout
        for relative, blob_sha in entries:
            header_end = stdout.index(0x0A, offset)
            header = stdout[offset:header_end].decode()
            if not header.startswith(f"{blob_sha} blob "):
                raise SystemExit(f"cat-file record mismatch for {relative}: {header}")
            size = int(header.split(" ")[2])
            start = header_end + 1
            end = start + size
            blob = stdout[start:end]
            rows.append({"path": relative, "bytes": len(blob), "sha256": sha256(blob)})
            offset = end + 1
        manifest = {"sourceIdentity": {"kind": "worktree", "base": BASE}, "exclusions": EXCLUSIONS, "files": rows}
        return (json.dumps(manifest, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()
    finally:
        if os.path.exists(index):
            os.unlink(index)


def main() -> None:
    PATCH.parent.mkdir(parents=True, exist_ok=True)
    source_before = manifest_bytes(ROOT)

    index = temp_index("lingxi-round3-index-")
    env = os.environ.copy()
    env["GIT_INDEX_FILE"] = index
    try:
        subprocess.run(["git", "read-tree", BASE], cwd=ROOT, env=env, check=True)
        subprocess.run(["git", "add", "-A", "--", "."], cwd=ROOT, env=env, check=True)
        # 防自我嵌套：补丁输出文件不得进入补丁（否则每代嵌入上一代，体积递归膨胀）。
        # 用 rm --cached 把它从 temp index 摘掉（reset 会回到 HEAD 版本，仍会嵌入）。
        subprocess.run(
            ["git", "rm", "--cached", "-q", "--ignore-unmatch", "--", str(PATCH.relative_to(ROOT))],
            cwd=ROOT, env=env, check=True,
        )
        patch = subprocess.check_output(
            ["git", "diff", "--binary", "--cached", BASE], cwd=ROOT, env=env
        )
    finally:
        if os.path.exists(index):
            os.unlink(index)
    if not patch:
        raise SystemExit("round3 patch is empty")
    PATCH.write_bytes(patch)

    replayed = replay_manifest(patch)
    identical = replayed == source_before
    result = {
        "base": BASE,
        "result": "VERIFIED" if identical else "MISMATCH",
        "patch": str(PATCH.relative_to(ROOT)),
        "patchBytes": len(patch),
        "patchSha256": sha256(patch),
        "sourceManifestHash": sha256(source_before),
        "replayedSourceManifestHash": sha256(replayed),
    }
    print(json.dumps(result, ensure_ascii=False, indent=2))
    if not identical:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
