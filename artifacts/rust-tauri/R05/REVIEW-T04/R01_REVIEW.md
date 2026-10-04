# R05-T04 独立审查报告（REV-T04 R01）

- 审查对象：R05-T04「流式解码、规范化与部分结果」（任务书 §4-T04、附录 B C01–C16、附录 A 之 A07/A08/A09、附录 E 记录模板）
- 审查轮次：R01（首轮独立验收）
- 基线：分支 `codex/rust-tauri-migration`，HEAD `c549ff654508ab951e2cf39cf9d309fc9c6b8656`；全部 R05 改动为未提交工作树 diff
- 审查日期：2026-10-03；审查者：未参与 R05 任何实现的独立会话
- 证据目录：`artifacts/rust-tauri/R05/REVIEW-T04/`（本审查者亲跑产生，与实现者证据 `artifacts/rust-tauri/R05/T04/` 物理分离）

## 总体判定：GO（离线范围；附 1 个轻量文档必修项 F-01，不阻塞、不需重验代码）

16 个检查点全部 PASS，均有运行路径上的真实支撑；无一依赖台账自述。两个最高风险项（C12 反假流式、C01 任意分片等价）除复跑实现者证据外，另由审查者自写探针独立亲验（自有 PRNG/seed、自有导线级屏障服务器），结论与实现者一致。附录 D 要求的「PENDING 优先于放行」未被触发：无 PENDING 项；LIVE 供应商验证维持任务书允许的 `BLOCKED_NOT_AUTHORIZED`，故判定限定为离线范围。

## 复核范围声明

- 亲自运行：静态门禁 4 项、聚焦测试 13 套件 134 个用例、审查者探针 2 个（见「亲跑证据」）。
- 源码推断（亲读，未单独动态复现）：D6 持久化顺序、批次准入内部状态机、401 刷新重放的 sink 安全性、ReservedTagScanner 与 TS 源的逐段对应。
- 未验证：真实供应商 LIVE 流量（未授权，维持 BLOCKED_NOT_AUTHORIZED）；全 workspace 套件未由本审查者重跑（实现者已跑 1142 passed / 0 failed，见其 `gates-test-workspace-nofailfast.log`；本审查者以 13 个聚焦套件 + 探针覆盖 T04 及其回归面）。

## 亲跑证据（命令均真实执行，退出码亲见）

