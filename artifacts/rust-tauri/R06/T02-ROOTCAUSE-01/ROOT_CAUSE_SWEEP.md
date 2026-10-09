# R06-T02 根因扫描报告（ROOT-CAUSE-SWEEP-R06-T02-R2）

- **SWEEP_ID**: ROOT-CAUSE-SWEEP-R06-T02-R2（一次性根因扫描；两轮独立验收 FAIL 后启动）
- **TASK_ID**: R06-T02（token 预算、压缩与长会话）
- **HEAD**: `b174e3ff44c61000abbb84595cf69dfad85dd531`（分支 `codex/rust-tauri-migration`）
- **输入**: R1 审查 `artifacts/rust-tauri/R06/T02-REVIEW-01/REVIEW.md`（F-01..F-06）；R2 审查 `artifacts/rust-tauri/R06/T02-REVIEW-02/REVIEW.md`（R2-F-01..F-05 + I-1/I-2）；执行者报告 `docs/rust-tauri/R06/R06-T02_REPORT.md`；现役 TS 全链与 Rust 三 crate 全链（均亲读，行号见各条目）
- **写盘纪律**: 本代理不写产品代码，只写本目录

---

## 一、根因（含证据）

两轮共 11 个 findings + 2 个 informational，归并为三个根因簇。核心假设「现役语义提取不完整 + 差异台账纪律失效」**被证据证实**，并补出第三簇「测试方法论缺陷」。

### RC-1 勘察单元错位：按「文件/函数」而非「生产链端到端」勘察现役语义

执行者读了 `cache-preserving-compaction-agent-run.ts` 全文与 `session-compactor.ts` 的指令/清洗段，但五条链各自断在不同位置：

| 断点 | 证据 | 后果 |
|---|---|---|
| 读了预算估算函数 `getCachePreservingCompactionMaxTokens`（session-compactor.ts:377）就当作线体行为，未沿 PROVIDER_DEFAULT 生产链追到 `compaction-guard-ext.ts:474-480` 的 onPayload normalize → `session-compactor.ts:1867-1875`（`delete options.maxTokens`、仅 BOUNDED 回填）→ `:313-352`（optional 族删全部 cap 字段 / required 族回填 `min(model.maxTokens‖maxOutput, contextWindow)`） | R2-F-01 FILE_AND_LINE 四处行号互证 | 线体 max_tokens 族分流整条缺失 |
| 摘要管线勘察止于 validate/repair（agent-run.ts:523-601），未读 `session-compactor.ts:1897-1912` 的摘要后处理 enrichment 追加链（fileOps/skill-recall/plan-file/context-notes/history-recovery 五段） | R2-F-02 grep 零命中对照现役五段 | 段族整体未迁移且未声明 |
| 读 `findCutPoint` 时把「合法切点集合不含 toolResult」误推为「跨界项计入保留区」，未逐行走查 `compaction.js:253-294` 循环本体（L266-273 是**前跳**：取第一个 ≥i 的合法切点） | R2-F-03 + Probe E 实证（同形状 rust cut=2 retained=25289 vs incumbent cut=4 retained=228） | snap 方向反转 + 注释/报告双重误锚 |
| `agent-run.ts:596`（validate 用 sanitized.text）与 `:601`（repair 用 rawText）相差 5 行，勘察时合并为「sanitized 一路到底」 | R2-F-04 | repair 载荷分叉 |
| 指令模板按文本面逐字迁移，未迁移基于切点形状的条件分支（`session-compactor.ts:410-414` 的 isSplitTurn scope 行） | R2-F-05 grep 零命中 | split-turn scope 行缺失 |

**共同模式**：现役语义分布在「主文件 + 扩展钩子 + SDK 内部 + 后处理链」四个层面，按文件勘察必然漏层。正确的勘察单元是「从触发到线上字节的端到端生产链」。

### RC-2 差异台账纪律失效：未核对语义被误锚为「与现役一致」

T01 建立了 18 条冻结差异的先例（每条 = 差异 + 理由 + 去向）。T02 的 §十 只有 6 条，且其中 #5 本身就是误锚（把预算公式锚为线体行为并宣称「Rust 对齐现役」）。误锚的三处落点：

1. 报告 §十#5（max_tokens「对齐现役」）——R2-F-01
2. 报告 §五 L106 + §十三风险#1 L191（findCutPoint「同语义」）——R2-F-03
3. kernel 注释 `compaction.rs:552-553`（「现役 findCutPoint 语义：……回退」）——与代码方向相反的锚定写进了源码注释

