#!/usr/bin/env python3
"""Generate rust/crates/xtask/src/stage_maps/R04.json (R04-T08).

The 124 R00 REQUIRED_SUPPLEMENTAL leaves the acceptance ledger binds to
R04 are mirrored VERBATIM from both R00 ledgers (ACCEPTANCE_MAP.json for
identity/responsibility, FEATURE_STAGE_ACCEPTANCE.json for assertions and
due) — the xtask cross-check compares every mirror field before any
command runs. The classification (basisKind, share texts, pinned cases)
is the R04-T08 executor's stage-share decision, recorded here so the map
is reproducible from the ledgers + this table.

Stage-share policy (the master prompt §6 split): each leaf separates the
"R04 工具基础责任" (the tool-chain mechanism this stage delivered and
machine-verifies — catalog/gateway/permission/approval/exec/file/worker/
MCP mechanisms) from the "完整业务交互" (the product behavior the later
stage owns). 46 R04-only leaves + read/write/edit + the six on-demand
file-family shapes are stage_share_satisfied (their R04 mechanism share
is pinned by named cases the matrix producer re-verifies); the remaining
69 are deferred_to_later_stage (no R04 gating share of their own; the
R04 substrate they consume — ResourceAccess/ResourceRef, the worker RPC,
the native executors — is named in the share text but never faked).

Run:  python3 scripts/rust-tauri/r04_t08_generate_stage_map.py
"""
import json
import pathlib

REPO = pathlib.Path(__file__).resolve().parents[2]
OUT = REPO / "rust/crates/xtask/src/stage_maps/R04.json"

acceptance = json.loads((REPO / "docs/rust-tauri/R00/ACCEPTANCE_MAP.json").read_text())
fsa = json.loads((REPO / "docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json").read_text())
fsa_by_id = {e["id"]: e for e in fsa["supplemental_scenarios"]}

r04_leaves = {
    sid: entry
    for sid, entry in acceptance["scenarios"].items()
    if "R04" in entry.get("execution_stage_ids", [])
}
assert len(r04_leaves) == 124, f"expected 124 R04-bound leaves, got {len(r04_leaves)}"

MATRIX = "r04_tool_matrix"
CASES = "{EVIDENCE}/R04_MATRIX/leaf-cases.json"
EVIDENCE_NOTE = (
    "本阶段份额由 r04_tool_matrix 生产者经真实工具链路（注册表→统一网关→T03 策略/批准面→"
    "原生执行器/worker/MCP 桥，必要处经真实 run 链）逐案例机器核验"
    "（lingxi.leaf-case-results.v1，actual==图钉）；份额图钉全过即 R04 份额 PASS"
)

# ── share classification: leaf id -> (cases, stageShare, laterShare) ────────

SHARE = {}

def share(sid_prefix, cases, stage_share, later_share):
    # Leaf ids may be given as unique prefixes (readability).
    matches = [sid for sid in r04_leaves if sid.startswith(sid_prefix)]
    assert len(matches) == 1, f"{sid_prefix}: expected one leaf, got {matches}"
    sid = matches[0]
    assert sid not in SHARE, sid
    SHARE[sid] = (cases, stage_share, later_share)

def future_case(name):
    return [(f"future-tool-shape-{name}-discoverable-not-callable", 1)]

FUTURE_LATER = (
    "工具执行本体（完整匹配/变换/诊断/内核语义）= R07 外围能力接入（R04_SCOPE_MATRIX "
    "explicit_non_goals 与 R04_HANDOFF deferred_registrations 登记，不丢失）"
)

# — the four basic tools + write_stdin: the R04 share IS the native tool —
share("R00-T02-LA-93D5237ADE81",
      [("tool-read-real-chain", 1)],
      "R04 份额=read 工具本体：Rust 原生执行器经统一网关/资源授权在真实 run 链执行，"
      "分页/截断/二进制诚实语义（T04 套件）+ 矩阵真实链案例钉住",
      "R06 份额=附件/上下文消费与 GBK/office 解码（T04 §8 登记的未迁移面）")
share("R00-T02-LA-4F0466F7C090",
      [("tool-write-real-chain", 1), ("a15-real-delivery-registered", 1)],
      "R04 份额=write 工具本体：原子写/陈旧冲突/ResourceRef 交付经登记前核验"
      "（存在性+常规文件）+ 矩阵真实链案例钉住",
      "R06 份额=写入内容的上下文/附件消费")
share("R00-T02-LA-C6D338A24CF8",
      [("tool-edit-real-chain", 1), ("tool-edit-conflict-preserves-user-version", 1)],
      "R04 份额=edit 工具本体：精确/fuzzy 匹配（OBS-1 修复后重复计数与现役 fuzzy 空间对齐）、"
      "并发冲突保用户版本、TOCTOU 防护；矩阵真实链+冲突案例钉住",
      "R06 份额=编辑历史的上下文消费；NFKC fold 仍为 T04 §8.6 登记缺口")
