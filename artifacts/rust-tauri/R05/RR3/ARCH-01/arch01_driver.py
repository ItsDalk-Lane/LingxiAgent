#!/usr/bin/env python3
"""RR3 ARCH-01 driver: localOnly 原件删除/迁出收尾（全新空历史实施）。

阶段（每阶段落盘检查点，可安全重跑，幂等跳过已完成项）：
  precheck  — 5,912 逐项：在 local-paths.nul / 类别匹配 / 非 tracked / 存在 / bytes / SHA256
  relocate  — 61 LOCAL_BINARY mv 至仓库外 evidence 根，逐文件目的地 SHA 复核 + 父目录 RELOCATED-ARCH01 标记
  delete    — 5,851 逐项删除前即时 SHA256 复核（不等即跳过记录），os.remove 逐文件
  rmdirs    — 仅删净后真空目录（os.rmdir，非空自动失败）
Git 全程只读：仅 status/ls-files/diff 读命令，且全部经 --no-optional-locks。
"""

import hashlib
import json
import os
import subprocess
import sys
import time
from collections import defaultdict
from datetime import datetime, timezone

REPO = "/Users/study_superior/Desktop/Code/LingxiAgent"
HOME = "/Users/study_superior"
ARCH = os.path.join(REPO, "artifacts/rust-tauri/R05/RR3/ARCH-01")
DF2 = os.path.join(REPO, "artifacts/rust-tauri/R05/RR3/DELIVERY-FINAL-02")
EVIDENCE_ROOT = "/Users/study_superior/Desktop/Code/LingxiAgent-RR3-localonly-evidence"

DELETE_CATS = {"LOCAL_BUILD_CACHE", "LOCAL_ISOLATED_SOURCE", "LOCAL_ONLY", "LOCAL_RUNTIME_STATE"}
RELOCATE_CATS = {"LOCAL_BINARY"}
ALL_LOCAL_CATS = DELETE_CATS | RELOCATE_CATS

CHECKPOINT = os.path.join(ARCH, "checkpoint.json")
COMMANDS = os.path.join(ARCH, "commands.jsonl")

assert os.path.expanduser("~") == HOME, f"HOME 必须为 {HOME}"
assert os.getcwd() == REPO or os.path.abspath(__file__).startswith(ARCH)


def utcnow():
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def log_cmd(argv, exit_code, note=""):
    rec = {"utc": utcnow(), "argv": argv, "exit": exit_code}
    if note:
        rec["note"] = note
    with open(COMMANDS, "a") as f:
        f.write(json.dumps(rec, ensure_ascii=False) + "\n")


def git(args, check=True):
    argv = ["git", "--no-optional-locks"] + args
    p = subprocess.run(argv, cwd=REPO, capture_output=True)  # bytes 输出（-z 安全）
    log_cmd(argv, p.returncode)
    if check and p.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} exit={p.returncode}: {p.stderr[:500]!r}")
    return p


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024 * 4), b""):
            h.update(chunk)
    return h.hexdigest()


def load_checkpoint():
    if os.path.exists(CHECKPOINT):
        with open(CHECKPOINT) as f:
            return json.load(f)
    return {"phases": {}}


def save_checkpoint(cp):
    tmp = CHECKPOINT + ".tmp"
    with open(tmp, "w") as f:
        json.dump(cp, f, ensure_ascii=False, indent=1)
    os.replace(tmp, CHECKPOINT)


