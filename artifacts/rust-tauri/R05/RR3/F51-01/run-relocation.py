#!/usr/bin/env python3
"""F51 relocation driver (run from repository root). Run 3: resume with tri-state logic.

Run history:
 - run 1: entries 1-39 renamed out and verified digest-equal; entry 40 renamed out
   but the path-based post-scan died with OSError ENAMETOOLONG (~100-level nested
   tree + slash-encoded destination prefix exceeded macOS PATH_MAX 1024); entries
   41-56 untouched; all 56 pre-digests checkpointed; no bytes deleted.
 - run 2: aborted at gate 1: already-relocated entries were missing from the live
   enumeration and wrongly treated as "FINAL-01 entries no longer present".
 - run 3 (this run): the authoritative target set is the FINAL-01 frozen 56-entry
   list cross-checked against disk in three states (still in main tree / already
   at a destination under its encoded or short name / missing). Only a genuinely
   missing entry, or a new unexpected directory entry, aborts. Then:
   refresh pre-digests with the fd-based PATH_MAX-safe scanner for entries still
   in the main tree (verified against the run-1 checkpoint); relocate remaining
   entries (short '<idx>--<basename>' name whenever the encoded prefix plus
   max_relpath_len exceeds 1000, incl. renaming run-1 destinations that violate
   the bound); post-digest every destination and compare field by field; write
   RELOCATED-F51.json markers into every original parent; verify the live
   enumeration contains zero directory entries.

Safety: no deletion is ever performed; only renames. Any gate failure aborts
before further moves and writes RELOCATION-RECEIPT.json.aborted.
"""
import json
import os
import subprocess
import sys
from datetime import datetime, timezone

REPO = "/Users/study_superior/Desktop/Code/LingxiAgent"
DEST_ROOT = "/Users/study_superior/Desktop/Code/LingxiAgent-RR3-localonly-fixtures"
EVID = os.path.join(REPO, "artifacts/rust-tauri/R05/RR3/F51-01")
FROZEN = os.path.join(REPO, "artifacts/rust-tauri/R05/RR3/FINAL-01/command-records/frozen-inputs.json")
RECEIPT = os.path.join(EVID, "RELOCATION-RECEIPT.json")
CHECKPOINT = os.path.join(EVID, "relocation-checkpoint.json")
SELFOUT = os.path.join(EVID, "selfcheck-output.txt")

sys.path.insert(0, EVID)
from digest_tree import scan_tree, du_kb, nested_git_head  # noqa: E402

PATH_LIMIT = 1000  # safety bound below macOS PATH_MAX 1024

_log_lines = []


def log(msg):
    line = "%s %s" % (datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), msg)
    print(line, flush=True)
    _log_lines.append(line)


def utcnow():
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def git_head():
    return subprocess.run(["git", "-C", REPO, "rev-parse", "HEAD"],
                          capture_output=True, text=True, check=True).stdout.strip()


def enumerate_dir_entries():
    out = subprocess.run(["git", "-C", REPO, "ls-files", "--cached", "--others",
                          "--exclude-standard"], capture_output=True, text=True, check=True)
    return sorted(l.rstrip("/") for l in out.stdout.splitlines() if l.endswith("/"))


def short_name(idx, entry):
    return "%02d--%s" % (idx, os.path.basename(entry))


def summary_with_meta(path):
    s, _ = scan_tree(path)
    out = dict(s)
    out["du_kilobytes"] = du_kb(path)
    out["nested_git_head"] = nested_git_head(path)
    return out