share("R00-T02-LA-8A3C87812B4F",
      [("tool-exec-command-real-chain", 1), ("tool-exec-cancel-cleanup", 1)],
      "R04 份额=exec_command 工具本体：结构化 argv/真实进程监督/输出有界/退出码/超时/"
      "取消清理进程树（T05 A09 全树版本在 T05 套件；矩阵钉真实链+轻量取消清理）",
      "完整多平台 shell 解析收窄与 wait_mode=auto 形态=T05 §8 登记的受控缺口（R07 按需）")
share("R00-T02-LA-C90F42576683",
      [("tool-write-stdin-continuation", 1), ("tool-write-stdin-foreign-writes", 0)],
      "R04 份额=write_stdin 工具本体：会话/句柄所有权三重校验、续写投递新输出"
      "（跨主体写入 0 成功为图钉）",
      "终端 close/list 产品动作与 jsonl transcript 持久化=R06+（T05 §8 登记）")

# — the on-demand file family: catalog honesty is the R04 mechanism share —
for name, sid in [
    ("file", "R00-T02-LA-3CBCAE"),
    ("find", "R00-T02-LA-F88B0A969094"),
    ("grep", "R00-T02-LA-EC77C2B04726"),
    ("ls", "R00-T02-LA-B97D0D4296FA"),
    ("materialize", "R00-T02-LA-AD7FAE6288FA"),
    ("stage_files", "R00-T02-LA-1CD1C3"),
]:
    share(
        sid,
        future_case(name),
        f"R04 份额=目录/网关机制面：{name} 工具形态可注册可发现（Availability::Future），"
        "未迁移能力诚实拒调用（不伪装 available），按需解析经同一网关与权限面",
        f"{name} 工具执行本体=R06（按需文件工具族）；R04 已交付其消费的 read/write/edit "
        "原生执行器与 ResourceAccess/ResourceRef 机制",
    )

# — the dev/scan tools (R04-only): mechanism share + honest non-availability —
for name, sid in [
    ("ast_edit", "R00-T02-LA-96DD1FF9E9D5"),
    ("ast_grep", "R00-T02-LA-196D50D8DD6E"),
    ("lsp", "R00-T02-LA-2D896560381E"),
    ("run_code", "R00-T02-LA-C88F29B5114A"),
    ("security_scan", "R00-T02-LA-CD5D7FC02D8E"),
]:
    share(
        sid,
        future_case(name),
        f"R04 份额=执行面机制+目录诚实性：{name} 形态在目录中 Future 可发现不可调用"
        "（零派发拒词汇稳定）；其将消费的进程执行面（结构化 argv/监督/取消清理/资源授权）"
        "由 exec_command/write_stdin 真实案例同图钉组覆盖",
        FUTURE_LATER,
    )

# — the terminal WS frames (R04-only): the PTY transcript mechanism —
share("R00-T02-LA-4C35E6AEC6F7",
      [("terminal-tail-cursor-continuation", 1)],
      "R04 份额=tail 机制层：持久终端游标增量投递（write_stdin 续写只回新输出），"
      "terminalId/sinceSeq 语义的执行面已成立",
      "WS-IN 帧形态与传输呈现=R06-T04/R08-T07（R03 图同口径）")
share("R00-T02-LA-CE69063550AA",
      [("terminal-snapshot-current-transcript", 1)],
      "R04 份额=snapshot 机制层：当前 transcript 快照读取（第二次写入只见新输出="
      "游标推进的当前尾部，非重放）",
      "WS-IN 帧形态与传输呈现=R06-T04/R08-T07")
share("R00-T02-LA-CFD9F02BC6AA",
      [("terminal-close-stops-terminal", 1)],
      "R04 份额=close 机制层：显式关闭终止终端（有界、可观察、迟到写入诚实失败）",
      "WS-IN 帧形态与传输呈现=R06-T04/R08-T07")

# — the MCP tool family (R04-only) —
share("R00-T02-LA-483E461BB59D",
      [("mcp-tool-call-full-chain", 1), ("matrix-route-consistency", 1)],
      "R04 份额=mcp 工具调用本体：真实 rmcp 握手/清单同步入目录/经统一网关在真实 run 链执行，"
      "同目标直调与目录调用权限一致（A03 路线一致性案例）",
      "HTTP/SSE transport 与真实外装 server=R07（D-06）")
share("R00-T02-LA-8BCB8A749864",
      [("mcp-describe-real-identity", 1)],
      "R04 份额=describe 机制层：source+server+tool 命名空间身份的目录描述（T01 同源语义）",
      "产品级工具详情 UI=R07/R08")
