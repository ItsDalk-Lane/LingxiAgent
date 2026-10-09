# R06-T02 第二轮独立对抗性审查报告（REVIEWER-R06-T02-R2）

- **REVIEWER**: REVIEWER-R06-T02-R2（第二轮，一次性、独立、对抗；未参加实现与修复，非 R1 审查者）
- **TASK_ID**: R06-T02（token 预算、压缩与长会话）
- **TASK_BASE_SHA / 审查 HEAD**: `b174e3ff44c61000abbb84595cf69dfad85dd531`（分支 `codex/rust-tauri-migration`，`git rev-parse HEAD` 实测逐字一致）
- **修复者自报候选摘要**: `02304fc62d4386ff7e66f2ef70fdb806a15ebc8e853c702bdf67d4eb2284aff6`
- **摘要核对**: 按 `docs/rust-tauri/R06/R06-T02_REPORT.md` 头部 L7 的精确口径（`git diff` 输出 + 未跟踪文件逐一 sha256，排除报告自身、`T02-REVIEW-01/`、一切 `/probe/target/`；本代理增补排除自己的 `T02-REVIEW-02/`）重算，结果与自报候选**逐字一致**。
- **工具链**: 仅 `/Users/study_superior/.cargo/bin/cargo`（`cargo --version` = 1.98.1）。全部命令亲跑并记录真实退出码。
- **写盘纪律**: 未改动任何产品代码与测试实现；仅写 `artifacts/rust-tauri/R06/T02-REVIEW-02/`（本报告 + `probe/` + `probe_output.txt`）。

---

## 一、范围 A：从完整原始任务规格的独立重审

重审依据的原始规格（全部亲读，不以执行/修复者报告替代）：

- 任务书 `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R06_上下文、会话语义、记忆与知识库.md` L84-130（R06-T02 四步「怎么做」+ 必须交付 + A03/A04 + 禁止替代条款）；
- 02/01/05 约束文档（压缩前已读）；
- `docs/rust-tauri/R05/R05_HANDOFF.json`（compaction 相关条目，json walk 核对）；
- `docs/rust-tauri/R06/R06-T01_REPORT.md` §十（18 条冻结差异，含 #17 messages[0] 恒 system 槽）；
- 现役源码：`node_modules/@earendil-works/pi-agent-core/dist/harness/compaction/compaction.js`（findValidCutPoints/findCutPoint/prepareCompaction/compactWithRequest）、`core/session-compactor.ts`（触发/指令/摘要后处理/线体整备全链）、`lib/extensions/compaction-guard-ext.ts`、`lib/llm/cache-preserving-compaction-agent-run.ts`、`core/provider-compat/output-budget.ts`、`core/compaction-utils.ts`。

### 任务书逐条核对

| 任务书要求 | 核对结论 |
|---|---|
| ①迁移现役 compaction 触发、手动压缩、摘要上下文、缓存保留与错误恢复语义；不按字符数切断 | 触发（80% FORCE 线/reserve/keep_recent/真实 usage+尾部估算）已迁移且边界精度亲测（Probe D）；手动压缩入口 `compact_now` 在；缓存保留形状（摘要包装+notice 逐字、配对完整）经闭环测试与本代理精读核实；错误恢复（sanitize/validate/repair/失败保留原上下文）在。**但「摘要上下文」存在未迁移且未声明的段族（R2-F-02），线体 max_tokens 行为分叉且被误锚为「对齐现役」（R2-F-01）** |
| ②预算为系统约束/请求/工具定义/历史/引用/输出留空间；tokenizer 或保守估算并标来源 | `TurnBudgetReport` 五分量账目 + 输出 reserve 在；估算为保守估算且来源在注释/报告中标注。成立 |
| ③工具调用与结果成对保留，待完成调用/批准/当前目标不被压掉；摘要不是可信新系统指令 | 配对证明（`UnprovableToolPairs` 响亮拒绝）+ pending 组硬上界 + 保留区无孤儿三条件切点（Probe A/C 行为级复证）；摘要以 user 消息包装入场、非 system 槽。成立 |
| ④压缩本身使用 ModelGateway，失败保留原上下文并返回可诊断策略，不截断重要内容继续宣称成功 | 摘要走 auxiliary.summarize 路由（ModelGateway 面）；失败臂 exchange 不动、台账可诊断；无截断冒充成功路径（hard-truncate 出口未迁移，见 informational I-2）。成立 |
| 必须交付：ContextBudgeter / CompactionService / 长会话夹具 | `ContextBudget`（kernel）、service 压缩驱动、闭环长会话夹具均在。成立 |
| A03（压缩后工具对完整） | 闭环测试 `a03_mid_run_compaction_lands_on_the_real_binary` 在真二进制上断言配对完整+保留区逐字；本代理 Probe A/C 独立复证孤儿拒绝与级联无毒。成立 |
| A04（摘要失败不破坏会话） | 失败臂：原 exchange 保留、错误明确、空摘要不覆盖历史（service 测试 + 台账断言）；本代理精读坐实。成立 |
| 禁止替代 | 无占位实现、无跳过断言；质量门全绿（见范围 C）。但发现两处「误锚证据」（把预算估算函数当线体行为、把回退方向当现役同语义），构成防虚假完成层面的文档缺陷，计入 R2-F-01/R2-F-03 |

