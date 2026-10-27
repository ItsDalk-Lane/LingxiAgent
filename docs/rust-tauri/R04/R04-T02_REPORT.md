# R04-T02 报告｜实现唯一工具执行网关

执行代理：EXECUTOR-R04-T02-E01（一次性执行代理，ZCode Agent 工具派发）。
日期：2026-09-30。分支 `codex/rust-tauri-migration`，基线 `fd5fc2c2c583c6f4273ad3fa2122530edd8b152c`（R04-T01 已推送成果；派单文件为工作区唯一既有未跟踪文件，无用户已提交修改被触碰）。
状态：**READY_FOR_REVIEW**（普通逐项 + 对抗性两层自查完成；不自行判独立 PASS）。

---

## 1. 目标与结论

不同调用路线不能改变同一主体、工具、参数应得到的授权结论（R04-A03）；模型参数里的任何伪
principal/capability/prepared/approved 都不能覆盖宿主身份事实、扩大授权或产生副作用
（R04-A04）。

交付了 Rust `ToolInvocationGateway` + 服务端生成不可伪造的 `PreparedInvocation` + 策略
service ports（`ToolPolicyPort`/fail-closed 默认）+ 静态边界检查器 B1。网关严格接入 R03 已
有授权判定点与 journal 写序（R04-SUP-02）：**授权判定点仍是 runs.rs 驱动在 journal
authorized 步的 RunGrant 应用（本 Task 未移未改）；journal 每个 phase 仍由驱动唯一写入
（网关不持有任何存储端口）**。真实存在的入口（直接/按需/子代理/后台/委派）全部转为统一
`InvocationRequest`；MCP/插件/开发路由在 Rust 栈尚不存在（T07），未虚构。

验证：fmt/clippy/xtask 双门禁/十套件钉数/A15/A16 全部 exit 0；workspace 测试 533 通过、
唯一失败为已登记的 `r00_management_leaves` macOS 防火墙环境失败（§5.3，形态与 T01 报告
§5.3 完全一致并经隔离复跑核证）。

## 2. 实现摘要（关键文件:行）

### 2.1 网关本体（核心交付）

