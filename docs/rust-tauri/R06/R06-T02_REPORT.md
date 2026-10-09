# R06-T02 token 预算、压缩与长会话 — 实施报告

- **TASK_ID**: R06-T02
- **TASK_BASE_SHA**: `b174e3ff44c61000abbb84595cf69dfad85dd531`（分支 `codex/rust-tauri-migration`）
- **CANDIDATE_SHA**: `b174e3ff44c61000abbb84595cf69dfad85dd531`（本任务改动**未提交**，以工作区形式交付；候选 = 基点 + 工作区改动）
- **CANDIDATE_DIGEST**（R3-F-02 修复后的口径）：计算口径与命令已固化为可执行脚本 `scripts/rust-tauri/r06_candidate_digest.sh`（本报告头部的声明口径与该脚本逐字一致）。**本报告在口径内，故报告正文不含 digest 值**；值在本报告定稿之后由该脚本计算，只落 `artifacts/rust-tauri/R06/T02-REPAIR-03/` 证据文件与本修复轮的交付消息。
  - 口径（与脚本逐字一致）：`git diff HEAD`（已跟踪改动的完整 diff，含暂存与未暂存）+ 全部未跟踪文件（`git ls-files --others --exclude-standard | LC_ALL=C sort`）的逐文件 sha256，再取总 sha256。排除：(a) `artifacts/rust-tauri/R06/` 下全部产物目录（审查/根因/修复证据——**含 digest 载体文件自身**，载体自引用在数学上无不动点，必须排除）；(b) 任何位置的候选 digest 值文件（`candidate_digest*.txt`——双保险；脚本自身不含 digest 值，留在口径内）。计算时点：全部口径内文件（含本报告）定稿之后。
  - 历史值（均被取代；R2 轮值经 R3 审查复算**不可复现**，根因见 §十六 R3-F-02：载体自引用 + 声明口径与执行口径不一致 + 计算时点早于报告定型）：R2 轮 `3cabcb4647fe1d53d68f330ee0df6ed85b8602a0aa564d066c963f00dbac5023`，R2 前轮 `02304fc62d4386ff7e66f2ef70fdb806a15ebc8e853c702bdf67d4eb2284aff6`，R1 执行轮 `c24b44052e68fb38882325969e4c9a632e44e1a9b906fb61af67dee63b97a8aa`。
  - 注意：工作区 `docs/rust-tauri/R06/R06_PROGRESS.json` 的改动由总控写入（T01 DONE / T02 IN_PROGRESS + task_base_sha），非本执行代理改动，但包含在 diff 口径内。
- **执行代理**: EXECUTOR-R06-T02（一次性执行代理，仅本任务）
- **工具链实测**: 见 `artifacts/rust-tauri/R06/T02/00_toolchain.txt`（`/Users/study_superior/.cargo/bin/rustc --version` 实际输出；全部命令只用 `/Users/study_superior/.cargo/bin/cargo`，遵守 T01 教训）。
- **锁文件**: `rust/Cargo.lock` sha256 = `259f983e98a4da13eab1b592c2efd7b79579ad6678a08995a4b9e3fca618eff3`，任务前后逐字节一致，未新增任何第三方依赖（含 dev-dependencies）。验证：`git diff --stat` 无 `Cargo.lock` 条目。

---

## 一、已完成的 Steps（任务书四步逐条核对）

| # | 任务书步骤 | 完成情况 |
|---|-----------|---------|
| ① | 迁移现役 compaction 触发、手动压缩、摘要上下文、缓存保留与错误恢复语义（不要仅按字符数切断聊天） | 完成。**触发**＝`core/session-compaction-runtime.ts:201` 的 `ratio >= FORCE(80%) ‖ shouldCompact(contextTokens, window, reserve)`，`reserve=max(16384, ceil(window×20%))`（同文件 L60/L83），`contextTokens=usage 总量+尾部工具结果估算`，窗口未声明/为 0 或 usage 不可得/为 0 → 不触发 → kernel `ContextBudget::evaluate`。**手动压缩**＝现役 `/compact` → `bridge-session-manager.ts compactSession`（L1710 起）→ 同一 `runCachePreservingCompactionForSession` 管线（`session-compactor.ts:1985`）→ `CompactionService::compact_now`（跳过阈值判定，执行管线与自动路径逐行同一；窗口未声明时按现役 reserve 公式缺省 16384）。**摘要上下文**＝现役九标题模板 + `Internal compaction-only run.` 指令（`session-compactor.ts:402` 起）→ `build_summary_instruction` / `build_repair_instruction`。**缓存保留**＝压缩请求形状 `[system, submission, ...live 历史, 指令]`（`cache-preserving-compaction-agent-run.ts`，L354-357 注释的线上等价形状）且工具快照随请求携带。**错误恢复**＝清洗（剥 `<mood>/<pulse>/<reflect>` 闭合块与围栏块、折叠空行）+ 九标题校验 + 一次 format_repair（`cache-preserving-compaction-agent-run.ts:162/199/590-623`）→ `sanitize_summary`/`validate_summary`/修复重试；**工具意图单次恢复**＝现役 `tool_recovery`（同文件 L122-135 `clonePlaceholderTools` + L523-538 `shouldStopAfterTurn`）：首次应答的单个工具意图由宿主铸造的 placeholder 结果（逐字 `PLACEHOLDER_TOOL_RESULT_TEXT`）应答后续跑，第二次意图或一次多调用 = 响亮失败（现役 toolViolation 两文本）；失败＝保留原上下文+明确错误，run 续跑。**从不按字符数切断聊天**：切点由 `plan_compaction` 在交换项边界（assistant 组/既有摘要）规划，先证明全部工具配对。 |
| ② | 预算为系统约束/当前请求/工具定义/历史/引用/输出留空间，用模型实际 tokenizer 或明确保守估算并标来源 | 完成。`TurnBudgetFacts` 五分量：系统约束（T01 冻结 artifact 渲染文本）、当前请求（submission）、工具定义快照、历史、尾部（usage 后新进工具结果）；`TurnBudgetReport` 逐分量记账并随 `Compacted` 返回。估算器＝T01 冻结的 `estimate_text_tokens`（CJK 字符 ×1.1、其余字符 ÷4、向上取整）——**明确标注的保守估算**（任务书允许的替代项），来源注释锚定 T01；判定主信号不用估算而用 provider 上报的真实 usage（`calculateContextTokens` 形态：input+output+cacheRead+cacheWrite——input 先按族旗标还原为现役口径再四分量饱和加，见 §五 inclusion 行与修订记录 F-02）。输出留空间＝摘要输出上限按现役族分流（REPAIR-R06-T02-R2 FIX-01）：optional-cap 族线上**不发**输出上限字段（provider 默认），required-cap 族回填 `min(model.maxTokens, contextWindow)`；现役公式 `max(512, floor(0.8×reserve))`（`session-compactor.ts:377`）只服务预算估算与 required 族双缺兜底（§十#5）。 |
| ③ | 工具调用与结果成对保留，待完成调用/批准/当前目标不被压掉，压缩摘要不是可信新系统指令 | 完成。**成对保留**：`plan_compaction` 先做全交换配对证明（tool_call_id → 归属 assistant 组；孤立结果/重复结果/结果先于调用 → `UnprovableToolPairs` 响亮失败），切点只落 assistant 组边界或既有摘要项，保留区首项永不为 ToolResult；四族渲染器线体断言（每个 tool 消息都有先行 assistant 调用）。**待完成调用**：pending（未闭合）组是切点硬上界（kernel 测试 `planner_pending_tail_is_always_retained`）；待批准工具调用在 R04 语义下即未闭合调用，同上界保护。**当前目标**：submission 不在交换内（渲染顺序 system→submission→prior），永不可压。**摘要非系统指令**：`ExchangeItem::CompactionSummary` 在四族渲染器一律渲染为 user 角色消息（现役 `convertToLlm` 包装文本 `The conversation history before this point was compacted into the following summary:` 逐字，来自上游 `@earendil-works/pi-agent-core` `harness/messages.js:1`），mid-run 追加独立 user 消息 `MIDRUN_COMPACTION_NOTICE`（`session-compaction-runtime.ts:79` 逐字，`[System compaction notice — not a user message]` 开头）；`summary_never_enters_any_system_slot` 对全部五族循环断言。 |
| ④ | 压缩本身使用 ModelGateway，失败保留原上下文并返回可诊断策略，不能截断重要内容继续宣称成功 | 完成。摘要调用走 `AuxiliaryExecutor::complete_step_for_operation`（压缩槽消费 ToolRequests 半臂以实现现役 tool_recovery；其余槽仍走 `complete` 单 turn 契约），operation 由 CompactionService 显式决策：**summarize 槽显式配置优先、槽未配置回退 chat 路由**（与现役 auxiliary-slots.ts 的 summarize 槽 `fallback:"chat"` 对齐——§十#17，R3-F-01 修复；回退决策只在调用方，执行器自身仍永不静默改道）→ `ConfigModelGateway::resolve_dispatch` → `CredentialService`（零旁路；能力门、凭证、网络平面全在链上）。失败语义：`Failed{detail}`（可诊断文本）+ 原交换逐字保留 + run 续跑 + 台账落 failed 行；绝不把空/不合规摘要写进历史（清洗+九标题校验不过 = 失败，不是降级写入）；`Cancelled` 臂台账 outcome=Cancelled、attempts=None（镜像 R05 RR1 F38 的 dropped-before-settlement 形状）。 |

## 二、已交付的 Deliverables

| 交付物 | 位置 | 说明 |
|--------|------|------|
| **ContextBudgeter** | `rust/crates/lingxi-kernel/src/compaction.rs`：`ContextBudget` / `TurnBudgetFacts` / `TurnBudgetReport` / `CompactionDecision` / `compute_reserve_tokens` / `context_tokens_from_usage` / `estimate_exchange_item_tokens` / `estimate_exchange_tokens` / `summary_output_cap` | 纯函数预算器：窗口→reserve/keep_recent 派生、五分量账目、FORCE/reserve 双触发线、判定报告。 |
| **CompactionService** | `rust/crates/lingxi-service/src/compaction.rs`：`CompactionService::{maybe_compact_mid_run, compact_now}` + `MidRunCompactionInput` / `CompactionOutcome`；kernel 侧 `plan_compaction` / `apply_plan` / `build_summary_instruction` / `build_repair_instruction` / `sanitize_summary` / `validate_summary` | 触发源无关的压缩管线（自动判定与手动入口共用 `compact_exchange` 执行体）：切点规划→summarize 路由→配额→摘要调用→清洗校验（一次修复）→apply。每次摘要模型调用落一行 `auxiliary.summarize` 台账（成功/失败/取消同纪律）。 |
| **长会话夹具** | kernel 测试 `long_exchange`/`assistant_turn`/`tool_result`/`golden_summary` 构造器（配对完整的长历史、pending 尾、既有摘要边界）；service 测试 `long_exchange`（6 组配对历史）+ `StubServer`（脚本化 SSE loopback）；闭环测试 `compacting_router`/`failing_summary_router`（大 content 工具轮堆历史） | 三个层级共用同一语义：历史可切点必须超 keep_recent=20000、配对必须完整。 |

## 三、实际生产调用链（无平行构造者）

