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
entries = {
    "3CBCAE9AD914": ("r06_resource_tools_completion", ["current_file_copy_extract_stat_and_real_permission_failures_preserve_targets", "current_file_extract_office_all_available_sheets_source_and_explicit_truncation"], "正式模型file stat/copy/extract经现役网关权限与当前工作目录；复制内容逐字相同、真正登记产物/媒体，默认不覆盖、显式rename/overwrite；目标越权/缺源/目录提取/坏策略/只读复制拒绝且旧目标不误覆盖。真实Word/多表Excel与UTF8文本提取带原路径/真实SHA，字符/表数截断明确，坏Office失败不改源。当前会话拥有的fileId实际消费，另一会话先被原消息引用存储拒绝500，无外部内容泄漏；不修改原归属检查。"),
    "AD7FAE6288FA": ("r06_resource_tools_completion", ["current_file_copy_extract_stat_and_real_permission_failures_preserve_targets", "current_file_extract_office_all_available_sheets_source_and_explicit_truncation", "current_public_url_materialization_uses_real_bytes_and_no_cross_session_grant"], "正式只读materialize经当前真实Run/会话/主体解析本地或已拥有附件真实绝对路径，源不改。URL实际公网HTTPS完整响应才暂存，磁盘SHA与原响应一致，下一真实模型file.extract消费；逐跳公有IP/DNS固定/限跳10秒8MiB，无私网绕过。暂存仅当前主体/会话/精确文件版本可读，外会话/替换版本拒绝。缺目标/不可读/越权显式失败无假路径；UrlProvider缺席的原独立内核仍unsupported，正式CLI明确配置可用提供者。"),
    "8E2D44B48905": ("r06_session_map_browser_completion", ["current_map_seven_groups_real_storage_models_and_receipts"], "原七组实际生产地图+真实CLI/HTTP/WS：布局/焦点展开/换连接隔离，真实拖动/镜头/折叠/重置与写失败乐观显示按原约定保留，坏JSON保持原200但显式degraded/readError并warn，16并发写不丢。当前修订缓存零GET，实际外部新回合切回重读。加号与Inspector创建草稿，continue原SID、branch真实fork新SID发送；真实供应商忙态入原队列零多prompt，真实WS已发但15秒unknown保原文，恢复accepted/terminal后零重发。默认pair/思考在新fork前冻结，权限/记忆/项目/元数据继承，原唯一SQL插入保留proactive策略。19真实窗口检查、12Run/36HTTP请求/36reported账本逐项核对。"),
    "FBDEEDAC0B11": ("r06_session_map_browser_completion", ["current_browser_host_restores_target_preserves_old_view_and_reports_disconnect"], "正式setupBrowserCommands本机Bearer宿主WS→真实Rust owned switch→原handleBrowserCommand resume→真实Electron WebContentsView B挂回，A原页面JS仍活、恢复不弹窗；16有界队列/相关ID/5秒回执。无目标found=false，外来宿主403/重复409、缺会话404/越权403零偏好改动，真实TCP断开502不得假成功，原自动重连后恢复；无宿主明确skipped，不把unsupported当恢复完成。断回执时焦点可能已提交，错误不虚称整体回滚。"),
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
    row.update(status="READY", gaps=[], reviewSource=(HERE / "final-four-proof-review.json").relative_to(ROOT).as_posix())
assert {r["id"]: (r["assertions"], r["then"]) for r in inventory["items"]} == originals
a.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
ids = [rows[suffix]["id"] for suffix in entries]
(HERE / "final-four-leaf-ids.json").write_text(json.dumps(ids, indent=2) + "\n")
p = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
reviewed = json.loads(p.read_text())
for leaf in ids:
    if leaf not in reviewed:
        reviewed.append(leaf)
p.write_text(json.dumps(reviewed, indent=2) + "\n")
print("剩余文件/暂存/地图/浏览器四叶已登记，实际当前候选汇总通过后才计数")