误锚一旦写进报告就反向固化进测试：闭环测试 `r06_t02_closed_loop.rs:745` 把 `max_tokens=13107`（=0.8×16384，Rust 自己的公式输出）当作与现役一致的证据断言。**台账不只是文档，它是测试断言的事实源；台账失信则测试失信。**

### RC-3 测试是「Rust 自洽测试」而非「现役对照测试」

57+9 个测试断言的全部是 Rust 实现的内部一致性（公式输出、形状配对、渲染键存在性），没有任何一条以现役线体/现役语义清单为对照基准。三类盲区直接对应三个 finding 族（详见 §五）。测试全绿与「对齐现役」之间没有因果链——这正是两轮验收都能在全绿下 FAIL 的方法论原因。

---

## 二、涉及架构边界

```
现役 TS：
  pi-agent-core compaction.js ── session-compactor.ts ── compaction-guard-ext.ts
       （切点/原生路径）            （触发/指令/后处理/线体整备）   （L1截断/L2 reserve/L3钩子）
  cache-preserving-compaction-agent-run.ts（摘要执行+清洗+repair+placeholder恢复）
  core/provider-compat/output-budget.ts（required-cap 族清单）

Rust：
  lingxi-kernel/src/compaction.rs      —— 语义核心：触发/reserve/切点/指令/清洗/账目
  lingxi-service/src/compaction.rs     —— 驱动：auxiliary 路由、placeholder 恢复臂、repair 臂、台账
  lingxi-service/src/runs.rs           —— loop 顶触发评估、usage 捕获、tail 复位
  lingxi-adapters/src/models/auxiliary.rs        —— complete_step（工具意图上报）
  lingxi-adapters/src/models/{openai_completions,openai_responses,google_generative_ai,anthropic_messages}.rs
                                          —— 各族线体渲染（max_tokens 键存在性在此最终决定）
  lingxi-adapters/src/models/config.rs —— RouteCompatHints（max_tokens/context_window；无 outputCapRequired 面）
```

跨边界责任要点：max_tokens 键的最终决定在**渲染器**（openai_completions.rs:597 None→线上无键；anthropic_messages.rs:177 `unwrap_or(16384)` 族缺省），但值的决定在 **service**（compaction.rs:549 恒 Some）——修复 R2-F-01 必须动 service 的值决策层，渲染器已具备「None 不发键」能力（anthropic 族因协议必需除外）。

---

## 三、所有已知症状（R1/R2 归并映射）

| 症状 | 裁决轮 | 级别 | 根因簇 | 现状 |
|---|---|---|---|---|
| F-01 保留区无孤儿保证缺失 | R1 | HIGH | 实现缺陷（已修：kernel 三条件统一扫描 + suffix_min_owner） | R2 确认真实关闭（Probe A/C 复证） |
| F-02 cache_read 双计 | R1 | HIGH | 实现缺陷（已修：CacheInclusion 按 USAGE_MAPPINGS 仲裁） | R2 确认真实关闭（Probe B 复证） |
| F-03 placeholder 单次恢复未迁移 | R1 | MEDIUM | RC-1（agent-run.ts 恢复臂漏读） | R2 确认真实关闭 |
| F-04 手动压缩硬编码 mid_run=true | R1 | LOW | 实现缺陷（已修：apply_plan 参数化） | R2 确认真实关闭（Probe C0/C0b） |
| F-05 报告「字节÷4」笔误 | R1 | LOW | RC-2（文档精度） | R2 确认关闭 |
| F-06 工具回调失败消息误称 | R1 | LOW | 实现缺陷（已修：auxiliary.rs:238-252 两臂分流） | R2 确认关闭 |
| R2-F-01 max_tokens 线体族分流缺失 + §十#5 误锚 | R2 | MEDIUM | RC-1 + RC-2 | **开放** |
| R2-F-02 enrichment 段族未迁移未声明 | R2 | MEDIUM | RC-1 | **开放** |
| R2-F-03 keep_recent snap 方向反转 + 注释/报告误锚 | R2 | LOW | RC-1 + RC-2 | **开放** |
| R2-F-04 repair 载荷用 sanitized 而非 raw | R2 | LOW | RC-1 | **开放** |
| R2-F-05 split-turn scope 行缺失 | R2 | LOW | RC-1 | **开放** |
| I-1 ASK 事件未迁移（码内已声明，§十 未列） | R2 | info | RC-2 | **开放** |
| I-2 hard-truncate 出口未迁移（§十 未声明） | R2 | info | RC-1/RC-2；本扫描升级为决策项（见 N-1） | **开放** |

