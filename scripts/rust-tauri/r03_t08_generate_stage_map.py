#!/usr/bin/env python3
"""Generate rust/crates/xtask/src/stage_maps/R03.json (R03-T08).

The 48 R00 REQUIRED_SUPPLEMENTAL leaves the acceptance ledger binds to
R03 are mirrored VERBATIM from both R00 ledgers (ACCEPTANCE_MAP.json for
identity/responsibility, FEATURE_STAGE_ACCEPTANCE.json for assertions and
due) — the xtask cross-check compares every mirror field before any
command runs. The classification (basisKind, share texts, pinned cases)
is the R03-T08 executor's stage-share decision, recorded here so the map
is reproducible from the ledgers + this table.

Run:  python3 scripts/rust-tauri/r03_t08_generate_stage_map.py
"""
import json
import pathlib
import sys

REPO = pathlib.Path(__file__).resolve().parents[2]
OUT = REPO / "rust/crates/xtask/src/stage_maps/R03.json"

acceptance = json.loads((REPO / "docs/rust-tauri/R00/ACCEPTANCE_MAP.json").read_text())
fsa = json.loads((REPO / "docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json").read_text())
fsa_by_id = {e["id"]: e for e in fsa["supplemental_scenarios"]}

r03_leaves = {
    sid: entry
    for sid, entry in acceptance["scenarios"].items()
    if "R03" in entry.get("execution_stage_ids", [])
}
assert len(r03_leaves) == 48, f"expected 48 R03-bound leaves, got {len(r03_leaves)}"

# ── R03 share classification ────────────────────────────────────────────────
# producer for every machine-checked case (the acceptance matrix drives the
# REAL chain; cases are asserted in-process and packaged by the script).
MATRIX = "a15_combo_and_leaves"
LEAF_CASES = "{EVIDENCE}/A15/leaf-cases.json"

