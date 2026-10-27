# R01-T02 独立对抗性验收报告（REVIEW_R1）

- 审查者：ZCode:R01-T02-review-r1（全新独立代理，未参与执行；只读审查 + /tmp 隔离复跑）
- 基线：HEAD = `a98d24877e056dcb490f23ec739713ca937e646e`（codex/rust-tauri-migration）
- 执行者报告：`docs/rust-tauri/R01/R01-T02_REPORT.md`（ZCode:R01-T02-exec-r1）
- 验收场景：R01-A03（跨语言 round-trip，REQUIRED）、R01-A04（版本不兼容可诊断，REQUIRED）
- 审查时间：2026-09-25；环境 macOS 27.0 arm64，Node v24.16.0，cargo 1.93.0，Python 3.14.3
- 结论性质标注：【运行】= 本审查实际重跑；【源码】= 读源码确证；【环境限制】= 未覆盖

## 最终判定：**PASS**

两个 REQUIRED 验收场景经独立复跑与对抗性加固验证均真实通过；全部生成物门禁、
T01 回归门禁、离线可复现性、生产零改动核实通过。发现 5 个非阻塞问题（2 中 / 3 低），
全部为 fail-loud（硬错误/字节比对检出），不存在静默语义丢失；见 §5，建议后续阶段修复。

## 1. 候选清单核实【运行】

`git status --porcelain` = 20 条（执行者报告称 19 条，计数微差，清单本身与声明范围完全一致，
无多余改动）：

- `M`：rust/Cargo.lock、rust/Cargo.toml、rust/crates/lingxi-kernel/src/ports.rs、
  rust/crates/lingxi-protocol/{Cargo.toml,src/lib.rs} + ORCHESTRATOR_PROGRESS.json
  （总控账本，既存修改，经 `git diff` 核实仅账本条目，本验收未触碰）
- `??`：contracts/、tests/migration/r01-t02/、scripts/rust-tauri/r01-t02-*（4 个）、
  rust/crates/lingxi-protocol/src/{bin/,canon.rs,handshake.rs,wire.rs}、
  docs/rust-tauri/R01/{PROTOCOL_SPEC.md,API_COMPAT_MATRIX.json,R01-T02_REPORT.md}、
  artifacts/rust-tauri/R01/T02/

生产零改动【运行】：`git diff HEAD -- desktop server core lib shared cli hub plugins
skills2set package.json package-lock.json` 为空；任务书目录与 .sync-audit 零改动
（diff --stat 行数 0）。原型未接入生产入口【运行】：在 desktop/ server/ core/ lib/
shared/ cli/ hub/ plugins/ package.json 中 grep `lingxi-protocol|contracts/generated|
lingxi.wire` 无任何引用。

哈希复算【运行】：

- `contracts/generated/MANIFEST.json`：55 个生成文件逐文件 sha256 复算全部吻合，
  目录内无未登记的多余文件（os.walk 全量比对）。
- `artifacts/rust-tauri/R01/T02/SHASUMS.txt`：`shasum -a 256 -c` 12 项全部 OK。
- API_COMPAT_MATRIX.json 624 条目（423 HTTP + 2 WS + 95 preload + 104 PlatformApi），
  与执行者声明一致。

## 2. 关键命令独立复跑（真实退出码）【运行】

我的隔离目录：`CARGO_TARGET_DIR=/tmp/r01t02-review/target`（非执行者的
`/tmp/lingxi-r01t02-target`），对抗样本与产物全部在 /tmp/r01t02-review/ 下。

