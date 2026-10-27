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
stage owns). The 9 leaves whose R00 registration is DUAL-STAGE (R04+R06 —
the read/write/edit natives and the six on-demand file-family shapes)
stay stage_share_satisfied (their R04 mechanism share is pinned by named
cases the matrix producer re-verifies; the R06 remainder has a real later
owner). The remaining 69 are deferred_to_later_stage (no R04 gating share
of their own; the R04 substrate they consume — ResourceAccess/ResourceRef,
the worker RPC, the native executors — is named in the share text but
never faked).

R05 RR3 F54 closeout (F54-CLOSEOUT-01, 2026-10-08): a per-assertion
semantic audit of M-01 found that some of the 46 exclusive leaves were
pinned to cases that do NOT actually prove the pinned assertion's
business semantics (tool discoverability is not callability; terminal
echo is not variable sharing; a connector handshake is not a read of a
named resource; a config-generation change is not persistence). The
closeout fixes each leaf according to its AUDIT category:
- full_original_behavior stays ONLY for leaves whose every original
  assertion has a real pinned case that proves it (6 leaves — the
  exec/write_stdin terminals and the MCP tool mechanism face; re-bindings
  and two NEW producer cases mcp-describe-no-side-effect /
  mcp-search-honest-availability correct the false legs),
- stage_share_satisfied + R00 ledger revision (execution_stage_ids +=
  R07/R08) for the 40 leaves whose remainder is a LATER-STAGE obligation
  with authoritative taskbook clauses (dev-tool bodies / management APIs
  / settings UI → R07, client/HTTP surfaces → R08; the ledger now carries
  the real owner, so the share has a real later holder).
No original assertion was deleted, no case or evidence path invented;
verify.rs semantics untouched.

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
FULL_EVIDENCE_NOTE = (
    "R04 独占叶（r00ExecutionStageIds 仅 R04）按 full_original_behavior 验收：每条 R00 "
    "原断言各钉专属具名案例，案例由 r04_tool_matrix 生产者经真实工具链路"
    "（注册表→统一网关→T03 策略/批准面→原生执行器/worker/MCP 桥，必要处经真实 run 链）"
    "逐案例机器核验（lingxi.leaf-case-results.v1，actual==图钉）——每条断言的案例即该断言 "
    "R04 机制面的真实 per-case 事实（R05 RR3 F54：独占叶不得以无人承接的 share 分类放行）"
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

# F54 closeout: the 6 R04-EXCLUSIVE leaves whose every original assertion
# has a real pinned case proving it — full_original_behavior with one case
# group per ORIGINAL R00 assertion, in assertion order. Each group is a list
# of (case, expect) pairs; the inline comment carries the per-assertion
# justification. Cases come ONLY from the real producer set (the 56 prior
# cases + the 2 F54 producer cases).
FULL = {}

def full(sid_prefix, groups, justification):
    matches = [sid for sid in r04_leaves if sid.startswith(sid_prefix)]
    assert len(matches) == 1, f"{sid_prefix}: expected one leaf, got {matches}"
    sid = matches[0]
    assert sid not in SHARE and sid not in FULL, sid
    # The groups are checked against the REAL R00 assertion count below
    # (after fsa_by_id is consulted in the emit loop); case names/expect
    # values are checked against the producer's record_case mirror in the
    # xtask map-pinning tests.
    FULL[sid] = (groups, justification)

def future_case(name):
    return [(f"future-tool-shape-{name}-discoverable-not-callable", 1)]

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
# — exec_command / write_stdin: R04-EXCLUSIVE leaves → full_original —
# F54 closeout: each ORIGINAL R00 assertion pinned to its own real matrix
# case, with the binding matched to the case's ACTUAL semantics (F54
# audit: the foreign-write case proves cross-session refusal, the close
# case proves refusal after exit — the former mapping had them swapped).
full("R00-T02-LA-8A3C87812B4F",
     [[("tool-exec-command-real-chain", 1)],
      [("tool-write-stdin-continuation", 1)],
      [("tool-exec-cancel-cleanup", 1)],
      [("semantics-failed-never-dispatched-receipt", 1)]],
     "断言0 输出与退出码正确=exec_command 真实链（结构化 argv/输出/退出码收据）；"
     "断言1 PTY 输入可续接=write_stdin 续写投递新输出；断言2 取消后进程清理=cancel "
     "cleanup 进程树；断言3 PathGuard/沙盒/批准失败阻止运行=越界写在 prepare 拒、"
     "dispatched=false 且无文件落地（不执行也不谎报）")
full("R00-T02-LA-C90F42576683",
     [[("tool-write-stdin-continuation", 1)],
      [("terminal-close-stops-terminal", 1)],
      [("tool-exec-command-real-chain", 1)],
      [("tool-write-stdin-foreign-writes", 0)]],
     "断言0 输入进入指定 PTY=续写投递；断言1 退出后拒绝写入=显式关闭后 late write "
     "诚实失败（not running）；断言2 状态和退出码一致=exec_command 真实链收据（argv/"
     "退出码如实）；断言3 终端不存在/已退出/会话不匹配拒绝，不写错进程=跨主体写入 "
     "0 成功（expect=0 图钉，所有权拒绝）")

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

# — the dev/scan tools (F54 closeout: the EXECUTION body belongs to R07) —
# R04's verified mechanism share is catalog honesty (Availability::Future,
# discoverable and NEVER callable — the real per-case fact the producer
# records); the tool body itself is a R07 obligation (the taskbook §3.2
# "后续业务工具尚未迁移时，不假装其已可执行" + R07 §1/T12 full-parity closure),
# now carried by the R00 ledger (execution_stage_ids += R07) and this share.
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
        f"R04 份额=目录诚实机制面：{name} 工具形态可注册可发现（Availability::Future），"
        "未迁移能力诚实拒调用（不伪装 available）；Rust 生产代码无该工具执行本体"
        "（旧栈 lib/tools/ 为现役实现，OPTIONAL_TOOL_NAMES 按需目录）",
        f"{name} 执行本体与完整业务交互=R07（现役产品能力全部接到 Rust 服务：R07 §1；"
        "逐功能真实闭环 R07-T12/R07-A23）；R04 已交付其消费的目录/网关/权限机制"
        "（F54 收口：登记错配修订，R00 台账 execution_stage_ids += R07）",
    )