```
lingxi-service 二进制（真 argv/--config）
  → ServiceState::bootstrap_with_deps            [lib.rs：wired_real_model_chain 分支]
      → ConfigModelGateway::from_validated(plane)        （models.chat.compat.contextWindow 等声明式 compat）
      → CredentialService::bootstrap(plane, …)           （凭证唯一来源）
      → AuxiliaryExecutor::new_with_network(gateway, credentials, …)
      → CompactionService::new(auxiliary, gateway, runs.quotas_shared(), clock)
      → RunSupervisor::with_compaction(service)
  → HTTP /sessions/:id/execute → SessionService::execute_submission_for
  → RunSupervisor::drive_run
      → 每 turn loop 顶（tool_snapshot 之后、call_deadline 之前）：
        CompactionService::maybe_compact_mid_run
          → resolve_dispatch(Chat) → compat.context_window（未声明→NotTriggered）
          → ContextBudget::evaluate(TurnBudgetFacts{…, last_usage: 最近一次 settle 的 ReportedUsage::Known})
          → Force → plan_compaction（配对证明→pending 上界→keep_recent 回退切点）
          → 生效摘要路由解析（§十#17，R3-F-01）：resolve_dispatch(Auxiliary(Summarize))
            优先；不可路由 → 回退 chat 路由快照（复用入口同代快照）；双缺 → Failed
          → fit 检查（生效路由窗口——未声明时取回退后路由窗口——×0.85；不通过 →
            无进展护栏或 HardTruncated 硬截断，不调模型——FIX-07）
          → QuotaManager.acquire(Model)（同一配额管理器）
          → AuxiliaryExecutor::complete_step_for_operation(生效 operation,
              AuxiliaryRequest{prompt=submission,
              system_prompt=T01 artifact, prior=[…exchange, CompactionInstruction],
              tools=Some(live 快照), max_output_tokens=按族分流 Option<u32>
              （optional 族 None=线上无键；required 族 Some(min)——FIX-01）})
              ── ToolRequests 半臂：首次单调用意图 → placeholder 结果入会话 →
                 恰好一次恢复调用（现役 tool_recovery）；二次意图/多调用 → Failed
          → sanitize_summary → validate_summary（不过→一次 repair 调用→再不过→Failed）
          → apply_plan(…, mid_run) → exchange = [CompactionSummary{mid_run=触发源}] + 保留区
      → 下一 turn 的 ModelTurnInput.prior = 压缩后交换（渲染器把摘要项渲染为
        user 消息 + notice；线体配对完整）
```

测试替身边界：`ScriptedUsageProvider`（service 测试）与 loopback `StubServer`（全部测试）只扮演外部模型应答，逐字记录请求形状；从不决定权限/运行状态/审批。闭环测试（`r06_t02_closed_loop.rs`）走真二进制子进程——上链全链无注入点。

## 四、代码修改清单

已跟踪文件（`git diff --stat`）：

| 文件 | 改动 |
|------|------|
| `rust/crates/lingxi-kernel/src/lib.rs` | +1：`pub mod compaction;` |
| `rust/crates/lingxi-kernel/src/model_exchange.rs` | +25：`ExchangeItem::CompactionSummary{summary, covered_items, mid_run}` / `ExchangeItem::CompactionInstruction{text}` 两变体（含文档注释：摘要=普通历史身份，绝不进系统槽） |
| `rust/crates/lingxi-adapters/src/models/openai_completions.rs` | +23：摘要/notice/指令三渲染臂（user 角色） |
| `rust/crates/lingxi-adapters/src/models/anthropic_messages.rs` | +20：同上（user turn） |
| `rust/crates/lingxi-adapters/src/models/google_generative_ai.rs` | +25：同上（user parts） |
| `rust/crates/lingxi-adapters/src/models/openai_responses.rs` | +32：同上（input_text message；codex 经 `render_input_items` 继承） |
| `rust/crates/lingxi-adapters/src/models/auxiliary.rs` | +167/−39（R3 轮后累计）：`AuxiliaryRequest` +3 字段（`system_prompt`/`prior`/`tools`）；`complete_step` 暴露 ToolRequests 半臂（R06-T02 压缩槽 tool_recovery），`complete()` 委托之并按槽区分工具意图失败消息（F-06）；R3 轮：`complete_step` 委托新增 `complete_step_for_operation`（operation/label 显式形参——回退决策只在调用方，执行器自身永不回退，§十#17/R3-F-01），四处失败消息改用 label（summarize 槽文本逐字不变）；R05 行为不变 |
| `rust/crates/lingxi-service/src/lib.rs` | +64：`pub mod compaction;`；`ServiceDeps.compaction_service`（Default None）；bootstrap 的 wired_real_model_chain 分支构建生产 CompactionService 并 `with_compaction` 接线 |
| `rust/crates/lingxi-service/src/runs.rs` | +121：`RunSupervisor.compaction` 字段 + `with_compaction` + 两构造点补 None；drive_run 的 `last_turn_usage`/`tail_from`/`compaction_seq` 状态、loop 顶触发点、usage 捕获点、两处 AssistantTurn push 后的 tail_from 推进 |
| `rust/crates/lingxi-service/src/workermodel.rs` | +6：`AuxiliaryRequest` 唯一其他构造点补三字段 None/空 |
| `rust/crates/lingxi-service/tests/r05_t05_timeouts.rs` | +8：`describe_prior` 加两个穷举臂（编译必需，非语义改动） |
| `rust/crates/lingxi-adapters/src/models/config.rs` | +4（REPAIR-R06-T02-R2，FIX-01）：`RouteCompatHints` 增 `output_cap_required`（camelCase=outputCapRequired）+ 文档映射行——N-8 表达面关闭 |
| `rust/crates/lingxi-adapters/tests/r05_t05_compat.rs` | +1（REPAIR-R06-T02-R2）：全字段字面构造补 `output_cap_required`（FIX-01 加字段的连带编译修补，非语义改动） |
| `docs/rust-tauri/R06/R06_PROGRESS.json` | 总控改动（T01 DONE / T02 IN_PROGRESS），非本代理改动 |

新增文件（未跟踪）：

| 文件 | 行数 | 内容 |
|------|------|------|
| `rust/crates/lingxi-kernel/src/compaction.rs` | 937 | ContextBudgeter + planner（含保留区无孤儿校验）+ CacheInclusion + 摘要指令/清洗/校验/估算（纯函数，零 IO） |
| `rust/crates/lingxi-service/src/compaction.rs` | 969（R3 轮后） | CompactionService（全部 IO/台账/配额/取消 + placeholder 恢复臂 + R3-F-01 生效路由回退链） |
| `rust/crates/lingxi-kernel/tests/r06_t02_compaction.rs` | 980 | kernel 单测（36 → R2 后 44 个） |
| `rust/crates/lingxi-adapters/tests/r06_t02_compaction_render.rs` | 555 | 四族渲染断言（11 → R2 后 12 个） |
| `rust/crates/lingxi-service/tests/r06_t02_compaction.rs` | 2424（R3 轮后） | 26 个 service 测试（含 R3 新增 3 个回退链测试 + 交叉锁双向精确匹配重写与篡改自证） |
| `rust/crates/lingxi-service/tests/r06_t02_closed_loop.rs` | 908 | 2 个真二进制闭环（A03/A04 腿） |
| `scripts/rust-tauri/r06_candidate_digest.sh` | 26 | R3-F-02：候选摘要口径固化脚本（排除 digest 载体自身，声明口径=执行命令逐字一致） |

## 五、现役语义逐条锚定

| 语义 | 现役来源 | Rust 落点 |
|------|---------|----------|
| FORCE 线 80% | `session-compaction-runtime.ts:201`（`COMPACTION_FORCE_RATIO`） | `COMPACTION_FORCE_RATIO_BP=8_000`（基点） |
| reserve = max(16384, ceil(window×20%)) | 同文件 L60（`MIN_COMPACTION_RESERVE_TOKENS`）/L83（`computeCompactionReserveTokens`） | `compute_reserve_tokens`（`window.div_ceil(5).max(16_384)`） |
| 触发 = FORCE ‖ contextTokens > window−reserve | 同文件 L201 | `ContextBudget::evaluate` |
| contextTokens = usage 总量 + 尾部工具结果估算 | 同文件（`calculateContextTokens` + tail）；pi-ai `openai-completions.js:1173`（OpenAI 族 `input = prompt − cacheRead − cacheWrite`，cache-exclusive 口径） | `context_tokens_from_usage(usage, inclusion)`：先按族 inclusion 旗标把 wire input 还原为现役 cache-exclusive 口径（`cache_read_included_in_input == Some(true)` → 扣 cache_read；cache_write 同理），再四分量饱和加；全 None→None + `tail_tokens`（修订记录 F-02） |
| 族 inclusion 旗标（R05 RR1 F23 消费者纪律：included 分量绝不重复加计） | `rust/crates/lingxi-adapters/src/models/usage.rs` USAGE_MAPPINGS（OpenAI×3/Google `Some(true)`；Anthropic `Some(false)`） | service `usage_inclusion_of(chat_dispatch.route.protocol)` → kernel `CacheInclusion`；解码器保留 wire 事实不动，仲裁只在消费侧 |
| 窗口未声明/为 0、usage 不可得/为 0 → 不触发 | 同文件（`if (!(contextWindow > 0)) return false` 等） | `for_declared_window`→None / `CompactionDecision::Unavailable`→NotTriggered |
| keep_recent = 20000 | `session-compactor.ts:1695` | `KEEP_RECENT_TOKENS=20_000` |
| 切点候选集合与 snap 方向 | `findCutPoint`（pi-sdk `compaction.js:253-294`，经 `core/compaction-utils.ts:36`）：候选=除 toolResult 外全角色，跨界后**前跳**（取第一个 ≥i 的合法切点，保留区可 <keepRecent） | `plan_compaction`：候选=组边界（AssistantTurn‖CompactionSummary 且 index>0，结构继承——Rust 交换无 user 项），跨界后 `(1..=crossing).rev()` 统一扫描三条件（**回退**：组边界 ∧ ≤pending 硬上界 ∧ 保留区无孤儿（suffix 最小 owner 下标 ≥ cut）），无候选 = `Ok(None)`（宁可不压，绝不制造孤儿——修订记录 F-01）。**snap 方向与现役不同**（Rust 保留区恒 ≥keep_recent）——方向差异见 §十#11（REPAIR-R06-T02-R2 FIX-03 改锚） |
| 配对修复/拒绝（孤立 toolResult） | `session-compactor.ts`（`repairOrphanToolResultEntriesInFile` 读时修复 + provider-compat 兜底） | `plan_compaction` 规划期证明：孤立/重复/倒挂 → `UnprovableToolPairs` 响亮失败（不落账、不调模型） |
| 摘要指令模板（9 标题 + Internal compaction-only run. + split-turn scope 条件行） | `session-compactor.ts:402` 起（L410-414 是 isSplitTurn scope 行） | `build_summary_instruction`（边界按交换项数表述——记录在案差异 #1；split-turn 行按切点项类型两臂——FIX-05） |
| 清洗（剥 mood/pulse/reflect + 围栏 + 空行折叠） | `cache-preserving-compaction-agent-run.ts:162 sanitizeSummary` | `sanitize_summary` |
| 校验（非空 + 无未闭合标签 + 9 标题按序） | 同文件 L199 `validateSummary` | `validate_summary` |
| 一次 format_repair（携带问题清单 + `<draft-summary>` 内嵌**当轮 rawText**） | 同文件 L590-623（L601 载荷为 rawText，L596 validate 用 sanitized——五行之差） | `build_repair_instruction(&issues, &draft_raw)` + `compact_exchange` 修复臂（载荷与 prior 草稿 turn 均用 rawText——FIX-04） |
| 输出上限 max(512, floor(0.8×reserve))——**预算估算/BOUNDED/兜底三处用途** | `session-compactor.ts:377` `getCachePreservingCompactionMaxTokens` | `summary_output_cap`（公式逐字一致；用途锚定经 FIX-01 修正——线体默认按族分流，见下两行） |
| 摘要请求线体 max_tokens 族分流 | `compaction-guard-ext.ts:474-480` onPayload normalize → `session-compactor.ts:1867-1875` / `:313-352`（optional 族删全部 cap 字段；required 族回填 `min(model.maxTokens‖maxOutput, contextWindow)`）；族清单 `core/provider-compat/output-budget.ts` | kernel `output_cap_required`（族判定顺序逐字）+ `required_output_cap`；service 按**生效摘要路由**快照分流 `Option<u32>`——optional 族 None（线上无键）、required 族 Some(min)；config.rs `RouteCompatHints.outputCapRequired`（FIX-01）；生效路由随 R3-F-01 回退链（§十#17） |
| **摘要模型来源**（压缩摘要调用走哪个模型） | `session-compactor.ts:1987`（`model = session?.model`——**会话模型本人**；fit 检查 :2064-2074 与硬截断判定 :1654 用同一 model）；现役 `AUXILIARY_SLOTS.summarize`（`core/auxiliary-slots.ts:83-88`）只服务 activity 摘要/autolearn 且带 `fallback:"chat"`（`core/auxiliary-model-resolver.ts`：未配置→按 descriptor.fallback 回退 chat） | CompactionService 显式决策的**生效路由**：summarize 槽显式配置优先（可选覆盖），槽未配置/不可路由 → 回退 chat 路由，双缺 → Failed（detail 同时点名 summarize 与 chat）；经 `AuxiliaryExecutor::complete_step_for_operation` 点名 operation，执行器自身永不回退（R05 C07 不变）；台账 purpose 恒 `auxiliary.summarize`、served_by 携带 RESOLVED 路由身份——§十#17（R3-F-01） |
| 摘要请求自身超窗的 fit 检查与硬截断兜底 | `session-compactor.ts:2064-2083`（共享管线：摘要模型窗口 ×0.85 阈值——该窗口即 :1987 的会话模型窗口；不通过→硬截断 marker 摘要，不调模型） | kernel `cache_preserving_request_fits` + `HARD_TRUNCATE_MARKER_TEXT`（逐字）+ `COMPACTION_REQUEST_BUFFER_TOKENS=1024`；service fit 检查用**生效路由窗口**（未声明时取回退后路由窗口；两端皆未声明→0→同一硬截断臂）→ `HardTruncated` 变体（诚实降级 + 无进展护栏；native-fallback 臂无挂载面——FIX-07） |
| fileOps enrichment（`<read-files>`/`<modified-files>` 段 + 跨轮续传） | `session-compactor.ts:1893-1915`（五段链之 fileOps 段）+ `compaction.js:16-41`（续传）+ `compaction-utils.js`（computeFileLists/formatFileOperations） | kernel `extract_file_operations` / `append_file_operation_context` / `parse_file_operation_section`（modified=edited∪written、readOnly=read−modified、排序、逐字段格式；旧区最后一个摘要的段解析播种续传）；service validate 通过后追加（FIX-02；其余四段归因 REG-02） |
| stopReason∈{error,aborted} 不触发压缩 | `session-compaction-runtime.ts`（触发门子项） | runs.rs usage 捕获点：`ProviderTurn::Failed` 的 usage 不捕获（不触发）；重试后 loop 顶再评估（FIX-08） |
| 摘要包装文本（convertToLlm） | 上游 `@earendil-works/pi-agent-core` `harness/messages.js:1`（`COMPACTION_SUMMARY_PREFIX`） | kernel 同名常量逐字 + `render_summary_message_text` |
| mid-run notice | `session-compaction-runtime.ts:79`（`MIDRUN_COMPACTION_NOTICE` 逐字；L220-224 仅 mid-run 触发路径追加） | kernel 同名常量；渲染为独立 user 消息；**仅自动（run 内）压缩携带**——`apply_plan(…, mid_run)` 按触发源落标记（修订记录 F-04） |
| 手动压缩入口（/compact → 同一管线） | `bridge-session-manager.ts:1710 compactSession`（不追加 notice）→ `session-compactor.ts:1985` | `CompactionService::compact_now`（跳过判定，管线同一，产物 `mid_run=false` 不带 notice） |
| 摘要模型的工具意图单次恢复（tool_recovery） | `cache-preserving-compaction-agent-run.ts:122-135`（`clonePlaceholderTools` placeholder 应答逐字）+ L523-538（`shouldStopAfterTurn` toolViolation 两文本） | `AuxiliaryExecutor::complete_step` 暴露 ToolRequests 半臂；`compact_exchange` 恢复臂：首次单调用意图 → 会话追加意图 turn + placeholder 结果（`PLACEHOLDER_TOOL_RESULT_TEXT` 逐字）→ 恰好一次恢复调用；第二次意图（「after the first placeholder recovery turn」）或一次多调用（「ceiling exceeded」语义）= 响亮失败；工具从不执行（修订记录 F-03） |
| 压缩失败永不中断 run | `session-compaction-runtime.ts`（compactIfNeeded 失败只记日志） | `Failed{detail}` + run 以原交换续跑 |
| 压缩请求形状 = [system, submission, …live, 指令] | `cache-preserving-compaction-agent-run.ts`（L354-357 注释的线上等价） | `AuxiliaryRequest{system_prompt, prompt=submission, prior=[…exchange, Instruction], tools}` |