def main():
    phase = sys.argv[1]
    cp = load_checkpoint()

    # ---------- 输入 ----------
    with open(os.path.join(DF2, "classification.json")) as f:
        cls = json.load(f)
    rows = [r for r in cls["rows"] if r["category"] in ALL_LOCAL_CATS]
    by_path = {r["path"]: r for r in rows}
    nul = [p.decode() for p in open(os.path.join(DF2, "local-paths.nul"), "rb").read().split(b"\0") if p]
    assert len(nul) == len(by_path) == 5912, (len(nul), len(by_path))
    assert set(nul) == set(by_path), "local-paths.nul 与 classification localOnly 集合不等"
    inc = set(p.decode() for p in open(os.path.join(DF2, "include-paths.nul"), "rb").read().split(b"\0") if p)
    assert not (inc & set(by_path)), "localOnly 与 include 重叠"

    if phase == "precheck":
        # tracked 检查：git ls-files 全量快照 + 抽样 --error-unmatch 逐路径复核
        p = git(["ls-files", "-z"])
        tracked = set(x.decode() for x in p.stdout.split(b"\0") if x)
        overlap = sorted(tracked & set(by_path))
        print(f"tracked_total={len(tracked)} overlap_with_local={len(overlap)}")
        if overlap:
            json.dump({"overlap": overlap}, open(os.path.join(ARCH, "tracked-overlap.json"), "w"), indent=1)
            raise SystemExit("FATAL: localOnly 与 tracked 重叠")

        results = {}
        cnt = defaultdict(int)
        bytes_sum = defaultdict(int)
        t0 = time.time()
        for i, (path, r) in enumerate(sorted(by_path.items())):
            full = os.path.join(REPO, path)
            st = {"path": path, "category": r["category"], "cls_sha256": r["sha256"], "cls_bytes": r["bytes"]}
            if not os.path.lexists(full):
                st["status"] = "missing"
            elif not os.path.isfile(full) or os.path.islink(full):
                st["status"] = "not_regular_file"
            else:
                sz = os.path.getsize(full)
                st["actual_bytes"] = sz
                if sz != r["bytes"]:
                    st["status"] = "bytes_mismatch"
                else:
                    st["actual_sha256"] = sha256_file(full)
                    if st["actual_sha256"] != r["sha256"]:
                        st["status"] = "sha_mismatch"
                    else:
                        st["status"] = "ok"
            results[path] = st
            cnt[st["status"]] += 1
            if st["status"] == "ok":
                bytes_sum[r["category"]] += sz
            if (i + 1) % 500 == 0:
                print(f"  precheck {i+1}/5912 elapsed={time.time()-t0:.0f}s", flush=True)

        # LOCAL_BINARY 文本矛盾检查（魔数口径：8KiB 内无 NUL 且整体可 UTF-8 解码 → 疑似文本，跳过记录）
        for path, st in results.items():
            if st["status"] != "ok" or st["category"] != "LOCAL_BINARY":
                continue
            with open(os.path.join(REPO, path), "rb") as f:
                head = f.read(8192)
            st["head_magic_hex"] = head[:8].hex()
            if b"\0" not in head:
                try:
                    head.decode("utf-8")
                    st["status"] = "local_binary_looks_like_text"
                    cnt["local_binary_looks_like_text"] += 1
                    cnt["ok"] -= 1
                except UnicodeDecodeError:
                    pass

        # 抽样（+全部 LOCAL_BINARY）git ls-files --error-unmatch 逐路径验证（任务书口径）
        sample = [p for p, s in results.items() if s["category"] == "LOCAL_BINARY"]
        import random
        others = [p for p, s in results.items() if s["category"] != "LOCAL_BINARY"]
        sample += random.Random(20261008).sample(others, 120)
        um_fail = 0
        for pth in sample:
            q = git(["ls-files", "--error-unmatch", pth], check=False)
            if q.returncode == 0:
                um_fail += 1
        print(f"error-unmatch sample={len(sample)} unexpectedly_tracked={um_fail}")
        assert um_fail == 0

        summary = {
            "utc_start": cp.get("precheck_start", utcnow()),
            "utc_end": utcnow(),
            "counts": dict(cnt),
            "bytes_ok_by_category": dict(bytes_sum),
            "error_unmatch_sample": len(sample),
            "error_unmatch_tracked_hits": um_fail,
            "tracked_total": len(tracked),
        }
        json.dump({"summary": summary, "results": results},
                  open(os.path.join(ARCH, "precheck.json"), "w"), indent=1)
        print(json.dumps(summary, indent=1))
        cp["phases"]["precheck"] = summary
        save_checkpoint(cp)

    elif phase == "relocate":
        pre = json.load(open(os.path.join(ARCH, "precheck.json")))["results"]
        todo = [p for p, r in by_path.items() if r["category"] in RELOCATE_CATS]
        done = set(cp["phases"].get("relocate", {}).get("done", []))
        receipt_path = os.path.join(ARCH, "relocation-receipt.json")
        receipt = json.load(open(receipt_path)) if os.path.exists(receipt_path) else {
            "task": "RR3 ARCH-01: 61 LOCAL_BINARY relocated out of main tree (user-approved delete+relocate closeout)",
            "destination_root": EVIDENCE_ROOT, "entries": []}
        entries_by_old = {e["old_relative_path"]: e for e in receipt["entries"]}
        fails, skipped = [], []
        for i, path in enumerate(sorted(todo)):
            if path in done:
                continue
            st = pre[path]
            if st["status"] != "ok":
                skipped.append({"path": path, "reason": st["status"]})
                continue
            src = os.path.join(REPO, path)
            dst = os.path.join(EVIDENCE_ROOT, path)
            assert not os.path.lexists(dst), f"目的地已存在: {dst}"
            if not os.path.lexists(src):
                skipped.append({"path": path, "reason": "missing_at_move_time"})
                continue
            sz = os.path.getsize(src)
            if sz != by_path[path]["bytes"]:
                skipped.append({"path": path, "reason": f"size_changed_before_move {sz}"})
                continue
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            t = utcnow()
            os.rename(src, dst)  # 同卷 rename，字节保持
            dsha = sha256_file(dst)
            if dsha != by_path[path]["sha256"]:
                os.rename(dst, src)  # 回滚
                fails.append({"path": path, "dest_sha256": dsha})
                continue
            dsize = os.path.getsize(dst)
            receipt["entries"].append({
                "old_relative_path": path,
                "new_absolute_path": dst,
                "sha256_before": st["actual_sha256"],
                "sha256_after": dsha,
                "sha256_classified": by_path[path]["sha256"],
                "bytes": dsize,
                "moved_utc": t,
            })
            entries_by_old[path] = receipt["entries"][-1]
            done.add(path)
            if (i + 1) % 10 == 0:
                print(f"  relocate {i+1}/61", flush=True)
        receipt["utc"] = utcnow()
        receipt["totals"] = {
            "entries": len(receipt["entries"]),
            "bytes": sum(e["bytes"] for e in receipt["entries"]),
            "all_sha_equal": all(e["sha256_before"] == e["sha256_after"] == e["sha256_classified"] for e in receipt["entries"]),
        }
        tmp = receipt_path + ".tmp"
        json.dump(receipt, open(tmp, "w"), indent=1)
        os.replace(tmp, receipt_path)
        cp["phases"]["relocate"] = {"done": sorted(done), "utc": receipt["utc"]}
        save_checkpoint(cp)
        json.dump({"fails": fails, "skipped": skipped},
                  open(os.path.join(ARCH, "relocate-exceptions.json"), "w"), indent=1)
        print(f"relocated={len(receipt['entries'])} fails={len(fails)} skipped={len(skipped)}")
        print(json.dumps(receipt["totals"], indent=1))

    elif phase == "delete":
        pre = json.load(open(os.path.join(ARCH, "precheck.json")))["results"]
        todo = [p for p, r in by_path.items() if r["category"] in DELETE_CATS]
        done = set(cp["phases"].get("delete", {}).get("done", []))
        receipt_path = os.path.join(ARCH, "cleanup-receipt.json")
        receipt = json.load(open(receipt_path)) if os.path.exists(receipt_path) else {
            "task": "RR3 ARCH-01: localOnly originals deleted from main tree (user-approved delete+relocate closeout)",
            "categories": sorted(DELETE_CATS), "entries": []}
        skipped, sha_skips = [], []
        t0 = time.time()
        for i, path in enumerate(sorted(todo)):
            if path in done:
                continue
            full = os.path.join(REPO, path)
            if not os.path.isfile(full) or os.path.islink(full):
                skipped.append({"path": path, "reason": "missing_or_not_regular"})
                continue
            sz = os.path.getsize(full)
            sha = sha256_file(full)  # 删除前即时复核（实读）
            if sha != by_path[path]["sha256"]:
                sha_skips.append({"path": path, "actual_sha256": sha, "cls_sha256": by_path[path]["sha256"]})
                continue
            os.remove(full)
            receipt["entries"].append({
                "path": path, "category": by_path[path]["category"],
                "bytes": sz, "sha256_before_delete": sha, "utc": utcnow()})
            done.add(path)
            if (i + 1) % 500 == 0:
                print(f"  delete {i+1}/{len(todo)} elapsed={time.time()-t0:.0f}s", flush=True)
                tmp = receipt_path + ".tmp"
                json.dump(receipt, open(tmp, "w"), indent=1)
                os.replace(tmp, receipt_path)
                cp["phases"]["delete"] = {"done": sorted(done)}
                save_checkpoint(cp)
        receipt["utc"] = utcnow()
        bycat = defaultdict(int)
        for e in receipt["entries"]:
            bycat[e["category"]] += 1
        receipt["totals"] = {
            "entries": len(receipt["entries"]),
            "bytes": sum(e["bytes"] for e in receipt["entries"]),
            "by_category": dict(bycat),
            "all_sha_equal_classified": all(
                e["sha256_before_delete"] == by_path[e["path"]]["sha256"] for e in receipt["entries"]),
        }
        tmp = receipt_path + ".tmp"
        json.dump(receipt, open(tmp, "w"), indent=1)
        os.replace(tmp, receipt_path)
        cp["phases"]["delete"] = {"done": sorted(done), "utc": receipt["utc"]}
        save_checkpoint(cp)
        json.dump({"skipped": skipped, "sha_skips": sha_skips},
                  open(os.path.join(ARCH, "delete-exceptions.json"), "w"), indent=1)
        print(f"deleted={len(receipt['entries'])} skipped={len(skipped)} sha_skips={len(sha_skips)}")
        print(json.dumps(receipt["totals"], indent=1))

    elif phase == "markers":
        # 每个受影响的原父目录写 RELOCATED-ARCH01.json（沿 F51 格式）
        receipt = json.load(open(os.path.join(ARCH, "relocation-receipt.json")))
        groups = defaultdict(list)
        for e in receipt["entries"]:
            parent = os.path.dirname(e["old_relative_path"])
            groups[parent].append(e)
        markers = []
        for parent, ents in sorted(groups.items()):
            mpath = os.path.join(REPO, parent, "RELOCATED-ARCH01.json")
            doc = {
                "marker": "RELOCATED-ARCH01",
                "receipt": "artifacts/rust-tauri/R05/RR3/ARCH-01/relocation-receipt.json",
                "relocated_children": [
                    {"name": os.path.basename(e["old_relative_path"]),
                     "new_absolute_path": e["new_absolute_path"],
                     "old_relative_path": e["old_relative_path"]}
                    for e in ents],
                "task": "RR3 ARCH-01: localOnly LOCAL_BINARY originals relocated out of the main tree",
                "written_utc": utcnow(),
            }
            tmp = mpath + ".tmp"
            with open(tmp, "w") as f:
                json.dump(doc, f, indent=1, ensure_ascii=False)
                f.write("\n")
            os.replace(tmp, mpath)
            markers.append(parent + "/RELOCATED-ARCH01.json")
        cp["phases"]["markers"] = {"written": markers, "utc": utcnow()}
        save_checkpoint(cp)
        print(f"markers_written={len(markers)}")
        for m in markers:
            print(" ", m)

    elif phase == "rmdirs":
        # 仅删除「删净后真空」的目录：自深至浅 os.rmdir（非空会失败），绝不触及含 tracked/标记/其余文件的目录
        pre = json.load(open(os.path.join(ARCH, "precheck.json")))["results"]
        rel = json.load(open(os.path.join(ARCH, "relocation-receipt.json")))
        deleted = {e["path"] for e in json.load(open(os.path.join(ARCH, "cleanup-receipt.json")))["entries"]}
        relocated = {e["old_relative_path"] for e in rel["entries"]}
        handled = deleted | relocated
        dirs = set()
        for p in handled:
            d = os.path.dirname(p)
            while d and d != "." and d.startswith("artifacts/"):
                dirs.add(d)
                nd = os.path.dirname(d)
                if nd == d:
                    break
                d = nd
        removed, kept = [], []
        for d in sorted(dirs, key=lambda x: -x.count("/")):
            full = os.path.join(REPO, d)
            if not os.path.isdir(full):
                continue
            try:
                entries = os.listdir(full)
            except OSError:
                kept.append(d)
                continue
            if entries:
                kept.append(d)  # 非空：含 tracked/标记/include 文件等，不动
                continue
            try:
                os.rmdir(full)
                removed.append(d)
            except OSError as exc:
                kept.append(f"{d} ({exc})")
        json.dump({"removed": removed, "kept_non_empty": kept},
                  open(os.path.join(ARCH, "rmdir-result.json"), "w"), indent=1)
        cp["phases"]["rmdirs"] = {"removed": len(removed), "utc": utcnow()}
        save_checkpoint(cp)
        print(f"rmdir_removed={len(removed)} kept_non_empty={len(kept)}")
        for d in removed[:80]:
            print(" -", d)
        for d in kept[:40]:
            print(" K", d)

    elif phase == "final":
        out = {}
        out["utc"] = utcnow()
        q = git(["rev-parse", "HEAD"])
        out["head"] = q.stdout.decode().strip()
        q = git(["status", "--porcelain", "-z"])
        items = [x for x in q.stdout.split(b"\0") if x]
        unst = []
        for it in items:
            code, _, rest = it[:2].decode(), None, it[3:].decode()
            unst.append({"code": code, "path": rest})
        out["status_items"] = unst
        out["status_count"] = len(unst)
        out["modified_tracked"] = [u for u in unst if u["code"].strip() and u["code"].strip() not in ("??",)]
        q = git(["diff", "--cached", "--name-only"])
        out["staged"] = [l for l in q.stdout.decode().splitlines() if l]
        q = git(["diff", "--name-only"])
        out["diff_working"] = [l for l in q.stdout.decode().splitlines() if l]
        q = git(["diff", "HEAD"])
        import hashlib as _h
        out["diff_head_sha256"] = _h.sha256(q.stdout).hexdigest()
        q = git(["ls-files", "-s"])
        out["ls_files_s_sha256"] = _h.sha256(q.stdout).hexdigest()
        json.dump(out, open(os.path.join(ARCH, "final-verification.json"), "w"), indent=1)
        print(json.dumps({k: out[k] for k in ("head", "status_count", "diff_head_sha256", "ls_files_s_sha256")}, indent=1))
    else:
        raise SystemExit(f"unknown phase {phase}")


if __name__ == "__main__":
    main()
