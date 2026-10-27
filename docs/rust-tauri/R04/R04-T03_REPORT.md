# R04-T03 报告｜批准、撤销与只读模式

执行代理：EXECUTOR-R04-T03-E01（一次性执行代理，ZCode Agent 工具派发）。
日期：2026-09-30。分支 `codex/rust-tauri-migration`，基线 `39168bfb0`（R04-T02 已独立
PASS 并推送确认）。工作区起点仅含总控编排器更新的 `ORCHESTRATOR_PROGRESS.json` 与本
派单文件（均非用户已提交修改，未触碰前者）。
状态：**READY_FOR_REVIEW**（普通逐项 + 对抗性两层自查完成；不自行判独立 PASS）。

---

## 1. 目标与结论

批准的是**具体动作**，而不是不受限的后续执行权：批准记录绑定参数摘要/真实目标/资源
集合/主体/Run/代次/期限/使用次数（R04-A05）；等待期间禁用/卸载工具后批准旧请求仍拒绝
且错误言明目标已失效（R04-A06）；operate/ask/read_only 权限裁决迁入 Rust（R04-SUP-01
关闭 R03-T06-O1 的 ask 档继承坍缩差距）；T02 遗留 O05（policy Allowed 与已接批准面的
优先级）正式裁定并测试钉住。

交付：`ApprovalService`（新文件 `rust/crates/lingxi-service/src/approval_service.rs`，
生产策略面 + 批准等待面 + 受认证最小批准交互面 + 会话级预授权）＋ 权限上下文贯穿网关
（`InvocationPermissionContext`）＋ 驱动/会话/子代理的 session-mode 快照传递。R02-T02
网关的授权判定点/journal 写序/句柄单次语义**零改动**（SUP-02 保持）；生产默认入口
（`ServiceDeps.tool_gateway=None` 的 R03 形状）不变。

验证：fmt/clippy/xtask 双门禁（含 self-test N1–N17）/T03 专项（集成 11 + lib 单测 6）/
T01/T02 套件/R03 十套件钉数+A15+A16 全部 exit 0；workspace `--locked` **788 通过 / 0
失败 / 0 ignored**（r00 防火墙环境项本轮通过——与 T02 登记的间歇行为一致，不销项）。

## 2. 实现摘要（关键文件:行）

### 2.1 ApprovalService（核心交付，approval_service.rs）

- **策略面**（`adjudicate`，实现 `ToolPolicyPort`；矩阵文档在函数注释）——现役
  `classifySessionPermission` + `resolveSessionApprovalPolicy` 的 Rust 映射：
  - 用户会话：operate→写类 Allowed（policy Never）；ask→写类 NeedsApproval
    （Interactive）；read_only→写类 **Denied `ACTION_BLOCKED_BY_READ_ONLY`**
    （session 层，提权拒绝）；Read 类一律 Allowed。
  - 子代理（`Subagent{tier, parent_mode}`，**SUP-01 核心**）：
    - Operate tier + **ask 父**（省略 access 继承或显式 write——现役
      `resolvePermissionMode` 的 write 请求继承父档而非坍缩 operate）→ 写类
      **Denied `TOOL_APPROVAL_UNAVAILABLE`**（layer `approval_policy`，文案携带
      `deny_on_prompt` / `allowHumanApproval false` / "not run"）——不自动同意、不无限
      等待、不降级裸执行；Read 类照常 Allowed（研究面不关）。
    - Operate tier + operate 父 → 写类 Allowed（正常 Operate 合法写）。
    - ReadOnly tier → 策略面 Allowed、**交由 kernel 授权判定点拒绝**（R04-SUP-02 单一
      授权判定点；`authorize_child_tool_with_registry_id` 的 ACTION_BLOCKED_BY_READ_ONLY
      照常生效，不重复词表、不回退 T02 F01 修复）。
    - Operate tier + read_only 父（经 `resolve_subagent_access` 不可达：显式 write 被
      SUBAGENT_WRITE_DENIED_BY_PARENT_READ_ONLY 拒、省略继承 ReadOnly）→ 防御性 Denied
      （安全方向）。
  - `ApprovalPolicy`（interactive/deny_on_prompt/never 词汇）+ `resolve_approval_policy`
    （resolveSessionApprovalPolicy 映射；子代理**任何**上下文固定 deny_on_prompt——
    allowHumanApproval:false 是现役 subagent 派发的无条件参数，subagent-tool.ts L479/813）。
