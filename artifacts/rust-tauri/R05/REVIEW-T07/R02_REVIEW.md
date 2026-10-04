# R05-T07 独立审查报告（REV-T07 R02 — 修复轮复审）

- 审查对象：R05-T07「用量、trace 及持久化」修复轮（R01 verdict CONDITIONAL_GO 的 F-01/F-02/F-03 三条 mustFix 的关闭验证）
- 审查轮次：R02（第 2 轮独立验收；复审重点：根因关闭而非表面修补、同类路径一并处理、修复无回归）
- 基线：分支 `codex/rust-tauri-migration`，HEAD `c549ff654508ab951e2cf39cf9d309fc9c6b8656`；全部 R05 工作仍是未提交工作树（本审查未执行任何 git 写操作）
- 审查日期：2026-10-03；审查者：R01 同一独立会话（未参与实现）
- 证据目录：`artifacts/rust-tauri/R05/REVIEW-T07/`（本审查者亲跑产物：`probe-rerun-r02.log`、`logs-fix-r1-file-hashes.txt`、`logs-manifest-check.txt`（R01 遗留））；实现者修复证据：`artifacts/rust-tauri/R05/T07/fix-r1/`
- 树绑定：R01 的 70 文件 manifest 复算 **67 unchanged / 3 changed**——changed 恰为三个测试文件（families/worker_model/persistence）；修复另触及 manifest 未覆盖的两个未跟踪源文件（anthropic_messages.rs、google_generative_ai.rs，整个 models/ 目录本就不在 R01 manifest 内）。本审查者对 5 个修复触及文件 + 语义文档现值另行 sha256 登记（`logs-fix-r1-file-hashes.txt`）。

## 总体判定：PASS（离线范围）

三条 mustFix 全部**按根因关闭**，且经本审查者独立执行验证（非采信实现者日志）：

1. **F-01（关闭）**：修复与 openai raw-splice 参照同构——违规片段的**原始 JSON** 进粘性 `usage_violation` 槽（首违规获胜），finish splice 将其并回缓冲体，由**同一严格解码器**在终点把整个事实标 `Invalid` 并点名字段与性质。不是"补个错误标志"的表面修补：违规数字真正流入了一次统一解码。同类路径我独立审计（全仓 8 个生产消费点逐一亲读 + 终局 grep 无残留 `if let UsageDecode::Usage` 模式），确认缺陷仅原来两处、其余本就正确。R01 探针原样复跑于修复树：P1/P2/P4 全部 `Invalid` 点名字段、P0 合法流仍 `Reported`（无误报）；**新增三条修复轮对抗腿 R02-A/B/C**（先违规后合法、双违规、google 先违规后合法终帧）全部 `Invalid`——粘性槽在后续合法折叠下不丢失。9 条流式违规腿已固化为产品测试（含 R01 发现的最坏情形）。
2. **F-02（关闭）**：round-trip 测试补 `Estimated{basis:"chars/4"}` 用例，四态（partial/invalid/unknown/estimated）全记录 `assert_eq!` 无损往返——文档 §10 的覆盖声明现在为真（文档本身无需改写，亲核 §4/§10 现文与实现一致）。
3. **F-03（关闭）**：空引用的"见C02链路"由真实注入测试取代——`FailingTrace`（恒 Err 的 `WorkerCallbackTracePort`）挂在**真实链**上（config plane→gateway→credentials→aux executor→GatewayWorkerModel→BoundedWorkerModel→真实 worker 子进程），注入点正是发布边界（provider 腿成功后），断言 ok=false / `model_provider_refused` / message 点名 `usage ledger` / stub 恰 1 次物理请求。台账 C10 条目如实改写（含自认原引用不实）。

回归：我亲跑 8 组共 **177 个测试全绿**（见下表），覆盖修复直接面（families/persistence/worker/usage_trace）+ 受影响适配器邻面（goldens/streaming/protocol_adapters/adapters lib 109 单测）+ r04_t08（修复者报告的唯一首跑抖动腿所在套件，我单跑 10/10 绿，佐证"负载抖动非回归"）。修复触及面经 mtime + manifest 差分核实**恰为 5 个 rust 文件**（2 源 + 3 测试）+ 2 台账，无越权改动。

R01 的三条建议项（F-04/F-05/F-06，均 mustFix=false）按实现者声明未修、已在 `PROGRESS_LEDGER.fix_rounds[0].not_fixed_registered` 登记——与本轮复核所见一致（F-04 探针 P6 仍现、C09 command 字段未改、操作面仍零 usage 测试）。它们随本报告继续携带，不阻塞 PASS。

## 复核范围声明

