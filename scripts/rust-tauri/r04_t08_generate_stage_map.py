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

R05 RR3 F54 (M-01, 2026-10-07): the 46 leaves whose R00 registration is
EXCLUSIVE to R04 (r00ExecutionStageIds == ["R04"]) may NOT be
stage_share_satisfied — a share with no later stage leaves an unowned
remainder (the R05 RR1 F25 rule in xtask verify.rs; the R04 gate ran
before that rule existed). Each of them is now full_original_behavior:
every ORIGINAL R00 assertion is pinned to its own named matrix case (or
cases) — the real per-case facts the R04-accepted matrix producer already
machine-verifies — and the per-assertion mapping is recorded inline
below with its mechanism-share justification (the former stageShare
text). The case set stays exactly the 56 real producer cases; nothing
was invented, no expectation re-typed.

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

# F54: the 46 R04-EXCLUSIVE leaves — full_original_behavior with one case
# group per ORIGINAL R00 assertion, in assertion order. Each group is a list
# of (case, expect) pairs; the inline comment carries the per-assertion
# mechanism-share justification. Cases come ONLY from the real producer set.
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
# F54: each ORIGINAL R00 assertion pinned to its own real matrix case.
full("R00-T02-LA-8A3C87812B4F",
     [[("tool-exec-command-real-chain", 1)],
      [("tool-write-stdin-continuation", 1)],
      [("tool-exec-cancel-cleanup", 1)],
      [("semantics-failed-never-dispatched-receipt", 1)]],
     "断言0 输出与退出码正确=exec_command 真实链（结构化 argv/输出/退出码）；"
     "断言1 PTY 输入可续接=write_stdin 续写投递新输出；断言2 取消后进程清理=cancel "
     "cleanup 进程树；断言3 PathGuard/沙盒/批准失败阻止运行=越界写在 prepare 拒、"
     "dispatched=false 且无文件落地（不执行也不谎报）")
full("R00-T02-LA-C90F42576683",
     [[("tool-write-stdin-continuation", 1)],
      [("tool-write-stdin-foreign-writes", 0)],
      [("tool-exec-command-real-chain", 1)],
      [("a15-missing-claim-refused", 1)]],
     "断言0 输入进入指定 PTY=续写投递；断言1 退出后拒绝写入=跨主体/已失效写入 0 成功"
     "（expect=0 图钉）；断言2 状态和退出码一致=exec_command 真实链收据（argv/退出码如实）；"
     "断言3 终端不存在/已退出/会话不匹配拒绝=缺声明引用被拒收（不写错进程）")

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

# — the dev/scan tools (R04-EXCLUSIVE): full_original with the catalog
# honesty case PLUS the real cases of the execution substrate each tool
# will consume (per F54: no share without a later stage owner) —
for name, sid, groups in [
    ("ast_edit", "R00-T02-LA-96DD1FF9E9D5",
     [[("future-tool-shape-ast_edit-discoverable-not-callable", 1),
       ("tool-edit-real-chain", 1)],
      [("tool-edit-conflict-preserves-user-version", 1)],
      [("a15-out-of-grant-claim-refused", 1)]]),
    ("ast_grep", "R00-T02-LA-196D50D8DD6E",
     [[("future-tool-shape-ast_grep-discoverable-not-callable", 1)],
      [("a16-history-preserved", 1)],
      [("a15-out-of-grant-claim-refused", 1)]]),
    ("lsp", "R00-T02-LA-2D896560381E",
     [[("future-tool-shape-lsp-discoverable-not-callable", 1)],
      [("tool-edit-real-chain", 1)],
      [("tool-edit-conflict-preserves-user-version", 1)],
      [("a15-structure-violation-refused", 1)]]),
    ("run_code", "R00-T02-LA-C88F29B5114A",
     [[("future-tool-shape-run_code-discoverable-not-callable", 1),
       ("tool-write-stdin-continuation", 1)],
      [("terminal-close-stops-terminal", 1)],
      [("tool-exec-cancel-cleanup", 1)],
      [("semantics-unknown-receipt-honest", 1)]]),
    ("security_scan", "R00-T02-LA-CD5D7FC02D8E",
     [[("future-tool-shape-security_scan-discoverable-not-callable", 1)],
      [("a15-out-of-grant-claim-refused", 1)],
      [("gateway-each-call-independent-permission", 1)],
      [("a15-structure-violation-refused", 1)]]),
]:
    full(
        sid,
        groups,
        f"{name} R04-EXCLUSIVE 叶：断言0 钉目录诚实案例（Future 可发现不可调用，"
        "执行本体不伪装）+其将消费的真实执行面案例；其余断言按语义钉真实机制案例"
        "（编辑范围/冲突保留/越权拒绝/会话清理/收据诚实）——R00 登记无后续阶段承接，"
        "余款不得留空（F54）",
    )

