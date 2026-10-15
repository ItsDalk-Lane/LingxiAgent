"""将会话、交互与原生目录六叶绑定到当前真实检查，不改原验收文字。"""
import importlib.util
import json
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
spec = importlib.util.spec_from_file_location("anchors", HERE.parent / "continuation-20261013" / "review-proof-anchors.py")
a = importlib.util.module_from_spec(spec)
spec.loader.exec_module(a)
inventory = json.loads(a.MAP.read_text())
baseline = json.loads(subprocess.check_output(["git", "show", "6d19332793c0819f5219a4ed42fcc8f0d7f813a4:scripts/rust-tauri/r06_original_leaf_bindings.json"], cwd=ROOT))
originals = {r["id"]: (r["assertions"], r["then"]) for r in baseline["items"]}
assert len(originals) == 279
assert {r["id"]: (r["assertions"], r["then"]) for r in inventory["items"]} == originals
rows = {r["id"].rsplit("-", 1)[-1]: r for r in inventory["items"]}
focus = "focused_creation_consumes_pinned_model_workspace_and_native_input_drafts"
history = "original_fixed_ui_types_refuse_missing_run_foreign_owner_and_missing_user_anchor_before_model"
transaction = "original_tool_ui_transaction_refuses_stale_attempt_generation_owner_and_terminal_before_write"
native = "original_interactive_tools_current_tauri_components_real_http_and_iframe_sandbox"
entries = {
    "D901852E1B1A": ("r06_session_focus_completion", [focus], "真实生产InputArea/Tiptap按稳定sessionId保存草稿，受信JSONL路径和native定位器先解析SID；重开恢复原稿、双服务同SID旧防抖零错误PUT，拒绝不改已有记录；换连接原子清理旧侧聊和path override。"),
    "B9DA1A584C6E": ("r06_session_focus_completion", [focus], "真实当前CLI/HTTP按助手、工作目录、记忆、模型和思考等级创建焦点会话，统一SQL元数据与模型选择、偏好实际持久化；三个用户Run的read/ls/exec分别真消费所选目录，每Run精确3请求和3账本。成功后所属WS收到相同session_created，外主体零帧；无效输入/权限/真实偏好落盘失败明确拒绝，撤回新会话初始化，不冒称此前目录历史已回滚。"),
    "94CB746D04B7": ("r06_interactive_completion", ["original_batched_question_real_answers_timeout_dismiss_abort_and_refusals", native, history, transaction, "original_question_real_run_cancellation_clears_pending_without_fabricating_answers"], "正式ask_user经生产网关真实调用，当前原生卡片问题/选项可见并按key稳定value/label提交；超时只选推荐项并标timedOut/autoSelected，无推荐仍unanswered；拒答/中止空答案，Run取消清卡，晚答拒绝。无效题目/无会话/不可用明确错误；真实身份/attempt/generation及历史篡改负控不产生业务记录。"),
    "B34F2B298BFA": ("r06_interactive_completion", ["original_full_todo_list_persists_all_states_emits_full_stream_and_invalid_preserves", native, history, transaction], "正式todo_write完整替换清单，持久activeForm/blockedReason与当前原生清单一致，服务重启恢复；真实状态和blocked理由校验先于写，错误保留旧清单；记录/完整事件同事务且绑定真实Run和attempt，旧历史及事务身份负控拒绝。"),
    "EE2ACB390FCE": ("r06_interactive_completion", ["original_card_persisted_session_result_and_real_review_refusal_without_source_write", native, history, transaction], "真实show_card在会话结果持久cardId/title/code并可重开，复用真实PreviewService沙箱；当前Tauri真iframe脚本执行，opaque origin null、无Node、读parent.document真实SecurityError且宿主不变，按钮执行计数。源文件无写；沿现行Execute权限审查，拒绝零记录/iframe；服务重启读回同卡。"),
    "52A5D1660088": ("r06_directory_picker", ["original_system_directory_dialog_real_path_cancel_and_no_install"], "当前生产Tauri系统pick_folder真实返回所选绝对路径到技能目录输入，取消为null并保留原选值；选择只提供后续查看/配置路径、原SKILL.md字节不变且无安装目录。仅主窗口可开选择器，真实只读技能窗口命令被拒绝。实际系统对话框由CUA选择和取消，不替代为模拟回包。"),
}
for suffix, (suite, names, meaning) in entries.items():
    row = rows[suffix]
    for index in range(len(row["assertions"])):
        group = next((g for g in row["groups"] if g["index"] == index), None)
        if group is None:
            group = {"index": index, "tests": [], "proofLocations": []}
            row["groups"].append(group)
        for name in names:
            ref = {"package": "lingxi-service", "suite": suite, "test": name}
            if ref not in group["tests"]:
                group["tests"].append(ref)
                group["proofLocations"].append(a.test_location(suite, name))
        group.update(coverage="FULL", missingClauses=[], meaning=meaning + " 当前macOS合成资料实际运行；不声称外部付费供应商、其他系统或全阶段验收。")
    row["groups"].sort(key=lambda g: g["index"])
    row.update(status="READY", gaps=[], reviewSource=(HERE / "interaction-focus-directory-proof-review.json").relative_to(ROOT).as_posix())
assert {r["id"]: (r["assertions"], r["then"]) for r in inventory["items"]} == originals
a.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
ids = [rows[suffix]["id"] for suffix in entries]
(HERE / "interaction-focus-directory-leaf-ids.json").write_text(json.dumps(ids, indent=2) + "\n")
p = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
reviewed = json.loads(p.read_text())
for leaf in ids:
    if leaf not in reviewed:
        reviewed.append(leaf)
p.write_text(json.dumps(reviewed, indent=2) + "\n")
print("会话、交互与原生目录六叶已登记，实际当前候选汇总通过后才计数")