### 生产路径真实性

压缩驱动的生产接线（runs.rs 每 turn loop 顶评估 → plan → auxiliary.summarize → apply）已经代码精读+闭环测试+台账三路核实为真实生产路径，非测试专用旁路。`compact_now` 手动入口共用同一执行面。

---

## 二、范围 B：R1 findings（F-01..F-06）修复验证

| Finding | 验证方式（本代理独立执行） | 结论 |
|---|---|---|
| F-01 HIGH：保留区无孤儿保证缺失 | 精读 kernel `plan_compaction` 三条件统一扫描（组边界 ∧ ≤pending 硬上界 ∧ `suffix_min_owner[cut] ≥ cut`）；Probe A 中段 pending 形状（cut=2=pending 起点、pending@0→Ok(None)）；Probe C 夹层+级联（cut2=1、产物无孤儿、二次规划可证明） | **真实关闭** |
| F-02 HIGH：OpenAI×3/Google cache_read 双计 | Probe B 直接消费适配器层真实 `USAGE_MAPPINGS`（与 service `usage_inclusion_of` 同一解析式，不手构旗标）：openai/responses/codex/google `cache_read_in_input=true`、anthropic `false`；OpenAI Some(110500) 不双计、Anthropic Some(140500) 独立分量、半截 usage Some(10300) 不虚构扣除、饱和减法 Some(10000) 不下溢、全 None→None | **真实关闭**（比 R1 手构旗标更强的行为级证据） |
| F-03 MEDIUM：placeholder 工具单次恢复未迁移且误述 | 精读 service 修复臂与 adapter 渲染；闭环测试 placeholder 逐字配对断言核实 | **真实关闭** |
| F-04 LOW：手动压缩产物硬编码 mid_run=true | Probe C0/C0b 行为级复证 `apply_plan(..., mid_run)` 两臂（true/false 分叉正确） | **真实关闭** |
| F-05 LOW：报告「字节÷4」笔误 | 报告现行文本已修正 | **真实关闭** |
| F-06 LOW：工具回调失败消息误称「declared NO tools」 | `rust/crates/lingxi-adapters/src/models/auxiliary.rs` L240/L247 两臂分流消息坐实（declared tools … ONLY / declared NO tools …） | **真实关闭** |

F-01..F-06 全部真实关闭，无复活、无象征性修复。

---

## 三、范围 C：亲跑质量门证据