share("R00-T02-LA-CEDC75156D33",
      [("mcp-search-namespaced", 1)],
      "R04 份额=search 机制层：命名空间目录查询命中真实 target 身份",
      "产品级搜索 UI=R07/R08")
share("R00-T02-LA-0818574ABD43",
      [("gateway-each-call-independent-permission", 1)],
      "R04 份额=子调用独立权限机制：统一网关对每次调用按 target+参数独立裁决"
      "（同一 read_only 上下文 read 派发而 write 拒绝）——run_tools 的每个子调用"
      "必须经过的正是这一判定面",
      "run_tools 工具本体（脚本编排/子调用摘要呈现）=R07 外围能力接入")
share("R00-T02-LA-B4BB2438E855",
      [("catalog-face-lists-mcp-tools-with-permission", 1)],
      "R04 份额=数据机制面：设置页消费的连接器/工具权限目录面（快照列出 MCP 工具"
      "及权限契约）真实可读",
      "设置页 UI 投影（容器初始化/表单/批量反馈/OAuth 呈现）=R07/R08（不倒灌 R04）")
share("R00-T02-LA-DA5AD5C40098",
      [("permission-face-modes-verifiable", 1)],
      "R04 份额=数据机制面：安全设置消费的会话权限模式面（operate/ask/read_only "
      "可设置可读取）真实成立",
      "设置页 UI 投影=R07/R08")

# — the MCP connector management family (R04-only): composition-entry share —
CONNECTOR_SUBSTRATE = (
    "连接器持久化存储/产品路由/OAuth 产品流/批量导入语义=R07 外围接入"
    "（R04_SCOPE_MATRIX explicit_non_goals 与 R04_HANDOFF deferred_registrations 登记）"
)
share("R00-T02-LA-0199B843D759",
      [("mcp-connector-register-handshake", 1)],
      "R04 份额=组合入口层：register_mcp_server 真实 initialize 握手+协议协商+清单入目录"
      "（失败响亮拒绝不注册），连接器『保存并启动』的执行面",
      CONNECTOR_SUBSTRATE)
share("R00-T02-LA-04A6A2BD1547",
      [("mcp-connector-catalog-sync", 1)],
      "R04 份额=连接器资源读取的目录面：清单同步入注册表（target 身份/schema 快照），"
      "资源内容按其类型验证（远程 URI 永不铸本地引用）",
      "MCP resources/read 协议面与产品资源浏览=R07")
share("R00-T02-LA-18EFB2D9D5FD",
      [("mcp-connector-register-handshake", 1)],
      "R04 份额=start 机制层：连接槽宿主所有，启动=真实握手（connect_count 审计）",
      CONNECTOR_SUBSTRATE)
share("R00-T02-LA-E9C7A48CADC4",
      [("mcp-connector-register-handshake", 1)],
      "R04 份额=stop 机制层：连接生命周期归宿主（连接槽/死传输检测/重连=新握手从不重放调用"
      "——A13 套件钉住），停止不残留受管状态",
      CONNECTOR_SUBSTRATE)
share("R00-T02-LA-2D194C1684BC",
      [("matrix-lifecycle-uninstall-holes", 0)],
      "R04 份额=delete 机制层：注册表 uninstall 语义（名称/代次/句柄全失效，"
      "矩阵 uninstall 零洞案例钉住）",
      "连接器持久化行的产品删除流=R07")
share("R00-T02-LA-F4DA2AFCB72B",
      [("matrix-lifecycle-generation-refusals", 0)],
      "R04 份额=put 更新机制层：清单更新=代次上升，旧句柄/旧描述随代次死亡"
      "（矩阵 generation 零洞案例钉住）",
      "连接器编辑表单与配置持久化=R07")
share("R00-T02-LA-48B0C7A7453E",
      [("mcp-connector-catalog-sync", 1)],
      "R04 份额=批量导入的机制层：多连接器注册=多次真实握手+清单同步（逐项成功/失败"
      "区分由各次注册的响亮拒绝构成）",
      "批量导入产品流与逐项反馈呈现=R07")
share("R00-T02-LA-21B3F4DC9140",
      [("mcp-connector-catalog-sync", 1)],
      "R04 份额=refresh-tools 机制层：refresh_mcp_server 重列+注册/更新/消失同步"
      "（死传输恰一次重连，A13 套件钉住）",
      "产品级刷新按钮/通知呈现=R07")
share("R00-T02-LA-2DA782C5C7B9",
      [("mcp-tool-permission-face", 1)],
      "R04 份额=OAuth 前置机制面：连接器工具调用恒经 T03 权限面（operate/ask/read_only "
      "三档真实裁定），凭证材料永不下发（A14 套件钉住）",
      "OAuth 授权流（HTTP transport D-06 未携带）与登录产品流=R05/R07")
