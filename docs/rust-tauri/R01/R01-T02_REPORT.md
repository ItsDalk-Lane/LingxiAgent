# R01-T02 执行报告 — 定义单一协议源及兼容边界

- 执行者：ZCode:R01-T02-exec-r1（独立执行代理；不含独立验收，不 commit/push）
- 基线：HEAD = `a98d24877e056dcb490f23ec739713ca937e646e`（codex/rust-tauri-migration，R01-T01 已 PASS 推送）
- 开工留证：`git status --porcelain` 仅 `M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`（总控账本，本任务未触碰）
- 环境：macOS 27.0 arm64；Node v24.16.0；cargo/rustc 1.93.0（Homebrew）；Python 3.14.3
  （详见 artifacts/rust-tauri/R01/T02/ENVIRONMENT.txt）。代理已死，全部 cargo 联网命令经
  `env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY` 直连；
  构建产物隔离于 `CARGO_TARGET_DIR=/tmp/lingxi-r01t02-target`。
- 结论：**READY_FOR_REVIEW**（判定归独立验收代理）

## 1. 范围与观察事实

按任务书 §4 R01-T02 五步 + 版本协商原型（步骤 6）执行。观察事实（读自真实源码，非文档手抄）：

- 现役契约版本：`shared/contract-versions.json` = PRELOAD_API_VERSION 1 / SERVER_PROTOCOL_VERSION 1 / DATA_EPOCH 1（R00 冻结基线，本任务未修改）。
- 现役 HTTP/WS surface：`server/index.ts` 直接路由 + `composition/open-root.ts`/`full-root.ts` 挂载 49 个路由模块，`server/routes/*.ts` 共 423 条 HTTP 声明；WS 面 = `chat.ts` 的 `/ws`（upgradeWebSocket）+ `server/index.ts` 的 `/internal/browser`（raw ws）。
- window.hana surface：`desktop/preload.cjs` 暴露 95 个成员；`desktop/src/react/types.ts` 的 `PlatformApi` 104 个成员（差异项均为 on\* 事件订阅与调试入口，清单见 API_COMPAT_MATRIX，逐项核对归 R09）。
- 事件规范化词汇（S09 `server/assistant-event-normalizer.ts`）：`assistant_segment_start/delta/end` + 语义相位 `reasoning/commentary/final_answer/unresolved` —— 新协议事件词汇与之逐一对应。

## 2. 交付物与 SHA-256

| 交付物 | 说明 |
|---|---|
| `docs/rust-tauri/R01/PROTOCOL_SPEC.md`（a6b856a3…） | 协议规格 v1：版本轴、身份、信封、错误、游标、资源、摘要、握手、未知内容策略、规范 JSON |
| `docs/rust-tauri/R01/API_COMPAT_MATRIX.json`（e76de0ec…） | 624 条目（423 HTTP + 2 WS + 95 preload + 104 PlatformApi），业务兼容/原生宿主分面，逐项 disposition |
| `rust/crates/lingxi-protocol/` 扩展 | serde 权威类型（wire.rs/handshake.rs/canon.rs + lib.rs 既有词汇加 serde），3 个 bin：gen/verify/proto-server |
| `contracts/generated/`（56 文件，逐文件 sha256 见 MANIFEST.json） | JSON Schema 2020-12 ×40 + TS 绑定 + 12 个跨语言 golden + 索引/清单 |
| `tests/migration/r01-t02/` | TS 消费端原型（canonical-json/validator/roundtrip/handshake-client/type-usage），不接任何生产入口 |
| `scripts/rust-tauri/r01-t02-*.sh/.mjs` | 门面脚本：roundtrip、check-generated（重新生成无 diff 门禁）、handshake、API 提取 |
| `artifacts/rust-tauri/R01/T02/` | 证据（逐文件 sha256 见 SHASUMS.txt） |

完整逐文件 SHA-256：`artifacts/rust-tauri/R01/T02/`（证据）与上文交付清单（/tmp 摘要已并入
证据目录 ENVIRONMENT.txt 之外的日志；交付物哈希见第 6 节场景表与 MANIFEST/SHASUMS）。

## 3. 关键设计决定