- **等待面**（实现 `ApprovalGate::request`）：预授权命中（锁内 compare-and-decrement，
  原子消费一次）→ 立即 Approved；否则铸造**绑定记录**（CSPRNG `approval:{16字节hex}`
  id；principal×2/session/run/attempt/generation/tool_call_id/target/规范化 args 摘要/
  形状摘要/期限；铸造时 info 审计日志输出全部绑定事实）并 park 于
  `select{notify, sleep_until(deadline)}`；**等待期零预执行**（本 future 只裁决，执行器
  从不被触及）；期限=ConfirmStore 5min 默认（DEFAULT_APPROVAL_TIMEOUT_MS）。
  - **取消**：future 被 drop（驱动取消树 CALL 级 biased abort）→ `AbortOnDrop` RAII 将
    记录标 Aborted（迟到 answer→Refused，不复活）；CSPRNG 熵失败→Aborted（拒绝等待，
    不降级可猜测 id）。
  - **有界**：pending cap（满=响亮 Aborted，清扫仅回收非 Pending 过期记录——park 中的
    记录绝不回收）、settled ring 4096（迟到/重放诊断保留、内存有界）。
- **受认证最小批准交互面**（不新做设置 UI）：`pending_of`（仅同 principal+session 的
  Pending 可见；视图只带 approval_id/target/run_id/形状摘要/时间戳——`summarize_arguments`
  仅键名+类型标签，值零泄漏）＋ `answer`（归属校验：外来 principal/session 统一 Refused，
  不泄露存在性；状态机：Pending→Settled 一次定胜负，双击→`AlreadySettled`（首个决定
  原样返回），Aborted/Expired/未知 id（含重启前铸造）→`Refused` 确定拒绝）。
- **会话级预授权**（现役 `preAuthorizedInvocationCapabilities` 真实契约："capability
  字符串是完整 key，grant 永不超出被授予的确切 invocation"）：`InvocationGrantKey`
  {target_id, capability_base, args_digest_hex} + principal + session 为完整 key；
  `grant_preauthorization`（单次默认；max_uses=0 / ttl=0 响亮拒绝；TTL 有界——永不过期
  的授权=无限后续执行权，恰是任务书禁止的）；`spend_preauthorization` 锁内原子减次
  （两个并发请求不可能各花掉同一次——测试 `concurrent_spends_of_one_grant_...`
  tokio::join 双花恰一赢家）。
- **重启规则（显式、诚实）**：批准记录与预授权为进程内存态——重启即全部失效（旧
  approval id→Refused(unknown…do not survive restarts)、旧预授权不再命中、重放 prepared
  句柄对新网关注册表 unknown）；不默认延续，不做静默假持久化（durable 批准状态是 R06
  会话状态范围）。模块文档明文记载。
- **撤销 vs 已开始边界**：撤销（禁用/卸载/权限修订）落在外部派发**之前**则永远赢
  （execute_prepared 注册表复验→零派发+明确错误，A06 测试）；执行器已进入的外部副作用
  不声称可撤销（任务书口径原文写入模块文档）。

### 2.2 权限上下文贯穿网关（toolgateway.rs）

- `InvocationPermissionContext`（新枚举）：`UserSession{mode}` / `Subagent{tier,
  parent_mode}`——tier 单独无法表达 ask 继承（正是 R03-T06-O1 缺口：ask 父省略 access
  坍缩为 Operate tier 后丢失 ask 事实）；`{tier, parent_mode}` 二元组复现现役 ask 档
  deny_on_prompt 语义。