share("R00-T02-LA-E7F852F9BF60",
      [("mcp-tool-permission-face", 1)],
      "R04 份额=取消后机制面：授权未完成=调用不可执行（权限面拒绝语义）",
      "OAuth 取消产品流=R07")
share("R00-T02-LA-C70E819F8DA4",
      [("mcp-tool-permission-face", 1)],
      "R04 份额=退出后机制面：无凭证状态下工具调用按权限面裁定，秘密不残留不下发",
      "OAuth 退出产品流=R07")
share("R00-T02-LA-240E200EF440",
      [("mcp-tool-permission-face", 1)],
      "R04 份额=按助手连接器工具面的机制层：工具授权=目录权限契约+T03 面裁定（同一面）",
      "按助手的连接器配置持久化与产品呈现=R07")
share("R00-T02-LA-E1FBE1A59BC6",
      [("mcp-connector-catalog-sync", 1)],
      "R04 份额=apps 清单机制层：连接器工具清单入目录（真实握手同步）",
      "apps 产品路由与呈现=R07")
share("R00-T02-LA-15AD6ED13B4D",
      [("mcp-tool-call-full-chain", 1)],
      "R04 份额=app 工具调用机制层：经统一网关真实 run 链执行（同 mcp-call）",
      "app 级产品路由=R07")
share("R00-T02-LA-7FF8D4E48BC9",
      [("mcp-tool-call-full-chain", 1)],
      "R04 份额=按工具名调用机制层：命名空间 target 解析+网关执行（同 mcp-call）",
      "app 级产品路由=R07")
share("R00-T02-LA-DD47275AC5AE",
      [("mcp-connector-catalog-sync", 1)],
      "R04 份额=state 机制层：同步报告（协商协议/server 身份/registered 数）+is_connected "
      "真实可读",
      "连接器状态产品路由与轮询=R07")
share("R00-T02-LA-FAE7503D0D0F",
      [("matrix-lifecycle-disable-holes", 0)],
      "R04 份额=enabled 机制层：全局启停映射到目录 availability 面（Disabled 可发现"
      "不可调用，矩阵 disable 零洞案例钉住——expect=0 即零洞；返回合并状态=sync 报告）",
      "全局开关产品路由与配置持久化=R07")
share("R00-T02-LA-BB1BB3A9C5F4",
      [("catalog-face-lists-mcp-tools-with-permission", 1)],
      "R04 份额=延迟工具加载机制层：Deferred availability 按需目录语义（可发现、"
      "按需解析经同一网关）",
      "延迟阈值设置与产品呈现=R07")

# — the permission-mode / plan-mode / confirm family (R04-only) —
share("R00-T02-LA-D01766AF4475",
      [("permission-face-modes-verifiable", 1)],
      "R04 份额=权限模式面：setSessionPermissionMode 的执行面（会话模式可设置、运行链"
      "按快照裁决——T03 套件 user_session_mode_matrix 钉住）",
      "HTTP 路由形态与前端呈现=R08（服务层面已成立）")
share("R00-T02-LA-25C4FF66FEE5",
      [("permission-face-modes-verifiable", 1)],
      "R04 份额=偏好读取机制层：会话权限模式的可读面（session_supervisor 读取真实状态）",
      "偏好持久化路由与产品呈现=R07/R08")
share("R00-T02-LA-8D3CEB6133E1",
      [("permission-face-modes-verifiable", 1)],
      "R04 份额=偏好写入机制层：模式设置经真实面生效（拒绝对目标状态不变——T03 套件）",
      "偏好持久化路由与产品呈现=R07/R08")
share("R00-T02-LA-0B669B30C854",
      [("permission-face-modes-verifiable", 1)],
      "R04 份额=server 读取机制层：模式读取面真实（同上）",
      "管理面路由形态=R08")
share("R00-T02-LA-5E61048F19B9",
      [("permission-face-modes-verifiable", 1)],
      "R04 份额=server 修改机制层：模式修改经真实面生效（同上）",
      "管理面路由形态=R08")
share("R00-T02-LA-A336F79E964D",
      [("permission-face-modes-verifiable", 1)],
      "R04 份额=plan-mode 切换机制层：plan 档=read_only 预设的会话模式切换（同一面）",
      "plan 模式产品交互与提示词条件注入=R06/R08")
share("R00-T02-LA-BC2FBD618278",
      [("permission-face-modes-verifiable", 1)],
      "R04 份额=plan-mode 读取机制层：模式读取面真实（同上）",
      "管理面路由形态=R08")
share("R00-T02-LA-75C0AE981505",
      [("approval-answer-executes-once", 1), ("approval-duplicate-idempotent", 1)],
      "R04 份额=confirm 机制层：ConfirmStore 的执行面=ApprovalService 答复面"
      "（确认→恰执行一次；重复点击 AlreadySettled 确定性 no-op）",
      "slash 命令分发与 dispatcher 权限检查的产品形态=R06-T04/R08-T07")
