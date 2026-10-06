# R05 RR1 WP-T07 执行者报告（R1，F21/F22/F23）

- 执行者：R05-T07-执行者（首任，全新上下文，2026-10-05/06）
- 工具链：全部 cargo/rustc 调用经 rustup 代理绝对路径 `/Users/study_superior/.cargo/bin/cargo`（实测解析 1.98.1，仓库根 rust-toolchain.toml 锁定）；未使用 PATH/Homebrew cargo；无依赖增删、无 lock 改动。
- 边界遵守：未 commit/push；无真实供应商/OAuth/用户 home 访问；全部合成凭证（RR1_T07_*、AUDIT_ONLY_* 沿用原探针命名）；未建第二套权威。

## 1. 旧行为反例（先红，修复前、当前候选树）

原审计探针 P4/P5/P6/P7/P8 的断言原文迁移为仓库永久测试，修复前实跑：

| 反例 | 迁移位置 | 修复前结果 |
|---|---|---|
| P4 worker 模型 500 → HTTP1、usage 0 行 | `lingxi-service/tests/r05_t07_rr1_usage_ledger.rs::rr1_f21_worker_failed_physical_request_must_leave_unknown_usage_row` | RED：provider_requests=1, usage_rows=0（必须 1） |
| P5 畸形 usage 数组含凭证 → 永久 invalid_detail | `…::rr1_f22_operation_invalid_usage_must_not_persist_secret_payload` | RED：leaked=true（invalid_detail 含 Bearer RR1_T07_AUDIT_ONLY_EMBEDDING_CREDENTIAL） |
| P6 Gemini 100/10/40/150 billed=10 | `lingxi-adapters/tests/r05_t07_rr1_usage_strict.rs::rr1_f23_gemini_thoughts_are_separate_from_candidate_tokens` | RED：billed=10（必须 50） |
| P7 rerank null/"7" → Reported 0/7 | `…::rr1_f22_rerank_non_numeric_usage_must_not_be_coerced_to_reported_zero` | RED：Reported(0,7)（必须 Invalid） |
| P8 queue-timeout HTTP0 attempts=1（attempts 半边） | `…ledger.rs::rr1_f21_operation_queue_timeout_never_invents_an_http_attempt` | RED：rows=0（必须 attempts=0 行；deadline 半边 F16 已修，elapsed=28ms<120 已绿） |
| P6 组件矩阵 thoughts>candidate | `…::rr1_f23_gemini_component_matrix_and_family_controls` | RED：billed=3（必须 100）；无 thoughts/OpenAI/Anthropic 对照绿 |
| P5' invalid_detail 回显（unit 级） | `…::rr1_f22_invalid_detail_never_echoes_the_payload` | RED：detail 含完整 payload |
| MiniMax total_tokens 强转 | `…::rr1_f22_minimax_total_tokens_keeps_its_raw_type` | RED："41"→Reported（必须 Invalid） |

日志：`old-red-adapters-strict.log`（exit=101，5 红/2 对照绿）、`old-red-service-ledger.log`（exit=101，3 红）。service 探针以修复前签名（5 参 complete/2 参 embed）编译运行，随后按接口演进切换到修复后签名（迁移规则允许调用签名适配，断言原文未动）。

## 2. 修复内容

### F23（Gemini thoughts/candidate 口径）
- 统一 output 定义=**总生成输出**：`decode_family_usage` 对 Google 归一 `output_tokens = candidatesTokenCount + thoughtsTokenCount`（saturating；thoughts 缺失→candidates；candidates 缺失→thoughts）；`reasoning_tokens` 保留 thoughts 分量；`reasoning_included_in_output=Some(true)` 澄清为统一事实口径（与 OpenAI 同一约定：计费输出=output_tokens，不漏 thoughts 也不再相加）。
- 流式回写 round-trip：`GenerateStreamAccumulator::finish` 把 fold（统一值）投影回 wire 形状时按 `output - reasoning` 拆回候选字段，重解码恰好还原 fold——修复中实证的双重归一坑（26+7=33）已消除。
- 文档/测试同步：`models/usage.rs` 模块注释、`MODEL_USAGE_SEMANTICS.md §2`（Google 口径修订段）、google golden（19→574=19+555）、`r05_t07_usage_families.rs` c07/c04 Google 腿（26=19+7）、google lib 单测（9=6+3）。

