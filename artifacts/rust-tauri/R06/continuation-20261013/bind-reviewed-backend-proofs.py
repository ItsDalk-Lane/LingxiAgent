"""Bind semantically reviewed backend repairs to their exact original clauses."""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("anchors", HERE / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)

REPAIRS = {
    "R00-T02-LA-07A5CE3637C7": {
        1: ("r06_original_persona_proofs", "original_persona_reads_template_custom_alias_and_refusal",
            "AGENTS.md and its ishiki alias both reject invalid and missing assistant IDs with 404, without template/custom-file disclosure or writes."),
    },
    "R00-T02-LA-0F7D128E01B1": {
        0: ("r06_original_memory_skill_proofs", "original_manual_skill_install_enables_only_requested_agent_and_refreshes_runtime",
            "Real manual HTTP install preserves package files, persists the requested assistant's enabled list, and immediately updates the existing runtime catalog and authenticated runtime list."),
        1: ("r06_original_memory_skill_proofs", "original_manual_skill_install_enables_only_requested_agent_and_refreshes_runtime",
            "Missing/invalid/duplicate assistant IDs and actual missing/relative package failures reject without installing a refused package or changing either assistant's configuration."),
    },
    "R00-T02-LA-0CDC88FC0999": {
        0: ("r06_original_resource_proofs", "resourceio_write_conflict_refuses_external_change_at_commit_boundary",
            "Ordinary and expected-version writes return 409 for a genuine commit-boundary concurrent edit, preserve the user's bytes and leave registration/event state unchanged."),
    },
    "R00-T02-LA-895DAC8DD5B7": {
        1: ("r06_original_resource_proofs", "file_history_failures_and_conflict_preserve_user_bytes_and_snapshot_rows",
            "Real restore write failure/conflict preserve files and snapshots; a SQLite snapshot-insert trigger failure returns 500 with fileWritten=true/snapshotRecorded=false, and successful retry plus repeated restore each persist a restore audit snapshot."),
    },
    "R00-T02-LA-00ECC9568490": {
        0: ("r06_original_chat_proofs", "subagent_list_launch_progress_and_permission_intersection_use_real_facts",
            "Actual parent launch returns taskId/threadId, persisted parent ownership and genuine run progress."),
        2: ("r06_original_chat_proofs", "subagent_actual_panic_and_provider_failure_return_reasons_and_reclaim_capacity",
            "Actual provider panic yields its stable error code; provider failure reason and completed child results reach only the owning parent; capacity is reclaimed."),
        3: ("r06_original_chat_proofs", "subagent_session_and_global_caps_refuse_and_recover_without_ghost_children",
            "Session/global caps reject excess work without ghost children and recover capacity; candidate-list and permission-intersection checks use actual agent/run facts."),
    },
    "R00-T02-LA-041F938C351F": {
        1: ("r06_original_memory_skill_proofs", "original_restore_detail_hash_refuses_concurrent_edits_and_preserves_agent_scope",
            "A changed confirmed snapshot returns 409 without writes; real compile/maintenance/dream/restore activity rejects restoration, with independent agent isolation."),
    },
    "R00-T02-LA-1CD1C31CCF44": {
        2: ("r06_original_resource_proofs", "stage_files_gateway_only_delivers_real_authorized_session_files",
            "Missing/empty/whitespace/foreign session identities, bad paths, expired payloads and actual SQLite registration failures refuse without new sessions/files/references."),
    },
    "R00-T02-LA-207DF594B497": {
        1: ("r06_original_chat_proofs", "subagent_close_and_reply_refusals_preserve_other_threads_and_sessions",
            "Both durable thread state and the actual parent model tool result report streamStatus/status closed for only the requested thread."),
    },
    "R00-T02-LA-3F95164A488C": {
        0: ("r06_original_session_proofs", "original_archive_children_recurses_skips_running_and_preserves_failed_transaction",
            "Real running child is skipped; parent and two descendant levels archive and restore; success counts and transaction-failure error/rollback counts are read from HTTP."),
    },
    "R00-T02-LA-5F879158C679": {
        0: ("r06_original_chat_proofs", "subagent_reply_restores_actual_thread_history_and_returns_only_to_its_parent",
            "Same thread launches a fresh child with real prior user/assistant history and retained result; foreign parent never receives history or result."),
    },
    "R00-T02-LA-A18187567AF9": {
        0: ("r06_original_session_proofs", "original_session_search_returns_scoped_title_content_and_explicit_read_failures",
            "Authenticated title/content searches return matched snippets, including long multibyte titles; empty query, limits, foreign isolation and real read failure are checked."),
    },
}