# — run_tools: the batch script tool body belongs to R07; the per-call
# gateway/permission/receipt mechanism it consumes is R04's real share —
share("R00-T02-LA-0818574ABD43",
      [("gateway-each-call-independent-permission", 1),
       ("semantics-success-receipt-dispatched", 1),
       ("semantics-failed-never-dispatched-receipt", 1),
       ("matrix-lifecycle-disable-holes", 0)],
      "R04 份额=子调用的网关机制面：每调用独立权限判定（同一上下文 read 派发 write 拒）"
      "+统一收据（成功 dispatched/失败如实 dispatched=false）+停用目标全路由拒绝",
      "run_tools 脚本工具本体（子调用编排/摘要/失败不自动回滚）=R07（现役 OPTIONAL "
      "工具迁移，R07-T12 逐功能真实闭环）；R04 已交付其消费的网关/权限/收据机制")

# — the terminal WS frames (the PERSISTENT-TERMINAL mechanism is R04-T05's
# delivered share; the WS frame PROTOCOL belongs to R08's legacy-client
# integration) —
share("R00-T02-LA-4C35E6AEC6F7",
      [("terminal-tail-cursor-continuation", 1),
       ("tool-write-stdin-foreign-writes", 0)],
      "R04 份额=持续终端机制面：游标增量投递（按句柄续读新输出，真实 PTY 时序）"
      "+所有权拒绝（跨主体写入 0 成功，不接续错误进程）",
      "WS 帧协议面（terminalId/sinceSeq 尾读请求经 terminalWsBridge）=R08（旧 "
      "Electron 客户端接入、React 服务传输）；R04-T05 已交付 write_stdin 游标语义本体")
share("R00-T02-LA-CE69063550AA",
      [("terminal-snapshot-current-transcript", 1),
       ("tool-write-stdin-foreign-writes", 0)],
      "R04 份额=快照机制面：当前 transcript 尾部快照（第二次写入只见新输出，非重放）"
      "+会话身份不符拒绝（不送他人终端内容）",
      "WS 帧协议面（terminalWsBridge 快照请求帧）=R08（旧客户端接入）；R04-T05 已交付"
      "快照语义本体（游标读取）")
