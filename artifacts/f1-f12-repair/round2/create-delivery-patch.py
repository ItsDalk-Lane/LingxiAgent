#!/usr/bin/env python3
import gzip
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile

BASE = "89bc0b64bf0a9b84ef3532efaa66c23213affb70"
ROOT = pathlib.Path(__file__).resolve().parents[3]
OUT = ROOT / "artifacts/f1-f12-repair/round2"
# 存储为确定性 gzip（mtime=0）：补丁体积随分支积压增长，v0.1.43 起未压缩体积
# 超 GitHub 单文件 100MB 硬限；压缩后低于该限。重放验证仍对未压缩原始字节执行，语义不变。
PATCH = OUT / "patches/89bc0b64-to-r01-r10-source.patch.gz"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def keep(relative: str) -> bool:
    if relative.startswith("artifacts/f1-f12-repair/") and not relative.endswith(".py"):
        return False
    return not relative.endswith(".patch")


def stage_current(index_name: str) -> None:
    """当前已跟踪树 + 未忽略新文件 → 临时 index。补丁与校验共用。

    起点是真实 index 的全部已跟踪条目（ls-files -s 经 update-index --index-info
    逐项回放），而非 BASE read-tree：相对 BASE 新增、已跟踪却匹配 .gitignore 的
    证据文件（artifacts/refactor-2026/**、artifacts/rust-tauri/R00/**）在空 index
    下会被 git add 当作未跟踪忽略文件跳过（C1 独立审查 F2，曾漏 932 条）。
    回放后 git add -A 只再纳入未忽略的新文件。
    """
    probe_env = os.environ.copy()
    probe_env.pop("GIT_INDEX_FILE", None)
    cached = subprocess.check_output(["git", "ls-files", "-s", "-z"], cwd=ROOT, env=probe_env)
    env = os.environ.copy()
    env["GIT_INDEX_FILE"] = index_name
    subprocess.run(
        ["git", "update-index", "-z", "--index-info"],
        cwd=ROOT, env=env, input=cached, check=True,
    )
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
        # 显式路径逐个加入：忽略规则对显式路径仍生效，-f 保证已跟踪脚本必入。
        subprocess.run(["git", "add", "-f", "--", *python_files], cwd=ROOT, env=env, check=True)


def rows_from_index(index_name: str) -> list[dict]:
    """从临时 index 的 blob（git 归一化字节）逐条计算 path/bytes/sha256。

    不再读工作树字节：Windows autocrlf 会把工作树字节与 blob 字节拆开
    （CRLF/LF），导致 current 与 replay 天然不一致（R10-09 Windows CI 实测）。
    两侧统一走 index blob 后，行尾归一化由 git 同一套规则处理；仓库
    .gitattributes 强制 text=auto eol=lf，blob 字节与工作树字节一致，
    因此重放结果可再与冻结 SOURCE_MANIFEST.json（工作树字节）逐项对盘。
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
    return rows


def manifest_from_rows(rows: list[dict]) -> bytes:
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


def manifest_from_index(index_name: str) -> bytes:
    return manifest_from_rows(rows_from_index(index_name))


def temp_index(prefix: str) -> str:
    fd, name = tempfile.mkstemp(prefix=prefix)
    os.close(fd)
    os.unlink(name)
    return name


def build_patch() -> tuple[bytes, bytes]:
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
    stored = gzip.compress(content, compresslevel=6, mtime=0)
    PATCH.write_bytes(stored)
    return content, stored


def replay_and_verify(patch: bytes, stored: bytes) -> dict:
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
        replay_rows = rows_from_index(replay_index)
        replayed = manifest_from_rows(replay_rows)
        if replayed != current:
            raise RuntimeError(
                f"replayed source manifest mismatch: current={sha256(current)} replay={sha256(replayed)}"
            )
        # 与完整冻结 SOURCE_MANIFEST.json 逐项对盘（路径 + 字节数 + SHA-256），
        # 不只比较生成 index 与重放 index（C1 独立审查 F2）。
        frozen_raw = (OUT / "SOURCE_MANIFEST.json").read_bytes()
        frozen = json.loads(frozen_raw.decode())["files"]
        if replay_rows != frozen:
            replay_by_path = {row["path"]: row for row in replay_rows}
            frozen_by_path = {row["path"]: row for row in frozen}
            diff_paths = sorted(
                (set(replay_by_path) ^ set(frozen_by_path))
                | {p for p in replay_by_path.keys() & frozen_by_path.keys()
                   if replay_by_path[p] != frozen_by_path[p]}
            )
            raise RuntimeError(
                "replayed source tree does not cover frozen SOURCE_MANIFEST.json: "
                f"replayed={len(replay_rows)} frozen={len(frozen)} "
                f"firstDiff={json.dumps(diff_paths[:3], ensure_ascii=False)}"
            )
        return {
            "base": BASE,
            "patch": str(PATCH.relative_to(ROOT)),
            "patchBytes": len(stored),
            "patchSha256": sha256(stored),
            "patchUncompressedBytes": len(patch),
            "sourceManifestHash": sha256(current),
            "replayedSourceManifestHash": sha256(replayed),
            "frozenSourceManifestHash": sha256(frozen_raw),
            "frozenManifestEntries": len(frozen),
            "replayedMatchesFrozenManifest": True,
            "result": "VERIFIED",
        }
    finally:
        pathlib.Path(current_index).unlink(missing_ok=True)
        pathlib.Path(replay_index).unlink(missing_ok=True)


if __name__ == "__main__":
    patch, stored = build_patch()
    print(json.dumps(replay_and_verify(patch, stored), ensure_ascii=False, sort_keys=True))
