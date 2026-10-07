#!/usr/bin/env python3
"""FINAL-03 recursive layer audit: walk every verify-stage-result.json under the
evidence root; verify per-layer: overall, all commands (exit/status/timeout/
missingEvidence/preExisting), candidateSourceBinding.stable + every checkpoint,
runnerSourceBinding.status, scenario failures, supplementalLeafCoverage, and that
every evidencePath/manifestPath referenced actually exists on disk."""
import json, os, sys, datetime

ROOT = "/Users/study_superior/Desktop/Code/LingxiAgent/artifacts/rust-tauri/R05/RR3/FINAL-03/verify-R05"
REPO = "/Users/study_superior/Desktop/Code/LingxiAgent"
report = {"generated_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
          "evidence_root": ROOT, "root_exists": os.path.isdir(ROOT), "layers": [], "all_failures": []}

def repo_abs(p):
    return p if os.path.isabs(p) else os.path.join(REPO, p)

for dirpath, dirnames, filenames in os.walk(ROOT):
    if "verify-stage-result.json" not in filenames:
        continue
    p = os.path.join(dirpath, "verify-stage-result.json")
    rel = os.path.relpath(p, ROOT)
    try:
        d = json.load(open(p))
    except Exception as e:
        report["layers"].append({"layer": rel, "parse_error": str(e)})
        report["all_failures"].append({"layer": rel, "kind": "parse_error", "detail": str(e)})
        continue
    fails = []
    cmds = []
    for c in d.get("commands", []):
        cmds.append({"key": c.get("key"), "exit": c.get("exitCode"), "status": c.get("status"),
                     "timedOut": c.get("timedOut"), "internalError": c.get("internalError"),
                     "durationMs": c.get("durationMs"),
                     "missingEvidence": c.get("missingEvidence"),
                     "preExistingEvidence": c.get("preExistingEvidence")})
        if c.get("status") != "PASS":
            fails.append({"kind": "command", "key": c.get("key"), "exit": c.get("exitCode"),
                          "status": c.get("status"), "timedOut": c.get("timedOut"),
                          "internalError": c.get("internalError")})
    csb = d.get("candidateSourceBinding", {})
    stable = csb.get("stable")
    checkpoints = csb.get("checkpointAfterEveryCommand", [])
    unstable = [i for i, cp in enumerate(checkpoints) if not cp.get("stable", False)]
    if stable is not True:
        fails.append({"kind": "candidateSourceBinding.stable", "value": stable,
                      "finalChanged": csb.get("finalChangedPathBytesHex"),
                      "beforeError": csb.get("beforeError"), "afterError": csb.get("afterError")})
    if unstable:
        fails.append({"kind": "unstable_checkpoints", "indices": unstable,
                      "detail": [checkpoints[i] for i in unstable][:3]})
    rsb = d.get("runnerSourceBinding", {})
    if rsb.get("status") != "PASS":
        fails.append({"kind": "runnerSourceBinding", "status": rsb.get("status"),
                      "before": rsb.get("before", {}).get("status"), "after": rsb.get("after", {}).get("status"),
                      "errors": [rsb.get("beforeError"), rsb.get("afterError")]})
    scen_fail = [{"id": s.get("id"), "status": s.get("status")} for s in d.get("scenarios", []) if s.get("status") not in ("PASS", None)]
    if scen_fail:
        fails.append({"kind": "scenarios", "count": len(scen_fail), "items": scen_fail})
    slc = d.get("supplementalLeafCoverage", {})
    if slc and (slc.get("fail", 0) or slc.get("blocked", 0)):
        fails.append({"kind": "supplementalLeafCoverage", "fail": slc.get("fail"), "blocked": slc.get("blocked"),
                      "commandsNotPassing": slc.get("commandsNotPassing")})
    if d.get("overall") != "PASS":
        fails.append({"kind": "overall", "value": d.get("overall")})
    # evidence reference existence
    missing_files = []
    for c in d.get("commands", []):
        for ep in (c.get("evidencePaths") or []):
            if not os.path.exists(repo_abs(ep)):
                missing_files.append(ep)
    for k in ("before", "after"):
        mp = csb.get(k, {}).get("manifestPath")
        if mp and not os.path.exists(mp):
            missing_files.append(mp)
    if missing_files:
        fails.append({"kind": "missing_evidence_refs", "paths": missing_files})
    report["layers"].append({
        "layer": rel, "stage": d.get("stage"), "overall": d.get("overall"),
        "testedSha": d.get("testedSha"), "stable": stable,
        "checkpoint_count": len(checkpoints), "unstable_checkpoints": len(unstable),
        "runner": rsb.get("status"),
        "commands": cmds, "scenarios_total": len(d.get("scenarios", [])),
        "scenario_fail_count": len(scen_fail),
        "leaves": {k: slc.get(k) for k in ("declaredInStageMap","expectedFromR00Ledger","pass","fail","blocked","deferredToLaterStage")} if slc else None,
        "startedAtUnixMs": d.get("startedAtUnixMs"), "finishedAtUnixMs": d.get("finishedAtUnixMs"),
        "failures": fails,
    })
    for f in fails:
        report["all_failures"].append({"layer": rel, **f})

with open("/private/tmp/rr3-final03-dir/audit-layers.json", "w") as f:
    json.dump(report, f, indent=1)
print(json.dumps({k: (report[k] if k != "layers" else
      [{"layer": l.get("layer"), "stage": l.get("stage"), "overall": l.get("overall"),
        "stable": l.get("stable"), "runner": l.get("runner"),
        "unstable_cp": l.get("unstable_checkpoints"),
        "cmd_fail": [c["key"] for c in l.get("commands", []) if c["status"] != "PASS"],
        "scen_fail": l.get("scenario_fail_count"),
        "leaf_fail": (l.get("leaves") or {}).get("fail"), "leaf_blocked": (l.get("leaves") or {}).get("blocked")}
       for l in report["layers"]]) for k in ("root_exists", "layers", "all_failures")}, indent=1, ensure_ascii=False))