- `InvocationRequest.permission`（`from_trusted_entry` 新参数——仍无 from-JSON 路径，
  模型 arguments 无从填充）；`PolicyAdjudicationInput.permission_context`（策略面输入）；
  `PreparedRecord.permission`（审计事实；execute_prepared 派发日志输出
  `permission_context`）。`prepare_from_request` 新增 permission 参数。

### 2.3 驱动与会话快照传递（runs.rs / sessions.rs / subagents.rs）

- `DriveAuthorization.session_mode`（新字段）：run 准入/派发时**快照一次**的会话权限模式
  （用户 run=会话当前模式，`sessions.rs` 两处 `user_submission` 经
  `gate.permission_mode(session_id)` 传入；子代理 run=父模式，
  `subagents.rs` `spawn_child` 新 parent_mode 参数贯穿 dispatch/reply）。
  `invocation_permission_context()` 从 grant+session_mode 推导网关上下文。
  **映射决策（文档化）**：现役在执行层逐调用读模式；Rust 运行层在 run 准入时绑定快照
  ——run 中途切档对**后续准入的 run** 生效、不追溯已驱动的 run（prepare 与 kernel 授权步
  之间无中途 TOCTOU；交互面每次新提交重读快照）。
- 驱动 prepare 传 permission（runs.rs L1140 附近）；批准等待请求补形状摘要
  （`args_summary.clone().or_else(summarize_arguments)`——批准人看到形状、值零泄漏）。
- **O05 正式裁定**（approval_service.rs 模块文档+测试钉住）：**策略面是批准需求的唯一
  来源**——policy Allowed→直接 advance authorized，不再询问已接批准面（总控 §4.2 不重复
  弹审批）；NeedsApproval→等待批准面；无批准面→驱动关闭为 TOOL_APPROVAL_UNAVAILABLE
  结构化拒绝（T02 语义逐字保留）；R03 最小 gate 仅在无网关 legacy 接线上被咨询（原样）。

### 2.4 approval.rs（R03-T03 最小接口演进）

- `ApprovalRequest` 增加 `args_summary: Option<String>`（形状摘要；驱动侧在 wire 未带时
  由 `summarize_arguments` 派生）。既有 trait 形状、取消契约（drop=aborted、迟到决定
  返回 false）不变。

### 2.5 依赖与 TS/Node 面

- 无新增 crate 依赖（Cargo.lock/Cargo.toml 零变化）；现役 TS 栈未触碰（仅语义参照）；
  npm 侧检查按"如触碰"条件不适用。

## 3. 修改文件清单

- 新增：`rust/crates/lingxi-service/src/approval_service.rs`（ApprovalService + 单测
  6）；
  `rust/crates/lingxi-service/tests/r04_t03_approval_service.rs`（11 集成验收/对抗测试）；
  `artifacts/rust-tauri/R04/T03-E01/**`（证据+复跑脚本）；本报告。
- 修改（产品）：`lingxi-service/src/{approval.rs,lib.rs,runs.rs,sessions.rs,subagents.rs,
  toolgateway.rs}`、`tests/r04_t02_tool_gateway.rs`（公共签名演进的等价更新：新参数传
  operate 上下文/快照——其结论不变，13/13 复绿证明）。
- 修改（文档）：`docs/rust-tauri/R04/R04_TEST_MAP.json`（T03 条目）。
- 未触碰：生产默认入口（ServiceDeps 默认 tool_gateway=None=行为与 R03 一致）、
  kernel crate（subagent.rs 词表/判定零变化）、协议 wire 面（check-contracts 626 entries
  零漂移）、xtask stage maps、check 脚本白名单（B1 现状覆盖：approval_service.rs 不触
  受保护符号）。