def main():
    os.chdir(REPO)
    started = utcnow()
    receipt = {
        "task": "F51 relocation of nested-.git untracked fixture directories out of the main tree",
        "run": 3,
        "run_history": {
            "run1": {
                "started_utc": "2026-10-07T11:47:31Z",
                "outcome": "aborted at entry 40/56 post-digest: OSError ENAMETOOLONG (path-based scanner; ~100-level nested tree + slash-encoded destination prefix exceeded macOS PATH_MAX 1024)",
                "state_at_abort": "entries 1-39 renamed out and verified digest-equal; entry 40 renamed out (bytes intact) but unverified; entries 41-56 untouched in main tree; all 56 pre-digests saved in relocation-checkpoint.json; no bytes deleted (log: run1-abort-log.txt)",
            },
            "run2": {
                "started_utc": "2026-10-07T11:53:23Z",
                "outcome": "aborted at gate 1 (resume semantics bug): already-relocated entries missing from live enumeration were misclassified as 'FINAL-01 entries no longer present'; zero filesystem changes (receipt: RELOCATION-RECEIPT.json.aborted-run2)",
            },
        },
        "started_utc": started,
        "driver_argv": sys.argv,
        "repo": REPO,
        "destination_root": DEST_ROOT,
        "method": {
            "enumeration": "git ls-files --cached --others --exclude-standard (trailing-slash entries)",
            "relocation": "os.rename (same-filesystem rename(2), byte-preserving; mv equivalent)",
            "digest": "digest_tree.scan_tree (fd-based, PATH_MAX-safe): sorted manifest of (type,size,sha256-content-or-linktarget,relpath); tree_digest=sha256(manifest); manifest format identical to run 1",
            "naming": "slash-encoded relative path; when len(dest_prefix)+max_relpath_len>1000 a short '<idx>--<basename>' name is used (documented deviation, PATH_MAX)",
            "path_safety_bound": PATH_LIMIT,
        },
        "gates": {},
        "entries": [],
        "markers": [],
        "naming_deviations": [],
        "aborted": None,
    }
    try:
        head = git_head()
        receipt["head"] = head
        log("HEAD=%s" % head)

        # Gate 1: authoritative target set = FINAL-01 frozen 56-entry list,
        # cross-checked against disk in three states (resume-safe).
        current = enumerate_dir_entries()
        frozen = json.load(open(FROZEN))
        master = sorted(e.rstrip("/") for e in frozen["binder_observation"]["all_directory_entries"])
        current_set = set(current)
        master_set = set(master)
        unexpected = [e for e in current if e not in master_set]
        missing = []
        state = {}
        for idx, e in enumerate(master, 1):
            enc_dst = os.path.join(DEST_ROOT, e.replace("/", "_"))
            s_dst = os.path.join(DEST_ROOT, short_name(idx, e))
            if os.path.exists(os.path.join(REPO, e)):
                state[e] = "in_main_tree"
            elif os.path.isdir(enc_dst):
                state[e] = "at_destination_encoded"
            elif os.path.isdir(s_dst):
                state[e] = "at_destination_short"
            else:
                missing.append(e)
        receipt["gates"]["enumeration"] = {
            "final01_master_count": len(master),
            "live_directory_entries_at_start": len(current),
            "live_entries_in_main_tree": sum(1 for e in master if state[e] == "in_main_tree"),
            "already_at_destination": sum(1 for e in master if state[e].startswith("at_destination")),
            "unexpected_new_entries": unexpected,
            "missing_entries": missing,
            "live_matches_master": not unexpected,
        }
        if unexpected:
            raise SystemExit("ABORT: new unexpected directory entries appeared: %r" % unexpected)
        if missing:
            raise SystemExit("ABORT: entries from FINAL-01 neither in main tree nor at destination: %r" % missing)
        log("gate1: master=%d (from FINAL-01); live dir entries=%d; in_main=%d already_at_dest=%d; missing=0 unexpected=0"
            % (len(master), len(current), receipt["gates"]["enumeration"]["live_entries_in_main_tree"],
               receipt["gates"]["enumeration"]["already_at_destination"]))
        entries = master

        # Gate 2: sanity (dirs, nested .git, no entry nesting, no encoded collisions)
        problems = []
        for e in entries:
            p = os.path.join(REPO, e)
            if os.path.exists(p) and not os.path.isdir(p):
                problems.append("%s: not a directory" % e)
                continue
            if os.path.exists(p):
                gp = os.path.join(p, ".git")
                if not (os.path.isdir(gp) or os.path.isfile(gp)):
                    problems.append("%s: no nested .git" % e)
        for a in entries:
            for b in entries:
                if a != b and (b + "/").startswith(a + "/"):
                    problems.append("entry nesting: %s ancestor of %s" % (a, b))
        names = {}
        for e in entries:
            enc = e.replace("/", "_")
            if enc in names:
                problems.append("encoded-name collision: %s vs %s" % (names[enc], e))
            names[enc] = e
        receipt["gates"]["entry_sanity"] = {"checked": len(entries), "problems": problems}
        if problems:
            raise SystemExit("ABORT: %r" % problems)
        log("entry sanity: %d entries OK (dirs with nested .git where still present; no nesting; no collisions)" % len(entries))

        os.makedirs(DEST_ROOT, exist_ok=True)
        log("destination root ready: %s" % DEST_ROOT)

        # Pre-digests: load run-1 checkpoint; refresh from source for entries still present
        pre = json.load(open(CHECKPOINT))["pre"]
        log("checkpoint pre-digests loaded: %d" % len(pre))
        for i, e in enumerate(entries, 1):
            src = os.path.join(REPO, e)
            if not os.path.exists(src):
                continue  # already relocated in run 1 (or earlier in this run)
            if "max_relpath_len" in pre.get(e, {}):
                continue
            fresh = summary_with_meta(src)
            if e in pre:
                old = pre[e]
                for k in ("file_count", "symlink_count", "other_count", "total_bytes", "tree_digest"):
                    if old.get(k) != fresh[k]:
                        raise SystemExit("ABORT: checkpoint mismatch for %s on %s: %r vs %r" % (e, k, old.get(k), fresh[k]))
                log("pre-digest refreshed (fd-scanner, equals checkpoint) %2d %s" % (i, e))
            pre[e] = fresh
            json.dump({"pre": pre}, open(CHECKPOINT, "w"), sort_keys=True)
        receipt["pre_digests_complete_utc"] = utcnow()

        # Rename + post-digest per entry
        mismatches = []
        for i, e in enumerate(entries, 1):
            src = os.path.join(REPO, e)
            enc = e.replace("/", "_")
            long_dst = os.path.join(DEST_ROOT, enc)
            short_dst = os.path.join(DEST_ROOT, short_name(i, e))
            if os.path.exists(src):
                # not yet relocated: choose destination name under PATH bound
                prefix_len = len(long_dst.encode()) + 1
                if prefix_len + pre[e]["max_relpath_len"] > PATH_LIMIT:
                    dst = short_dst
                    receipt["naming_deviations"].append({
                        "entry": e, "name": short_name(i, e),
                        "reason": "slash-encoded prefix + max_relpath_len %d exceeds bound %d (macOS PATH_MAX 1024)" % (pre[e]["max_relpath_len"], PATH_LIMIT),
                    })
                    log("naming deviation for %s -> %s (max_relpath_len=%d)" % (e, short_name(i, e), pre[e]["max_relpath_len"]))
                else:
                    dst = long_dst
                if os.path.exists(dst):
                    raise SystemExit("ABORT: destination already exists: %s" % dst)
                os.rename(src, dst)
            elif os.path.exists(long_dst):
                dst = long_dst
                # verify reachability bound with real scan below; rename to short if needed
                tmp = summary_with_meta(dst)
                if len(long_dst.encode()) + 1 + tmp["max_relpath_len"] > PATH_LIMIT:
                    if os.path.exists(short_dst):
                        raise SystemExit("ABORT: short destination already exists: %s" % short_dst)
                    os.rename(long_dst, short_dst)
                    dst = short_dst
                    receipt["naming_deviations"].append({
                        "entry": e, "name": short_name(i, e),
                        "reason": "relocated in run 1 under slash-encoded name; deepest absolute child paths exceeded macOS PATH_MAX 1024, renamed to short form to restore reachability (bytes untouched)",
                        "max_relpath_len": tmp["max_relpath_len"],
                    })
                    log("naming deviation (run-1 entry) %s -> %s (max_relpath_len=%d)" % (e, short_name(i, e), tmp["max_relpath_len"]))
            elif os.path.exists(short_dst):
                dst = short_dst
            else:
                raise SystemExit("ABORT: neither source nor destination found for %s" % e)
            moved_utc = utcnow()
            if os.path.exists(src):
                raise SystemExit("ABORT: source still present after rename: %s" % src)
            post = summary_with_meta(dst)
            equal = all(pre[e].get(k) == post.get(k)
                        for k in ("file_count", "symlink_count", "other_count", "total_bytes", "tree_digest", "nested_git_head"))
            if not equal:
                mismatches.append(e)
            receipt["entries"].append({
                "old_relative_path": e + "/",
                "new_absolute_path": dst,
                "moved_utc": moved_utc,
                "pre": pre[e],
                "post": post,
                "digests_equal": equal,
            })
            log("verified %2d/%d %-70s -> %s  equal=%s" % (i, len(entries), e, os.path.basename(dst), equal))
        receipt["relocation_complete_utc"] = utcnow()
        if mismatches:
            raise SystemExit("ABORT: digest mismatch after relocation: %r" % mismatches)

        # In-place RELOCATED-F51.json markers per original parent
        by_parent = {}
        for e in entries:
            by_parent.setdefault(os.path.dirname(e), []).append(e)
        for parent, moved in sorted(by_parent.items()):
            marker_children = []
            for m in sorted(moved):
                enc_dst = os.path.join(DEST_ROOT, m.replace("/", "_"))
                if os.path.exists(enc_dst):
                    final_dst = enc_dst
                else:
                    final_dst = os.path.join(DEST_ROOT, short_name(entries.index(m) + 1, m))
                marker_children.append({
                    "name": os.path.basename(m) + "/",
                    "old_relative_path": m + "/",
                    "new_absolute_path": final_dst,
                })
            marker = {
                "marker": "RELOCATED-F51",
                "task": "RR3 F51: nested-.git untracked fixture directories relocated out of the main tree",
                "written_utc": utcnow(),
                "receipt": "artifacts/rust-tauri/R05/RR3/F51-01/RELOCATION-RECEIPT.json",
                "relocated_children": marker_children,
            }
            mpath = os.path.join(REPO, parent, "RELOCATED-F51.json")
            with open(mpath, "w") as f:
                json.dump(marker, f, indent=1, sort_keys=True)
                f.write("\n")
            receipt["markers"].append(os.path.relpath(mpath, REPO))
            log("marker written: %s (%d children)" % (os.path.relpath(mpath, REPO), len(moved)))

        # Gate: post-enumeration must contain zero directory entries
        post_entries = enumerate_dir_entries()
        receipt["gates"]["post_enumeration_dir_entries"] = post_entries
        if post_entries:
            raise SystemExit("ABORT: directory entries remain after relocation: %r" % post_entries)
        log("post-enumeration: 0 directory entries")

        # Global byte totals
        receipt["totals"] = {
            "entries": len(entries),
            "total_files": sum(x["post"]["file_count"] for x in receipt["entries"]),
            "total_symlinks": sum(x["post"]["symlink_count"] for x in receipt["entries"]),
            "total_bytes": sum(x["post"]["total_bytes"] for x in receipt["entries"]),
            "all_digests_equal": all(x["digests_equal"] for x in receipt["entries"]),
        }
        log("totals: entries=%d files=%d symlinks=%d bytes=%d all_equal=%s"
            % (receipt["totals"]["entries"], receipt["totals"]["total_files"],
               receipt["totals"]["total_symlinks"], receipt["totals"]["total_bytes"],
               receipt["totals"]["all_digests_equal"]))

        receipt["aborted"] = False
        receipt["finished_utc"] = utcnow()
        json.dump(receipt, open(RECEIPT, "w"), indent=1, sort_keys=True)
        open(SELFOUT, "w").write("\n".join(_log_lines) + "\n")
        log("RECEIPT written: %s" % RECEIPT)
        log("self-check output written: %s" % SELFOUT)
        return 0
    except SystemExit as e:
        receipt["aborted"] = True
        receipt["abort_reason"] = str(e)
        receipt["finished_utc"] = utcnow()
        json.dump(receipt, open(RECEIPT + ".aborted", "w"), indent=1, sort_keys=True)
        open(SELFOUT + ".aborted", "w").write("\n".join(_log_lines) + "\n")
        print("ABORTED: %s" % e, file=sys.stderr)
        return 1
    except OSError as e:
        receipt["aborted"] = True
        receipt["abort_reason"] = "OSError: %s" % e
        receipt["finished_utc"] = utcnow()
        json.dump(receipt, open(RECEIPT + ".aborted", "w"), indent=1, sort_keys=True)
        open(SELFOUT + ".aborted", "w").write("\n".join(_log_lines) + "\n")
        print("ABORTED (OSError): %s" % e, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