- `rust/crates/lingxi-service/src/toolgateway.rs`（新文件，1544 行含单测）：
  - `CallerSurface`（L116）/`InvocationEntry`（L126）：真实入口盘点——`UserRun`
    （直接=常驻 Available / 按需=Deferred，由 manifest 可用性分类）、`SubagentRun`
    （子代理子运行）、`DelegationDispatch`（subagent/subagent_reply/subagent_close 委派分
    发）。文档明示：background.rs 驱动的是同一 RunSupervisor 链（入口面不同、工具路径相
    同，A03 用真实 background 提交面验证）；MCP/插件/开发路由归 T07 不虚构。
  - `InvocationRequest`（L149）＋`from_trusted_entry`（唯一构造器，L173 附近）：统一请
    求由**可信入口**从 `RunContext` 构造——principal/session/run/attempt/generation/
    agent 全部来自运行上下文，**不存在 from-JSON 路径**，模型 arguments 永远无法填充身
    份字段。
  - 策略 service ports：`PolicyVerdict`（L200，Allowed/NeedsApproval/Denied）+
    `ToolPolicyPort`（L240）+ `PolicyAdjudicationInput`（只携带可信事实）。生产默认
    `FailClosedPolicy`（L254）：Read 类按契约允许；Execute/参数感知类＝**显式需批准**
    （理由写明"cannot safely adjudicate … never auto-allows"）——未经配置的机制拒绝其不能
    安全判定的操作，绝不自动 Full 放行。
  - `GatewayRefusal`（L278）：13 类响亮拒绝词表（稳定 `code()` + `to_tool_error()` 结构
    化错误）。
  - `PreparedInvocationHandle`（L460，CSPRNG 不透明句柄，`prep:` + 16 字节 hex；CSPRNG 失
    败＝拒绝 prepare，绝不降级为可猜测句柄）＋ `PreparedInvocation`（L480，调用方可见视
    图：句柄/入口/target/权限/摘要/双代次/期限/policy 判定）＋ 服务端绑定记录
    `PreparedRecord`（principal 双元组/session/run/attempt/generation/agent/入口/target+
    双代次/有效参数+可信摘要/call id/期限；**不交给模型**）。
  - `ToolInvocationGateway`（L546）：持有 `Arc<ToolRegistry>`（T01 交付）、策略端口、私
    有 per-target 执行器绑定（`bind_executor` L600，绑定附 registrar 理由）、有界 prepared
    注册表（live cap 1024 + 消费即回收进 4096 环 `CONSUMED_RING_CAP` L541；容量压力时清
    扫过期条目——对抗自查修复的内存无界缺陷，见 §7）。
  - `prepare`（L637）：registry.prepare_invocation（target 解析/可用性/代次 pin/当前
    schema 校验+规范化/可信摘要）→ 入口推导（用户运行按 manifest 可用性分 Direct/
    OnDemand）→ 策略裁决（Denied＝零派发拒绝；NeedsApproval＝**prepare 成功**，批准等待
    属驱动批准面）→ 铸句柄入注册表。
  - `prepare_from_request`（L789）：驱动请求形状 → 先做 wire 反伪造（`digest_matches_
    arguments`，对驱动 T01 门的纵深防御）→ 统一 prepare。**摘要语义（对抗自查修复）**：
    反伪造对比只覆盖 wire 对；prepare 规范化合法填充 schema 默认值后的摘要与 wire 摘要不
    同——这是可信边界在做事，不是伪造（修复前会误拒一切带默认值的调用，回归测试
    `schema_defaults_fill_without_digest_forgery_and_reach_the_executor`）。
  - `execute_prepared`（L827）：锁内原子**先验后耗**——存活→未消费→未过期→身份七元组
    （principal×2/session/run/attempt/generation/call id）比对→消费（记录移出 live 表入
    环）。**外来错误身份呈现不烧毁合法句柄**（合法主人仍可花费）。锁外：注册表复验
    （target 仍在/可调用/target_generation 相等）→ 绑定执行器存在（无绑定＝fail-closed
    拒绝）→ `dispatch_executor`（L1008，唯一受保护派发路径）以**服务端绑定的有效参数**派
    发。执行器收到的永远不是 wire 重推导数据。
  - `dispatch_executor` 不对业务公开：网关**没有** `ToolExecutorPort` impl（一个裸
    `execute(ctx,call,request)` 就是绕过 prepare 的后门），执行器只能经核验句柄到达。

### 2.2 驱动接线（runs.rs，授权判定点/journal 写序不动）

- `RunSupervisor.tool_gateway`（runs.rs L291）+ `with_tool_gateway`（L380，组合根构造后
  绑定；绑定后网关是该监督者唯一执行路径，裸 `tools` 端口不再被咨询——无双轨）。
  `ServiceDeps.tool_gateway`（lib.rs L515，默认 None＝R03 形状原样）→ bootstrap
  `with_tool_gateway`（lib.rs L898）。
- 工具环新序（全部在既有 journal 写序内）：
  1. digest 门（T01 既有，未动）→
  2. **网关 prepare**（runs.rs L1106，位于 intent 写入之前）：拒绝→镜像 digest 门形状
     journal（intent 先行绑定可信摘要）+ `failed+dispatched=false` 收据 + 结构化拒绝，零
     派发；成功→继续。**policy 裁决与一切 target/参数拒绝都发生在 `started` 之前**——
     拒绝/需批准永远不会留下 started-未派发条目（该 phase 语义保留给真实外部派发）。
  3. `record_invocation_intent`（既有，未动）→ RunGrant 授权判定（**既有判定点未动**，
     L1236 仅增加：网关路径额外用 prepared local_name 复验 kernel `authorize_child_tool`
     ——`tool:first-party:subagent` 经注册表 id 仍命中反递归名单，名字空间化 id 不是绕过，
     kernel 词表不改）→
  4. **批准需求**（L1319）：网关路径取自已裁决 policy（Allowed→直接 advance authorized，
     不重复弹审批；NeedsApproval→走既有 waiting_approval 批准面；**无批准面→结构化拒绝
     TOOL_APPROVAL_UNAVAILABLE 语义、零派发、绝不自动放行**）。无网关＝R03 形状逐字保留
     （有 gate 必问、无 gate 直授）。
  5. 委派分支（L1601）：family 匹配用 prepared local_name（回退裸 target）；委派目标经过
     与所有 target 相同的网关 prepare 检查——特殊运行机制不绕网关；T01 交付的只读衰减/
     取消树/父子关系全部保留（衰减在 dispatch 时 resolve_subagent_access，未触碰）。
  6. `advance_invocation(started)`（既有）→ **网关 execute_prepared**（L1786，监督子任
     务内）：`Err(refusal)`＝零派发→`failed+dispatched=false` 收据（拒绝词表自带）；
     `Ok(result)`→既有 fence 裁决/journal_receipt_of/persist_tool_event 逐字保留。