## 4. 验收场景与逐项结果（全部真实链：bootstrap_with_deps 组合根→真实驱动→真实
journal→真实注册表→真实网关→真实 ApprovalService；替身仅外部答复者/外部系统/模型脚本）

| ID | 要求 | 实现/测试 | 结果 |
|---|---|---|---|
| R04-A05 批准后改参 | A 批准 B 提交→拒绝/重审，B 不被写入 | `r04_a05_approved_digest_a_cannot_execute_payload_b`：**等待形态**——run A park→用户批准（pending 视图仅形状摘要 `{path:str}`，值零泄漏）→A 执行恰 1 次且载荷=`file-A`；同 id 双击→AlreadySettled 不变；提交 B（同工具不同参数）→B park 出**独立记录**（CSPRNG id≠A）→拿 A 的 settled id 去 answer 是 B 面上的 Refused/AlreadySettled（B 的 pending 不受影响）→用户 Reject B→B 收据 Failed+dispatched=false、执行器仍只有 A；**预授权形态**——grant 绑 (target+digest(A2)) 完整 key→A2 无新弹窗执行；B2（不同 digest）不命中→新 pending→Reject→零执行；最终执行器载荷恰 [A, A2] | PASS |
| R04-A06 等待期禁用 | 禁用/卸载后批准旧请求→仍不执行、错误言明目标失效 | `r04_a06_wait_disabled_then_approved_still_refuses`：**禁用腿**——park 中 `set_availability(Disabled)`→批准旧请求→execute_prepared 注册表复验拒→收据 Failed+dispatched=false+detail 含 `gateway_target_changed`+disabled 理由、执行器 0 次；**卸载腿**——`uninstall` 后批准旧请求→not registered/target 拒绝、0 次执行 | PASS |
| SUP-01 ask 档差距 | ask 父+省略 access/请求写+不可人工审批→结构化拒绝；保护四个邻居 | `sup01_ask_tier_subagent_matrix_on_the_real_chain`（真实 launcher 链：父 run→subagent 委派→SubagentRuntime→子 run→写类调用）：(1) ask+省略→TOOL_APPROVAL_UNAVAILABLE+deny_on_prompt+零执行；(2) ask+显式 write→同拒绝（write 继承 ask 不坍缩 operate）；(3) ask+读类→放行；(4) operate+省略→写执行（合法写对照）；(5) read_only+显式 write→派发即拒（零子运行）；(6) read_only+省略→子写类 ACTION_BLOCKED_BY_READ_ONLY（kernel 层、非误归 approval 层）+子读类放行；(7) read_only+显式 read→T02 F01 修复保持（注册 Read 类派发） | PASS |
| O05 优先级收口 | policy Allowed 与已接 gate 优先级正式契约+钉住 | `o05_policy_allowed_never_re_prompts_the_wired_gate`：operate 会话+**RejectAllGate 已接线**→写类执行恰 1 次（gate 从未被问；真实 ApprovalService 的 pending 面空）——Allowed 抑制重复弹审批是正式契约；对照 ask 会话→NeedsApproval→gate 拒绝生效（零执行+收据"external answerer rejects"） | PASS |
| 超时 | 审批超时确定结果 | `approval_timeout_is_a_definite_rejection_and_late_approval_is_refused`：120ms 短期限→park 无人答复→超时=Rejected/Aborted 语义→收据 not dispatched、0 执行；迟到批准→Refused(expired)、仍 0 执行 | PASS |
| 取消后迟到批准 | 不复活 | `cancel_during_wait_then_late_approval_never_resurrects`：park 中 `cancel_run`（真实用户取消入口）→驱动 cancelled 终态（CALL 级 biased abort drop 等待 future→AbortOnDrop 标 Aborted）；迟到批准→Refused(aborted)；再等 120ms 仍 0 执行 | PASS |
| 一次批准并发消费 | 两个并发请求不能各花掉同一次 | `one_approval_cannot_be_spent_by_two_concurrent_requests`：两会话并发 park 同 payload（同 digest 对抗形态）→两独立记录；批准其一→恰 1 执行；另一 pending 原样等待（跨会话重放 A id→Refused）→Reject 后总执行=1。另 `concurrent_spends_of_one_grant_...`：**单次预授权** tokio::join 双花→恰 1 赢家、败者开自己的 pending | PASS |
| 重启票据重放 | 旧批准明确失效不延续 | `restart_invalidates_old_approvals_and_grants`：真实链批准+执行后新建 ApprovalService（重启形态）→旧 approval id→Refused(unknown…do not survive restarts)；旧预授权不存在→重放请求 park 出**新** pending（拒绝于 60ms 观察窗内未从旧状态 resolve）→对新 pending 的 answer 只决定它自己 | PASS |
| 别名复用授权 | 不同别名不得复用一次授权 | `alias_cannot_reuse_a_single_use_grant`：probe_write 带 alias 注册；单次预授权经 canonical 名花掉→Approved；经**别名**再来同 digest→grant 耗尽→park 自己的新 pending（60ms 观察窗无旧授权 resolve）→Reject→Rejected。预授权 key=(target_id,digest) 且单次——别名与主名解析到同 target 属同一 invocation 语义，单次预算耗尽即拒绝（lib 单测 `preauthorization_key_discrimination_and_use_budget` 另证跨 digest/跨 session/跨 target 不匹配+单次耗尽） | PASS |
| 用户模式矩阵 | read_only 拒提权/operate 合法写/ask 真实往返 | `user_session_mode_matrix_on_the_real_chain`：read_only 会话写类→gateway_policy_denied+ACTION_BLOCKED_BY_READ_ONLY、0 执行；ask 会话一 run 两调用（write park→批准→执行；read 免审执行）→两条 journal Succeeded；lib 单测钉 operate/ask/read_only×Read/Execute 全格 | PASS |