share("R00-T02-LA-CFD9F02BC6AA",
      [("terminal-close-stops-terminal", 1),
       ("tool-write-stdin-foreign-writes", 0)],
      "R04 份额=关闭机制面：显式关闭终止终端（有界可观察，Terminated/Exited 记录）"
      "+ID 不匹配拒绝（不谎报已停止）",
      "WS 帧协议面（close 请求的 killed/already_stopped 返回）=R08（旧客户端接入）；"
      "R04-T05 已交付关闭语义本体（supervisor 显式终止+关闭后写入诚实失败）")

# — the MCP tool family (the tool-body mechanism IS R04-T01/T02/T07's
# delivered share): full_original, one real case per original assertion —
# F54 closeout: bindings corrected to the case's ACTUAL semantics (the
# route-consistency case now ALSO compares both routes on the MCP target;
# the describe/search honesty legs get dedicated new producer cases).
full("R00-T02-LA-483E461BB59D",
     [[("mcp-tool-call-full-chain", 1), ("matrix-route-consistency", 1)],
      [("matrix-lifecycle-disable-holes", 0)],
      [("matrix-lifecycle-generation-refusals", 0)]],
     "断言0 同目标直调与目录调用权限一致=同一 MCP 目标经直调网关与目录 run 链两路"
     "授权并执行一致（F54 收口：路由一致性案例补 MCP 目标腿后的字面证明）+目录路由"
     "真实 run 链执行；断言1 禁用或过期目标不执行=disable 零洞（6 路由全拒，"
     "expect=0）；断言2 目录过期/目标失效不执行=代次更新旧句柄 TargetChanged 拒"
     "（expect=0）")
full("R00-T02-LA-8BCB8A749864",
     [[("mcp-describe-real-identity", 1)],
      [("mcp-describe-no-side-effect", 1)],
      [("matrix-lifecycle-uninstall-holes", 0)]],
     "断言0 schema 和真实执行参数一致=describe 命名空间身份从真实注册目标生成"
     "（同一 schema 源经 from_effective_arguments 校验执行参数）；断言1 无副作用="
     "描述前后目录代次不变（F54 补充生产者案例，专测只读路径）；断言2 目标消失/"
     "重名明确提示=uninstall 后解析/执行全拒（expect=0，不给错误 schema）")
full("R00-T02-LA-CEDC75156D33",
     [[("mcp-search-namespaced", 1), ("mcp-describe-real-identity", 1)],
      [("matrix-lifecycle-disable-holes", 0)],
      [("mcp-search-honest-availability", 1)]],
     "断言0 搜索项能被 describe/call 解析=命名空间查询命中真实 target 身份+该身份"
     "可 describe（真实 schema）；断言1 禁用项不可调用=disable 零洞（expect=0）；"
     "断言2 禁用/不可用目标不冒充可执行+无命中为空=F54 补充生产者案例（搜索诚实："
     "停用项仍列出但标 Disabled，无命中返回空）")
full("R00-T02-LA-15AD6ED13B4D",
     [[("mcp-tool-call-full-chain", 1)],
      [("mcp-tool-permission-face", 1)]],
     "断言0 按 arguments 执行指定连接器工具并返回结果或错误=经统一网关真实 run 链"
     "执行（收据 dispatched）；断言1 权限不足明确错误=read_only 档 gateway_policy_denied "
     "拒绝腿（不呈现成功结果）")

# — the MCP connector MANAGEMENT family: the management API (persisted
# connector config, start/stop/delete/update/batch, OAuth, agent binding,
# state/apps/defer/enable surfaces) is a R07-T08 obligation ("迁移…工具
# 启停…等真实API"; "设置页不是迁移后的空壳") with the settings-UI
# projection in R08; R04's delivered share is the connector MECHANISM
# face (rmcp handshake/sync/refresh/lifecycle + the T03 permission plane
# + the preauthorization lifecycle). F54 closeout moves these leaves to
# stage_share_satisfied with the R00 ledger now carrying the R07/R08
# remainder (a full classification here was a false completeness claim:
# no management API exists in the Rust service yet).
share("R00-T02-LA-0199B843D759",
      [("mcp-connector-register-handshake", 1), ("mcp-tool-permission-face", 1)],
      "R04 份额=连接机制面：真实 initialize 握手+协议协商+清单入目录（失败响亮拒绝"
      "不注册），权限面三档裁定",
      "连接器配置持久化管理 API（保存/返回公开配置与当下状态/异步启动/失败错误状态）"
      "=R07-T08（旧栈 server/routes/mcp.ts）+R08（UI 投影回归）；R04-T07 到期="
      "transport 接入与目录同步")
