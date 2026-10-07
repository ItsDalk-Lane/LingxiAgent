#!/usr/bin/env python3
"""Recursively audit every verify-stage-result.json under the FINAL-02 evidence root.
For each layer: stage, overall, commands (each exit/ok), candidateSourceBinding.stable,
every checkpoint, runnerSourceBinding.status, testedSha, dependency refs, missing files."""
import json, os, sys, hashlib

ROOT = "/Users/study_superior/Desktop/Code/LingxiAgent/artifacts/rust-tauri/R05/RR3/FINAL-02/verify-R05"
report = []

def load(p):
    with open(p) as f:
        return json.load(f)

def audit_file(path):
    try:
        d = load(path)
    except Exception as e:
        return {"file": path, "error": f"unparseable: {e}"}
    rel = os.path.relpath(path, ROOT)
    layer = os.path.relpath(path, ROOT).split(os.sep)
    depth = len([x for x in layer if x == "verify-stage-result.json"]) - 1
    cmds = []
    for c in d.get("commands", []):
        name = c.get("name") or c.get("label") or c.get("id") or "?"
        cmds.append({"name": name,
                     "exit": c.get("exitCode", c.get("exit")),
                     "ok": c.get("ok", c.get("passed"))})
    csb = d.get("candidateSourceBinding", {}) or {}
    cps = csb.get("checkpoints", []) or []
    cp_bad = [c for c in cps if not c.get("stable", False)]
    rsb = d.get("runnerSourceBinding", {}) or {}
    entry = {
        "file": rel,
        "stage": d.get("stage"),
        "overall": d.get("overall"),
        "stable": csb.get("stable"),
        "checkpoints_total": len(cps),
        "checkpoints_unstable": len(cp_bad),
        "unstable_checkpoint_paths": sorted({c.get("path") for c in cp_bad})[:20],
        "runner_status": rsb.get("status"),
        "runner_error": rsb.get("error"),
        "testedSha": d.get("testedSha"),
        "worktreeDirty": d.get("worktreeDirty"),
        "commands": cmds,
        "n_commands_ok": sum(1 for c in cmds if c["ok"] is True or (isinstance(c["exit"], int) and c["exit"] == 0)),
        "n_commands": len(cmds),
        "startedAtUnixMs": d.get("startedAtUnixMs"),
        "finishedAtUnixMs": d.get("finishedAtUnixMs"),
    }
    return entry

def main():
    if not os.path.isdir(ROOT):
        print(json.dumps({"error": f"evidence root not found: {ROOT}"}, indent=1))
        return
    for dirpath, dirnames, filenames in os.walk(ROOT):
        for fn in filenames:
            if fn == "verify-stage-result.json":
                report.append(audit_file(os.path.join(dirpath, fn)))
    report.sort(key=lambda r: r.get("file", ""))
    print(json.dumps({"evidence_root": ROOT, "layers": report}, indent=1))

if __name__ == "__main__":
    main()