归并结论：R1 的 6 项是**实现层缺陷**（修法明确、已真实关闭）；R2 的 5+2 项是**勘察/台账层缺陷**（RC-1/RC-2 主导），且每一项都伴随「测试全绿但语义分叉」——RC-3 是它们穿过质量门的共同通道。

---

## 四、可能遗漏的同族生产路径（含新发现）

对每类缺陷做了 Rust 侧同族扫描；以下为**超出 R1/R2 点名范围**的新发现，按「与现役同族」归类。

### N-1 压缩请求自身超窗的兜底链缺失（I-2 升级为决策项）
- 现役：`session-compactor.ts:2064-2102` fit 检查 → 硬截断 → native-fallback error 臂，三层显式降级。
- Rust：摘要请求若自身超窗 → auxiliary Failed → 原 exchange 保留 → runs.rs 下一 turn 顶重评估再次触发。**本次取证未见失败熔断**（runs.rs:1274-1330 每 turn 顶无计数/锁存），同一 run 内会反复重试直至 chat 调用自身超窗失败结束 run。
- 同族面：compact_now 手动入口共用同一执行面，同样无兜底。
- 判定：任务书④「失败保留原上下文并返回可诊断策略」成立，但「可诊断策略」不含循环防护；现役有显式降级链而 Rust 连声明都没有。**必须决策**（实现诚实截断或声明+熔断），不能只入 §十 了事。

### N-2 stopReason∈{error,aborted} 触发门未迁移
- 现役 `session-compaction-runtime.ts`：stopReason=error/aborted 不触发压缩。
- Rust：runs.rs 触发评估只看 usage 是否 Known（L1764-1767 仅从 ReportedUsage::Known 捕获），失败调用的 Known usage 同样会触发压缩。
- 同族面：取消臂（aborted）在 Rust 由 run 取消面处理，压缩评估未见取消门。
- 影响：失败 turn 后多一次无谓的摘要调用（成本+时延），方向安全但语义分叉未声明。

### N-3 settings.enabled 用户开关缺失
- 现役触发门第一项：`settings.enabled` 用户级开关。
- Rust：无对应面（压缩恒启用，仅 contextWindow>0 等结构性门）。
- 判定：Rust 服务配置面无现役 settings 系统，属「配置面未接」而非「语义丢弃」，应入 §十 声明。

### N-4 估算器口径分叉（现役自身两子系统口径不一致）
- 现役切点规划与 mid-run 尾部估算用 pi-sdk 纯 chars/4（compaction.js:166-204，无 CJK 加权）；hana 自家 `lib/llm/estimate-text-tokens.ts` 是 CJK×1.1 加权。
- Rust `estimate_text_tokens` 与 hana 估算器同源（CJK×1.1）。
- 后果：CJK 密集会话下 Rust 的切点/尾部估算比现役切点链路**高估约 2.75 倍**（CJK 字符 1.1/0.25），方向保守（更早触发、保留区按现役口径实量更小），不产安全事故但保真分叉未声明。
- 判定：现役自身不一致（切点链=pi-sdk 口径，其余=hana 口径），Rust 对齐了 hana 一侧。入 §十 声明并注明方向。

### N-5 customInstructions 面未接
- 现役摘要指令支持用户 customInstructions 注入；Rust service 恒 None。入 §十 声明（随配置面任务）。

### N-6 现役 SDK post-run backstop 检查不适用
- 现役在 agent run 结束后由 SDK 侧再做一次压缩检查（持久会话层）。Rust 每 run 内存交换、无持久会话层（R06-T04），该检查无挂载点。不适用 + 归因 R06-T04。

### N-7 keepRecent/reserve 用户设置覆盖面未接
- 现役 settings 可覆盖 keepRecent/reserve；Rust 常量冻结（KEEP_RECENT=20000、MIN_RESERVE=16384）。与 N-3 同族，入 §十 声明。

### N-8 RouteCompatHints 无法表达 outputCapRequired
- `config.rs:247-273` 有 max_tokens/context_window，无 outputCapRequired 字段且 `deny_unknown_fields`；现役 `output-budget.ts` 的 required-cap 族清单（explicit-required/anthropic-native/bedrock-native/anthropic-messages）在 Rust 配置面无表达。
- 判定：这是 R2-F-01 选 (a) 的**前置缺口**——service 要按族分流就必须先有一个族判定面（扩 RouteCompatHints 或按协议族映射）。registered gap，修复时一并决策。