### 2.3 边界检查器（xtask check-boundaries 扩展）

- `docs/rust-tauri/R01/r01_t01_check_ownership.py`：新检查 **B1**（`check_protected_
  executors` L347，白名单 `PROTECTED_EXECUTOR_SYMBOLS` L322）：受保护符号
  `dispatch_executor`（仅 toolgateway.rs——定义即唯一派发权威）与 `execute_prepared`
  （toolgateway.rs 定义 + runs.rs 唯一业务调用者）只能出现在**带理由**的白名单文件
  （lingxi-service/src 业务代码范围；集成测试在 tests/ 不是业务入口，检查范围明示）。
  白名单自校验：无理由条目＝B1 违规。负向电池 **N16**（临时源树伪造 sessions.rs 直接调
  dispatch_executor→B1 拒绝）、**N17**（无理由白名单条目→B1 拒绝）。xtask
  check-boundaries（不传 --self-test）跑正向；`--self-test` 跑全电池（含 N16/N17），均
  exit 0。

### 2.4 依赖与 TS/Node 面

- 无新增 crate 依赖（`Cargo.lock`/`Cargo.toml` 零变化——网关只消费既有 kernel/protocol
  类型与 crate 内 auth CSPRNG）。
- **共享 TS/Node 消费面未触碰**：现役 TS 网关（`core/tool-invocation-gateway.ts`）仅作
  语义参照（prepare→revalidate→dispatch 的绑定面与拒绝词表族），无迁移无修改；npm 侧检
  查按派单"如触碰"条件不适用。

## 3. 调用链与修改范围

真实调用链（网关接线）：provider/适配器 `ToolRequests` → 驱动 digest 门（T01）→
**网关 prepare**（registry.prepare_invocation → 入口推导 → 策略裁决 → 服务端
PreparedInvocation）→ journal intent(prepared)（驱动）→ RunGrant 授权判定（驱动，既有
点+local_name 复验）→ 批准需求（policy 判定×批准面）→ journal authorized（驱动）→
[委派：started→真实子运行派发→收据] / [执行：started→网关 execute_prepared（身份/期限/
单次/注册表复验→私有执行器绑定）→ fence→收据]。每个 journal phase 唯一写入者＝驱动；
网关无存储端口、无 RunGrant 逻辑（无第二授权面）；网关策略端口是 §4.2 允许的分层"工具
策略"面，最终有效权限取约束交集。

修改文件清单：
- 新增：`rust/crates/lingxi-service/src/toolgateway.rs`；
  `rust/crates/lingxi-service/tests/r04_t02_tool_gateway.rs`（9 集成验收+对抗测试）；
  `artifacts/rust-tauri/R04/T02-E01/**`（证据）；本报告。
- 修改（产品）：`lingxi-service/src/{runs.rs,lib.rs}`（监督者字段/组合根注入/工具环接线；
  网关为 None 时 R03 路径逐字保留，十套件+A15/A16 证明）。
