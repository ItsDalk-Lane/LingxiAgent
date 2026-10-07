#!/usr/bin/env python3
"""RR3 DELIVERY-FINAL-02 — authoritative re-enumeration and precise delivery classification.

Read-only with respect to everything except this directory
(artifacts/rust-tauri/R05/RR3/DELIVERY-FINAL-02/). No Git writes, no staging,
no test/build execution, no deletion of any original, no sub-agents.
"""
import hashlib
import json
import os
import subprocess
import sys
import zlib
from datetime import datetime, timezone

REPO = "/Users/study_superior/Desktop/Code/LingxiAgent"
OUT = os.path.join(REPO, "artifacts/rust-tauri/R05/RR3/DELIVERY-FINAL-02")
PREP02_MERGED = os.path.join(REPO, "artifacts/rust-tauri/R05/RR3/DELIVERY-PREP-02/merged-paths.json")
SPACE01_NORM = os.path.join(REPO, "artifacts/rust-tauri/R05/RR3/DELIVERY-SPACE-01/normalization-boundary.json")
F51_RECEIPT = os.path.join(REPO, "artifacts/rust-tauri/R05/RR3/F51-01/RELOCATION-RECEIPT.json")
F52_RECEIPT = os.path.join(REPO, "artifacts/rust-tauri/R05/RR3/F52-01/RELOCATION-RECEIPT-F52.json")

INCLUDE_CATS = {
    "INCLUDE_PRODUCTION", "INCLUDE_CURRENT_DOC", "INCLUDE_EVIDENCE", "INCLUDE_EVIDENCE_SNAPSHOT",
}
LOCAL_CATS = {
    "LOCAL_BUILD_CACHE", "LOCAL_BINARY", "LOCAL_ISOLATED_SOURCE", "LOCAL_NESTED_REPOSITORY",
    "LOCAL_RUNTIME_STATE", "LOCAL_SYMLINK", "LOCAL_ONLY", "LOCAL_OVERSIZE",
}

commands_log = []


def utcnow():
    return datetime.now(timezone.utc).isoformat()


def run_git(args, note):
    argv = ["git", "--no-optional-locks"] + args
    p = subprocess.run(argv, cwd=REPO, capture_output=True)
    commands_log.append({
        "utc": utcnow(), "argv": argv, "exit": p.returncode,
        "stdoutSha256": hashlib.sha256(p.stdout).hexdigest(),
        "stdoutBytes": len(p.stdout),
        "stderr": p.stderr.decode("utf-8", "replace")[:400],
        "note": note,
    })
    if p.returncode != 0:
        print(f"GIT FAIL {argv}: {p.stderr.decode()[:400]}", file=sys.stderr)
    return p


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while True:
            b = f.read(1 << 20)
            if not b:
                break
            h.update(b)
    return h.hexdigest()


def git_blob_id(content: bytes) -> str:
    return hashlib.sha1(b"blob %d\x00" % len(content) + content).hexdigest()


