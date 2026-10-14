"""Bind R06 closeout round-2 backend-gap leaves to the 2026-10-13/14 R2 suites.

10 leaves flip GAP->READY (every original assertion covered FULL):
38EEA99077E8 (studio-scoped session list projection: sessions.read +
  Studio identity + summary/revision/attribution/running badges),
AA9EB3FD034D (history before/limit/all/reconciliation + todos/revision
  projection + ETag 304 eligibility),
EE18A57F7348 (workbench writeText same-version save / conflict keeps
  external content / explicit save failure / snapshot-retry chain),
464E58DEC9E2 (G6 port-structure branch: explicit agentId=A save writes
  A's YAML and never falls back to focus B — asserted directly),
3A50AD28E08E + A9DAA48DC603 + AF8E24A2F524 (file-history query.agentId
  resolves the per-agent home workspace with inter-agent isolation),
55B24FEE5AFB (attachments/local snapshot mode copies bytes + registers),
485B2D0A8A02 (rotate 409 session_busy while the previous session runs),
604C64F4828A (authorized-folder add expands the checkpoint file scope
  with the rejection matrix).

Leaves inspected and deliberately KEPT GAP this round (groups stay as
registered): 7432B00DEC94 / 76EE734926B4 / 93D5237ADE81 — all-FULL groups
but documented uncovered clauses (semantic retrieval status; ticker-absent
503 vs store-served revisions; Office/vision extraction), plus the
host-attribution classes reported separately.

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

# (suite, test) pairs for the R2 proofs.
SLP = ("r06_session_list_projection", "studio_list_projection_carries_summary_revision_attribution_and_badges")
HIST = ("r06_t04_history", "history_reconciliation_todos_and_revision_projection")
HIST_NEG = ("r06_t04_history", "history_cursor_validation_and_owner_isolation")
WB = ("r06_workbench_write", "workbench_write_text_same_version_conflict_and_retry_chain")
READBACK = ("r06_agent_config_readback", "put_config_inline_credential_transfer_readback")
FH_SCOPE = ("r06_t05_filehistory", "file_history_routes_scope_per_agent_workspace")
FH_CHAIN = ("r06_t05_filehistory", "file_history_routes_full_chain_and_error_vocabulary")
FH_NOTTRACKED = ("r06_t05_filehistory", "file_history_routes_workspace_not_tracked")
ATT = ("r06_t05_session_files", "attachments_local_snapshot_mode_copies_bytes_and_dedupes")
ROTATE_BUSY = ("r06_session_focus_detached_project", "rotate_rejects_busy_previous_session_with_409")
ROTATE = ("r06_session_focus_detached_project", "rotate_continue_deleted_agent_and_draft_cleanup")
FOLD_ADD = ("r06_t03_session_tree", "authorized_folder_add_expands_checkpoint_file_scope")
FOLD_403 = ("r06_t03_session_tree", "checkpoint_file_paths_outside_authorized_folders_are_403")
FOLD_MATRIX = ("r06_session_authorized_folders", "authorized_folders_roundtrip_and_rejections")


def tests(*pairs):
    return [{"package": S, "suite": suite, "test": test} for suite, test in pairs]


def locations(*pairs):
    return [anchors.test_location(suite, test) for suite, test in pairs]


def full_group(index, test_pairs, meaning):
    return {
        "index": index,
        "tests": tests(*test_pairs),
        "proofLocations": locations(*test_pairs),
        "coverage": "FULL",
        "missingClauses": [],
        "meaning": meaning,
    }


# id -> list of rebuilt groups (must cover every assertion index in order).
BATCH = {
    "R00-T02-LA-38EEA99077E8": [
        (0, [SLP],
         "现役 GET /sessions 列表投影的端口面：经 sessions.read scope 与 Studio 身份闸（主体 Studio 与 server Studio 不一致 → 403 studio_scope_mismatch），"
         "本地主人全见 active 会话（含合成种子），每会话返回摘要（title/hasSummary 滚动摘要记录在场）、修订点（r{head}-e{event}，与 /history ETag 同源，写入后推进）、"
         "归属（agentId/agentName/ownerUserId/projectId/parentSessionId）与运行徽章（running + activeRuns；execute 挂起时出现、终态后回落）。"),
        (1, [SLP],
         "拒绝腿真实：scope 不足 → 403 insufficient_scope；Studio 不一致 → 403 studio_scope_mismatch；设备主体（本人无会话）→ 空列表不泄露本地主人会话（can_access 同口径）。"),
    ],
    "R00-T02-LA-AA9EB3FD034D": [
        (0, [HIST],
         "阅读会话消息页的完整入口语义：before（不透明游标，引用不存在 400）、limit（默认 50 钳 200，非法 400）、all=1（全量 complete=true）、reconciliation=1"
         "（前后采样活动运行，页尾附 reconciliation 证明块 runRevision/complete/runStatus——空闲+完整页 reconciled_idle、页不完整 unknown）；"
         "每页返回消息+块投影（items）、待办投影（todos/todoPanel 与面板读同一存储快照）与修订（revision 与 ETag 同源）；ETag 命中回 304，"
         "而 reconciliation 分支按现役 E01 不参与条件 GET（If-None-Match 命中也回 200）。"),
        (1, [HIST, HIST_NEG],
         "拒绝/边界腿：reconciliation=0/true/重复 → 400（闭合查询纪律）；畸形游标/未知会话/跨主体 404/403 沿既有证明（r06_t04_history 游标校验与归属隔离）。"),
    ],
    "R00-T02-LA-EE18A57F7348": [
        (0, [WB],
         "同版本返回 ok:true 且磁盘内容、新版本吻合：stat 后以当前版本 writeExpectedVersion → {ok:true, version:{size=新内容}}，磁盘回读一致。"),
        (1, [WB],
         "版本不同时返回 conflict:true（HTTP 200 结果对象）且不覆盖磁盘当前内容：ok:false + 当前 version + 外部字节保持。"),
        (2, [WB],
         "非法路径（../、含分隔符、空名）→ 400 invalid_name；未知 action → 400 unknown_action；拒绝路径不落盘、不呈现保存成功。"),
        (3, [WB],
         "封面冲突基于新快照最多再试：冲突 → re-stat 新快照 → 同接口重试成功；外部再改后仍失败 → 显式冲突错误，用户后改不被覆盖。"),
        (4, [WB],
         "先读后写不宣称跨进程原子性：读取期间外部修改使 expectedVersion 失配 → 冲突结果如实报告（非原子成功），磁盘外部内容保持。"),
    ],
    "R00-T02-LA-464E58DEC9E2": None,  # 特例：仅重建 G6（端口结构分支）。
    "R00-T02-LA-3A50AD28E08E": [
        (0, [FH_CHAIN, FH_SCOPE],
         "返回 workspace 中 relPath 的版本清单：单根链路既有证明 + 按助手根的清单（各助手只见自己根的 relPath 集）。"),
        (1, [FH_SCOPE, FH_CHAIN, FH_NOTTRACKED],
         "query.agentId 解析指定助手 home workspace（显式 desk.home_folder；无显式根回落实例默认根）并保持助手间历史隔离：B 查 A 的 relPath → 空清单；"
         "绝对/含 .. 的 relPath → 400；根不存在 → 404 workspace not tracked；B 读不到 A 的版本（不扩大披露范围）。"),
    ],
    "R00-T02-LA-A9DAA48DC603": [
        (0, [FH_CHAIN],
         "读取历史快照内容返回快照路径、时间、来源和 UTF-8 正文（既有单根链路证明）。"),
        (1, [FH_SCOPE, FH_CHAIN],
         "query.agentId 正整数 id 边界 + 助手隔离：B 用 A 的快照 id → 404（内容不泄露）；不存在 404；按根 scoped 视图不扩大披露范围。"),
    ],
    "R00-T02-LA-AF8E24A2F524": [
        (0, [FH_SCOPE, FH_CHAIN],
         "返回指定助手 home workspace 的已跟踪文件：显式根按助手解析（A 只见 A 根清单）；无显式根的助手回落实例默认根，不从其他助手继承。"),
        (1, [FH_SCOPE, FH_NOTTRACKED, FH_CHAIN],
         "agentId 必填 400；根不存在 → 404 workspace not tracked；跨助手 relPath/清单不串（不扩大披露范围）。"),
    ],
    "R00-T02-LA-55B24FEE5AFB": [
        (0, [ATT],
         "复制快照并登记附件：mode=snapshot 经安全闸后把源字节复制进会话托管缓存（storageKind=managed_cache，realPath 落 session-files/），"
         "副本字节 = 源字节；源文件后续外部修改不影响已复制快照；新快照行按新源身份另铸。"),
        (1, [ATT],
         "拒绝/无效输入：非法 mode → 400 无登记；目录 → 安全闸显式拒绝无登记；错误对用户可见且无越权持久化（reference 缺省模式保持 external 引用回归）。"),
    ],
    "R00-T02-LA-485B2D0A8A02": None,  # 特例：G1 补 busy 证明，G0 保留既有 FULL。
    "R00-T02-LA-604C64F4828A": [
        (0, [FOLD_ADD, FOLD_403],
         "增加会话授权目录：PATCH add 真实目录后范围扩大（初始+追加），同一文件由 checkpoint 403（追加前越出授权目录）变为 200 接受并登记文件版本。"),
        (1, [FOLD_ADD, FOLD_MATRIX],
         "拒绝矩阵：未知会话 404、普通文件当目录 400、无凭证 401；拒绝路径不改变既有范围（追加后范围保持初始+追加，重新读取核对）。"),
    ],
}

review = []
report = HERE / "r06r2-backend-gaps-proof-review.json"
ready_ids = []

for identifier, spec_row in BATCH.items():
    row = next(row for row in inventory["items"] if row["id"] == identifier)
    assert row["status"] == "GAP", identifier
    original = {key: row[key] for key in ["then", "assertions", "groups", "gaps"]}

    if identifier == "R00-T02-LA-464E58DEC9E2":
        # G0-G5 保持既有 FULL 绑定；G6 以端口真实行为翻 FULL：
        # 端口无懒加载运行时实例概念——显式 agentId=A 的保存只写 A 的
        # YAML（新增直接断言：B 的 YAML 不含 A 的密钥/provider），
        # 不存在「A 未加载回退焦点 B」分支。
        rebuilt = [g for g in original["groups"] if g["index"] != 6]
        rebuilt.append(full_group(
            6,
            [READBACK],
            "端口结构行为：无懒加载运行时实例概念，显式 agentId=A 的 updateConfig 即目标 A（A 的 YAML 清空内联凭证、写回清空字段），"
            "焦点 B 的 YAML 不被 A 的显式保存污染（新增直接断言：B YAML 不含 sk-new-1/myprov）；只有对 B 的显式保存才写 B。"
            "「A 运行时实例未加载时回退焦点 B」分支在端口结构性不存在，按端口真实行为断言。",
        ))
        rebuilt.sort(key=lambda g: g["index"])
    elif identifier == "R00-T02-LA-485B2D0A8A02":
        g0 = next(g for g in original["groups"] if g["index"] == 0)
        rebuilt = [g0, full_group(
            1,
            [ROTATE_BUSY, ROTATE],
            "前会话运行中（真实 execute 持有 session admission，provider 闸门挂起）时轮换 → 409 session_busy（reason 显式）；"
            "run 终态释放后同一路径轮换成功（201，旧会话归档）。无凭证 401/无可解析助手 400 沿既有证明。",
        )]
    else:
        rebuilt = [full_group(index, pairs, meaning) for index, pairs, meaning in spec_row]

    assert [g["index"] for g in rebuilt] == list(range(len(row["assertions"]))), identifier
    row["groups"] = rebuilt
    assert all(group["coverage"] == "FULL" for group in rebuilt), identifier
    row.update(
        status="READY",
        gaps=[],
        reviewSource=report.relative_to(anchors.ROOT).as_posix(),
    )
    ready_ids.append(identifier)
    assert row["then"] == original["then"] and row["assertions"] == original["assertions"], identifier
    review.append({"id": identifier, "original": original, "became": row["status"]})

anchors.MAP.write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
report.write_text(json.dumps({
    "status": "R06_R2_BACKEND_GAPS_BOUND_NOT_STAGE_ACCEPTANCE",
    "ready": ready_ids,
    "keptGap": [
        "R00-T02-LA-7432B00DEC94",
        "R00-T02-LA-76EE734926B4",
        "R00-T02-LA-93D5237ADE81",
    ],
    "review": review,
    "stageAcceptance": False,
}, ensure_ascii=False, indent=2) + "\n")
ids = HERE / "reviewed-original-ids.json"
selected = json.loads(ids.read_text())
assert not set(ready_ids).intersection(selected)
selected.extend(ready_ids)
ids.write_text(json.dumps(selected, indent=2) + "\n")
(HERE / "registered-ready-r06r2-backend-ids.json").write_text(
    json.dumps(ready_ids, indent=2) + "\n")
ready_total = sum(1 for row in inventory["items"] if row["status"] == "READY")
print(f"{len(ready_ids)} READY registered this batch ({ready_total} total); "
      f"3 inspected leaves kept GAP; full R06/R07 not accepted.")