- 修改（门禁）：`docs/rust-tauri/R01/r01_t01_check_ownership.py`（B1+N16/N17）。
- 修改（文档）：`docs/rust-tauri/R04/R04_TEST_MAP.json`（T02 条目）。
- 未触碰：生产默认入口（ServiceDeps 默认 tool_gateway=None＝R03 行为）、Node/Electron 栈、
  kernel/protocol crate、xtask stage maps（R04.json 属 T08）、`ORCHESTRATOR_PROGRESS.json`。

## 4. 验收场景与逐项结果

| ID | 要求 | 实现/测试 | 结果 |
|---|---|---|---|
| R04-A03 允许组 | 同主体/target/参数经所有适用入口结论与副作用一致 | `r04_a03_allow_group_is_identical_across_all_entries`：probe_read（Read,Available）经**直接**（sessions.execute_for 真实链）、**按需**（Deferred 目标）、**子代理**（drive_run+Subagent{Operate} 授权，与 SubagentRuntime 派发同链）、**后台**（execute_background_for 真实提交面）四入口：各执行恰 1 次、canonical 载荷逐字节相同、journal Succeeded+dispatched=true | PASS |
| R04-A03 拒绝组 | 同一 Execute 类 target 每入口拒绝+零副作用 | `r04_a03_deny_group_unconfigured_policy_refuses_every_entry`：**真实 FailClosedPolicy（生产默认姿态）+无批准面**：直接/子代理(Operate)/后台三入口全部 `failed+dispatched=false`、执行器 0 次 | PASS |
| R04-A03 显式更严格 | 差异必须是显式更严格策略且登记依据 | `r04_a03_stricter_subagent_attenuation_is_explicit`：同一 probe_write 经 ReadOnly 子代理→kernel `ACTION_BLOCKED_BY_READ_ONLY`（authorize_child_tool 硬衰减），比用户入口的策略拒绝更严、永不更宽——依据＝R03-A11 冻结的父子交集规则，收据 detail 携带该码 | PASS（登记） |
| R04-A03 需批准组 | 需批准语义每入口一致 | `r04_a03_needs_approval_group_round_trips_every_entry`：FailClosed+批准面（ApproveAll 替身）→直接与子代理入口都经 waiting_approval 后各执行 1 次（收据 Succeeded+dispatched）；RejectAll 替身→拒绝+零派发；**policy Allowed+已接 gate→不再重复弹审批**（Read 工具在接了拒绝型 gate 时仍执行 1 次——被配置策略已裁定的调用不二次询问，§4.2 不重复弹审批） | PASS |
| R04-A03 委派入口 | 特殊分支走同一网关检查 | `r04_a03_delegation_entry_goes_through_the_gateway`：注册的 subagent 目标经网关 prepare（身份/可用性/当前 schema 参数/策略）后由真实 launcher 派发真实子运行；journal 目标＝`tool:first-party:subagent`、Succeeded+dispatched；执行器 0 次（委派副作用是子运行本身） | PASS |
| R04-A04 伪参数 | 参数携带假 principal/capability/prepared/approved 无效 | `r04_a04_forged_arguments_never_override_host_facts`：(a) 严格 schema→unknown-key 零派发拒绝（gateway_arguments_invalid）；(b) 宽松 schema→伪字段逐字随数据到达执行器但**不成为事实**（判定与诚实请求完全一致）；(c) **真实 FailClosed 下 `"approved":true` 不满足批准需求**——拒绝照旧、零派发 | PASS |
| R04-A04 伪句柄 | 伪/陈旧句柄不能达执行器 | `r04_a04_forged_and_stale_handles_are_refused_by_the_real_gateway` + lib 单测：垃圾句柄→unknown；跨 session/run 复用→identity_mismatch（且**不烧毁**合法句柄，合法主人随后花费成功）；并发双花（tokio::join）→恰 1 次派发、败者拒绝；禁用后缓存句柄→target_changed 零派发；代次升级后缓存句柄→target_changed；已耗句柄重放→consumed | PASS（全部由真实网关拒绝） |
| 对抗·摘要A参数B | 摘要与真实载荷不一致被拒 | `adversarial_summary_a_payload_b_is_refused_at_every_layer`：wire 摘要 A+参数 B→网关 `gateway_digest_mismatch`（对驱动 T01 门的纵深防御，驱动级已由 T01 测试覆盖）；A/B 各自 prepare 得不同句柄+不同摘要；花 A 句柄执行器只见 A 载荷——**execute_prepared 不接受任何参数**，B 无入口 | PASS |
| 对抗·同名/别名/陈旧/卸载 | 工具身份族 | T01 注册表语义（别名同权限/同名不覆盖/Ambiguous）由网关全量继承（网关只经 registry.prepare_invocation）；陈旧 pin→gateway_stale_catalog；卸载/禁用→target 复验拒绝；子代理反递归在注册表 id 下仍成立（下条） | PASS |
| 对抗·注册表 id 绕过名单 | `tool:first-party:subagent` 反递归 | `adversarial_subagent_blocklist_holds_under_registry_target_ids`：Operate 子运行调用注册表 id 的委派目标→prepared local_name 双重检查命中 `ACTION_BLOCKED_IN_SUBAGENT`、零子运行、零派发 | PASS |
| 对抗·单据并发/期限/有界 | 句柄生命周期 | lib：`prepared_handles_are_unique_and_single_use`（CSPRNG 唯一+单次）、`expired_and_foreign_handles_are_refused`（ManualClock 过期）、`consumed_records_are_reclaimed_under_cap_pressure`（cap=4 下 10 轮 prepare+消费全部成功——消费即回收、有界内存、重放诊断保留） | PASS |
| T02 怎么做 3 | 策略端口缺失时行为诚实 | FailClosedPolicy 单测：Read→Allowed；Execute/ArgumentAware×2→NeedsApproval（理由含 "cannot safely adjudicate"+"never"）；生产默认无 gate→TOOL_APPROVAL_UNAVAILABLE 语义结构化拒绝（§4 A03 拒绝组真实链验证）；T03 完整批准生命周期未提前实现（无假策略） | PASS |
| T02 怎么做 4 | 边界扫描+负向 | B1+N16/N17（§2.3），xtask check-boundaries exit 0 | PASS |

