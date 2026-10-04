# R05_REPORT — 模型协议、凭证、流式处理与完整任务闭环

- 生成：EXECUTOR-R05-T08，2026-10-04。阶段：R05（T01–T08 全部八任务）。
- 基线：分支 `codex/rust-tauri-migration`，HEAD `c549ff654508ab951e2cf39cf9d309fc9c6b8656`；R05 全部交付为**未提交工作树改动**（no_commit_push_authorization=true，全程零 git commit/push/stash/checkout/reset/clean）。
- 本报告只陈述真实运行过的命令与结果；命令与退出码见各证据文件。

## 1. 阶段状态（§9 格式）

```text
offline_gate:            PASS（verify-stage R05 overall PASS：7/7 命令、18/18 场景、130/130 叶、绑定 stable；§5.1）
independent_review:      PENDING（R05_INDEPENDENT_REVIEW.md 由独立审查者按附录 D 执行）
live_verification:       BLOCKED_NOT_AUTHORIZED（RR-BLK-CREDENTIALS，最迟 R10；R05_LIVE_VERIFICATION.json）
platform_verification:   macOS arm64 = 本机全离线门禁；Windows = 未验证（继承 R04）；Linux = 未真机验证
stage_readiness:         READY_FOR_INDEPENDENT_REVIEW（离线范围全部通过且已登记 LIVE 延期；独立审查通过后可取 ACCEPTED_OFFLINE_SCOPE_WITH_REGISTERED_LIVE_DEFERRALS）
release_state:           NOT_IN_SCOPE
```

## 2. 实现概要（按任务）

- **T01**：统一 `ConfigModelGateway`（pin-pair 路由/能力矩阵/reload 代次）+ `model_exchange` 最小交换契约（工具声明快照、宿主 ToolCallId、provider 关联、opaque 状态、截止时间）+ 正式 bootstrap 接线（`lib.rs` wired_real_model_chain：文件工具+审批服务+统一网关+凭证服务）；`--config` 模型面与无配置诚实不可执行。交付 SCOPE/CALLSITE/PROVIDER 三矩阵与 INTERFACE_EVOLUTION。
- **T02**：`CredentialService`（per-provider 刷新协调、撤销栅栏、并发合并、原子持久化、代次核验）+ OAuth device/PKCE/刷新全流程（adapters oauth 模块）+ 脱敏（redaction 全链，base64/slash 变体）。CREDENTIAL_FLOW_MATRIX 落档。
- **T03**：五协议族（openai-completions / openai-responses / openai-codex-responses / anthropic-messages / google-generative-ai）真实编码解码、工具声明与结果回传、外部↔内部调用 ID 映射、opaque/签名状态保真、服务端引用绑定；脱敏 golden 固化（PROTOCOL_WIRE_MATRIX）。
- **T04**：SSE 帧解析 + 五族流式累积 + 分片不变性（含 UTF-8 跨片/CRLF/BOM）+ 半批不执行 + 停止原因表 + 真流式首片先达 + MOOD/思考块与签名隔离（streaming_norm）。
- **T05**：分阶段超时/重试分类/429-5xx 退避/取消传递/已接受不盲重发/工具副作用不重做 + egress URL/内网边界（models::egress）+ incumbent 兼容层（models::compat）。
- **T06**：操作面（embedding/speech/transcribe/media/rerank 的 wire 与校验）+ `GatewayWorkerModel`（同一 QuotaManager、单 permit 嵌套链、invocation 级预算、callback 台账）+ 媒体 job 诚实状态。WORKER_MODEL_BOUNDARY 落档。
- **T07**：usage 四态（reported/partial/unknown/invalid→estimated 扩展）+ 五族公式表（USAGE_MAPPINGS）+ 台账 migration v5 + 先提交后发布 + owner 范围查询隔离。MODEL_USAGE_SEMANTICS 落档。
- **T08**（本任务）：`r05_t08_closed_loop` 正式二进制闭环（10 测试）+ 阶段图/生产者/TSV 表/xtask 双处注册（含 runner_identity 绑定——负向 N16 抓到并修正的注册缺口）+ 附录 C 16 项负向门禁 + 本报告与交接。

## 3. 变化与影响面