| 门 | 命令（均为 `/Users/study_superior/.cargo/bin/cargo`） | 结果 | 证据 |
|---|---|---|---|
| fmt | `cargo fmt --check`（workspace） | exit 0 | 压缩前亲跑 |
| clippy | `cargo clippy --workspace --locked --all-targets -- -D warnings` | exit 0，Finished 无警告 | `/tmp/r2_clippy.log` |
| kernel 目标 | `cargo test -p lingxi-kernel --test r06_t02_compaction --locked` | 36 passed / 0 failed | `/tmp/r2_t_kernel.log` |
| adapters 目标 | `cargo test -p lingxi-adapters --test r06_t02_compaction_render --locked` | 11 passed / 0 failed | `/tmp/r2_t_adapters.log` |
| service 目标 | `cargo test -p lingxi-service --test r06_t02_compaction --locked` | 17 passed / 0 failed | `/tmp/r2_t_service.log` |
| 闭环目标 | `cargo test -p lingxi-service --test r06_t02_closed_loop --locked` | 2 passed / 0 failed | `/tmp/r2_t_loop.log` |
| 全量 | `cargo test --workspace --locked` | **exit 0；122 个 target 全 ok；合计 1594 passed / 0 failed / 0 ignored**（grep+awk 从原始日志重算） | `/tmp/r2_workspace.log`（142KB 原始输出，WORKSPACE_EXIT=0） |

修复者声明 1585+9=1594 与本代理独立重算逐字一致。修复者的 `10_test_workspace.txt` 为注释拼贴（尾部有重复 0-passed 行），不作为证据采用；以本代理后台原始运行日志为准。

## 四、对抗探针（5 个全新构造，不重放 R1）

探针源码 `artifacts/rust-tauri/R06/T02-REVIEW-02/probe/src/main.rs`（独立 workspace，path 依赖三个 crate），运行输出 `probe_output.txt`，`cargo run` exit 0、**ALL PROBE GATES PASSED**：

- **Probe A（新形状）**：pending 组在历史**中段**（非尾部）——切点必须以其为硬上界（cut=2=pending 起点 PASS）；pending 在下标 0 时无可行切点 → Ok(None) PASS。R1 未覆盖该形状。
- **Probe B（新接法）**：CacheInclusion 经真实 `USAGE_MAPPINGS` 接线验证 F-02 族仲裁，含半截 usage 不虚构扣除与饱和减法不下溢两个新边界。全 PASS。
- **Probe C（新形状）**：夹层（owner 与 result 间插无调用 turn）+ 级联二次压缩（摘要领头产物再次规划）：cut2=1、切点落在组边界、产物无孤儿、mid_run 两臂。全 PASS。
- **Probe D（边界精度）**：window=100_000 时 79_999→Below（ratio_bp=7999）/ 80_000→Force（ratio_bp=8000，含边界）；window=3 reserve 饱和 Force；window=0→None；无 usage/零 usage→Unavailable。全 PASS。
- **Probe E（finding 证据，非门）**：构造跨界项为大 ToolResult 的形状，同一估算口径下并行模拟现役 findCutPoint 与 Rust plan_compaction——`crossing_item_index=3 (is ToolResult: true)`；**rust cut=2 retained=25289（≥ keep_recent 5000）；incumbent cut=4 retained=228（< 5000）**。方向分叉实证（R2-F-03）。

---

## 五、Findings（固定格式）

### R2-F-01

- **FINDING_ID**: R2-F-01
- **SEVERITY**: MEDIUM
- **REQUIREMENT_ID**: 任务书步骤①（迁移现役……语义）；防虚假完成条款（不得以误锚证据宣称对齐现役）
- **FILE_AND_LINE**:
  - Rust 线体：`rust/crates/lingxi-service/src/compaction.rs:549`（`max_output_tokens: Some(max_output_tokens)`，恒发；同值亦用于 L337/L423/L485 三条摘要请求路径）；公式 `rust/crates/lingxi-kernel/src/compaction.rs:248` `summary_output_cap` = `max(512, floor(0.8×reserve))`
  - 现役生产链：`core/session-compactor.ts:2127`（生产路径 `outputPolicy: PROVIDER_DEFAULT`）→ `:1868-1875`（`delete options.maxTokens`，仅 BOUNDED 才回填公式值）→ `lib/extensions/compaction-guard-ext.ts:474-480`（`normalizeCompactionProviderPayload(..., { outputPolicy: PROVIDER_DEFAULT, ... })`）→ `core/session-compactor.ts:336-342`（**optional-cap 族**：`!capability.required` 时删除全部输出上限字段，线体无任何 cap）与 `:344-352`（**required-cap 族**（Anthropic）：缺 cap 时回填 `safeRequiredOutputCap(model, boundedMaxTokens)` = L296-303 `min(model.maxTokens||model.maxOutput, model.contextWindow)`，0.8×reserve 公式仅作模型两值皆缺时的兜底）
  - 误锚证据：`docs/rust-tauri/R06/R06-T02_REPORT.md` §十#5（L167「现役公式 max(512, floor(0.8×reserve)) 不看模型 maxTokens，Rust 对齐现役」——把**预算估算函数** `getCachePreservingCompactionMaxTokens`（session-compactor.ts:377）锚为现役**线体行为**）；闭环测试 `rust/crates/lingxi-service/tests/r06_t02_closed_loop.rs:745` 把 `max_tokens=13107` 线上值当作现役一致证据
