"""登记会话四叶、技能七叶的真实生产行为证据；原断言保持不变。"""
import hashlib
import importlib.util
import json
import re
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
session = "r06_session_completion"
skills = "r06_skills_completion"
checkpoint = "production_cli_checkpoint_tools_human_review_deferred_rewind_and_frozen_prompt"
refusals = "registered_checkpoint_tool_refuses_missing_session_and_user_anchor_before_snapshot"
alias = "explicitly_authorized_root_alias_captures_real_files_but_inner_symlink_degrades"
ui = "original_skills_settings_and_panel_current_real_service_projection"
native = "original_native_skill_viewer_current_tauri_window_and_read_boundaries"

def add(suffix, suite, names, meaning):
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
        group.update(coverage="FULL", missingClauses=[], meaning=meaning + " 当前macOS实际运行；外部供应商、其他平台及全阶段验收未宣称通过。")
    row["groups"].sort(key=lambda g: g["index"])
    row.update(status="READY", gaps=[], reviewSource=(HERE / "session-skills-proof-review.json").relative_to(ROOT).as_posix())

add("6207F7E8C697", session, [checkpoint, refusals, alias], "真实CLI模型发现与调用checkpoint create/list/drop；已有Run的可信许可复用，read_only只准list；无会话/无用户锚点/无效名字/缺失drop明确失败且零快照。实际版本表核对包含两份文件；声明的根路径别名规范化，目录内链接仍拒绝且显式degraded。")
add("BB0A168006D7", session, ["user_edit_absolute_checkpoint_reasons_binary_bytes_and_failure_have_no_partial_archive", alias], "真实HTTP只接受绝对文件及edit-start/autosave-interval；二进制快照和恢复逐字节一致；非法原因/相对/缺失/目录/读权限/磁盘失败无残留档案，根别名与内部链接分别验证。")
add("E1625F70C7CC", session, [checkpoint], "真实模型工具必须等用户确认，risk_accepted不可绕过；当前生产React界面拒绝零回退、确认后忙Run只排队，Run结束才按冻结锚点回退并持久回执；实际分支reset、文件v1恢复及外部conflicted不覆盖。A→B→A连接和同服务会话迟到响应不能解除新请求busy或跨会话批准。")
add("FF4F06011B48", session, ["prompt_sidecar_reads_actual_sent_input_survives_restart_and_read_error_is_not_empty", checkpoint], "真实模型发送准备边界的系统提示词和工具名写入专属sidecar，和实际请求逐字核对；实际专属读接口只读该冻结文件，重启保留；跨主体/会话/损坏明确拒绝，不用另一个观察接口替代。")
add("0DDB5AA05072", skills, [ui], "现役SkillsPanel真实HTTP服务完成安装并启用、组合CRUD/成员/排序/导出及助手投影双GET；原界面加载/空态/loading/noUser/取消/错误toast及局部失败语义原断言保留，生产桥边界仅原生选择/显示替代。")
add("771296A194E0", skills, ["original_lesson_review_one_shot_content_binding_conflict_and_real_retrieval"], "真实learn_lesson注册与写入，审核采用明确可控SkillGuardPort；风险拒绝无写、一次性token绑定正文且不可重用，冲突/缺助手/坏名称/写根错误拒绝；真实ExperienceStore正文/索引/助手启用和运行时读回一致，启用失败撤销本次新目录。")
add("7803B231FC36", skills, ["original_translation_uses_real_title_model_protocol_accounts_and_refuses_cross_catalog"], "真实生产ProviderRegistry/ModelGateway/AuxiliaryExecutor经本地OpenAI SSE供应商，使用现役Title槽并记录quota/调用账本；完整目标名称映射、供应商/解析失败、必填助手、无效names/lang/en及跨助手目录拒绝。")
add("8A69F0CFFFA1", skills, ["original_skill_packages_workspace_source_global_and_agent_install_rejections", ui], "真实目录/zip/.skill/base64完整包安装到获准工作区.agents/skills，真实installedSkillSource和刷新；全局安装不自动启用，指定助手安装启用。越界压缩条目/链接/重复/坏包/无写权限/跨工作区明确错误，无伪持久化。")
add("99F8CD228733", skills, [ui, native], "现役SkillsTab及原初始化/缓存/15秒/摘要不阻塞/能力确认/偏好GET和PUT回滚/查看器边界等32原界面断言保留；新增真实HTTP贯穿安装、组合、外部目录保存重载、翻译、启停、导出和goal/autolearn实际磁盘读回15项。外部目录PUT后重载失败传toast；独立Tauri查看器真实读取且不安装。原断言明确的console或静默未加载行为没有扩大承诺。")
add("9CA8A47BE208", skills, [native], "当前Tauri真实独立窗口和真实WebView，树点击显示所选中文正文；非绝对/非目录/超2MiB/二进制空结果，绑定每窗根且拒绝另一根/写命令，原文件不变不安装。另在真实生产reader打开前替换外部链接，实际失败反例后修复并重跑：已打开文件身份/路径前后复核拒绝替换，外部字节保留。")
add("BA490D930CA9", skills, [ui], "真实重载重新扫描并广播skills-changed，现役界面按agentId重新GET；不回无助手视角列表。持有现役mutation_lock时，三个独立线程reload/install/delete真实等待，释放才结束；没有仅靠源码字符串证明锁。")

# 复用现行登记器已支持的库内具名检查，保留此前集成检查及定位。
row = rows["BA490D930CA9"]
name = "skills::completion::tests::original_reload_install_delete_wait_for_the_same_mutation_lock"
path = Path("rust/crates/lingxi-service/src/skills_completion.rs")
raw = (ROOT / path).read_text()
declaration = re.search(r"\bfn\s+" + name.rsplit("::", 1)[-1] + r"\s*\(", raw)
assert declaration
depth = 0
opened = False
for token in a.tokens(raw[declaration.end():]):
    if token.group() == "{":
        opened = True
        depth += 1
    elif token.group() == "}":
        depth -= 1
        if opened and depth == 0:
            end = declaration.end() + token.end()
            break
anchor = {"path": path.as_posix(), "test": name, "sourceSHA256": hashlib.sha256((ROOT / path).read_bytes()).hexdigest(), "testStartLine": raw[:declaration.start()].count("\n") + 1, "testEndLine": raw[:end].count("\n") + 1, "assertions": a.assertions_in_body(raw, raw[declaration.start():end], declaration)}
assert anchor["assertions"]
ref = {"kind": "rust_library_test", "package": "lingxi-service", "test": name, "sourcePath": path.as_posix()}
for group in row["groups"]:
    if ref not in group["tests"]:
        group["tests"].append(ref)
        group["proofLocations"].append(anchor)

assert {r["id"]: (r["assertions"], r["then"]) for r in inventory["items"]} == originals
a.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
suffixes = ["6207F7E8C697", "BB0A168006D7", "E1625F70C7CC", "FF4F06011B48", "0DDB5AA05072", "771296A194E0", "7803B231FC36", "8A69F0CFFFA1", "99F8CD228733", "9CA8A47BE208", "BA490D930CA9"]
ids = [rows[suffix]["id"] for suffix in suffixes]
(HERE / "session-skills-leaf-ids.json").write_text(json.dumps(ids, indent=2) + "\n")
p = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
reviewed = json.loads(p.read_text())
for leaf in ids:
    if leaf not in reviewed:
        reviewed.append(leaf)
p.write_text(json.dumps(reviewed, indent=2) + "\n")
print("会话四叶及技能七叶原断言登记；实际当前候选绑定通过后才计入完成")