- 新增生产模块：`lingxi-kernel/src/{model_exchange,usage}.rs`；`lingxi-adapters/src/models/{gateway,config,dispatch,streaming,egress,compat,tool_render,usage,anthropic_messages,google_generative_ai,openai_*,oauth,…}.rs`；`lingxi-service/src/{credentials/,operations.rs,streaming_norm.rs,workermodel.rs}`。
- 演进接口（消费者已同步）：`TurnProviderPort`→`next_turn(ModelTurnInput)`（r03/r04/r05 全部调用方迁移）；`ProviderTurnResult` 新增 usage_report/served_protocol/transport_attempts；`StoragePort` 新增 model-call-usage 两方法；`ParsedChat` 改两字段+usage()。
- 存储：migration v5（`model_call_usage`），隔离副本验证，旧数据不覆盖。
- 正式入口：`--config` providers/models 闭合键集；无 plane = 显式 unconfigured（C03 双层钉住）。
- 旧产品零改动：`git diff --name-only c549ff654 -- desktop/ shared/ package.json package-lock.json` = 空集（证据 `T08-E01/old-product-diff.txt`）。

## 4. 验收与映射

- 附录 A 16 个 A-ID 全部保留（阶段图 REQUIRED 场景 + R05_TEST_MAP aIndex 16/16 有 C-ID 归属）。
- 附录 B 103 个 C-ID（T01–T07 的 89 + T05/T06 补充 B 系 + T08 的 14）全部映射到具体命令/测试：91 个由生产者 cid 表逐测试唯一归属（254 对），8 个证据非 cargo 类（矩阵脚本/门禁命令本体/登记延期/别名），4 个 T08 场景级绑定（C10/C11/C13/C14）——逐条见 `R05_TEST_MAP.json`。
- 台账更正（登记）：T01-C06 两个 lib 测试旧名按 `cargo -- --list` 实名归一（更名前名称，语义不变）。
- 已登记延期：T05-C12（代理/私有 CA 能力差距，REV-T05 复核）、LIVE 全家（RR-BLK-CREDENTIALS）。

## 5. 门禁与命令（真实运行记录）