### N-9 现役 L1 会话级 tool_result 32KB head+tail 截断 guard 未迁移
- 现役 compaction-guard-ext.ts 的 L1 钩子对**所有** tool_result 做 32KB head+tail 截断（会话级通用兜底）。
- Rust：无会话级通用截断 guard；仅 exectools 有 per-tool max_output_bytes + `truncated` 诚实旗标（工具级有界，部分覆盖）。
- 判定：功能上「工具输出有界」由工具预算层部分兜底，但「任意工具结果统一 32KB 截断」的现役语义不存在。入 §十 声明（理由：Rust 工具输出已按工具预算有界且带诚实旗标；或评估是否补通用兜底）。

### N-10 压缩作用域：现役=整个会话 vs Rust=当前 run 的 exchange
- 现役摘要请求历史含多 user 消息（跨 submission 会话史）；Rust run 级 exchange 单 submission、assistant/toolResult only（R03/R05 结构继承）。
- 判定：非 T02 分叉（结构继承），但报告未说明压缩作用域差异，入 §十 注明 + 归因 R06-T04（持久会话）。

### 已确认「不适用」的同族项（防止后续审查再误报）
- 现役原生压缩路径 `compaction.js:380` 的 `maxTokens=min(floor(0.8×reserve), model.maxTokens)`——hana 生产链不走原生路径（走缓存保留 agent-run 链），**现役已死代码，不适用**。0.8×reserve 公式在现役只服务预算估算/BOUNDED 策略/兜底三处，均非线体默认。
- `projectMessagesToLatestCompactionUsageEpoch`（session-compactor.ts:104-136）——现役为「消息数组跨压缩纪元混排」做的 usage 归属投影；Rust last_turn_usage 每次 settle 重赋、压缩后 tail_from 复位，无跨纪元回扫窗口，**结构性不适用**。
- reasoning 级别参数——现役压缩链同样不存在（R05 面），非 T02 分叉。

---

## 五、已有测试盲区（为什么 57+9 个测试没捕获 R2 findings）

| 盲区 | 对应 finding | 机制 |
|---|---|---|
| 断言值取自 Rust 自身输出，无现役黄金对照 | R2-F-01 | 闭环 L745 断言 `max_tokens=13107`——13107 是 Rust 公式算出来的，断言通过只证明「Rust 算得对 Rust 的公式」，证明不了「与现役一致」 |
| presence-only 断言查不到「条件分支行缺失」 | R2-F-05 | kernel 指令测试断言已逐字迁移的行存在；split-turn 行在恒缺席的分支下永远不缺断言对象，全绿 |
| 无按族参数化的线体键集合断言 | R2-F-01 | openai/anthropic/google 渲染测试各自断言「键存在且值对」，没有「同一输入下五族键集合对照现役」的矩阵测试 |
| 无「语义清单驱动」扫描测试 | R2-F-02 | enrichment 五段、触发门子项、线体族分流从未被列成「每条必须已迁移或已声明」的机器可查清单，漏迁无红色信号 |
| 文档锚定无交叉校验 | R2-F-01/F-03 | §十 条目与代码/测试之间无机器核对，误锚可以静默存活 |

### 防再发测试设计（随修复一并落地）
1. **现役黄金线体 fixture 对照**：为五族各锚定一份「现役压缩请求出站形状」fixture（键集合 + 关键值来源行号注释）；Rust 渲染测试逐键对照，族分叉即红。覆盖 R2-F-01 族。
2. **语义清单参数化测试**：把报告 §五语义锚定表转为测试输入清单（每条 = 语义项 + 现役锚点行号 + 期望状态[已迁移/已声明+§十条目号]），测试逐条断言代码面存在 + §十 状态一致，缺一即红。覆盖 R2-F-02/F-05 族与全部「未声明」类。
3. **指令模板条件分支测试**：split_turn=true/false 两臂各一条断言。覆盖 R2-F-05。
4. **报告-测试交叉锁**：闭环测试断言数值处必须注释引用 §十 条目号；加一个 grep 门禁脚本（CI 或 pre-commit）核对「§十 条目数 = 测试引用数」，防止误锚再固化。
5. **族矩阵三维参数化**：USAGE_MAPPINGS（cache 口径）× 输出政策（required/optional）× 渲染键集合，openai×3/google/anthropic-messages/anthropic-native/bedrock 全覆盖。

---

## 六、完整修复集合

优先级：**P0=必须现在修（通向 PASS 的阻断项）**；**P1=可入 §十 声明差异（随修复一并落文档）**；**P2=后续阶段（归因其他任务，本次只登记）**。