## 5. 验证（真实命令与退出码）

环境：macOS darwin 27.0.0 arm64；rustup 1.98.1（`/Users/study_superior/.cargo/bin/cargo`）。
仓库根执行；原始输出归档 `artifacts/rust-tauri/R04/T03-E01/`，复跑脚本
`run_t03_validation.sh`。以下为**最终候选（代码冻结后）**一轮的退出码：

| # | 命令 | 退出码 | 备注 |
|---|---|---|---|
| 1 | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 | |
| 2 | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 0 | |
| 3 | `cargo test --manifest-path rust/Cargo.toml --workspace --locked` | 0 | **788 passed / 0 failed / 0 ignored**（76 个 `test result: ok` 复算合计；r00_management_leaves 本轮通过） |
| 4 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts` | 0 | 626 entries 零漂移（本 Task 未触碰协议面） |
| 5 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries` | 0 | O1–O8+D1–D5+B1；`--self-test`（N1–N17）另跑 exit 0 |
| 6 | `cargo test -p lingxi-service --locked --test r04_t03_approval_service -- --nocapture` | 0 | 11/11 |
| 7 | `cargo test -p lingxi-service --locked --lib approval_service` | 0 | 6/6 |
| 8 | `cargo test -p lingxi-service --locked --test r04_t02_tool_gateway` | 0 | 13/13（签名演进等价更新后复绿） |
| 9 | `cargo test -p lingxi-service --locked --test r04_t01_tool_catalog` | 0 | 6/6 |
| 10 | `cargo test -p lingxi-kernel --locked --lib` | 0 | 77（含 registry_id_wiring_* 等价扫描） |
| 11 | `bash scripts/rust-tauri/r03_g07_repair_suites.sh /tmp/r04t03-e01/...` | 0 | 十套件钉数精确全绿 |
| 12 | `bash scripts/rust-tauri/r03_t08_matrix.sh /tmp/r04t03-e01/...` | 0 | A15 28 叶+11 组合全绿 |
| 13 | `bash scripts/rust-tauri/r03_t08_a16_seed_mechanism.sh /tmp/r04t03-e01/...` | 0 | A16 四相全绿 |

