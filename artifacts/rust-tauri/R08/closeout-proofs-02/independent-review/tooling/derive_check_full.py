#!/usr/bin/env python3
"""独立复算：门-08 当轮 A/B 原始观测 → 全部 350 个 AB 叶图钉（含手写派生表）。
按 r08_stage_inputs.py 的派生口径逐条改写为独立实现（从原始 observed 值计算），
与门内 cases.json 对照。"""
import json
from pathlib import Path

SCRATCH = Path("/private/tmp/lingxi-r08-verify/r08_t07_ab-61788-1793016143174/out")
GATE = Path("/Users/study_superior/Desktop/Code/LingxiAgent/artifacts/rust-tauri/R08/verify-r08-closeout-08")

node = json.loads((SCRATCH / "ab-node.json").read_text())["observations"]
rust = json.loads((SCRATCH / "ab-rust.json").read_text())["observations"]
gate_cases = {c["case"]: c for c in json.loads((GATE / "SUITES/r08_t07_ab/cases.json").read_text())["cases"]}

def obs(leg, task):
    return ((leg.get(task) or {}).get("observed") or {})

def both(task, *fields):
    return 1 if all(obs(leg, task).get(f) for leg in (node, rust) for f in fields) else 0

C = {}
# shared
a_list, b_list = obs(node, "sessions.list"), obs(rust, "sessions.list")
pin, pin_b = obs(node, "sessions.pin"), obs(rust, "sessions.pin")
todos, todos_b = obs(node, "sessions.todos"), obs(rust, "sessions.todos")
maint, maint_b = obs(node, "sessions.maintenance"), obs(rust, "sessions.maintenance")
perm, perm_b = obs(node, "permissions.default"), obs(rust, "permissions.default")

C["ab::sessions.list.count"] = 1 if (a_list.get("count", 0) > 0 and b_list.get("count", 0) > 0) else 0
C["ab::sessions.list.parity"] = 1 if a_list.get("count") == b_list.get("count") else 0
C["ab::sessions.rename.ok"] = both("sessions.rename", "ok")
C["ab::sessions.rename.persisted"] = both("sessions.rename", "persisted")
C["ab::sessions.pin.pin"] = 1 if (pin.get("pinA") and pin.get("pinB") and pin_b.get("pinA") and pin_b.get("pinB")) else 0
C["ab::sessions.pin.unpin"] = 1 if (pin.get("unpinA") and pin.get("unpinB") and pin_b.get("unpinA") and pin_b.get("unpinB")) else 0
C["ab::sessions.todos.complete"] = 1 if (todos.get("complete") and todos_b.get("complete")) else 0
C["ab::sessions.todos.cancel"] = 1 if (todos.get("cancel") and todos_b.get("cancel")) else 0
C["ab::sessions.todos.dismiss"] = 1 if (todos.get("dismiss") and todos_b.get("dismiss")) else 0
C["ab::sessions.maintenance.cleanup"] = 1 if (maint.get("cleanupDeletedIsNumber") and maint_b.get("cleanupDeletedIsNumber")) else 0
C["ab::sessions.maintenance.sweep"] = 1 if (maint.get("sweptIsNumber") and maint_b.get("sweptIsNumber")) else 0
C["ab::permissions.default.parity"] = 1 if (perm.get("mode") == perm_b.get("mode") and perm.get("mode") is not None) else 0
C["leaf::permissions.mode-nonnull"] = 1 if (perm.get("mode") is not None and perm_b.get("mode") is not None) else 0

neg, neg_b = obs(node, "leaf.rename.negative"), obs(rust, "leaf.rename.negative")
pin_neg, pin_neg_b = obs(node, "leaf.pin.negative"), obs(rust, "leaf.pin.negative")
todos_neg, todos_neg_b = obs(node, "leaf.todos.negative"), obs(rust, "leaf.todos.negative")
clean, clean_b = obs(node, "leaf.cleanup.boundary"), obs(rust, "leaf.cleanup.boundary")
fields, fields_b = obs(node, "leaf.list.fields"), obs(rust, "leaf.list.fields")
pidem, pidem_b = obs(node, "leaf.permissions.read-idempotent"), obs(rust, "leaf.permissions.read-idempotent")
aneg, aneg_b = obs(node, "leaf.archive.negative"), obs(rust, "leaf.archive.negative")
psm, psm_b = obs(node, "leaf.permission.session-mode"), obs(rust, "leaf.permission.session-mode")
pds, pds_b = obs(node, "leaf.permission.default-set"), obs(rust, "leaf.permission.default-set")
mcp, mcp_b = obs(node, "leaf.mcp.state"), obs(rust, "leaf.mcp.state")
mcpu, mcpu_b = obs(node, "leaf.mcp.state.unauthorized"), obs(rust, "leaf.mcp.state.unauthorized")
memsnap, memsnap_b = obs(node, "leaf.memory-snapshot"), obs(rust, "leaf.memory-snapshot")
authf, authf_b = obs(node, "leaf.auth-folders"), obs(rust, "leaf.auth-folders")
rport, rport_b = obs(node, "leaf.read-ports"), obs(rust, "leaf.read-ports")
pprj, pprj_b = obs(node, "leaf.pin.projection"), obs(rust, "leaf.pin.projection")
awsp, awsp_b = obs(node, "leaf.archive.workspace"), obs(rust, "leaf.archive.workspace")
fnd, fnd_b = obs(node, "leaf.find.in-session"), obs(rust, "leaf.find.in-session")
pino, pino_b = obs(node, "leaf.pin-order"), obs(rust, "leaf.pin-order")
adel, adel_b = obs(node, "leaf.archived-delete"), obs(rust, "leaf.archived-delete")
srch, srch_b = obs(node, "leaf.search"), obs(rust, "leaf.search")
archv, archv_b = obs(node, "sessions.archive"), obs(rust, "sessions.archive")