## 5. 验证（真实命令与退出码）

环境：macOS darwin 27.0.0 arm64；rustup 1.98.1（`/Users/study_superior/.cargo/bin/cargo`
先于 `/opt/homebrew/bin`）。仓库根执行；原始输出归档
`artifacts/rust-tauri/R04/T02-E01/`，复跑脚本 `run_t02_validation.sh`。以下为**最终候选
（代码冻结后）**一轮的退出码：

| # | 命令 | 退出码 | 备注 |
|---|---|---|---|
| 1 | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 | |
| 2 | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 0 | |
| 3 | `cargo test --manifest-path rust/Cargo.toml --workspace --locked` | 101（仅 1 环境失败） | **533 通过 / 0 断言失败**；唯一失败 `r00_management_leaves` 见 §5.3 |
| 4 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts` | 0 | 生成契约零漂移（本 Task 未触碰协议面） |
| 5 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries` | 0 | O1–O8+D1–D5+**B1**；`--self-test` 另跑 N1–**N17** 亦 exit 0 |
| 6 | `cargo test -p lingxi-service --test r04_t02_tool_gateway -- --nocapture` | 0 | 9/9（A03 五组+A04 两组+对抗两条） |
| 7 | `cargo test -p lingxi-service --lib toolgateway` | 0 | 6/6 网关单测 |
| 8 | `cargo test -p lingxi-service --test r04_t01_tool_catalog` | 0 | 6/6（T01 验收在网关合入后复绿） |
| 9 | `bash scripts/rust-tauri/r03_g07_repair_suites.sh <fresh dir>` | 0 | 十套件（G01–G06+RR2-F05）钉数精确全绿 |
| 10 | `bash scripts/rust-tauri/r03_t08_matrix.sh <abs dir>` | 0 | A15 组合矩阵 28 叶+11 组合全绿（真实链） |
| 11 | `bash scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh <abs dir>` | 0 | A16 种子机制四相全绿 |

