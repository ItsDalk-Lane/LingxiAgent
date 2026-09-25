#!/usr/bin/env python3
"""R01-T07: generate DATA_COMPATIBILITY_MATRIX.json from R00 STORES.json +
R01 OWNERSHIP_TARGET.json + measured epoch-probe conclusions.

Cutover strategy assignment (ADR-004):
  switch-to-new-authority : run/message semantic stores whose authority moves
                            to the Rust kernel's new run/message store.
  preserve-format         : store keeps its current on-disk format; new kernel
                            writes the same format (knowledge/memory/config/
                            attachments etc. per taskbook 02 §8).
  regenerable             : rebuildable cache; dropped and rebuilt, never migrated.
  shell-local             : per-shell state, owned by whichever host runs;
                            never migrated, never authoritative.
  epoch-mechanism         : the epoch stamp/journal/checkpoint stores themselves.
"""
import json
from pathlib import Path

REPO = Path("/Users/study_superior/Desktop/Code/LingxiAgent")
stores = json.loads((REPO / "docs/rust-tauri/R00/STORES.json").read_text())["stores"]
owners = {o["store_id"]: o for o in json.loads(
    (REPO / "docs/rust-tauri/R01/OWNERSHIP_TARGET.json").read_text())["store_ownership"]}

SWITCH = {
    "session-jsonl", "session-sidecars", "session-manifest-sqlite",
    "conversation-map-layout", "workflow-state", "subagent-state",
    "deferred-result-state", "loop-state", "plugin-task-registry",
    "terminal-session-state", "execution-leases",
}
SHELL_LOCAL = {
    "desktop-diagnostics", "desktop-gpu-startup-state",
    "desktop-win32-install-acl-heal-state", "desktop-window-version-state",
    "desktop-update-channel", "signed-artifacts",
}
EPOCH_MECH = {
    "data-epoch-stamp", "data-epoch-transition-journal", "data-epoch-checkpoints",
    "data-epoch-restore-quarantine",
}

MIGRATION = {
    "switch-to-new-authority": (
        "read-only-import: old records stay untouched; converted copy written to "
        "the new isolated store; per-session receipt + integrity check; authority "
        "flips per session/import-batch only after validation; no dual-write"),
    "preserve-format": (
        "format-frozen: new kernel reads/writes the SAME on-disk format; any "
        "format evolution requires an epoch bump; no conversion at cutover"),
    "regenerable": "drop-and-rebuild: never migrated; rebuilt on demand in the new world",
    "shell-local": "not-migrated: each shell keeps its own local state",
    "epoch-mechanism": "owned by the epoch coordinator; carried forward by the transition itself",
}

# measured epoch-gate conclusions (probe evidence ids under artifacts/rust-tauri/R01/T07/)
def epoch_conclusion(s):
    writers = s["processes"]["writers"]
    sid = s["id"]
    if sid in EPOCH_MECH:
        return {"status": "self", "evidence": "source: core/data-epoch-coordinator.ts",
                "note": "gate metadata itself; written only by coordinator/maintenance"}
    if sid == "server-runtime-info":
        return {"status": "not-protected-pre-gate", "evidence": "probe A6",
                "note": ("same-home mutex runs BEFORE the epoch gate: a stale "
                         "server-info.json in a high-epoch home is unlinked "
                         "pre-gate (measured, probe A6). Regenerable runtime "
                         "state; the only pre-gate mutation found.")}
    if set(writers) <= {"server"}:
        return {"status": "protected-if-stamp-intact", "evidence": "probes A1/A2/A3b/R11 (zero fs diff)",
                "note": ("server/index.ts epoch gate runs before any store opens; an intact "
                         "readable higher stamp blocks (A1/A2), and a higher-epoch transition "
                         "journal blocks ONLY when the coordinator classifies it as "
                         "incomplete-transition carrying toEpoch (A3b, R11). MEASURED BOUNDARY "
                         "(repair-r1, per REVIEW_R1 F1): corrupt-class failures "
                         "(corrupt-stamp/corrupt-journal/corrupt-transition) drop the journal "
                         "toEpoch (core/data-epoch-coordinator.ts:547-550), so with a "
                         "corrupt/missing stamp the gate fails OPEN under the DATA_EPOCH=1 "
                         "baseline EVEN WHEN a readable journal targeting epoch 2 is present "
                         "(A8, R7/R9/R10, 55 files written each); refusal without a stamp "
                         "depends on journal phase (prepared -> blocked R11; barrier_raised "
                         "-> corrupt-transition -> fail-open R10). "
                         "LINGXI_ALLOW_DATA_DOWNGRADE=1 overrides (probe A5) and the stamp "
                         "is tamperable (cooperative gate, ADV-B) -> physical separation "
                         "(ADR-004 D2) is required, not optional. Production defect filed "
                         "for R02: carry journal from/toEpoch on corrupt-class failures and "
                         "fix the 'no higher-epoch evidence' warning text.")}
    if "desktop" in writers:
        return {"status": "not-gated-by-construction", "evidence": "source: desktop/main.cjs has no coordinator call",
                "note": ("desktop shell never runs the epoch coordinator; it only "
                         "reacts to the server's stderr marker with a dialog. "
                         "Shell-local non-authoritative state; isolation comes from "
                         "separated data roots, not from the gate. Probe-testing the "
                         "GUI shell was out of scope (headless env); status by source audit. "
                         "RISK (REVIEW_R1 F3, owner R09): these shell-local paths live INSIDE "
                         "the shared data root, so under the separated-roots + pointer model "
                         "the old and new shells would race on the same paths inside the "
                         "pointer-target root. R09 must place new-shell shell-state outside "
                         "the shared root (or per-shell isolated paths) and make 'never "
                         "launch the old shell while the pointer targets epoch-2' an "
                         "install-chain acceptance gate.")}
    if set(writers) <= {"cli(data)"} or "cli(data)" in writers:
        return {"status": "maintenance-surface", "evidence": "probe A9",
                "note": "cli data diagnose reads a high-epoch home without writing (exit 0)"}
    return {"status": "unknown", "evidence": "none", "note": "writer set not classified: " + ",".join(writers)}


