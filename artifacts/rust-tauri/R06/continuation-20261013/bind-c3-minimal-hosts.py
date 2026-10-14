"""Bind the C3 minimal-host leaves (STAGE-REVIEWER-R06-AR-R1 adjudication b).

Seven leaves flip GAP->READY once every original assertion group is FULL and
backed by an executed test; original assertion text is never edited.
Registration is not stage acceptance.
"""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("anchors", HERE / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())
rows = {row["id"]: row for row in inventory["items"]}

KP = "r06_original_knowledge_proofs"
CFG = "r06_agent_config_readback"
SPEC = {
    "R00-T02-LA-BC675C664910": {
        0: ([(CFG, "get_config_reads_real_yaml_injects_globals_and_masks_every_secret"),
             (CFG, "get_config_experience_enabled_is_boolean_and_invalid_config_fails_loudly"),
             (CFG, "get_config_without_catalog_reads_without_creating_it")],
            "GET reads the agent's real config.yaml, injects the 21 global fields from preferences, adds _raw, "
            "provider summary (masked api_key, model_count) and availableTools; every secret key is masked as "
            "******** with no plaintext anywhere in the body; on-disk yaml/preferences/catalog bytes unchanged."),
        1: ([(CFG, "get_config_gcs_gone_workspace_history_before_answering_only_for_that_agent"),
             (CFG, "get_config_provider_registry_failure_gives_empty_providers_not_an_error"),
             (CFG, "get_config_identity_is_keyed_by_path_not_focus_and_never_discloses_another_agent"),
             (CFG, "get_config_unknown_or_invalid_agent_is_404_with_no_side_effects"),
             ("auth_matrix", "agent_config_get_requires_settings_read_scope_without_side_effects")],
            "Gone-folder workspace history is GC'd on disk before the answer (other agent untouched, second GET "
            "rewrites nothing); corrupt provider catalog gives providers {} with 200; identity is keyed by path "
            "not focus and never discloses another agent; unknown ids 404 with zero side effects; scope/credential "
            "refusals (401/403) leave files unchanged."),
    },
    "R00-T02-LA-01E1E3CF7FA8": {
        0: ([(KP, "original_notebook_delete_purges_owned_sources_and_keeps_shared_ones"),
             (KP, "original_notebook_delete_refuses_frozen_foreign_and_missing_without_side_effects")],
            "Real DELETE returns the deleted notebook with deletedSourceIds/cancelledJobs; owned source rows, jobs, "
            "snapshot/artifact files and FTS variants are gone while a shared source keeps row, bytes and the "
            "other notebook's membership; a frozen run scope gives 409 with every row/file unchanged, and the "
            "same delete succeeds after the freeze is released."),
        1: ([(KP, "original_notebook_delete_refuses_frozen_foreign_and_missing_without_side_effects")],
            "409 (frozen), 404 (foreign studio, missing) statuses match actual side effects: notebook, memberships, "
            "job rows, source rows and files unchanged on every refusal."),
    },
    "R00-T02-LA-9283371CDAD8": {
        1: ([(KP, "original_source_delete_refuses_foreign_studio_without_changing_rows_or_files")],
            "Foreign-studio and missing source ids give 404 with foreign source row, membership, failed job and "
            "snapshot file unchanged; local delete cancels jobs and removes only its own rows/files. Frozen 409 and "
            "delete-wins remain proven by the earlier registered test."),
    },
    "R00-T02-LA-8A62470CC75D": {
        1: ([(KP, "original_reingest_refuses_foreign_and_non_member_sources_without_creating_jobs")],
            "Foreign-studio, non-member, membership-removed, missing notebook and missing source each give 404 "
            "with total job-row count unchanged; a local member source without job gets one new job, a second "
            "reingest is 409 with no new job. Non-failed 409 / deleted-source 404 / delete-wins stay proven by "
            "the earlier registered tests."),
    },
    "R00-T02-LA-76E2F5AAF035": {
        0: ([(KP, "original_ingestion_listing_is_studio_scoped_and_reports_unreachable_sources")],
            "Jobs and counts equal exactly the caller studio's stored job rows (no foreign id in the body); after "
            "the original file is deleted unreachableSources lists only the caller's file source (ENOENT, no path "
            "disclosure) and clears when the file is recreated."),
        1: ([(KP, "original_ingestion_listing_is_studio_scoped_and_reports_unreachable_sources")],
            "notebookId/sourceId filters narrow correctly; a foreign notebook/source id is indistinguishable from a "
            "nonexistent id (empty jobs, zero counts); foreign job rows and the job table are unchanged."),
    },
    "R00-T02-LA-5F5365322CFF": {
        0: ([(KP, "original_outline_sections_drive_knowledge_read_and_refuse_out_of_scope")],
            "Real scripted run: sectionId from knowledge_outline reads exactly that section's block range, "
            "aroundChunkId/neighborWindow read exact neighbourhoods, maxChars paging reports nextOffset."),
        1: ([(KP, "original_outline_sections_drive_knowledge_read_and_refuse_out_of_scope")],
            "Section/chunk ids from an unselected same-notebook source, a foreign studio or nonexistent ids are "
            "KNOWLEDGE_SCOPE_VIOLATION without leaking text; selector conflicts are KNOWLEDGE_INVALID_ARGUMENT."),
        2: ([(KP, "original_outline_sections_drive_knowledge_read_and_refuse_out_of_scope")],
            "outline over foreign/unselected/nonexistent notebook or source ids is an error, not empty success; an "
            "empty document yields sections [] and chunkCount 0 while read reports KNOWLEDGE_PARSE_NOT_READY; rows "
            "unchanged."),
    },
    "R00-T02-LA-269958F82F8A": {
        2: ([(KP, "original_grep_filters_reject_invalid_values_and_apply_heading_prefix")],
            "Re-anchored to current source: grep_scope rejects foreign/missing sourceIds with "
            "KNOWLEDGE_SCOPE_VIOLATION (loop over calls 12-14 in the authentic-run test), sourceIds [] / blank / "
            "over-256 headingFilter / non-string headingFilter as invalid, no-match returns matches [] explicitly."),
    },
}

