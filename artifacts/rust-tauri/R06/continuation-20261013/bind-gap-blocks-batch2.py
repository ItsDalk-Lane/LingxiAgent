"""Bind GAP_DISPATCH_PLAN B-phase leaves (G/C1/A/B5/F) after lib.rs integration.

14 leaves flip GAP->READY; 2 leaves register PARTIAL groups with explicit
missingClauses (7244E7DCE678 no-store sub-clause, 464E58DEC9E2 runtime
fallback branch) and stay GAP. C2 leaves wait for their test suite.
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

S = "lingxi-service"
TODO = "r06_todo_panel"
DISP = "r06_workspace_disposal"
READ = "r06_agent_config_readback"
SKILL = "r06_skill_dir_delete_and_export"

BATCH = {
    # ── G 块 todos ──
    "R00-T02-LA-45C5CE361CCB": {"tests": {0: [(TODO, "complete_cancel_dismiss_semantics_and_broadcast"), (TODO, "cancel_semantics_version_mismatch_and_isolation")], 1: [(TODO, "gate_rejections"), (TODO, "streaming_gate_complete_cancel_reject_dismiss_passes")]},
        "meaning": {0: "Complete appends the completion record only while unfinished items exist (completedTodos snapshot) and broadcasts todos-changed; a second complete returns the current panel without appending.", 1: "Missing sessionId 400, unknown session 404, archived 409, unauthenticated 401, streaming 409 and version-mismatch 409 (todo_version_mismatch) are all explicit rejections."}},
    "R00-T02-LA-5CD35D9EED20": {"tests": {0: [(TODO, "complete_cancel_dismiss_semantics_and_broadcast"), (TODO, "cancel_semantics_version_mismatch_and_isolation")], 1: [(TODO, "gate_rejections"), (TODO, "streaming_gate_complete_cancel_reject_dismiss_passes")]},
        "meaning": {0: "Cancel appends the cancel record for non-terminal items only and broadcasts todos-changed; terminal items stay untouched and other sessions' panels are unaffected (cancel has no run/background termination surface — isolation is asserted cross-session).", 1: "Same explicit gate vocabulary as complete: 400/404/409 busy/409 archived/409 version/401."}},
    "R00-T02-LA-17FCFE45576B": {"tests": {0: [(TODO, "complete_cancel_dismiss_semantics_and_broadcast"), (TODO, "cancel_semantics_version_mismatch_and_isolation")], 1: [(TODO, "gate_rejections")]},
        "meaning": {0: "Dismiss appends the collection record only for an ended-and-not-collected panel; history entries remain readable, a second dismiss is an idempotent no-op that does not append.", 1: "Explicit rejections mirror the family gates; dismiss has no streaming gate by incumbent TS semantics (asserted directly)."}},
    "R00-T02-LA-7244E7DCE678": {"partial": {2: ["「无 store 显示不可用」子句在端口无对应概念（存储常在，不存在无 store 形态）；已覆盖空面板与未知会话 404，不猜造 unavailable 状态"]},
        "tests": {0: [(TODO, "cancel_semantics_version_mismatch_and_isolation")], 1: [(TODO, "complete_cancel_dismiss_semantics_and_broadcast"), (TODO, "version_hash_matches_incumbent_shape")], 2: [(TODO, "gate_rejections")]},
        "meaning": {0: "The panel read returns only the owning session's items (cross-session isolation asserted).", 1: "Panel payload (todos/version/finished/allCompleted) is recomputed from the stored entries with the incumbent version hash shape.", 2: "A session with no todo entries yields the empty panel and an unknown session yields 404."}},
    # ── C1 处置/清扫 ──
    "R00-T02-LA-2D4EBE0FF3C2": {"tests": {0: [(DISP, "disposal_matches_mount_and_cwd_forms")], 1: [(DISP, "disposal_and_sweep_require_credentials_and_identity"), (DISP, "disposal_skips_busy_and_collects_per_session_errors")]},
        "meaning": {0: "Workspace disposal with action=archive matches sessions by workspaceMountId or normalized cwd and moves each matched non-streaming session into the recoverable archived lifecycle.", 1: "Missing identity 400 (missing_workspace_identity), unauthenticated 401, busy sessions skipped with counts, per-session failures collected into errors without interrupting the batch."}},
    "R00-T02-LA-21A99DB98735": {"tests": {0: [(DISP, "sweep_archives_mount_and_dir_orphans_only"), (DISP, "sweep_registry_missing_or_corrupt_falls_back_to_default_only"), (DISP, "sweep_skips_streaming_orphans")], 1: [(DISP, "disposal_and_sweep_require_credentials_and_identity"), (DISP, "cross_principal_sessions_never_enter_disposal_or_sweep")]},
        "meaning": {0: "Sweep archives exactly the two orphan kinds (mount removed from the active registry set, or cwd directory gone from disk), keeps non-orphans untouched, skips streaming orphans, and falls back to default-only when the registry is missing or corrupt (never mis-archives).", 1: "Unauthenticated 401; foreign principals never see or touch other principals' sessions in the scanned/matched sets."}},
    # ── A 块读回叶 ──
    "R00-T02-LA-9E3981A9BFCA": {"tests": {0: [(READ, "put_config_provider_save_readback")], 1: [(READ, "put_config_provider_save_readback")], 2: [(READ, "put_config_provider_save_readback")], 3: [(READ, "put_config_validation_and_auth")], 4: [(READ, "put_config_validation_and_auth")]},
        "meaning": {0: "Provider save reads back provider-catalog.json; a pure provider success refreshes the focus agent's config.yaml while the requested agent A's explicit YAML stays untouched.", 1: "Only local custom/unregistered plugins write provider-plugins files; built-in providers never do; deleting a built-in overlay keeps the built-in declaration; deleting an absent entry leaves the catalog unchanged.", 2: "models.json is a derived projection — unchanged content with correct permissions is not rewritten (mtime asserted).", 3: "A later-segment failure returns 500 while earlier catalog/plugin writes persist — no atomic-rollback claim; a missing focus agent makes the refresh fail explicitly.", 4: "Multiple providers in one request process item by item (later failure keeps earlier entries); a pure provider request writes no preferences and never touches persona/memory files."}},
    "R00-T02-LA-38A989948FDD": {"tests": {0: [(READ, "put_config_provider_save_readback")], 1: [(READ, "put_config_provider_save_readback")], 2: [(READ, "put_config_provider_save_readback")], 3: [(READ, "put_config_validation_and_auth")], 4: [(READ, "put_config_validation_and_auth")]},
        "meaning": {0: "Provider delete removes the overlay entry from provider-catalog.json and refreshes the focus agent's projection without touching the requested agent's explicit YAML.", 1: "Local plugin deletions remove the plugin files and record meta.deletedProviders; built-in deletions keep the built-in declaration; deleting what is absent in both catalog and plugins returns early with no file changes.", 2: "models.json stays a derived projection with the no-rewrite rule asserted.", 3: "Partial success on later-segment failures is preserved and reported; no atomic rollback claim.", 4: "Item-by-item processing and preferences-only-when-global-fields rules asserted."}},
    "R00-T02-LA-464E58DEC9E2": {"partial": {6: ["「请求助手运行时实例未加载时 updateConfig 回退焦点 B」分支在端口结构性不存在（无懒加载实例概念，A 可解析即目标 A、皆无显式失败）——已按端口真实行为断言"]},
        "tests": {0: [(READ, "put_config_inline_credential_transfer_readback")], 1: [(READ, "put_config_inline_credential_transfer_readback")], 2: [(READ, "put_config_inline_credential_transfer_readback")], 3: [(READ, "put_config_validation_and_auth")], 4: [(READ, "put_config_validation_and_auth")], 5: [(READ, "put_config_inline_credential_transfer_readback")], 6: [(READ, "put_config_inline_credential_transfer_readback")]},
        "meaning": {0: "Inline api/embedding_api credential transfer reads back provider-catalog.json, the focus refresh YAML and A's explicit YAML; the inline secret fields are cleared in A's YAML after the transfer.", 1: "Plugin file writes follow the same local-custom-only rule as save.", 2: "models.json projection rules asserted.", 3: "Later-segment failures keep prior writes with an explicit error; the port resolves the target agent structurally (A resolvable = A, none = explicit failure).", 4: "Item-by-item and preferences rules asserted.", 5: "An unresolvable provider rejects 400 BEFORE any write; after a successful transfer the cleared fields are still verified, and a later failure never presents the stored catalog credential as a rollback.", 6: "Port structure: no lazy runtime instances exist, so the fallback branch cannot occur — the requested agent A resolves or the request fails explicitly."}},
    "R00-T02-LA-38667593C3E1": {"tests": {0: [(READ, "current_status_list_get_and_readonly")], 1: [(READ, "current_status_list_get_and_readonly")], 2: [(READ, "current_status_list_get_and_readonly")], 3: [(READ, "current_status_list_get_and_readonly")]},
        "meaning": {0: "The list payload equals per-key get payloads for every key (same names, same values).", 1: "Model/session values come from the live focus config and real session-file/subagent registries.", 2: "Unknown values surface as explicit null/empty, never fabricated.", 3: "Unknown key 400 with a visible error; unreadable model config yields explicit null instead of a guessed provider result."}},
    "R00-T02-LA-A5DE44FF584F": {"tests": {0: [(READ, "experiments_read_and_write")], 1: [(READ, "experiments_read_and_write")]},
        "meaning": {0: "GET /experiments returns the four registered experiments with their effective values (retired -> off, defaults applied).", 1: "Reads never persist; errors are user-visible."}},
    "R00-T02-LA-C99A4322D0C1": {"tests": {0: [(READ, "experiments_read_and_write")], 1: [(READ, "experiments_read_and_write")]},
        "meaning": {0: "PATCH saves a registered experiment's value (enum/boolean normalized) into preferences.experiments and returns the effective value.", 1: "Unregistered id 400 and invalid values reject with zero persistence (preference bytes compared)."}},
    "R00-T02-LA-AD0384FBE1F2": {"tests": {0: [(READ, "cache_snapshot_observation_read")], 1: [(READ, "cache_snapshot_observation_read")]},
        "meaning": {0: "GET returns the agent's normalized observation from agents/<id>/experiments/cache-snapshot-reflection/latest.json; a missing file yields observation:null without 404.", 1: "Missing/escaping agentId 400, corrupt/array bodies 400 with visible errors; the read leaves the file bytes unchanged."}},
    # ── B5 观察删除 ──
    "R00-T02-LA-3C8CE915255E": {"tests": {0: [(READ, "cache_snapshot_observation_delete")], 1: [(READ, "cache_snapshot_observation_delete")]},
        "meaning": {0: "DELETE unlinks the real latest.json under the settings.write gate and returns {ok:true, deleted:true}; ENOENT returns deleted:false as success semantics.", 1: "Missing/escaping agentId 400, unauthenticated 401; the read face reports null after deletion and repeated deletes are idempotent."}},
    # ── F 块 ──
    "R00-T02-LA-337738D5D233": {"tests": {0: [(SKILL, "workspace_skill_dir_delete_only_allows_known_workspace_dirs_then_resyncs")], 1: [(SKILL, "workspace_skill_dir_delete_rejections_match_actual_side_effects")]},
        "meaning": {0: "Delete only accepts directories inside the known workspace skill roots; a successful recursive delete re-syncs the directory listing.", 1: "Rejections (unknown/escaping targets, missing credentials) leave the skill tree and disk state byte-identical."}},
    "R00-T02-LA-FAE2BDEFDB04": {"tests": {0: [(SKILL, "export_builds_downloadable_zip_and_result_metadata")], 1: [(SKILL, "export_rejections_are_explicit_without_expanding_writes")]},
        "meaning": {0: "Export builds a real store-mode zip (unzip-verified) containing skills/<name>/ plus bundle.json metadata and returns the result metadata with the unique output name.", 1: "Missing bundle/skill entries, corrupt bundle storage and empty exportable sets reject explicitly; a rejected export never rewrites the bundle store or expands writes."}},
}

review = []
report = HERE / "gap-blocks-batch2-proof-review.json"
ready_ids = []
partial_ids = []
for identifier, spec_row in BATCH.items():
    row = next(row for row in inventory["items"] if row["id"] == identifier)
    assert row["status"] == "GAP", identifier
    original = {key: row[key] for key in ["then", "assertions", "groups", "gaps"]}
    assert set(spec_row["tests"]) == set(range(len(row["assertions"]))), identifier
    groups = []
    for index in sorted(spec_row["tests"]):
        tests = [{"package": S, "suite": suite, "test": test} for suite, test in spec_row["tests"][index]]
        locations = [anchors.test_location(suite, test) for suite, test in spec_row["tests"][index]]
        missing = spec_row.get("partial", {}).get(index, [])
        groups.append({
            "index": index,
            "tests": tests,
            "proofLocations": locations,
            "coverage": "PARTIAL" if missing else "FULL",
            "missingClauses": missing,
            "meaning": spec_row["meaning"][index],
        })
    row["groups"] = groups
    if spec_row.get("partial"):
        partial_ids.append(identifier)
        row["gaps"] = original["gaps"]
    else:
        assert all(group["coverage"] == "FULL" for group in groups), identifier
        row.update(status="READY", gaps=[],
                   reviewSource=report.relative_to(anchors.ROOT).as_posix())
        ready_ids.append(identifier)
    assert row["then"] == original["then"] and row["assertions"] == original["assertions"], identifier
    review.append({"id": identifier, "original": original, "became": row["status"]})

anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
report.write_text(json.dumps({
    "status": "GAP_BLOCKS_BATCH2_BOUND_NOT_STAGE_ACCEPTANCE",
    "ready": ready_ids,
    "partial": partial_ids,
    "review": review,
    "stageAcceptance": False,
}, ensure_ascii=False, indent=2) + "\n")
ids = HERE / "reviewed-original-ids.json"
selected = json.loads(ids.read_text())
assert not set(ready_ids).intersection(selected)
selected.extend(ready_ids)
ids.write_text(json.dumps(selected, indent=2) + "\n")
(HERE / "registered-ready-gap-blocks-batch2-ids.json").write_text(
    json.dumps(ready_ids, indent=2) + "\n")
ready_total = sum(1 for row in inventory["items"] if row["status"] == "READY")
print(f"{len(ready_ids)} READY registered this batch ({ready_total} total); "
      f"{len(partial_ids)} partial; full R06/R07 not accepted.")