1. **权威与生成链**：Rust serde 类型为唯一权威 → schemars 1.2.2 生成 JSON Schema 2020-12；
   自研生成器（`lingxi-protocol-gen.rs` 内 TsEmitter）由同一 schema 树生成 TS 绑定。
   TS 不另写一套类型；消费端校验器（validator.mjs）对生成 schema 求值，遇到未实现的
   schema 关键字**硬报错**（曾因拒绝跳过 `minimum` 关键字抓到真实缺口并补上实现）。
2. **版本轴**：新协议为独立轴 `lingxi.wire` v1；现役三轴（preload/server/data epoch）全部冻结，
   与现役值的关系写入 PROTOCOL_SPEC §2。握手中 `dataEpoch` 仅如实报告，epoch 迁移归 R01-T07。
3. **精度规则**：wire 上一切 u64（seq/tokens/bytes/时间戳/generation）为十进制字符串；
   裸 JSON 数字被拒收，不开静默精度路径。TS 端 canonical-json 遇非安全整数直接抛。
4. **未知内容策略**（PROTOCOL_SPEC §8）：封闭结构（握手/信封/批准/错误/已知事件 payload）
   `deny_unknown_fields` 硬错误；未知事件类型解码为 `EventPayload::Unknown{event_type, raw}`
   **逐字节保留并显式上抛**；未知 schema 方言显式 `unknown_schema_dialect`；第三方工具 schema
   与 provider opaque 块原样保留（均有 golden 证明）。
5. **eventType 冗余校验**：envelope 顶层 `eventType` 与 payload tag 不一致 = `invalid_message`，
   不猜哪个为准；`schemaVersion != 1` 硬拒绝。
6. **握手原型形态**：std::net 最小 HTTP/1.1 + RFC 6455（sha1/base64 两个微依赖），loopback 固定
   5 连接后自退，确定性可复跑。不引 axum/tokio——原型只为契约证据，R02 才定服务端形态。
7. **依赖与锁**：新增 serde/serde_json/schemars/sha1/sha2/base64 仅进 `rust/` 体系，
   `Cargo.lock` 已更新（版本精确锁定在锁文件内；锁定策略本身归 R01-T03）。
   离线可复现：`cargo fetch` 后 `CARGO_NET_OFFLINE=true` 全量构建/测试通过。
8. **kernel 微调**：`ProtocolError` 增加 `details` 后不再 `Eq`（JSON 值无全等），
   kernel `ToolOutcome` 同步降为 `PartialEq`（唯一对 T01 交付物的修改，派生级别，无语义变化）。

## 4. 逐场景执行（命令 / 预期 / 实际 / 退出码）

### R01-A03 跨语言 round-trip — 证据：roundtrip.log + negative-proofs.log

门面：`bash scripts/rust-tauri/r01-t02-roundtrip.sh`（5 步，exit=0）：

1. `cargo test -p lingxi-protocol` — 19 项单测全绿（含 >2^53 seq 字符串化、未知事件保留、
   封闭结构拒未知字段、方言显式错误）。
2. `lingxi-protocol-gen -- --check` — 56 文件重新生成无 diff。
3. `node tests/migration/r01-t02/roundtrip.mjs` — 12 个 golden（含中文、空值、128+ 字符长 ID、
   附件、错误、seq=9007199254741003 > 2^53-1）：TS 解码 + 对生成 JSON Schema 校验 +
   TS 规范重编码，**逐字节等于 Rust golden**；专项断言：BigInt 证明 Number 丢精度、
   TS 重算 sha256(canonical(args)) 与 golden 内嵌参数摘要相等（0e7018f1…）、未知事件
   `future_recall_started` 的 permissionHint 存活、厂商 `x-vendor-extension` 原样存活。
4. `lingxi-protocol-verify -- --ts <TS输出目录>` — Rust 读回 TS 重编码副本，逐字节相等 +
   按权威类型反序列化再编码稳定（Rust→TS→Rust 闭环）。
5. `npx tsc -p tests/migration/r01-t02/tsconfig.json` — 生成 TS 绑定 + 消费用法类型检查 0 错。

负向证明（全部如期失败，negative-proofs.log）：N1 篡改 TS 副本 → Rust verify exit=1
（"NOT semantically equal"）；N2 篡改 golden → TS roundtrip exit=1（sha 不符）；
N3 篡改生成树 → gen --check exit=1（drift）；复原后 roundtrip exit=0。

