"""Bind the C2 leaves (session history content faces) to their real test suite.

All 8 implemented C2 leaves flip GAP->READY against
tests/r06_session_history_content.rs (production-router HTTP proofs plus the
pub collab-tool face). The 2 suspended C2 leaves (clear-history, sidecar)
stay GAP with recorded rationale. Registration is not stage acceptance.
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
SUITE = "r06_session_history_content"
FIND = "find_route_reports_match_positions_and_strips_model_envelopes"
FIND_BRIDGE = "find_bridge_sanitizer_strips_time_tags_only"
REPLAY = "latest_user_message_replay_replays_by_version_with_optional_replacement"
REPLAY_BUSY = "latest_user_message_replay_returns_409_while_run_in_flight"
OVERVIEW = "history_overview_reports_counts_and_unready_availability"
CONTENT = "session_content_serves_only_history_referenced_resources"
SUMMARY = "session_summary_returns_rolling_record_and_blank_is_not_a_summary"
TURNS = "conversation_map_turns_folds_question_answer_cards"
COMPACT_503 = "fresh_compact_refuses_503_when_compaction_service_not_wired"
COMPACT_REAL = "fresh_compact_compacts_and_rebuilds_prompt_through_real_service"
COMPACT_BUSY = "fresh_compact_returns_409_while_session_streaming"
COLLAB = "session_collab_tools_scope_drafts_and_delivery_semantics"

BATCH = {
    "R00-T02-LA-4F1F69CCA760": {"tests": {0: [(SUITE, FIND), (SUITE, FIND_BRIDGE)], 1: [(SUITE, FIND)]},
        "meaning": {0: "find returns match positions as displayable ordinals over the real session history (exact match ranks before token hits), strips model-side envelope blocks from searchable text and strips Bridge time tags from visible content.", 1: "Unknown session 404, foreign session 403, missing credential 401, empty/oversized query (400 query_too_long) and unknown/duplicate query keys are explicit."}},
    "R00-T02-LA-4DD3F71FA2CD": {"tests": {0: [(SUITE, REPLAY), (SUITE, REPLAY_BUSY)], 1: [(SUITE, REPLAY), (SUITE, REPLAY_BUSY)]},
        "meaning": {0: "Replay re-submits the latest user message by snapshotVersion with optional replacement text (written into turnInputContentJson), and returns 409 session_busy while a run is in flight.", 1: "Non-latest turn 400 session.replay_not_latest, exhausted turn 400 turn_target_invalid, archived 409 session_not_active, invalid version/blank replacement/unknown fields/malformed JSON 400, 404/403/401."}},
    "R00-T02-LA-CDBFE6616EB9": {"tests": {0: [(SUITE, OVERVIEW)], 1: [(SUITE, OVERVIEW)]},
        "meaning": {0: "The overview reports real counts (displayRecords, runsWithAssistant, run size distribution, pagination) with the protocol headers; an unreliable legacy no-head region yields 200 + available:false + revision_unknown instead of an error.", 1: "Unknown/foreign/unauthenticated sessions and malformed requests fail explicitly."}},
    "R00-T02-LA-ED089133C106": {"tests": {0: [(SUITE, CONTENT)], 1: [(SUITE, CONTENT)]},
        "meaning": {0: "content serves the real file bytes only for resource ids that appear in this session's projected history (final-message ResourceRef), cross-session or unreferenced ids 404.", 1: "Unknown id 404, foreign session 403, unknown session 404, missing credential 401."}},
    "R00-T02-LA-F0319FC4F93D": {"tests": {0: [(SUITE, SUMMARY)], 1: [(SUITE, SUMMARY)]},
        "meaning": {0: "The rolling summary record is read back from the TS disk contract (agents/{agentId}/memory/summaries/{sessionId}.json) with hasSummary only for non-blank summaries.", 1: "Missing or corrupt records report hasSummary:false honestly; 404/403/401 explicit."}},
    "R00-T02-LA-F6A1A50E97E4": {"tests": {0: [(SUITE, TURNS)], 1: [(SUITE, TURNS)]},
        "meaning": {0: "turns folds the real history into question/answer cards with entry ids anchored on both the user message and the final message, truncating >8000 chars with truncated:true.", 1: "Empty session yields an empty array; no cross-session leakage; 404/403/401 explicit."}},
    "R00-T02-LA-D62A81C2AEB2": {"tests": {0: [(SUITE, COMPACT_REAL), (SUITE, COMPACT_503)], 1: [(SUITE, COMPACT_503), (SUITE, COMPACT_BUSY)]},
        "meaning": {0: "With the compaction service injected, fresh-compact really compacts the old context (token count drops, exactly one summarize auxiliary call, ledger row) and rebuilds the prompt artifact with a material digest; nothing_to_compact is reported honestly for empty history.", 1: "Unwired compaction service yields an explicit 503 compaction_service_unavailable (never a silent rebuild-only), streaming 409 session_busy, missing context compiler 503, archived 409, 404/403/401."}},
    "R00-T02-LA-5DCB0352EB08": {"tests": {0: [(SUITE, COLLAB)], 1: [(SUITE, COLLAB)], 2: [(SUITE, COLLAB)], 3: [(SUITE, COLLAB)]},
        "meaning": {0: "list/read scope is principal-bounded (local owner sees own-machine sessions; device principals only their owned ones) with query filtering.", 1: "send creates a one-shot confirmation draft and applies exactly one delivery to the named target; self-targeting/foreign/nonexistent targets refuse.", 2: "create keeps the new session id on half success (session created + first delivery failed) and a retry only re-delivers without re-creating.", 3: "Unavailable target, invalid model and partial success are reported separately; unauthorized reads/sends to other principals' sessions are refused with uniform wording."}},
}

review = []
report = HERE / "c2-leaves-batch3-proof-review.json"
ready_ids = []
for identifier, spec_row in BATCH.items():
    row = next(row for row in inventory["items"] if row["id"] == identifier)
    assert row["status"] == "GAP", identifier
    original = {key: row[key] for key in ["then", "assertions", "groups", "gaps"]}
    assert set(spec_row["tests"]) == set(range(len(row["assertions"]))), identifier
    groups = [{
        "index": index,
        "tests": [{"package": S, "suite": SUITE, "test": test} for _s, test in spec_row["tests"][index]],
        "proofLocations": [anchors.test_location(SUITE, test) for _s, test in spec_row["tests"][index]],
        "coverage": "FULL",
        "missingClauses": [],
        "meaning": spec_row["meaning"][index],
    } for index in sorted(spec_row["tests"])]
    row["groups"] = groups
    row.update(status="READY", gaps=[],
               reviewSource=report.relative_to(anchors.ROOT).as_posix())
    ready_ids.append(identifier)
    assert row["then"] == original["then"] and row["assertions"] == original["assertions"], identifier
    review.append({"id": identifier, "original": original, "became": row["status"]})

anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
report.write_text(json.dumps({
    "status": "C2_LEAVES_BOUND_NOT_STAGE_ACCEPTANCE",
    "ready": ready_ids,
    "suspendedWithRationale": {
        "R00-T02-LA-3FCB259FF69C": "TS 自身拒绝 desktop kind 的 /reset 清历史（session-ops.ts:39），映射到桌面会话删除即造语义；语义宿主未移植",
        "R00-T02-LA-FF4F06011B48": "Rust 存储无会话级 promptSnapshot/toolNames sidecar（冻结面为 per-run model_context_snapshot），sidecar 写入面未移植前读面无真实数据源",
    },
    "review": review,
    "stageAcceptance": False,
}, ensure_ascii=False, indent=2) + "\n")
ids = HERE / "reviewed-original-ids.json"
selected = json.loads(ids.read_text())
assert not set(ready_ids).intersection(selected)
selected.extend(ready_ids)
ids.write_text(json.dumps(selected, indent=2) + "\n")
(HERE / "registered-ready-c2-batch3-ids.json").write_text(json.dumps(ready_ids, indent=2) + "\n")
ready_total = sum(1 for row in inventory["items"] if row["status"] == "READY")
print(f"{len(ready_ids)} READY registered this batch ({ready_total} total); full R06/R07 not accepted.")