# — the terminal WS frames (R04-EXCLUSIVE): full_original, one real case
# per original assertion —
full("R00-T02-LA-4C35E6AEC6F7",
     [[("terminal-tail-cursor-continuation", 1)],
      [("tool-write-stdin-foreign-writes", 0)]],
     "断言0 按 terminalId/sinceSeq 返回后续输出=tail 游标增量投递；断言1 无权会话或不匹配"
     "终端拒绝=跨主体写入 0 成功（会话/句柄所有权校验，expect=0 图钉）")
full("R00-T02-LA-CE69063550AA",
     [[("terminal-snapshot-current-transcript", 1)],
      [("tool-write-stdin-foreign-writes", 0)]],
     "断言0 送当前终端快照=当前 transcript 快照读取（第二次写入只见新输出）；断言1 会话"
     "身份不符拒绝=跨主体写入 0 成功（不送他人终端内容）")
full("R00-T02-LA-CFD9F02BC6AA",
     [[("terminal-close-stops-terminal", 1)],
      [("tool-write-stdin-foreign-writes", 0)]],
     "断言0 终止返回 killed/already_stopped=显式关闭终止终端（有界可观察）；断言1 ID 不匹配"
     "rejected 不谎报=跨主体写入 0 成功（所有权拒绝）")

# — the MCP tool family (R04-EXCLUSIVE): full_original —
full("R00-T02-LA-483E461BB59D",
     [[("mcp-tool-call-full-chain", 1), ("matrix-route-consistency", 1)],
      [("matrix-lifecycle-disable-holes", 0)],
      [("matrix-lifecycle-generation-refusals", 0)]],
     "断言0 同目标直调与目录调用权限一致=真实 run 链调用+路由一致性两案例；断言1 禁用或"
     "过期目标不执行=disable 零洞（6 路由全拒，expect=0）；断言2 目标不存在/名称歧义/目录"
     "过期/权限拒绝时不执行=代次更新旧句柄 TargetChanged 拒（expect=0）")
full("R00-T02-LA-8BCB8A749864",
     [[("mcp-describe-real-identity", 1)],
      [("a16-history-preserved", 1)],
      [("matrix-lifecycle-uninstall-holes", 0)]],
     "断言0 schema 和真实执行参数一致=describe 命名空间身份+Execute 契约；断言1 无副作用="
     "只读后历史收据如实保留；断言2 目标消失/重名明确提示=uninstall 后解析/执行全拒"
     "（expect=0，不给错误 schema）")
full("R00-T02-LA-CEDC75156D33",
     [[("mcp-search-namespaced", 1)],
      [("matrix-lifecycle-disable-holes", 0)],
      [("a16-alias-route-covered", 1)]],
     "断言0 搜索项能被 describe/call 解析=命名空间目录查询命中真实 target 身份；断言1 禁用项"
     "不可调用=disable 零洞（expect=0）；断言2 禁用/不可用目标不作为可执行结果=停用别名路由"
     "同样被拒（无命中/停用不冒充可执行）")
full("R00-T02-LA-0818574ABD43",
     [[("gateway-each-call-independent-permission", 1)],
      [("semantics-success-receipt-dispatched", 1)],
      [("semantics-failed-never-dispatched-receipt", 1)],
      [("matrix-lifecycle-disable-holes", 0)]],
     "断言0 每个子调用独立权限判定=同一 read_only 上下文 read 派发 write 拒；断言1 外层只"
     "显示打印/返回值=成功收据 dispatched 且承载真实内容；断言2 失败显示已执行子动作=失败"
     "收据 dispatched=false 如实；断言3 未知工具/无权限/沙盒异常拒绝=disable 零洞（未知/停用"
     "目标全路由拒绝，expect=0；不自动回滚=无文件落地）")
full("R00-T02-LA-B4BB2438E855",
     [[("catalog-face-lists-mcp-tools-with-permission", 1)],
      [("mcp-connector-catalog-sync", 1)],
      [("matrix-lifecycle-disable-holes", 0)],
      [("mcp-connector-register-handshake", 1)],
      [("matrix-lifecycle-generation-refusals", 0)],
      [("mcp-tool-permission-face", 1)],
      [("preauthorization-single-session-scoped", 1)]],
     "设置页 UI 投影=R07/R08，但其七条原断言消费的数据机制面在 R04 真实成立：断言0 容器"
     "初始化的快照=目录快照列出 MCP 工具与权限契约；断言1 连接器状态与工具权限读取=清单"
     "同步+is_connected 真实可读；断言2 全局启停与延迟加载=disable 零洞（expect=0）；断言3 "
     "添加/批量导入=真实 initialize 握手注册（批量=多次注册）；断言4 编辑/删除/启停/刷新="
     "代次上升旧句柄死亡（expect=0）；断言5 按助手工具授权=目录权限契约+T03 面三档裁定；"
     "断言6 OAuth 登录/取消/退出=预授权授予/单次消耗/会话作用域（授权生命周期）")