npm/TS 侧未运行：本 Task 未触碰任何 TS/Node 源（§2.5），按派单"如触碰"条件不适用。

### 5.1 r00 环境项（如实登记，不销项）

`r00_management_leaves` 为 T01/T02 登记的 macOS 防火墙拦截未签名测试二进制的**间歇**
环境失败：本轮全 workspace 运行中**通过**（与 T02-R2 审查轮同形——连续未复现但保留
登记；T08 Gate 重跑 R03 Gate 时如再现需环境层处置）。

## 6. R03 行为不变量与回归证明（R04-SUP-05）

- **授权判定点未动**：RunGrant 应用仍在 journal authorized 步（runs.rs 既有块，仅新增
  permission 传参）；网关无 grant 逻辑、无存储端口；journal 写序逐字保留；网关路径的
  一切拒绝仍为驱动写出的 `failed+dispatched=false` 收据。
- **无网关路径逐字保留**：`approval_requirement` 的 else 分支（有 gate 必问/无 gate 直
  授）未触碰；十套件（background_steering 8/8、request_id_canonicalization 7/7 等钉数
  精确）+A15 28 叶+A16 四相全绿。
- **kernel 零变化**：subagent.rs 词表与判定（authorize_child_tool/_with_registry_id）
  未改一行；kernel lib 77 全绿（含两词表×两 tier 等价扫描）。
- T02 成果复验：13/13（含 f01_* 四条——read-only 子代理注册 Read 类派发、衰减仍拒写、
  委派 first-party 限定）。

## 7. 两层自查记录（普通→对抗）

普通逐项：§4 表逐 ID 给出前提/动作/预期/实际/退出码；每个拒绝场景都有合法对照（合法写
在 operate 下执行、读类全档放行、批准往返真实执行）。

对抗性自查（推翻自己的尝试与结果）：
1. **A05"扩展资源范围"变体**：预授权形态的 B2（同工具不同 digest）覆盖"扩展路径"类
   攻击——完整 key 不命中→重审→零执行；等待形态的 B 腿证明 settled id 无法跨记录结算。
2. **A06 只测了 disable？** 自查发现后补 **uninstall 腿**（describe→TargetNotFound→
   零派发+not registered 错误）——禁用/卸载两形态都钉。
3. **并发消费的真实攻击面**：同 run 内 tool calls 顺序执行（驱动 for 循环）不构成并发；
   真实面=跨 run/跨 session park（已测）+**预授权原子双花**（tokio::join 恰一赢家，补
   测试）。跨会话重放 A 的 id 在 B 会话被归属校验统一拒绝。
4. **取消语义是否及时**：spawn_linked 的 CALL 级取消是 biased select——future 在 await
   点 drop→AbortOnDrop 及时标记（cancel 测试的 Refused(aborted) 证明，非 8s 超时后）。
5. **waiting_approval 期间 session busy**：同会话第二个 run 会被 busy gate 拒——A05 的
   A2 腿曾因此在 A2 未终态时提交 B2 而 Busy panic；修复为 await A2 settle（终态释放
   lease）后再提交。并发消费测试改用双会话（语义等价：record 层并发）。
6. **pending 视图值泄漏**：断言摘要为 `{path:str}`（键名+类型标签）；驱动侧对 wire 未带
   summary 的请求用 `summarize_arguments` 补齐（值零泄漏由 T01 的
   `summary_never_carries_values` 保证）。
7. **策略面重复 kernel 词表的风险**：read_only 用户会话的写类拒绝在 TS 是 session 层
   （非 subagent_access）——Rust 侧同层（layer "session"）；子代理 ReadOnly tier 不在
   策略面重复判定（单一判定点原则），Operate+ReadOnly 不可达形状防御性拒绝（安全方向）。
8. **CSPRNG 失败路径**：批准 id 铸造失败→Aborted（拒绝等待、零执行），与网关句柄熵失败
   同姿态，不降级为可猜测 id。

