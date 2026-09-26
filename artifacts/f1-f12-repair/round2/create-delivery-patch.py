#!/usr/bin/env python3
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
BASE = os.environ.get("LINGXI_PATCH_BASE_OVERRIDE") or "89bc0b64bf0a9b84ef3532efaa66c23213affb70"
ROOT = pathlib.Path(__file__).resolve().parents[3]
OUT = ROOT / "artifacts/f1-f12-repair/round2"
# 存储为确定性 gzip（mtime=0）：补丁体积随分支积压增长，v0.1.43 起未压缩体积
# 超 GitHub 单文件 100MB 硬限；压缩后低于该限。重放验证仍对未压缩原始字节执行，语义不变。
PATCH = OUT / "patches/89bc0b64-to-r01-r10-source.patch.gz"
VERIFIED_SOURCE_SHA_FILE = ROOT / ".sync-audit/verified-source-sha.txt"
SEAL_GUARD = ROOT / ".sync-audit/verify-post-verification-diff.mjs"

# 受控分片交付（R01 阶段验收 R2 F03）：R01 证据树进入 BASE→current 全树 diff 后，
# 重生成的确定性 gzip 达 267MB（round3 577MB），超 GitHub 普通 Git 单文件
# 100 MiB 推送硬限，单体交付不可推送。超过 SHARD_BYTES 时改为分片交付：
# 补丁字节本身不变（patchSha256/patchBytes 仍钉住完整 gzip 载荷），仅存储分帧为
#   <patch>.part-00001 … .part-NNNNN + <patch>.shards.json（格式 patch-gzip-shards/v1，
#   逐片钉住 path/bytes/sha256，清单自身亦记录载荷总哈希）；消费者按序重组后必须
#   复算出同一 patchSha256——完整性校验语义不损失。45,000,000 B 同时低于
#   GitHub 100 MiB 硬限与 50 MB 警告线。LINGXI_PATCH_SHARD_BYTES_OVERRIDE 仅供
#   /tmp 合成夹具测试分片路径（证据测试断言真实运行时未设置，与 BASE 覆盖同约定）。
SHARD_BYTES = int(os.environ.get("LINGXI_PATCH_SHARD_BYTES_OVERRIDE") or 45_000_000)
SHARD_SUFFIX = ".part-"
SHARDS_MANIFEST_SUFFIX = ".shards.json"
SHARDS_FORMAT = "patch-gzip-shards/v1"


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


def rows_from_entries(entries: list[tuple[str, str]]) -> list[dict]:
    """entries 为 (path, blob_sha) 有序列表；逐条计算 path/bytes/sha256（blob 字节）。

    不再读工作树字节：Windows autocrlf 会把工作树字节与 blob 字节拆开
    （CRLF/LF），导致 current 与 replay 天然不一致（R10-09 Windows CI 实测）。
    两侧统一走 blob 后，行尾归一化由 git 同一套规则处理；仓库 .gitattributes
    强制 text=auto eol=lf，blob 字节与工作树字节一致，因此重放结果可再与
    冻结 SOURCE_MANIFEST.json（工作树字节）逐项对盘。
    """
    if not entries:
        return []
    batch_input = "\n".join(sha for _rel, sha in entries) + "\n"
    batch = subprocess.run(
        ["git", "cat-file", "--batch"], cwd=ROOT,
        input=batch_input.encode(), stdout=subprocess.PIPE,
    )
    if batch.returncode != 0:
        raise RuntimeError("cat-file --batch failed while building manifest rows")
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


def build_patch() -> tuple[bytes, bytes]:
    """仅在内存中生成补丁字节，不写交付路径。

    交付 patch.gz 只在全部适用条件 VERIFIED 后由 deliver_patch() 原子替换
    （R2 独立验收连带修复）：此前先写交付补丁再验收，MISMATCH 或异常时交付
    字节已被覆盖，会误导随后的历史哈希与工作区状态判读。
    """
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
    return content, stored