- **OBSERVED**: Rust 对**所有族**的摘要请求恒发 `max_output_tokens = max(512, floor(0.8×reserve))`（闭环实测 openai-completions 线上 `max_tokens=13107`）。现役 PROVIDER_DEFAULT 生产链：OpenAI 等 optional-cap 族（`core/provider-compat/output-budget.ts` default-optional required:false）**删除全部输出上限字段**，线上无 cap；Anthropic required-cap 族回填 `min(model.maxTokens, contextWindow)`，与 0.8×reserve 公式只在模型元数据全缺时才相等。
- **EXPECTED**: 迁移任务①要求线体语义对齐现役；若有意偏离，须在报告 §十 差异清单声明并给出理由。报告 §十#5 目前宣称「Rust 对齐现役」，与现役线体行为不符。
- **REPRODUCTION**: 读 `core/session-compactor.ts:1868-1875` + `:336-352` + `lib/extensions/compaction-guard-ext.ts:474` 得现役行为；读 `rust/crates/lingxi-service/src/compaction.rs:549` 得 Rust 行为；闭环测试 L745 断言值 13107 = 0.8×16384 即公式值，证明 Rust 线上恒发。
- **ROOT_CAUSE**: 把现役只用于**预算估算/BOUNDED 策略/兜底**的 `getCachePreservingCompactionMaxTokens` 误认为现役线体默认值，未沿 PROVIDER_DEFAULT 生产链追到有 erase 行为的 `normalizeCompactionProviderPayload`。
- **SAME_ROOT_CAUSE_PATHS**: kernel `summary_output_cap` 注释（compaction.rs:245-247）锚定「现役公式」本身无误（公式逐字一致），但其调用方把它当线体默认值；报告 §五 同样以该公式描述线体行为；闭环 L745 的断言把分叉固化为「一致证据」。三处同一误锚。
- **IMPACT**: OpenAI 族摘要请求线上从无上限变为恒有 0.8×reserve 上限——长摘要场景可能被截断（stopReason≠stop 走失败臂，行为变化但可诊断）；Anthropic 族上限口径从 `min(model.maxTokens, contextWindow)` 变为 0.8×reserve，数值可大可小。均为真实线体语义分叉。
- **REQUIRED_FIX**: 二选一——(a) 对齐现役：optional-cap 族不发输出上限字段，required-cap 族发 `min(model.maxTokens, contextWindow)`（缺省时回退公式值）；或 (b) 保留现行为并在报告 §十 声明该分叉及理由，同时修正 §十#5「对齐现役」的误锚表述与闭环测试的证据措辞（断言可保留，但不得表述为与现役一致）。
- **REGRESSION_TESTS**: 若选 (a)：闭环测试按族断言——openai 族线上无 `max_tokens`、anthropic 族线上 = `min(model.maxTokens, contextWindow)`；kernel 层保留 `summary_output_cap` 公式测试（预算估算用途）。若选 (b)：补一条测试固定「恒发公式值」为有意行为。

### R2-F-02