C["leaf::rename.empty-title.rejected"] = 1 if (neg.get("emptyRejected") and neg_b.get("emptyRejected")) else 0
C["leaf::rename.invalid-session.explicit-error"] = 1 if (neg.get("invalidRejected") and neg_b.get("invalidRejected")) else 0
C["leaf::pin.invalid-session.explicit-error"] = 1 if (pin_neg.get("rejected") and pin_neg_b.get("rejected")) else 0
C["leaf::todos.invalid-session.explicit-error"] = 1 if (todos_neg.get("rejected") and todos_neg_b.get("rejected")) else 0
C["leaf::cleanup.fresh-archive-kept"] = 1 if (clean.get("freshKept") and clean_b.get("freshKept")) else 0
C["leaf::cleanup.archived-delete-with-effect"] = 1 if (clean.get("purgedOk") and clean_b.get("purgedOk") and clean.get("expiringGone") and clean_b.get("expiringGone")) else 0
C["leaf::list.revision-field-present"] = 1 if (fields.get("hasRevision") and fields_b.get("hasRevision")) else 0
C["leaf::list.summary-or-badge-present"] = 1 if (fields.get("hasSummaryOrBadge") and fields_b.get("hasSummaryOrBadge")) else 0
C["leaf::list.unauthorized-rejected"] = 1 if (obs(node, "error-path.unauthorized").get("rejected") and obs(rust, "error-path.unauthorized").get("rejected")) else 0
C["leaf::permissions.read-idempotent"] = 1 if (pidem.get("identical") and pidem_b.get("identical") and pidem.get("mode") == pidem_b.get("mode")) else 0
C["leaf::mcp.state.shape"] = 1 if (mcp.get("ok") and mcp_b.get("ok") and mcp.get("enabledIsBool") and mcp_b.get("enabledIsBool") and mcp.get("connectorsIsArray") and mcp_b.get("connectorsIsArray")) else 0
C["leaf::mcp.state.unauthorized-rejected"] = 1 if (mcpu.get("rejected") and mcpu_b.get("rejected")) else 0
C["leaf::permission.session-mode-present"] = 1 if (psm.get("modePresent") and psm_b.get("modePresent")) else 0
C["leaf::permission.session-mode-idempotent"] = 1 if (psm.get("idempotent") and psm_b.get("idempotent")) else 0
C["leaf::permission.default-roundtrip"] = 1 if (pds.get("roundTrip") and pds_b.get("roundTrip")) else 0
C["leaf::permission.default-invalid-rejected"] = 1 if (pds.get("badRejected") and pds_b.get("badRejected")) else 0
C["leaf::archive.negative.rejected"] = 1 if (aneg.get("archiveRejected") and aneg_b.get("archiveRejected") and aneg.get("restoreRejected") and aneg_b.get("restoreRejected")) else 0
C["leaf::memory.read"] = 1 if (memsnap.get("memoryRead") and memsnap_b.get("memoryRead")) else 0
C["leaf::memory.set"] = 1 if (memsnap.get("memorySet") and memsnap_b.get("memorySet")) else 0
C["leaf::prompt-snapshot.read"] = 1 if (memsnap.get("snapshotOk") and memsnap_b.get("snapshotOk")) else 0
C["leaf::memory-snapshot.invalid-rejected"] = 1 if (memsnap.get("invalidRejected") and memsnap_b.get("invalidRejected")) else 0
C["leaf::auth-folders.read"] = 1 if (authf.get("readOk") and authf_b.get("readOk")) else 0
C["leaf::auth-folders.add"] = 1 if (authf.get("addOk") and authf_b.get("addOk")) else 0
C["leaf::auth-folders.remove"] = 1 if (authf.get("removeOk") and authf_b.get("removeOk")) else 0
C["leaf::auth-folders.set"] = 1 if (authf.get("setOk") and authf_b.get("setOk")) else 0
C["leaf::auth-folders.invalid-rejected"] = 1 if (authf.get("invalidRejected") and authf_b.get("invalidRejected")) else 0
C["leaf::read.history-page"] = 1 if (rport.get("historyOk") and rport_b.get("historyOk")) else 0
C["leaf::read.overview"] = 1 if (rport.get("overviewOk") and rport_b.get("overviewOk")) else 0
C["leaf::read.summary"] = 1 if (rport.get("summaryOk") and rport_b.get("summaryOk")) else 0
C["leaf::read.invalid-rejected"] = 1 if (rport.get("invalidRejected") and rport_b.get("invalidRejected")) else 0
C["leaf::pin.state-projected"] = 1 if (pprj.get("pinnedProjected") and pprj_b.get("pinnedProjected")) else 0
C["leaf::pin.state-cleared"] = 1 if (pprj.get("clearedProjected") and pprj_b.get("clearedProjected")) else 0
C["leaf::archive-workspace.recoverable"] = 1 if (awsp.get("archivedToRecoverable") and awsp_b.get("archivedToRecoverable")) else 0
C["leaf::archive-workspace.invalid-rejected"] = 1 if (awsp.get("invalidRejected") and awsp_b.get("invalidRejected")) else 0
C["leaf::find.in-session-hit"] = 1 if (fnd.get("hitAtLeastOne") and fnd_b.get("hitAtLeastOne")) else 0
C["leaf::find.in-session-invalid-rejected"] = 1 if (fnd.get("invalidRejected") and fnd_b.get("invalidRejected")) else 0
C["leaf::pin-order.reorder-applies"] = 1 if (pino.get("reorderOk") and pino_b.get("reorderOk")) else 0
C["leaf::pin-order.duplicate-rejected"] = 1 if (pino.get("dupRejected") and pino_b.get("dupRejected")) else 0
C["leaf::archived-delete.file-and-lifecycle-cleared"] = 1 if (adel.get("deleted") and adel_b.get("deleted") and adel.get("goneFromArchive") and adel_b.get("goneFromArchive")) else 0
C["leaf::archived-delete.unrecoverable"] = 1 if (adel.get("restoreAfterDeleteRefused") and adel_b.get("restoreAfterDeleteRefused")) else 0
C["leaf::search.content-hit"] = 1 if (srch.get("hitAtLeastOne") and srch_b.get("hitAtLeastOne")) else 0
C["leaf::search.empty-query-empty-result"] = 1 if (srch.get("emptyIsEmpty") and srch_b.get("emptyIsEmpty")) else 0
C["leaf::archive.positive-cycle"] = 1 if (archv.get("inArchive") and archv_b.get("inArchive") and archv.get("goneFromVisible") and archv_b.get("goneFromVisible") and archv.get("restored") and archv_b.get("restored") and archv.get("backVisible") and archv_b.get("backVisible")) else 0
C["leaf::cleanup.expired-deleted"] = 1 if (maint.get("cleanupDeletedAtLeastOne") and maint_b.get("cleanupDeletedAtLeastOne")) else 0