## 8. 未验证项与风险（如实登记）

1. **r00 环境项**：本轮通过、保留登记（§5.1）。
2. **跨平台**：仅 macOS arm64 实测；ApprovalService 为纯 Rust 领域逻辑无平台分支，本地
   结果不替代其他平台验证。
3. **映射决策——模式快照粒度**：现役执行层逐调用读模式 vs Rust run 准入快照（§2.3）。
   run 中途切档不追溯已驱动 run；对新提交立即生效。保守、确定性，无 TOCTOU；完整逐调
   用实时性可在 R06 会话状态集成时按需演进（当前无既有测试锁定逐调用语义——R03 冻结的
   是"派发时衰减"=快照语义）。
4. **映射决策——委派家族（subagent 工具）权限分类**：T01 注册面（测试形状 kind=Read）
   下，父 ask 会话调用 subagent 工具本身不弹批准；现役 TS 将 subagent 列入
   SIDE_EFFECT_TOOLS（ask 弹审）。**提权面已闭合**（子代理运行内写类被 SUP-01 拒），
   但"父 ask 下派发子代理"的弹审语义取决于该 target 的 PermissionKind 注册——真实第一
   方注册（SUP-04 归 T01 目录/T08 矩阵）时按现役 SIDE_EFFECT 分类为 Execute 类即自动
   获得 ask 弹审。不属本 Task 改 T01 冻结契约。
5. **资源集合绑定的当前形态**：批准绑定 capability（target_id+capability_base+规范
   化 digest）——digest 即"确切 invocation 的 capability key"（现役契约同构）；路径级
   ResourceRef 绑定归 T04（PreparedRecord 扩展位已留）。
6. **生产接线**：ApprovalService 是可注入的生产组件（policy+gate 同一 Arc）；生产默认
   （无网关）仍是 R03 形状。真实桌面/CLI 批准交互面（pending_of/answer 的 UI 包装）归
   R06+；本 Task 交付受认证最小接口（归属校验+形状摘要）。
7. **委派 prepared 句柄不经 execute_prepared 消费**（T02-R2 观察 3 沿用）：按 TTL 自然
   过期，行为正确；T04/T07 交接继续显式说明。
8. **测试替身边界**：CountingExecutor/ScriptedProvider/MarkerScriptedProvider/
   RejectAllGate 均为外部答复者/外部系统替身；**A05/A06/SUP-01/O05 的关键裁决全部由
   真实 ApprovalService/真实网关/真实 kernel 判定链产生**（§4 已逐条标注）；无替身替代
   被测网关、权限决定、PreparedInvocation 生成、journal、存储或监督。

## 9. 回退

回退范围=本 Task 修改：还原 §3 产品文件清单、删除 approval_service.rs 与
r04_t03_approval_service.rs、还原 TEST_MAP T03 条目即回到基线 39168bfb0 形态。R03 语义
由既有套件保护；ApprovalService 是注入面（不改变生产默认），删除不影响任何现有生产
路径。

## 10. 证据索引

- `artifacts/rust-tauri/R04/T03-E01/run_t03_validation.sh`——复跑脚本（绝对路径口径）。
- `gates/`：rust fmt/clippy（退出码见报告表+复跑脚本）、check_contracts /
  check_boundaries（含 selftest N1–N17）/ r04_t03_acceptance_tests（T03 11+lib 6、T02
  13+lib 7、T01 6、kernel 77）/ r03_g07_repair_suites / r03_t08_matrix_a15 /
  r03_t08_a16_seed 各 .log。
- `workspace-test-cargo-test-workspace-locked.log`——全 workspace 原始输出（788/0/0）。
- 新测试文件：`rust/crates/lingxi-service/tests/r04_t03_approval_service.rs`；
  ApprovalService 单测内嵌 `rust/crates/lingxi-service/src/approval_service.rs`。
