# R05-T07 独立审查报告（REV-T07 R01）

- 审查对象：R05-T07「用量、trace 及持久化」（任务书 §4-T07、附录 B T07 C01–C10、附录 A 之 A13/A14、附录 E 记录模板）
- 审查轮次：R01（首轮独立验收）
- 基线：分支 `codex/rust-tauri-migration`，HEAD `c549ff654508ab951e2cf39cf9d309fc9c6b8656`；R05-T01..T07 全部以未提交工作树交付（no_commit_push_authorization=true，本审查未执行任何 git 写操作）
- 审查日期：2026-10-03；审查者：未参与 R05 任何实现的独立会话
- 证据目录：`artifacts/rust-tauri/R05/REVIEW-T07/`（本审查者亲跑产生，与实现者证据 `artifacts/rust-tauri/R05/T07/` 物理分离）
- 工作树一致性：实现者 `working-tree-manifest.txt` 70 个文件 sha256 本审查者全量复算 **70/70 匹配**（`logs-manifest-check.txt`）——台账绑定的树即本审查者测试的树

## 总体判定：CONDITIONAL_GO（离线范围；3 个必修项 F-01/F-02/F-03，3 个登记建议 F-04/F-05/F-06）

T07 的主链路是真实且过硬的：三个定向套件（12+9+4=25 测试）本审查者全部亲跑复现绿；写序契约（台账先于 completed 事件、失败不发布假成功）、401 重试计 2 物理请求、Failed 调用也留行、owner 范围 JOIN 隔离、migration v5 隔离副本升级、幂等重放/冲突拒绝，均既有测试证据又有源码亲读支撑，未发现任何伪造证据或 PASS 空心化。

但本审查者的对抗探针发现**一个真实行为缺口（F-01）**：anthropic-messages 与 google-generative-ai 的**流式**路径会静默丢弃违反契约的 usage 片段（负数/浮点/字符串），最终事实被降级为 partial/unknown——最坏情形（先有效后非法的 message_delta）记出一行干净的 `reported`，违规痕迹完全消失。这与 `MODEL_USAGE_SEMANTICS.md` §4 自己声明的族无关规则（"行记 invalid + 违规细节"）和任务书 C06 的通过条件（"明确报错/标无效"）直接矛盾；openai 族同场景（对照腿）行为正确。另有两个**证据/文档真实性**必修项（F-02 estimated 覆盖声明不实、F-03 worker 台账失败腿引用了不存在的证据）。三者修复前 T07 不应记无条件 PASS。

LIVE 供应商验证维持任务书允许的 `BLOCKED_NOT_AUTHORIZED`（RR-BLK-CREDENTIALS，最迟 R10），判定限定为离线范围。

## 复核范围声明

- **亲自运行**：三个 T07 定向测试套件、审查者探针（6 组对抗腿，独立 crate，path-dep 现工作树源码）、70 文件工作树摘要全量复算、台账/证据文件逐条核对（python + grep）。
- **源码推断（亲读，未单独动态复现）**：`lingxi-kernel/src/usage.rs` 全文（UsageFolder/状态词表）、`models/usage.rs` 全文（公式表+严格解码）、anthropic/google/openai_completions 三族 accumulator 的 usage 折叠与 splice、`run_store.rs` 台账读写全段（2542-2883）、`migrations.rs` v5、`runs.rs` 驱动五类终局的记账路径（1390-1548、2625-2762、3014-3139）、`workermodel.rs`（60-314）、`operations.rs`（160-366 + 全部 record_operation_usage 调用点 19 处）、`lib.rs` bootstrap 接线（1187-1245）、`sessions.rs` 双 port 转发（1878/2475）、三份 T07 测试文件全文、`MODEL_USAGE_SEMANTICS.md` 全文。
- **未验证**：真实供应商 LIVE 流量（未授权）；五门禁（fmt/clippy/contracts/boundaries/全仓测试）未由本审查者重跑——编排者在本树已跑（`T07/gates-*.log`，全仓 105 组 ok / 1266 passed / 0 failed，r00 含 LAN 腿全绿、ALF 环境项未触发），其产物存在且与当前树一致（manifest 70/70）；Windows/Linux 平台行为。

## 亲跑证据（命令均真实执行，退出码亲见）

