# R06-T02 独立对抗性审查报告（REVIEWER-R06-T02-R1）

- **审查对象**：R06-T02 token 预算、压缩与长会话
- **候选**：HEAD `b174e3ff44c61000abbb84595cf69dfad85dd531` + 未提交工作树
- **候选摘要核对**：按执行者口径（`{ git diff; git ls-files --others --exclude-standard | sort | grep -v '^docs/rust-tauri/R06/R06-T02_REPORT.md$' | xargs shasum -a 256; } | shasum -a 256`，排除本审查产物目录后）重算 = `c24b44052e68fb38882325969e4c9a632e44e1a9b906fb61af67dee63b97a8aa`，与执行者声称**逐字一致**。
- **工作树核对**：`git status --porcelain` = 12 修改（含总控账本 R06_PROGRESS.json，diff 确认仅为 T01 DONE / T02 IN_PROGRESS 回填，非执行者代码改动）+ 8 新增路径，与预期一致。
- **工具链**：`/Users/study_superior/.cargo/bin/rustc` = 1.98.1 (48a229cea 2026-09-01)，锁定工具链，全部命令亲跑。

---

## 亲自运行验证（硬性项，全部真实退出码）

| 命令 | 结果 |
|------|------|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| `cargo test --locked -p lingxi-kernel --test r06_t02_compaction` | 30 passed / 0 failed（测试名与报告 §六逐字一致） |
| `cargo test --locked -p lingxi-adapters --test r06_t02_compaction_render` | 11 passed / 0 failed |
| `cargo test --locked -p lingxi-service --test r06_t02_compaction` | 14 passed / 0 failed |
| `cargo test --locked -p lingxi-service --test r06_t02_closed_loop` | 2 passed / 0 failed |
| R05 回归：`r05_t01_binary_wiring` / `r05_t05_timeouts`(service 9+adapters 13) / `r05_t08_closed_loop` / `r05_t08_production_tools` / `r05_t08_resources` / adapters 全量 | 全绿（2 / 9+13 / 11 / 6 / 2 / 全部 ok） |
| `cargo test --workspace --locked` 全量（后台亲跑） | **exit 0，122 个 target 全 ok，1585 passed / 0 failed**——与执行者证据 `07_test_workspace.txt` 逐字一致 |
| `r04_rr1_f05_spill_failure` 独立重跑 ×2 | 2/2 绿（0.02s）；且我的全量跑同 target 亦绿。**执行者「首轮崩溃=环境压力」的判定独立成立，非代码回归，非阻断** |

## 生产路径真实性（逐行核实，成立）

bootstrap `wired_real_model_chain` 分支（lib.rs L1272-1322）以同一 gateway/credential/网络面建 AuxiliaryExecutor → `CompactionService::new(auxiliary, gateway, runs.quotas_shared(), clock)` → `RunSupervisor::with_compaction`；drive_run loop 顶（runs.rs L1274，tool_snapshot 之后、call_deadline 之前）调 `maybe_compact_mid_run` → `ContextBudget::evaluate` → `compact_exchange`（plan → summarize 路由前检 → 同一 QuotaManager 配额 → AuxiliaryExecutor → 清洗校验（一次修复）→ apply_plan）。自动与手动（`compact_now`）共用同一执行体 ✓；摘要调用经 ModelGateway+CredentialService 零旁路 ✓；闭环测试为真二进制子进程（真 argv/--config/认证面/loopback stub），A03 腿断言线体配对/摘要身份/台账行，断言真实非恒真 ✓。Depends On=T01 成立：压缩请求 system 槽来自 T01 artifact，估算器复用 T01 冻结函数 ✓。

## R05 回归静态审查（成立）

四族渲染器 diff 逐行核对：**只新增** CompactionSummary（user 角色 + 可选 notice）与 CompactionInstruction（user 角色）两臂，既有臂零变化；codex 经 `render_input_items` 继承 ✓。`r05_t05_timeouts.rs` 仅 +8 行穷举臂，无断言删除/放宽 ✓。`ExchangeItem` 无 serde 派生（纯内存态），新变体无持久化向后兼容面 ✓。现役锚定抽查：`MIDRUN_COMPACTION_NOTICE`/九标题/包装前后缀/`max(512, floor(0.8×reserve))`/reserve 公式/FORCE 0.8/keep_recent 20000 全部逐字一致 ✓；`estimate_text_tokens` 与现役 `estimateTextTokens`（CJK×1.1、其余字符÷4、同七段码点表）逐项一致 ✓。

---

## FINDINGS

### F-01｜plan_compaction 未实现其契约声称的「保留区无孤儿 tool result」保证