| 编号 | 关联 | 优先级 | 根因层 | 涉及文件 | 修复方式 | 回归测试 | 验证方法 |
|---|---|---|---|---|---|---|---|
| FIX-01 | R2-F-01 + N-8 | P0 | RC-1+RC-2 | `lingxi-service/src/compaction.rs:337/423/485/549`；`lingxi-adapters/src/models/config.rs`（族判定面）；kernel `compaction.rs:248` 保留公式为预算用途 | 选 (a) 按族分流：service 计算 `Option<u32>`——optional-cap 族（openai×3/google/deepseek）→ None（渲染器已支持不发键）；required-cap 族（anthropic-messages/anthropic-native/bedrock）→ `min(compat.max_tokens, context_window)`，双缺回退 `max(512, floor(0.8×reserve))`；族判定面二选一（扩 RouteCompatHints 增 output_cap_required 字段 / 按协议族映射函数），并在 §十 声明现役 explicit-required 在 Rust 配置面无对应；同时修正 §十#5 误锚表述 | 闭环按族断言：openai 线上无 max_tokens 键、anthropic 线上 = min(model_max, ctx)；kernel 公式测试保留（预算用途）；族矩阵测试（§五-5） | 闭环测试抓五族线体键集合；报告 §十#5 重读核对 |
| FIX-02 | R2-F-02 | P0 | RC-1 | kernel `compaction.rs`（增 extract_file_ops + apply_plan 追加段）；service 台账记录追加事实 | 迁移 fileOps 段：扫 summarized 区 AssistantTurn.tool_calls 中 target∈{read,write,edit} 的 arguments.path，与上一摘要的 `<read-files>`/`<modified-files>` 段解析结果合并续传（对齐现役 computeFileLists：modified=edited∪written、readOnly=read−modified、排序），无 details 时逐字不追加；其余四段入 §十 归因：skill-recall→R06-T06、plan-file→plan 模式、context-notes→context_notes 工具、history-recovery→R06-T04 | kernel：有 details 时摘要含两段且清单正确、无 details 时不追加；跨轮续传（上一轮摘要段→本轮合并）测试 | 语义清单测试（§五-2）含五条段族条目 |
| FIX-03 | R2-F-03 | P0（文档） | RC-2 | kernel `compaction.rs:552-553` 注释；报告 §五 L106、§十三风险#1 L191 | 三处锚定改为「Rust 回退（保留区恒 ≥keep_recent，无孤儿更强）vs 现役前跳（retained 可 <keepRecent）」；§十 增列方向差异条 | 无需新行为测试（现有 planner 测试已固定 Rust 方向）；语义清单测试含此条目 | Probe E 形状保留为回归（已在审查侧 probe 落证） |
| FIX-04 | R2-F-04 | P0 | RC-1 | `lingxi-service/src/compaction.rs:470/474-481/494-500` | repair 载荷与 prior 草稿 turn 改用当轮 raw text；sanitize 只服务 validate 与最终落库 | 构造带可剥除内容（未配对标记/闭合叙述块）的失败草稿，断言修复指令 `<draft-summary>` 内嵌原文逐字 | 测试红→绿 |
| FIX-05 | R2-F-05 | P0 | RC-1 | kernel `compaction.rs`（SummaryInstructionSpec 增 split_turn；build_summary_instruction 补 scope 行）；service 调用处按切点项类型传参（切点=AssistantTurn→true，CompactionSummary→false） | 迁移 session-compactor.ts:410-414 条件行，逐字 | 指令构造两臂断言（§五-3） | 测试红→绿 |
| FIX-06 | I-1/I-2 | P0（文档） | RC-2 | 报告 §十 | ASK 事件（kernel 死常量+码内声明已有）与 hard-truncate 出口（有意不实现，更优）补入 §十，单一事实源 | 语义清单测试含两条 | 文档核对 |
| FIX-07 | N-1 | P0（决策） | RC-1 | runs.rs 触发评估 + service 失败臂 | 决策项二选一——(a) 实现诚实硬截断兜底（fit 检查→截断→摘要/台账显式标注降级，不冒充全量摘要）；(b) 声明不实现 + 加失败熔断（同一 exchange 连续 N 次压缩失败则本 run 停止重试并显式报错入台账）。现役 native-fallback error 臂在 Rust 无挂载面，随决策一并声明 | (a)：超窗摘要请求→截断后成功且产物带降级标注；(b)：连续失败 N 次后不再发摘要请求且台账有熔断记录 | 构造摘要请求自身超窗的闭环夹具 |
| FIX-08 | N-2 | P1（建议直接实现，成本极低） | RC-1 | runs.rs 触发评估处 | 补 stopReason 门：settle 为 error/aborted 的 turn 不触发压缩评估；若选择声明须给理由 | 失败 turn 后不触发压缩的 service 测试 | 测试红→绿 |
| FIX-09 | N-3/N-7 | P1 | RC-2 | 报告 §十 | 声明：settings.enabled 与 keepRecent/reserve 覆盖面未接（Rust 无现役 settings 系统，常量冻结），归配置面任务 | 语义清单测试含条目 | 文档核对 |
| FIX-10 | N-4 | P1 | RC-2 | 报告 §十 | 声明估算器口径：Rust 对齐 hana CJK×1.1，现役切点/尾部链为 pi-sdk 纯 chars/4（现役自身两子系统不一致），方向保守（更早触发、保留区按现役口径实量更小） | 无需行为测试；清单条目 | 文档核对 |
| FIX-11 | N-5 | P1 | RC-2 | 报告 §十 | 声明 customInstructions 面未接（service 恒 None），归配置面任务 | 清单条目 | 文档核对 |
| FIX-12 | N-9 | P1 | RC-2 | 报告 §十 | 声明 L1 会话级 32KB 截断 guard 未迁移；理由：Rust 工具输出经 per-tool max_output_bytes 有界 + truncated 诚实旗标（部分覆盖），会话级通用兜底待评估 | 清单条目 | 文档核对 |
| FIX-13 | N-10 | P1 | RC-2 | 报告 §十 | 声明压缩作用域=当前 run 的 exchange（现役=整会话），结构继承自 R03/R05，归因 R06-T04 | 清单条目 | 文档核对 |
| REG-01 | N-6 | P2 | — | — | 登记：post-run backstop 检查随 R06-T04 持久会话层一并设计 | — | — |
| REG-02 | 四段 enrichment | P2 | — | — | 登记：skill-recall/plan-file/context-notes/history-recovery 各归 R06-T06/plan 模式/context_notes/R06-T04 | — | — |