## 六、新增测试（82 个，全部通过；R3 轮后 = kernel 44 / adapters 12 / service 26 / 闭环 2）

（名单详列保持 R1 轮 66 个原貌——见下；REPAIR-R06-T02-R2 追加 13 个：kernel +8 `output_cap_required_matches_the_incumbent_capability_list` / `required_output_cap_is_the_incumbent_min_with_formula_fallback` / `split_turn_scope_line_rides_only_the_split_turn_arm` / `cache_preserving_request_fits_follows_the_085_window_threshold` / `file_ops_compute_the_incumbent_lists` / `file_ops_sections_append_only_when_non_empty` / `file_ops_chain_across_compaction_epochs` / `incumbent_semantics_checklist_stays_wired`；adapters +1 `output_cap_key_set_matches_the_incumbent_family_split`；service +6 `required_cap_family_backfills_min_of_declared_caps_on_the_wire` / `over_window_summary_request_hard_truncates_without_a_model_call` / `hard_truncate_no_progress_guard_skips_when_only_summaries_remain` / `file_ops_sections_ride_the_compacted_summary` / `failed_turn_usage_never_triggers_compaction` / `report_deviation_register_matches_test_references`；service 原有 17 个中 3 处断言随 FIX-01/04 强化——`trigger_fires…`/`manual_compaction…` 的 max_tokens 误锚断言改为「optional 族线上无任何输出上限字段」、`repair_flow_accepts_a_fixed_summary` 的载荷断言改锚 rawText。REPAIR-R06-T02-R3 追加 3 个：service `missing_summarize_slot_falls_back_to_the_chat_route` / `undeclared_summarize_window_falls_back_to_the_chat_window_not_hard_truncation` / `unresolvable_summarize_slot_and_chat_route_fail_loudly`（R3-F-01 两臂 + 双缺失败臂）；交叉锁 `report_deviation_register_matches_test_references` 重写为双向精确匹配 + 篡改自证四臂（R3-F-03，强化非新增）。）

kernel（36 → R2 后 44）：`reserve_floor_and_proportional_ratio` / `undeclared_or_zero_window_is_unavailable` / `unknown_usage_never_triggers` / `zero_usage_never_triggers` / `below_threshold_is_silent` / `force_line_at_eighty_percent_of_large_window` / `reserve_line_fires_before_eighty_percent_on_small_windows` / `tail_tool_results_since_last_usage_count_toward_trigger` / `budget_report_accounts_every_component` / `cache_components_count_toward_usage_total`（REPAIR 重写：族维度两臂，锚定注释修正）/ `cache_write_inclusion_is_arbitrated_the_same_way`（REPAIR 新增）/ `budget_judgement_uses_the_family_arbitrated_total`（REPAIR 新增：OpenAI 含 cache 口径不误判 Force）/ `planner_cut_respects_keep_recent_budget` / `planner_cut_never_lands_on_a_tool_result` / `planner_keep_recent_one_keeps_only_the_last_group` / `planner_pending_tail_is_always_retained` / `planner_never_orphans_in_either_direction`（REPAIR 重证：紧邻 + 夹层两种夹具 × 5 档 keep_recent 扫描）/ `planner_sandwich_shape_refuses_the_orphaning_boundary`（REPAIR 新增：审查探针 1 精确形状 → Ok(None)）/ `planner_falls_back_to_a_pair_whole_boundary_when_one_exists`（REPAIR 新增：孤儿化边界被否决后回退到更早安全边界）/ `a_compacted_exchange_stays_provable_on_the_next_pass`（REPAIR 新增：审查探针 3 级联无毒化）/ `planner_unprovable_pairs_fail_loudly` / `planner_no_beneficial_cut_returns_none` / `planner_previous_summary_is_a_valid_cut_boundary` / `apply_plan_replaces_old_region_with_one_summary_item` / `apply_plan_marks_the_notice_by_trigger_source`（REPAIR 新增：F-04 两臂）/ `sanitize_strips_closed_narration_blocks` / `sanitize_flags_unmatched_narration_tags` / `sanitize_collapses_blank_line_runs` / `validate_accepts_the_golden_summary` / `validate_rejects_empty_summary` / `validate_rejects_missing_and_misordered_headings` / `instruction_carries_incumbent_format_and_no_tool_rule` / `repair_instruction_names_issues_and_carries_draft` / `summary_output_cap_follows_the_incumbent_reserve_formula` / `exchange_estimate_counts_blocks_calls_and_results` / `summary_wrapper_and_notice_are_the_incumbent_texts`

adapters（11）：`openai_summary_notice_and_instruction_are_user_messages` / `openai_no_notice_when_not_mid_run` / `openai_compacted_exchange_has_no_orphan_tool_messages` / `anthropic_summary_notice_and_instruction_are_user_turns` / `anthropic_compacted_exchange_pairs_every_tool_result` / `google_summary_notice_and_instruction_are_user_parts` / `google_compacted_exchange_pairs_every_function_response` / `responses_summary_notice_and_instruction_are_input_messages` / `responses_compacted_exchange_pairs_every_call_output` / `codex_inherits_the_summary_rendering` / `summary_never_enters_any_system_slot`

service（17 → R3 后 26）：`trigger_fires_and_compacts_through_the_real_chain` / `provider_failure_keeps_the_original_exchange_and_accounts_it` / `repair_flow_accepts_a_fixed_summary` / `repair_exhaustion_is_a_loud_failure` / `placeholder_recovery_answers_a_single_tool_intent_and_compacts`（REPAIR 新增：F-03 恢复成功 + 线体 placeholder 配对 + 台账 2 行）/ `second_tool_intent_after_recovery_fails_loudly`（REPAIR 新增：现役 toolViolation 文本）/ `multiple_tool_calls_in_one_turn_fail_loudly`（REPAIR 新增：ceiling 语义）/ `below_threshold_never_calls_the_model` / `undeclared_window_never_triggers` / `unknown_usage_never_triggers_the_service` / `capability_gate_refuses_a_toolsless_summarize_route` / `unprovable_pairs_fail_without_a_model_call` / `cancellation_during_the_summary_call_accounts_cancelled` / `manual_compaction_skips_the_threshold_and_compacts`（REPAIR 增补 mid_run==false 断言）/ `driven_run_compacts_mid_run_and_continues_with_the_summary` / `driven_run_survives_a_failed_compaction` / `without_compaction_service_the_drive_shape_is_unchanged` / `missing_summarize_slot_falls_back_to_the_chat_route`（R3 新增：F-01 臂 A——槽未配置→回退 chat 路由，线体 model=chat-model + 台账随行）/ `undeclared_summarize_window_falls_back_to_the_chat_window_not_hard_truncation`（R3 新增：F-01 臂 B——槽显式覆盖但窗口未声明→取回退后路由窗口正常压缩，不再硬截断）/ `unresolvable_summarize_slot_and_chat_route_fail_loudly`（R3 新增：双缺→Failed，detail 同时点名 summarize 与 chat，0 物理调用，原交换逐字不动）

闭环（2，真二进制）：`a03_mid_run_compaction_lands_on_the_real_binary` / `a04_summary_provider_failure_keeps_the_original_history`

证据（完整命令回显 + 真实退出码 + rustc 版本）：`artifacts/rust-tauri/R06/T02/00_toolchain.txt` ~ `07_test_workspace.txt`。

## 七、现有回归测试（真实命令与退出码）

- `cargo fmt --all -- --check` → exit 0（`01_fmt.txt`）。
- `cargo clippy --workspace --all-targets --locked -- -D warnings` → exit 0（`02_clippy.txt`）。
- `cargo test --workspace --locked` 全量（含 R00-R06 全部既有套件与本任务 57 个新测试）→ exit 0，122 个 test target 全部 ok，**1585 passed / 0 failed / 0 ignored**（`07_test_workspace.txt`）。首轮全量曾在 `r04_rr1_f05_spill_failure` 目标处崩溃（测试进程无任何断言输出即中断，当时机器上有其他会话的重负载进程在跑）；该目标单独重跑 2/2 绿，全量重跑 exit 0——判定为环境压力，非代码回归。
- 单个新测试目标的独立运行证据：`03_test_kernel.txt`（30）/ `04_test_adapters.txt`（11）/ `05_test_service.txt`（14）/ `06_test_closed_loop.txt`（2）。
- REPAIR-R06-T02-R1 轮的复检证据（修复后真实退出码）：`artifacts/rust-tauri/R06/T02-REPAIR-01/00_toolchain.txt` ~ `10_test_workspace.txt`（fmt/clippy/66 个本任务测试/R05 邻接面/全量），修复前后探针对照 `01_prefix_probe_verdicts.txt` / `02_postfix_probe.txt`。
- REPAIR-R06-T02-R2 轮的复检证据：`artifacts/rust-tauri/R06/T02-REPAIR-02/00_toolchain.txt` ~ `11_candidate_digest.txt`（其中 11 号的 digest 值经 R3 审查复算不可复现——根因与修复见 §十六 R3-F-02）。
- REPAIR-R06-T02-R3 轮的复检证据：`artifacts/rust-tauri/R06/T02-REPAIR-03/`（修复前 R3 探针重放 `01_prefix_probe_d2d3.txt`、R3-F-01 红→绿 `02_red_r3f01.txt`/`05_green_r3f01.txt`、R3-F-03 红→绿 `03_red_r3f03.txt`/`04_green_r3f03.txt`、最终全量验证与 digest 证据——见 §十六 本轮验证）。