share("R00-T02-LA-CD1524CC7DC3",
      [("approval-reject-zero-dispatch", 0)],
      "R04 份额=reject 机制层：拒绝→零执行、收据如实（ dispatched=0 为图钉）",
      "slash 命令分发产品形态=R06-T04/R08-T07")
share("R00-T02-LA-EC033184BA37",
      [("approval-answer-executes-once", 1)],
      "R04 份额=confirmed 机制层：批准绑定有效参数摘要+真实资源范围后待批动作才继续"
      "（T03 A05 套件钉住批准后改参不可执行）",
      "HTTP 400/403/404 路由形态与广播呈现=R08")
share("R00-T02-LA-EF43ADCE19A1",
      [("approval-reject-zero-dispatch", 0)],
      "R04 份额=rejected 机制层：拒绝后待批动作不执行、状态与实际副作用一致",
      "HTTP 路由形态与广播呈现=R08")
share("R00-T02-LA-67256417FB2B",
      [("preauthorization-single-session-scoped", 1)],
      "R04 份额=会话级能力放行机制层：预授权整键匹配（target+capability+digest）、"
      "单次使用、会话作用域、不持久化（重启失效）——矩阵经真实 gate 消费钉住",
      "MCP 会话权限产品路由=R08")

# ── deferred classification: family reason + later stage ────────────────────

DEFERRED = {}

def defer(prefix_or_ids, reason, later):
    if isinstance(prefix_or_ids, str):
        matched = [
            sid for sid, entry in r04_leaves.items()
            if prefix_or_ids in entry["feature_id"]
        ]
        assert matched, prefix_or_ids
        for sid in matched:
            assert sid not in DEFERRED and sid not in SHARE, sid
            DEFERRED[sid] = (reason, later)
    else:
        for sid in prefix_or_ids:
            assert sid in r04_leaves, sid
            assert sid not in DEFERRED and sid not in SHARE, sid
            DEFERRED[sid] = (reason, later)

defer("SEMANTIC_EFFECT-SEMANTIC-EFFECT-RESOURCE-IO-",
      "资源 IO API（list/stat/rename/move/trash/watch/subscribe/search/events）不在 R04 交付面；"
      "其消费的 R04 基底已交付并测试（原生 read/write/edit 执行器、ResourceAccess 授权、"
      "ResourceRef 登记前核验）",
      "R06-T05 资源 IO 完整 API（验收归 R06，R04 图钉的基底案例继续作为其输入）")
defer("SEMANTIC_EFFECT-SEMANTIC-EFFECT-RESOURCE-CONTENT-",
      "资源内容读取 API 归 R06；R04 交付其底层 read 语义（分页/截断/二进制诚实）",
      "R06-T05")
defer("SEMANTIC_EFFECT-SEMANTIC-EFFECT-RESOURCES-RESOURCES-",
      "按 resource id 的资源下载/传输 API 归 R06；R04 交付其身份基底（ResourceRef 登记"
      "前核验——存在性+常规文件，A15 套件钉住）",
      "R06")
defer("SEMANTIC_EFFECT-SEMANTIC-EFFECT-ATTACHMENT-",
      "附件（blob 媒体/语音/复用/本地快照/目录引用）归 R06 上下文消费面；"
      "R04 只交付其文件读取与 ResourceRef 基底",
      "R06")
defer("SEMANTIC_EFFECT-SEMANTIC-EFFECT-BRIDGE-",
      "Bridge 媒体 token 归 R06 Bridge 面；R04 无 Bridge 接入",
      "R06/R08")
defer("SEMANTIC_EFFECT-SEMANTIC-EFFECT-HTML-PREVIEW-",
      "HTML 预览 API 归 R06；R04 无预览服务",
      "R06")
defer("UI_BEHAVIOR-UI-BEHAVIOR-FILE-PREVIEW",
      "文件预览 UI 行为归 R06/R09（含 R09 真机）；R04 无预览",
      "R06/R09")
defer("SEMANTIC_EFFECT-SEMANTIC-EFFECT-FILE-HISTORY-",
      "文件历史产品面（快照/版本/恢复）归 R06；R04 已交付其冻结局接口"
      "（FileModificationRecord/FileChangeLog，T04 套件钉住）",
      "R06（产品集成）/R07")
defer("ROUTE_BEHAVIOR-BEHAVIOR-FILE-HISTORY-",
      "文件历史恢复路由归 R06；R04 冻结接口已交付（同上）",
      "R06")
defer("SEMANTIC_EFFECT-SEMANTIC-EFFECT-FS-",
      "docx/xlsx/base64 读取归 R06（T04 §8.3 登记为元数据-only 诚实结果，未迁移解码）",
      "R06")