**不修的项（防止范围蔓延）**：kernel `summary_output_cap` 公式本身（注释锚定无误，公式逐字一致，保留为预算用途）；Rust 回退切点方向本身（功能更强，只改锚定不改行为）；placeholder 恢复臂（F-03 已真实关闭）。

---

## 七、现役语义 × Rust 状态完整矩阵

状态分类：**已对齐** / **已修复**（R1 修复经 R2 确认）/ **仍分叉未声明** / **已声明差异**（码内或 §十）/ **不适用**（现役死代码或结构差异）。

| # | 现役语义项 | 现役锚点 | Rust 状态 | 分类 | 关联 |
|---|---|---|---|---|---|
| 1 | 触发门：settings.enabled | session-compaction-runtime.ts | 无对应面 | 仍分叉未声明 | N-3/FIX-09 |
| 2 | 触发门：contextWindow>0 | 同上 | 已迁移（window=0→None，Probe D 实证） | 已对齐 | — |
| 3 | 触发门：role==assistant 且 stopReason∉{error,aborted} | 同上 | role 门已迁移；stopReason 门缺 | 仍分叉未声明 | N-2/FIX-08 |
| 4 | 触发门：usage 在场且 calculateContextTokens>0 | 同上 | 已迁移（无 usage/零 usage→Unavailable，Probe D） | 已对齐 | — |
| 5 | 触发门：usage 纪元检查（message.timestamp > 最近压缩时间戳） | 同上 | 结构性不适用（last_turn_usage 每次 settle 重赋、压缩后 tail_from 复位） | 不适用 | — |
| 6 | FORCE=0.8 严格大于阈值 | compaction.js:144-148 | 已迁移（ratio_bp≥8000 含边界，Probe D 逐点实证） | 已对齐 | — |
| 7 | ASK=0.5 事件线（5% 重问增量/20% 重置降幅） | session-compaction-runtime.ts | 未迁移（kernel 死常量 + 码内声明 L47-51） | 已声明差异（码内；§十 待补） | I-1/FIX-06 |
| 8 | reserve 动态计算 | compaction-guard-ext.ts L2 | 已迁移（compute_reserve_tokens，window=3 饱和 Force Probe D） | 已对齐 | — |
| 9 | MIN_RESERVE=16384 / KEEP_RECENT=20000 常量 | 现役 settings 可覆盖 | 常量冻结、覆盖面未接 | 仍分叉未声明 | N-7/FIX-09 |
| 10 | 用量口径：cache_read 族仲裁（OpenAI×3/Google 含 input，Anthropic 独立分量） | 现役 USAGE_MAPPINGS 消费纪律 | CacheInclusion 按真实 USAGE_MAPPINGS 仲裁，不双计、不虚构扣除、饱和减法不下溢 | 已修复 | F-02 |
| 11 | 尾部估算=本 turn toolResults | session-compaction-runtime.ts | 已迁移（runs.rs tail_from 口径，压缩后复位） | 已对齐 | — |
| 12 | 估算器：chars/4 纯字符（pi-sdk 切点/尾部链） | compaction.js:166-204 | Rust 用 CJK×1.1（对齐 hana estimate-text-tokens.ts） | 仍分叉未声明 | N-4/FIX-10 |
| 13 | 切点候选=除 toolResult 外全角色 | compaction.js:205-235 | Rust 候选=组边界（AssistantTurn‖CompactionSummary），无 user 项（结构继承） | 已声明差异（部分；snap 方向误锚） | R2-F-03 |
| 14 | 切点 snap 方向：前跳（retained 可 <keepRecent） | compaction.js:253-294 | Rust 回退（retained 恒 ≥keep_recent） | 仍分叉未声明 + 误锚 | R2-F-03/FIX-03 |
| 15 | 配对保护：工具调用与结果成对、无孤儿 | 任务书③/A03 | 三条件统一扫描 + suffix_min_owner + UnprovableToolPairs 响亮拒绝 | 已修复 | F-01 |
| 16 | pending 组硬上界（待完成调用不被压掉） | 任务书③ | 已迁移（Probe A：pending@0→Ok(None)） | 已对齐（经修复） | F-01 |
| 17 | 摘要指令模板 9 标题 + boundary placeholder + recent-tail 提示 | session-compactor.ts | 逐字迁移 | 已对齐 | — |
| 18 | split-turn scope 条件行 | session-compactor.ts:410-414 | 未迁移 | 仍分叉未声明 | R2-F-05/FIX-05 |
| 19 | customInstructions 注入 | session-compactor.ts | service 恒 None | 仍分叉未声明 | N-5/FIX-11 |
| 20 | 摘要包装 + MIDRUN notice 逐字、双 user 消息入场、非 system 槽 | session-compactor.ts / agent-run.ts | 逐字 + renderer 双臂核实 | 已对齐 | — |
| 21 | enrichment：fileOps `<read-files>`/`<modified-files>`（无外部依赖） | session-compactor.ts:1897-1912 + compaction-utils | 未迁移 | 仍分叉未声明 | R2-F-02/FIX-02 |
| 22 | enrichment：skill-recall / plan-file / context-notes / history-recovery | 同上 | 未迁移（依赖未迁移子系统） | 仍分叉未声明（可归因） | R2-F-02/FIX-02+REG-02 |
| 23 | placeholder 工具单次恢复（首次单工具意图→placeholder 续跑；二次/多调用→响亮失败） | agent-run.ts:122-135/523-538 | 已迁移（逐字 PLACEHOLDER 文本 + 恢复臂 + 闭环逐字配对断言） | 已修复 | F-03 |
| 24 | max_tokens 线体：optional-cap 族删除全部输出上限字段 | session-compactor.ts:335-342 + guard-ext:474 | Rust 恒发 Some（openai 线上实测 max_tokens=13107） | 仍分叉未声明 + 误锚 | R2-F-01/FIX-01 |
| 25 | max_tokens 线体：required-cap 族回填 min(model.maxTokens‖maxOutput, contextWindow) | session-compactor.ts:344-352 | Rust 恒发 0.8×reserve 公式值 | 仍分叉未声明 + 误锚 | R2-F-01/FIX-01 |
| 26 | 0.8×reserve 公式（预算估算/BOUNDED/兜底用途） | session-compactor.ts:377 | kernel summary_output_cap 公式逐字一致 | 已对齐（用途被误锚为线体默认） | R2-F-01 |
| 27 | RouteCompatHints：outputCapRequired 表达 | output-budget.ts | 配置面无表达（deny_unknown_fields） | 仍分叉未声明（registered gap） | N-8/FIX-01 |
| 28 | sanitize（闭合叙述块/未配对标记/空行塌缩）→ validate 输入 | agent-run.ts:596 | 已迁移（validate 输入一致） | 已对齐 | — |
| 29 | repair 载荷=当轮 rawText、草稿 turn=原始 assistant message | agent-run.ts:601 | Rust 用 sanitized.text | 仍分叉未声明 | R2-F-04/FIX-04 |
| 30 | 摘要失败保留原上下文、不截断冒充成功 | 任务书④ | 已迁移（失败臂 exchange 不动 + 台账可诊断） | 已对齐 | — |
| 31 | 压缩请求自身超窗：fit 检查→硬截断→native-fallback error | session-compactor.ts:2064-2102 | 未迁移且无熔断（每 turn 顶重试） | 仍分叉未声明（决策项） | N-1/FIX-07 |
| 32 | 手动压缩入口（Already compacted/Nothing to compact 语义） | session-compactor.ts | compact_now + NotTriggered 对应；mid_run 两臂正确 | 已对齐（经修复） | F-04 |
| 33 | 台账（触发/计划/结果/失败可诊断） | 现役日志事件面 | 已迁移（service L678-719） | 已对齐 | — |
| 34 | L1 会话级 tool_result 32KB head+tail 截断 guard | compaction-guard-ext.ts | 未迁移（per-tool 预算+truncated 旗标部分覆盖） | 仍分叉未声明 | N-9/FIX-12 |
| 35 | 压缩作用域=整会话（多 submission 历史） | 现役会话模型 | Rust=当前 run 的 exchange（单 submission） | 不适用（结构继承 R03/R05）→ 声明 | N-10/FIX-13 |
| 36 | usage 纪元投影 projectMessagesToLatestCompactionUsageEpoch | session-compactor.ts:104-136 | 结构性不适用 | 不适用 | — |
| 37 | 原生压缩路径 maxTokens=min(0.8×reserve, model.maxTokens) | compaction.js:380 | hana 生产链不走原生路径 | 不适用（现役死代码） | — |
| 38 | SDK post-run backstop 检查 | pi-sdk | 无挂载面 | 不适用（归 R06-T04） | N-6/REG-01 |
| 39 | reasoning 级别参数 | 现役压缩链同样无 | 无 | 不适用（R05 面，非 T02 分叉） | — |
| 40 | auxiliary 工具回调失败消息按 declared_tools 分流 | —（Rust 诚实规则） | auxiliary.rs:238-252 两臂 | 已修复 | F-06 |

