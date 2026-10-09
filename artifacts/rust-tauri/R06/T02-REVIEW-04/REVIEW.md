# REVIEWER-R06-T02-R4 — 第四轮独立对抗性审查报告

## 〇、身份与候选

- 审查人：REVIEWER-R06-T02-R4（未参加本 Task 的实现、修复与前三轮审查）。
- 仓库：`/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。
- 候选 = HEAD `b174e3ff44c61000abbb84595cf69dfad85dd531` + 未提交工作树。
- 候选摘要（亲算，`bash scripts/rust-tauri/r06_candidate_digest.sh`，DIGEST_EXIT=0）：
  `02d43a2292513db978d0246106e2567ad5da88a6a25364434b2098174632d6af` —— 与修复者
  T02-REPAIR-03/12_candidate_digest.txt 的值逐字一致；本审查产物全部落在排除面
  （`artifacts/rust-tauri/R06/`）内，落盘后再算一次仍逐字一致（证据：
  T02-REVIEW-04/ 下日志头部）。
- 工具链：`/Users/study_superior/.cargo/bin/cargo`，`rustc 1.98.1 (48a229cea 2026-09-01)`。
- 纪律遵守：未修改任何产品代码与测试实现；只写 `artifacts/rust-tauri/R06/T02-REVIEW-04/`。

## 一、范围 A：完整原始 Task 重审（非只验修复点）

| 验收项 | 结果 | 证据 |
|---|---|---|
| Goal：token 预算 / 压缩 / 长会话生产线 | 成立 | kernel `compaction.rs`（1183 行规划/估算/fit/fileOps）+ service `compaction.rs`（969 行触发→路由→配额→调用→清洗→落账全链） |
| Step① 触发判定（FORCE 80% 线 + reserve=max(16384,⌈window×20%⌉) 线，真实 usage + 尾部估算） | 成立 | kernel `evaluate` L411-433（`ratio_bp >= 8000` 含边界 ∥ `context_tokens > window − reserve`）；`compute_reserve_tokens` L98-102（div_ceil(5).max(16384)）；runs.rs loop 顶触发 |
| Step② 切点规划（配对证明 + pending 硬上界 + 无孤儿三条件） | 成立 | kernel `plan_compaction` L530-682：配对证明（UnprovableToolPairs 响亮拒绝）+ `earliest_pending` 硬上界 L660 + `suffix_min_owner[cut] ≥ cut` L617-633；候选扫描 `(1..=crossing).rev()` L661 |
| Step③ 摘要调用（缓存保留形状 + placeholder 单次恢复 + 清洗/一次修复） | 成立 | service L524-623（prior 会话式累积、单次恢复、二次/多调用响亮失败）；L626-682（sanitize→validate→repair 载荷=当轮 rawText）；探针 P2/P6 行为级复证 |
| Step④ 失败保留原上下文、台账可诊断 | 成立 | `Failed{detail}` 臂 exchange 不动（探针 P3 对照 + 既有测试 `provider_failure_keeps_the_original_exchange_and_accounts_it`）；台账行含 outcome/attempts/身份 |
| Deliverable 1：kernel 压缩模块 | 成立 | `lingxi-kernel/src/compaction.rs` + 44 测试全绿 |
| Deliverable 2：service CompactionService | 成立 | `lingxi-service/src/compaction.rs` + 26 测试全绿 |
| Deliverable 3：渲染器四族摘要/指令臂 | 成立 | adapters 12 渲染测试全绿（`r06_t02_compaction_render`） |
| R06-A03（长历史越线自动压缩闭环） | 成立 | `r06_t02_closed_loop.rs` A03 腿（2 测试全绿，本轮亲跑） |
| R06-A04（摘要失败 run 续跑） | 成立 | 同文件 A04 腿（content_filter 拒绝 → 原历史续跑） |
| Depends On = T01 | 成立 | runs.rs 取 `CompiledContext` 同一 artifact（T01 冻结产物），无第二次 prompt 重建 |
| 生产调用链 | 成立 | lib.rs L1273-1283：injected 优先（测试缝），wired_real_model_chain 时在同一 gateway/凭证/网络面（C07）与同一配额管理器（C06）上构建生产实例；无真实模型链 = 无压缩 |

## 二、范围 B：历史 findings 全闭环独立复证

### R1 六项
- F-01（保留区孤儿，HIGH）：kernel 三条件统一扫描 + `suffix_min_owner` 在源（L610-633）；kernel 44 测试含双向无孤儿断言，本轮亲跑绿。**真实关闭**。
- F-02（cache_read 双计，HIGH）：`usage_inclusion_of(chat 路由族)` 在 service L183 解析，族仲裁逻辑在 kernel `context_tokens_from_usage`；本轮亲跑 kernel 套件绿。**真实关闭**。
- F-03（placeholder 单次恢复，MEDIUM）：service L554-585 恢复臂在源；本轮新探针 P2 行为级复证（意图 turn + 恰好一次恢复、placeholder 逐字、二次路径未触发）。**真实关闭**。
- F-04（手动 mid_run 硬编码，LOW）：`apply_plan(..., mid_run)` 参数化在源（L458-463/L636）；探针 P4/P5 实证手动产物 `mid_run=false`。**真实关闭**。
- F-05（报告笔误，LOW）：报告 §十 现行文本无「字节÷4」表述。**关闭**。
- F-06（工具回调失败消息误称，LOW）：auxiliary.rs L238-252 两臂分流（declared tools ONLY / declared NO tools）在源。**真实关闭**。

### R2 七项
- R2-F-01（max_tokens 族分流 + §十#5 误锚，MEDIUM）：service L485-499 读生效路由快照分流 `Option<u32>`；kernel `output_cap_required` 判定顺序与现役 `output-budget.ts` 逐字；`RouteCompatHints.outputCapRequired`（camelCase、deny_unknown_fields 下可表达）在 config.rs。**真实关闭**。
- R2-F-02（enrichment 段族，MEDIUM）：fileOps 段已迁移（kernel `extract_file_operations`/`append_file_operation_context`），其余四段 §十#6 登记 REG-02 归因（skill-recall→R06-T06 等）。**真实关闭**。
- R2-F-03（snap 方向误锚，LOW）：代码注释 L649-655 与报告 §十#11 均锚定为「Rust 回退、保留区恒 ≥keep_recent」的方向分叉；行为保留不改（「不修」项）。**真实关闭（文档向）**。
- R2-F-04（repair 载荷，LOW）：service L644-649 repair 用当轮 `draft`（原始文本），sanitize 只服务 validate 与落库；探针 P6 实证 repair 载荷内嵌 `<mood>busy</mood>` 原始文本。**真实关闭**。
- R2-F-05（split-turn scope 行，LOW）：kernel `SummaryInstructionSpec.split_turn` + 条件行 L879-885（recent-tail 之后、customInstructions 之前，逐字）在源。**真实关闭**。
- I-1（ASK 事件线，info）：kernel 死常量 + 码内声明 L47-51；§十#8 在册。**关闭（声明）**。
- I-2（hard-truncate 出口，info）：fit 检查 + 诚实硬截断臂在源（service L436-477）；§十#12 在册（含 R3 轮改锚）。**关闭**。

### R3 三项
- R3-F-01（摘要路由分叉两个生产可达退化面，HIGH）：见范围 C 深查 + 探针 P1-P6。**真实关闭**。
- R3-F-02（digest 口径，MEDIUM）：口径固化脚本存在（26 行，排除面 = 审查产物目录 + 载体文件），声明口径与脚本头注释逐字一致；两轮亲算同一值。**真实关闭**。
- R3-F-03（交叉锁 stray 判定近似，MEDIUM）：`check_register_lock` 已改为双向精确集合匹配（service 测试 L2343-2367），篡改自证四臂（删中间条目+引用残留→红；有条目无引用→红；精确相等→绿；端到端临时目录→红）在源；本轮集合级手工核对 rust/ 引用集合 == 报告条目集合 {1..17}。**真实关闭**。

## 三、范围 C：R3 修复重点深查（全部通过）

1. **回退链代码形状**（service compaction.rs L344-387）：summarize 槽解析失败 → chat 回退；双缺 → `Failed{detail}` 同时点名 summarize 与 chat（L377-381）。逐行核读成立。
2. **自动路径同代快照复用**：L223 `Some(&chat_dispatch)` —— 入口（L160-173）解析的同代快照克隆进回退腿，绝不二次读取（R05 RR1 F02 纪律）；手动路径 L300 传入 `chat_dispatch.as_ref()`，None 时回退腿补一次解析（L366-369）。成立。
3. **双缺双点名**：detail 含 `summarize` 与 `chat` 两词；既有测试 `unresolvable_summarize_slot_and_chat_route_fail_loudly` 断言；探针 P3 实证自动路径该臂结构上不可达（入口门 L171 NotTriggered）。
4. **现役锚点逐字对齐**：`core/auxiliary-slots.ts:83-88` summarize 槽 `fallback:"chat"` ✓；`core/auxiliary-model-resolver.ts` 未配置→回退 / 已配置不可用→配置错误 ✓；`core/session-compactor.ts:1987` `model = session?.model` ✓、`:1655` contextWindow 与 `:2064-2074` fit 检查同一 model ✓。
5. **settle_summary_call 三调用点透传**：L533（首发）/L590（恢复）/L665（修复）均传 `summary_operation` + `&summary_label`；探针 P2（恢复臂）与 P6（修复臂）实证两调用点在回退路由下行为正确。
6. **complete_step_for_operation 等价性**（auxiliary.rs L289-330）：operation 照常经 gateway 自解析（执行器自身永不回退，C07 不变）；`complete_step` 委托时 label = `"auxiliary slot {config_key()}"`（Summarize→"summarize"，与旧文本逐字一致）；其他槽行为不变。
7. **§十#17 三后果声明**：fit 窗口（L414-423 生效路由窗口 orElse 回退后路由窗口 unwrap_or(0)）、族分流与 max_tokens 回填（L485-499 读生效路由快照）、能力门（provider.rs L325-357 按发送时生效路由的同代快照判定）全部随生效路由。成立。
8. **§十#12 新锚定**：fit 窗口改锚「生效摘要路由窗口」，并记录 R2 轮误锚史。成立。
9. **digest 脚本排除规则与自引用消除**：排除 (a) 审查产物目录（含载体自身——自引用无不动点）(b) `candidate_digest*.txt` 双保险；报告在口径内且正文不含 digest 值（grep 实证）。成立。
10. **同根因横扫**：rust/ 全树仅 compaction.rs:355 一处为压缩解析 summarize 槽（L359 为 Ok 臂身份、L888 为台账 purpose 常量；config.rs/model_exchange.rs 为槽位管道），无第二处缺回退的解析点。簇真实关闭。

## 四、范围 D：全新对抗探针（7 个，非重放；全部绿，PROBE_EXIT=0）

探针 crate：`artifacts/rust-tauri/R06/T02-REVIEW-04/probe/`（path 依赖 rust/ 四 crate）；
输出证据：`T02-REVIEW-04/probe_output.txt`。

| # | 探针（新组合） | 结果 |
|---|---|---|
| P1 | 回退成功但 chat 窗口仍不足（无槽 × chat 窗口 1000 × 自动路径） | HardTruncated：marker 逐字、mid_run=true、保留区逐字、**0 调用 0 台账**——诚实降级臂在回退路由上成立 |
| P2 | placeholder 单次恢复 × chat 回退链 | 2 发线体均 `model=chat-model`、恢复请求含 placeholder 逐字 + role:tool 配对、台账 2 行 purpose=`auxiliary.summarize` 身份=chat-model、意图行 emitted_tool_calls=1 |
| P3 | 自动路径 × chat/summarize 双缺（usage 给足 9M） | 触发门 NotTriggered——双缺 Failed 臂自动路径结构上不可达 |
| P4 | 手动 compact_now × 无槽 × chat 窗口未声明 | HardTruncated、mid_run=false、0 调用（手动×回退×窗口未声明三重组合） |
| P5 | 手动两轮 × 回退路由 | fileOps 跨纪元续传成立（播种 ∪ 新增；/r4/one、/r4/two、/r4/three 俱在；write 过的留在 modified）；两发 chat-model |
| P6 | 修复臂（settle 第三调用点）× 回退路由 | 首发无效摘要 → 恰好一次修复；两发 chat-model；repair 载荷内嵌当轮原始文本（`<mood>busy</mood>` 仍在 `<draft-summary>` 内） |
| P7 | 交叉锁正文稀释演示（纯逻辑） | 编号集合不变、正文掏空 → 锁仍绿。锁的声明契约 = 锚定存在性（编号双向匹配），不管正文保真——记录为 R4-I-01（informational），非缺陷 |

## 五、范围 E：R05 回归与质量门（全部亲跑，真实退出码）

| 门 | 结果 | 证据 |
|---|---|---|
| `cargo fmt --all -- --check` | FMT_EXIT=0 | fmt.log |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | CLIPPY_EXIT=0 | clippy.log |
| kernel `r06_t02_compaction` | 44 passed / 0 failed | target_suites.log |
| adapters `r06_t02_compaction_render` | 12 passed / 0 failed | 同上 |
| service `r06_t02_compaction` | 26 passed / 0 failed | 同上 |
| service `r06_t02_closed_loop`（A03/A04） | 2 passed / 0 failed | 同上 |
| service `r05_t01_binary_wiring` | 2/0 | r05_adjacent.log |
| service `r05_t05_timeouts` | 9/0 | 同上 |
| adapters `r05_t05_timeouts` | 13/0 | 同上 |
| adapters `r05_t05_compat` | 1/0 | 同上 |
| service `r05_t08_closed_loop` / `production_tools` / `resources` | 11/0、6/0、2/0 | 同上 |
| **`cargo test --workspace --locked` 全量** | **1612 passed / 0 failed，WORKSPACE_EXIT=0**（与 REPAIR-03 证据 11_test_workspace.full.log 的 1612/0 一致） | workspace_test.log |

R05 回归 diff 逐行核对（runs.rs 触发接线/FIX-08 三分支/tail_from 推进；四族渲染器只增两臂；config.rs +outputCapRequired；model_exchange.rs +2 变体；lib.rs 生产装配；workermodel.rs +3 字段；r05_t05_compat.rs +1 行；r05_t05_timeouts.rs +8 穷举臂）——全部只增不改语义，既有 R05 行为面零回归（上表套件实证）。

## 六、范围 F：防虚假完成

- **§十 抽查 10/17 条**（#1 指令边界项数 #2 读时修复未迁移 #5 族分流 #7 marker 逐字 #8 ASK 死常量 #10 CJK×1.1 估算器 #11 切点回退 #12 fit 检查 #16 stopReason 门落位 #17 回退链）——全部与代码/现役锚点相符。
- **40 行矩阵抽查 12 行**（#2 window=0→None #4 无/零 usage→Unavailable #6 FORCE≥8000 含边界 #8 reserve 公式 #15 三条件+响亮拒绝 #16 pending 硬上界 #20 MIDRUN notice 逐字 #23 placeholder 逐字+恢复臂 #28 sanitize→validate #30 失败保留原文 #32 手动 mid_run=false #40 两臂分流）——全部相符。
- **「不修」三项未被顺手改**：summary_output_cap 公式 `(reserve×4/5).max(512)` 原样（kernel L257-260）；切点回退方向 `(1..=crossing).rev()` 原样（L661）；placeholder 恢复臂原样（service L554-585）。
- **无平行实现**：`plan_compaction`/`apply_plan`/`estimate_exchange_tokens` 全树唯一定义于 kernel compaction.rs。
- **REPAIR-03 证据链完整**：00 工具链 → 01 修复前探针 → 02/03 红（三测试 CARGO_EXIT=101 ×3）→ 04/05/06/07 绿 → 08 fmt/clippy → 09/10/11 测试 → 12 digest，编号无缺口，红绿相位正确。

## 七、Findings

### R4-F-01（LOW，非阻断）

- **FINDING_ID**：R4-F-01
- **SEVERITY**：LOW（文档精度，非行为）
- **REQUIREMENT_ID**：报告 §十#17 / R3-F-01
- **FILE_AND_LINE**：`rust/crates/lingxi-service/src/lib.rs:673-678`（ServiceDeps::compaction_service 文档注释）；`rust/crates/lingxi-service/src/runs.rs:597`（RunSupervisor::compaction 文档注释）
- **OBSERVED_BEHAVIOR**：两处文档注释仍写「compacts the live exchange through the Summarize auxiliary slot」，未提 R3-F-01 引入的 chat 回退链。
- **EXPECTED_BEHAVIOR**：与 compaction.rs L306/L344 及报告 §十#17 一致，点明「summarize 槽显式覆盖 + chat 回退」。
- **REPRODUCTION**：grep 对照（本报告 §三-10 的检索输出）。
- **ROOT_CAUSE**：R3 修复只改了执行体与紧邻注释，外围两处结构体文档未同步（RC-2 文档精度的残余涟漪，但主文档面已准确）。
- **SAME_ROOT_CAUSE_PATHS**：全树检索 `Summarize auxiliary slot` 仅这三处；lib.rs:1275 的一处是 C07 网关共享声明（槽经同一 ModelGateway 解析），回退不改变该事实，不算滞后。
- **IMPACT**：读者仅从这两处注释理解时会漏知回退链；行为、测试、报告均正确。
- **REQUIRED_FIX**：两处注释补「（槽未配置/不可路由时回退 chat 路由）」半句。可在任意后续顺手轮完成，不阻断本 Task 验收。
- **REGRESSION_TESTS**：无需（文档）；如修，保持 grep 可证即可。

### R4-I-01（informational）

- **FINDING_ID**：R4-I-01（探针 P7）
- **SEVERITY**：informational
- **REQUIREMENT_ID**：RC-3 §五-4 交叉锁
- **FILE_AND_LINE**：`rust/crates/lingxi-service/tests/r06_t02_compaction.rs:2284-2367`
- **OBSERVED_BEHAVIOR**：交叉锁双向匹配的是「§十 编号集合 == 代码引用集合」；条目正文稀释（编号保留、内容掏空）不改变集合，锁仍绿。
- **EXPECTED_BEHAVIOR**：锁的声明契约即锚定存在性；正文保真由人工抽查与本类对抗审查承担（本轮 §十 抽查 10/17 即一例）。
- **REPRODUCTION**：探针 P7（probe_output.txt）。
- **ROOT_CAUSE**：设计边界，非实现缺陷。
- **SAME_ROOT_CAUSE_PATHS**：无。
- **IMPACT**：无（契约明示范围内）。
- **REQUIRED_FIX**：无。可选加固（正文哈希入锁）属新需求，不在本 Task。
- **REGRESSION_TESTS**：既有篡改自证四臂已足。

### R4-I-02（informational）

- **FINDING_ID**：R4-I-02
- **SEVERITY**：informational
- **REQUIREMENT_ID**：§十#17
- **FILE_AND_LINE**：`rust/crates/lingxi-service/src/compaction.rs:363-386`
- **OBSERVED_BEHAVIOR**：Rust 回退在 summarize 槽**任何**解析错误（含「已配置但不可用」）时触发；现役 resolver 在「已配置不可用」臂报配置错误不回退。
- **EXPECTED_BEHAVIOR**：与现役**压缩**语义对齐——现役压缩根本不经槽（`session.model` 本人），任何槽态下压缩都走会话模型；Rust 的「任何槽错误→回退 chat」在方向上更贴近现役压缩语义。
- **REPRODUCTION**：代码形状 + 现役锚点对照（§三-4）。
- **ROOT_CAUSE**：有意的设计取舍（槽语义服务于压缩主目标，非逐字复刻 resolver 的全槽通用语义）。
- **SAME_ROOT_CAUSE_PATHS**：无。
- **IMPACT**：配置错误的 summarize 槽不再阻断压缩（回退 chat）；配置错误本身仍可由槽自身的其他消费面暴露。方向安全。
- **REQUIRED_FIX**：无。
- **REGRESSION_TESTS**：探针 P1-P6 已覆盖回退各臂。

## 八、裁决

- 全部 REQUIRED 验收成立（范围 A/B/C/E/F 全绿）；
- 根因簇真实闭环（R1×6、R2×7、R3×3 全部独立复证关闭，同根因横扫无遗漏解析点）；
- 无新阻断项（新发现仅 1 项 LOW 文档滞后 + 2 项 informational）。

**VERDICT: PASS**