## 八、每个 Acceptance ID 对应证据

### R06-A03：长历史尾部含未闭合工具调用时触发自动压缩 → 不留孤立 tool result、不丢待完成调用

事实澄清（报告义务）：在 drive_run 生产循环中，压缩判定在 **turn 的 loop 顶**执行，此时上一次模型调用的工具结果已全部追加——尾部恒为**闭合**状态，「尾部含未闭合调用」的形状在真实循环不可达。因此 A03 的证据是三层断言的合取：

1. **planner 层（kernel）**：`planner_pending_tail_is_always_retained` 精确构造 pending 尾（未闭合调用组）并断言切点 ≤ pending 起点（待完成调用永不入旧区）；`planner_cut_never_lands_on_a_tool_result` / `planner_never_orphans_in_either_direction`（双向无孤儿——被压区的调用其结果必同区，保留区的结果其调用必同区或被摘要覆盖）；`planner_unprovable_pairs_fail_loudly`（孤立/重复/倒挂结果 = 响亮失败）。
2. **渲染层（adapters）**：四族 `*_compacted_exchange_has_no_orphan_*` / `*_pairs_every_*`——压缩后交换上线时每个 tool 消息都能在前面的 assistant 消息里找到归属调用；`summary_never_enters_any_system_slot`（五族）。
3. **生产链层（service drive + 真二进制闭环）**：`driven_run_compacts_mid_run_and_continues_with_the_summary` 断言压缩后首个模型调用的 prior[0]=compaction_summary、保留区逐字等于原交换尾部；`trigger_fires_and_compacts_through_the_real_chain` 对真实出站请求体断言配对完整（每个 role=tool 有先行 tool_calls）+ 缓存保留形状 + 台账；`a03_mid_run_compaction_lands_on_the_real_binary` 在真二进制上断言：摘要请求 1 次（身份=summarize 槽模型、**线上无 max_tokens 键**——openai-completions 属 optional-cap 族，§十#5/FIX-01、末尾指令、自身配对完整）、压缩后 chat 请求的历史以现役包装 user 消息开头、notice 独立 user 消息在场、被压旧区（`call_t02_read_0`）离开线体、保留区（`call_t02_read_1..3`）配对完整、system 槽恰 1 条、数据库 `auxiliary.summarize` succeeded 行 1 + chat succeeded 行 5。

### R06-A04：摘要 provider 返回错误时触发压缩并重开历史 → 原始记录保留、错误明确、不把空摘要覆盖历史

1. **直测**：`provider_failure_keeps_the_original_exchange_and_accounts_it`——stub 回 content_filter（非重试协议级拒绝）→ `Failed{detail}`（detail 含可诊断文本）+ 原交换逐字不动（12 项全在、无摘要项）+ 台账 failed 行 1（attempts=Some(1)）+ chat 行 0（对照：原始记录未受污染）。
2. **drive 腿**：`driven_run_survives_a_failed_compaction`——压缩失败后 run 以原交换续跑完成（第 5 次调用 prior=4 个原 assistant 项、无摘要），台账 chat 5 行全 succeeded + summarize failed 1 行。
3. **真二进制腿**：`a04_summary_provider_failure_keeps_the_original_history`——run completed；末次 chat 请求无任何摘要包装消息、原始历史（含 `call_t02_read_0`）原样在线；数据库 `auxiliary.summarize` failed 行 1 + chat succeeded 行 5。
4. **重开历史对照**（数据库与上下文对照）：run 的 exchange 是内存态（R03 语义），「原始记录」的持久化对应物是台账与 run 行——两腿都断言了 chat 行数与 outcome 不变、失败行落账；空/不合规摘要另有 `repair_exhaustion_is_a_loud_failure`（两次不合规 → Failed，绝不写入）与 `repair_flow_accepts_a_fixed_summary`（一次修复成功 → 落清洗后文本）夹住「不把空摘要覆盖历史」的另一侧。

## 九、R00 原始断言对应结果

`R06_PROGRESS.json` 的 `r00_leaf_map` 自述：「T02/T08 为横切任务无直接叶子」。本任务无 R00 叶子需逐条回填；横切保护由 §七 的全量回归（R00-R06 全部既有套件绿）与 §十一 的接口兼容结论承担。

## 十、记录在案差异（与现役的偏离清单；现役语义 × Rust 状态 40 行完整矩阵归位）

本节是「现役语义 × Rust 状态」矩阵（`artifacts/rust-tauri/R06/T02-ROOTCAUSE-01/ROOT_CAUSE_SWEEP.md` §七，40 行）的完整归位：矩阵中「已对齐/已修复/不适用」之外的每一行在此各占一条（或并入其所属条）。每条 = 差异 + 理由 + 去向；REPAIR-R06-T02-R2 修复轮新增的条目以（R2 新增）标注，R3 轮以（R3 新增）标注。**防再发（RC-3 §五-4）：测试代码中以 `§十#N` 引用本清单条目，由 service 测试 `report_deviation_register_matches_test_references` 机器交叉锁——每条目至少一次引用，且代码引用集合与条目集合精确相等（R3-F-03 强化：双向精确匹配，「删中间条目+引用残留」组合经篡改自证必红）。**

