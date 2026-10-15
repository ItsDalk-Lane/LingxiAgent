"""真实设置页、删除收据和自动记忆宿主逐原断言登记；原文与阶段规则不变。"""
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
spec = importlib.util.spec_from_file_location("anchors", HERE.parent / "continuation-20261013" / "review-proof-anchors.py")
anchors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(anchors)
inventory = json.loads(anchors.MAP.read_text())
rows = {row["id"]: row for row in inventory["items"]}
ids = ["R00-T02-LA-206785535A40", "R00-T02-LA-3516A7326DD9", "R00-T02-LA-0B483D7F6A43"]
original = {leaf: (list(rows[leaf]["assertions"]), rows[leaf]["then"]) for leaf in ids}
suite = "r06_agent_settings_live"
workflow = "native_settings_nineteen_actions_actual_electron_and_service"
failure = "native_settings_current_electron_real_io_failures_and_partial_writes"
reads = "native_settings_reads_keep_real_failures_and_paid_dream_consumption"
roundtrip = "native_character_card_zip_roundtrip_real_materials_and_memory_choice"
card_owner = "native_character_card_owner_two_real_services_and_delayed_import_response"
remaining = "native_remaining_tenet_health_and_order_real_failures_and_pointer_cancel"
owner_suite = "r06_settings_owner_current_source"
owner = "current_settings_owner_switch_cannot_edit_other_real_service_before_identity_ready"

def bind(row, index, proofs, meaning):
    existing = next((group for group in row["groups"] if group["index"] == index), None)
    if existing is None:
        existing = {"index": index, "tests": [], "proofLocations": []}
        row["groups"].append(existing)
    for proof_suite, test in proofs:
        reference = {"package": "lingxi-service", "suite": proof_suite, "test": test}
        if reference not in existing["tests"]:
            existing["tests"].append(reference)
            existing["proofLocations"].append(anchors.test_location(proof_suite, test))
    existing.update(coverage="FULL", missingClauses=[], meaning=meaning)

settings = rows[ids[0]]
specific = {
    0: ["native_agent_settings_current_electron_entry_smoke", "native_initialization_cache_summary_and_real_fifteen_second_timeout", "native_settings_current_electron_actual_read_permission_denied_is_error"],
    1: [roundtrip, card_owner, "native_character_card_real_zip_refuses_escape_links_and_expansion_without_agents", "native_character_card_duplicate_skill_source_and_sparse_path_refuse_without_public_mutation", "native_character_card_application_and_http_delete_never_recreate_deleted_directory"],
    2: [failure], 3: [failure], 4: [failure], 5: [failure],
    6: [roundtrip, card_owner], 7: [], 8: [], 9: [], 10: [roundtrip],
    11: [reads, failure], 12: [reads, failure], 13: [failure],
    14: [remaining], 15: [failure, reads], 16: [reads], 17: [remaining, reads], 18: [remaining],
}
details = {
    0: "真实冷启动、同归属缓存重开、供应商慢/实际500不阻ready、真实15秒只结束遮罩不假装资料加载，403明确错误。",
    1: "实际浏览与六项读取、空名称拒绝、创建/切换分别回执、真实ZIP规划/导入、图片/记忆选择与取消；路径/链接/稀疏文件/重复技能/容量拒绝，导入与删除共享真实应用门。",
    2: "真实改名/主助手/主模型写盘与回读；新连接identity等待期间旧名称不可编辑、B零PUT、两源名称保留。",
    3: "真实Yuan/身份/AGENTS独立写入，真正一项200另一项500时分别保留实际结果；无改动不写。",
    4: "有辅助模型的真实启停、保存失败及写成功后读取失败；真实无辅助模型禁用且零PUT。",
    5: "实际经验启停/列表编辑删除/读取；保留原乐观写入失败语义，不虚报自动回滚。",
    6: "真实人物卡预览、图片与ZIP导出、记忆显式选择、现存文件reveal调用；跨连接与往返后的旧规划无效。",
    7: "实际任选工具配置与页面投影；目录回退/空目录和未初始化沿当前组件合同。",
    8: "实际注册beautify工具的启停配置、磁盘与页面投影；不冒称工具业务执行。",
    9: "实际注册office工具的启停配置、磁盘与页面投影；不冒称文档业务执行。",
    10: "真实选择图片取消零上传、浏览器图片解码/Canvas裁剪编码、保存PNG及GET显示。",
    11: "真实编译记忆各段读写/清理、源事实保留；真实读取失败保留错误而非空成功。",
    12: "真实查看/清空全部记忆，取消零DELETE，确认后真实事实为空，失败不造清理成功。",
    13: "真实模型读取、选择与显式null清空；真实写失败回滚选择器、写盘未变，遵守原无额外config GET链。",
    14: "真实原则各动作；POST200已持久后仅刷新GET真I/O失败，旧列表与内联错误保留，修复读取后确认新条目，未谎报回滚。",
    15: "真实状态与自动开关；先真实自动五阶段成功，再用户单独POST202/不同manual runId五阶段，两运行供应商请求与逐项23/8费用一一对应。",
    16: "真实修订列表/详情/确认恢复/完成提示及后续读取；下一Run消费实际修订。",
    17: "真实健康状态及明确daily未实现降级；带旧状态撤销前置后清除且零GET，真读取失败unavailable，撤销前置再清错误。",
    18: "原生鼠标真实排序；可信pointercancel零PUT、磁盘顺序不变，真实order PUT500后GET恢复原顺序。",
}
assert len(settings["assertions"]) == 19
for index in range(19):
    proofs = [(suite, workflow), *[(suite, test) for test in specific[index]]]
    if index in (0, 1, 2, 3, 4, 6):
        proofs.append((owner_suite, owner))
    bind(settings, index, proofs, details[index] + " 当前macOS实际Electron生产设置组件直连Rust；其他平台/Tauri/正式打包和阶段未宣称通过。")