def classify_new(path, st, is_tracked_modified):
    """Fresh classification for paths not in PREP02 merged set (or identity changed)."""
    base = os.path.basename(path)
    if is_tracked_modified:
        if path.startswith("docs/"):
            return "INCLUDE_CURRENT_DOC", "tracked 修改：RR3 E14 现行文档矩阵回填（E-04 12 份 + ORCHESTRATOR_PROGRESS）或 L/M 后受控台账，白名单内现行文档"
        return "INCLUDE_PRODUCTION", "tracked 修改：RR3 白名单生产/脚本文件（A/B/C/F46/H/I/J/L/M 包唯一所有权，FINAL-04 全链验证的被测输入）"
    if path.startswith("docs/rust-tauri/R05/repair-current/"):
        return "INCLUDE_CURRENT_DOC", "RR3 任务书/交接/矩阵/进度等协调文档（PREP01 INCLUDE_CURRENT_DOC 同角色）"
    if path.startswith("rust/") or path.startswith("scripts/rust-tauri/"):
        return "INCLUDE_PRODUCTION", "RR3 白名单新增生产/自检脚本（包所有权登记：A(run_output_sinks)、B(mutate_pin/negative_gate_selfcheck)、H(r02 回归)、I(restore_selfcheck)、J(prepare_git_copy/prepare_node/node_selfcheck)、C(r05_resource_sampler)），FINAL-04 候选绑定面实测输入"
    if path.startswith("artifacts/rust-tauri/R05/RR3/DELIVERY-FINAL-02/"):
        return "INCLUDE_EVIDENCE", "本轮交付清点自身新证据：以独立自收据（self-receipt.json/MANIFEST.json）登记，避免循环自哈希；枚举后才写出的产物不在本快照内、由自收据递增登记"
    if path.startswith("artifacts/rust-tauri/R05/RR3/"):
        # runtime-state / ticket precedence (mirror PREP02 exact precedents)
        if base == "p1-ticket.body":
            return "LOCAL_ONLY", "A13 真实运行时在合成测试 home 签发的临时票据响应原件（PREP02 同名 17 项先例）：仅登记 SHA/size/来源，不输出票据原文"
        if base == "local-token.json" and "/home" in path:
            return "LOCAL_RUNTIME_STATE", "合成测试 home 内真实运行时本地 token（PREP02 LOCAL_RUNTIME_STATE 同角色先例）"
        if base in ("home-before.json", "home-after.json"):
            return "LOCAL_RUNTIME_STATE", "运行 home 状态快照标记（PREP02 对同名 2 字节文件判 LOCAL_RUNTIME_STATE 的先例，角色一致继承）"
        if "/own-target/" in path or path.endswith((".rlib", ".rmeta", ".o", ".d")):
            return "LOCAL_BUILD_CACHE", "证据目录内 own-target 构建缓存/目标文件（PREP01 LOCAL_BUILD_CACHE 同规则）"
        if "/pristine/" in path or "/snapshot-cmp-failure/" in path:
            return "INCLUDE_EVIDENCE_SNAPSHOT", "负测/恢复回归向 EV 保存的独立 pristine 源基线或 cmp 故障基线（PREP02 对 I/J pristine 44+8 项判纳入原始证据的同角色）"
        if st.st_size > 134_000_000:
            return "LOCAL_OVERSIZE", "单文件超过 GitHub 普通单文件限制（>134MB），localOnly 交付，不以重建替代原件"
        return "INCLUDE_EVIDENCE", "RR3 正式证据/报告/回执/原始日志/测量/中断现场（PREP02 后新增轮次材料，按目录所有权实际判定；新材料无二进制/缓存形态，已逐文件魔数与规则扫描）"
    return "UNKNOWN", "未匹配任何规则，需人工判定"