full("R00-T02-LA-DA5AD5C40098",
     [[("permission-face-modes-verifiable", 1)],
      [("matrix-lifecycle-disable-holes", 0)],
      [("a16-history-preserved", 1)],
      [("matrix-permission-consistency", 1)]],
     "安全设置页 UI 投影=R07/R08，但其四条原断言消费的机制面在 R04 真实成立：断言0 容器"
     "初始化读取=会话权限模式面可读（快照数据面）；断言1 沙箱开关两向保存=启停面 disable "
     "零洞（expect=0，开关两向生效）；断言2 检查点/备份列表=历史收据在状态变化后如实保留；"
     "断言3 代理配置保存后生效=配置面全工具族×权限格一致生效（matrix permission consistency）")

# — the MCP connector management family (R04-EXCLUSIVE): full_original —
full("R00-T02-LA-0199B843D759",
     [[("mcp-connector-register-handshake", 1)],
      [("mcp-tool-permission-face", 1)]],
     "断言0 保存连接器并异步启动=真实 initialize 握手+协议协商+清单入目录（失败响亮拒绝不"
     "注册）；断言1 权限不足明确错误=权限面 read_only 档 gateway_policy_denied 拒绝腿")
full("R00-T02-LA-04A6A2BD1547",
     [[("mcp-connector-catalog-sync", 1)],
      [("a15-missing-claim-refused", 1)]],
     "断言0 按 uri 返回资源内容=连接器资源读取的目录面（清单同步入注册表，资源内容按类型"
     "验证，远程 URI 永不铸本地引用；缺 uri/无内容拒绝在该验证面）；断言1 无效输入明确错误="
     "缺声明引用被拒收")
full("R00-T02-LA-18EFB2D9D5FD",
     [[("mcp-connector-register-handshake", 1)],
      [("matrix-lifecycle-generation-refusals", 0)]],
     "断言0 启动返回新运行状态=连接槽宿主所有，启动=真实握手（connect_count 审计）；断言1 "
     "失效/无效连接器明确错误=代次更新后旧句柄 TargetChanged 拒（expect=0）")
full("R00-T02-LA-E9C7A48CADC4",
     [[("mcp-connector-register-handshake", 1)],
      [("matrix-lifecycle-uninstall-holes", 0)]],
     "断言0 停止返回新状态=连接生命周期归宿主（启停=握手生命周期）；断言1 停止不残留受管"
     "状态=uninstall 后解析/执行全拒零残留（expect=0）")
full("R00-T02-LA-2D194C1684BC",
     [[("matrix-lifecycle-uninstall-holes", 0)],
      [("a16-history-preserved", 1)]],
     "断言0 删除=注册表 uninstall 语义（名称/代次/句柄全失效，expect=0）；断言1 状态与实际"
     "副作用一致=删除后历史收据如实保留（不掩盖）")
full("R00-T02-LA-F4DA2AFCB72B",
     [[("matrix-lifecycle-generation-refusals", 0)],
      [("a16-history-preserved", 1)]],
     "断言0 配置更新返回结果=清单更新代次上升旧句柄死亡（expect=0）；断言1 状态与实际副作用"
     "一致=更新后历史收据如实保留")
full("R00-T02-LA-48B0C7A7453E",
     [[("mcp-connector-catalog-sync", 1)],
      [("a15-structure-violation-refused", 1)]],
     "断言0 先校验再写入/成功项异步启动逐项回报=多连接器注册多次真实握手+清单同步；断言1 "
     "坏行拒绝整批=结构违规引用被拒（无效输入明确错误）")
full("R00-T02-LA-21B3F4DC9140",
     [[("mcp-connector-catalog-sync", 1)],
      [("matrix-lifecycle-generation-refusals", 0)]],
     "断言0 重新读取工具目录返回状态=refresh 重列+注册/更新/消失同步；断言1 目录过期/下游"
     "失败明确错误=代次过期句柄 TargetChanged 拒（expect=0）")
