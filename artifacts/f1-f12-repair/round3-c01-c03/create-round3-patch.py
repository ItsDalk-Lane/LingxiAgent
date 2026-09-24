#!/usr/bin/env python3
"""round3（C01-C03）增量补丁生成与重放验证 — temp-index 技法（同 round2）。

防自我嵌套：补丁输出文件从 staging 摘除（否则每代嵌入上一代，体积递归膨胀，
曾涨到 116MB 超 GitHub 硬限）。临时 index 以真实 index 的已跟踪条目逐项回放
为起点：相对 BASE 新增、已跟踪却匹配 .gitignore 的证据文件在空 index 下会被
git add 当作未跟踪忽略文件跳过（C1 独立审查 F2，曾漏 932 条）。current 与
replay 两侧的源码 manifest 都从 git index blob（归一化字节）计算：Windows
autocrlf 会拆开工作树字节与 blob 字节（CRLF/LF），读工作树的对比在 Windows
上天然不一致；仓库 .gitattributes 强制 text=auto eol=lf，blob 字节与工作树
字节一致，因此重放结果再与完整冻结 SOURCE_MANIFEST.json 逐项对盘
（路径 + 字节数 + SHA-256），不只比较生成 index 与重放 index。
输出 JSON 摘要（exit 0 = VERIFIED）。
"""
import gzip
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile

BASE = "67dee5d2de9d3b9fc75ec5ef5c555e93c65b3ccd"
ROOT = pathlib.Path(__file__).resolve().parents[3]
OUT = ROOT / "artifacts/f1-f12-repair/round3-c01-c03"
# 存储为确定性 gzip（mtime=0）：补丁体积随分支积压增长，v0.1.43 起未压缩体积
# 超 GitHub 单文件 100MB 硬限；压缩后低于该限。重放验证仍对未压缩原始字节执行，语义不变。
PATCH = OUT / "patches/67dee5d2-to-round3-c01-c03.patch.gz"


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


def temp_index(prefix: str) -> str:
    fd, name = tempfile.mkstemp(prefix=prefix)
    os.close(fd)
    os.unlink(name)
    return name


def rows_from_index(index_name: str) -> list[dict]:
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
        raise SystemExit("cat-file --batch failed while building index manifest")
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
    return rows


def manifest_from_rows(rows: list[dict]) -> bytes:
    manifest = {"sourceIdentity": {"kind": "worktree", "base": BASE}, "exclusions": EXCLUSIONS, "files": rows}
    return (json.dumps(manifest, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()


def manifest_from_index(index_name: str) -> bytes:
    return manifest_from_rows(rows_from_index(index_name))


def stage_current(index_name: str) -> None:
    """当前已跟踪树 + 未忽略新文件 → 临时 index；补丁输出自身摘除（防自我嵌套）。

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
        ["git", "rm", "--cached", "-q", "--ignore-unmatch", "--", str(PATCH.relative_to(ROOT))],
        cwd=ROOT, env=env, check=True,
    )


def main() -> None:
    PATCH.parent.mkdir(parents=True, exist_ok=True)
    current_index = temp_index("lingxi-round3-current-")
    replay_index = temp_index("lingxi-round3-replay-")
    try:
        stage_current(current_index)
        current = manifest_from_index(current_index)

        env = os.environ.copy()
        env["GIT_INDEX_FILE"] = current_index
        patch = subprocess.check_output(
            ["git", "diff", "--binary", "--cached", BASE], cwd=ROOT, env=env
        )
        if not patch:
            raise SystemExit("round3 patch is empty")
        stored = gzip.compress(patch, compresslevel=6, mtime=0)
        PATCH.write_bytes(stored)

        replay_env = os.environ.copy()
        replay_env["GIT_INDEX_FILE"] = replay_index
        subprocess.run(["git", "read-tree", BASE], cwd=ROOT, env=replay_env, check=True)
        apply = subprocess.run(
            ["git", "apply", "--cached", "--binary", "--whitespace=nowarn", "-"],
            cwd=ROOT, env=replay_env, input=patch,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        )
        if apply.returncode != 0:
            raise SystemExit(f"patch replay failed: {apply.stdout.decode(errors='replace')[:2000]}")
        replay_rows = rows_from_index(replay_index)
        replayed = manifest_from_rows(replay_rows)
    finally:
        for name in (current_index, replay_index):
            if os.path.exists(name):
                os.unlink(name)

    identical = replayed == current
    # 与完整冻结 SOURCE_MANIFEST.json 逐项对盘（路径 + 字节数 + SHA-256），
    # 不只比较生成 index 与重放 index（C1 独立审查 F2）。
    frozen_raw = (OUT / "SOURCE_MANIFEST.json").read_bytes()
    frozen = json.loads(frozen_raw.decode())["files"]
    covers_frozen = replay_rows == frozen
    result = {
        "base": BASE,
        "result": "VERIFIED" if identical and covers_frozen else "MISMATCH",
        "patch": str(PATCH.relative_to(ROOT)),
        "patchBytes": len(stored),
        "patchSha256": sha256(stored),
        "patchUncompressedBytes": len(patch),
        "sourceManifestHash": sha256(current),
        "replayedSourceManifestHash": sha256(replayed),
        "frozenSourceManifestHash": sha256(frozen_raw),
        "frozenManifestEntries": len(frozen),
        "replayedMatchesFrozenManifest": covers_frozen,
    }
    print(json.dumps(result, ensure_ascii=False, indent=2))
    if not identical or not covers_frozen:
        if not covers_frozen:
            replay_by_path = {row["path"]: row for row in replay_rows}
            frozen_by_path = {row["path"]: row for row in frozen}
            diff_paths = sorted(
                (set(replay_by_path) ^ set(frozen_by_path))
                | {p for p in replay_by_path.keys() & frozen_by_path.keys()
                   if replay_by_path[p] != frozen_by_path[p]}
            )
            print(
                "replayed source tree does not cover frozen SOURCE_MANIFEST.json: "
                f"replayed={len(replay_rows)} frozen={len(frozen)} "
                f"firstDiff={json.dumps(diff_paths[:3], ensure_ascii=False)}"
            )
        raise SystemExit(1)


if __name__ == "__main__":
    main()