| 证据 | 命令（摘要） | 结果 |
| --- | --- | --- |
| `gates-fmt.log` | `cargo fmt --all -- --check` | exit 0（本文件为本审查者重跑所留；通过时无 diff 输出属正常） |
| `gates-clippy.log` | `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0。**如实注明：本次为全缓存命中**（0.19s Finished，无重新编译）；cargo 指纹保证产物对应当前源码，但非从零编译 |
| `gates-check-contracts.log` | `cargo run -p xtask -- check-contracts` | exit 0（drift-free） |
| `gates-check-boundaries.log` | `cargo run -p xtask -- check-boundaries` | exit 0（ownership + 依赖规则 + 负向电池 OK） |
| `test-suites.log` | 13 个聚焦套件（见下） | 全部 ok，0 failed |
| `probe-run.log` + `zz_rev_t04_probe.rs` | `cargo test -p lingxi-adapters --test zz_rev_t04_probe` | 2/2 ok |

13 个聚焦套件明细（passed）：adapters `r05_t04_streaming` 18、service `r05_t04_streaming` 9、`r05_t01_model_plane` 7、`r05_t01_binary_wiring` 2、`r05_t02_credentials` 16、`r05_t02_oauth_flows` 20、`r05_t03_protocol_adapters` 10、`r05_t03_goldens` 3、`cancel_terminal_race` 13、`late_result_fence` 5、`cancellation_tree` 8、service lib `streaming_norm` 14、adapters lib `streaming` 9。合计 134。

### 审查者探针（自写、跑后即删、源码与输出归档于本目录）

探针文件为 `zz_rev_t04_probe.rs`，运行期间位于 `rust/crates/lingxi-adapters/tests/`，跑绿后已从工作树删除（`git status` 无残留），源码与 `probe-run.log` 归档在本目录。探针在树期间未运行任何 clippy 门禁，不污染实现者门禁证据。

- Probe A（C01，评审者种子）：同一 fixture（anthropic 全要素 / openai 双语种双工具交错），一次性解码为参照，用**审查者自己的 SplitMix64**（非实现者 xorshift）与**审查者自选两个 seed**（`0x5EED2026_1003_0001`、`0xBADA55DE_ADBEEF99`）各随机切 300 轮，断言 delta 序列、闭合批次（turn+usage）逐字节相等。结果：通过。证明分片等价性不是实现者 seed 选择的假象。
- Probe B（C12，导线级）：自写 raw-TCP 门控 SSE 服务器——发 200 + 首帧后**停在屏障**，结构性地扣住后续帧。生产路径 `dispatch::drive_sse_stream`（真实 reqwest 增量读）上的 `FamilyStreamDrive` 在屏障释放**之前**把首 delta 送达 sink（`tokio::select!` biased 第一分支：若 drive 在首 delta 前结束即 panic——未触发），且首 delta 内容为屏障前帧；释放后余片按序到达、`drive_sse_stream` 返回 Ok。结果：通过。在真实网络读取层排除「全量缓冲后切片冒充 streaming」。

## 16 个检查点逐条核对

| C-ID | 结论 | 运行路径上的支撑（本审查者核实） |
| --- | --- | --- |
| C01 任意字节分片等价 | PASS | 套件 3 测试（逐字节/全二切点/固定 seed 随机 200 轮 × 两协议）亲跑绿；评审者探针 A 独立种子复验绿；A07 映射精确 |
| C02 非法 UTF-8 处理确定 | PASS | `c02_invalid_utf8_is_deterministic_at_every_fragmentation`：每种分片下同一 InvalidMessage（响亮不可重试），已交付前缀完好，后续缓冲不泄漏 |
| C03 SSE 与协议帧组合 | PASS | CRLF、注释心跳、多 data 行、多帧一包、单帧多包夹具；心跳不当文本、传输包不当消息边界 |
| C04 缓冲及解析成本有界 | PASS | `SSE_BUFFER_LIMIT=8MiB`、`SSE_FRAME_MAX_BYTES=1MiB`、`TOOL_ARGUMENTS_MAX_BYTES=1MiB` 与 `R05_BASELINE.json` 预登记值核对一致（buffer 上限 8MiB 比预登记 16MiB 更紧，属任务书 §8 允许的收紧，非实现后放宽）；超限响亮拒绝并弃流 |
| C05 半截 JSON 不得执行 | PASS | 工具片段永不进 live delta；`finish` 复用 buffered 解析器，半 JSON 在闭合时响亮报错、零派发（调用计数=0）。空 `partial_json`→`{}` 的裁定见「裁定说明」 |
| C06 可解析 JSON 不等于调用完成 | PASS | 批次准入仅在协议终结条件（openai `[DONE]` / anthropic `message_stop`）后发生；可解析但未闭合不执行 |
| C07 错误参数类型与歧义 | PASS | null/数组/错误类型/重复键/超精度数按冻结 schema/canonical 规则一致处理；无静默转型 |
| C08 交错工具增量不串块 | PASS | 双工具 index 交错夹具（两协议）：按 index/id 独立累积，无全局单缓冲 |
| C09 完整批次与截断批次 | PASS | openai `length`、anthropic `max_tokens`：一轮内一个工具完整另一个被截断时整轮零派发；service 侧重试干净（无半批执行后伪称全未执行） |
| C10 重复与冲突事件可区分 | PASS | 重复完成事件不重复派发；同 ID 不同参数冲突响亮拒绝；相邻相同文本 delta 不按内容去重 |
| C11 停止原因语义完整 | PASS | stop/tool_use/length/refusal/协议错误/EOF 分别映射内部 outcome；只有可信正常终结形成 final |
| C12 确实流式而非事后分片 | PASS | 实现者 service 级 `c12_…`（GatedSse 屏障）亲跑绿；评审者探针 B 在导线级独立复验绿。`TURN_STREAM_LIVE_DELTA_CAP=1024` 为背压上限而非缓冲重切。durable 序 `model_call_started`（runs.rs:996）< deltas < `normalizer.finish()` 的 segment_end（runs.rs:1208）< `model_call_completed`（runs.rs:1237/1248）亲读确认，且源码注释（1207）钉住该顺序 |
| C13 MOOD/思考与普通文本不误伤 | PASS（附观察项 N-01） | `ReservedTagScanner` 逐段核对确为 `shared/reserved-tag-stream.ts` 的移植（常量 16*1024/128、openTag/unknownStack/code 三态、dropUnknownOrphanClosers 一致；UTF-16→UTF-8 索引论证在模块头注释成立：结构字符全 ASCII）；字面标签（代码示例中的 `<think>`）不误删；实时与历史经同一 `split_reserved_tag_segments`，同源性在函数级成立 |
| C14 未知块与签名隔离 | PASS | 非关键未知块按规则保留/忽略并记录、必需未知结构响亮；`signature_delta` 累积进 thinking 块签名、不露正文 |
| C15 HTTP200 后的流错误 | PASS | 五腿（错误事件/断流/重复终止/只 usage 无正文/decoder 级失败）亲跑绿；A08 映射精确；错误不被 200 盖掉，partial 保留、usage 未知性保留，取消臂不写 completed，底层连接取消 |
| C16 取消与 EOF 竞态 | PASS | `c16_a09_…` + `cancel_terminal_race` 两竞态测试亲跑绿；A09 映射精确；唯一终态、迟到 delta/final 受 fence、不跨 Run 写入 |

## §5 统一验收方法对照

- 四层验收：T04 落在纯逻辑/性质层（decoder/accumulator 为字节流纯函数）与协议/真实服务层（真实 RunSupervisor/Storage + loopback TCP 协议替身）；两层均有真实执行，未以逻辑层冒充服务层。
- 协议覆盖矩阵：成功、工具往返、流式、错误/截断、取消、usage 在 openai-completions 与 anthropic-messages 两族均有用例；google/responses 两族共用同一 decoder 与闭合批次解析器（台账 dimensions 已声明该映射，本审查者抽查共享路径属实）。
- 确定性：屏障（GatedSse/Notify/探针 raw-TCP barrier）+ 固定 seed，无「sleep 够久」式时序断言。
- 台账字段：抽查条目含 caseId/A-ID/scope/命令/testNames/exitCode/sourceSha/工作树摘要/runnerDigest/证据/mockBoundary，符合附录 E 最低字段。

## 既有测试演进复核（5 个，逐个 diff）

1. `cancellation_tree.rs`（tracked）：stream_read 期望序列新增 `model_call_started` 事件。判定：**等价演进**——这是 D6 要求的诚实新事实（调用先于 delta 登记），原有取消语义断言全部保留，非放宽。
2. `late_result_fence.rs`（tracked）：mc0002 零写入断言改为 `event_type <> 'model_call_started'` 精确排除。判定：**合理**——attempt2 的调用真实发生，`model_call_started` 是唯一合法新增事实；fence 的其余零写入断言（无 delta/completed/工具副作用）原样保留。这是任务书点名项，未发现放松。
3. `r05_t01_model_plane.rs` / `r05_t01_binary_wiring.rs` / `r05_t02_credentials.rs`（T01/T02 新建未跟踪文件）：stub 由非流式应答改为 SSE（`Content-Type: text/event-stream` + `[DONE]`），并断言请求体 `stream == true`。判定：**适配性演进**——execute_chat 委托 execute_chat_stream 后的必然同步，断言强度未降（请求形状检查反而更严）。

## 台账抽查（6/16 条）

C01/C04/C09/C12/C15/C16：全部 `status=PASS`、`exitCode=0`；`testNames` 逐一 grep 核实真实存在于对应测试文件（adapters 3 条、service 3 条）；`evidence` 路径全部存在于 `artifacts/rust-tauri/R05/T04/`；A-ID 映射 A07→C01、A08→C15、A09→C16 与附录 A 语义精确对应；mockBoundary 声明一致（loopback TCP stub / NOT_REAL_API）。未发现台账与运行路径脱节。

## 发现清单

### F-01（轻，文档必修，不阻塞放行；纯文档修正，无需重验代码）

`docs/rust-tauri/R05/R05_INTERFACE_EVOLUTION.md:414`（§21）声称 `ModelTurnDelta` 有 4 个变体（Text / Reasoning / ToolCallFragment / Opaque），但 `rust/crates/lingxi-kernel/src/ports.rs:1021-1031` 实际只定义 `Text` / `Reasoning` 两个变体。后果：后续阶段（T05–T08）若按文档假设 ToolCallFragment/Opaque 变体存在，会产生接口误设。根因：接口演进文档未随实现收敛回写。影响面：仅该文档 §21；代码、测试、台账均以两变体实际定义为准，运行路径无缺陷。修复：将 §21 改述为实际两变体设计（工具片段不进 live delta 是经 accumulator 批次准入实现的，文档应说明这一点而非虚构变体）。

### N-01（观察项，归 T08，非本阶段缺陷）

`split_reserved_tag_segments`（`rust/crates/lingxi-service/src/streaming_norm.rs:925`）在 Rust 侧当前无生产消费者：引用者仅模块内自测（:1225）与 c13 测试。核查 `sessions.rs` 无 history/messages 读取路径——Rust 侧历史读取面尚不存在，属 T08 范围。C13 的「实时与历史投影同源」在函数级证明成立（同一函数同时服务两条路径的设计已被测试钉住），但「历史面真实消费该函数」要等 T08 落地后方可端到端复验。建议 T08 验收时把本项列为必查。

## 裁定说明（非发现，显式记录裁断理由）

anthropic `finish` 路径把空 `partial_json` 视为 `"{}"`（`anthropic_messages.rs` 约 :1003-1007）。裁定：**协议正确，不构成 C05 的「空对象占位当真实参数」违规**。理由：(1) 该分支只在 `content_block_stop` 已到达、协议已宣布该工具块闭合之后执行——批次准入条件已满足，不存在「半截 JSON 补括号猜测」；(2) 与 Anthropic 官方 SDK 对流式 `input_json_delta` 零片段的处理一致；(3) 与同文件 buffered（非流式）解析器行为一致，`finish` 复用同一解析器，流式未引入第二套语义；(4) C05 禁止的是「协议未宣布完成时把占位当参数派发」，而此处派发前提是协议完整闭合。

## Mock 边界声明

本审查全部在离线范围执行：唯一替身是 loopback TCP 协议 stub（实现者 GatedSse 与审查者探针的 raw-TCP 门控服务器），RunSupervisor、Storage、持久化、规范化桥均为真实生产组件；无任何真实供应商密钥、账户或流量（NOT_REAL_API）。取消竞态由固定调度与屏障确定化，未依赖 sleep。

## 已知边界

- clippy 门禁为全缓存命中运行（见亲跑证据表）；fmt/contracts/boundaries 为真实执行。
- 全 workspace 套件（实现者 1142 passed / 0 failed）本审查者未重跑；T04 及其回归面已由 134 个聚焦用例 + 2 个探针覆盖。
- `r00_management_leaves` 的 LAN 段在本机 ALF 下可能 stall——任务书允许的唯一 ALF 环境项，与 R05-T04 断言无关，本次未触碰。
- LIVE 供应商验证维持 `BLOCKED_NOT_AUTHORIZED`；本判定的有效性边界为离线四层中的适用层。