share("R00-T02-LA-04A6A2BD1547",
      [("mcp-connector-catalog-sync", 1)],
      "R04 份额=资源内容机制面：连接器内容经真实同步入目录；内容块映射按类型验证，"
      "远程 URI 永不铸本地引用（mcpbridge 单测钉住）",
      "按 uri 的连接器资源读取 API（缺 uri/无内容明确拒绝）=R07-T08（旧栈 readResource "
      "路由迁移）；R04 无 resources/read 端点")
share("R00-T02-LA-18EFB2D9D5FD",
      [("mcp-connector-register-handshake", 1),
       ("matrix-lifecycle-generation-refusals", 0)],
      "R04 份额=启动机制面：启动=真实握手（connect_count 审计，连接槽宿主所有）；"
      "失效句柄 TargetChanged 拒（expect=0）",
      "已保存连接器的启动管理入口与新运行状态返回=R07-T08+R08；R04 已交付启动的"
      "机制本体（register=握手+注册）")
share("R00-T02-LA-E9C7A48CADC4",
      [("mcp-connector-register-handshake", 1),
       ("matrix-lifecycle-uninstall-holes", 0)],
      "R04 份额=连接生命周期机制面：连接/断开归宿主（握手生命周期审计），移除后"
      "解析/执行全拒零残留（expect=0）",
      "停止指定连接器的管理 API 与新运行状态返回=R07-T08+R08；R04 已交付 disconnect/"
      "uninstall 机制本体")
share("R00-T02-LA-2D194C1684BC",
      [("matrix-lifecycle-uninstall-holes", 0), ("a16-history-preserved", 1)],
      "R04 份额=移除机制面：注册表 uninstall 语义（名称/代次/句柄全失效），历史收据"
      "如实保留（不掩盖）",
      "连接器删除管理 API（配置及运行状态移除、删除结果）=R07-T08+R08")
share("R00-T02-LA-F4DA2AFCB72B",
      [("matrix-lifecycle-generation-refusals", 0), ("a16-history-preserved", 1)],
      "R04 份额=更新机制面：清单更新代次上升旧句柄死亡（expect=0），历史收据如实",
      "连接器配置更新管理 API 与更新结果=R07-T08+R08")
share("R00-T02-LA-48B0C7A7453E",
      [("mcp-connector-catalog-sync", 1), ("a15-structure-violation-refused", 1)],
      "R04 份额=校验机制面：多连接器注册=多次真实握手+清单同步；结构违规引用被拒"
      "（无效输入明确错误）",
      "批量导入管理 API（先校验全部再写入/坏行拒整批/成功项异步启动逐项回报）=R07-T08+R08")
share("R00-T02-LA-21B3F4DC9140",
      [("mcp-connector-catalog-sync", 1),
       ("matrix-lifecycle-generation-refusals", 0)],
      "R04 份额=refresh 机制面：重列+注册/更新/消失同步（R04-A13 断线恰一次新握手"
      "不重放），代次失效句柄拒绝",
      "指定连接器的刷新管理入口与状态返回=R07-T08（refresh_mcp_server 机制本体已由 "
      "R04-T07 套件真实测试交付）")
share("R00-T02-LA-240E200EF440",
      [("mcp-tool-permission-face", 1), ("matrix-lifecycle-disable-holes", 0)],
      "R04 份额=工具授权机制面：T03 三档裁定+停用零洞（expect=0，配置停用后明确拒绝）",
      "Agent↔连接器绑定配置管理 API（更新并返回配置）=R07-T08+R08")
share("R00-T02-LA-2DA782C5C7B9",
      [("mcp-tool-permission-face", 1), ("approval-reject-zero-dispatch", 0)],
      "R04 份额=授权前置机制面：连接器工具调用恒经 T03 权限面三档裁定，拒绝后零派发"
      "（expect=0，不呈现成功结果）",
      "连接器 OAuth 授权流程（生成授权资料/回调/轮询作内部步骤）=R07-T08（旧栈 "
      "oauth/callback+poll 路由迁移）；依据 R04_PROMPT §3.2「不迁移…真实凭证/OAuth 体系」")