# generic cases:{name:bool} tasks
generic = {}
for task in sorted(set(node) | set(rust)):
    ca = (node.get(task) or {}).get("observed", {}).get("cases")
    cb = (rust.get(task) or {}).get("observed", {}).get("cases")
    if not isinstance(ca, dict) and not isinstance(cb, dict):
        continue
    ca = ca if isinstance(ca, dict) else {}
    cb = cb if isinstance(cb, dict) else {}
    for name in sorted(set(ca) | set(cb)):
        generic[f"leaf::{task}.{name}"] = 1 if ca.get(name) is True and cb.get(name) is True else 0
C.update(generic)

# adjudicated rust-authoritative
ADJ = {
    "leaf.slash.new": ["slash.new.rotates-and-archives", "slash.new.old-recoverable", "slash.new.explicit-reply", "slash.new.invalid-session-rejected"],
    "leaf.slash": ["slash.reset.explicit-reply", "slash.reset.history-cleared", "slash.reset.other-sessions-intact", "slash.reset.invalid-session-rejected"],
}
for task, names in ADJ.items():
    cases = (rust.get(task) or {}).get("observed", {}).get("cases") or {}
    for name in names:
        C[f"leaf::{task}.{name}"] = 1 if cases.get(name) is True else 0

# compare with gate cases.json (exclude cargo side case)
mismatch = []
for case, row in gate_cases.items():
    if case == "leaf::find.bridge-redaction":
        continue
    if case not in C:
        mismatch.append({"case": case, "reason": "no independent derivation"})
    elif C[case] != row["actual"] or row["actual"] != 1 or row["ok"] is not True:
        mismatch.append({"case": case, "independent": C[case], "gate": row})

verified = sum(1 for c in gate_cases if c in C and c != "leaf::find.bridge-redaction")
print(json.dumps({"verified_ab_cases": verified, "gate_ab_cases": len(gate_cases), "mismatch": mismatch,
                  "adj_detail": {t: {n: {"A": ((node.get(t) or {}).get("observed", {}).get("cases") or {}).get(n),
                                          "B": ((rust.get(t) or {}).get("observed", {}).get("cases") or {}).get(n)}
                                      for n in names} for t, names in ADJ.items()}},
                 ensure_ascii=False, indent=1))
Path("/private/tmp/review-independent/ab-derivation-full.json").write_text(json.dumps({"verified": verified, "mismatch": mismatch}, ensure_ascii=False, indent=1) + "\n")
