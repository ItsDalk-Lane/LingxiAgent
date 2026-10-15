"""登记当前实验两叶及焦点广播回归，原验收文字不变。"""
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
    "D8D47C4680C9": ("r06_experiments_completion", ["current_experiment_persistence_events_observation_and_real_system_permissions", "original_settings_container_experiments_computer_components_and_current_live_http", "original_experiments_current_tauri_window_real_service"], "当前生产设置容器/实验页/电脑权限组件真实Rust HTTP和Tauri窗口核对冷启动、缓存、15秒就绪与错误；四实验目录/快照同份偏好，保存先落盘后广播，失败零广播/无假成功；观察记录真实读删与失败。cache snapshot当前已退役，遵守未注册无卡片，不复活原卡；原shadow条件分支保留并由原组件检查。系统权限实际查询和按钮请求返回当前布尔与待请求，既有批准只展示；写后GET失败回退快照不误当写失败；控制引擎当前明确不可用，不宣称已实现电脑控制。"),
    "980C099F356F": ("r06_experiments_completion", ["new_main_session_policy_is_frozen_durable_and_never_grants_tools", "actual_cli_focused_creation_and_frozen_hint_reach_wire_model_after_toggle_off"], "默认关闭，所有新会话创建入口在SQL插入时冻结策略；真实focused/detached/fork会话配置进入实际HTTP模型提示，关闭后旧会话保留原创建值，子代理不注入；真实subagent写请求仍拒绝且零子任务，权限列和授权目录不变。旧数据库默认false，重开持久一致，9请求/9账本。"),
    "B9DA1A584C6E": ("r06_session_focus_completion", ["current_rust_session_created_reaches_production_renderer_projection"], "已完成焦点创建的生产renderer广播回归补充：真实CLI发出session_created，现役WS白名单实际分发，正确会话SID进入可见列表与locator；原负控真实收到帧但未消费，补一个类型token后消费成功。保留该叶所有先前创建/模型/目录/回滚证明。"),
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
    row.update(status="READY", gaps=[], reviewSource=(HERE / "experiments-proof-review.json").relative_to(ROOT).as_posix())
assert {r["id"]: (r["assertions"], r["then"]) for r in inventory["items"]} == originals
a.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
ids = [rows[suffix]["id"] for suffix in entries]
(HERE / "experiments-leaf-ids.json").write_text(json.dumps(ids, indent=2) + "\n")
p = HERE.parent / "continuation-20261013" / "reviewed-original-ids.json"
reviewed = json.loads(p.read_text())
for leaf in ids:
    if leaf not in reviewed:
        reviewed.append(leaf)
p.write_text(json.dumps(reviewed, indent=2) + "\n")
print("实验两叶及已完成焦点广播回归已登记，实际当前候选汇总通过后才计数")