- **SEVERITY**: HIGH（契约虚假 + A03 证据链缺口；当前生产形状恰好不可达，见下）
- **REQUIREMENT_ID**: R06-A03（「不留下孤立 tool result」）；任务书步骤③；kernel 注释 L377-382 声称的保证 2
- **FILE_AND_LINE**: `rust/crates/lingxi-kernel/src/compaction.rs:383-512`（`plan_compaction`；配对证明 L390-444 只验证「result 有 owner 且 owner 下标 < result 下标」，切点回退 L487 只保证切点项类型合法）；注释声称在 L377-382
- **OBSERVED_BEHAVIOR**: 对交换 `[A0(调用tc1), A1(无调用), R(tc1), A2(调用tc2), R(tc2)]`（配对证明可通过：每个 result 有 owner 且 owner 在前、无重复），keep_recent=2000 时产出 `cut_index=1`——保留区 `[A1, R(tc1), A2, R(tc2)]` 中 **R(tc1) 的 owner A0 被压进摘要区，保留区携带孤立 tool result**。渲染上线即产生无先行 tool_calls 的 role:"tool" 消息（OpenAI 族 = provider 400 协议拒绝）。
- **EXPECTED_BEHAVIOR**: 注释声称「保留区内每个 toolResult 的归属 assistant turn 同在保留区（无孤立 result）」；代码无任何机制实现它（没有「owner 与 result 不得被切点分开」的约束或切点后验证）。
- **REPRODUCTION**: `artifacts/rust-tauri/R06/T02-REVIEW-01/probe/`（独立 crate，path 依赖 lingxi-kernel，不改产品代码）：`cargo run` 输出 `probe1: cut_index = 1` / `ORPHANED tool result tc0001 lands in the RETAINED region` / `verdict: VULNERABLE`。
- **ROOT_CAUSE**: 配对证明只建「result→owner 下标序」关系，切点选择（`(1..=crossing).rev().find(valid_cut)`）只按「项类型 + 下标>0」回退，从不检查保留区内 result 的 owner 是否同区。执行者全部测试夹具（`long_exchange`）的 owner 与 result **恒紧邻**，「切点落组边界」在该形状下自动蕴含「owner-result 同区」——`planner_never_orphans_in_either_direction` 的断言在该夹具上恒真，无法暴露夹层形状。
- **SAME_ROOT_CAUSE_PATHS**: 探针 3 坐实后果放大——孤儿化压缩产物再次进入 `plan_compaction` 时配对证明失败（`tool result tc0001 has no owning assistant turn`）→ **该会话此后所有压缩永久 Failed**（上下文只涨不压）。`apply_plan`（L515-528）信任 plan 不复核。当前唯一生产者 drive_run 的 push 序（AssistantTurn→其工具批结果→下一 turn AssistantTurn）使 owner-result 恒紧邻，**该夹层形状在当前生产循环不可达**——但 (a) kernel 公共契约注释明确声称该保证；(b) 执行者报告 §八-1 引用 planner 层断言作为 A03 证据的一部分；(c) R06-T03（fork/重试插入 turn）/R06-T04（恢复历史进交换）将扩大交换来源；(d) 现役有 provider-compat 兜底而 Rust 侧无任何兜底。
- **IMPACT**: 若形状可达：压缩后下一次模型调用即协议违例被拒，run 失败——正是任务书 Goal「压缩不破坏工具协议」要防的事；且该 run 之后永不压缩。
- **REQUIRED_FIX**: `plan_compaction` 在切点选定后校验保留区每个 ToolResult 的 owner 同在保留区，不满足则继续回退至满足或 `Ok(None)`；或把 owner 组与其 result 作为原子单元参与切点回退。
- **REGRESSION_TESTS**: 夹层形状 `[A(call), A(no-call), R, A(call), R]` 在多种 keep_recent 下切点不得制造孤儿；现有 `planner_never_orphans_in_either_direction` 应改用含夹层的夹具重证。

### F-02｜OpenAI×3/Google 族的 cache_read 在触发判定中被双计（违反 R05 消费者纪律）