defer("DESKTOP_BEHAVIOR-",
      "桌面行为路由（文件打开/复制/编辑/读取/监视/二进制写/遗留写/trash/编辑命令）"
      "归 R06 桌面接入面；R04 无桌面路由",
      "R06")
defer("BUILTIN_PLUGIN_TOOL-",
      "内置插件工具业务本体（beautify/office/media 各操作）归 R07 外围能力；"
      "R04 交付其宿主机制（worker RPC：单操作/期限/取消/资源许可/预算/沙盒绑定，"
      "T07+A14 套件钉住）",
      "R07")
defer("BUILTIN_PLUGIN_ADAPTER-",
      "内置插件适配器（jimeng CLI）归 R07；R04 交付 worker 宿主机制（同上）",
      "R07")
defer("PLUGIN_TOGGLE-",
      "插件启停产品面归 R07；R04 交付其机制映射（availability 面 disable/uninstall 语义，"
      "矩阵 A16 案例钉住）",
      "R07")
defer("SEMANTIC_EFFECT-SEMANTIC-EFFECT-DESK-",
      "desk 美化产品面（封面上传/预设/状态）归 R07；R04 交付 worker 宿主机制",
      "R07")

assert set(SHARE) | set(DEFERRED) == set(r04_leaves), (
    sorted((set(SHARE) | set(DEFERRED) ^ set(r04_leaves)))
)
assert not (set(SHARE) & set(DEFERRED))

leaves = []
for sid in sorted(r04_leaves):
    entry = r04_leaves[sid]
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
        cases, stage_share, later_share = SHARE[sid]
        leaves.append({
            **base,
            "basisKind": "stage_share_satisfied",
            "stageShare": stage_share,
            "laterShare": later_share,
            "evidenceRequired": EVIDENCE_NOTE,
            "evidenceCommandRefs": [MATRIX],
            "assertionContract": {
                "producerCommand": MATRIX,
                "evidencePath": CASES,
                "cases": [{"case": c, "expect": e} for c, e in cases],
            },
        })
    else:
        reason, later = DEFERRED[sid]
        leaves.append({
            **base,
            "basisKind": "deferred_to_later_stage",
            "stageShare":
                f"无 R04 叶专属门禁份额：{reason}；工具基础边界由基础场景 R04-A01..A16 与 "
                f"r04_tool_matrix 全矩阵门禁承担，无叶专属份额需要图钉",
            "laterShare": later,
            "evidenceRequired":
                "R04 不判本叶（DEFERRED，仍 REQUIRED）：验收归属 r00ExecutionStageIds 中的"
                "后续阶段（R06/R07/R08/R09），后续阶段图必须消费本叶余款",
            "evidenceCommandRefs": [],
        })

share_count = sum(1 for l in leaves if l["basisKind"] == "stage_share_satisfied")
deferred_count = sum(1 for l in leaves if l["basisKind"] == "deferred_to_later_stage")