- **FINDING_ID**: R2-F-02
- **SEVERITY**: MEDIUM
- **REQUIREMENT_ID**: 任务书步骤①（迁移现役……**摘要上下文**……语义）
- **FILE_AND_LINE**: 现役摘要后处理管段 `core/session-compactor.ts:1897-1912`：`appendFileOperationContext`（`<read-files>`/`<modified-files>` 段）、`appendSkillRecallContext`、`appendPlanFileContext`、`appendContextNotesContext`、`appendHistoryRecoveryContext` 全部追加进最终摘要。Rust 侧 grep 零命中（无对应段族），报告 §十 六条差异（L162-169）无一提及。
- **OBSERVED**: 现役摘要产出必经 enrichment 段族追加（其中 `appendFileOperationContext` 为纯函数、只依赖本次压缩的 details，无外部子系统依赖）；Rust `apply_plan` 直接使用模型摘要文本，无任何段追加；报告未声明该缺口。
- **EXPECTED**: 「摘要上下文」语义整体迁移；依赖未迁移子系统的段（skill-recall→R06-T06、plan-file→plan 模式、context-notes→context_notes 工具、history-recovery→落盘历史 R06-T04）至少须在 §十 声明「未迁移+归因后续任务」；无依赖的 fileOps 段应迁移。
- **REPRODUCTION**: `grep -rn "read-files\|modified-files\|skillRecall\|planFile\|contextNotes\|historyRecovery" rust/crates/lingxi-kernel rust/crates/lingxi-service` 零命中；对照 session-compactor.ts:1897-1912 现役追加链。
- **ROOT_CAUSE**: 摘要管线勘察止于 validate/repair，未覆盖现役 summary post-processing 的 enrichment 族。
- **SAME_ROOT_CAUSE_PATHS**: 同一勘察缺口的其余两条未迁移段见 informational I-1/I-2（transform-proof 相关的 hard-truncate 出口；ASK 事件线——后者 kernel 注释 L47-51 有码内声明但未入 §十）。
- **IMPACT**: 压缩后摘要不带文件操作/技能/计划/笔记段，长会话续跑的上下文信息量低于现役；属语义保真缺口而非功能故障，故 MEDIUM。
- **REQUIRED_FIX**: 迁移 `appendFileOperationContext`（无依赖部分）；其余段在报告 §十 显式声明「未迁移 + 依赖任务编号」。
- **REGRESSION_TESTS**: fileOps 段追加的 kernel/service 测试（有 details 时摘要含 `<read-files>`/`<modified-files>`，无 details 时逐字不追加）；§十 声明的文档核对。

### R2-F-03

- **FINDING_ID**: R2-F-03
- **SEVERITY**: LOW
- **REQUIREMENT_ID**: 任务书步骤①；防虚假完成（注释/报告误锚）
- **FILE_AND_LINE**: Rust 回退扫描 `rust/crates/lingxi-kernel/src/compaction.rs:560-563`（`(1..=crossing).rev()` 取 ≤crossing 的最近合法组边界）+ 误锚注释 `:552-553`（「现役 findCutPoint 语义：跨界项计入保留区，切点**回退**到跨界项所在组的起点」）；现役前跳 `node_modules/@earendil-works/pi-agent-core/dist/harness/compaction/compaction.js:253-294`（从尾累积到 ≥keepRecent 后，L266-273 取**第一个 ≥ i 的合法切点**；合法切点=除 toolResult 外所有消息角色）；报告误锚 `docs/rust-tauri/R06/R06-T02_REPORT.md` L106（差异表把 Rust 扫描锚为 `findCutPoint` 同义）与 L191（风险#1「保留区按组回退可超 keep_recent（现役 findCutPoint 同语义）」）。
- **OBSERVED**: Probe E 实证（同一估算口径并行模拟）：跨界项为大 ToolResult 时，Rust cut=2 retained=**25289**（≥ keep_recent 5000，回退组起点）；现役算法 cut=4 retained=**228**（< 5000，前跳越过跨界 toolResult）。两侧保留量差两个数量级。
- **EXPECTED**: 注释与报告不得把方向相反的算法锚为「同语义」；若有意为之（Rust 的组边界+无孤儿约束本身要求回退），应在 §十 声明方向分叉。
- **REPRODUCTION**: `cd artifacts/rust-tauri/R06/T02-REVIEW-02/probe && /Users/study_superior/.cargo/bin/cargo run --locked` → Probe E 段输出（`probe_output.txt`）；对照 compaction.js L266-273 前跳循环。
- **ROOT_CAUSE**: 读现役 findCutPoint 时把「合法切点集合不含 toolResult」误推为「跨界项计入保留区」，实际现役是前跳（crossing 项被留在旧区），Rust 是回退（crossing 项计入保留区）。
- **SAME_ROOT_CAUSE_PATHS**: kernel 注释 L552-553、报告 L106、报告 L191 三处误锚同根。
- **IMPACT**: 保留区大小语义不同（Rust 恒 ≥keep_recent，现役可 <keepRecent）；功能安全（无孤儿）反而更强，故 LOW；但「同语义」表述会误导后续审查与维护。
- **REQUIRED_FIX**: 修正 kernel 注释与报告 L106/L191 的锚定，在 §十 增列「keep_recent snap 方向：Rust 回退（保留区恒 ≥keep_recent）vs 现役前跳（可 <keepRecent）」差异条。
- **REGRESSION_TESTS**: 现有 planner 切点测试已固定 Rust 方向；补一条注释级核对即可，无需新行为测试。