share("R00-T02-LA-E7F852F9BF60",
      [("mcp-tool-permission-face", 1), ("approval-reject-zero-dispatch", 0)],
      "R04 份额=授权未完成不可执行机制面：权限面拒绝语义+拒绝零派发",
      "OAuth 等待取消 API（结束在途往返、已保存凭证不变）=R07-T08")
share("R00-T02-LA-C70E819F8DA4",
      [("mcp-tool-permission-face", 1),
       ("preauthorization-single-session-scoped", 1)],
      "R04 份额=凭证生命周期机制面：授权未完成调用不可执行；预授权单次消耗+会话作用域"
      "+不持久化（秘密不残留不下发）",
      "OAuth 凭证清除与公开状态返回 API=R07-T08")
share("R00-T02-LA-7FF8D4E48BC9",
      [("mcp-tool-call-full-chain", 1), ("mcp-tool-permission-face", 1)],
      "R04 份额=工具调用机制面：命名空间 target 解析+网关真实执行+权限面裁定",
      "连接器应用（apps）launch API（把 launchInput 交给指定连接器工具）=R07-T08"
      "（旧栈 /apps 域）；R04 无 apps 语义")
share("R00-T02-LA-DD47275AC5AE",
      [("mcp-connector-catalog-sync", 1)],
      "R04 份额=状态数据机制面：同步报告（协商协议/server 身份/registered 数）与 "
      "is_connected 真实可读",
      "按 agent 配置合成的 MCP state API（合入内置工具延迟加载开关）=R07-T08+R08")
share("R00-T02-LA-E1FBE1A59BC6",
      [("mcp-connector-catalog-sync", 1)],
      "R04 份额=连接器状态数据机制面：真实握手同步后状态可读",
      "MCP apps 列表 API（返回应用及连接器状态/未初始化 503）=R07-T08（旧栈 /apps）")
share("R00-T02-LA-BB1BB3A9C5F4",
      [("catalog-face-lists-mcp-tools-with-permission", 1),
       ("matrix-lifecycle-disable-holes", 0)],
      "R04 份额=按需目录机制面：Deferred availability 可发现、按需解析经同一网关，"
      "停用面不可调用（expect=0）",
      "deferEnabled/deferThreshold/builtinDeferEnabled 独立更新的设置持久化 API=R07-T08"
      "（旧栈 /settings/defer）+R08")
share("R00-T02-LA-FAE7503D0D0F",
      [("matrix-lifecycle-disable-holes", 0), ("a16-alias-route-covered", 1)],
      "R04 份额=启停映射机制面：availability 禁用映射全路由零洞（expect=0），停用"
      "别名路由同样被拒（不冒充可用）",
      "MCP 全局开关设置持久化与合并连接状态 API=R07-T08+R08")

# — the MCP settings-page UI leaf: the UI projection is R07-T08 (management
# APIs) + R08 (regression of the existing product UI); R04's real share is
# the data-mechanism face those seven original assertions consume —
share("R00-T02-LA-B4BB2438E855",
      [("catalog-face-lists-mcp-tools-with-permission", 1),
       ("mcp-connector-register-handshake", 1),
       ("mcp-tool-permission-face", 1),
       ("preauthorization-single-session-scoped", 1)],
      "R04 份额=设置页消费的数据机制面：目录快照列出 MCP 工具与权限契约、真实 "
      "initialize 握手注册（添加/批量=多次注册的机制）、T03 三档授权面、预授权"
      "会话作用域（OAuth 生命周期机制）",
      "MCP 连接器设置页 UI 投影与管理 API=R07-T08（「迁移…工具启停…等真实API；"
      "设置页不是迁移后的空壳」）+R08（全产品现有界面回归）；依据 R04_PROMPT §3.2"
      "「不要求新建完整产品 UI」——逐动作 UI 断言（loading/toast/开关）非 R04 到期")

# — the security-settings UI leaf: same split (preference/sandbox/proxy
# APIs = R07-T08, UI regression = R08) —
share("R00-T02-LA-DA5AD5C40098",
      [("permission-face-modes-verifiable", 1), ("matrix-permission-consistency", 1)],
      "R04 份额=安全设置消费的模式/权限数据面：三档模式真实可设置可读取，全工具族×"
      "权限格一致生效",
      "沙箱/检查点/代理设置页 UI 与偏好持久化 API（写入全局 preferences/代理广播）"
      "=R07-T08（权限、偏好真实 API）+R08（界面回归）；依据 R04_PROMPT §3.2")