npm/TS 侧未运行：本 Task 未触碰任何 TS/Node 源（§2.4），按派单"如触碰"条件不适用。

### 5.3 已知环境失败（如实登记，非本改动回归）

`r00_management_leaves::r00_management_positive_and_negative_branches_on_real_service`：
panic 文本自证为 macOS 应用防火墙/代理 TUN 拦截未签名测试二进制入站连接（"request to
192.168.3.5:… stalled during write/read exchange … macOS application firewall / proxy TUN
may be blocking inbound connections to this unsigned test binary; this is an environment
failure surfaced honestly"）——与 R04-T01 报告 §5.3 同一环境失败同形（kernel/service 变化
→测试二进制哈希变化→防火墙视为新未签名 app）。隔离复跑（本报告证据
`r00_management_leaves_isolated_rerun.log`）复现同一防火墙 panic 文本。该测试与工具网关无
任何交集；533 个绿测试与其互不掩盖。T08 最终候选重跑 R03 Gate 时将再现，需环境层处置。

## 6. R03 行为不变量与回归证明（R04-SUP-05）

- **驱动重构不改 R03 语义的证明**：网关为 None 时（全部 R03 测试的接法）新代码路径是
  `tools` 裸端口+既有批准流（`approval_requirement` 的 else 分支逐字等价于原
  `if self.approval.is_none() {advance} if let Some(gate)` 结构）；workspace 全量 533 通过
  （除环境项）＋十套件钉数（13/6/5/5/5/2/8/7…精确）＋A15 矩阵 28 叶＋A16 种子四相全绿。
- 父先取消树传播/取消终态裁决/Unknown 不盲重试/requestId 规范化/前后台保真/steering/子
  权限不超父：分别由十套件（background_steering 8/8、request_id_canonicalization 7/7 等）
  与 A15 组合链复验，无回归。
- journal 写序（intent→authorized→started→执行→receipt）逐字保留；网关的一切拒绝都以驱
  动写出的 `failed+dispatched=false` 收据落账（收据存在性由存储层强制——先 intent 后
  receipt），`started` 仍只出现在真实外部派发之前。

## 7. 两层自查记录（普通→对抗）

普通逐项：§4 表逐 A-ID/对抗项给出前提/动作/预期/实际/退出码；每组同时有合法对照（拒绝全
部不是安全绿灯——允许组真实执行、字节级载荷断言、journal dispatched=true 对照）。

对抗性自查（推翻自己的尝试与结果）：
1. **内存无界（发现并修复）**：初版已消费/过期句柄条目在 HashMap 永不清理——长寿命进程
   无界增长。修复：消费即移出 live 表入 4096 有界环（重放仍报 consumed 诊断）、容量压力
   时清扫过期条目；`consumed_records_are_reclaimed_under_cap_pressure`（cap=4、10 轮
   prepare+消费全部成功）证明回收与诊断并存。
2. **外来呈现烧毁句柄（发现并修复）**：初版先消费后验身份——敌对/错误身份的呈现会 DoS
   掉合法执行。修复：身份核验移入锁内、消费之前（单次语义由锁内原子性保证，并发双花仍恰
   一次）；A04 测试显式断言"跨会话复用后合法主人仍可花费"。
3. **schema 默认值误拒（发现并修复）**：初版比较 prepared 摘要与 wire 摘要——规范化合法
   填充默认值即不相等，一切带默认值的调用被误拒。修复：反伪造对比只覆盖 wire 对
   （`digest_matches_arguments`），绑定摘要取规范化结果；默认值回归测试证明填充后执行器
   收到默认化载荷、伪造 wire 对仍被拒。
4. **started-未派发窗口**：对抗性审视"policy 拒绝若发生在 started 之后会污染 Unknown 语
   义"——这正是把 prepare 放在 intent 之前、批准需求放在 authorized 之前的原因（§2.2）；
   网关执行期拒绝（started 已 durable 后的禁用/代次变化）收据 `dispatched=false`、崩溃窗
   口与 R03 既有窗口同形（started→派发之间），恢复面保守分类不变。