### R01-A04 版本不兼容可诊断 — 证据：handshake.log + handshake/server.log + handshake/transcript.jsonl

门面：`bash scripts/rust-tauri/r01-t02-handshake.sh artifacts/rust-tauri/R01/T02/handshake`（exit=0；
server exit=0，client exit=0）。真实 HTTP（fetch）与真实 WS（Node WebSocket，RFC 6455
含掩码帧）对 127.0.0.1 确定性替身，5 场景：

| 场景 | 预期 | 实际 |
|---|---|---|
| http-ok（1..=1，含未知 cap） | 200 + selectedProtocol=1 + rejectedCaps=["teleport"] | 符合 |
| http-too-new（2..=2） | 400 + code=version_incompatible + details(clientMin/clientMax/supportedMin/supportedMax)，无 selectedProtocol | 符合 |
| http-too-old（0..=0） | 同上 | 符合 |
| ws-ok（1..=1） | ServerHello 帧 + close 1000 | 符合 |
| ws-too-new（3..=3） | 错误帧 + close 4409 "version_incompatible" | 符合 |

"不进入默认猜测模式"由断言硬性保证：任何拒绝响应携带 `selectedProtocol` 即失败。

### 回归门禁（改 rust/ 后必跑）— EXIT_CODES.txt 全 0

- `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py`（正向）exit=0
- `... --self-test`（含 N1–N15 负向弹药）exit=0
- `python3 -B docs/rust-tauri/R01/r01_t01_build_ownership.py --check` exit=0
- `cargo test --workspace` exit=0（kernel 7 + protocol 19）
- `bash scripts/rust-tauri/r01-t02-check-generated.sh` exit=0（两棵生成树无 diff）

## 5. 未执行 / 环境受限项（如实声明）

- 未跑全量 `npm test`（任务说明明确不必；预存 4 个审计封印 FAIL 与 round2/3 夹具重写均为已知
  预存问题，本任务未触碰）。TS 消费端未注册进 vitest 套件，由独立门面脚本执行。
- 握手原型仅 loopback 本机替身（任务书允许）；非真实外网服务。
- Windows/Linux 未验证（本机 macOS arm64；跨平台门禁归 R09/R10）。
- Cargo.lock 已在工作区更新但未提交（本任务无 commit 授权）。

## 6. 回退

删除以下新增即可完整回退：`contracts/`、`tests/migration/r01-t02/`、`scripts/rust-tauri/r01-t02-*`、
`artifacts/rust-tauri/R01/T02/`、`docs/rust-tauri/R01/{PROTOCOL_SPEC.md,API_COMPAT_MATRIX.json}`，
并将 `rust/` 下改动 `git checkout` 回基线（lib.rs/wire.rs/canon.rs/handshake.rs/bin/\*、
两个 Cargo.toml、Cargo.lock、kernel ports.rs 的 Eq 微调）。无生产代码改动，回退不影响现役系统。

## 7. 交接给后续阶段

- R01-T03：锁定策略待定项——serde 1.0.229 / serde_json 1.0.151 / schemars 1.2.2 / sha1 0.11.0 /
  sha2 0.11.0 / base64 0.23.1（Cargo.lock 已锁精确版本）。
- R02：`lingxi.wire` v1 类型与握手协商可直接复用；HTTP/WS 原型仅为契约证据，服务端形态自决。
- R04：工具 schema 方言边界（SUPPORTED_SCHEMA_DIALECTS）与 ArgsDigest 已固定。
- R08：API_COMPAT_MATRIX 的 624 条目为旧客户端接入的逐项映射底账。
- R09：preload/PlatformApi 两表面差异项（preload 多出 debugOpenOnboarding、quickChatShow、
  screenshotRender、debugOpenOnboardingPreview；PlatformApi 多出 13 个 on\* 订阅）需逐项核对。

最终 `git status --porcelain`：19 条 = 上述交付 + 既有 `M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`
（未触碰）。生产目录 desktop/ server/ core/ lib/ shared/ cli/ hub/ plugins/ skills2set/ package\*.json
零改动；任务书目录、.sync-audit 未触碰。
