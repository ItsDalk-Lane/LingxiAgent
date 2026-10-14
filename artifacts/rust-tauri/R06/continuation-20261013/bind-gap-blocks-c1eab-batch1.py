"""Bind GAP_DISPATCH_PLAN C1/E/A/B batch-1 leaves to the 2026-10-13 suites.

30 leaves flip GAP->READY (every original assertion covered FULL); 3 leaves
(FBDEEDAC0B11 browser resume, 485B2D0A8A02 busy-rotate, 24BC2E961E3E layout
controls) register PARTIAL groups with explicit missingClauses and stay GAP.
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
FOLDERS = "r06_session_authorized_folders"
FOCUS1 = "focus_switch_detached_create_and_project_assignment"
FOCUS2 = "rotate_continue_deleted_agent_and_draft_cleanup"
CATALOG = "project_folder_catalog_roundtrip_and_rejections"
LIFE1 = "agent_lifecycle_roundtrip_and_rejections"
LIFE2 = "delete_agent_cascades_orphan_skills"
PREFS1 = "user_preference_pairs_and_tenets_broadcast"
PREFS2 = "experience_search_and_global_config_setter"
F04_DELETE = ("r06_ar_f04_lifecycle", "f04_permanent_delete_removes_owned_cache_and_retains_other_branch")
F04_FAIL = ("r06_ar_f04_lifecycle", "f04_delete_physical_failure_preserves_identity_until_restart_cleanup")
ROLLBACK = ("r06_original_session_proofs", "original_workspace_rollback_preference_persists_without_restoring_files_and_controls_real_gate")

# id -> {"tests": {assertion_index: [(suite, test), ...]}, "meaning": {index: text},
#        "partial": {index: [missing clauses]} or {}}
BATCH = {
    # ── C1 会话授权目录 ──
    "R00-T02-LA-1E0D477755CD": {"tests": {0: [(FOLDERS, "authorized_folders_roundtrip_and_rejections")], 1: [(FOLDERS, "authorized_folders_roundtrip_and_rejections")]},
        "meaning": {0: "Actual PATCH remove drops only the named directory from the owned session scope; absent entry removal is idempotent and the remaining scope stays.", 1: "Nonexistent target, plain file, missing folder, unknown action, unknown session 404, archived session 409 and missing credential 401 all fail explicitly; the pre-failure list is re-read unchanged after every rejection."}},
    "R00-T02-LA-73B631D77531": {"tests": {0: [(FOLDERS, "authorized_folders_roundtrip_and_rejections")], 1: [(FOLDERS, "authorized_folders_roundtrip_and_rejections")]},
        "meaning": {0: "Actual PATCH set replaces the whole scope with the new directory list atomically; a bad entry in the new list keeps the previous list.", 1: "Bad entry set, missing folders set, unknown action, archived session and missing credential reject explicitly without scope drift."}},
    "R00-T02-LA-7C959E75B8C3": {"tests": {0: [(FOLDERS, "authorized_folders_roundtrip_and_rejections")], 1: [(FOLDERS, "authorized_folders_roundtrip_and_rejections")], 2: [(FOLDERS, "authorized_folders_roundtrip_and_rejections")]},
        "meaning": {0: "Approval (add) changes only the owning session scope; a fresh second session reads an empty scope before and after the first session's add.", 1: "After every rejection the scope is re-read and equals the pre-request value.", 2: "Unauthenticated 401, unknown session 404, invalid directory/file/missing-field 400, archived 409, and cross-session isolation (s1 approval never widens s2) are all asserted."}},
    "R00-T02-LA-98DDC94776F1": {"tests": {0: [(FOLDERS, "authorized_folders_roundtrip_and_rejections")], 1: [(FOLDERS, "authorized_folders_roundtrip_and_rejections")]},
        "meaning": {0: "Actual GET returns the owned active session scope plus fixed-null workspace/sandbox projections.", 1: "Unknown session 404, archived session 409 and missing credential 401 are explicit; no success shape on failure."}},
    # ── C1 焦点/独立创建/项目归属 ──
    "R00-T02-LA-FBDEEDAC0B11": {"partial": {0: ["浏览器宿主在线时的真实恢复与旧会话浏览器不挂起行为未证：端口按现役 server/PWA 分支返回 typed skip {status:skipped, reason:no_browser_host}，不猜造浏览器态"]},
        "tests": {0: [("r06_session_focus_detached_project", FOCUS1)], 1: [("r06_session_focus_detached_project", FOCUS1)]},
        "meaning": {0: "Switch persists current_session_id, returns frozen memoryEnabled and scope, and reports the browser-resume port contract as a typed skip.", 1: "Unknown 404, blank id 400, archived 409 with focus unchanged."}},
    "R00-T02-LA-9D9E72CB6051": {"tests": {0: [("r06_session_focus_detached_project", FOCUS1)], 1: [("r06_session_focus_detached_project", FOCUS1)]},
        "meaning": {0: "Detached create keeps the persisted focus untouched, records lineage anchors 1->2, rejects depth overrun with 409 and links the project assignment at creation.", 1: "Unknown lineage source 404, no resolvable agent 400, missing credential 401."}},
    "R00-T02-LA-20E9C8F26CEA": {"tests": {0: [("r06_session_focus_detached_project", FOCUS1)], 1: [("r06_session_focus_detached_project", FOCUS1)]},
        "meaning": {0: "PUT assigns the trimmed project id, null/empty-string clears, GET reads the assignment back on the real sessions.project_id column.", 1: "Unknown session 404, archived session 409 reject without state change."}},
    # ── C1 轮换/续接/永久删除 ──
    "R00-T02-LA-485B2D0A8A02": {"partial": {1: ["前会话运行中（Busy/接管态）时轮换的 409 session_busy 拒绝未在本证明覆盖"]},
        "tests": {0: [("r06_session_focus_detached_project", FOCUS2)], 1: [("r06_session_focus_detached_project", FOCUS2)]},
        "meaning": {0: "Rotate archives the latest active session, hands focus to the minted successor, archives successively across rounds and still creates a fresh session when the agent has no history.", 1: "No resolvable agent 400 and missing credential 401 are explicit; busy-session rejection is registered as a missing clause."}},
    "R00-T02-LA-3818ADEAFA7A": {"tests": {0: [("r06_session_focus_detached_project", FOCUS2)], 1: [("r06_session_focus_detached_project", FOCUS2)]},
        "meaning": {0: "Continue-deleted-agent gates on agent_not_deleted 400, then after the real deletion marker mints a new agent identity with the original display name, inherits the authorized folders and reports compacted=false honestly.", 1: "Unknown session 404 and blank id 400; the pre-gate refusal leaves the session and marker untouched."}},
    "R00-T02-LA-6658E9B80314": {"tests": {0: [("r06_session_focus_detached_project", FOCUS2), F04_DELETE], 1: [("r06_session_focus_detached_project", FOCUS2), F04_FAIL]},
        "meaning": {0: "After archive, real permanent delete removes the owned cached payload files (F04 suite) and clears the input-draft entries on both surfaces (draft file bytes re-read without the session).", 1: "Unknown/blank session and missing credential reject explicitly; a real physical delete failure keeps the durable identity until restart cleanup instead of reporting success."}},
    # ── E 项目/文件夹目录（9）──
    "R00-T02-LA-AFA7227BFB63": {"tests": {0: [("r06_session_project_catalog", CATALOG)], 1: [("r06_session_project_catalog", CATALOG)]},
        "meaning": {0: "GET returns the real folders+projects catalog (empty first, populated later).", 1: "Missing credential 401; malformed requests fail explicitly."}},
    "R00-T02-LA-60A58027CCCE": {"tests": {0: [("r06_session_project_catalog", CATALOG)], 1: [("r06_session_project_catalog", CATALOG)]},
        "meaning": {0: "Folder create folds whitespace-only names and returns the new item.", 1: "Blank name 400."}},
    "R00-T02-LA-E167298FD87C": {"tests": {0: [("r06_session_project_catalog", CATALOG)], 1: [("r06_session_project_catalog", CATALOG)]},
        "meaning": {0: "Project create lands in the named folder or at root (folderId null) and returns the new project.", 1: "Unknown folder 400."}},
    "R00-T02-LA-059066969D44": {"tests": {0: [("r06_session_project_catalog", CATALOG)], 1: [("r06_session_project_catalog", CATALOG)]},
        "meaning": {0: "Project patch renames and moves across folders; the move appends order at the destination tail.", 1: "Unknown project 400."}},
    "R00-T02-LA-D43836A3A579": {"tests": {0: [("r06_session_project_catalog", CATALOG)], 1: [("r06_session_project_catalog", CATALOG)]},
        "meaning": {0: "Project patch returns the updated state (name/folder/order) on the real user/session-projects.json contract store.", 1: "Unknown project id 400 keeps the catalog unchanged."}},
    "R00-T02-LA-EEDFB7AB889C": {"tests": {0: [("r06_session_project_catalog", CATALOG)], 1: [("r06_session_project_catalog", CATALOG)]},
        "meaning": {0: "Folder reorder returns the catalog with the requested global order.", 1: "Malformed reorder bodies fail explicitly."}},
    "R00-T02-LA-D58B651BF099": {"tests": {0: [("r06_session_project_catalog", CATALOG)], 1: [("r06_session_project_catalog", CATALOG)]},
        "meaning": {0: "Project reorder applies inside the folderId scope and returns the catalog.", 1: "Malformed bodies fail explicitly; out-of-scope ids cannot leak into other scopes."}},
    "R00-T02-LA-3ECCF7C95483": {"tests": {0: [("r06_session_project_catalog", CATALOG)], 1: [("r06_session_project_catalog", CATALOG)]},
        "meaning": {0: "Project delete clears the live sessions' assignment (cwd: report, storage NULL, sessionIds listed) and the session content survives.", 1: "Unknown project delete fails explicitly."}},
    "R00-T02-LA-EF5DD65C558E": {"tests": {0: [("r06_session_project_catalog", CATALOG)], 1: [("r06_session_project_catalog", CATALOG)]},
        "meaning": {0: "Folder delete moves its projects to root (folderId null) and reorders; projects and sessions are not deleted.", 1: "Unknown folder delete fails explicitly."}},
    # ── A 助手集合面（6）──
    "R00-T02-LA-26AE3688B8DE": {"tests": {0: [("r06_agent_admin_lifecycle", LIFE1)], 1: [("r06_agent_admin_lifecycle", LIFE1)]},
        "meaning": {0: "List returns ids/names/yuan/memoryEnabled/isPrimary plus homeFolder/effectiveHomeFolder per agent; configured home folders round-trip and unconfigured are explicit null; fresh=1 accepted.", 1: "fresh refresh accepted; unauthenticated 401; deleted-marker agents excluded."}},
    "R00-T02-LA-9CC1B13F1787": {"tests": {0: [("r06_agent_admin_lifecycle", LIFE1)], 1: [("r06_agent_admin_lifecycle", LIFE1)]},
        "meaning": {0: "Create persists agents/<id>/config.yaml, returns ok/id/name and broadcasts agent-created with the name payload.", 1: "Blank name 400, path-shaped id 400, duplicate 409, missing credential 401."}},
    "R00-T02-LA-C0ADBABE5E89": {"tests": {0: [("r06_agent_admin_lifecycle", LIFE1)], 1: [("r06_agent_admin_lifecycle", LIFE1)]},
        "meaning": {0: "PUT primary persists primary_agent_id and the next listing marks exactly that agent isPrimary.", 1: "Unknown id 404, path-shaped id 400."}},
    "R00-T02-LA-8D4356B7EC51": {"tests": {0: [("r06_agent_admin_lifecycle", LIFE1)], 1: [("r06_agent_admin_lifecycle", LIFE1)]},
        "meaning": {0: "PUT order persists the order and the next listing returns ids in that order.", 1: "Duplicate id, unknown id, non-array and missing order all 400."}},
    "R00-T02-LA-8B48FD65CAFB": {"tests": {0: [("r06_agent_admin_lifecycle", LIFE1)], 1: [("r06_agent_admin_lifecycle", LIFE1)]},
        "meaning": {0: "Switch returns the context, persists current_agent_id, writes last_cwd/cwd_history into the agent config (dedup, move-to-tail) and broadcasts agent-switched with agentName/cwd.", 1: "Unknown 404, path-shaped 400, missing id 400; deleted-marker agents refuse switch 404."}},
    "R00-T02-LA-517E697740D9": {"tests": {0: [("r06_agent_admin_lifecycle", LIFE1)], 1: [("r06_agent_admin_lifecycle", LIFE1), ("r06_agent_admin_lifecycle", LIFE2)]},
        "meaning": {0: "Delete removes the directory and registration; deleting the current agent switches to the replacement in order and persists it; the app-event feed shows the replacement agent-switched BEFORE agent-deleted.", 1: "Default deleteSkills returns skillsDeleted=[]/skillsSkipped=[] with directories retained; cascade honors orphan/non-orphan/removal_failed semantics (LIFE2); last-agent delete 400 leaves the directory intact; unknown 404."}},
    # ── B 记忆/偏好（7 READY + 1 PARTIAL）──
    "R00-T02-LA-1685D5E08812": {"tests": {0: [("r06_user_preferences", PREFS1)], 1: [("r06_user_preferences", PREFS1)]},
        "meaning": {0: "PUT autolearn saves the enabled switch (wrapped and flat body forms), returns the new state and merges into preferences.json preserving other keys.", 1: "Non-boolean 400 with the saved value unchanged; missing credential 401."}},
    "R00-T02-LA-22150190C7B1": {"tests": {0: [ROLLBACK], 1: [ROLLBACK]},
        "meaning": {0: "GET workspace-rollback returns the current enabled state (default false) and the read leaves the workspace files untouched.", 1: "The response state always mirrors the persisted/actual switch; failures return explicit errors, never a synthetic success."}},
    "R00-T02-LA-7D05B144AE21": {"tests": {0: [("r06_user_preferences", PREFS2)], 1: [("r06_user_preferences", PREFS2)], 2: [("r06_user_preferences", PREFS2)]},
        "meaning": {0: "Search hits carry the real file path of the CURRENT agent and excerpt.", 1: "File bytes before/after search are equal; the read path performs no memory writes.", 2: "A second enabled agent with no experience gets count=0 for the same keyword (no cross-agent leak); no-match query returns 0; missing q is 400."}},
    "R00-T02-LA-99900EB5ACC9": {"tests": {0: [("r06_user_preferences", PREFS1)], 1: [("r06_user_preferences", PREFS1)]},
        "meaning": {0: "Tenet delete removes the row and the app-event feed shows tenets-changed with agentId+tenetId.", 1: "Deleting a nonexistent tenet id is 404; no cross-agent disclosure."}},
    "R00-T02-LA-B5CB2993A5C0": {"tests": {0: [("r06_user_preferences", PREFS1)], 1: [("r06_user_preferences", PREFS1)]},
        "meaning": {0: "GET quick-chat returns the settings (defaults first) and the preference file bytes are identical across the read.", 1: "PUT accepts wrapped and flat forms; invalid values reject without state drift."}},
    "R00-T02-LA-C3FB70F9925E": {"tests": {0: [("r06_user_preferences", PREFS1)], 1: [("r06_user_preferences", PREFS1)]},
        "meaning": {0: "GET autolearn returns the switch (default enabled=true) and the read does not create or rewrite preferences.json.", 1: "PUT accepts wrapped and flat forms with strict boolean validation; 400 keeps the previous value; 401 unauthenticated."}},
    "R00-T02-LA-D92CD567D6D6": {"tests": {0: [("r06_user_preferences", PREFS1)], 1: [("r06_user_preferences", PREFS1)]},
        "meaning": {0: "PUT quick-chat saves shortcut and reuse timeout (snake_case normalized, clamped to 120) into the quick_chat preferences key, returning the new state.", 1: "Wrapped and flat forms both accepted; invalid input rejects without writing; missing credential 401."}},
    "R00-T02-LA-24BC2E961E3E": {"partial": {0: ["跨助手读取同一全局偏好的读回面未证（A 块配置读回路由未实现）"], 1: ["editor 排版与 chat 布局两处控件请求（前者全局叶、后者助手叶）的双请求语义未证"], 2: ["mixed patch 后段失败时全局 setter 已写入的部分成功读回未证"]},
        "tests": {0: [("r06_user_preferences", PREFS2)], 1: [("r06_user_preferences", PREFS2)], 2: [("r06_user_preferences", PREFS2)]},
        "meaning": {0: "Pure global patch (keep_awake/locale) writes preferences.json only and reports globalApplied; the agent config.yaml is not polluted.", 1: "Global-vs-agent leaf separation is proven for keep_awake vs memory in distinct requests.", 2: "A real mixed patch (memory+keep_awake) applies both sides in one request; type errors reject before any write."}},
}

review = []
report = HERE / "gap-blocks-c1eab-batch1-proof-review.json"
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
        # Partial bindings keep the leaf GAP with groups recorded; no READY flip.
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
    "status": "GAP_BLOCKS_C1EAB_BATCH1_BOUND_NOT_STAGE_ACCEPTANCE",
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
(HERE / "registered-ready-gap-blocks-batch1-ids.json").write_text(
    json.dumps(ready_ids, indent=2) + "\n")
ready_total = sum(1 for row in inventory["items"] if row["status"] == "READY")
print(f"{len(ready_ids)} READY registered this batch ({ready_total} total); "
      f"{len(partial_ids)} partial; full R06/R07 not accepted.")
