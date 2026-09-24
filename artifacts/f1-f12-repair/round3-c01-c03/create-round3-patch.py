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
冻结对盘按两种可证明状态验收（C2 独立复核 F1 修复合同，与 round2 同构）：
source 状态要求当前清单==冻结清单全等（guard 红不否决）；纯审计 seal 状态要求
冻结清单==VERIFIED_SOURCE_SHA 真实 commit 全树、现有 diff guard 绿、且当前
清单==HEAD（防未提交/未跟踪改动），输出逐字段如实区分，任一不等即 MISMATCH。
两态共同的坐标合同（R2 独立验收 F1）：任一 VERIFIED 出口前，坐标必须经
`git cat-file -t` 精确判定为存在的原始 commit 对象（tree/tag 同样被
ls-tree/diff 接受，40 位格式检查不足，C3 独立复核 F1；source 快捷返回曾跳过
该校验）。交付 patch.gz 只在 VERIFIED 后原子替换，MISMATCH/异常不改写。
输出 JSON 摘要（exit 0 = VERIFIED）。
"""
import gzip
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile

# 证据/生产运行一律使用硬编码 BASE。LINGXI_PATCH_BASE_OVERRIDE 仅供 /tmp 合成
# 夹具里的负向场景测试复用同一段校验逻辑（证据测试断言该变量在真实运行时未设置）。
BASE = os.environ.get("LINGXI_PATCH_BASE_OVERRIDE") or "67dee5d2de9d3b9fc75ec5ef5c555e93c65b3ccd"
ROOT = pathlib.Path(__file__).resolve().parents[3]
OUT = ROOT / "artifacts/f1-f12-repair/round3-c01-c03"
# 存储为确定性 gzip（mtime=0）：补丁体积随分支积压增长，v0.1.43 起未压缩体积
# 超 GitHub 单文件 100MB 硬限；压缩后低于该限。重放验证仍对未压缩原始字节执行，语义不变。
PATCH = OUT / "patches/67dee5d2-to-round3-c01-c03.patch.gz"
VERIFIED_SOURCE_SHA_FILE = ROOT / ".sync-audit/verified-source-sha.txt"
SEAL_GUARD = ROOT / ".sync-audit/verify-post-verification-diff.mjs"


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


def rows_from_entries(entries: list[tuple[str, str]]) -> list[dict]:
    if not entries:
        return []
    batch_input = "\n".join(sha for _rel, sha in entries) + "\n"
    batch = subprocess.run(
        ["git", "cat-file", "--batch"], cwd=ROOT,
        input=batch_input.encode(), stdout=subprocess.PIPE,
    )
    if batch.returncode != 0:
        raise SystemExit("cat-file --batch failed while building manifest rows")
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
    return rows_from_entries(entries)


def git_object_type(sha: str) -> str | None:
    """对象存在返回其 Git 类型（commit/tree/tag/blob），不存在返回 None。"""
    probe = subprocess.run(
        ["git", "cat-file", "-t", sha], cwd=ROOT,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
    )
    if probe.returncode != 0:
        return None
    return probe.stdout.decode(errors="replace").strip()


def rows_from_commit(commit: str) -> list[dict]:
    """同一 keep() 范围内某 commit 全树的 path/bytes/sha256（blob 字节）。"""
    probe = subprocess.run(
        ["git", "ls-tree", "-r", "-z", commit], cwd=ROOT,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
    )
    if probe.returncode != 0:
        raise RuntimeError(
            f"git ls-tree failed for {commit}: {probe.stdout.decode(errors='replace')[:500]}"
        )
    entries = []
    for record in probe.stdout.decode().split("\0"):
        if not record:
            continue
        meta, relative = record.split("\t", 1)
        parts = meta.split(" ")
        if len(parts) != 3 or parts[1] != "blob":
            continue
        if keep(relative):
            entries.append((relative, parts[2]))
    entries.sort()
    return rows_from_entries(entries)


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


def first_diff_paths(a_rows: list[dict], b_rows: list[dict]) -> str:
    a_by_path = {row["path"]: row for row in a_rows}
    b_by_path = {row["path"]: row for row in b_rows}
    diff_paths = sorted(
        (set(a_by_path) ^ set(b_by_path))
        | {p for p in a_by_path.keys() & b_by_path.keys()
           if a_by_path[p] != b_by_path[p]}
    )
    return json.dumps(diff_paths[:3], ensure_ascii=False)


def seal_guard() -> tuple[bool, str]:
    """现有 post-verification diff guard（两份 allowlist 原样）：VERIFIED..HEAD 仅审计文件。"""
    probe = subprocess.run(
        ["node", str(SEAL_GUARD)], cwd=ROOT,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
    )
    return probe.returncode == 0, probe.stdout.decode(errors="replace").strip()


def build_patch() -> tuple[bytes, bytes, list[dict], bytes]:
    """仅在内存中生成补丁字节，不写交付路径（与 round2 同构）：
    交付 patch.gz 只在全部适用条件 VERIFIED 后由 deliver_patch() 原子替换，
    MISMATCH 或异常时保留原字节（原本不存在则仍不存在）。"""
    current_index = temp_index("lingxi-round3-current-")
    try:
        stage_current(current_index)
        current_rows = rows_from_index(current_index)
        current = manifest_from_rows(current_rows)
        env = os.environ.copy()
        env["GIT_INDEX_FILE"] = current_index
        patch = subprocess.check_output(
            ["git", "diff", "--binary", "--cached", BASE], cwd=ROOT, env=env
        )
    finally:
        pathlib.Path(current_index).unlink(missing_ok=True)
    if not patch:
        raise SystemExit("round3 patch is empty")
    stored = gzip.compress(patch, compresslevel=6, mtime=0)
    return patch, stored, current_rows, current


def deliver_patch(stored: bytes) -> None:
    """同目录临时文件 + 原子替换写入交付路径；异常时删除临时文件，
    交付路径保持原字节（原本不存在则仍不存在）。"""
    PATCH.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp_name = tempfile.mkstemp(prefix=f".{PATCH.name}.", suffix=".tmp", dir=PATCH.parent)
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(stored)
        os.replace(tmp_name, PATCH)
    except BaseException:
        pathlib.Path(tmp_name).unlink(missing_ok=True)
        raise


def replay_patch(patch: bytes) -> list[dict]:
    replay_index = temp_index("lingxi-round3-replay-")
    try:
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
        return rows_from_index(replay_index)
    finally:
        pathlib.Path(replay_index).unlink(missing_ok=True)


def evaluate(
    *, patch: bytes, stored: bytes,
    current_rows: list[dict], current: bytes,
    replay_rows: list[dict], replayed: bytes,
) -> dict:
    """双状态验收（source / 纯审计 seal），与 round2 同构。

    不变式：replayedMatchesCurrent（重放清单==当前清单逐路径、字节数、SHA-256）。
    source 状态：当前清单==完整冻结 SOURCE_MANIFEST.json（C1 F2 严格对盘；
    guard 红不否决）。seal 状态：frozenMatchesVerifiedCommit（冻结==VERIFIED
    commit 全树）且 sealGuardPassed（VERIFIED..HEAD 仅审计白名单）且
    currentMatchesHead（当前==HEAD，防未提交/未跟踪改动）。坐标校验对每个
    独立成功出口生效（R2 独立验收 F1）：source/seal 分叉后、任一
    result=VERIFIED 返回前，VERIFIED_SOURCE_SHA 必须经 `git cat-file -t`
    精确判定为存在的原始 commit 对象（拒绝 tree/tag/blob/缺失/非法/缺文件）；
    两态 VERIFIED 都回报 verifiedSourceSha 与 verifiedSourceObjectType="commit"，
    非法对象保留实际 state、result=MISMATCH 与失败原因，exit 1。任一不等即
    result=MISMATCH，failures 逐条记录；seal 状态 replayedMatchesFrozenManifest
    必为 false，不得解释为逐字全等。
    """
    frozen_raw = (OUT / "SOURCE_MANIFEST.json").read_bytes()
    frozen = json.loads(frozen_raw.decode())["files"]
    result = {
        "base": BASE,
        "patch": str(PATCH.relative_to(ROOT)),
        "patchBytes": len(stored),
        "patchSha256": sha256(stored),
        "patchUncompressedBytes": len(patch),
        "sourceManifestHash": sha256(current),
        "replayedSourceManifestHash": sha256(replayed),
        "frozenSourceManifestHash": sha256(frozen_raw),
        "frozenManifestEntries": len(frozen),
        "replayedMatchesCurrent": replay_rows == current_rows,
        "replayedMatchesFrozenManifest": replay_rows == frozen,
        "state": None,
        "verifiedSourceSha": None,
        "verifiedSourceObjectType": None,
        "frozenMatchesVerifiedCommit": None,
        "currentMatchesHead": None,
        "sealGuardPassed": None,
        "failures": [],
        "result": "MISMATCH",
    }
    if not result["replayedMatchesCurrent"]:
        result["failures"].append(
            f"replayed source manifest mismatch: current={sha256(current)} "
            f"replay={sha256(replayed)} firstDiff={first_diff_paths(current_rows, replay_rows)}"
        )
        return result
    result["state"] = "source" if replay_rows == frozen else "seal"
    # 坐标校验在 source/seal 共同成功出口之前：两态都必须先证明
    # VERIFIED_SOURCE_SHA 字面是存在的原始 commit 对象（R2 独立验收 F1：
    # source 快捷返回曾跳过该校验，tree 坐标也能报 VERIFIED）。
    try:
        verified = VERIFIED_SOURCE_SHA_FILE.read_text().strip()
    except OSError:
        result["failures"].append(
            f"缺少坐标文件 {VERIFIED_SOURCE_SHA_FILE.relative_to(ROOT)}"
        )
        return result
    if not re.fullmatch(r"[0-9a-f]{40}", verified):
        result["failures"].append(f"VERIFIED_SOURCE_SHA 非 40 位十六进制: {verified}")
        return result
    result["verifiedSourceSha"] = verified
    object_type = git_object_type(verified)
    result["verifiedSourceObjectType"] = object_type
    # 坐标必须精确指向 commit 对象：tree/tag 同样被 git ls-tree / git diff 接受，
    # 40 位格式检查不足以证明坐标是真实源码提交（C3 独立复核 F1）。
    if object_type != "commit":
        result["failures"].append(
            "VERIFIED_SOURCE_SHA 必须指向真实 commit 对象"
            f"（git cat-file -t 实际返回 {object_type or '对象不存在'}）: {verified}"
        )
        return result
    if result["state"] == "source":
        result["result"] = "VERIFIED"
        return result
    try:
        verified_rows = rows_from_commit(verified)
    except RuntimeError as exc:
        result["failures"].append(str(exc))
        return result
    head_rows = rows_from_commit("HEAD")
    result["frozenMatchesVerifiedCommit"] = verified_rows == frozen
    result["currentMatchesHead"] = current_rows == head_rows
    guard_ok, guard_output = seal_guard()
    result["sealGuardPassed"] = guard_ok
    if not result["frozenMatchesVerifiedCommit"]:
        result["failures"].append(
            "frozen SOURCE_MANIFEST.json does not match VERIFIED commit tree: "
            f"verified={verified} frozenEntries={len(frozen)} "
            f"firstDiff={first_diff_paths(verified_rows, frozen)}"
        )
    if not result["currentMatchesHead"]:
        result["failures"].append(
            "current source manifest does not match HEAD (uncommitted or untracked "
            f"source changes): firstDiff={first_diff_paths(head_rows, current_rows)}"
        )
    if not guard_ok:
        result["failures"].append(f"post-verification diff guard failed: {guard_output[-500:]}")
    if not result["failures"]:
        result["result"] = "VERIFIED"
    return result


def main() -> None:
    patch, stored, current_rows, current = build_patch()
    replay_rows = replay_patch(patch)
    replayed = manifest_from_rows(replay_rows)
    result = evaluate(
        patch=patch, stored=stored,
        current_rows=current_rows, current=current,
        replay_rows=replay_rows, replayed=replayed,
    )
    print(json.dumps(result, ensure_ascii=False, indent=2))
    if result["result"] != "VERIFIED":
        for failure in result["failures"]:
            print(failure, file=sys.stderr)
        raise SystemExit(1)
    # 只在全部适用条件 VERIFIED 后原子替换交付补丁；MISMATCH/异常不改写。
    deliver_patch(stored)


if __name__ == "__main__":
    main()