# stage_share_satisfied leaves: id -> (cases, refs, stageShare, laterShare)
SHARE = {
    "R00-T02-LA-980C099F356F": (
        [("subagent-policy-proactive-delegation-default-off", 1),
         ("subagent-escalation-write-denied-zero-children", 0)],
        [MATRIX],
        "R03 份额=实验开关默认值与子代理衰减边界：subagent.proactive_delegation 默认关闭"
        "（frozen incumbent 默认）、只读父会话派生 write 档子代理被统一拒绝且零 child run"
        "（T06 权限衰减在真实运行层判定）",
        "开启后的提示词条件注入与完整研究子代理行为（工具按权限执行的完整语义）= R06-T03",
    ),
    "R00-T02-LA-B47CAC08F418": (
        [("cancel-live-run-accepted-terminal-cancelled", 1),
         ("cancel-terminal-run-already-terminal", 1),
         ("cancel-unknown-run-not-found", 1)],
        [MATRIX, "r02_auth_matrix"],
        "R03 份额=任务中止的运行层语义：live run 取消→四相取消+唯一 finalize 落 cancelled；"
        "已终态 run→AlreadyTerminal 诊断式 no-op（already_stopped 形态）；未知 run→NotFound；"
        "所有权边界与 execute 同链",
        "HTTP POST /chat/tasks/{taskId}/abort 路由形态（aborted/already_stopped 响应体）与"
        "跨端呈现 = R06-T04/R08-T07（R03 为服务层 cancel_run_for 入口）",
    ),
    "R00-T02-LA-00ECC9568490": (
        [("subagent-dispatch-child-lineage-recorded", 1)],
        [MATRIX],
        "R03 份额=subagent 派发运行层：dispatch 建 child run（四元 lineage：parent/origin="
        "subagent/source/cause 持久化）、父会话即时返回、结果经 steering 回流（T06 适配）",
        "subagent 工具的完整目录注册（list/launch 参数面、agent/model 选择）= R04/R06",
    ),
    "R00-T02-LA-207DF594B497": (
        [("subagent-close-thread-closed-status", 1)],
        [MATRIX],
        "R03 份额=线程关闭运行层：close 关闭同会话 OPEN 且非 busy 线程，reason 记为线程"
        " summary 锚（closed: {reason}），父会话可查询最终状态",
        "close 的工具参数面与父会话 UI 呈现 = R04/R06/R08",
    ),
    "R00-T02-LA-5F879158C679": (
        [("subagent-reply-continues-thread", 1)],
        [MATRIX],
        "R03 份额=线程续接运行层：reply 校验 thread 存在/同会话/open/非 busy 后在同线程开新"
        " child run，lineage.parent 指向发起回复的 run，结果回父会话",
        "reply 的工具参数面与隔离会话完整可见性规则 = R04/R06",
    ),
    "R00-T02-LA-857747F848F6": (
        [("subagent-child-cancel-stops-child-only", 1)],
        [MATRIX],
        "R03 份额=按 taskId 停止子任务的运行层：cancel_run_for(child) 只停目标 child run，"
        "父任务不受连带（同一取消树按 run 粒度）",
        "WS-IN subagent_stop_request 帧形态与终端呈现 = R06-T04/R08-T07",
    ),
    "R00-T02-LA-84E22CD74BE1": (
        [("parent-cancel-stops-child-run", 1)],
        [MATRIX],
        "R03 份额=停止传播到任务处理器：父 run 取消→取消树向下传播，子任务（含子代理/后台"
        " child）停止且不完成；durable 行经恢复扫描诚实收口（不伪造终态）",
        "媒体等其余任务处理器的停止形态 = R06+",
    ),
    "R00-T02-LA-8661576FEAFF": (
        [("background-run-completes-decoupled", 1)],
        [MATRIX],
        "R03 份额=cron 触发的执行基底：execute_background_for 同一 RunSupervisor detached"
        " 驱动、与调用方连接寿命解耦、完成后可查询可确认（RunOrigin::Cron 词表已入库）；"
        "cron job 存储与调度器本体归 R07-T01",
        "cron job 持久化/调度/触发（以指定 actor Agent 独立 Run 执行并广播 cron_job_done）"
        "= R07-T01",
    ),
    "R00-T02-LA-CFA5BB0273FA": (
        [("background-run-completes-decoupled", 1)],
        [MATRIX],
        "R03 份额=heartbeat 巡检的执行基底：同一后台提交/监督入口（RunOrigin::Heartbeat"
        " 词表已入库），detached 驱动+可确认收束；巡检内容与活动页呈现归 R06/R07",
        "heartbeat 扫描工作区、隔离巡检执行、活动页/通知显示结果 = R06-T03/R07-T01",
    ),
    "R00-T02-LA-D483E4CD0935": (
        [("combo-normal-run-completed", 1),
         ("combo-multi-turn-one-terminal", 1)],
        [MATRIX],
        "R03 份额=经授权会话 Run 的执行语义：admission→状态机→多轮模型调用→唯一终态+最终"
        "消息（替身只给外部响应，状态/事件全由真实 Supervisor/存储产生——A01/A15 形态）",
        "WS-IN prompt 帧形态、流式阶段/工具进度的传输呈现 = R06-T04/R08-T07",
    ),
    "R00-T02-LA-7CBF6F4A6760": (
        [("combo-cancel-stream-terminal-cancelled", 1),
         ("cancel-terminal-run-already-terminal", 1)],
        [MATRIX],
        "R03 份额=中止的运行层语义：流读取中取消→四相取消落 cancelled、迟到结果被栅栏"
        "（内容不落流）；已停 run 的再中止为诊断式 no-op（UI 恢复空闲的服务端依据）",
        "WS-IN abort 帧形态与 abort_result accepted 的客户端呈现 = R06-T04/R08-T07",
    ),
    "R00-T02-LA-C1FB9C92E813": (
        [("steer-busy-accepted", 1),
         ("steer-idle-miss", 1),
         ("steer-text-drained-into-model-input", 1)],
        [MATRIX],
        "R03 份额=steer 的冻结语义：busy→有界收件箱 Accepted、idle→Miss（调用方降级普通"
        "提交）、下一模型轮 drain 进入 input、打断策略不断循环（T02 冻结映射）",
        "WS-IN steer 帧（流已停降级 prompt）与 steered 回执呈现 = R06-T04/R08-T07",
    ),
    "R00-T02-LA-81664285F927": (
        [("reconnect-resume-no-model-restart", 1)],
        [MATRIX, "r02_events_matrix"],
        "R03 份额=重连只订阅不重启：断开后按 cursor 续读补发遗漏事件（R02 快照/游标面），"
        "重连绝不重启模型/不新建 run（T04 永久测试+矩阵案例）",
        "WS-IN resume_stream 帧（streamId/sinceSeq、重置/截断标记）的传输形态 = R06-T04/R08-T07",
    ),
    "R00-T02-LA-010BA6693880": (
        [("combo-events-subscriber-saw-terminal", 1)],
        [MATRIX, "r02_auth_matrix", "r02_events_matrix"],
        "R03 份额=事件转发与订阅登记：认证订阅者在 run 期间收到提交后的完整事件链（含终态"
        "事件）；R02 的按权限登记/转发原语在 R03 运行层持续成立（SUP-02 回归命令同链）",
        "聊天 WS 传输的客户端登记与断线重连完整行为 = R06-T04/R08-T07",
    ),
    "R00-T02-LA-81B217A7E581": (
        [("combo-crash-recovery-honest-interrupted", 1)],
        [MATRIX],
        "R03 份额=聊天页投影的状态权威：run 终态（含崩溃恢复 interrupted_needs_attention、"
        "无空白无假成功、不虚构最终回复）经事件/存储可查——页面投影失败不伪装成空数据的"
        "服务端依据",
        "聊天页完整 UI 投影（逐动作状态呈现）= R06-T04/R08-T07",
    ),
    "R00-T02-LA-94CB746D04B7": (
        [("cancel-waiting-approval-cancels-cleanly", 1)],
        [MATRIX],
        "R03 份额=ask 的审批等待运行层：waiting_approval durable 腿、审批等待可取消（取消"
        "后零工具执行）、晚到决策不复活（T03 ApprovalGate 语义）",
        "ask_user 工具本体（批量提问卡、按 key 映射答案、超时自动填推荐项、dismissed）="
        " R04（工具网关+T06 O-1 ask 档审批策略面）/R06-T04/R08-T07",
    ),
    "R00-T02-LA-4EA88B5073E4": (
        [("cancel-terminal-run-already-terminal", 1)],
        [MATRIX],
        "R03 份额=停止命令的运行层语义：无活动流时的停止请求为诊断式 AlreadyTerminal"
        " no-op（不误报、不改状态）——已停止提示的服务端依据",
        "/slash stop 命令解析与已停止提示的终端呈现 = R06-T04/R08-T07",
    ),
}