- **亲自运行**：四套定向 + 四组回归 + adapters lib 单测 + R01 探针复跑（含 3 条新增对抗腿）+ manifest/hash 差分 + 台账核对。
- **源码推断（亲读）**：anthropic_messages.rs 修复段全文（759-794、998-1025、1123-1158）、google_generative_ai.rs 修复段全文（737-758、885-922）、openai_responses.rs ResponsesStreamAccumulator 全文（682-775，终端对象 raw 重析——同类路径审计）与 codex 复用点（openai_codex_responses.rs:221）、operations.rs 两处 decode_operation_usage 消费点（570/626 三分支）、三份测试的新增段、两台账 fix_rounds/C06/C09/C10 条目、语义文档 §4/§10 现文。
- **未验证**：LIVE 供应商流量（未授权）；五门禁与全仓 1269 测试未由本审查者整跑——编排者在本树已复跑五门禁全 PASS（ask 所述），修复者 fix-r1 的全仓复跑日志聚合数（1269 passed / 0 failed / 105 组 ok）我已程序化核对一致；r04_t08 我单跑核实。

## 亲跑证据（命令均真实执行，退出码亲见）

| 证据 | 命令（摘要） | 结果 |
| --- | --- | --- |
| families（含 2 条新增流式违规测试） | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --test r05_t07_usage_families` | exit 0，**14 passed / 0 failed**（R01 时 12） |
| persistence（含 Estimated 用例） | `… -p lingxi-service --test r05_t07_persistence` | exit 0，**4 passed / 0 failed** |
| worker model（含 c10 FailingTrace 新测试） | `… -p lingxi-service --test r05_t06_worker_model` | exit 0，**10 passed / 0 failed**（R01 时 9） |
| usage trace（回归） | `… -p lingxi-service --test r05_t07_usage_trace` | exit 0，**9 passed / 0 failed** |
| R01 探针复跑（+3 新腿） | `cargo run`（`REVIEW-T07/probe/`，同一探针源码在修复树重编译运行） | exit 0，`probe-rerun-r02.log`：P1/P2/P4→Invalid 点名字段、P0→Reported（无误报）、P3/P5/P6 不变、**R02-A/B/C→Invalid** |
| 适配器邻面回归 | `-p lingxi-adapters --test r05_t03_goldens --test r05_t04_streaming`；`-p lingxi-service --test r05_t03_protocol_adapters --test r05_t04_streaming` | exit 0：3+18+10+9 全绿 |
| adapters lib 单测 | `-p lingxi-adapters --lib` | exit 0，**109 passed** |
| r04_t08（抖动腿所在套件） | `-p lingxi-service --test r04_t08_tool_matrix` | exit 0，**10 passed**（300.48s） |
| 树差分 | python3 复算 R01 70 文件 manifest + mtime 扫描 + 5 文件 sha256 | 67 unchanged / 3 changed（均为测试文件）；修复窗口触及 `rust/crates` 下恰 5 文件（`logs-fix-r1-file-hashes.txt`） |
| 实现者证据核对 | grep/程序化聚合 fix-r1 各日志 | families 14、trio 10+4+9、workspace 首跑唯一 FAILED=`terminal_family_share_cases`、复跑聚合 **1269 passed / 0 failed / 105 组 ok**；其 `probe-verification/src/main.rs` 与我的 R01 探针源**逐字节一致**（diff 为空）——其 probe-rerun-on-fix.log 由未改动探针产出 |

## 逐条修复验证

### F-01｜anthropic/google 流式违规 usage 片段静默丢弃 → 已按根因关闭

- **根因验证（源码亲读）**：两族 accumulator 各新增 `usage_violation: Option<serde_json::Value>`（anthropic_messages.rs:706、google_generative_ai.rs:683）；message_start/message_delta/usageMetadata 的解码改为三分支 match（anthropic:776-793、1004-1024；google:742-757）——`Usage`→折叠、`Invalid`→`get_or_insert_with` 保留违规片段**原始 JSON**、`Absent`→无操作；finish 的 splice 把违规字段逐键并入回写体（anthropic:1148-1154、google:908-916），随后 `parse_*_response` 经 `decode_family_usage` 对合并体做**一次统一严格解码**——与 openai raw-splice 参照（openai_completions.rs:819-828/1021-1022）同构。被 R01 证伪的行内注释已改写为真实设计描述。
- **粘性语义检查**：首违规获胜 + 后续合法折叠照常进行——拼接时违规原始字段覆盖折叠值，终点解码必为 `Invalid`（探针 R02-A/B/C 三腿实证：先违规后合法、双违规、google 先违规后合法终帧，全部 `Invalid` 且无一退化为干净 reported/partial/unknown）。
- **同类路径独立审计**：全仓 grep `decode_family_usage|decode_operation_usage` 共 8 个生产消费点，逐一亲读——openai_completions:607 / openai_responses:409 / anthropic:475 / google:448（四个缓冲态三分支）、anthropic:776,1004 + google:742（本轮修复的两个流式点）、operations.rs:570,626（操作面三分支）；openai_responses/codex 的流式为终端对象 raw 保留重析（ResponsesStreamAccumulator:682-775，codex 复用之）无此缺陷。终局 grep 无残留 `if let … UsageDecode::Usage` 模式。**结论：缺陷仅原来两处，已全数关闭，无第三处。**
- **产品测试钉死**：families 新增 `c06_anthropic_streaming_violating_usage_fragments_mark_the_fact_invalid`（5 腿，含最坏情形先合法后违规）与 `c06_gemini_streaming_violating_usage_metadata_marks_the_fact_invalid`（4 腿，含同型最坏情形），断言 `ReportedUsage::Invalid` + detail 点名字段与性质——与我的探针腿一一对应且我亲跑绿。
- **边界行为核验**：`usage_seen` 收紧为 usage 对象出现即置位——只影响重复 message_start 守卫（本就非法），合法流无行为变化（P0 + 109 lib 单测 + 全部流式回归绿）。

### F-02｜estimated 覆盖声明不实 → 已关闭

round-trip 测试新增 `Estimated{basis:"chars/4"}` 记录，四态全量 `assert_eq!`（PartialEq 含 provenance 与全部数字）无损往返（r05_t07_persistence.rs:597-626，亲读+亲跑绿）。文档 §10 原句"字段与序列化已就位并被 round-trip 测试覆盖"现在为真；文档文件本未改（mtime 核实），语义也无须改。台账 C09 observed 已同步"四态 round-trip 无损（fix-r1 补 estimated 用例）"。

### F-03｜worker 台账失败腿空引用 → 已关闭（真注入测试）

`FailingTrace`（r05_t06_worker_model.rs:298-317，恒 `StorageError::Io`）；`real_worker_model` 签名泛化为 `Arc<dyn WorkerCallbackTracePort>`（323-369，其余 10 个调用点仅类型适配）；新测试 `c10_a_failing_usage_trace_refuses_the_callback_reply_never_answers_unaccounted`（653-686）在真实链上注入于**发布边界**（provider 腿先成功），断言 worker 收到 `ok=false` + `model_provider_refused` + message 含 `usage ledger` + stub 恰 1 次物理请求——恰是 C10"不存在成功事件先于关键提交"在 worker 面的正确形态。台账 C10 的 command/testNames/evidence/observed 已如实更新并自认原引用不实。亲跑绿。

## 残留与携带项（均非阻塞）

- **F-04（low，携带）**：`UsageFolder::fold` 相等判定含 provenance 的注释矛盾仍在（探针 P6 复现）；当前调用方不可达。已在 fix_rounds.not_fixed_registered 登记。
- **F-05（low，携带）**：`decode_operation_usage` 非对象分支仍将供应商可控 JSON 全文嵌入 invalid_detail（未截断/未 scrub）。已登记。
- **F-06（low，携带）**：操作面台账行仍零测试覆盖；C09 条目 command 字段仍只写 `--test r05_t07_usage_trace` 而 4/5 testName 在 persistence 套件（本轮复核再确认未改）。已登记。
- **新观察（本轮，low，登记不阻塞）**：fix-r1 证据目录无 per-file 工作树 manifest，且 R01 的 70 文件 manifest 本就不含 models/ 目录五个族文件——修复轮对源文件的证据绑定弱于 R01 惯例（本审查者以 `logs-fix-r1-file-hashes.txt` 补上现值绑定；实质验证由亲跑覆盖，不影响结论）。

## 反向验证声明

- 不采信实现者探针日志：其 `probe-verification` 与我的 R01 探针 diff 为空（证明跑的是我的原码），但我仍在修复树**自行重编译运行**同一探针并新增 3 条修复轮对抗腿（针对粘性槽与折叠交互的镜像场景）——修复面在实现者未测的组合上同样成立。
- 不满足于报错消失：核对修复是"违规数字真正流入统一解码"（raw 保留 + splice + 单次严格解码）而非仅加 Invalid 标志；同类 8 消费点逐一亲读排除第三处；`if let UsageDecode::Usage` 模式全仓归零。
- 不满足于定向绿：回归网覆盖适配器邻面（goldens/streaming/protocol/lib 109）与修复者报告的抖动套件 r04_t08（我单跑 10/10）。
- 触及面核查：mtime + manifest 差分确认修复未越权改动其他产品文件。

## 环境与阻塞登记

- `R05-ENV-ALF-UNSIGNED-TEST-BINARY`：修复者全仓复跑（fix-r1 rerun 日志）r00 LAN 腿全绿未触发；本审查者未重跑 r00，以日志为准，不放宽不改门禁。
- `RR-BLK-CREDENTIALS`（LIVE 供应商验证未授权，最迟 R10）：维持 BLOCKED_NOT_AUTHORIZED，不计入 FAIL。