矩阵合计：已对齐/已修复 21 项；仍分叉未声明 12 项（全部已编入 FIX-01..FIX-13）；已声明待补 §十 1 项；不适用 6 项。

---

## 八、未解决风险

1. **N-1 循环重试窗口（修复前存续）**：摘要请求自身超窗场景下，同一 run 每 turn 顶都会重发注定失败的摘要请求，直至 chat 调用自身超窗结束 run。成本与延迟真实存在；FIX-07 决策前无防护。
2. **FIX-01 族判定面的腐化风险**：若按协议族硬编码映射，未来新增协议族/路由时容易漏配（现役 explicit-required 已是配置驱动的先例）。建议归 RouteCompatHints 并以 `deny_unknown_fields` 的失败显式性兜底；无论选哪个，族矩阵测试（§五-5）必须随修复落地，否则下一轮审查还会在同一处再翻出来。
3. **CJK 估算差异的体验面**：N-4 方向保守（安全），但中文长会话会更早触发压缩、保留区按现役口径实量更小——属「安全但可感知」的保真差，声明后如需收敛应单独立项（改估算口径会同时影响触发点与切点，需重新标定 Probe D 的边界值）。
4. **fileOps 续传的解析依赖**：FIX-02 需从上一摘要的 `<read-files>`/`<modified-files>` 段解析续传；Rust 摘要包装器与现役不同，解析目标段是摘要正文内的段（非包装器），实现时须核对该段在 Rust 包装形状下的可解析性，避免「段追加了但下一轮续传读不到」的半修复。
5. **RC-2 的系统性残留**：本次扫描覆盖 compaction 全链，但同一执行者同期交付的其他 R06 面（如 T01 已冻结的 18 条差异之外的新增锚定）未在本扫描范围内；建议后续任务的执行者报告把「语义清单驱动测试 + §十 交叉锁」（§五-2/4）作为强制门禁，而不是靠审查轮次事后捕捞。
6. **本扫描自身的边界**：现役取证以仓库内 `node_modules/@earendil-works/pi-agent-core` 与 `core/`、`lib/` 现行源码为准；若上游 SDK 版本漂移，findCutPoint 等行号锚点需重新核对。矩阵 #37「原生路径=现役死代码」的判断基于 hana 生产链现行接法，未验证全部历史分支。