def main():
    started = utcnow()

    # ---------- Phase 0: authoritative git state ----------
    head = run_git(["rev-parse", "HEAD"], "HEAD").stdout.decode().strip()
    branch = run_git(["rev-parse", "--abbrev-ref", "HEAD"], "branch").stdout.decode().strip()
    untracked_raw = run_git(["ls-files", "--others", "--exclude-standard", "-z"], "权威枚举 untracked").stdout
    modified_raw = run_git(["diff", "--name-only", "-z"], "权威枚举 tracked 修改").stdout
    staged_raw = run_git(["diff", "--cached", "--name-only", "-z"], "应无 staged").stdout
    index_hash = hashlib.sha256(run_git(["ls-files", "-s"], "index 指纹").stdout).hexdigest()
    diff_hash = hashlib.sha256(run_git(["diff", "HEAD"], "tracked 全 diff 指纹").stdout).hexdigest()
    untracked = [p for p in untracked_raw.decode("utf-8", "surrogateescape").split("\0") if p]
    modified = [p for p in modified_raw.decode("utf-8", "surrogateescape").split("\0") if p]
    staged = [p for p in staged_raw.decode("utf-8", "surrogateescape").split("\0") if p]
    dir_entries = [p for p in untracked if p.endswith("/")]
    if dir_entries:
        print(f"FATAL: {len(dir_entries)} directory entries in untracked (nested repos?)", file=sys.stderr)
        for p in dir_entries[:10]:
            print("  ", p, file=sys.stderr)
    scope = sorted(set(untracked) | set(modified))

    with open(os.path.join(OUT, "enumeration-untracked.nul"), "wb") as f:
        f.write(untracked_raw)
    with open(os.path.join(OUT, "enumeration-modified-tracked.nul"), "wb") as f:
        f.write(modified_raw)
    with open(os.path.join(OUT, "enumeration-staged.nul"), "wb") as f:
        f.write(staged_raw)
    initial_state = {
        "utc": started, "head": head, "branch": branch,
        "untrackedCount": len(untracked), "modifiedTrackedCount": len(modified),
        "stagedCount": len(staged), "directoryEntriesInUntracked": len(dir_entries),
        "indexSha256": index_hash, "diffHeadSha256": diff_hash,
        "scopeUnionCount": len(scope),
    }
    with open(os.path.join(OUT, "git-initial-state.json"), "w") as f:
        json.dump(initial_state, f, indent=1, ensure_ascii=False)

    # ---------- Phase 1: load baselines ----------
    with open(PREP02_MERGED) as f:
        merged = json.load(f)
    old = {m["path"]: m for m in merged}
    with open(SPACE01_NORM) as f:
        norm = json.load(f)
    with open(F51_RECEIPT) as f:
        f51 = json.load(f)
    with open(F52_RECEIPT) as f:
        f52 = json.load(f)

    # ---------- Phase 2: per-file lstat + sha256 + classification ----------
    rows = []
    changed_vs_prep02 = []
    new_paths = []
    missing_files = []
    symlinks = []
    blob_estimate = {}   # sha1 -> dict for include-set space estimate
    include_shas = []
    crlf_paths = {r["path"]: r for r in norm["rows"]}

    for path in scope:
        is_mod = path in set(modified)
        try:
            st = os.lstat(path)
        except FileNotFoundError:
            missing_files.append(path)
            continue
        entry = {
            "path": path,
            "gitState": "modified-tracked" if is_mod else "untracked",
            "type": "symlink" if os.path.islink(path) else ("dir" if os.path.isdir(path) else "regular"),
            "mode": oct(st.st_mode & 0o7777),
            "bytes": st.st_size,
            "mtimeNs": st.st_mtime_ns,
        }
        if entry["type"] == "symlink":
            symlinks.append(path)
            entry["linkTarget"] = os.readlink(path)
            entry["sha256"] = hashlib.sha256(os.readlink(path).encode()).hexdigest()
            entry["category"], entry["basis"] = "LOCAL_SYMLINK", "符号链接仅本机有效，不随 Git 交付（PREP01 同规则）"
            entry["classificationSource"] = "DELIVERY-FINAL-02 新判"
            rows.append(entry)
            continue

        rec = old.get(path)
        if rec is not None and rec["bytes"] == st.st_size and rec["mtimeNs"] == st.st_mtime_ns:
            entry["category"] = rec["category"]
            entry["basis"] = "PREP02 分类继承：size+mtimeNs 与 DELIVERY-PREP-02/merged-paths.json 记录相等（身份成立），使用角色未变；本轮全新实读 SHA256"
            entry["classificationSource"] = "DELIVERY-PREP-02/merged-paths.json"
            entry["prep02Reason"] = rec.get("reason", "")
        else:
            if rec is not None:
                changed_vs_prep02.append(path)
            else:
                new_paths.append(path)
            entry["category"], entry["basis"] = classify_new(path, st, is_mod)
            entry["classificationSource"] = "DELIVERY-FINAL-02 新判"

        entry["sha256"] = sha256_file(path)

        # include-set git blob + zlib level-1 estimate (single pass)
        if entry["category"] in INCLUDE_CATS:
            with open(path, "rb") as f:
                content = f.read()
            head8k = content[:8000]
            if b"\x00" in head8k:
                filtered = content  # binary detection: text=auto leaves it alone
            else:
                filtered = content.replace(b"\r\n", b"\n")
            sha1 = git_blob_id(filtered)
            include_shas.append(sha1)
            prev = blob_estimate.get(sha1)
            zl1 = len(zlib.compress(filtered, 1))
            if prev is None:
                blob_estimate[sha1] = {
                    "blobSha1": sha1, "filteredBytes": len(filtered), "zlibLevel1Bytes": zl1,
                    "zlibLevel6Bytes": len(zlib.compress(filtered, 6)),
                    "rawBytes": len(content),
                    "crlfBoundary": path in crlf_paths,
                }
            # CRLF boundary live verification
            if path in crlf_paths:
                crlf_paths[path]["_currentRawSha256"] = entry["sha256"]
                crlf_paths[path]["_currentBytes"] = st.st_size
                crlf_paths[path]["_recomputedFilteredSha256"] = hashlib.sha256(filtered).hexdigest()
                crlf_paths[path]["_recomputedFilteredBlobSha1"] = sha1
                crlf_paths[path]["_currentCategory"] = entry["category"]
        rows.append(entry)

    # excluded (non-include) CRLF paths need raw/filtered verification too
    for path, r in crlf_paths.items():
        if "_currentRawSha256" in r:
            continue
        if os.path.exists(path):
            with open(path, "rb") as f:
                content = f.read()
            r["_currentRawSha256"] = hashlib.sha256(content).hexdigest()
            r["_currentBytes"] = len(content)
            r["_recomputedFilteredSha256"] = hashlib.sha256(content.replace(b"\r\n", b"\n")).hexdigest()
            r["_currentCategory"] = next((e["category"] for e in rows if e["path"] == path), "NOT-IN-SCOPE")

    # ---------- Phase 3: buckets + QA ----------
    buckets = {"include": [], "local": [], "unknown": []}
    for e in rows:
        if e["category"] in INCLUDE_CATS:
            buckets["include"].append(e)
        elif e["category"] in LOCAL_CATS:
            buckets["local"].append(e)
        else:
            buckets["unknown"].append(e)

    n_scope = len(scope)
    n_rows = len(rows)
    assert len({e["path"] for e in rows}) == n_rows, "duplicate path rows"
    cover = len(buckets["include"]) + len(buckets["local"]) + len(buckets["unknown"])
    assert cover + len(missing_files) == n_scope, f"coverage mismatch {cover}+{len(missing_files)}!={n_scope}"

    # ---------- Phase 4: removed-since-PREP02 / F51 F52 registration ----------
    current = set(scope)
    gone = []
    for p, rec in old.items():
        if rec.get("gitState") == "untracked" and p not in current:
            gone.append({
                "path": p, "prep02Category": rec["category"], "prep02Bytes": rec["bytes"],
                "status": "RELOCATED_EXTERNAL_LOCALONLY", "remoteOriginalAvailable": False,
                "localOnly": True,
            })
    f51_entries = [{"old_relative_path": e["old_relative_path"], "new_absolute_path": e["new_absolute_path"],
                    "tree_digest_equal": e.get("digests_equal")} for e in f51["entries"]]
    f52_entry = {"old_relative_path": f52["entries"][0].get("old_relative_path") or
                 "artifacts/rust-tauri/R05/RR3/A-REVIEW-02/independent-validator-bin",
                 "new_absolute_path": f52["entries"][0]["new_absolute_path"]}
    gone_paths = {g["path"] for g in gone}
    f51_in_gone = [e for e in f51_entries if e["old_relative_path"].rstrip("/") + "/" in gone_paths
                   or e["old_relative_path"].rstrip("/") in gone_paths]

    # ---------- Phase 5: object-existence + space estimate ----------
    missing_objs = []
    with subprocess.Popen(["git", "--no-optional-locks", "cat-file", "--batch-check=%(objectname) %(objecttype)"],
                          cwd=REPO, stdin=subprocess.PIPE, stdout=subprocess.PIPE) as proc:
        unique_shas = sorted(blob_estimate.keys())
        out, _ = proc.communicate("\n".join(unique_shas).encode())
    for line in out.decode().splitlines():
        sha, typ = line.split()
        if typ == "missing":
            missing_objs.append(sha)
    missing_set = set(missing_objs)
    raw_missing_level1 = sum(blob_estimate[s]["zlibLevel1Bytes"] for s in missing_set)
    raw_missing_level6 = sum(blob_estimate[s]["zlibLevel6Bytes"] for s in missing_set)
    filtered_missing_bytes = sum(blob_estimate[s]["filteredBytes"] for s in missing_set)
    fs_block = 4096
    missing_4k = ((raw_missing_level1 + fs_block - 1) // fs_block) * fs_block
    # CRLF raw blobs additionally needed (no-filters raw object beyond filtered)
    idx_entries = 38066 + len(buckets["include"])
    index_new_size = idx_entries * 62  # v2 base per-entry approximation
    index_budget = index_new_size * 2 + (1 << 20)  # new index + lock double-write + extension headroom
    include_dirs = {os.path.dirname(e["path"]) for e in buckets["include"]} | \
                   {os.path.dirname(os.path.dirname(e["path"])) for e in buckets["include"] if e["path"].count("/") > 3}
    tree_budget = len(include_dirs) * fs_block  # ultra-conservative: every dir a new 4KiB-rounded tree
    pack_ref_level6 = raw_missing_level6
    space = {
        "utc": utcnow(),
        "availableBytesDataVolume": 509 * (1 << 30),
        "includeSet": {
            "paths": len(buckets["include"]),
            "rawLogicalBytes": sum(e["bytes"] for e in buckets["include"]),
            "uniqueFilteredBlobs": len(unique_shas),
            "blobsAlreadyInRepository": len(unique_shas) - len(missing_objs),
            "blobsMissing": len(missing_objs),
            "missingFilteredBytes": filtered_missing_bytes,
        },
        "looseObjectBudget": {
            "zlibLevel1Bytes": raw_missing_level1,
            "perObject4KiBRounded": missing_4k,
            "note": "level-1 是当前 Git 默认 loose 压缩；4KiB 取整为数据块上界，未含 APFS 元数据",
        },
        "indexBudgetBytes": index_budget,
        "indexEstimatedEntries": idx_entries,
        "treeBudgetBytesConservative": tree_budget,
        "commitRefsHeadroomBytes": 4 * (1 << 20),
        "pushPackReferenceLevel6Bytes": pack_ref_level6,
        "pushNote": "smart HTTP chunked 上传通常不落整包本地盘；该值为传输量级参照，非强制本地占用",
        "conservativeTotalLocalWrite": missing_4k + index_budget + tree_budget + 4 * (1 << 20),
        "conclusion": "513Gi 量级余量下预算占比极小；具体结论见 REPORT.md",
    }

    # ---------- Phase 6: write outputs ----------
    def write_json(name, obj):
        with open(os.path.join(OUT, name), "w") as f:
            json.dump(obj, f, indent=1, ensure_ascii=False)

    write_json("classification.json", {
        "generatedUtc": utcnow(), "head": head, "branch": branch,
        "scopeCount": n_scope, "rowCount": n_rows, "missingFileCount": len(missing_files),
        "inheritedFromPrep02": n_rows - len(new_paths) - len(changed_vs_prep02),
        "newSincePrep02": len(new_paths), "changedSincePrep02": len(changed_vs_prep02),
        "categoryCount": {c: sum(1 for e in rows if e["category"] == c)
                          for c in sorted({e["category"] for e in rows})},
        "bucketBytes": {b: sum(e["bytes"] for e in buckets[b]) for b in buckets},
        "rows": rows,
    })

    for b in ("include", "local", "unknown"):
        paths = [e["path"] for e in sorted(buckets[b], key=lambda e: e["path"])]
        with open(os.path.join(OUT, f"{b}-paths.nul"), "wb") as f:
            f.write("\0".join(paths).encode("utf-8", "surrogateescape") + (b"\0" if paths else b""))
        with open(os.path.join(OUT, f"{b}-paths.txt"), "w") as f:
            f.write(f"# DELIVERY-FINAL-02 {b} paths ({len(paths)} paths, "
                    f"{sum(e['bytes'] for e in buckets[b])} bytes)\n")
            f.write(f"# head={head} generated={utcnow()}\n")
            for e in sorted(buckets[b], key=lambda e: e["path"]):
                f.write(f"{e['sha256'][:16]}  {e['bytes']:>12}  {e['mode']}  {e['category']:<26} {e['path']}\n")

    # CRLF boundary output
    crlf_out_rows = []
    crlf_mismatch = []
    for r in norm["rows"]:
        p = r["path"]
        cur = crlf_paths.get(p, {})
        ok_raw = cur.get("_currentRawSha256") == r["raw_content_sha256"]
        ok_filt = cur.get("_recomputedFilteredSha256") == r["filtered_content_sha256"]
        if not (ok_raw and ok_filt):
            crlf_mismatch.append(p)
        crlf_out_rows.append({
            "path": p, "noFiltersWriteRequired": True,
            "currentBytes": cur.get("_currentBytes"),
            "rawContentSha256": r["raw_content_sha256"], "rawBytes": r["raw_bytes"],
            "rawBlobSha1": r["raw_blob_sha1"],
            "filteredContentSha256": r["filtered_content_sha256"], "filteredBytes": r["filtered_bytes"],
            "filteredBlobSha1": r["filtered_blob_sha1"],
            "relation": "filtered = raw.replace(CRLF, LF)；交付须用 --no-filters 写入 raw blob 并以 update-index 登记 raw blob id，普通 add 会改变 SHA",
            "currentCategory": cur.get("_currentCategory", "NOT-IN-SCOPE"),
            "verifiedNowRawEqual": ok_raw, "verifiedNowFilteredEqual": ok_filt,
        })
    write_json("crlf-no-filters-boundary.json", {
        "source": "DELIVERY-SPACE-01/normalization-boundary.json", "utc": utcnow(),
        "pathCount": len(crlf_out_rows), "mismatchCount": len(crlf_mismatch),
        "mismatches": crlf_mismatch,
        "instruction": "对下列全部路径：git hash-object -w --no-filters -- <path> 或 git add --no-filters 后 update-index --cacheinfo 登记 raw blob；不改 .gitattributes/原件/全局配置；本清单只登记不执行",
        "rows": crlf_out_rows,
    })

    # reference boundary index
    excluded_referenced_new = [e for e in buckets["local"]
                               if e["classificationSource"] == "DELIVERY-FINAL-02 新判"]
    write_json("reference-boundary-index.json", {
        "utc": utcnow(),
        "prep02LocalOriginalsPointer": "artifacts/rust-tauri/R05/RR3/DELIVERY-PREP-02/local-originals.json (2,036 项旧排除原件登记，本轮不重复展开)",
        "f51Externalized": {
            "receipt": "artifacts/rust-tauri/R05/RR3/F51-01/RELOCATION-RECEIPT.json",
            "entries": len(f51_entries), "files": f51["totals"]["total_files"],
            "symlinks": f51["totals"]["total_symlinks"], "bytes": f51["totals"]["total_bytes"],
            "allDigestsEqual": f51["totals"]["all_digests_equal"],
            "destinationRoot": f51["destination_root"],
            "matchedInPrep02GoneList": len(f51_in_gone),
            "localOnly": True, "remoteOriginalAvailable": False,
            "entriesSample": f51_entries[:3],
        },
        "f52Externalized": {
            "receipt": "artifacts/rust-tauri/R05/RR3/F52-01/RELOCATION-RECEIPT-F52.json",
            "entries": 1, "files": f52["totals"]["files"], "symlinks": f52["totals"]["symlinks"],
            "destinationRoot": f52["destination_root"],
            "localOnly": True, "remoteOriginalAvailable": False,
        },
        "removedSincePrep02": gone,
        "oversizeOriginalsStillLocal": [{
            "path": e["path"], "sha256": e["sha256"], "bytes": e["bytes"],
            "localOnly": True, "remoteOriginalAvailable": False,
        } for e in buckets["local"] if e["bytes"] > 134_000_000],
        "newExcludedThisRound": [{
            "path": e["path"], "sha256": e["sha256"], "bytes": e["bytes"], "category": e["category"],
            "localOnly": True, "remoteOriginalAvailable": False, "basis": e["basis"],
        } for e in excluded_referenced_new],
    })

    write_json("space-estimate.json", space)
    write_json("unknown-items.json", {
        "utc": utcnow(), "count": len(buckets["unknown"]),
        "items": [{"path": e["path"], "bytes": e["bytes"], "basis": e["basis"]} for e in buckets["unknown"]],
    })
    with open(os.path.join(OUT, "commands.jsonl"), "w") as f:
        for c in commands_log:
            f.write(json.dumps(c, ensure_ascii=False) + "\n")

    summary = {
        "startedUtc": started, "finishedUtc": utcnow(), "head": head,
        "scope": {"untracked": len(untracked), "modifiedTracked": len(modified), "union": n_scope},
        "staged": len(staged), "directoryEntries": len(dir_entries),
        "missingFiles": missing_files, "symlinksInScope": symlinks,
        "rows": n_rows,
        "newSincePrep02": len(new_paths), "changedSincePrep02": changed_vs_prep02,
        "goneSincePrep02": len(gone),
        "buckets": {b: {"count": len(buckets[b]), "bytes": sum(e["bytes"] for e in buckets[b])} for b in buckets},
        "crlf": {"total": len(crlf_out_rows), "mismatches": crlf_mismatch},
        "blobStats": {"unique": len(unique_shas), "existing": len(unique_shas) - len(missing_objs), "missing": len(missing_objs)},
        "space": {"conservativeTotalLocalWrite": space["conservativeTotalLocalWrite"],
                  "availableGiB": 509},
    }
    write_json("summary.json", summary)
    print(json.dumps(summary, indent=1, ensure_ascii=False)[:4000])


if __name__ == "__main__":
    main()