def _atomic_write(path: pathlib.Path, data: bytes) -> None:
    """同目录临时文件 + 原子替换写入；异常时删除临时文件。"""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp_name = tempfile.mkstemp(prefix=f".{path.name}.", suffix=".tmp", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(data)
        os.replace(tmp_name, path)
    except BaseException:
        pathlib.Path(tmp_name).unlink(missing_ok=True)
        raise


def _stale_shard_paths(keep_rel: set) -> list:
    """交付族中不在本次目标集内的既有分片文件（按名枚举，不含清单/单体）。"""
    if not PATCH.parent.is_dir():
        return []
    return [p for p in sorted(PATCH.parent.glob(PATCH.name + SHARD_SUFFIX + "*"))
            if str(p.relative_to(ROOT)) not in keep_rel]


def plan_delivery(stored: bytes) -> tuple:
    """计算交付形态：(result 字段 dict, 分片清单字节或 None)。

    单体（len(stored) <= SHARD_BYTES）：原合同原路径，无清单无分片。
    分片：逐片 path/bytes/sha256 + 清单（含载荷总哈希），单体路径不落盘。
    """
    if len(stored) <= SHARD_BYTES:
        return ({
            "patchFormat": "single",
            "patchShardBytes": SHARD_BYTES,
            "patchManifest": None,
            "patchShards": None,
            "patchMaxDeliveredFileBytes": len(stored),
        }, None)
    shards = []
    count = (len(stored) + SHARD_BYTES - 1) // SHARD_BYTES
    for i in range(count):
        chunk = stored[i * SHARD_BYTES:(i + 1) * SHARD_BYTES]
        shards.append({
            "path": f"{PATCH.relative_to(ROOT)}{SHARD_SUFFIX}{i + 1:05d}",
            "bytes": len(chunk),
            "sha256": sha256(chunk),
        })
    manifest = {
        "format": SHARDS_FORMAT,
        "patch": str(PATCH.relative_to(ROOT)),
        "patchBytes": len(stored),
        "patchSha256": sha256(stored),
        "shardBytes": SHARD_BYTES,
        "shards": shards,
    }
    manifest_bytes = (json.dumps(manifest, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()
    return ({
        "patchFormat": "sharded",
        "patchShardBytes": SHARD_BYTES,
        "patchManifest": str(PATCH.relative_to(ROOT)) + SHARDS_MANIFEST_SUFFIX,
        "patchShards": shards,
        "patchMaxDeliveredFileBytes": max(s["bytes"] for s in shards),
    }, manifest_bytes)


def deliver_patch(stored: bytes, plan: dict, manifest_bytes) -> None:
    """按 plan 交付；只在全部适用条件 VERIFIED 后被调用（MISMATCH 不改写）。

    分片交付顺序：逐片原子写入 → 清单最后原子就位（清单 = 分片交付的提交点）
    → 回收单体与陈旧分片。中途 IO 异常留下的半成品会被消费者的逐片/总哈希
    复算识别，不可能被当成有效交付。单体与分片两种形态不共存（互斥回收）。
    """
    if plan["patchFormat"] == "single":
        _atomic_write(PATCH, stored)
        pathlib.Path(str(PATCH) + SHARDS_MANIFEST_SUFFIX).unlink(missing_ok=True)
        for stale in _stale_shard_paths(set()):
            stale.unlink()
        return
    keep_rel = set()
    for i, shard in enumerate(plan["patchShards"]):
        chunk = stored[i * SHARD_BYTES:(i + 1) * SHARD_BYTES]
        _atomic_write(ROOT / shard["path"], chunk)
        keep_rel.add(shard["path"])
    _atomic_write(ROOT / plan["patchManifest"], manifest_bytes)
    PATCH.unlink(missing_ok=True)
    for stale in _stale_shard_paths(keep_rel):
        stale.unlink()


def replay_and_verify(patch: bytes, stored: bytes) -> dict:
    """重放校验 + 双状态验收（source / 纯审计 seal）。

    不变式：补丁从 BASE 重放后的清单必须与当前临时 index 清单逐路径、字节数、
    SHA-256 全等（replayedMatchesCurrent），不等即 MISMATCH。
    随后按两种可证明的状态验收（C2 独立复核 F1 修复合同）：
    - source 状态：当前清单与完整冻结 SOURCE_MANIFEST.json 全部条目全等
      （C1 F2 的严格对盘原样保留；seal guard 红不否决该状态）。
    - 纯审计 seal 状态：冻结清单与 VERIFIED_SOURCE_SHA 指向的真实 commit 全树
      逐项全等（frozenMatchesVerifiedCommit）；现有 diff guard 绿
      （VERIFIED..HEAD 仅审计白名单，sealGuardPassed）；当前清单与 HEAD 在
      同一范围逐项全等（currentMatchesHead，防未提交/未跟踪源码与审计改动）。
    坐标校验对每个独立成功出口生效（R2 独立验收 F1）：source/seal 分叉后、
    任一 result=VERIFIED 返回前，VERIFIED_SOURCE_SHA 必须是存在的原始
    commit 对象（40 位格式 + git cat-file -t 精确判定；tree/tag 同样被
    ls-tree/diff 接受，格式检查不足，C3 独立复核 F1）；source 快捷返回曾
    跳过该校验。两态 VERIFIED 都如实回报 verifiedSourceSha 与
    verifiedSourceObjectType="commit"；非法对象保留实际 state、
    result=MISMATCH 与失败原因，exit 1。
    任一适用条件不满足即 result=MISMATCH，failures 逐条记录差异路径；
    replayedMatchesFrozenManifest 在 seal 状态必为 false，不得解释为逐字全等。
    """
    current_index = temp_index("lingxi-r01-r10-current-")
    replay_index = temp_index("lingxi-r01-r10-replay-")
    try:
        stage_current(current_index)
        current_rows = rows_from_index(current_index)
        current = manifest_from_rows(current_rows)

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
    finally:
        pathlib.Path(current_index).unlink(missing_ok=True)
        pathlib.Path(replay_index).unlink(missing_ok=True)

    # 与完整冻结 SOURCE_MANIFEST.json 逐项对盘（路径 + 字节数 + SHA-256），
    # 不只比较生成 index 与重放 index（C1 独立审查 F2）。
    frozen_raw = (OUT / "SOURCE_MANIFEST.json").read_bytes()
    frozen = json.loads(frozen_raw.decode())["files"]
    plan, _manifest_bytes = plan_delivery(stored)
    result = {
        "base": BASE,
        "patch": str(PATCH.relative_to(ROOT)),
        "patchBytes": len(stored),
        "patchSha256": sha256(stored),
        "patchUncompressedBytes": len(patch),
        **plan,
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


if __name__ == "__main__":
    patch, stored = build_patch()
    outcome = replay_and_verify(patch, stored)
    print(json.dumps(outcome, ensure_ascii=False, sort_keys=True))
    if outcome["result"] != "VERIFIED":
        for failure in outcome["failures"]:
            print(failure, file=sys.stderr)
        sys.exit(1)
    # 只在全部适用条件 VERIFIED 后原子交付；MISMATCH/异常不改写。
    plan, manifest_bytes = plan_delivery(stored)
    deliver_patch(stored, plan, manifest_bytes)