### R2-F-04

- **FINDING_ID**: R2-F-04
- **SEVERITY**: LOW
- **REQUIREMENT_ID**: 任务书步骤①（错误恢复语义）
- **FILE_AND_LINE**: Rust `rust/crates/lingxi-service/src/compaction.rs:470`（`build_repair_instruction(&issues, &sanitized.text)`）与 `:474-481`（推入 prior 的草稿 turn 同为 sanitized.text）；现役 `lib/llm/cache-preserving-compaction-agent-run.ts:601`（`createRepairInstruction(validation.issues, rawText)`，未清洗原文；repair 会话中的草稿消息亦为原始 assistant message）。
- **OBSERVED**: 修复轮的 `<draft-summary>` 载荷与 prior 草稿 turn 在 Rust 用清洗后文本，现役用原始文本。两侧 validate 的输入一致（现役 L596 `validateSummary(sanitized.text, …)`，Rust L459/L495 同），分叉只在 repair 载荷。
- **EXPECTED**: 错误恢复语义对齐现役或声明分叉。
- **REPRODUCTION**: 对照上述行号。
- **ROOT_CAUSE**: sanitize 结果复用过度——repair 臂直接拿了手边的 sanitized.text，未保留 rawText 到 repair 载荷。
- **SAME_ROOT_CAUSE_PATHS**: service compaction.rs L470 与 L477 两处同一来源；repair 重试臂 L494-500 同样只流转 sanitized。
- **IMPACT**: 仅当 sanitizer 实际剥除内容（闭合叙述块/未配对标记/空行塌缩）时，模型在修复轮看到的草稿与其真实输出不同；常见情形 rawText==sanitized.text 无差异。LOW。
- **REQUIRED_FIX**: repair 载荷改用当轮原始文本（draft/text），sanitize 只服务 validate 与最终落库；或在 §十 声明。
- **REGRESSION_TESTS**: 构造带可剥除内容的失败草稿，断言修复指令内嵌原文。

### R2-F-05

- **FINDING_ID**: R2-F-05
- **SEVERITY**: LOW
- **REQUIREMENT_ID**: 任务书步骤①（摘要上下文/指令语义）
- **FILE_AND_LINE**: 现役 `core/session-compactor.ts:410-414`（`isSplitTurn` 时追加 scope 行 "This is a split-turn compaction: preserve the original request and early progress needed to understand the retained suffix."；`:192-280` `deriveCachePreservingCompactionBoundary` 证实现役旧区=previousSummary+messagesToSummarize+turnPrefixMessages）。Rust 指令模板无该行（grep 零命中），报告 §十 未声明。
- **OBSERVED**: Rust 切点恒为组边界（AssistantTurn‖CompactionSummary），按现役 turn 框架（user 开场）这类切点常态构成现役语义下的 split-turn（发起该 turn 的 user 消息留在旧区），但摘要指令从不含 split-turn scope 行。
- **EXPECTED**: 迁移该行（当切点为 AssistantTurn 即现役语义的 mid-turn 时）或在 §十 声明不迁移及理由。
- **REPRODUCTION**: `grep -rn "split.turn\|split_turn" rust/crates/lingxi-kernel/src/compaction.rs rust/crates/lingxi-service/src/compaction.rs` 零命中；对照 session-compactor.ts:410-414。
- **ROOT_CAUSE**: 指令模板按文本面逐字迁移，未迁移基于切点形状的 scope 分支。
- **SAME_ROOT_CAUSE_PATHS**: 无第二处（指令其余分支已逐字，含 boundary placeholder 与 recent-tail 提示）。
- **IMPACT**: 摘要模型缺少「保留原始请求与早期进展以理解保留尾部」的指引，split-turn 形状下摘要可能丢失续跑所需的请求上下文。LOW（摘要主格式与边界提示仍在）。
- **REQUIRED_FIX**: 当切点落在 AssistantTurn（即现役 isSplitTurn 等价形状）时向 scope 行追加该句；或 §十 声明。
- **REGRESSION_TESTS**: 指令构造测试：切点为 AssistantTurn 时含 split-turn 行、切点为 CompactionSummary 时不含。