DEFERRED_REASONS = {
    "R00-T02-LA-A2C385A33493": ("悬浮窗可见性/尺寸/聚焦主窗 = 纯桌面 UI 行为", "R06-T04/R08-T07 完整悬浮窗行为"),
    "R00-T02-LA-1B712E189DDF": ("输入草稿读取 = 会话内容持久化面", "R06-T04/R08-T07 草稿读取呈现"),
    "R00-T02-LA-F829AEA5931C": ("home 草稿保存 = 会话内容持久化面", "R06-T04/R08-T07 草稿保存"),
    "R00-T02-LA-D901852E1B1A": ("会话草稿保存（sessionPath 解析）= 会话内容持久化面", "R06-T04/R08-T07"),
    "R00-T02-LA-081FACA79F2C": ("空 text 删除草稿 = 会话内容持久化面", "R06-T04/R08-T07"),
    "R00-T02-LA-B5CB2993A5C0": ("快捷聊天窗口设置读取 = 偏好持久化面", "R06-T04/R08-T07"),
    "R00-T02-LA-D92CD567D6D6": ("快捷聊天窗口设置保存 = 偏好持久化面", "R06-T04/R08-T07"),
    "R00-T02-LA-0FE8BED686E2": ("compact 命令内容（token 统计/完成说明）= 上下文压缩语义", "R06-T04（压缩权威）/R08-T07"),
    "R00-T02-LA-5A6E0D526903": ("fresh compact（刷新提示词和记忆）= 记忆/上下文语义", "R06-T04/R08-T07"),
    "R00-T02-LA-421EEA1AC8D5": ("compaction_accepted→结果帧 = 上下文压缩语义", "R06-T04/R08-T07"),
    "R00-T02-LA-92620BF2264F": ("interject 的媒体校验+插入队列 = 媒体/会话语义（队列语义已由 steer 叶钉住）", "R06-T04/R08-T07"),
    "R00-T02-LA-B34F2B298BFA": ("todo_write 工具（清单事件与状态呈现）= 工具目录+UI", "R04（工具注册）/R06-T04/R08-T07"),
    "R00-T02-LA-38667593C3E1": ("current_status 工具内容（模型/文件/Bridge 当前值）= 工具目录+上下文", "R04/R06-T04/R08-T07"),
    "R00-T02-LA-689DB6429895": ("notify 工具（投递通道/Bridge 追加）= 工具目录+外发通道", "R04（工具）/R07-T01（通道）"),
    "R00-T02-LA-E6F239636060": ("活动页 UI 投影 = 纯客户端呈现", "R07-T01 完整活动面板"),
    "R00-T02-LA-FA0FF4DC0779": ("自动化面板 UI 投影 = 纯客户端呈现", "R07-T01 完整自动化面板"),
    "R00-T02-LA-D8D47C4680C9": ("实验设置页 UI 投影 = 纯客户端呈现", "R06-T03/R07（设置页）"),
    "R00-T02-LA-A5DE44FF584F": ("实验读取 API（已解析实验与有效值）不在 R03 范围；唯一 R03 消费的实验（subagent.proactive_delegation）已由 LA-980C099F356F 钉住", "R06-T03 实验注册表/读取 API"),
    "R00-T02-LA-C99A4322D0C1": ("实验写入 API 不在 R03 范围", "R06-T03 实验 PATCH/生效值"),
    "R00-T02-LA-1FB3D8BC916D": ("desk cron 读取路由 = R07 管理面路由", "R07-T01 cron 路由族"),
    "R00-T02-LA-095E0402EEEC": ("cron 建议生效路由 = R07 管理面", "R07-T01"),
    "R00-T02-LA-A765DFFB4BAC": ("cron 添加路由 = R07 管理面", "R07-T01"),
    "R00-T02-LA-65B2ECECB23A": ("cron 移除路由 = R07 管理面", "R07-T01"),
    "R00-T02-LA-2CAC28D96AFE": ("cron 启停路由 = R07 管理面", "R07-T01"),
    "R00-T02-LA-267BDFB75804": ("cron 更新路由 = R07 管理面", "R07-T01"),
    "R00-T02-LA-5882BC217F1D": ("desk heartbeat 触发路由 = R07 管理面", "R07-T01"),
    "R00-T02-LA-EC5167DAB40C": ("automation 工具（list/create/update+自动批准）= 工具目录+cron 存储", "R04（工具）/R07-T01（CronStore）"),
    "R00-T02-LA-5F394AA4C9A3": ("slash apply 命令（建议写为自动任务）= 命令目录+cron 存储", "R06-T04/R07-T01"),
    "R00-T02-LA-3CF3C154B311": ("fresh compact 调度内容 = 压缩语义（调度基底已由 LA-866157 钉住）", "R06（压缩）/R07-T01（调度）"),
    "R00-T02-LA-7244E7DCE678": ("会话 pending/resolved/failed 任务清单面不在 R03 范围（R03 交付 run/cancel/lineage 查询面）", "R06-T03 会话任务清单"),
    "R00-T02-LA-2FAA749DA790": ("slash 命令分发（同一 dispatcher 与 slash_result）= 命令目录/会话语义", "R06-T04/R08-T07 slash 帧与结果呈现"),
}