# — the permission-mode / plan-mode family: the mode ADJUDICATION face is
# R04-T03's delivered obligation (three modes settable/readable and
# governing the real policy); the management API projection (scopes,
# defaults, preference persistence, exact response bodies) is R07-T08
# with the R08 client regression —
share("R00-T02-LA-D01766AF4475",
      [("permission-face-modes-verifiable", 1),
       ("sup01-ask-subagent-write-refused", 1)],
      "R04 份额=权限裁决机制面（T03 到期）：三档模式真实设置/读取并决定裁决；ask 档"
      "结构化拒绝（副作用一致，deny_on_prompt）",
      "setSessionPermissionMode 管理 API 投影（body.mode/返回 mode/accessMode）=R07-T08"
      "（权限管理真实 API）+R08（旧客户端接入）")
share("R00-T02-LA-25C4FF66FEE5",
      [("permission-face-modes-verifiable", 1)],
      "R04 份额=模式读取机制面：会话真实状态可读（读取不改权限）",
      "新会话默认权限偏好（未设置时 ASK）的读取 API=R07-T08（偏好管理）")
share("R00-T02-LA-8D3CEB6133E1",
      [("permission-face-modes-verifiable", 1)],
      "R04 份额=模式设置机制面：模式设置经真实面生效",
      "默认权限偏好的校验/写入/持久化 API（返回保存后 permissionMode）=R07-T08")
share("R00-T02-LA-0B669B30C854",
      [("permission-face-modes-verifiable", 1),
       ("sup01-ask-subagent-write-refused", 1)],
      "R04 份额=会话级模式设置/拒绝机制面：模式设置与结构化拒绝真实",
      "按当前会话/待新建会话/指定会话/全局作用域的设置 API 与冲突错误=R07-T08+R08"
      "（Rust 当前仅会话级面；作用域与偏好持久化为管理 API 面）")
share("R00-T02-LA-5E61048F19B9",
      [("permission-face-modes-verifiable", 1)],
      "R04 份额=模式读取机制面：读取面真实（不改变权限）",
      "mode/accessMode/defaultMode 读取 API=R07-T08")
share("R00-T02-LA-A336F79E964D",
      [("permission-face-modes-verifiable", 1),
       ("matrix-lifecycle-disable-holes", 0)],
      "R04 份额=会话模式切换机制面：plan=read_only 预设，同一权限模式面真实可设置"
      "（开关两向=启停面零洞）",
      "setPlanMode 切换 API（!!enabled 语义/返回最新状态）=R07-T08（旧栈 planMode 是"
      "权限模式预设，core/session-permission-mode.ts；API 面随偏好管理迁移）")
share("R00-T02-LA-BC2FBD618278",
      [("permission-face-modes-verifiable", 1), ("a16-history-preserved", 1)],
      "R04 份额=模式读取面：读取真实且不改状态（历史收据如实保留）",
      "planMode/permissionMode/accessMode 读取 API=R07-T08")
share("R00-T02-LA-67256417FB2B",
      [("preauthorization-single-session-scoped", 1),
       ("approval-duplicate-idempotent", 1)],
      "R04 份额=预授权机制面：整键匹配/单次使用/会话作用域/跨会话不可见，经真实 "
      "gate 消费（不持久化）",
      "会话权限授予管理 API（校验 sessions.write 范围/仅本次会话放行 capability 的 "
      "HTTP 面）=R07-T08（旧栈 /session-permissions）")

# — the confirm/approval BODY_EFFECT and slash-command leaves: the
# ConfirmStore DECISION mechanism is R04-T03's delivered obligation; the
# event broadcast and the HTTP/slash surfaces are client-entry projections
# (R08 legacy business API / R07-T09 CLI) —
share("R00-T02-LA-EC033184BA37",
      [("approval-answer-executes-once", 1), ("approval-duplicate-idempotent", 1)],
      "R04 份额=审批决策机制面（T03 到期）：ConfirmStore 标记 confirmed+待批动作恰"
      "执行一次+重复决策 AlreadySettled no-op+越权/异会话拒绝",
      "confirmation_resolved 事件广播接线与确认 API 投影（无效 action 400/已处理 404/"
      "越权 403）=R08（旧业务 API 兼容；旧栈 server/routes/confirm.ts+engine 事件面）")