### Informational（不阻断，记录备查）

- **I-1 ASK 线事件未迁移**：kernel `COMPACTION_ASK_RATIO_BP=5_000`（compaction.rs:51）为死常量，码内注释 L47-51 已声明「R06-T02 不实现（现役只发事件、不阻塞）」，但报告 §十 未列。码内声明可接受，建议补入 §十 保持单一事实源。
- **I-2 hard-truncate 出口未迁移**：现役摘要失败兜底有硬截断路径；任务书④禁止截断冒充成功，Rust 不实现该出口是**有意且更优**，但 §十 未声明。建议声明。

---

## 六、同根因横向扫描汇总

| 根因 | 命中面 |
|---|---|
| 预算估算函数误锚为线体行为（R2-F-01） | service compaction.rs:337/423/485/549（四处同一 Some(...)）；kernel 注释 245-247；报告 §十#5 + §五；闭环测试 L745 |
| 摘要后处理勘察缺口（R2-F-02） | enrichment 五段全缺；I-1（ASK）、I-2（hard-truncate）同族未入 §十 |
| findCutPoint 方向误读（R2-F-03） | kernel 注释 552-553；报告 L106；报告 L191 |
| sanitize 结果复用过度（R2-F-04） | service compaction.rs:470、477、494-500 |
| 指令 scope 分支遗漏（R2-F-05） | 仅指令模板一处 |

已横向确认无其它同根因漏网面（保留区/触发/账目/台账/渲染面均经 Probe A-E 与四个测试目标覆盖）。

---

## 七、裁决

**VERDICT: FAIL**

裁决理由（按任务书规则「全部 REQUIRED 验收成立、上轮 findings 全部真实关闭且无新阻断项 → PASS，否则 FAIL」）：

- 范围 B：F-01..F-06 **全部真实关闭**（代码精读 + Probe A/B/C 行为级独立复证，非仅核对修复者声明）；
- 范围 C：fmt/clippy/四个测试目标/全量 1594 passed/0 failed（exit 0）亲跑全绿，候选摘要逐字一致；
- 范围 A：触发、预算、配对保留、失败保护、A03/A04 主链全部成立；
- **但存在两个 MEDIUM 级未声明保真分叉且伴随误锚证据**：R2-F-01（摘要请求 max_tokens 线体行为与现役 PROVIDER_DEFAULT 生产链分叉，报告 §十#5 与闭环测试把预算公式误锚为现役线体一致）与 R2-F-02（摘要 enrichment 段族整体未迁移且未声明，其中 fileOps 段无外部依赖借口）。语义保真与防虚假完成是本任务的核心验收性质，两者构成新阻断项；
- 另有 LOW×3（R2-F-03 方向反转误锚、R2-F-04 repair 草稿文本分叉、R2-F-05 split-turn scope 行缺失）与 informational×2，单独均不阻断，随修复一并处理。

**最小修复清单（通向 PASS）**：
1. R2-F-01：按 REQUIRED_FIX (a) 或 (b) 二选一落地，消除「对齐现役」误锚；
2. R2-F-02：迁移 fileOps enrichment 段；其余段入 §十 声明（归因 R06-T04/T06 等）；
3. R2-F-03：修正 kernel 注释与报告 L106/L191 锚定，§十 增列方向差异条；
4. R2-F-04：repair 载荷改用当轮原始文本（或 §十 声明）；
5. R2-F-05：补 split-turn scope 行（或 §十 声明）；
6. I-1/I-2 补入 §十。