1. **指令边界表述单位**：现役模板按「消息数」表述旧区边界（`liveMessageCount - retainedMessageCount`）；Rust 版按**交换项数**（`plan.cut_index`）表述。语义等价（现役消息与交换项在压缩域一一对应），模板其余文本逐字。
2. **读时孤儿修复（`repairOrphanToolResultEntriesInFile`）未迁移**：它是现役对**落盘 jsonl 历史**的读时修复；Rust 的 run 交换是内存权威（R03），历史持久化面归 R06-T04——Rust 侧等价保护是 `plan_compaction` 的规划期证明（不可证明即拒绝压缩，run 不受影响）。（编号沿用 R1 台账；原 #2「`COMPACTION_REQUEST_BUFFER_TOKENS` 未迁移」已被 FIX-07 关闭——该常量随 fit 检查一并迁移，见 #12。）
3. **手动压缩的 slash 命令面未接**：`/compact` 的派发属壳层/CLI，且 run 外会话历史面归 R06-T04；本任务交付触发源无关的执行入口 `compact_now`（直测证明跳过判定 + 缺省 reserve），命令面到来时直接调用。（原 R1 #4，编号随 #2 关闭前移。）
4. **reasoning-only turn 在 openai 族渲染时消失**：R05 既有渲染器形态（空文本+空调用跳过），T02 未改；drive 测试的 Continue 轮全部带 tool_calls 或大 content，不受影响。（原 R1 #6。）
5. **摘要请求 max_tokens 线体族分流**（R2 新增，FIX-01/矩阵 #24-27，R2-F-01 + N-8）：现役 PROVIDER_DEFAULT 生产链（`compaction-guard-ext.ts:474-480` onPayload normalize → `session-compactor.ts:1867-1875` / `:313-352`）按族分流——**optional-cap 族**（openai-completions/openai-responses/codex/google/deepseek）`delete` 全部输出上限字段（线上无键）；**required-cap 族**（explicit-required/anthropic-native/bedrock-native/anthropic-messages）回填 `min(model.maxTokens‖maxOutput, contextWindow)`。Rust 对齐：service 按 summarize 路由快照分流 `Option<u32>`（optional 族 None；required 族 `Some(required_output_cap(compat.max_tokens, compat.context_window, reserve))`，双缺回退 `max(512, floor(0.8×reserve))`）；族判定 = kernel `output_cap_required`，顺序与现役 `output-budget.ts` 逐字（declared `outputCapRequired===true` → deepseek provider/endpoint(optional) → anthropic provider/endpoint(required) → bedrock(required) → AnthropicMessages 族(required) → 默认 optional）；`RouteCompatHints` 新增 `outputCapRequired` 字段（deny_unknown_fields 下可表达，N-8 关闭）。**差异声明**：现役 `explicit-required` 族（配置驱动「本模型就是 required」）在 Rust 由 `outputCapRequired: true` 声明承担同一职责；现役公式 `max(512, floor(0.8×reserve))` 只服务预算估算与 required 族双缺兜底，不是线体默认。
6. **enrichment 段族只迁移 fileOps 段，其余四段归因**（R2 新增，FIX-02/矩阵 #21-22，R2-F-02）：现役摘要后处理五段链（`session-compactor.ts:1893-1915`）中仅 **fileOps** 段（`<read-files>`/`<modified-files>`，无外部依赖）已迁移（kernel `extract_file_operations`/`append_file_operation_context`，modified=edited∪written、readOnly=read−modified、排序、跨轮续传从旧区最后一个摘要的同名段解析播种）。其余四段依赖未迁移子系统，登记 **REG-02** 归因：skill-recall → R06-T06；plan-file → plan 模式任务；context-notes → context_notes 工具任务；history-recovery → R06-T04（持久会话）。
7. **硬截断兜底 marker 文本 = 共享管线变体**（R2 新增，FIX-07 相关）：现役硬截断 marker 有两处措辞变体——共享管线版（`session-compactor.ts` 的 `[由于对话过长且压缩请求本身会超限，早期对话历史已被硬截断（hana-cache-preserving-compaction）]`）与 guard-ext 版（含「摘要请求本身会超限」措辞，经 `core/compaction-utils.ts computeHardTruncation` 落 details 仅 `{reason, keepRecentTokens}`，**无 fileOps 段**）。Rust 采用共享管线版逐字（`HARD_TRUNCATE_MARKER_TEXT`），硬截断产物不再追加 fileOps 段（与现役 computeHardTruncation 一致）。
8. **ASK 事件线（0.5 阈值 + 5% 重问增量/20% 重置降幅）未迁移**（R2 新增，FIX-06/矩阵 #7，I-1）：现役 `session-compaction-runtime.ts` 的 ASK 线是「压缩前再问一次模型」的交互面；Rust 无对应交互面（壳层问答面未接），kernel 保留死常量 + 码内声明（`compaction.rs` L47-51 注释）。归配置/壳层面任务。
9. **settings.enabled 用户开关与 keepRecent/reserve 设置覆盖面未接**（R2 新增，FIX-09/矩阵 #1/#9，N-3/N-7）：现役触发门第一项是用户级 `settings.enabled`，且 settings 可覆盖 keepRecent/reserve；Rust 服务配置面无现役 settings 系统，`KEEP_RECENT_TOKENS=20_000` / `MIN_COMPACTION_RESERVE_TOKENS=16_384` 常量冻结。归配置面任务。
10. **估算器口径：Rust 对齐 hana CJK×1.1，现役切点/尾部链是 pi-sdk 纯 chars/4**（R2 新增，FIX-10/矩阵 #12，N-4）：现役自身两子系统口径不一致（切点规划与 mid-run 尾部估算用 `compaction.js:166-204` 纯 chars/4 无 CJK 加权；hana 自家 `lib/llm/estimate-text-tokens.ts` 是 CJK×1.1）。Rust `estimate_text_tokens` 与 hana 估算器同源。后果：CJK 密集会话下 Rust 切点/尾部估算比现役切点链**高估约 2.75 倍**，方向保守（更早触发、保留区按现役口径实量更小），不产安全事故。
11. **切点 snap 方向：Rust 回退 vs 现役前跳**（R2 新增，FIX-03/矩阵 #13-14，R2-F-03）：现役 `findCutPoint`（`compaction.js:253-294`，L266-273）跨界后**前跳**（取第一个 ≥i 的合法切点，保留区可 <keepRecent；候选=除 toolResult 外全角色）；Rust `plan_compaction` 跨界后**回退**（跨界项计入保留区，保留区恒 ≥keep_recent；候选=组边界——Rust 交换无 user 项，结构继承）。方向分叉功能上更强（保留区更大 + 无孤儿校验），行为保留不改，锚定修正见 §五 L106 与 §十三风险#1。
12. **压缩请求自身超窗兜底：fit 检查 + 硬截断已迁移；native-fallback 臂无挂载面**（R2 新增，FIX-07/矩阵 #31，N-1/I-2；R3 轮改锚）：现役 `session-compactor.ts:2064-2102` 三层降级（fit 检查→硬截断→native-fallback error）。Rust 迁移前两层：fit 检查窗口——**现役该处窗口是会话模型窗口**（`session-compactor.ts:2064-2074` 与硬截断判定 `:1654` 的 model 即 `:1987` 的 `session.model`；R2 轮误锚为「summarize 路由窗口」，R3-F-01 改锚）；Rust 用**生效摘要路由**窗口（R3-F-01 修复后：summarize 槽显式覆盖 → 槽窗口；槽未配置回退 chat 路由 → chat 窗口；槽窗口未声明 → 回退后路由窗口；两端皆未声明 → 0 → 不通过，与现役 `contextWindow<=0 → shouldHardTruncate` 同一臂，§十#17），阈值 `floor(window×0.85)`（`HARD_TRUNCATE_THRESHOLD_BP=8_500`），估算总量 = 系统提示 + submission + 交换 + 指令 + 工具 schema + `COMPACTION_REQUEST_BUFFER_TOKENS=1_024` + `summary_output_cap(reserve)`；不通过 → `apply_plan(HARD_TRUNCATE_MARKER_TEXT)` 硬截断（不调模型，天然消除每 turn 重试），产物 `CompactionOutcome::HardTruncated`（诚实降级，驱动 warn 日志显式标注）。无进展护栏：旧区只剩摘要/指令项（无可再切内容）→ `NotTriggered`（对应现役 `effectiveCutIndex≤0 → null`，防同文标记每 turn 互替循环）。**native-fallback error 臂在 Rust 无挂载面**（现役该臂回落到 SDK 原生压缩路径，hana 生产链不走原生路径、矩阵 #37 已判现役死代码）——统一落硬截断。
13. **customInstructions 注入面未接**（R2 新增，FIX-11/矩阵 #19，N-5）：现役摘要指令支持用户 customInstructions 注入；Rust `SummaryInstructionSpec.custom_focus` service 侧恒 None（kernel 构造面存在）。归配置面任务。
14. **L1 会话级 tool_result 32KB head+tail 截断 guard 未迁移**（R2 新增，FIX-12/矩阵 #34，N-9）：现役 `compaction-guard-ext.ts` L1 钩子对**所有** tool_result 做 32KB head+tail 截断（会话级通用兜底）；Rust 无会话级通用截断 guard，仅 exectools 有 per-tool `max_output_bytes` + `truncated` 诚实旗标（工具级有界，部分覆盖）。会话级通用兜底待评估。
15. **压缩作用域 = 当前 run 的 exchange（现役 = 整会话）**（R2 新增，FIX-13/矩阵 #35，N-10）：现役摘要请求历史含多 user 消息（跨 submission 会话史）；Rust run 级 exchange 单 submission、assistant/toolResult only（R03/R05 结构继承）。归 R06-T04（持久会话）。
16. **stopReason 门按 Rust 结构落位**（R2 新增，FIX-08/矩阵 #3，N-2）：现役触发门含「role==assistant 且 stopReason∉{error,aborted}」；Rust 无 stopReason 字段，等价语义落在 usage 捕获点——`ProviderTurn::Failed` 的 usage 不捕获（失败 turn 不触发压缩评估）；取消臂由 run 取消面处理（aborted 不进入 usage 捕获）。**差异残留**：现役失败 turn 后 usage 纪元继续推进；Rust 失败 turn 后 `last_turn_usage` 保持上一成功值，重试 `continue` 回 loop 顶会按旧 usage 再过一次压缩检查（方向安全，多一次评估）。
17. **摘要模型路由：Rust = summarize 槽显式覆盖 + chat 回退；现役 = 会话模型本人**（R3 新增，R3-F-01）：现役压缩摘要调用用 `session.model`（`session-compactor.ts:1987` `model = session?.model`；fit 检查 `:2064-2074` 与硬截断判定 `:1654` 用同一 model），根本不经辅助槽；现役 `AUXILIARY_SLOTS.summarize`（`core/auxiliary-slots.ts:83-88`）只服务 activity 摘要/autolearn 且带 `fallback:"chat"`（`core/auxiliary-model-resolver.ts`：未配置 → 按 descriptor.fallback 回退 chat；已配置不可用 → 配置错误不 fallback）。Rust 把压缩摘要绑到 summarize 辅助槽是架构决策（槽语义即「摘要」，能力门/凭证/台账纪律随槽面复用），为消除「槽未配置 → 压缩永不发生（每 turn Failed+warn，run 续跑至上下文无限增长）」与「槽配置但窗口未声明 → 每次触发硬截断（marker 替换真实历史）」两个生产可达退化面，按现役 fallback 对齐实现**显式回退链**（决策只在 CompactionService，可声明、可测试；执行器自身仍永不静默改道——R05 C07 不变）：**summarize 槽显式配置 → 用槽路由（可选覆盖）；槽未配置/不可路由 → 回退 chat 路由（自动路径复用入口同代快照，手动路径需要时补一次解析）；双缺 → Failed，detail 同时点名 summarize 与 chat**。fit 窗口（#12）、族分流与 max_tokens 回填（#5）、能力门、凭证/端点全部随生效路由快照走（同族横扫结论见 §十六）。台账 purpose 恒 `auxiliary.summarize`（语义用途），served_by 携带 RESOLVED 路由身份（回退臂 = chat 路由身份，不伪造槽身份）。行为级回归：§六 service 名单 R3 三测试（臂 A 槽缺失→回退 chat；臂 B 槽窗口未声明→回退路由窗口而非硬截断；双缺→Failed 且 0 物理调用、原交换逐字不动）。**「槽缺失即压缩失败」「槽窗口未声明即静默硬截断」的默认行为已删除，不复存在。**

矩阵中未单列的「不适用」行（防后续审查误报）：#5 usage 纪元检查（结构性不适用）、#36 usage 纪元投影（结构性不适用）、#37 原生压缩路径 maxTokens 公式（现役死代码）、#38 SDK post-run backstop（无挂载面，登记 **REG-01** 归 R06-T04）、#39 reasoning 级别参数（R05 面，非 T02 分叉）。

## 十一、R05 接口兼容结果

- **ExchangeItem 加两变体**：全部穷举 match 消费者已更新——四族渲染器（openai_completions/anthropic_messages/google_generative_ai/openai_responses，codex 经共享函数继承）+ `r05_t05_timeouts.rs::describe_prior`（+8 行穷举臂）。非穷举消费者（`matches!`/if-let）不受影响。
- **AuxiliaryRequest 加三字段**（`system_prompt`/`prior`/`tools`）：`complete()` 对 None→空快照透传，R05 行为逐字节不变（workermodel 唯一其他构造点补 None/空）；`AuxiliaryOutcome`/`AuxiliaryFailure` 形状未动。
- **能力门复用而非修改**：压缩请求带非空工具快照 → **生效摘要路由**（summarize 槽显式覆盖或 chat 回退——§十#17）必须声明 `tools:true`，否则本地响亮拒绝（0 物理请求、台账 attempts=Some(0)）——`capability_gate_refuses_a_toolsless_summarize_route`（槽配置但能力不足 = 配置错误，不触发回退，与现役 auxiliary-model-resolver「已配置不可用即配置错误」同臂）。
- **台账**：复用 `record_model_call_usage`/`ModelCallUsageRecord` 既有表与 `CallOutcome` 既有变体；purpose=`auxiliary.summarize`（与 workermodel 回调台账同一命名法）。
- **零改动面**：R03 Run/Attempt 生命周期与终态机、R04 统一工具网关与审批面、R05 协议契约（ProtocolFamily/DeltaNormalizer/usage 解析）、CredentialService 内部、T01 ContextCompiler——全部未触碰（git diff 可证）。
- **不接线时零行为变化**：`without_compaction_service_the_drive_shape_is_unchanged`（ServiceDeps 默认 None → drive 形状与 pre-T02 一致，零摘要台账行）。

## 十二、未验证事项

- 真实付费供应商：NOT_RUN（全部 loopback 确定性替身，遵守任务书；STUB_API_KEY 是字面量占位）。
- Windows 平台：NOT_RUN（无 Windows 环境）。
- anthropic/google/responses/codex 四族的**真二进制**压缩闭环：NOT_RUN（四族渲染臂由 adapters 单测覆盖；真二进制腿走 openai-completions 一族——渲染臂是纯函数且四族同构，风险低）。
- 同 run 多次压缩（compaction_seq>1 的生产触发）：NOT_RUN（直测覆盖 seq 参数化与「既有摘要项是合法切点」的 kernel 形态；drive/闭环各触发一次）。
- `compact_now` 的生产调用方：NOT_RUN（无宿主——见差异 #3；直测覆盖其执行语义）。
- 压缩中途取消的 drive 级形态：NOT_RUN 于 drive 层（直测 `cancellation_during_the_summary_call_accounts_cancelled` 覆盖 service 层 Cancelled 臂与台账形状；drive 层 `Cancelled → continue 'turns` 走既有取消结算）。
- quota 拒绝臂：NOT_RUN（QuotaManager 默认配额充裕；拒绝路径代码与 summarize 路由失败同构， detail 文本可诊断）。
- R3-F-01 回退链的真二进制闭环形态：NOT_RUN（闭环夹具 summarize 槽已配置，走显式覆盖臂；回退两臂 + 双缺臂由 service 直测覆盖——线体 model 字段、台账身份、窗口来源均断言）。

## 十三、已知风险

1. **反复压缩同一段**：保留区按组**回退**可超 keep_recent（**与现役 findCutPoint 方向不同**——现役是前跳、保留区可 <keepRecent；差异与理由见 §十#11，REPAIR-R06-T02-R2 FIX-03 改锚）；若每轮 usage 都过线且保留区始终 >keep_recent，会每轮各压一次（不循环——summarized=0 时 plan=None 停止；硬截断另有「旧区仅摘要」无进展护栏，§十#12）。感知层面摘要链变密。缓解锚点：usage 门要求新 settle 的 usage（loop 顶只在 turn 边界评估）。
2. **小窗口（window<16384）时触发线饱和为 0**：任何非零 usage 都过 reserve 线（现役 `window - reserve` 负值同形）。测试正依赖此形状（window=1000）；生产中小窗口配置会更激进压缩。
3. **估算口径是保守估算而非真 tokenizer**（任务书允许的标注替代）：CJK×1.1/其余÷4 与真实 BPE 有偏差（CJK 偏高、代码/token 密集文本偏低）。判定主信号用真实 usage，估算只用于尾部增量与切点规划——偏差有界。
4. **StubServer 硬编码 200 OK**：provider 错误用 SSE 内 `finish_reason:"content_filter"` 表达（非重试协议级拒绝）；HTTP 层 4xx/5xx 的压缩失败形态未在闭环演练（AuxiliaryExecutor 的重试/失败语义由 R05 套件覆盖，压缩只消费其 Outcome/Failure 二分）。
5. **多次压缩间的 `tail_from`/`compaction_seq` 状态**：drive_run 内压缩成功后 `tail_from=exchange.len()`（尾部清零）——若压缩当轮的工具结果在压缩后追加，tail 从新交换尾部起算，语义与现役「usage 之后的尾部」一致；仅单压缩场景有测试覆盖（见未验证 #4）。

---

**STATUS: READY_FOR_INDEPENDENT_REVIEW**

---

## 十四、修订记录（REPAIR-R06-T02-R1，2026-10-09）

第 1 轮独立验收（`artifacts/rust-tauri/R06/T02-REVIEW-01/REVIEW.md`）裁决 FAIL，六个 findings。本轮按「先复现 → 同根因扫描 → 修复 → 回归测试 → 全量验证」修复；旧报告内容按上述行内修订保持与代码一致，本节承载修复矩阵。**验收结论仍属独立复验方，本节不自称 PASS。**