5. **并行 flake**：首轮全绿后一次并行跑出现临时目录碰撞（`schema_migrations already
   exists`）——修复为进程内原子计数目录名；随后 4 连跑全绿。
6. **注册表 id 绕过子代理名单**：预判并在实现时闭合（local_name 双重检查+对抗测试）；
   B1 静态面同样防业务代码绕网关直调执行器。

## 8. 未验证项与风险

1. `r00_management_leaves` 环境失败（§5.3）——需环境处置，代码无对应修改；T08 重跑 R03
   Gate 时将再现。
2. 跨平台（Windows/Linux）未验证：仅 macOS arm64 实测；网关为纯 Rust 领域逻辑无平台分支，
   但按任务书口径本地结果不替代其他平台验证。
3. **策略面诚实缺口（设计内）**：`FailClosedPolicy` 是无配置时的 fail-closed 默认，不是
   批准策略——Execute 类在无批准面时全部拒绝是**有意的严格姿态**；完整批准生命周期（预
   授权/绑定参数摘要的批准记录/撤销/只读模式迁移/ask 继承 SUP-01）属 T03。生产默认
   （ServiceDeps.tool_gateway=None）行为与 R03 完全一致，未切换任何生产默认入口。
4. 资源绑定面：PreparedInvocation 绑定 capability+参数摘要+身份；**ResourceRef/路径资源
   绑定属 T04**（资源层尚不存在，不虚构——绑定点的扩展位即 PreparedRecord，T04 接入）。
5. agent_id 记录于绑定但 ctx 不含 agent——跨 agent 复用实际被 session/run/attempt/call
   绑定集合阻断（另一 agent 无法同时满足该集合）；agent_id 作为审计事实保留。报告如实
   说明此层不可分辨性而非宣称独立阻断。
6. 委派 payload（DelegationRequest）与 arguments 的关系沿用 R03 形状（payload 由适配器
   边界结构化附加，launcher 校验 task 非空）——args.task 与 delegation.task 的逐字段一致
   性绑定未在本 Task 引入新检查（继承 R03 语义；T07/R05 适配器面可收紧）。
7. 集成测试中 `VerdictPolicy`/`ApproveAllGate`/`RejectAllGate`/`CountingExecutor`/
   `ScriptedProvider` 均为外部答复者/外部系统替身；**A04 的关键拒绝全部由真实
   FailClosedPolicy/真实网关产生**（§4 已逐条标注）；无任何替身替代被测网关、权限决定、
   PreparedInvocation 生成、journal、存储或监督。

## 9. 回退

回退范围＝本 Task 获准修改（§3 清单）：还原 `runs.rs`/`lib.rs`、删除 `toolgateway.rs` 与
`r04_t02_tool_gateway.rs`、还原 check 脚本 B1/N16/N17 与 TEST_MAP T02 条目即回到基线
fd5fc2c2c 形态。R03 语义在回退后由既有套件保护；网关是注入面（默认 None），删除不影响任
何现有生产路径。

## 10. 证据索引

- `artifacts/rust-tauri/R04/T02-E01/run_t02_validation.sh`——复跑脚本（绝对路径口径）。
- `gates/`：rust_fmt_check / rust_clippy / check_contracts / check_boundaries /
  r04_t02_acceptance_tests（含 lib toolgateway + T01 复验）/ r03_g07_repair_suites /
  r03_t08_matrix_a15 / r03_t08_a16_seed 各 .log。
- `workspace-test-cargo-test-workspace-locked.log`——全工作区测试原始输出（533 通过/1 环境
  失败）。
- `r00_management_leaves_isolated_rerun.log`——环境失败隔离复跑核证。
- `r03-repair-suites-regression-r3/`、`r03-regression-r3/A15|A16/`——R03 回归证据目录。
- 新测试文件：`rust/crates/lingxi-service/tests/r04_t02_tool_gateway.rs`；
  网关单测内嵌 `rust/crates/lingxi-service/src/toolgateway.rs`。