### F22（usage 严格解析 + 诊断不泄漏）
- `normalize_rerank_usage`：形状选择器——原始值原样透传（null=缺失、字符串/浮点/容器=Invalid）；`total_tokens` 只在两半都是真非负整数时合成；`usage.total_tokens` 分支原样透传。
- MiniMax `total_tokens` 原样透传（不经 f64；9007199254740993 精确）。
- `decode_operation_usage` 非对象诊断：类型+序列化字节数，绝不回显 payload（构造即有界）；非法数值诊断沿用既有字段路径+类型/数值模板。
- 同路径扫描结论：embedding/rerank 是仅有的数值转换点（transcribe 无 token usage、image/video 按项计费、其余 embedding 家族早已原样透传）。

### F21（调用事实、父子归属、查询面）
- **not-sent**：`ProviderTurnResult::mark_not_sent()`（ports.rs）；provider.rs 五个 pre-dispatch 拒绝位（gateway 解析/逐模型能力/凭证 resolve/无 adapter）记 attempts=0；401 刷新/重试路径保持 1/2。
- **失败不漏账**：`AuxiliaryFailure` 携带完整 settle 事实；`GatewayWorkerModel::complete` 成功/失败/quota 拒绝全路径先落台账行再回话（quota 拒绝 provider/model='unreported'，不虚构路由；预算拒绝是主事实、台账失败附注）。
- **operation**：`OperationCallContext`（session/run/attempt/cause_ref）可选参数；admit 超时先落 attempts=0 行（outcome=failed、usage unknown）。
- **父子 JOIN**：`WorkerModelPort::complete` +`parent_tool_call` 参数（真实 ToolCallId 穿过 RPC）；台账 v6 迁移（ALTER 追加 outcome/started_at/settled_at/parent_tool_call_id/emitted_tool_calls）；driver 在 ToolRequests 回合把批次 tool ids 写入父 model 行——child→parent tool→parent MODEL 两跳 JOIN 全在台账内闭合。
- **查询面**：`ModelUsageQuery` +purpose/model/recorded_from/to（四种 scope 组合）；返回行携带全部新列。

## 3. 自检（亲自运行，命令原文）

- `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --test r05_t07_rr1_usage_strict` → 7/7 绿
- `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --test r05_t07_usage_families` → 14/14 绿
- `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t07_rr1_usage_ledger` → 8/8 绿（`green-service-ledger.log`，exit=0；含最终补入的"空正文有usage"腿：usage 21/8 随失败行存活）
- `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters` → 全量绿（含 golden round-trip、migration idempotency）
- `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-kernel` → 全量绿
- `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --no-fail-fast` → `regression-lingxi-service-full.log`：**899 通过 / 67 套件 ok / 0 产品失败**；唯一失败 r00_management 为既有 macOS ALF 环境项：非环回自地址 192.168.3.5 入站被本机防火墙拦截（panic 文本自证环境归因，与 T03-T06 记录同一项；cargo exit=101 仅因该测试二进制）
- `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` → exit 0
- `cargo clippy --manifest-path rust/Cargo.toml --locked --workspace --all-targets -- -D warnings` → exit 0

## 4. 关键测试说明

- **真实 worker 子进程链 JOIN**（`rr1_f21_worker_parent_join_through_the_real_subprocess_chain`）：真实 run driver（RunSupervisor+GatewayedProvider）→ stub chat 回 tool_call（wire name）→ 真实 ToolInvocationGateway → `r04_t07_fixture` 子进程（ask_model）→ 回调经真实 aux 链 → 生产 `LedgerWorkerCallbackTrace` 落行。断言：stub 物理 3 请求；child.parent_tool_call_id == driver 铸造的 `{run}-tc0001`；恰一父 model 行的 emitted_tool_calls 认领该 id；child usage=aux stub 报告的 17/5；timing/outcome 齐备。非测试端伪造父 id。
- 既有测试仅机械适配新签名（WorkerModelPort 5 个 impl+全部调用点、ModelUsageQuery 字面量 `..Default::default()`、embed/rerank 第三参）与 F23 新口径；行为断言未削弱（被更新的 Google 期望值来自官方语义而非实现反推）。

## 5. 登记的边界（不虚标）

1. driver 在 model call 流中被取消的窗口仍无完成行（§5/§10 既有登记）；worker/operation 面的 pre-send 拒绝有行（attempts=0）。
2. 媒体（image/video/speech/transcribe）行带 outcome 但 started/settled=NULL（R07 业务面落地时补观测）。
3. 正式组合根的 worker 工具注册归 F24（本包 JOIN 经真实子进程链+真实 driver 证明）。
4. `WorkerModelPort::complete` 签名演进已同步所有实现与调用点；接口演进条目归 F28 收口统一（sharedFileLedger 已登记）。