| # | 命令（仓库根） | 退出码 | 结论 |
|---|---|---|---|
| R1 | `cargo build --offline --workspace`（全新空 target 目录，从零编译） | 0 | 离线从零构建通过 |
| R2 | 全新 `CARGO_HOME=/tmp/.../cargo-home` 下 `cargo fetch`（去代理直连）再 `CARGO_NET_OFFLINE=true cargo build --offline` | 0 | 执行者"fetch 后离线可复现"声明属实 |
| R3 | `cargo build --locked --offline --workspace` | 0 | Cargo.lock 与清单一致 |
| R4 | `bash scripts/rust-tauri/r01-t02-roundtrip.sh`（A03 门面） | 0 | 5/5 步全过 |
| R5 | `bash scripts/rust-tauri/r01-t02-handshake.sh /tmp/.../handshake`（A04 门面） | 0（client=0 server=0） | 5 场景全过 |
| R6 | `bash scripts/rust-tauri/r01-t02-check-generated.sh` | 0 | 两棵生成树无 diff（56 文件 + 624 条目） |
| R7 | `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0 | T01 正向门禁绿 |
| R8 | 同上 `--self-test` | 0 | N1–N15 负向弹药 15 条全部如期拒绝 |
| R9 | `python3 -B docs/rust-tauri/R01/r01_t01_build_ownership.py --check` | 0 | OWNERSHIP_TARGET 无漂移 |
| R10 | `cargo test --workspace --offline` | 0 | kernel 7 + protocol 19 全绿 |
| R11 | `npx tsc -p tests/migration/r01-t02/tsconfig.json`（含于 R4 第 5 步） | 0 | 生成 TS 绑定 + 消费用法类型检查 0 错 |

A03 门面复跑细节（R4）：19 项 cargo 单测全绿；gen --check 56 文件无 diff；
TS 消费端 12 个 golden 解码 + 对生成 schema 校验 + 规范重编码逐字节等于 Rust golden；
专项检查实跑输出：seq=9007199254741003 字符串保留且 `Number()→9007199254741004`
精度丢失被证明、args digest parity sha256=0e7018f1…、未知事件 permissionHint 存活、
x-vendor-extension 存活；Rust verify 读回 TS 副本逐字节相等。

A04 门面复跑细节（R5）：http-ok 200 selected=1 + rejectedCaps=["teleport"]；
http-too-new / http-too-old 均 400 version_incompatible（details 四元组齐、无
selectedProtocol）；ws-ok ServerHello 帧 + close 1000；ws-too-new 错误帧 + close 4409。
transcript 与 server.log 逐条核对一致。

## 3. A03 对抗性验证（自造样本，非执行者样本）【运行】

自建独立工具（不信任执行者样本与脚本）：

- `/tmp/r01t02-review/adv/`：独立 cargo 工程，路径依赖只读引用
  `rust/crates/lingxi-protocol`，用权威 Rust 类型编码我设计的 10 个对抗样本为 canonical
  golden，并做 Rust 读回闭环。
- `/tmp/r01t02-review/adv-probe.mjs`：独立 TS 探针，直接 import 仓库的
  `canonical-json.mjs`/`validator.mjs`，解码→对生成 schema 校验→重编码→逐字节比对。

样本与结果（Rust 编码→TS 解码→TS 重编码→Rust 读回）：

| 样本 | 内容 | 结果 |
|---|---|---|
| s1 | emoji/ZWJ/国旗/组合符文本，seq=u64::MAX "18446744073709551615" | PASS 逐字节相等 |
| s2 | seq=2^53-1（最后安全整数） | PASS |
| s3 | seq=2^53（首个不安全整数）+ usage u64::MAX | PASS |
| s4 | 未知事件：嵌套 null/空容器/permission+historySemantics 字段/中文键 | PASS 逐字节保留 |
| s5 | 未知事件：键混排 BMP 高位（U+E000）与非 BMP（U+10000/U+1F004） | **DIFF**（发现 F1） |
| s6 | 第三方 schema 含浮点 multipleOf 0.01/default 3.14 与 >2^53 整数 default | **TS 硬错误**（发现 F2） |
| s7 | 第三方 schema 深嵌套/null default/unicode 厂商扩展（对照组） | PASS |
| s8 | ModelRequest：u64::MAX generation/budget + 200 字 emoji 长 ID | PASS |
| s9 | ApprovalRequest：u64::MAX expiry/generation + 资源引用 | PASS |
| s10 | ProtocolError.details 嵌套 null | PASS |

BigInt 真实生效验证【运行】：s1 的 seq 在 TS 侧 `typeof === "string"`，
`BigInt("18446744073709551615")` 精确往返，`Number()` 得 18446744073709552000 证明精度
丢失；seq 全程不经 Number（字符串端到端）。TS canonical-json 对 9007199254740992、
3.14、0.01、-9007199254740992、1e300 全部硬抛错（不静默截断），对 9007199254740991 通过。

篡改检出【运行】（全部在 /tmp 副本上，仓库未动）：

| 测试 | 操作 | 预期/实际 |
|---|---|---|
| T1 | gen --check 对 /tmp 纯净副本 | exit 0 ✓ |
| T2 | 篡改 1 字节 golden（server-hello dataEpoch） | gen --check exit 1，drift 报告 ✓ |
| T3 | 手改生成 TS 绑定（追加一行） | exit 1 ✓ |
| T4 | 投放 stale 文件 | exit 1，"stale file on disk" ✓ |
| T5 | 篡改 schema 关键字（pattern→pettern） | exit 1 ✓ |
| T6 | 复原后复跑 | exit 0 ✓ |
| T7 | 篡改 golden 键名后跑 TS roundtrip | exit 1（sha256 不符）✓ |
| T8 | 篡改 TS 重编码副本后跑 Rust verify | exit 1，"NOT semantically equal" ✓ |
| T9 | 篡改 API matrix 条目后跑 extract --check | exit 1，DRIFT ✓（T9a 纯净副本 exit 0） |

## 4. A04 对抗性验证（自造畸形协商，打真实原型）【运行】

独立客户端 `/tmp/r01t02-review/adv-handshake.mjs` 对真实 `lingxi-proto-server`
（我自己构建的二进制，loopback）发起 21 场景（16 HTTP + 5 WS），client exit=0：

- HTTP 畸形全部 400 + 明确错误码、无 selectedProtocol：protocolMin>protocolMax、
  错误协议族、缺 protocolMax、缺 protocol、版本为字符串/浮点/负数/超 u32、多余未知字段、
  垃圾 JSON、空 body、caps 类型错误、protocol 为数字。
- HTTP 边界：0..=100 与 1..=999 均 200 selected=1（交集取最高，不越出客户端报价）。
- GET /lingxi/v1/handshake → 404 not_found（不被当握手处理）。
- WS 畸形：垃圾首帧、min>max、缺字段 → invalid_message 错误帧且无 selectedProtocol；
  0..=0 → version_incompatible + close 4409；正常 1..=1 → ServerHello + close 1000。
- "不进入默认猜测模式"：21 场景中无任何拒绝响应携带 selectedProtocol，全部显式失败。

## 5. 发现问题（非阻塞；均 fail-loud，无静默语义丢失）

### F1（低）规范 JSON 键排序在非 BMP 边界分叉 — 场景 ID：A03-adv-s5

- 最小重现：未知事件 payload 含键 `"z"`、`"\uE000"`、`"\u{10000}"`（𐀀）、`"\u{1F004}"`（🀄）。
  Rust（BTreeMap，码点序）产出 `z, \uE000, 𐀀, 🀄`；TS（`Object.keys().sort()`，UTF-16
  码元序）产出 `z, 𐀀, 🀄, \uE000`。两端 canonical 字节不同（值语义相等）。
- 影响：凡经开放扩展点（未知事件 raw、第三方 schema、opaque 块、error.details）且对象键
  同时含 BMP 高位（≥U+E000）与非 BMP 字符的内容，Rust 与 TS 算出的 canonical 字节与
  内容摘要不一致。round-trip 字节门禁会检出（fail-loud），但跨端 digest 对不上。
- 根因：PROTOCOL_SPEC §9 "UTF-16 码元序与码点序等价" 仅在 BMP 内成立；TS 默认字符串
  比较是 UTF-16 码元序。
- 修复方向：TS canonical-json 改用码点比较器（如 `[...keys].sort((a,b)=>a<b?-1:a>b?1:0)`
  结合 `Intl.Collator` 之外的逐码点实现，或直接按 code point 数组比较），并修正 spec 措辞；
  或在 spec 中显式限制 canonical 键为 BMP。

### F2（中）TS 规范编码无法承载含浮点/大整数的第三方 schema — 场景 ID：A03-adv-s6

- 最小重现：合法 draft-07 工具 schema `{"properties":{"ratio":{"multipleOf":0.01,
  "default":3.14},"big":{"default":9007199254740993}}}`。Rust 原样编码正常；TS 侧
  JSON.parse 后 `canonicalBytes` 对 0.01/3.14/被取整的大整数一律硬抛
  "non-safe-integer number on the wire is forbidden"。
- 影响：spec §8 承诺第三方工具 schema"逐字节原样保留"，但唯一交付的 TS canonical 实现对
  JSON Schema 中常见（合法）的浮点关键字无法重编码——此类 schema 经 TS 消费端即硬失败。
  spec §9"wire 上不存在浮点"与 §8 的 verbatim 扩展点自相矛盾（verbatim 内容可含浮点）。
  是硬错误而非静默损坏；R04 工具接入前必须解决或显式限定。
- 根因：canonical-json.mjs 用 `Number.isSafeInteger` 一刀切；JS Number 无法无损表示任意
  JSON 数字，需要 lossless 解析（如基于字符串的十进制保真编码）。
- 修复方向：TS 侧引入保十进制保真的 JSON 解析/编码（如对非安全整数与浮点按原始词法
  保留），或在 spec 中显式声明 TS 消费端对 verbatim 块不做 canonical 重编码而只做透传。

### F3（中）生成 schema 的 EventPayload 回退分支吞掉畸形已知事件；schema 未编码两项信封不变量 — 场景 ID：A03-adv-schema-probes

- 最小重现（对 `EventEnvelope.schema.json` 用仓库 validator.mjs 校验）：
  a) 合法 run_state_changed 事件把 `payload.to` 改成 `"exploded"` → 被接受
  （已知分支不匹配后落入 `additionalProperties:true` 的未知事件回退分支）；
  b) `eventType` 改成与 payload.type 不一致 → 被接受；
  c) `schemaVersion: 2` → 被接受。
  三者在 Rust 权威类型上均为硬错误（KnownEventPayload 解析失败不回退、eventType 一致性
  校验、schemaVersion!=1 拒绝——wire.rs 单测覆盖）。
- 影响：TS 消费端若仅以生成 schema 为校验层，会把"畸形已知事件"误归类为"未知事件"放行，
  且缺失 spec §5 声明的两项信封不变量。语义分歧方向是 TS 比 Rust 宽松。当前 round-trip
  门禁不受影响（字节仍相等），权威边界（Rust）仍强制；属纵深防御缺口。
- 根因：wire.rs 中 EventPayload 的手写 `json_schema` 回退分支未排除已知 type 标签；
  schemars 未为 schemaVersion 生成 const，eventType 无联动约束（schema 表达能力限制）。
- 修复方向：回退分支加 `"not":{"properties":{"type":{"enum":[已知12种]}},"required":["type"]}`
  （validator.mjs 需支持 `not`，当前遇未知关键字硬报错，属预期内扩展）；schemaVersion 在
  手写 schema 中加 `const:1`；eventType/payload 一致性在 spec 中明确"schema 不表达、
  由反序列化器强制"，并为 TS 消费端补对应代码检查。

### F4（低）Rust 侧 Seq/u64 解析比 schema pattern 宽松 — 场景 ID：A03-adv-rust-probe

- 最小重现：`Seq::from_wire_string("007")` → Ok(7)；`"+5"` → Ok(5)；UsageRecord
  `inputTokens:"007"` 反序列化成功。生成 schema 的 pattern `^(0|[1-9][0-9]*)$` 与 TS
  校验均拒绝这两种形式。全角数字、含空格形式两端都拒绝。
- 影响：Rust 端点会接受 schema 声明非法的 wire 输入（前导零/加号）；重编码后归一化，
  字节门禁会在 Rust→TS→Rust 闭环检出漂移，不会静默通过，但"schema 即契约"的严格性打折。
- 根因：直接用 `str::parse::<u64>()`（接受前导零与 `+`），未先按 spec pattern 校验词法。
- 修复方向：`from_wire_string`/`u64_wire_string::deserialize` 先匹配
  `^(0|[1-9][0-9]*)$` 再 parse。

### F5（低）WS 关闭码对 invalid_message 也用 4409/"version_incompatible" — 场景 ID：A04-adv-w17/18/20

- 最小重现：WS 首帧为垃圾 JSON 或 min>max，错误帧 code=invalid_message，但 close 帧恒为
  4409 且 reason="version_incompatible"（lingxi-proto-server.rs 硬编码
  CLOSE_VERSION_INCOMPATIBLE 用于一切拒绝）。
- 影响：仅原型表现层；错误帧内 code 正确，诊断信息不丢。spec §7 只说"不兼容 → close
  4409"，未覆盖 invalid_message 的关闭码。
- 修复方向：原型按错误码选关闭码（如 invalid_message 用 4400 或 1008），或在 spec 注明。

### 观察（不构成问题）

- proto-server `handle_hello` 有一个双分支同为 400 的死代码 if（VersionIncompatible/else），
  纯装饰性。
- matrix 中 `ws:GET:/ws` 的 module 标注为 "chat (unmounted?)"——如实标注了 wsRoute 不在
  工厂导入映射内，非错误。
- 执行者报告称工作区 19 条，实际 20 条（清单内容与声明范围一致，仅计数差）。

## 6. 单一协议源核查【源码 + 运行】

- 权威链属实：serde 类型（lib.rs/wire.rs/handshake.rs）→ schemars 1.2.2
  `into_root_schema_for` 生成 JSON Schema 2020-12（gen bin 的 schema_registry，40 类型
  逐一登记）→ 同一 schema 树经 TsEmitter（吸收 $defs）生成 TS 绑定；golden 由
  canonical_bytes 产出并附 sha256。TS 绑定与 schema 确由生成器产出（T3/T5 手改即被
  --check 检出）。
- 第三方 schema 原样保留：ToolSchemaDocument.schema 为原始 Value【源码】；golden
  tool-schema-document 与我对抗样本 s7 字节级证明【运行】；边界校验 check_schema_dialect
  对未知方言显式 unknown_schema_dialect【运行：cargo 单测 dialect_check_is_explicit】。
- 未知字段/事件/方言处理如 spec：封闭结构 deny_unknown_fields（我对 8 种畸形信封的
  schema 探针 + Rust 单测双向确认）；未知事件 UnknownEventPayload 逐字节保留（s4 自造
  permission/historySemantics 字段存活）；未知方言显式错误。未发现静默丢弃权限或历史
  语义字段的路径。例外即 F3：TS schema 层会把"畸形已知事件"当未知事件接受（宽松方向，
  Rust 权威不放松）。

## 7. API_COMPAT_MATRIX 抽查【运行】

确定性随机（seed 固定）抽 5 条逐条回查现役源码：

| 条目 | 源码核实 |
|---|---|
| `http:GET:/api/git/branches`（server/routes/git-environment.ts:252） | 行号、路由、挂载（full-root.ts:34 `app.route("/api", createGitEnvironmentRoute…)`)一致，retain + lingxi.v1 对应物合理 |
| `http:GET:/api/conversations/:id/export`（channels.ts:233，mounted /api） | 一致 |
| `ws:GET:/ws`（chat.ts:2256 upgradeWebSocket） | 一致，retain + 事件流对应物合理 |
| `preload:observabilityExportBegin` | preload.cjs:127 存在，native_host_mapping 归类合理 |
| `platform_api:speechPermissionRequest` | types.ts:616 存在，native_host 归类合理 |

business_compat（427 条）与 native_host（197 条）分面真实；controlled_deprecation 仅
/internal/browser 一条且带 R08 证据条件【源码：OVERRIDES 表】。S09 词汇声称属实：
server/assistant-event-normalizer.ts 确含 assistant_segment_start/delta/end 与
reasoning/commentary/final_answer/unresolved。

## 8. 身份字段覆盖（02 §3）【源码】

协议类型中真实定义且经 golden/单测覆盖：SessionId/RunId/AttemptId/ModelCallId/
ToolCallId/ResourceId/StreamId/EventId（opaque 字符串，serde transparent）、Seq（十进制
字符串，拒裸数字）、ProtocolError（code/message/retryable/details 封闭）、分页
Cursor/PageRequest/Page{items,nextCursor,snapshotSeq}、ResourceRef+ContentDigest、
ArgsDigest（=ContentDigest，sha256/canonicalization/hex）、版本协商
ClientHello/ServerHello/negotiate_protocol。ModelRequest 固定携带
principal/run/attempt/modelCall/purpose/provider/model/operation/budget/
configGeneration；ApprovalRequest 绑定 target/principal/run/attempt/argsDigest/
resources/generation/expiresAt/remainingUses。无缺失项。

## 9. T01 门禁未破坏【运行】

R7–R10 全绿：T01 校验器正向 exit 0、N1–N15 自测 exit 0（15 条负向弹药全检出）、
T01 生成器 --check exit 0、cargo workspace 测试 26 项全绿。DEPENDENCY_RULES 对
lingxi-protocol 新增依赖（serde/serde_json/schemars/sha1/sha2/base64）与新 bin 无违规
（DEP 检查在新 tree 上通过；Cargo.lock 30 个包均为上述六者的传递依赖，无
tauri/electron/tao/wry）。kernel ports.rs 的 Eq→PartialEq 微调与执行者声明完全一致
（diff 仅 3 行注释 + derive 行），无语义变化，cargo test 通过佐证。

## 10. 未覆盖 / 环境限制

- 已知预存失败（4 审计封印 FAIL 等）按任务说明未复跑、不影响本判定【环境限制】。
- 未跑全量 `npm test`（本任务未触碰 JS 生产代码，门面脚本独立于 vitest）【环境限制】。
- 握手原型仅 loopback；Windows/Linux 未验证（归 R09/R10）【环境限制】。
- Cargo.lock 未提交（执行者无 commit 授权，与本验收职责一致）。

## 11. 判定依据小结

A03（REQUIRED）：执行者门禁独立复跑 exit 0（R4），12 golden 逐字节、精度证明、
TS 校验+重编码+Rust 读回闭环真实；自造 10 对抗样本中 8 个全链路逐字节通过，2 个
触发的分叉均为 fail-loud 且属 §5 记录的边缘缺口，不在验收必需样本集（中文/空值/长 ID/
附件/错误/超 2^53 精度）内，不改变"类型和语义一致、超 JS 安全整数不丢精度"的验收结论。
A04（REQUIRED）：门面 5 场景 + 自造 21 畸形场景全部显式拒绝、诊断信息完整、
无任何默认猜测路径。交付物齐全（PROTOCOL_SPEC / 协议原型及生成脚本 /
API_COMPAT_MATRIX），生成物不可手改有门禁实证（T1–T9），T01 门禁完好，生产零改动。
§5 五项发现建议立项修复（F2/F3 建议先于 R04/R08 依赖这些边缘前处理），不构成
本任务 FAIL 条件。
