# Lingxi 目标协议规格（PROTOCOL_SPEC v1）

- 任务：R01-T02（定义单一协议源及兼容边界）
- 权威源：`rust/crates/lingxi-protocol/` 的 serde 数据类型。本文件是这些类型的人读契约说明；
  字段级定义以源码与生成的 JSON Schema 为准，三者不一致时以 Rust 源码为准并修复其他两者。
- 生成物（禁止手改，`--check` 无 diff 为准）：
  - `contracts/generated/jsonschema/<Type>.schema.json` — JSON Schema draft 2020-12
  - `contracts/generated/ts/lingxi-protocol.ts` — TypeScript 绑定
  - `contracts/generated/golden/*.json` — 跨语言序列化 golden（规范 JSON 字节）
  - `contracts/generated/MANIFEST.json` — sha256 清单
- 重新生成 / 校验：
  ```
  cargo run -p lingxi-protocol --bin lingxi-protocol-gen            # 生成
  cargo run -p lingxi-protocol --bin lingxi-protocol-gen -- --check # CI 门禁:无 diff
  node scripts/rust-tauri/r01-t02-extract-api-surface.mjs --check   # API 兼容矩阵门禁
  ```
  （`scripts/rust-tauri/r01-t02-check-generated.sh` 顺序执行两者。）

## 1. 范围

本规格固定 `lingxi.wire` v1 的：身份与 ID、事件信封与已知事件词汇、错误结构、
分页游标、资源引用、参数摘要、版本协商握手、未知内容处理策略、规范 JSON 序列化。

不在本规格内（后续阶段）：存储与 epoch 迁移策略（R01-T07）、工具目录与执行管线
（R04）、模型 provider 适配（R05）、宿主原生能力（R09）。本阶段不接入任何生产入口：
`desktop/ server/ core/ lib/ shared/` 零改动；TS 消费端与握手原型均为隔离原型。

## 2. 版本轴与现役 contract-versions.json 的关系

现役 `shared/contract-versions.json`（R00 基线冻结，本任务不修改）：

| 轴 | 现役值 | 含义 | 新协议下的处置 |
|---|---|---|---|
| PRELOAD_API_VERSION | 1 | window.hana 宿主桥版本 | 冻结。宿主桥是 native_host 映射（R09 Tauri 化），不进 `lingxi.wire`；其版本轴由 R09 另行管理 |
| SERVER_PROTOCOL_VERSION | 1 | 现役 HTTP/WS 业务 API 版本 | 冻结。迁移期由兼容层保留（API_COMPAT_MATRIX 逐项 retain），不升级不回退 |
| DATA_EPOCH | 1 | 数据格式纪元 | 冻结。迁移/拒写策略归 R01-T07；握手响应中的 `dataEpoch` 仅如实报告，不在本阶段协商 |

新协议版本是**独立轴** `lingxi.wire`，本规格固定 v1（`WIRE_PROTOCOL_MIN_SUPPORTED =
WIRE_PROTOCOL_MAX_SUPPORTED = 1`）。协议族名错误、范围不相交都不是"按 v1 猜着继续"的理由；
见 §7。演进规则：新增事件类型/可选字段需要 `schemaVersion` 或 wire 版本提升（见 §8）；
破坏既有字段语义必须提升 wire 版本。

## 3. 身份与 ID（02 §3）

- 一切 ID（`SessionId/RunId/AttemptId/ModelCallId/ToolCallId/ResourceId/StreamId/EventId`）
  是**不透明字符串**，serde transparent 序列化为裸 JSON 字符串；跨迁移原样保留，永不重新编号。
- `Seq`（流内单调序号）与一切 u64 量（token 数、字节数、时间戳、generation）在 wire 上是
  **十进制字符串**；解析只接受字符串形式（裸 JSON 数字被拒），JS 侧不得以 Number 承载
  （golden `event-assistant-segment-delta.json` 证明 2^53 之外不丢精度）。
- 所有异步结果必须携带并核验 owner/run/attempt/generation：事件信封（§5）携带
  `runId`/`attempt`；模型请求（`ModelRequest`）固定携带 principal/run/attempt/modelCall/
  purpose/provider/model/operation/budget/configGeneration。

## 4. 错误结构