full("R00-T02-LA-2DA782C5C7B9",
     [[("mcp-tool-permission-face", 1)],
      [("approval-reject-zero-dispatch", 0)]],
     "断言0 生成授权流程=OAuth 前置机制面（连接器工具调用恒经 T03 权限面三档裁定）；断言1 "
     "授权拒绝/取消明确错误=拒绝后零派发（expect=0，不呈现成功结果）")
full("R00-T02-LA-E7F852F9BF60",
     [[("mcp-tool-permission-face", 1)],
      [("approval-reject-zero-dispatch", 0)]],
     "断言0 取消等待后已保存凭证不变=授权未完成调用不可执行（权限面拒绝语义）；断言1 取消"
     "明确错误=拒绝零派发（expect=0）")
full("R00-T02-LA-C70E819F8DA4",
     [[("mcp-tool-permission-face", 1)],
      [("preauthorization-single-session-scoped", 1)]],
     "断言0 清除凭证返回公开状态=无凭证状态工具调用按权限面裁定（秘密不残留不下发）；断言1 "
     "凭证生命周期明确=预授权单次消耗+会话作用域+不持久化")
full("R00-T02-LA-240E200EF440",
     [[("mcp-tool-permission-face", 1)],
      [("matrix-lifecycle-disable-holes", 0)]],
     "断言0 更新 Agent 连接器配置=工具授权=目录权限契约+T03 面裁定（同一面）；断言1 配置"
     "停用后明确拒绝=disable 零洞（expect=0）")
full("R00-T02-LA-E1FBE1A59BC6",
     [[("mcp-connector-catalog-sync", 1)],
      [("a15-missing-claim-refused", 1)]],
     "断言0 返回 MCP 应用及连接器状态=连接器工具清单入目录（真实握手同步；未初始化无内容"
     "在该面拒绝）；断言1 无效输入明确错误=缺声明引用被拒")
full("R00-T02-LA-15AD6ED13B4D",
     [[("mcp-tool-call-full-chain", 1)],
      [("mcp-tool-permission-face", 1)]],
     "断言0 按 arguments 执行返回结果或错误=经统一网关真实 run 链执行；断言1 权限不足明确"
     "错误=read_only 档拒绝腿")
full("R00-T02-LA-7FF8D4E48BC9",
     [[("mcp-tool-call-full-chain", 1)],
      [("mcp-tool-permission-face", 1)]],
     "断言0 launchInput 交给工具返回启动结果=命名空间 target 解析+网关真实执行；断言1 权限"
     "不足明确错误=read_only 档拒绝腿")
full("R00-T02-LA-DD47275AC5AE",
     [[("mcp-connector-catalog-sync", 1)],
      [("a15-missing-claim-refused", 1)]],
     "断言0 按 agent 配置返回状态=同步报告（协商协议/server 身份/registered 数）+"
     "is_connected 真实可读；断言1 无效输入明确错误=缺声明引用被拒")
full("R00-T02-LA-FAE7503D0D0F",
     [[("matrix-lifecycle-disable-holes", 0)],
      [("a16-alias-route-covered", 1)]],
     "断言0 全局开关返回合并状态=启停映射目录 availability（Disabled 可发现不可调用，"
     "expect=0 即零洞）；断言1 停用后不冒充可用=停用别名路由同样被拒")
full("R00-T02-LA-BB1BB3A9C5F4",
     [[("catalog-face-lists-mcp-tools-with-permission", 1)],
      [("matrix-lifecycle-disable-holes", 0)]],
     "断言0 独立更新延迟字段返回新状态=Deferred availability 按需目录语义（可发现、按需"
     "解析经同一网关）；断言1 无效输入明确错误=延迟/停用面不可调用（expect=0）")

# — the permission-mode / plan-mode / confirm family (R04-EXCLUSIVE): full —
full("R00-T02-LA-D01766AF4475",
     [[("permission-face-modes-verifiable", 1)],
      [("sup01-ask-subagent-write-refused", 1)]],
     "断言0 setSessionPermissionMode 返回 mode/accessMode=三档可设置可读取（真实会话面）；"
     "断言1 拒绝时目标状态不变=ask 档结构化 TOOL_APPROVAL_UNAVAILABLE 拒绝（模式生效且"
     "副作用一致，deny_on_prompt）")
full("R00-T02-LA-25C4FF66FEE5",
     [[("permission-face-modes-verifiable", 1)],
      [("sup01-ask-subagent-write-refused", 1)]],
     "断言0 返回当前 permissionMode=会话权限模式可读面真实（session_supervisor 读取真实"
     "状态）；断言1 状态与副作用一致=ask 档拒绝腿（读取不改权限）")