assert set(SHARE) | set(DEFERRED_REASONS) == set(r03_leaves), (
    set(SHARE) | set(DEFERRED_REASONS) ^ set(r03_leaves)
)

leaves = []
for sid in sorted(r03_leaves):
    entry = r03_leaves[sid]
    fsa_entry = fsa_by_id[sid]
    base = {
        "id": sid,
        "featureId": entry["feature_id"],
        "requirement": entry["requirement"],
        "r00Kind": entry["kind"],
        "r00TaskIds": entry["task_ids"],
        "r00ExecutionStageIds": entry["execution_stage_ids"],
        "r00LedgerStatus": entry["ledger_status"],
        "r00ResultIds": entry["result_ids"],
        "r00TestIds": entry["test_ids"],
        "r00Then": entry["then"],
        "r00Assertions": fsa_entry["assertions"],
        "r00Due": fsa_entry["due"],
    }
    if sid in SHARE:
        cases, refs, stage_share, later_share = SHARE[sid]
        leaves.append({
            **base,
            "basisKind": "stage_share_satisfied",
            "stageShare": stage_share,
            "laterShare": later_share,
            "evidenceRequired":
                "本阶段份额由 a15_combo_and_leaves 生产者经真实服务链路逐案例机器核验"
                "（lingxi.leaf-case-results.v1，actual==图钉）；份额图钉全过即 R03 份额 PASS",
            "evidenceCommandRefs": refs,
            "assertionContract": {
                "producerCommand": MATRIX,
                "evidencePath": LEAF_CASES,
                "cases": [{"case": c, "expect": e} for c, e in cases],
            },
        })
    else:
        reason, later = DEFERRED_REASONS[sid]
        leaves.append({
            **base,
            "basisKind": "deferred_to_later_stage",
            "stageShare":
                f"无 R03 叶专属门禁份额：{reason}；运行/取消/身份/事件基底由基础场景 "
                f"R03-A01..A16 与 a15_combo_and_leaves 组合矩阵门禁承担，无叶专属份额需要图钉",
            "laterShare": later,
            "evidenceRequired":
                "R03 不判本叶（DEFERRED，仍 REQUIRED）：验收归属 r00ExecutionStageIds 中的"
                "后续阶段（R06/R07/R08），后续阶段图必须消费本叶余款",
            "evidenceCommandRefs": [],
        })