- **SEVERITY**: HIGH（任务书步骤①「迁移现役触发语义」对 4/5 协议族系统性失真；方向保守=更早压缩，不致超窗）
- **REQUIREMENT_ID**: R06-T02 步骤①；R05 RR1 F23 消费者纪律（`usage.rs:67-69`「A consumer NEVER re-adds a component when the flag says included」）
- **FILE_AND_LINE**: `rust/crates/lingxi-kernel/src/compaction.rs:96-114`（`context_tokens_from_usage` 四分量无条件相加）；同根因：`rust/crates/lingxi-adapters/src/models/usage.rs:88/123/138/151`（四族 `cache_read_included_in_input: Some(true)`）与 `decode_family_usage`（L296-303，不扣除）
- **OBSERVED_BEHAVIOR**: OpenAI 族 wire `prompt_tokens=100_000`（含 cached 30_000）、completion=500 时，Rust 判定期权 contextTokens = **130_500**。现役等价口径（pi-ai `openai-completions.js:1173` `input = prompt_tokens − cacheRead − cacheWrite`，`calculateContextTokens = totalTokens || 四分量加`）= **100_500**。高估 30%（缓存命中越多高估越甚，极限近 2 倍）。Anthropic 族不受影响（cache 分量原生独立，`Some(false)`）。
- **EXPECTED_BEHAVIOR**: included 旗标为 true 的族，其 cache_read 分量不得再次加入总量（R05 文档化纪律）。
- **REPRODUCTION**: 同一探针 crate `probe2`: `rust context_tokens = Some(130500), incumbent-equivalent = 100500` / `verdict: DOUBLE-COUNTS cache_read`。
- **ROOT_CAUSE**: 执行者核对了现役公式文本（`input+output+cacheRead+cacheWrite`）但未核对两边 **input 分量口径**（现役 input 已扣 cache；R05 统一结构的 input 含 cache）。kernel 测试 `cache_components_count_toward_usage_total`（r06_t02_compaction.rs:293-310）把双计行为锁成预期，其注释「现役 calculateContextTokens = …」锚定有误。
- **SAME_ROOT_CAUSE_PATHS**: `ContextBudget::report`/`evaluate`（同一函数供血）；`compact_now` 的报告分量。台账面不受影响（R05 台账口径独立）。
- **IMPACT**: 缓存命中率高的生产长会话（正是压缩的目标场景）触发大幅提前 → 压缩频率与摘要密度显著高于现役，上下文质量损失；与现役触发时机不可对照。
- **REQUIRED_FIX**: 判定时按族旗标扣除：`chat_dispatch` 可得 family → `mapping_of(family).cache_read_included_in_input == Some(true)` 时总量减去 cache_read（cache_write 同理，当前四族 cache_write included 均为 None/false，Anthropic false 不变）。kernel 函数加旗标入参或族入参，service 层传入。
- **REGRESSION_TESTS**: OpenAI 族 usage（input 含 cache_read）→ contextTokens = input+output+cache_write（不双计 cache_read）；Anthropic → 四分量全加；修正 `cache_components_count_toward_usage_total` 的锚定注释并补族维度用例。

### F-03｜现役「placeholder 工具单次恢复」语义未迁移且未列入差异清单

- **SEVERITY**: MEDIUM
- **REQUIREMENT_ID**: R06-T02 步骤①（错误恢复语义）
- **FILE_AND_LINE**: `rust/crates/lingxi-adapters/src/models/auxiliary.rs:264-273`（任何 ToolRequests → 立即失败）；现役对照 `lib/llm/cache-preserving-compaction-agent-run.ts:122-135`（clonePlaceholderTools）与 L523-538（`shouldStopAfterTurn`：首次单工具意图 → `tool_recovery` 继续，非失败）
- **OBSERVED_BEHAVIOR**: 摘要模型回调工具 → Rust 立即 `Failed`。现役：单次单工具意图由 placeholder 应答（"Tool intent was preserved…"）后 loop 继续产出摘要；仅重复/多调用才失败。执行者报告 §一称「模型回调工具 = 响亮失败（现役 placeholder 工具语义）」——把恢复语义误述为失败语义；§十差异清单未收录。
- **EXPECTED_BEHAVIOR**: 迁移或明确列入差异清单（T01 先例：全部有意差异逐条冻结）。
- **REPRODUCTION**: 静态对照（两处代码引用如上）；行为方向更保守（压缩成功率略降，不破坏会话）。
- **ROOT_CAUSE**: AuxiliaryExecutor 的单 turn 契约（R05 遗留「auxiliary 无工具循环」）直接复用，未为压缩槽恢复 tool_recovery。
- **SAME_ROOT_CAUSE_PATHS**: `complete()` 是所有 auxiliary 槽唯一执行体；仅压缩槽携带 tools 快照（其他槽工具回调失败语义本就如此）。
- **IMPACT**: 真实模型偶发的单工具意图在现役可恢复、在 Rust 判压缩失败（run 以原文续跑）——语义更保守但偏离现役，且未声明。
- **REQUIRED_FIX**: 列入差异清单（标注「更保守、可接受」），或为压缩请求移除 tools 声明改走 placeholder 等价物；二选一，须明示。
- **REGRESSION_TESTS**: 若选迁移：单次工具意图→placeholder 应答→摘要成功；若选声明：差异清单条目 + 现有失败测试保留。

### F-04｜手动压缩产物硬编码 mid_run=true（与现役 /compact 不带 notice 不符，未声明）