def binding(suite, test):
    source = anchors.ROOT / "rust/crates/lingxi-service/tests" / (suite + ".rs")
    raw = source.read_text()
    declaration = anchors.re.search(r"\bfn\s+" + anchors.re.escape(test) + r"\s*\(", raw)
    if not declaration:
        raise ValueError(test)
    level = 0
    opened = False
    for token in anchors.tokens(raw[declaration.end():]):
        if token.group() == "{":
            opened = True
            level += 1
        elif token.group() == "}":
            level -= 1
            if opened and level == 0:
                end = declaration.end() + token.end()
                break
    else:
        raise ValueError("Cannot delimit " + test)
    body = raw[declaration.start():end]
    return {
        "path": source.relative_to(anchors.ROOT).as_posix(), "test": test,
        "sourceSHA256": anchors.hashlib.sha256(source.read_bytes()).hexdigest(),
        "testStartLine": raw[:declaration.start()].count("\n") + 1,
        "testEndLine": raw[:end].count("\n") + 1,
        "assertions": anchors.assertions_in_body(raw, body, declaration),
    }


inventory = json.loads(anchors.MAP.read_text())
reviewed = []
for row in inventory["items"]:
    replacements = REPAIRS.get(row["id"])
    if replacements is None or row["status"] == "READY":
        continue
    for index in replacements:
        if not any(group["index"] == index for group in row["groups"]):
            row["groups"].append({"index": index, "tests": [], "proofLocations": []})
    for group in row["groups"]:
        update = replacements.get(group["index"])
        if update is None:
            continue
        suite, test, meaning = update
        additions = [(suite, test)]
        if row["id"] == "R00-T02-LA-041F938C351F":
            additions.append(("r06_original_memory_skill_proofs",
                              "original_restore_refuses_real_compilation_tenet_maintenance_dream_and_restore_busy"))
        if row["id"] == "R00-T02-LA-00ECC9568490":
            additions.append(("r06_original_chat_proofs",
                              "subagent_list_launch_progress_and_permission_intersection_use_real_facts"))
        for suite, test in additions:
            reference = {"package": "lingxi-service", "suite": suite, "test": test}
            if reference not in group["tests"]:
                group["tests"].append(reference)
                group["proofLocations"].append(binding(suite, test))
        group.update(coverage="FULL", missingClauses=[], meaning=meaning)
    if len(row["groups"]) != len(row["assertions"]) or any(g["coverage"] != "FULL" for g in row["groups"]):
        raise ValueError("Incomplete original clauses: " + row["id"])
    reviewed.append({"id": row["id"], "originalAssertions": row["assertions"],
                     "previousGaps": row["gaps"], "currentTests": [
                         test for group in row["groups"] for test in group["tests"]]})
    row.update(status="READY", gaps=[], reviewSource=(
        "artifacts/rust-tauri/R06/continuation-20261013/skill-persona-proof-review.json"))
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
(HERE / "skill-persona-proof-review.json").write_text(json.dumps({
    "status": "SEMANTIC_BINDINGS_REVIEWED_NOT_STAGE_ACCEPTANCE",
    "executionLogs": ["skill-persona-regression-r2.log", "skill-install-rejection-regression.log"],
    "reviewed": reviewed,
}, ensure_ascii=False, indent=2) + "\n")
print(f"Updated {len(reviewed)} reviewed backend leaves; formal execution evidence still required.")