| 检查 | 命令 | 结果 | 证据 |
|---|---|---|---|
| fmt | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | exit 0 | `T08-E01/gates-fmt.log` |
| clippy | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | exit 0 | `T08-E01/gates-clippy.log` |
| workspace | `cargo test --manifest-path rust/Cargo.toml --workspace --locked` | 首跑与重跑各 1 个失败：`r00_management_leaves` 的 LAN 腿（192.168.3.5 停驻）——与台账环境项 R05-ENV-ALF-UNSIGNED-TEST-BINARY 签名一致；其余 835 passed / 0 failed。静默窗口三跑见 §5.1 | `T08-E01/gates-test-workspace{,-rerun}.log` |
| contracts | `bash scripts/rust-tauri/r01-t02-check-generated.sh` | exit 0 | `T08-E01/gates-check-contracts.log` |
| boundaries | `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py` | exit 0 | `T08-E01/gates-check-boundaries.log` |
| 生产者 | `bash scripts/rust-tauri/r05_t08_stage_suites.sh <fresh>` | 82/82 运行全绿（18 套件 + 64 lib 钉、91 C-ID） | `T08-E01/verify-R05/R05_SUITES/` |
| 阶段门禁 | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R05 --evidence artifacts/rust-tauri/R05/T08-E01/verify-R05` | 见 §5.1 | `T08-E01/verify-R05/verify-stage-result.json` |
| 阶段门禁（FINAL-WFR2-1 终验复跑） | `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R05 --evidence artifacts/rust-tauri/R05/FINAL-WFR2-1/verify-R05` | **exit 0（overall PASS）**，见 §5.2 | `FINAL-WFR2-1/verify-R05/verify-stage-result.json` |

### 5.1 最终结果（attempt 3，2026-10-03T21:0x–22:16Z 窗口）

- **verify-stage R05：overall PASS（exit 0）**——7/7 命令 PASS（workspace 30.2min / r05_stage_suites / fmt / clippy / contracts / boundaries / r04_regression_gate 60.7min）、18/18 场景 PASS、130/130 R00 绑定叶 PASS、候选前后绑定 stable=true、testedSha `c549ff654`（dirty 工作树，per-file manifest `0b110d87eb01a20c181c32d2e835f84c495130ded07b23fe111cbbf1d2287018` 即 36,461 文件清单 sha256）。证据：`T08-E01/verify-R05/verify-stage-result.json` + `T08-E01/verify-R05-console.log`。
- 门禁内 workspace 腿：106 组 ok / **1286 passed / 0 failed**（清机窗口下 r00 LAN 腿通过——间歇环境项未触发；此前两次窗口的失败与签名已如实登记于 §7）。
- 嵌套前序链：`verify-R05/R04_REGRESSION` overall PASS（内嵌 R03 overall PASS → R02 链全绿，含修复后的 r02_storage_tx——见 §6 注 3 之外的第三处修复：R02 存储注册表补登 v5 迁移，指纹按注册表自身 sha256-of-SQL 方法重算并以复现 v4 指纹自校）。
- 前两次尝试如实归档：`verify-R05-attempt1-refused`（130 叶尚未镜像→门禁在命令前硬拒，正是 R13-F01 保护）与 `verify-R05-attempt2-r02-registry`（唯一失败=嵌套 R02 storage_tx 的注册表缺 v5 登记→已修注册表并整链复跑）。

### 5.2 FINAL-WFR2-1 终验复跑（2026-10-04T03:08:44–04:37:56Z，本报告收尾追加）

- 命令：`cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R05 --evidence artifacts/rust-tauri/R05/FINAL-WFR2-1/verify-R05` → **exit 0（机器记录 overall=PASS，internalError=null）**。证据目录：`artifacts/rust-tauri/R05/FINAL-WFR2-1/verify-R05/`（机器记录 `verify-stage-result.json` + 各命令子目录；本次无独立 console.log，以 JSON 记录为准）。
- **7/7 命令 PASS（逐条 exit 0）**：workspace（`cargo test --workspace --locked`，18.6min）/ r05_stage_suites / fmt / clippy / contracts / boundaries / r04_regression_gate（62.0min）。总 wall 89.2min，macOS arm64，toolchain 1.98.1。
- 门禁内 workspace 腿：106 组 ok / **1286 passed / 0 failed**（本窗口 r00 LAN 腿通过——ALF 间歇环境项未触发，§7 登记不变）。
- **18/18 场景 PASS**（R05-A01…A16 + R05-SUP-R04REG + R05-SUP-SCOPE）；**130/130 R00 绑定叶 PASS**（expectedFromR00Ledger=declaredInStageMap=130，fail=0，blocked=0）。
- 嵌套 `R04_REGRESSION` overall PASS（stable=true）。
- 候选绑定：前后一致 stable=true（digest sha256 `277ffa31…`，per-file manifest sha256 `1e46395970e06f81a394edfb016553a68fa136f4c79490859e7c38b41cef2544`，37,541 文件——较 §5.1 的 36,461 增加系 T08 收尾后工作树新增 R05 文档/证据所致）；testedSha `c549ff654`（dirty 工作树，与 §5.1 同一 HEAD）。
- 本节为追加记录，不改动 §5.1 及其他章节结论。

## 6. 负向门禁（附录 C）

- 执行：`bash scripts/rust-tauri/r05_t08_negative_gate.sh artifacts/rust-tauri/R05/T08-E01/negative`（隔离副本＝$HOME 下本地 clone + 未提交工作树 overlay；主树零注入，pristine 快照逐例还原）。
- **16/16 全部失败关闭且缺口点名；两个同环境对照全绿**。逐例表与两个由负向门禁自身抓出并修复的缺口（R05.json 漏 runner_identity 双处注册；生产者 FAILED 摘要双语序解析回归）见 `R05_NEGATIVE_GATE_REPORT.md` 与机器记录 `T08-E01/negative/case-results.json`（allRefused=true, controlsGreen=true）。

## 7. 未测与限定

- LIVE 真实供应商（未授权，最迟 R10）；Windows/Linux 平台（未验证，继承）。
- 间歇环境项 R05-ENV-ALF-UNSIGNED-TEST-BINARY：`r00_management_leaves` 的 LAN 腿（非回环自地址 192.168.3.5）在部分窗口停驻——二进制字节未变（T07 绿窗口与本阶段失败窗口的 r00 测试二进制哈希同为 `fd0d0f76ea681df4`），停驻为间歇性，且单独静默重跑同脚本能全绿（管理矩阵 67/67 亲跑）。处理：按台账登记（不放宽门禁、不改测试、不当回归修）；用户侧行动（ALF Allow / socketfilterfw / 开发者签名）后即可消失。
- 修复轮携带项（非阻塞）：REV-T07 的 F-04/F-05/F-06（见 R05_BLOCKERS §5）。

## 8. 回退

- 全部改动为工作树未提交差异；任何回退＝丢弃对应文件的未提交修改（无迁移风险：migration v5 只新增表；凭证存储为隔离文件；--config 为新增键集）。
- 门禁侧新增（R05.json/TSV/两脚本/xtask 注册/镜像单测）独立于运行时行为，回退不影响服务。

## 9. 交接

- 见 `R05_HANDOFF.json`（接口、R06 可消费范围、延期登记、审查状态）。