`ProtocolError`（封闭结构）：`code`（`ErrorCode` 枚举，snake_case wire 名）、`message`
（人读）、`retryable`（建议性）、`details`（可选，机读诊断扩展点）。错误扩展只进
`details`，envelope 本身不加字段。关键码：`invalid_message`、`version_incompatible`、
`unauthorized`、`forbidden`、`not_found`、`conflict`、`cancelled`、`budget_exceeded`、
`upstream_unavailable`、`cursor_expired`（游标过旧，必须重取快照）、
`unknown_schema_dialect`（未知 schema 方言，禁止宽松放行）、`internal`。

## 5. 事件信封与事件词汇（02 §7）

`EventEnvelope`（封闭结构，camelCase 字段）：`schemaVersion`（本规格=1）、`eventId`、
`streamId`、`seq`、`sessionId`、`runId?`、`attempt?`、`eventType`、`payload`。

- `eventType` 是 payload 判别标签的冗余副本，供不解码 payload 的路由使用；反序列化
  **校验两者一致**，不一致为 `invalid_message`，不猜哪个为准。
- `schemaVersion != 1` 为硬错误（不支持即拒绝，不降版本解析）。
- 已知事件词汇（`KnownEventPayload`，tag = `type`，snake_case）：
  `run_state_changed`、`model_call_started`、`model_call_delta`、`model_call_completed`、
  `tool_call_started`、`tool_call_completed`、`approval_requested`、`approval_decided`、
  `assistant_segment_start`、`assistant_segment_delta`、`assistant_segment_end`、
  `final_message_committed`。其中 assistant segment 三种与现役
  `server/assistant-event-normalizer.ts` 的规范化词汇（S09）一一对应；MOOD/思考标签的
  兼容解析只有一个权威入口（语义由 `AssistantPhase` 承载，`unresolved` 显式存在，
  不得默默猜成 final）。
- WS 承载：每条 WS text frame 是一个 canonical JSON 的 `EventEnvelope`（原型见 §7）。
  快照与后续订阅以 `snapshotSeq` + cursor 边界衔接；游标过旧返回 `cursor_expired`；
  重复事件按 `eventId`/`seq` 去重。

## 6. 分页游标、资源引用、参数摘要

- 分页：`PageRequest{cursor?, limit?}` → `Page<T>{items, nextCursor?, snapshotSeq}`。
  cursor 为不透明字符串；服务端独有解释权；过旧/未知 → `cursor_expired`，客户端重取快照，
  不静默丢关键事件。
- 资源引用 `ResourceRef`：`resourceId`、`kind`（session_file/attachment/artifact/export）、
  `displayName?`、`uri?`、`digest?`（`ContentDigest`）、`sizeBytes?`（十进制字符串）。
  真实文件与授权归 ResourceService（02 §3）；模型自称"已生成"不产生资源引用。
- 参数摘要 `ArgsDigest`（= `ContentDigest`）：`{algorithm:"sha256",
  canonicalization:"lingxi-canonical-json-v1", hex}`，覆盖规范化后的参数对象；
  批准绑定 target、principal、run、规范化参数摘要、资源、generation、期限与次数
  （`ApprovalRequest`）。跨语言 parity 由 golden `event-tool-call-started.json` 的
  `args_digest` 检查证明（TS 重算 sha256(canonical(args)) 与 golden 内嵌值相等）。

## 7. 版本协商握手

请求 `ClientHello`（封闭）：`protocol`（必须 `"lingxi.wire"`）、`clientKind`、
`clientVersion`、`protocolMin`、`protocolMax`、`caps?`。
成功响应 `ServerHello`（封闭）：`protocol`、`selectedProtocol`、`wireProtocolMin/Max`、
`dataEpoch`、`serverKind`、`serverVersion`、`rejectedCaps`（客户端声明了但服务端不认识的
能力——报告而非静默丢弃）。

协商规则（`handshake::negotiate_protocol`）：

1. `protocol != "lingxi.wire"` → `invalid_message`。
2. `protocolMin > protocolMax` → `invalid_message`。
3. 取客户端范围与本端支持范围交集的最大者；**交集为空 → `version_incompatible`，
   `details` 携带 clientMin/clientMax/supportedMin/supportedMax，不进入任何默认猜测模式**。

原型绑定（R01-A04 证据，确定性本地替身，127.0.0.1）：

- HTTP：`POST /lingxi/v1/handshake` → 200 + `ServerHello` | 400 + `ProtocolError`。
- WS：`GET /lingxi/v1/ws`（RFC 6455 Upgrade）→ 101；首条 text frame 必须是
  `ClientHello`；兼容 → `ServerHello` frame + close 1000；不兼容 → `ProtocolError`
  frame + close 4409（"version_incompatible"）。