share("R00-T02-LA-EF43ADCE19A1",
      [("approval-reject-zero-dispatch", 0), ("approval-duplicate-idempotent", 1)],
      "R04 份额=拒绝机制面：rejected 后零派发（expect=0）+已处理不重复决策",
      "confirmation_resolved 事件广播接线与拒绝 API 投影（拒绝后再次提交不得继续）"
      "=R08（旧业务 API 兼容）")
share("R00-T02-LA-75C0AE981505",
      [("approval-answer-executes-once", 1), ("approval-duplicate-idempotent", 1)],
      "R04 份额=ConfirmStore 决策机制面：confirmed 继续原待批动作并按收据证明恰一次"
      "执行，ID 缺失/已处理确定性 no-op",
      "斜杠命令 /confirm 入口（dispatcher 来源权限检查+状态广播）=CLI 归 R07-T09、"
      "客户端归 R08（旧栈 core/slash-commands/bridge-commands）")
share("R00-T02-LA-CD1524CC7DC3",
      [("approval-reject-zero-dispatch", 0), ("approval-duplicate-idempotent", 1)],
      "R04 份额=拒绝机制面：rejected 原待批动作不执行（零派发）+已处理不再决策",
      "斜杠命令 /reject 入口=CLI 归 R07-T09、客户端归 R08")

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

assert set(SHARE) | set(FULL) | set(DEFERRED) == set(r04_leaves), (
    sorted((set(SHARE) | set(FULL) | set(DEFERRED) ^ set(r04_leaves)))
)
assert not (set(SHARE) & set(DEFERRED))
assert not (set(FULL) & set(DEFERRED))
assert not (set(SHARE) & set(FULL))
# F54 closeout invariant: every FULL leaf is R04-EXCLUSIVE and every share
# leaf has a later-stage owner in the (revised) R00 ledger — a share with
# no later stage owner is the unowned remainder; a full leaf whose R00
# registration names a later stage would contradict the ledger.
for sid in FULL:
    assert r04_leaves[sid]["execution_stage_ids"] == ["R04"], sid
for sid in SHARE:
    assert any(s != "R04" for s in r04_leaves[sid]["execution_stage_ids"]), sid
assert len(FULL) == 6, f"expected 6 F54-closeout full leaves, got {len(FULL)}"

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
    elif sid in FULL:
        groups, _justification = FULL[sid]
        assertions = fsa_entry["assertions"]
        assert len(groups) == len(assertions), (
            f"{sid}: {len(groups)} case groups for {len(assertions)} original assertions"
        )
        # Deduplicate leaf-wide (each group holds distinct case names) and
        # keep first-seen order: the pinned set must equal the assigned set.
        pinned = []
        seen = set()
        for group in groups:
            for case, _expect in group:
                assert case not in seen, f"{sid}: case {case} reused in-leaf"
                seen.add(case)
                pinned.append((case, _expect))
        leaves.append({
            **base,
            "basisKind": "full_original_behavior",
            "stageShare": "",
            "laterShare": "",
            "evidenceRequired": FULL_EVIDENCE_NOTE,
            "evidenceCommandRefs": [MATRIX],
            "originalAssertionCases": [[c for c, _e in group] for group in groups],
            "assertionContract": {
                "producerCommand": MATRIX,
                "evidencePath": CASES,
                "cases": [{"case": c, "expect": e} for c, e in pinned],
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
full_count = sum(1 for l in leaves if l["basisKind"] == "full_original_behavior")
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
        f"{full_count} full_original_behavior（R04-EXCLUSIVE 独占叶——r00ExecutionStageIds "
        "仅 R04；R05 RR3 F54 收口 2026-10-08：仅保留每条原断言都有真实案例证明的叶——"
        "终端执行链与 MCP 工具机制面，绑定按案例实际语义逐一核对并补 2 个生产者案例，"
        "不得以案例语义不符的虚假绑定放行）+ "
        f"{share_count} stage_share_satisfied（R00 登记多阶段叶的 R04 份额=工具基础机制层——"
        "目录/网关/权限/批准/原生执行器/worker/MCP 桥；其中 40 叶为 F54 登记错配修订，"
        "余款按任务书条款归 R07（dev 工具本体/管理 API/设置页）或 R08（客户端/HTTP 面），"
        "案例由 r04_tool_matrix 生产者机器核验）+ "
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
      f"({full_count} full + {share_count} share + {deferred_count} deferred)")