report = {"status": "EXECUTED_TESTS_BOUND_NOT_STAGE_ACCEPTANCE", "leaves": {}}
for leaf_id, groups in SPEC.items():
    row = rows[leaf_id]
    original = {"then": row["then"], "assertions": list(row["assertions"])}
    for index, (tests, meaning) in groups.items():
        group = next((g for g in row["groups"] if g["index"] == index), None)
        if group is None:
            group = {"index": index, "tests": [], "meaning": "", "proofLocations": [],
                     "coverage": "PARTIAL", "missingClauses": ["x"]}
            row["groups"].append(group)
        for suite, test in tests:
            reference = {"package": "lingxi-service", "suite": suite, "test": test}
            if reference not in group["tests"]:
                group["tests"].append(reference)
                group["proofLocations"].append(anchors.test_location(suite, test))
        group.update(coverage="FULL", missingClauses=[], meaning=meaning)
    row["groups"].sort(key=lambda g: g["index"])
    assert [g["index"] for g in row["groups"]] == list(range(len(row["assertions"]))), leaf_id
    assert all(g["coverage"] == "FULL" and not g["missingClauses"] for g in row["groups"]), leaf_id
    assert row["then"] == original["then"] and row["assertions"] == original["assertions"]
    row_report = HERE / "c3-minimal-hosts-proof-review.json"
    row.update(status="READY", gaps=[], reviewSource=row_report.relative_to(anchors.ROOT).as_posix())
    report["leaves"][leaf_id] = {"boundGroups": sorted(groups)}
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
(HERE / "c3-minimal-hosts-proof-review.json").write_text(
    json.dumps(report, ensure_ascii=False, indent=2) + "\n")
ids = HERE / "reviewed-original-ids.json"
selected = json.loads(ids.read_text())
for leaf_id in SPEC:
    if leaf_id not in selected:
        selected.append(leaf_id)
ids.write_text(json.dumps(selected, indent=2) + "\n")
print("Bound", len(SPEC), "C3 leaves; no R06 or R07 verdict.")