delete = rows[ids[1]]
assert len(delete["assertions"]) == 3
for index, meaning in enumerate([
    "原完整删除助手、独占技能/全部引用与真实UI数量；待清理收据可继续，重试后实际完整删除。",
    "候选变化not_orphan、实际removal_failed与引用失败逐项读回，页面明确未完成而不伪装全部成功；失败读取保留待办。",
    "最后助手等拒绝保留助手/技能，实际UI失败；跨连接及旧确认均不向别的服务发删除。",
]):
    bind(delete, index, [
        ("r06_agent_delete_skills_live", "agent_delete_orphan_skills_current_desktop_rust_api_and_ui"),
        ("r06_agent_delete_skills_live", "agent_delete_connection_changes_preserve_both_real_service_roots"),
        (suite, "native_pending_reference_deletion_current_electron_shows_and_retries_truthfully"),
        (suite, "native_pending_receipt_owner_switch_two_real_services_never_posts_to_other_service"),
        (owner_suite, owner),
    ], meaning)

ticker = rows[ids[2]]
assert len(ticker["assertions"]) == 2
bind(ticker, 0, [("r06_memory_ticker_completion", test) for test in [
    "periodic_timer_default_auto_off_does_not_create_dream_or_call_models",
    "toggling_memory_really_pauses_then_resumes_timer_and_preserves_paid_cancel",
    "empty_attempt_date_boundary_restart_and_manual_success_are_real_eligibility",
    "busy_manual_does_not_spend_automatic_day_and_cancel_releases_actual_lease",
    "concurrent_real_config_put_cannot_be_overwritten_by_old_tick_publication",
    "paid_terminal_feedback_survives_restart_and_old_epoch_cannot_unpause_host",
]] + [(suite, workflow)], "真实周期宿主默认auto关闭；配置关闭写disabledSince且停止，重开写reenableAt并恢复；真实时钟/逻辑日/忙态/取消付费收尾/重启与旧epoch防回写；现役页面真实启停和独立手动运行，不拿后台模拟页面。")
bind(ticker, 1, [("r06_memory_ticker_completion", "mixed_patch_real_late_io_failure_preserves_committed_global_without_fake_rollback"), (suite, failure)], "混合patch前段全局真实落盘、后段助手真I/O失败，分别读回已发生效果，不虚报事务回滚；页面也呈现写成功后读取失败及部分写入。")

for leaf in ids:
    row = rows[leaf]
    row["groups"].sort(key=lambda group: group["index"])
    assert [group["index"] for group in row["groups"]] == list(range(len(row["assertions"])))
    assert (row["assertions"], row["then"]) == original[leaf]
    row.update(status="READY", gaps=[], reviewSource=(HERE / "settings-ticker-proof-review.json").relative_to(ROOT).as_posix())
anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
(HERE / "settings-ticker-leaf-ids.json").write_text(json.dumps(ids, indent=2) + "\n")
(HERE / "settings-ticker-proof-review.json").write_text(json.dumps({
    "status": "CURRENT_SOURCE_NATIVE_REVIEWED_WAITING_BINDING_WINDOW", "stageAccepted": False, "leaves": ids,
    "reviews": ["settings-r1-independent-original-matrix.md", "settings-r2-author-report.md", "settings-ticker-root-review.json"],
    "scope": "原19+3+2组全保留；Root合并后92检查、当前源独立S5窗口、原删除21与页面50单元通过。仅macOS当前Electron与Rust实际链；全阶段及其他平台未验收。",
}, ensure_ascii=False, indent=2) + "\n")
path = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
reviewed = json.loads(path.read_text())
for leaf in ids:
    if leaf not in reviewed:
        reviewed.append(leaf)
path.write_text(json.dumps(reviewed, indent=2) + "\n")
print("设置/删除/自动宿主三叶逐原断言登记，等待实际绑定窗口；阶段仍未验收。")