- **SEVERITY**: LOW（当前无生产调用方——报告 §十二-5 已声明 compact_now 无宿主）
- **REQUIREMENT_ID**: R06-T02 步骤①（手动压缩语义）
- **FILE_AND_LINE**: `rust/crates/lingxi-kernel/src/compaction.rs:515-528`（`apply_plan` 硬编码 `mid_run: true`）；现役对照 `core/bridge-session-manager.ts:1727 compactSession`（不追加 notice）vs `core/session-compaction-runtime.ts:220-224`（仅 mid-run 触发路径追加）
- **OBSERVED_BEHAVIOR**: `compact_now` 产出的摘要项渲染时附加 `MIDRUN_COMPACTION_NOTICE`（"You are still mid-task…"）。`manual_compaction_skips_the_threshold_and_compacts` 未断言 mid_run 值，未锁定该行为。
- **EXPECTED_BEHAVIOR**: 手动（run 外）压缩不带 mid-run notice（现役）；或列入差异清单。
- **REPRODUCTION**: 静态（`apply_plan` 唯一构造点）；service 直测可观测 `compacted[0].mid_run == true`。
- **ROOT_CAUSE**: `apply_plan` 无触发源参数；`compact_exchange` 未区分自动/手动语义。
- **SAME_ROOT_CAUSE_PATHS**: 无其他构造点。
- **IMPACT**: 未来命令面接入时，run 外手动压缩会向模型谎称「仍在任务中」；当前不可达。
- **REQUIRED_FIX**: `apply_plan`/`compact_exchange` 加 mid_run 入参，`compact_now` 传 false；补断言。
- **REGRESSION_TESTS**: `compact_now` 产物 mid_run=false 且渲染无 notice；自动路径保持 true。

### F-05｜报告 §一步骤②「其余字节 ÷4」为笔误（代码为「字符 ÷4」，与现役一致）

- **SEVERITY**: LOW（文档准确性；代码正确）
- **FILE_AND_LINE**: `docs/rust-tauri/R06/R06-T02_REPORT.md:21` vs `rust/crates/lingxi-kernel/src/context.rs:51-65`（`text.chars()` 码点迭代）与现役 `lib/llm/estimate-text-tokens.ts:36-47`
- **OBSERVED/EXPECTED**: 报告描述与实现/现役不符；实现正确。
- **REQUIRED_FIX**: 报告措辞改为「其余字符 ÷4」。
- **REGRESSION_TESTS**: 不需要（文档）。

### F-06｜压缩槽工具回调的失败消息误称「declared NO tools」

- **SEVERITY**: LOW（可诊断性措辞；行为正确）
- **FILE_AND_LINE**: `rust/crates/lingxi-adapters/src/models/auxiliary.rs:264-273`
- **OBSERVED_BEHAVIOR**: 压缩请求实际携带 tools 快照（已声明工具），模型回调工具时失败消息仍说「declared NO tools but the provider answered with tool requests」。
- **EXPECTED_BEHAVIOR**: 消息按槽区分（压缩槽：工具已声明但禁止调用——对应 F-03 的语义选择）。
- **REQUIRED_FIX**: 随 F-03 的处理一并修正消息文本。
- **REGRESSION_TESTS**: 失败消息内容断言（现有 `capability_gate_refuses_a_toolsless_summarize_route` 不受影响——那是路由声明面，非回调面）。

---

## 分项结论

- **一、任务完整性**：Goal/Steps/Deliverables/A04 形式上齐备且大部分真实；A03 的 planner 层证据含虚假声称（F-01）；步骤①触发语义失真（F-02）与恢复语义未声明缺口（F-03）。
- **二、生产路径真实性**：成立（loop 真实触发、真二进制闭环、零旁路、无平行构造者）。
- **三、对抗性验证**：执行者覆盖清单基本如实；我亲自构造的 3 个反例探针（孤儿切点 / cache 双计 / 孤儿毒化二次压缩）全部坐实。
- **四、R05 回归**：diff 逐行核对只增不改；受影响套件与 adapters 全量亲跑全绿。
- **五、防虚假完成**：现役锚定抽查大体逐字一致；F-02/F-03/F-04 即「机制可运行 ≠ 现役语义恢复」的实证。
- **六、亲自运行验证**：fmt/clippy/57 新测试/全量 1585/候选摘要重算，全部与执行者声明一致；r04 崩溃独立判定为环境 flake。

## 最终裁决

**VERDICT: FAIL**

阻断项：F-01（A03 核心保护「保留区无孤儿 tool result」的契约声称被反例推翻——注释承诺与测试声称的保护在代码中不存在；后果含「孤儿毒化全部未来压缩」）与 F-02（触发判定对 4/5 协议族双计 cache_read，违反 R05 文档化消费者纪律，与现役触发语义系统性偏离，有量化探针证据）。两项均为确定性、可复现、要求修复并补回归测试。F-03/F-04 要求至少列入差异清单，F-05/F-06 随修复一并处理。

非阻断确认：执行者证据与声明一致性、工具链纪律、R05 零回归、A04 三腿证据、候选摘要，均经亲自查证成立。