def rollback(s, strategy):
    kind = s["classification"]["kind"]
    if strategy == "switch-to-new-authority":
        return ("pre-rollback export to rollback archive (messages->JSONL, manifest "
                "+ sha256) BEFORE any restore; old backup restores only its own "
                "point-in-time; post-cutover content is NOT editable in the old "
                "version; re-import on next forward roll is idempotent (drill step7)")
    if strategy == "preserve-format":
        return ("files remain valid for the old version (same format); restored "
                "backup supersedes only itself; post-cutover edits written by the "
                "new kernel in the frozen format remain readable; if the format "
                "ever diverges, that divergence is itself an epoch bump")
    if strategy == "regenerable":
        return "no rollback action; dropped and rebuilt by whichever world is active"
    if strategy == "shell-local":
        return "per-shell state; old shell keeps its own copy; nothing to restore"
    return "restored only by the coordinated epoch transition/maintenance flow"


out = {
    "schema_version": "1.0",
    "task_id": "R01-T07",
    "stage_id": "R01",
    "generated_by": "ZCode:R01-T07-exec-r1 (2026-09-25), generator: docs/rust-tauri/R01/r01_t07_build_matrix.py; revised by ZCode:R01-T07-repair-r1 (2026-09-25) per REVIEW_R1 F1/F3 (epoch-boundary + shell-local notes only; strategy/counts unchanged)",
    "deterministic": True,
    "source_inputs": {
        "stores": "docs/rust-tauri/R00/STORES.json@HEAD",
        "owners": "docs/rust-tauri/R01/OWNERSHIP_TARGET.json@HEAD",
        "epoch_probes": "artifacts/rust-tauri/R01/T07/a13/ + snapshots/ (probes A1-A9); artifacts/rust-tauri/R01/T07/repair-r1/ (probes R7/R9/R10/R11 + C1 control, rounds 1-2)",
        "rollback_drill": "artifacts/rust-tauri/R01/T07/a14/ (ROLLBACK-DRILL-PASSED)",
    },
    "old_binary_basis": {
        "choice": "current-HEAD production server/CLI (node server/main-full.ts; node cli/entry.ts)",
        "justification": [
            "repo ItsDalk-Lane/LingxiAgent is the release single source of truth; HEAD=82870879 is the released code lineage",
            "the epoch gate (shared/data-epoch.cjs stamp schema v2 + core/data-epoch-coordinator.ts + server/index.ts gate + desktop stderr-marker dialog) predates baseline commit d5275e568, i.e. present in all current releases",
            "epoch behavior of the HEAD build is therefore byte-representative of the fielded old binary",
        ],
        "limitations": [
            "not a downloaded release DMG; installer-level behavior not re-probed",
            "Electron GUI shell not launched (headless env); desktop stores classified by source audit",
        ],
    },
    "counts": {},
    "stores": [],
}

count_by_strategy = {}
count_by_protection = {}
for s in sorted(stores, key=lambda x: x["id"]):
    sid = s["id"]
    if sid in EPOCH_MECH:
        strategy = "epoch-mechanism"
    elif sid in SHELL_LOCAL:
        strategy = "shell-local"
    elif s["classification"]["kind"] == "rebuildable_cache":
        strategy = "regenerable"
    elif sid in SWITCH:
        strategy = "switch-to-new-authority"
    else:
        strategy = "preserve-format"
    o = owners.get(sid, {})
    ep = epoch_conclusion(s)
    out["stores"].append({
        "store_id": sid,
        "classification": s["classification"],
        "format": s["format"],
        "path_patterns": s["path_patterns"],
        "current_writers": s["processes"]["writers"],
        "target_owner": o.get("target_owner"),
        "target_writer_process": o.get("target_writer_process"),
        "cutover_strategy": strategy,
        "migration": MIGRATION[strategy],
        "epoch_protection": ep,
        "rollback": rollback(s, strategy),
    })
    count_by_strategy[strategy] = count_by_strategy.get(strategy, 0) + 1
    count_by_protection[ep["status"]] = count_by_protection.get(ep["status"], 0) + 1

out["counts"] = {
    "stores": len(out["stores"]),
    "by_cutover_strategy": count_by_strategy,
    "by_epoch_protection": count_by_protection,
}
dest = REPO / "docs/rust-tauri/R01/DATA_COMPATIBILITY_MATRIX.json"
dest.write_text(json.dumps(out, ensure_ascii=False, indent=1) + "\n")
print("written", dest, len(out["stores"]), "stores")
print(json.dumps(out["counts"], ensure_ascii=False, indent=1))