### 修复矩阵

| Finding | 根因 | 修复 | 回归测试 | 修复前复现 | 修复后验证 |
|---------|------|------|----------|-----------|-----------|
| F-01 HIGH：plan_compaction 未实现「保留区无孤儿 toolResult」保证 | 配对证明只建「result→owner 下标序」，切点回退只按项类型，从不校验 owner 同区；执行者夹具 owner-result 恒紧邻使旧断言恒真 | kernel `plan_compaction`：O(n) 预计算 `suffix_min_owner`（位置 ≥ i 的全部 ToolResult 的 owner 最小下标），切点候选三条件统一一次扫描（组边界 ∧ ≤pending 上界 ∧ `suffix_min_owner[cut] ≥ cut`），无候选 → `Ok(None)`（宁可不压）；`apply_plan` 信任 plan 的前提由 planner 的不变量兑现 | kernel：`planner_sandwich_shape_refuses_the_orphaning_boundary`（审查探针 1 精确形状 → Ok(None)）、`planner_falls_back_to_a_pair_whole_boundary_when_one_exists`（孤儿化边界被否决后回退到 cut=3 配对完整边界）、`a_compacted_exchange_stays_provable_on_the_next_pass`（探针 3 级联：安全压缩产物再次规划绝不 UnprovableToolPairs）；`planner_never_orphans_in_either_direction` 改用紧邻+夹层两种夹具 × keep_recent∈{1,2000,6000,12000,20000} 扫描 + `assert_plan_has_no_orphans` 双向重证 | `T02-REVIEW-01/probe_output.txt`（探针 1 `cut_index=1` / `ORPHANED tool result tc0001` / `VULNERABLE`；探针 3 `ORPHAN POISONS ALL FUTURE COMPACTIONS`）；REPAIR 代理动手前亲跑同一探针独立复现，verdicts 逐字一致（`T02-REPAIR-01/01_prefix_probe_verdicts.txt`） | `T02-REPAIR-01/02_postfix_probe.txt`：探针 1 → `Ok(None)`；探针 3 → 首次 cut=12 配对完整、二次压缩 `Some(6)` 无孤儿；kernel 目标 36/36 绿 |
| F-02 HIGH：OpenAI×3/Google 族 cache_read 触发判定双计 | 两侧 input 口径不同：现役 pi-ai input 是 cache-exclusive（`prompt − cacheRead − cacheWrite`），R05 统一结构 input 是 wire 总量（含 cache）；旧实现四分量无条件相加 | kernel `context_tokens_from_usage(usage, inclusion)` 增 `CacheInclusion` 入参：旗标为 true 的分量先从 input 饱和扣除再四分量相加（included 分量扣了再加回 = 算术等价于不计；半截 usage 不虚构扣除）；service `usage_inclusion_of(chat_dispatch.route.protocol)` 经 R05 `USAGE_MAPPINGS` 解析旗标（OpenAI×3/Google `Some(true)`，Anthropic `Some(false)`，无映射→false/false）；解码器/台账保留 wire 事实不动（R05 设计），仲裁只在消费侧 | kernel：`cache_components_count_toward_usage_total` 重写（OpenAI 形状 100500 / Anthropic 形状 3014 / 全缺 None，锚定注释修正）、`cache_write_inclusion_is_arbitrated_the_same_way`、`budget_judgement_uses_the_family_arbitrated_total`（window=100_000 下 OpenAI wire 79_000 含 cache 30_000 → 79_100 BelowThreshold；对照 separate 口径 → Force） | 同上证据文件：探针 2 `rust context_tokens = Some(130500), incumbent-equivalent = 100500` / `DOUBLE-COUNTS cache_read` | `02_postfix_probe.txt`：OpenAI 族 `Some(100500)` = 现役逐字一致、Anthropic `Some(3014)` 不变；探针同时断言族旗标本体（`Some(true)`/`Some(false)`） |
| F-03 MEDIUM：placeholder 工具单次恢复未迁移且误述 | AuxiliaryExecutor 单 turn 契约直接复用，任何 ToolRequests 立即失败 | 按任务书「迁移现役错误恢复语义」**实现**：`AuxiliaryExecutor::complete_step` 暴露 ToolRequests 半臂（其他槽 `complete()` 行为不变）；service `compact_exchange` 恢复臂——首次单调用意图：会话式 prior 追加意图 turn + placeholder 结果（`PLACEHOLDER_TOOL_RESULT_TEXT` 逐字：「Tool intent was preserved for protocol continuity. No live tool was executed. Continue by returning the structured compaction summary without tools.」）→ 恰好一次恢复调用；第二次意图 → Failed（「tool intent appeared after the first placeholder recovery turn」）；一次多调用 → Failed（「requested N tool calls in one turn; the placeholder recovery answers exactly one call exactly once」）；修复调用上的意图同样 Failed；工具从不执行；台账每物理调用一行，意图行 Succeeded + emitted 占位调用身份（F21/F38 纪律） | service：`placeholder_recovery_answers_a_single_tool_intent_and_compacts`（两发请求、恢复线体 placeholder 逐字配对、台账 2 行 Succeeded + emitted id）、`second_tool_intent_after_recovery_fails_loudly`、`multiple_tool_calls_in_one_turn_fail_loudly` | 静态对照（现役 L122-135/L523-538 vs 旧 auxiliary.rs:264-273）；旧行为 = 任何意图即失败 | service 目标 17/17 绿；三测试线体断言真实（第二发请求含逐字 placeholder 与配对 tool 消息） |
| F-04 LOW：手动压缩产物硬编码 mid_run=true | `apply_plan` 无触发源参数 | `apply_plan(exchange, plan, summary, mid_run)` 参数化；`compact_exchange(…, mid_run)`：自动路径 true、手动 `compact_now` false（现役 compactSession 不追加 notice） | kernel：`apply_plan_marks_the_notice_by_trigger_source`（两臂 + 保留区逐字一致）；service：`manual_compaction_skips_the_threshold_and_compacts` 增补 `mid_run == false` 断言 | 静态（旧唯一构造点硬编码 true） | kernel/service 目标全绿 |
| F-05 LOW：报告「字节÷4」笔误 | 文档措辞错误（代码 `text.chars()` 码点迭代正确） | §一步骤②改为「其余字符 ÷4」（本行） | 不需要（文档） | REVIEW.md F-05 | 本行即修复 |
| F-06 LOW：工具回调失败消息误称「declared NO tools」 | 失败消息未按槽区分 | `complete()` 的工具意图失败消息按请求是否声明工具分流：压缩槽（声明了快照）→「declared tools for history wire-name resolution ONLY (an auxiliary call never executes tools) but the provider answered with tool requests; the calls were NOT executed」；未声明槽 → 原「declared NO tools…」文本 | adapters 既有套件全绿（`capability_gate_refuses_a_toolsless_summarize_route` 是路由声明面，不受影响）；F-03 三测试覆盖压缩槽回调路径 | 静态（旧消息文本） | adapters 全量绿（全量工作区测试） |

### 同根因扫描结论（修复矩阵之外的同类路径）

- `apply_plan` 全部调用点：service 2 处 + kernel 测试——均已按新签名传触发源。
- `context_tokens_from_usage` 唯一供血点 = `ContextBudget::report`/`evaluate`；`cache_read_tokens` 其他消费者均为台账/解码（正确口径，不动）。
- `complete()` 其他调用方仅 workermodel（`tools: None`）——失败消息「declared NO tools」臂对该面保持逐字。
- 台账面不受影响（R05 台账口径独立，wire 事实保留）。

### 本轮验证（真实退出码，证据 `artifacts/rust-tauri/R06/T02-REPAIR-01/`）

- `cargo fmt --all -- --check` → exit 0（首轮报 3 处排版差异，`cargo fmt --all` 应用后复检 0）。
- `cargo clippy --workspace --all-targets --locked -- -D warnings` → exit 0。
- 本任务测试目标：kernel r06_t02_compaction 36/36、adapters r06_t02_compaction_render 11/11、service r06_t02_compaction 17/17、service r06_t02_closed_loop 2/2 —— 全 exit 0。
- R05 邻接面：r05_t01_binary_wiring 2/2、r05_t05_timeouts（service 9/9 + adapters 13/13）、r05_t08_closed_loop 11/11、r05_t08_production_tools 6/6、r05_t08_resources 2/2 —— 全 exit 0。
- `cargo test --workspace --locked` 全量：见 `10_test_workspace.txt`（退出码与计数随证据落盘）。
- 修复后探针（`T02-REPAIR-01/probe/`，新签名独立 crate）：三探针 ALL SAFE，exit 0（`02_postfix_probe.txt`）。

---

## 十五、修订记录（REPAIR-R06-T02-R2，2026-10-09）

第 2 轮独立验收（`artifacts/rust-tauri/R06/T02-REVIEW-02/REVIEW.md`）裁决 FAIL，五项 findings（R2-F-01..F-05）+ 两项 informational（I-1/I-2）。总控启动根因扫描（`artifacts/rust-tauri/R06/T02-ROOTCAUSE-01/ROOT_CAUSE_SWEEP.md`），归并三个根因簇（RC-1 勘察单元错位 / RC-2 差异台账失信 / RC-3 自洽测试盲区），追加十项新发现（N-1..N-10）与 40 行现役语义 × Rust 状态矩阵，开出 13 项 FIX + 2 项 REG。本轮按根因扫描报告一次性消除同根因簇全部缺陷；**验收结论仍属独立复验方，本节不自称 PASS。**

### 修复矩阵（每项 FIX：根因 → 修复 → 回归测试 → 修复前复现 → 修复后验证）