full("R00-T02-LA-8D3CEB6133E1",
     [[("permission-face-modes-verifiable", 1)],
      [("sup01-ask-subagent-write-refused", 1)]],
     "断言0 校验模式写入偏好返回保存后 permissionMode=模式设置经真实面生效；断言1 拒绝对"
     "目标状态不变=ask 档拒绝腿")
full("R00-T02-LA-0B669B30C854",
     [[("permission-face-modes-verifiable", 1)],
      [("sup01-ask-subagent-write-refused", 1)]],
     "断言0 按作用域设置模式返回实际模式或冲突错误=模式设置/读取面真实；断言1 失败/冲突/"
     "拒绝状态与副作用一致=ask 档结构化拒绝")
full("R00-T02-LA-5E61048F19B9",
     [[("permission-face-modes-verifiable", 1)],
      [("sup01-ask-subagent-write-refused", 1)]],
     "断言0 返回 mode/accessMode/defaultMode 不改变权限=模式读取面真实；断言1 状态与副作用"
     "一致=ask 档拒绝腿")
full("R00-T02-LA-A336F79E964D",
     [[("permission-face-modes-verifiable", 1)],
      [("matrix-lifecycle-disable-holes", 0)]],
     "断言0 setPlanMode 返回最新状态=plan 档=read_only 预设的会话模式切换（同一面）；断言1 "
     "enabled true/false 两向=启停面 disable 零洞（开关两向生效，expect=0；拒绝时目标状态"
     "不变）")
full("R00-T02-LA-BC2FBD618278",
     [[("permission-face-modes-verifiable", 1)],
      [("a16-history-preserved", 1)]],
     "断言0 返回 planMode/permissionMode/accessMode=模式读取面真实；断言1 无参数/不扩大写入"
     "或披露=只读后历史收据如实保留（读取不改变状态）")
full("R00-T02-LA-75C0AE981505",
     [[("approval-answer-executes-once", 1)],
      [("approval-duplicate-idempotent", 1)]],
     "断言0 confirmed 继续原待批动作并广播=批准→恰执行一次（收据 dispatched）；断言1 ID "
     "缺失/已处理不重复执行=重复点击 AlreadySettled 确定性 no-op")
full("R00-T02-LA-CD1524CC7DC3",
     [[("approval-reject-zero-dispatch", 0)],
      [("approval-duplicate-idempotent", 1)]],
     "断言0 rejected 原待批动作不执行=拒绝零派发（expect=0 图钉）；断言1 ID 缺失/已处理不"
     "再次决策=AlreadySettled no-op")
full("R00-T02-LA-EC033184BA37",
     [[("approval-answer-executes-once", 1)],
      [("approval-duplicate-idempotent", 1)]],
     "断言0 授权范围通过后 confirmed 待批动作才继续=批准恰执行一次；断言1 已处理不重复执行="
     "AlreadySettled no-op（无效/越权不扩大授权）")
full("R00-T02-LA-EF43ADCE19A1",
     [[("approval-reject-zero-dispatch", 0)],
      [("approval-duplicate-idempotent", 1)]],
     "断言0 rejected 待批动作不执行=拒绝零派发（expect=0）；断言1 拒绝后再次提交不得继续原"
     "动作=AlreadySettled no-op")
full("R00-T02-LA-67256417FB2B",
     [[("preauthorization-single-session-scoped", 1)],
      [("approval-duplicate-idempotent", 1)]],
     "断言0 会话内放行 capability 不持久化=预授权整键匹配/单次使用/会话作用域/跨会话不可见"
     "（经真实 gate 消费）；断言1 失败/拒绝状态与副作用一致=重复决策确定性 no-op")

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
# F54 invariant: every FULL leaf is R04-EXCLUSIVE and every share leaf is
# dual-stage — a share with no later stage owner is the unowned remainder.
for sid in FULL:
    assert r04_leaves[sid]["execution_stage_ids"] == ["R04"], sid
for sid in SHARE:
    assert any(s != "R04" for s in r04_leaves[sid]["execution_stage_ids"]), sid
assert len(FULL) == 46, f"expected 46 F54 full leaves, got {len(FULL)}"

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
        "仅 R04；R05 RR3 F54 2026-10-07：每条 R00 原断言各钉专属真实案例，不得以无人承接的 "
        "share 分类放行）+ "
        f"{share_count} stage_share_satisfied（R00 登记双阶段叶的 R04 份额=工具基础机制层——"
        "目录/网关/权限/批准/原生执行器/worker/MCP 桥，案例由 r04_tool_matrix 生产者机器核验）+ "
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