stage_map = {
    "schemaVersion": 1,
    "resultVersion": "lingxi.xtask.verify-stage.v1",
    "stage": "R03",
    "defaultTimeoutSecs": 1200,
    "supplementalCoverageNote":
        "R03-T08 阶段图（2026-09-29）：本图由 R03_SCOPE_MATRIX.json 的 4 项补充义务驱动建立——"
        "SUP-01（本图自身+STAGE_MAPS/runner_identity 注册+verify-stage R03 门禁）、SUP-02（R02 "
        "接口消费回归：r02_auth_matrix/r02_events_matrix/r02_storage_tx/r02_backup_restore/"
        "r02_recovery_drill/r02_full_chain/r02_legacy_regression 七条定向命令在本图内重跑，另由"
        "T08 全新证据目录完整 verify-stage R02 一次）、SUP-03（CLI 聊天叶 R03 份额：运行层取消"
        "语义经基础场景 A05/A06/A07 与组合矩阵钉住，终端客户端不提前交付）、SUP-04（R07 九项"
        "递延叶保持 REQUIRED 递延于 R02.json 原图，R03 不改写；本图 supplementalLeafScenarios "
        "与 R00 账本相等性检查同样成立——48 项 R03 绑定叶逐叶全字段镜像+分类）。分类："
        "17 stage_share_satisfied（R03 份额=运行/取消/审批/子代理/后台/重连基底，案例由组合矩阵"
        "生产者机器核验）+ 31 deferred_to_later_stage（无 R03 叶专属份额；验收归 R06/R07/R08，"
        "仍 REQUIRED）。阶段中立 kinds（stage_share_satisfied/deferred_to_later_stage）为 R03-T08 "
        "新增，语义与 R02 的 r02_share_satisfied/deferred_to_r07 同构（含 R14-F01 反假绿：份额叶"
        "必带 assertionContract，递延叶不得绑门禁命令/证据/契约）；R02 图零改动。",
    "commands": {
        "rust_fmt": {
            "argv": ["cargo", "fmt", "--manifest-path", "rust/Cargo.toml", "--all", "--", "--check"],
            "timeoutSecs": 300,
            "evidencePaths": ["{EVIDENCE}/rust_fmt/stdout.log"],
        },
        "rust_clippy": {
            "argv": ["cargo", "clippy", "--manifest-path", "rust/Cargo.toml", "--workspace",
                      "--all-targets", "--locked", "--", "-D", "warnings"],
            "timeoutSecs": 1200,
            "evidencePaths": ["{EVIDENCE}/rust_clippy/stdout.log"],
        },
        "rust_test_workspace": {
            "argv": ["cargo", "test", "--manifest-path", "rust/Cargo.toml", "--workspace", "--locked"],
            "timeoutSecs": 2400,
            "evidencePaths": ["{EVIDENCE}/rust_test_workspace/stdout.log"],
        },
        "check_contracts": {
            "argv": ["bash", "scripts/rust-tauri/r01-t02-check-generated.sh"],
            "timeoutSecs": 900,
            "evidencePaths": ["{EVIDENCE}/check_contracts/stdout.log"],
        },
        "check_boundaries": {
            "argv": ["python3", "-B", "docs/rust-tauri/R01/r01_t01_check_ownership.py"],
            "timeoutSecs": 600,
            "evidencePaths": ["{EVIDENCE}/check_boundaries/stdout.log"],
        },
        "a15_combo_and_leaves": {
            "argv": ["bash", "scripts/rust-tauri/r03_t08_matrix.sh", "{EVIDENCE}/A15"],
            "timeoutSecs": 1200,
            "evidencePaths": [
                "{EVIDENCE}/A15/leaf-cases.json",
                "{EVIDENCE}/A15/combo-counts.json",
                "{EVIDENCE}/A15/integration.log",
                "{EVIDENCE}/A15/summary.txt",
            ],
        },
        "a16_seed_mechanism": {
            "argv": ["bash", "scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh", "{EVIDENCE}/A16"],
            "timeoutSecs": 2400,
            "evidencePaths": [
                "{EVIDENCE}/A16/summary.txt",
                "{EVIDENCE}/A16/captured-seed.txt",
                "{EVIDENCE}/A16/seed-mechanism.json",
            ],
        },
        "r02_auth_matrix": {
            "argv": ["bash", "scripts/rust-tauri/r02_t03_auth_matrix.sh", "{EVIDENCE}/R02/A05_A06"],
            "timeoutSecs": 900,
            "evidencePaths": [
                "{EVIDENCE}/R02/A05_A06/summary.txt",
                "{EVIDENCE}/R02/A05_A06/leaf-cases.json",
                "{EVIDENCE}/R02/A05_A06/ws-matrix.json",
                "{EVIDENCE}/R02/A05_A06/sessions-list-matrix.json",
            ],
        },
        "r02_storage_tx": {
            "argv": ["bash", "scripts/rust-tauri/r02_t04_storage_tx.sh", "{EVIDENCE}/R02/A07_A08"],
            "timeoutSecs": 900,
            "evidencePaths": [
                "{EVIDENCE}/R02/A07_A08/s1-counts-after-stop.json",
                "{EVIDENCE}/R02/A07_A08/s4-hashes-after.txt",
            ],
        },
        "r02_events_matrix": {
            "argv": ["bash", "scripts/rust-tauri/r02_t05_events_matrix.sh", "{EVIDENCE}/R02/A09_A10"],
            "timeoutSecs": 900,
            "evidencePaths": [
                "{EVIDENCE}/R02/A09_A10/probe-matrix.jsonl",
                "{EVIDENCE}/R02/A09_A10/key-events-dump.json",
            ],
        },
        "r02_backup_restore": {
            "argv": ["bash", "scripts/rust-tauri/r02_t06_backup_restore.sh", "{EVIDENCE}/R02/A11"],
            "timeoutSecs": 900,
            "evidencePaths": ["{EVIDENCE}/R02/A11/backup-restore/summary.txt"],
        },
        "r02_recovery_drill": {
            "argv": ["bash", "scripts/rust-tauri/r02_t06_recovery_drill.sh", "{EVIDENCE}/R02/A12"],
            "timeoutSecs": 900,
            "evidencePaths": ["{EVIDENCE}/R02/A12/recovery-drill/summary.txt"],
        },
        "r02_full_chain": {
            "argv": ["bash", "scripts/rust-tauri/r02_t08_full_chain_smoke.sh", "{EVIDENCE}/R02/A15"],
            "timeoutSecs": 1200,
            "evidencePaths": ["{EVIDENCE}/R02/A15/full-chain/summary.txt"],
        },
        "r02_legacy_regression": {
            "argv": ["env", "R02_LEGACY_REGRESSION_MODE=directed-no-seal-family",
                     "bash", "scripts/rust-tauri/r02_t08_legacy_entry_regression.sh", "{EVIDENCE}/R02/A16"],
            "timeoutSecs": 1200,
            "evidencePaths": ["{EVIDENCE}/R02/A16/legacy-entry/summary.txt"],
        },
    },
    "scenarios": [
        {"id": "R03-A01", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "a15_combo_and_leaves"]},
        {"id": "R03-A02", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "r02_storage_tx"]},
        {"id": "R03-A03", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace"]},
        {"id": "R03-A04", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace"]},
        {"id": "R03-A05", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "a15_combo_and_leaves"]},
        {"id": "R03-A06", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "a15_combo_and_leaves"]},
        {"id": "R03-A07", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "a15_combo_and_leaves"]},
        {"id": "R03-A08", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "a15_combo_and_leaves"]},
        {"id": "R03-A09", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace"]},
        {"id": "R03-A10", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace"]},
        {"id": "R03-A11", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "a15_combo_and_leaves"]},
        {"id": "R03-A12", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "a15_combo_and_leaves"]},
        {"id": "R03-A13", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "a15_combo_and_leaves", "r02_full_chain", "r02_backup_restore"]},
        {"id": "R03-A14", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "r02_recovery_drill"]},
        {"id": "R03-A15", "requirement": "REQUIRED", "commandRefs": ["a15_combo_and_leaves", "rust_test_workspace", "rust_fmt", "rust_clippy", "check_contracts", "check_boundaries", "r02_auth_matrix", "r02_legacy_regression"]},
        {"id": "R03-A16", "requirement": "REQUIRED", "commandRefs": ["a16_seed_mechanism", "rust_test_workspace"]},
    ],
    "supplementalLeafScenarios": leaves,
}

OUT.write_text(json.dumps(stage_map, indent=2, ensure_ascii=False) + "\n")
print(f"wrote {OUT} with {len(leaves)} supplemental leaves "
      f"({sum(1 for l in leaves if l['basisKind'] == 'stage_share_satisfied')} share + "
      f"{sum(1 for l in leaves if l['basisKind'] == 'deferred_to_later_stage')} deferred)")