- 实现：`rust/crates/lingxi-protocol/src/bin/lingxi-proto-server.rs`（std::net 最小
  HTTP/WS，无框架依赖）；交互记录见 `artifacts/rust-tauri/R01/T02/handshake-*`。
  该原型不是 R02 的服务端形态，仅为契约证据。

## 8. 未知内容处理策略（不得静默丢弃）

| 情况 | 处理 | 机制 |
|---|---|---|
| 封闭结构（握手、信封、批准、错误、身份类、已知事件 payload）出现未知字段 | **硬错误** `invalid_message` | `#[serde(deny_unknown_fields)]`；演进靠版本提升，不靠猜 |
| 未知**事件类型** | **原样保留 + 显式上抛**：解码为 `EventPayload::Unknown{event_type, raw}`，完整 raw（含 type 键）逐字节保留；消费端必须按"不支持"呈报，不得丢弃 | 自定义 Deserialize；golden `event-unknown-future.json` 证明 permissionHint 等字段存活 |
| 未知 **schema 方言**（第三方工具 schema） | 显式 `unknown_schema_dialect` 错误；支持集 = `json-schema/2020-12`、`json-schema/draft-07` | `check_schema_dialect`；禁止宽松按 JSON Schema 解析（02 §5） |
| 第三方工具 schema 内容 | **逐字节原样保留**，仅在校验边界解释 | `ToolSchemaDocument.schema` 为原始 Value；golden `tool-schema-document.json` 证明 `x-vendor-extension` 等厂商扩展存活 |
| Provider opaque/signature 块 | 逐字节保留，不得压平成普通正文 | `ContentBlock::Opaque{provider, data}`（02 §6） |
| 握手未知能力 | `ServerHello.rejectedCaps` 显式回告 | 见 §7 |
| 错误结构扩展 | 只进 `ProtocolError.details` | 见 §4 |

## 9. 规范 JSON（lingxi-canonical-json-v1）

golden、内容摘要、跨语言字节相等判定统一使用该 profile：

- UTF-8、无空白、无尾随换行；
- 对象键按 Unicode 码点排序（serde_json 默认 BTreeMap；TS 侧 `Object.keys().sort()`
  在 UTF-16 码元序下与码点序等价）；
- 非 ASCII 原样输出（不转 `\uXXXX`）；
- 仅整数；wire 上不存在浮点与 u64 数字（u64 一律十进制字符串）。TS 端遇到非安全整数
  直接抛错，不静默截断。

双端实现：`rust/.../src/canon.rs` 与 `tests/migration/r01-t02/canonical-json.mjs`；
字节级一致性由 R01-A03 round-trip（双向逐字节相等）证明。

## 10. 与旧 surface 的兼容边界

旧 HTTP/WS 与 window.hana surface 的逐项清单与映射见
`docs/rust-tauri/R01/API_COMPAT_MATRIX.json`（脚本生成，`--check` 无 diff）：

- `business_compat`（业务兼容映射）：旧业务能力迁移期保留（`retain`），逐项给出
  `lingxi.v1.<module>` 新协议对应物；旧状态名等差异由传输层兼容映射承担，不改名核心业务语义。
- `native_host`（原生宿主映射）：window.hana/IPC 的宿主能力不进 `lingxi.wire`，
  R09 以 Tauri command/event 承接（`native_host_mapping`）。
- `controlled_deprecation`：仅 `/internal/browser` 原始 WS 通道（由 R04/R09 浏览器引擎
  控制通道替代；退役需 R08 旧客户端下线证据）。

矩阵戳记与门禁语义（fix-headsha-r1）：`generatedFrom.contentSha` 是内容派生戳
（`sha256(JSON.stringify({fullInventory, surfaces, summary, sourceDigests}))`），为被扫描
源码内容的纯函数；矩阵**不嵌入** git HEAD 等移动坐标，因此"重新生成 → 无 diff"契约在
任意提交上成立，`--check` 为全文逐字节比较、无字段豁免。门禁
`scripts/rust-tauri/r01-t02-check-generated.sh` 解析 `rust-toolchain.toml` 的 channel 并
经 `rustup run` 显式调用锁定工具链，不依赖 PATH 顺序。