| FIX | 关联 | 根因 | 修复 | 回归测试 | 修复前复现 | 修复后验证 |
|-----|------|------|------|----------|-----------|-----------|
| FIX-01 | R2-F-01 + N-8（矩阵 #24-27） | RC-1+RC-2：勘察止于预算公式函数，未沿 PROVIDER_DEFAULT 生产链追到 onPayload normalize 的族分流；§十#5 把公式误锚为线体行为 | kernel 新增 `output_cap_required`（族判定顺序与现役 `output-budget.ts` 逐字）与 `required_output_cap`（min 回填，双缺回退公式）；service 按 summarize 路由快照分流 `Option<u32>`（optional 族 None=线上无键；required 族 Some(min)）；`config.rs` 增 `RouteCompatHints.outputCapRequired`（deny_unknown_fields 下可表达，N-8 关闭）；kernel `summary_output_cap` 文档改锚（公式只服务预算估算/BOUNDED/required 族兜底，非线体默认——公式本身不修） | adapters `output_cap_key_set_matches_the_incumbent_family_split`（五族键集合黄金形状：None→openai×3/google 无键、anthropic 键恒在；Some(v)→各族自己的键携带 v）；service `required_cap_family_backfills_min_of_declared_caps_on_the_wire`（anthropic 两个 min 方向各一例）；三处误锚断言改为「optional 族线上无任何输出上限字段」（service 两处 + 闭环 L746） | `T02-ROOTCAUSE-01` R2-F-01 四处行号互证；闭环 L745 曾断言 `max_tokens=13107`（=Rust 自己的公式输出）当作「与现役一致」——台账失信固化进测试的实证 | 修复后闭环/直测断言 openai 线上无键、anthropic=min(声明值)；adapters 12/12、service 23/23、闭环 2/2 绿 |
| FIX-02 | R2-F-02（矩阵 #21-22） | RC-1：摘要管线勘察止于 validate/repair，未读 session-compactor.ts:1893-1915 的后处理五段链 | kernel 新增 `extract_file_operations`（扫旧区 AssistantTurn.tool_calls 中 wire 名 read/write/edit + arguments.path；modified=edited∪written、readOnly=read−modified、排序）+ `parse_file_operation_section`（从旧区最后一个摘要的 `<read-files>`/`<modified-files>` 段解析播种续传，取最后一次出现）+ `append_file_operation_context`（清单均空→逐字原样；否则追加两段）；service 两处 validate 通过后 apply_plan 前追加。其余四段归因 REG-02 | kernel `file_ops_compute_the_incumbent_lists` / `file_ops_sections_append_only_when_non_empty`（逐字段格式）/ `file_ops_chain_across_compaction_epochs`（续传 + round_trip）；service `file_ops_sections_ride_the_compacted_summary`（含 `<read-files>`、旧区每个 read path 在列） | R2-F-02 grep 零命中对照现役五段 | kernel 44/44、service 23/23 绿；续传 round_trip 断言下轮可解析 |
| FIX-03 | R2-F-03（矩阵 #13-14） | RC-1+RC-2：读 findCutPoint 时把「合法切点集合不含 toolResult」误推为「跨界项计入保留区」，未逐行走查循环本体（L266-273 是前跳） | 三处误锚改锚（只改锚不改行为）：kernel 注释改「回退 vs 现役前跳」；报告 §五 L106 切点行改锚；§十三风险#1 改锚；§十 增列 #11 方向差异条 | 无需新行为测试（现有 planner 测试已固定 Rust 方向）；语义清单锚定测试含此条目 | `T02-ROOTCAUSE-01` Probe E 实证（同形状 rust cut=2 retained=25289 vs incumbent cut=4 retained=228） | 报告/注释三处锚定与代码方向一致；kernel 44/44 绿 |
| FIX-04 | R2-F-04（矩阵 #29） | RC-1：agent-run.ts:596（validate 用 sanitized）与 :601（repair 用 rawText）相差 5 行被合并为「sanitized 一路到底」 | service repair 臂 `build_repair_instruction(&issues, &draft_raw)` + prior 草稿 turn 用当轮 rawText（原两处 sanitized.text）；sanitize 只服务 validate 与最终落库 | service `repair_flow_accepts_a_fixed_summary` 强化：`<draft-summary>` 内嵌含 `<mood>busy</mood>` 的 raw 草稿，断言 assistant 草稿消息含可剥除内容（contains——渲染层尾部空白修整与载荷语义无关） | `T02-REPAIR-02/04_red_fix04.txt`：载荷临时回退 sanitized.text → 强化断言红 EXIT=101 | 恢复 rawText 后 service 23/23 绿 |
| FIX-05 | R2-F-05（矩阵 #18） | RC-1：指令模板按文本面逐字迁移，未迁移基于切点形状的条件分支 | kernel `SummaryInstructionSpec` 增 `split_turn: bool`；`build_summary_instruction` 在 recent-tail 行之后、custom_focus 之前插入 split-turn scope 行（逐字 "This is a split-turn compaction: preserve the original request and early progress needed to understand the retained suffix."）；service 按切点项类型传参（切点=AssistantTurn→true，CompactionSummary→false） | kernel `split_turn_scope_line_rides_only_the_split_turn_arm`（两臂断言） | R2-F-05 grep 零命中 | kernel 44/44 绿；service 全链 split_turn 传参经既有测试覆盖 |
| FIX-06 | I-1/I-2（矩阵 #7/#31） | RC-2：码内已声明但 §十 未列 | §十 新增 #8（ASK 死常量+码内声明）与 #12（hard-truncate 出口——随 FIX-07 落地为已迁移前两层+native-fallback 无挂载面声明） | 语义清单锚定测试含条目 | 文档核对（I-1/I-2 原文） | §十 单一事实源建立 |
| FIX-07 | N-1（矩阵 #31，决策项） | RC-1：现役 fit 检查→硬截断→native-fallback 三层降级链未勘察到；Rust 每 turn 顶重发注定失败的摘要请求无熔断 | 决策 = 实现诚实硬截断兜底（对现役前两层）：kernel `COMPACTION_REQUEST_BUFFER_TOKENS=1_024` / `HARD_TRUNCATE_THRESHOLD_BP=8_500` / `HARD_TRUNCATE_MARKER_TEXT`（共享管线版逐字）/ `cache_preserving_request_fits`（摘要模型窗口 ×0.85；window≤0→false）；service fit 检查（估算=系统+submission+交换+指令+工具+1024+公式上限）不通过 → 无进展护栏（旧区仅摘要/指令→NotTriggered，防同文标记互替循环）否则 `apply_plan(marker)` → `CompactionOutcome::HardTruncated`（诚实降级、不调模型、台账不落摘要行、驱动 warn 日志）；native-fallback 臂无挂载面统一落硬截断（§十#12） | service `over_window_summary_request_hard_truncates_without_a_model_call`（summarize 窗口 1000：HardTruncated、marker 逐字、保留区逐字、mid_run=true、stub.hits()==0、无台账行）/ `hard_truncate_no_progress_guard_skips_when_only_summaries_remain`（旧区仅摘要→NotTriggered）；kernel `cache_preserving_request_fits_follows_the_085_window_threshold`（85000/85001 边界 + floor 语义） | `T02-REPAIR-02/02_red_service_fix01_fix07.txt`：fit 落地后未声明 summarize 窗口的夹具全部走硬截断（现役 window≤0 语义）→ 12/17 FAILED EXIT=101（夹具修声明后转绿，证明红是语义生效而非回归） | service 23/23、闭环 2/2 绿；硬截断不再发摘要请求（stub.hits()==0） |
| FIX-08 | N-2（矩阵 #3） | RC-1：触发评估只看 usage 是否 Known，失败 turn 的 Known usage 同样触发 | runs.rs usage 捕获点三分支 match：`(ProviderTurn::Failed, _) → None`（不捕获不触发）、`(_, Known(u)) → Some(u)`、其余 None；重试 `continue` 回 loop 顶再过压缩检查（红演示实证） | service `failed_turn_usage_never_triggers_compaction`（4 组交换 + Failed(retryable, 大 usage) + final：priors 6 次无 compaction_summary、hits==0、无台账行） | `T02-REPAIR-02/05_red_fix08.txt`：触发门临时回退 → 红 EXIT=101（prior #5 = ["compaction_summary","assistant",...]；夹具鉴别力经 3 组→4 组迭代后成立） | 恢复门后 service 23/23 绿 |
| FIX-09 | N-3/N-7（矩阵 #1/#9） | RC-2 | §十#9 声明（settings.enabled 与 keepRecent/reserve 覆盖面未接，常量冻结，归配置面任务） | 语义清单锚定测试含条目 | — | 文档核对 |
| FIX-10 | N-4（矩阵 #12） | RC-2 | §十#10 声明（估算器口径：Rust 对齐 hana CJK×1.1；现役切点链 pi-sdk 纯 chars/4；方向保守） | 语义清单锚定测试含条目 | — | 文档核对 |
| FIX-11 | N-5（矩阵 #19） | RC-2 | §十#13 声明（customInstructions 面未接，service 恒 None，归配置面任务） | 语义清单锚定测试含条目 | — | 文档核对 |
| FIX-12 | N-9（矩阵 #34） | RC-2 | §十#14 声明（L1 会话级 32KB 截断 guard 未迁移；per-tool 预算 + truncated 旗标部分覆盖；通用兜底待评估） | 语义清单锚定测试含条目 | — | 文档核对 |
| FIX-13 | N-10（矩阵 #35） | RC-2 | §十#15 声明（压缩作用域=当前 run 的 exchange；结构继承 R03/R05；归 R06-T04） | 语义清单锚定测试含条目 | — | 文档核对 |
| REG-01 | N-6（矩阵 #38） | — | 登记：post-run backstop 检查随 R06-T04 持久会话层一并设计（§十 末尾不适用行注明） | — | — | — |
| REG-02 | 四段 enrichment（矩阵 #22） | — | 登记：skill-recall→R06-T06 / plan-file→plan 模式 / context-notes→context_notes 工具 / history-recovery→R06-T04（§十#6） | — | — | — |

「不修」三项（根因扫描报告明确）未动：kernel `summary_output_cap` 公式本身（只改文档锚定）；Rust 回退切点方向本身（只改锚）；placeholder 恢复臂（F-03 已真实关闭）。

### 测试防再发（RC-3 §五 五设计）落实清单

| 设计 | 落点 | 状态 |
|------|------|------|
| ① 现役黄金线体 fixture 族断言 | adapters `output_cap_key_set_matches_the_incumbent_family_split`（五族键集合黄金形状 + 现役锚点行号注释）；service/闭环三处线体断言锚定 §十#5 | ✅ |
| ② 语义清单参数化测试 | kernel `incumbent_semantics_checklist_stays_wired`（常量锚定：FORCE/reserve/keep_recent/buffer/threshold/marker/split-turn 行/placeholder 文本逐字在场）；`output_cap_required_matches_the_incumbent_capability_list`（15 行族矩阵） | ✅ |
| ③ 指令模板两臂断言 | kernel `split_turn_scope_line_rides_only_the_split_turn_arm`（split_turn=true/false 两臂） | ✅ |
| ④ 报告-测试交叉锁 | service `report_deviation_register_matches_test_references`（读报告 + rust/ 测试与源码引用扫描：每条目至少一次引用，且代码引用集合与条目集合**双向精确匹配**——引用∉条目集合即红；R3-F-03 强化 + 篡改自证四臂） | ✅（R3 强化） |
| ⑤ 族矩阵三维参数化 | kernel 15 行族判定矩阵（USAGE 面已有 `cache_components_count_toward_usage_total` 等）× render 键集合五族 × service 两个 min 方向 | ✅ |

### 本轮验证（真实退出码，证据 `artifacts/rust-tauri/R06/T02-REPAIR-02/`）

- `00_toolchain.txt`：rustc/cargo 1.98.1。
- `01_prefix_findings_repro.txt`：R2 findings 修复前复现。
- `02_red_service_fix01_fix07.txt`：FIX-01/07 落地后夹具未声明 summarize 窗口 → 12/17 FAILED EXIT=101（红）。
- `03_green_misanchor_fix01_partial.txt`：夹具修声明后 kernel/service/闭环转绿（写于新测试追加之前，计数是当时值）。
- `04_red_fix04.txt` / `05_red_fix08.txt`：FIX-04/FIX-08 临时回退法演示真红（EXIT=101）后恢复转绿。
- 最终全量验证（真实退出码）：
  - `cargo fmt --all -- --check` → EXIT=0（`06_fmt.txt`；首轮 3 处排版差异，`cargo fmt --all` 应用后复检 0）。
  - `cargo clippy --workspace --all-targets --locked -- -D warnings` → EXIT=0（`07_clippy.txt`；过程中修三处 doc-lazy-continuation 注释排版 + r05_t05_compat 全字段构造补 `output_cap_required`——FIX-01 加字段的连带编译修补）。
  - 四任务目标（`08_test_task_targets.txt`）：kernel r06_t02_compaction 44/44、adapters r06_t02_compaction_render 12/12、service r06_t02_compaction 23/23、service r06_t02_closed_loop 2/2 —— 全 EXIT=0。
  - R05 邻接面（`09_test_r05_adjacent.txt`）：r05_t01_binary_wiring 2/2、r05_t05_timeouts（service 9/9 + adapters 13/13）、r05_t05_compat 1/1（adapters）、r05_t08_closed_loop 11/11、r05_t08_production_tools 6/6、r05_t08_resources 2/2 —— 全 EXIT=0。
  - `cargo test --workspace --locked` 全量（`10_test_workspace.txt` + 完整输出 `10_test_workspace.full.log`）：首轮在 R03 既有目标 `cancellation_tree::r03_a06_cancel_parent_spares_unrelated_background` 单点失败（时序/环境压力形态——单独重跑该目标 8/8 绿，非本任务改动面）；**全量重跑 EXIT=0：122 个 test target 全部 ok，1609 passed / 0 failed / 0 ignored**。

**STATUS: READY_FOR_INDEPENDENT_REREVIEW**

---

## 十六、修订记录（REPAIR-R06-T02-R3，2026-10-09）

第 3 轮独立对抗性审查（`artifacts/rust-tauri/R06/T02-REVIEW-03/REVIEW.md`）裁决 FAIL：一项 MEDIUM 阻断（R3-F-01 摘要模型路由架构分叉未声明 + §十#12 误锚）、两项 LOW（R3-F-02 候选摘要不可复现；R3-F-03 交叉锁鉴别力缺口 + 报告过述）。R1/R2 全部修复项与根因簇闭环经审查独立复证确认关闭。本轮按任务书选定口径修复：**summarize 槽未配置时回退 chat 路由（与现役 `core/auxiliary-slots.ts:83-88` 的 summarize 槽 `fallback:"chat"` 对齐），槽显式配置作为可选覆盖；窗口来源同步取回退后路由的窗口**。「槽缺失即压缩失败」「槽窗口未声明即静默硬截断替换真实历史」的默认行为已删除。**验收结论仍属独立复验方，本节不自称 PASS。**