stage_map = {
    "schemaVersion": 1,
    "resultVersion": "lingxi.xtask.verify-stage.v1",
    "stage": "R04",
    "defaultTimeoutSecs": 1200,
    "supplementalCoverageNote":
        "R04-T08 阶段图（2026-09-30）：本图由 R04_SCOPE_MATRIX.json 的补充义务驱动建立——"
        "SUP-01（ask 档审批策略面：sup01-ask-subagent-write-refused 案例在矩阵图钉，"
        "T03 套件全量钉住）、SUP-02（唯一授权面/单一 journal 写者：网关零授权逻辑零存储，"
        "全矩阵经同一链）、SUP-03（canonical 序列化消费点关闭：T01 套件+F1/F2 关闭证据，"
        "R03 图 r03_g07 十套件回归）、SUP-04（R03 laterShare 工具目录面：unmigrated 形态"
        "Future 诚实注册逐叶图钉）、SUP-05（R03 回归硬保护：r03_regression_gate 命令在"
        "本图内完整重跑 verify-stage R03）。分类："
        f"{share_count} stage_share_satisfied（R04 份额=工具基础机制层——目录/网关/权限/"
        "批准/原生执行器/worker/MCP 桥，案例由 r04_tool_matrix 生产者机器核验）+ "
        f"{deferred_count} deferred_to_later_stage（无 R04 叶专属门禁份额；验收归 "
        "R06/R07/R08/R09，仍 REQUIRED）。阶段中立 kinds 与 R03 图同构（份额叶必带 "
        "assertionContract，递延叶不得绑门禁命令）；R02/R03 图零改动。"
        "RR1 修复轮 G05 增量（2026-10-01，CLOSE-C01）：追加 R04-RR1-F01..F05 五个"
        "REQUIRED 场景与 r04_rr1_repair_suites 生产者（六集成套件+二十六单测=63 测试，"
        "逐测试归属 22 个五 F C-ID；CLOSE-C01..C04 为收口检查不入图）；原 16 A-ID、"
        "SUP01/03/05 与 124 叶结构零改动，只增不减。",
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
        # The R04-T08 acceptance matrix producer: full tool matrix
        # (7 families × permission contexts × routes × lifecycle), A15/A16
        # base scenarios, and the leaf-case evidence file the 124 R00-leaf
        # assertion contracts consume.
        "r04_tool_matrix": {
            "argv": ["bash", "scripts/rust-tauri/r04_t08_matrix.sh", "{EVIDENCE}/R04_MATRIX"],
            "timeoutSecs": 1800,
            "evidencePaths": [
                "{EVIDENCE}/R04_MATRIX/leaf-cases.json",
                "{EVIDENCE}/R04_MATRIX/matrix-counts.json",
                "{EVIDENCE}/R04_MATRIX/integration.log",
                "{EVIDENCE}/R04_MATRIX/summary.txt",
            ],
        },
        # SUP-05: the FULL R03 stage gate re-run on the final R04 candidate
        # (its own 13 commands, R00-leaf cross-checks and repair suites
        # included — nested under this gate's fresh evidence root).
        "r03_regression_gate": {
            "argv": ["cargo", "run", "--manifest-path", "rust/Cargo.toml", "--locked",
                      "-p", "xtask", "--", "verify-stage", "R03",
                      "--evidence", "{EVIDENCE}/R03_REGRESSION"],
            "timeoutSecs": 5400,
            "evidencePaths": ["{EVIDENCE}/R03_REGRESSION/verify-stage-result.json"],
        },
        # R04 RR1 repair round G05 (CLOSE-C01, 2026-10-01): the adversarial-
        # repair suite producer for findings F01–F05 (candidates G01..G04 =
        # 1692d2314/da15c4bd9/614fab1af/1285c3bf6). Pins the six new
        # integration suites (37 tests) and the twenty-six RR1 lib unit
        # tests EXACTLY, and owns every executed test by exactly one of the
        # 22 five-F C-IDs (漏 ID/过滤0/计数漂移/改名 = named gap, fail closed).
        "r04_rr1_repair_suites": {
            "argv": ["bash", "scripts/rust-tauri/r04_rr1_g05_repair_suites.sh",
                      "{EVIDENCE}/R04_RR1_REPAIR"],
            "timeoutSecs": 2400,
            "evidencePaths": [
                "{EVIDENCE}/R04_RR1_REPAIR/rr1-cases.json",
                "{EVIDENCE}/R04_RR1_REPAIR/summary.txt",
            ],
        },
    },
    "scenarios": [
        {"id": "R04-A01", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "r04_tool_matrix"]},
        {"id": "R04-A02", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "r04_tool_matrix"]},
        {"id": "R04-A03", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "r04_tool_matrix"]},
        {"id": "R04-A04", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace"]},
        {"id": "R04-A05", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "r04_tool_matrix"]},
        {"id": "R04-A06", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "r04_tool_matrix"]},
        {"id": "R04-A07", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "r04_tool_matrix"]},
        {"id": "R04-A08", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace"]},
        {"id": "R04-A09", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "r04_tool_matrix"]},
        {"id": "R04-A10", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "r04_tool_matrix"]},
        {"id": "R04-A11", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace"]},
        {"id": "R04-A12", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace"]},
        {"id": "R04-A13", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "r04_tool_matrix"]},
        {"id": "R04-A14", "requirement": "REQUIRED", "commandRefs": ["rust_test_workspace", "r04_tool_matrix"]},
        # The final-acceptance scenario carries the STANDARD battery too
        # (the R03-A15 pattern): fmt/clippy/check-contracts/check-boundaries
        # must be REFERENCED by a scenario or the runner never executes
        # them inside the gate (unreferenced commands are skipped).
        {"id": "R04-A15", "requirement": "REQUIRED", "commandRefs": [
            "r04_tool_matrix", "rust_test_workspace",
            "rust_fmt", "rust_clippy", "check_contracts", "check_boundaries"]},
        {"id": "R04-A16", "requirement": "REQUIRED", "commandRefs": ["r04_tool_matrix", "rust_test_workspace"]},
        # The stage's supplemental duties ride the same gate as first-class
        # REQUIRED scenarios (ADDITIVE — the 16 A-IDs above are frozen).
        {"id": "R04-SUP01", "requirement": "REQUIRED",
         "commandRefs": ["rust_test_workspace", "r04_tool_matrix"],
         "note": "R03-T06-O1 ask 子代理审批差距关闭：ApprovalService 生产策略面对 ask 档继承"
                 "（deny_on_prompt/allowHumanApproval=false）返回结构化 TOOL_APPROVAL_UNAVAILABLE；"
                 "sup01-ask-subagent-write-refused 案例在矩阵图钉，T03 套件全量钉住。"},
        {"id": "R04-SUP03", "requirement": "REQUIRED",
         "commandRefs": ["rust_test_workspace"],
         "note": "RR-T02 F1/F2 在 R04 消费点（T01/T02 canonical 参数摘要）关闭；F3/F4 未被"
                 "R04 消费（事件面）按登记携带；canonical 一致性由 kernel/protocol 套件与"
                 "check-contracts 回归。"},
        {"id": "R04-SUP05", "requirement": "REQUIRED",
         "commandRefs": ["r03_regression_gate", "rust_test_workspace"],
         "note": "R03 回归硬保护：完整 verify-stage R03（十修复套件+A15 矩阵+A16 种子机制"
                 "与全部 R02 定向链）在本图最终候选上整体重跑。"},
        # R04 RR1 repair round G05 (CLOSE-C01, 2026-10-01): the five
        # adversarial-repair findings enter the formal gate as first-class
        # REQUIRED scenarios (ADDITIVE — the 16 A-IDs + SUP01/03/05 above
        # are frozen; nothing was removed, re-graded or deferred). Each
        # finding's C-ID acceptance lives in the r04_rr1_repair_suites
        # producer's cid table (every executed test owned by exactly one
        # C-ID; 22 five-F C-IDs of the 26-check RR1 checklist — the four
        # CLOSE-C01..C04 are closeout checks executed by the orchestrator,
        # not stage-map scenarios), machine-mirrored by the xtask
        # r04_rr1_* map-pinning tests inside rust_test_workspace.
        {"id": "R04-RR1-F01", "requirement": "REQUIRED",
         "commandRefs": ["r04_rr1_repair_suites", "rust_test_workspace"],
         "note": "RR1-F01 容量预留先于 OS 派发（G01/1692d2314）：登记满零派发/屏障争名额"
                 "恰一次/启动后异常恰一次补偿/失败后同进程可复用——C01-C04 由生产者"
                 "cid 表逐案例钉住（套件 r04_t05_registry_capacity 9 用例 + live_slot 3 单测）。"},
        {"id": "R04-RR1-F02", "requirement": "REQUIRED",
         "commandRefs": ["r04_rr1_repair_suites", "rust_test_workspace"],
         "note": "RR1-F02 泵归属/reap 竞态/诚实回收（G02/da15c4bd9）：孙进程持端冻结结果并"
                 "关读端/reaped-undrained 窗口零失效组信号/双泵单一预算/压力稳态——C01-C04"
                 "（套件 r04_rr1_f02_reaper_cleanup 11 用例 + group_signal 2 单测）。"},
        {"id": "R04-RR1-F03", "requirement": "REQUIRED",
         "commandRefs": ["r04_rr1_repair_suites", "rust_test_workspace"],
         "note": "RR1-F03 清理未确认不虚构 137/exit（G03/614fab1af）：StopUnconfirmed 全链"
                 "（exec/PTY/run）保真、重复终止不升级、控制组真实状态、wait 错误=观察失败"
                 "——C01-C05（套件 r04_rr1_f03_stop_honesty 7 用例 + 2 单测）。"},
        {"id": "R04-RR1-F04", "requirement": "REQUIRED",
         "commandRefs": ["r04_rr1_repair_suites", "rust_test_workspace"],
         "note": "RR1-F04 PTY 混合分片字节级恰一次消费（G04/1285c3bf6）：混合 chunk 最小反例"
                 "（旧分块控制组保留）、空闲 poll 暂存不计丢、分片不变性属性组、真实 PTY 握手"
                 "恰一次——C01-C04（套件 r04_rr1_f04_pty_consumption 1 集成 + transcript 10 单测）。"},
        {"id": "R04-RR1-F05", "requirement": "REQUIRED",
         "commandRefs": ["r04_rr1_repair_suites", "rust_test_workspace"],
         "note": "RR1-F05 输出完整性按全流事实标注（G04/1285c3bf6）：滚动窗前缀丢失必标真、"
                 "超长单行字节安全保头尾、spill 封顶=partial 不得称 Full、写失败准确诊断、"
                 "资源有界——C01-C05（套件 r04_rr1_f05_output_integrity 7 + "
                 "r04_rr1_f05_spill_failure 2 + exectools 输出完整性 9 单测）。"},
    ],
    "supplementalLeafScenarios": leaves,
}

OUT.write_text(json.dumps(stage_map, indent=2, ensure_ascii=False) + "\n")
print(f"wrote {OUT} with {len(leaves)} supplemental leaves "
      f"({share_count} share + {deferred_count} deferred)")