| 证据 | 命令（摘要） | 结果 |
| --- | --- | --- |
| adapters 套件 | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --test r05_t07_usage_families` | exit 0，**12 passed / 0 failed**，与实现者 `test-adapters-r05-t07-usage-families.log` 一致 |
| service 台账/轨迹套件 | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t07_usage_trace` | exit 0，**9 passed / 0 failed**，与实现者 log 一致（同一测试二进制指纹 5686b718…） |
| service 持久化套件 | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t07_persistence` | exit 0，**4 passed / 0 failed** |
| 审查者探针 | `cargo run`（`REVIEW-T07/probe/`，独立 crate，path-dep 当前工作树） | exit 0，P0–P6 七组腿，输出存 `probe-run.log`（F-01/F-02/F-04 的直接证据） |
| 工作树摘要复算 | python3 sha256 逐文件对照 `T07/working-tree-manifest.txt` | **70 matched / 0 mismatched**（`logs-manifest-check.txt`）；manifest 文件自身 sha256 = ae42a120…，与台账 workingTreeDigest 声明一致 |
| 台账核对 | python3 逐条读 `R05_ACCEPTANCE_LEDGER.json` 的 T07 C01–C10 + grep 测试名 | 10 条 PASS 的 testNames 全部真实存在于三份测试文件且被本审查者复跑；exitCode=0 与复跑一致 |

## 审查者探针（自写、隔离于 `REVIEW-T07/probe/`，不触碰产品代码）

独立 cargo crate，以 path 依赖引用**当前工作树**的 lingxi-adapters/kernel/protocol（编译即证明绑定当前源码），只调用生产公共入口（`MessagesStreamAccumulator` / `GenerateStreamAccumulator` / `ChatStreamAccumulator` / `RunDatabase`）：

- **P0 对照（探针有效性）**：合法 anthropic 流（message_start 25 + message_delta 9）→ `Known(25, 9, Reported)`；合法 google 流 → `Known(88, 19, Reported)`。探针驱动路径正确。
- **P1/P2（F-01 主体）**：anthropic 流 `message_delta.usage.output_tokens` 为 `-5` / `1.5` / `"9"` → 全部**静默丢弃**，成品为 `Partial{missing:["output_tokens"]}`；`message_start` 输入半边为 `-3` → `Unknown`。google 流 `usageMetadata` 同类违规 → 全部 `Unknown`。
- **P4（F-01 最坏情形）**：先合法 `output_tokens:9`、后非法 `-4` 的 anthropic 流 → 成品 `Known(25, 9, Reported)`——非法片段**无任何痕迹**。
- **P3（对照腿）**：同样的违规数字走 openai-completions 流式 → `Invalid("usage field /usage/prompt_tokens is negative (-3)…")`，字段与性质点名。openai 路径的行为（raw JSON 存 accumulator、finish 时统一严格解码）是正确的参照实现。
- **P5（F-02 佐证）**：手写 `Estimated{basis:"probe-basis"}` 行经真实 `RunDatabase` 写读 → basis 无损往返（**机械上成立**，但产品测试零覆盖，见 F-02）。
- **P6（F-04）**：FinalSnapshot 模式 fold 两份**数字相同、provenance 不同**（Reported→Estimated）的事实 → 报"conflict"，错误消息展示两组**完全相同**的快照 `(Some(10), Some(3)) then (Some(10), Some(3))`，与 `fold` 文档注释"provenance is IGNORED here"矛盾。当前调用方不可达（FinalSnapshot 族只在 finish 解码一次、从不多片段 fold），属潜在边角。

## 10 个检查点逐条核对

| C-ID | 结论 | 支撑（本审查者核实） |
| --- | --- | --- |
| C01 多轮会话轨迹连续 | PASS | `c01_…` 亲跑绿：同会话两 run 各 1 行、session 级查询 2 行有序、`{run}-mc0001` 宿主铸造调用号、attempt/provider/model/protocol 全关联字段在行上。源读 `runs.rs:3090-3117`（行构造）确认。 |
| C02 因果清楚 | PASS | `c02_…` 亲跑绿：main(origin=user, 无父)、background(独立根不误并)、subagent 子行 parent_run_id+cause_ref=父 tc0001、worker 回调行 origin=worker-callback + cause_ref=invocation。源读 `UsageLedgerContext::of_authorization`（runs.rs:264-276）：父子只来自 drive authorization 的 lineage 事实，非时间推断。 |
| C03 物理重试不吞计费 | PASS（登记说明） | `c03_refresh_…`（stub chat 命中 2、token 命中 1、1 行 transport_attempts=2、usage 取成功请求）与 `c03_retryable_…`（2 行 mc0001/mc0002，失败行 usage=None 非 0）亲跑绿。源读 `provider.rs:250-259`（refresh 重试标 2）与 `runs.rs:2663-2673`（Failed 调用先写行再开新 attempt）。**登记**：401-refresh 的两个物理请求合一行是设计选择（transport_attempts 计数披露），任务书"不同实际重试不能误合成一条省略成本"以"不省略"为界成立；run 级重试是两行。 |
| C04 累计/增量区分 | PASS | families 套件亲跑绿：anthropic 运行总量 3→9 替换不求和、回退响亮；gemini 运行快照替换；openai 终值重发幂等/不同响亮；responses/codex 单终值快照。源读两族 accumulator 的 `UsageFolder` 接线（anthropic:715-717/966-1001、google:692-743）与 splice（1099-1122 / 870-892）。kernel `usage::tests` 5 条随套件绿。 |
| C05 缺usage/中断保持未知 | PASS | `c05_…` 亲跑绿：unknown/partial/reported 三态在台账与 wire 事件双侧区分，wire `usage` 恰在非全知时为 null。`wire_record()` 只在两半已知时投影（usage.rs:100-108）源读确认。 |
| C06 非法计数不失真 | **有保留（F-01）** | 已验证腿真实：`c06_…`（openai 流式负数 → 行 invalid、wire null）与 families 的 c06（缓冲态负/浮/字符串/溢出 → Invalid 点名字段）亲跑绿；`strict_token` 源读确认负/浮/字符串/超 u64 全走 Invalid、无截断饱和。**未覆盖且行为相反**：anthropic/google 流式片段违规被静默丢弃（探针 P1/P2/P4），既不报错也不标 invalid——见 F-01，必修。台账 C06 dimensions 诚实地只声明了 openai-completions，但语义文档 §4 声明的是族无关规则。 |
| C07 缓存/推理口径 | PASS | families c07 四条亲跑绿：五族公式表逐字段断言；openai/gemini 分量含于总量不再加；anthropic cache 两类独立于 input。`USAGE_MAPPINGS`（models/usage.rs:61-135）与 `MODEL_USAGE_SEMANTICS.md` §2 表同源一致（人工逐行对照）。 |
| C08 费用不编造 | PASS | `c08_…` 亲跑绿：token 照记、`cost_basis=None`、schema 无 price/cost/amount 列（pragma 探针）。源读 `runs.rs:3114-3116`、`workermodel.rs:160-162`、`operations.rs:270-272` 三面均硬编码 None。 |
| C09 查询隔离/秘密 | PASS | `c09_…` 亲跑绿：他人 owner 行不跨域（owner JOIN，run_store.rs:2697-2716 源读确认 INNER JOIN 语义=不返回而非滤正文）；runs.db 整文件字节扫描无 API key。持久化腿（v4→v5 隔离副本保旧数据、换 provider config 重启后旧行原样新行追加、同 id 重放 no-op/改写 Conflict、三态 round-trip）4 测试亲跑绿。 |
| C10 写失败不假成功 | PASS（chat 面；worker 腿见 F-03） | `c10_a_…` 亲跑绿：注入 `record_model_call_usage` 失败 → 提交响亮 Err、订阅 mailbox 0 个 model_call_completed、台账 0 行；解除后新 run 恰 1 行；同行重放 Ok 行数不变。源读 `runs.rs:3052-3138`：ledger 提交 → completed 事件 record → publish 顺序，任一步失败即 `DriveError::Storage`。worker 腿实现正确（workermodel.rs:286-310，先记账再回话、失败=ProviderRefused）但**无故障注入测试**（F-03）。 |

## 发现清单

### F-01（必修，medium）：anthropic/google 流式路径静默丢弃违反契约的 usage 片段，与文档化 C06 语义相反

- 位置：`rust/crates/lingxi-adapters/src/models/anthropic_messages.rs:763-778`（message_start）、`:986-1001`（message_delta）、splice `:1099-1122`；`rust/crates/lingxi-adapters/src/models/google_generative_ai.rs:729-743`、splice `:870-892`。
- 问题：两处 `if let UsageDecode::Usage(fact) = decode_family_usage(...)` 只折叠合法事实，`UsageDecode::Invalid` 被静默跳过；finish 的 splice 只写**已折叠的可信数字**，所以缓冲终解析永远看不到违规数字。行内注释（anthropic:759-762"the finish splice re-parses it, so a contract-violating count marks the fact invalid there"；google:730-732 同义）**被探针证伪**。后果（探针 P1/P2/P4）：违规片段→成品 `partial`/`unknown`，先有效后非法→成品干净 `reported`，违规细节零痕迹。违反 `MODEL_USAGE_SEMANTICS.md` §4（族无关："行记 invalid + 违规细节"）与任务书 C06"明确报错/标无效"。
- 修复方向（任一，建议前者）：照 openai_completions 的成熟模式保留原始片段 JSON、finish 时统一严格解码（openai_completions.rs:819-828/1021-1022 即对照实现）；或在 accumulator 中记录 Invalid detail 并传导至最终事实。修后补 anthropic/google 流式违规腿测试（现有 families c06 只测 openai 缓冲态）。
- 严重度依据（medium 而非 high）：数字诚实性未破坏（无假 0、无截断/饱和），触发需供应商发送非法 usage 数字；但状态语义失真且与自身文档矛盾，属任务核心交付（五态口径）内的真实缺口。

### F-02（必修，low-medium）：语义文档声称 `estimated` 状态"被 round-trip 测试覆盖"——不存在该覆盖

- 位置：`docs/rust-tauri/R05/MODEL_USAGE_SEMANTICS.md:168-169`（§10）；对照 `rust/crates/lingxi-service/tests/r05_t07_persistence.rs:556-613`（round-trip 测试只有 partial/invalid/unknown 三态）。
- 问题：全仓 grep（service/adapters 全部测试 + kernel usage.rs）无任何测试构造 `UsageProvenance::Estimated`；文档声称的覆盖不存在。探针 P5 证明机械往返本身成立（真实 RunDatabase 写读 basis 无损），所以**实质无损、声明不实**——按诚实纪律，覆盖声明必须与实际测试一致。
- 修复：在 round-trip 测试补一条 Estimated 用例（一行成本），或把该句改为"序列化就绪，round-trip 由审查者探针验证、产品测试待补"。

### F-03（必修，low-medium）：台账 C10 的 worker 回调腿引用了不存在的证据（"见 C02 链路"）

- 位置：`R05_ACCEPTANCE_LEDGER.json` R05-T07-C10 observed（"worker回调同规则(台账失败=回调ProviderRefused, 见C02链路)"）；实现 `rust/crates/lingxi-service/src/workermodel.rs:286-310`。
- 问题：C02 测试走的是 `LedgerWorkerCallbackTrace` **成功**路径；全 service 测试中唯一的 trace double 是 `RecordingTrace`（r05_t06_worker_model.rs:269-287，恒返回 Ok）——没有任何测试注入失败的 `WorkerCallbackTracePort` 验证"台账失败=回调显式拒绝不回话"。任务书 C10 前置明确要求"发布边界注入失败"。实现源读正确（record 失败 `.map_err(ProviderRefused)?` 先于 `Ok(WorkerModelReply)`），但 PASS 条目的证据链在引用上是空的。
- 修复：补一个失败 trace double 的 worker 测试（与 c10 的 FailingLedgerPort 同型），或将 observed 改为如实声明"worker 腿实现已读、chat 腿注入验证"。

### F-04（建议，low）：`UsageFolder::fold` 的相等判定包含 provenance，与自身注释矛盾（潜在、当前不可达）

- 位置：`rust/crates/lingxi-kernel/src/usage.rs:212-221`。
- 问题：注释称"provenance is IGNORED here"，但 `current == fact` 是全字段相等——FinalSnapshot 模式下数字相同、provenance 不同（如 Reported→Estimated）会误报"conflict"，且错误消息展示两组相同快照、自相矛盾（探针 P6 实证）。当前调用方不可达（三个 FinalSnapshot 族只在 finish 解码一次），属潜在边角；若后续有调用方按片段 fold 混合 provenance 事实会踩中。
- 建议：相等比较排除 provenance 字段，或修注释。

### F-05（建议，low）：`decode_operation_usage` 非对象分支把供应商可控的任意 JSON 全文嵌入 `invalid_detail` 并持久化

- 位置：`rust/crates/lingxi-adapters/src/models/usage.rs:278-286`。
- 问题：`format!("operation usage is {usage} but must be an object…")` 将整个 usage JSON（可能是任意长字符串/数组）原样写入 detail，经 `record_model_call_usage` 落入 runs.db 的 `invalid_detail` 列并进入 `OperationFailure` 消息。与项目对供应商回显的既有纪律不一致（`dispatch.rs:352-359` `scrubbed_excerpt` 截 512 字符并抹除在用凭证材料）。对照：族解码器 `strict_token` 是纪律的（只打印类型名与数字值，不打印字符串内容）。
- 建议：对嵌入值做长度截断（如 512 字符）并复用 scrub 纪律；或只打印 `type_of(usage)`。

### F-06（建议，low）：操作面台账行零测试覆盖 + C09 台账 command 字段欠精确

- 位置：`docs/rust-tauri/R05/MODEL_USAGE_SEMANTICS.md:16-18`（§1 声明操作面"每次 dispatch 得到响应（或失败）后写一行"）；`rust/crates/lingxi-service/tests/r05_t06_operations.rs`（全文件 0 处 usage 引用）；`R05_ACCEPTANCE_LEDGER.json` R05-T07-C09 的 `command` 字段。
- 问题：(a) 操作面（embedding/rerank/…，`LedgerOperationUsageSink`，operations.rs:344-366 + 19 个调用点）行为仅源读成立，无任何执行测试——台账 C01–C10 未声称它，但语义文档 §1 把它写作已验证事实；(b) C09 条目 command 写 `--test r05_t07_usage_trace`，而其 5 个 testName 中 4 个实际在 `r05_t07_persistence`（evidence 数组两份日志都列了，实质无缺，记录欠精确）。
- 建议：为操作面 sink 补一条最小断言测试（任一 dispatch 后 `query_model_call_usage{owner:None}` 见 1 行）；修正 command 字段。

## 台账抽查与证据核对

- 10 条 T07 PASS 的 `testNames` 逐条 grep 核实存在于三份测试文件、且全部在本审查者复跑的套件内；`evidence` 路径全部存在于 `T07/`；exitCode=0 与复跑一致。
- A-ID 映射（C01/C02/C09→A13，C03–C08/C10→A14）合理且与附录 A 的"逐 C-ID 精确映射"要求一致。
- `evidence-window.txt` 声明的 12+9+4 通过数、五门禁、ALF 未触发、LIVE 未授权，与 `T07/` 内日志内容一致；`source-sha.txt`= c549ff654… 与 HEAD 一致；manifest sha256 ae42a120… 复算一致。
- 未发现伪造证据、虚构测试名或 PASS 空心化。问题集中在：一个真实行为缺口（F-01）与两处覆盖声明不实（F-02/F-03）。

## 反向测试声明

凡实现者标 PASS 的条目均先疑后验：C06 不满足于台账声明的 openai 范围，主动对五族×两模式做对抗探针，从而发现 F-01（anthropic/google 流式违规静默降级）；estimated"已覆盖"声明用全仓 grep 证伪（F-02）；worker 台账失败腿的"见 C02 链路"引用用 trace-double 全量枚举证伪（F-03）；fold 幂等声明用同数不同 provenance 探针找到注释矛盾（F-04）。对照腿（openai 同场景 Invalid、合法流 Reported）确保发现不是探针自身的伪影。

## 环境与阻塞登记

- `R05-ENV-ALF-UNSIGNED-TEST-BINARY`：本窗口编排者全仓日志显示 r00_management_leaves（含 LAN 腿）通过、未触发；本审查者未重跑 r00，以编排者日志为准。不据此放宽或修改任何门禁。
- `RR-BLK-CREDENTIALS`（LIVE 供应商验证未授权，最迟 R10）：维持 BLOCKED_NOT_AUTHORIZED，不计入 FAIL。