### 修复矩阵

| Finding | 根因 | 修复 | 回归测试 | 修复前复现（红） | 修复后验证（绿） |
|---------|------|------|----------|-----------------|-----------------|
| R3-F-01 MEDIUM：摘要模型路由架构分叉未声明 + 误锚 | 执行者把压缩摘要绑到 R05 summarize 辅助槽（新架构决策，工程上可辩护），但 §十 16 条无一声明该分叉、§十#12 把 fit 窗口来源误锚（现役 = 会话模型窗口，非 summarize 槽窗口）、§五 锚定表无「摘要模型来源」行；行为后果二臂：槽未配置 → 压缩永不发生（每 turn Failed+warn，run 续跑至上下文无限增长）；槽配置但窗口未声明 → 每次触发硬截断（marker 替换真实历史） | **实现侧**：`compact_exchange` 步骤 4 改显式回退链——summarize 槽显式配置优先（可选覆盖）；不可路由 → 回退 chat 路由（自动路径复用入口同代快照传入 `chat_dispatch`，手动路径需要时补一次解析）；双缺 → `Failed`，detail 同时点名 summarize 与 chat。fit 窗口 = 生效路由窗口，未声明 → 取回退后路由窗口，两端皆未声明 → 0 → 现役同一硬截断臂；族分流/max_tokens 回填随生效路由快照（FIX-01 之上自动跟随）。`AuxiliaryExecutor` 新增 `complete_step_for_operation`（operation/label 显式形参——**回退决策只在调用方，执行器自身仍永不静默改道**，R05 C07 不变），`complete_step` 委托之（summarize 槽四处失败消息文本逐字不变）。台账 purpose 恒 `auxiliary.summarize`（语义用途），served_by 携带 RESOLVED 路由身份（回退臂 = chat 路由身份，不伪造槽身份）。**声明侧**：§十 新增 #17（分叉 + 回退链 + 三后果闭环）、#12 改锚（现役 fit 窗口 = 会话模型窗口）、§五 补「摘要模型来源」行、§一④/§三 生产链/§十一 能力门措辞同步 | service 三测试（§六 R3 名单）：臂 A `missing_summarize_slot_falls_back_to_the_chat_route`（线体 model=chat-model、台账 1 行 purpose=auxiliary.summarize 且 model=chat-model Succeeded）；臂 B `undeclared_summarize_window_falls_back_to_the_chat_window_not_hard_truncation`（槽显式覆盖但窗口未声明 → 取回退后 chat 窗口正常压缩，线体 model=summarize-model）；双缺臂 `unresolvable_summarize_slot_and_chat_route_fail_loudly`（Failed 且 detail 双点名、0 物理调用、原交换逐字不动、无台账行） | `T02-REPAIR-03/01_prefix_probe_d2d3.txt`（修复前重放 R3 探针：D2/D3 逐字复现审查观察，PROBE_EXIT=0）；`02_red_r3f01.txt`（修复前三测试各 CARGO_EXIT=101：臂 A 红于 `Failed{detail:"the summarize slot has no resolvable route…"}`、臂 B 红于 `HardTruncated`、臂 C 红于 detail 不含 chat） | `05_green_r3f01.txt`（三测试 + 其余 22 个既有测试全绿）；`06_postfix_probe_d2d3.txt`（修复后重放：探针 D3 旧断言 panic——「槽缺失即 Failed」旧行为消失，回退链接管后按回退后窗口走 fit 臂；D2 行根因切换为「回退后窗口真实不足」，附注说明） |
| R3-F-02 LOW：候选摘要不可复现 | 三层叠加：(a) digest 载体文件自引用（在口径内且含 digest 字符串，数学上无不动点）；(b) 声明口径（`grep -v '^docs/.../R06-T02_REPORT.md$'` 锚定路径）与执行命令（`grep -v 报告自身` 字面子串）不一致；(c) 计算时点早于报告/载体落盘 | 口径固化为可执行脚本 `scripts/rust-tauri/r06_candidate_digest.sh`：范围 = `git diff HEAD` + 未跟踪文件逐一 sha256 后总 sha256；排除 `artifacts/rust-tauri/R06/` 全部产物目录（含一切 digest 载体）与任何位置的 `candidate_digest*.txt`（双保险）；报告在口径内 → **报告正文不含 digest 值**；声明口径与脚本逐字一致；值在报告定稿后计算，只落 `T02-REPAIR-03/12_candidate_digest.txt` 与交付消息。报告头部 CANDIDATE_DIGEST 块改为口径说明（历史值保留并如实标注 R2 值不可复现及根因） | 无（流程项——口径即脚本，审查方可独立重放） | R3 审查 `T02-REVIEW-03/00_digest_diagnosis.txt`：按声明口径与执行口径重算及六种变体均 ≠ 自报值 | `T02-REPAIR-03/12_candidate_digest.txt`：脚本实际执行回显 + 真实退出码 + digest 值（报告定稿后计算，见本节末尾） |
| R3-F-03 LOW：交叉锁鉴别力缺口 + 报告过述 | stray 检查用 `> max` 近似「∉ 条目集合」（省了一个 contains）：「删 §十 中间条目 N、代码中 §十#N 引用残留」组合在残留引用 ≤ max 时逃逸检测；报告声称「恰好一次」而实现是「至少一次」 | 交叉锁重构为 `parse_register_entries` / `collect_code_references` / `check_register_lock` 三函数；stray 臂改 `!entries.contains(n)`——**双向精确匹配**（每条目 ≥1 处引用 ∧ 每个引用必须对应存在条目 ∧ 条目数 == 引用目标集合大小）；主测试 = 真实仓库检查 + 篡改自证四臂（反例先行验证变红）：(a) 条目{1,3}+引用{1,2,3} → 红（R3 探针 F1 逃逸组合）；(b) 条目{1,2,3}+引用{1,3} → 红（条目无引用）；(c) 集合精确相等 → 绿；(d) 端到端临时目录（篡改报告删中间条目 + probe.rs 残留引用，解析→扫描→判定全链）→ 红。报告 §十 引言与 §十五 row④ 措辞改「至少一次引用且集合相等」（与实现对齐） | 篡改自证四臂内嵌于 `report_deviation_register_matches_test_references` 自身（每次运行必过反例鉴别力） | `03_red_r3f03.txt`：旧 `> max` 逻辑 + 篡改自证 → 红于自证 (a)，CARGO_EXIT=101 | `04_green_r3f03.txt`：修复后自证四臂绿；真实仓库检查臂红于「code references unknown §十 entries: [17]」——强化锁正确抓到新代码引用 §十#17 而报告未落盘的真实状态（附注）；§十#17 落盘后 `07_green_cross_lock.txt` EXIT=0 |

### 同族横扫（「模型/窗口/上限的来源路由」透镜 × 压缩链 7 个配置来源，逐条对照现役）

| # | 配置来源 | 现役 | Rust（R3 修复后） | 结论 |
|---|---------|------|-------------------|------|
| 1 | fit 检查窗口来源 | **会话模型窗口**（`session-compactor.ts:1987` 的 model 即 `:1654`/`:2064-2074` 同一 model；`contextWindow<=0 → shouldHardTruncate`） | 生效路由窗口（summarize 槽显式覆盖 → 槽窗口；槽未配置回退 chat → chat 窗口；槽窗口未声明 → 回退后路由窗口；两端皆未声明 → 0 → 同一硬截断臂） | 已闭环（§十#12 改锚 + #17；臂 B 回归测试钉死「不再硬截断」） |
| 2 | max_tokens 线体来源 | PROVIDER_DEFAULT 族分流（`compaction-guard-ext.ts:474-480` → `session-compactor.ts:1867-1875`/`:313-352`），按压缩实际用模型（=会话模型）的族 | 按**生效路由**快照分流（`output_cap_required` 读生效路由 protocol/provider/endpoint + `compat.output_cap_required`；required 族回填读生效路由 `compat.max_tokens`/`context_window`） | 已闭环（FIX-01 之上随回退自动跟随——显式覆盖臂 = 槽族、回退臂 = chat 族，两路都忠实于实际用模型） |
| 3 | 族判定来源（触发侧 usage 的 cache inclusion 口径） | 会话模型族（触发判定跑在会话模型的 usage 上） | `usage_inclusion_of(chat_dispatch.route.protocol)`——恒为 chat 路由族（触发判定主信号是 chat 模型的 usage，与摘要路由无关） | 已闭环（本就取 chat 路由；触发≠摘要，回退链不触碰触发侧） |
| 4 | fallback 链 | summarize 槽 `fallback:"chat"`（`core/auxiliary-slots.ts:83-88`）：未配置 → 回退 chat；已配置不可用 → 配置错误不 fallback（`core/auxiliary-model-resolver.ts`）；压缩摘要本身用会话模型 | CompactionService 显式回退链：槽未配置/不可路由 → chat；槽配置但能力门拒绝 → 配置错误响亮失败、**不 fallback**（与现役 resolver 同臂）；双缺 → Failed 双点名 | 已闭环（§十#17；三测试覆盖两臂 + 双缺） |
| 5 | 启用开关来源 | `settings.enabled` 用户开关 + settings 可覆盖 keepRecent/reserve | 未接（§十#9：常量冻结，归配置面任务） | 已声明差异（R2 已登记，不在本 finding 范围） |
| 6 | keepRecent/reserve 来源 | settings 可覆盖；缺省 20000 / `max(16384, ceil(window×20%))`，window = 会话模型窗口（触发侧） | 常量冻结 `20_000` / `compute_reserve_tokens(chat 窗口)`（§十#9/#10）；触发侧窗口恒为 chat 路由窗口（`maybe_compact_mid_run` 入口解析，未声明 → NotTriggered） | 已闭环（窗口来源恒 chat = 现役「会话模型窗口」；回退链不触碰） |
| 7 | 摘要指令模板来源 | `session-compactor.ts:402` 起九标题模板 + `:410-414` split-turn 条件行 + customInstructions 注入面 | `build_summary_instruction`（§五 锚定行；split-turn 两臂 FIX-05；`custom_focus` service 恒 None——§十#13 已声明） | 已闭环（R2 已锚定；模板不随模型路由变——无新分叉面） |

横扫结论：7 个配置来源中，#1/#2/#4 随 R3-F-01 回退链实现闭环并有行为级回归测试钉死；#3/#6 本就取 chat 路由（触发侧），不受摘要回退影响；#5/#7 为已声明差异（§十#9/#13），无新分叉面。

### 本轮验证（真实退出码，证据 `artifacts/rust-tauri/R06/T02-REPAIR-03/`）

- `00_toolchain.txt`：rustc/cargo 1.98.1（48a229cea 2026-09-01）。
- 修复前复现：`01_prefix_probe_d2d3.txt`（R3 探针 D2/D3 重放，PROBE_EXIT=0）、`02_red_r3f01.txt`（三新测试修复前各 CARGO_EXIT=101）、`03_red_r3f03.txt`（旧交叉锁逻辑 + 篡改自证红，CARGO_EXIT=101）。
- 修复后行为翻转：`05_green_r3f01.txt`（service 26 测试 = 3 新 + 22 既有 + 交叉锁；当时唯一红 = 交叉锁正确抓到 §十#17 未落盘，附注）、`04_green_r3f03.txt`（交叉锁修复后自证四臂绿 + 真实检查臂红于 [17]——§十#17 落盘前的如实记录）、`06_postfix_probe_d2d3.txt`（探针 D3 旧行为消失 panic + 附注）、`07_green_cross_lock.txt`（§十#17 落盘后交叉锁 EXIT=0）。
- 最终全量验证：`08_fmt_clippy.txt`（FMT_EXIT=0 / CLIPPY_EXIT=0）、`09_test_task_targets.txt`（kernel 44/44、adapters 12/12、service 26/26、闭环 2/2，全 EXIT=0）、`10_test_r05_adjacent.txt`（R05 邻接面七套件）、`11_test_workspace.txt` + `11_test_workspace.full.log`（`cargo test --workspace --locked` 全量）、`12_candidate_digest.txt`（报告定稿后由固化脚本计算的候选摘要）。

**STATUS: READY_FOR_INDEPENDENT_REREVIEW**
