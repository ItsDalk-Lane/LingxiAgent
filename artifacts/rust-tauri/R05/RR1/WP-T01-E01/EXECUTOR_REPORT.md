# R05 RR1 WP-T01 执行者报告（F01/F02，第 1 轮）

- 执行者：R05-T01 修复执行智能体（首任，全新上下文），2026-10-04。
- 基线：HEAD `d80737b6cb9186c8a18c0f35923aac00249d45c3`（分支 `codex/rust-tauri-migration`，与冻结被审 HEAD 一致）；工具链 rustc/cargo 1.98.1（`/Users/study_superior/.cargo/bin/cargo`，仓库根 `rust-toolchain.toml` 锁定；本机 macOS arm64）。
- 未 commit/push；仅准备可审查候选。合成凭证（dummy-*/sk-test-*）+ 环回替身，无真实供应商请求。

## 1. 旧行为反例固化（先红后修）

反例迁移自证据包 `audit/credential_network/src/lib.rs`（CN-F03→F01、CN-F01→F02），落在已注册进 stage suite 的
`rust/crates/lingxi-service/tests/r05_t01_model_plane.rs`（新模块 `rr1_f01_f02`），调用签名/夹具适配当前代码，行为断言未削弱（其中两条按总控 §3.3 强化为更强断言）。

旧红（修复前候选，命令
`cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t01_model_plane rr1_f01_f02 -- --test-threads=2`，exit 101，
证据 `old-red-f01-f02.log`）：

| 测试 | 旧红事实（抵达边界） |
|---|---|
| f01_undeclared_tools_capability_makes_zero_requests | 无 tools 声明的模型携带合法工具声明，物理 HTTP=1（要求 0） |
| f01_undeclared_image_capability_makes_zero_requests | 无 imageInput 声明的模型携带主机授权图像，物理 HTTP=1（要求 0） |
| f02_reload_during_401_never_sends_new_key_to_old_endpoint | 屏障控制下 A 收到第 1 个请求（Bearer dummy-old-key）后 reload 为 B+新钥匙，A 的 401 重试请求头为 `authorization: Bearer dummy-new-key` |
| f02_old_route_never_resolves_new_generation_material | 旧代次 route 解析到新钥匙 `Ok(Bearer("dummy-new-key"))` |
| f01_control_no_capability_needs_still_dispatches（正常对照） | 通过：同夹具无能力需求的回合真实出线（HTTP=1，Final） |

## 2. 根因与修复

### F01（逐模型能力声明与发送前检查）
- 根因：`RouteBinding` 无能力承载；`gateway.rs` 只按协议族×操作类匹配；`ModelTurnInput` 的工具/图像需求在发送前无对照。
- 修复：
  - `config.rs`：`RouteBinding.capabilities: Option<RouteCapabilities>`（`tools`/`imageInput` 三态：Some(true)/Some(false)/None），deny_unknown_fields 保持闭合 schema。
  - `gateway.rs`：新增 `resolve_dispatch()` —— 单次快照读同时产出 route+compat+capabilities（同代次）；`resolve_route`（kernel trait）行为不变。
  - `provider.rs`：`next_turn_for_operation` 在凭证解析/派遣前做发送前检查——仅 `Some(true)` 授权；未声明或明确不支持 ⇒ 本地拒绝（ErrorCode::InvalidMessage、retryable=false、消息点名 provider/model/operation/能力与声明状态），物理 HTTP=0，不静默删工具/图片、不换 provider/模型。该点是全部 turn 形态操作（chat/六个辅助槽位/worker callback 经 AuxiliaryExecutor）的唯一咽喉。
- 语义边界：模型名不作能力数据库（证据包红线）；reasoning/上下文与输出预算/协议选项的承载仍是既有 `compat` 块（TS model 对应字段），不重复建设。

### F02（配置代次一致解析）
- 根因：`CredentialService::resolve/report_unauthorized` 只按 provider 名取当前材料，忽略 `route.config_generation`；管理面 reload 两步非原子；`compat_hints` 另读当前快照可跨代次。
- 修复：
  - `ProviderCell` 新增 `seed_epoch`（该 cell 种子所属的平面代次；构造=1，与网关初代一致）。`reload(&plane, config_generation)`：种子变了的 provider 重建 cell 并打上安装代次；种子未变的 cell 保留（材料未变，仍是当世代次材料，服务新旧两代 route）。
  - `resolve`/`report_unauthorized`：`seed_epoch > route.config_generation` ⇒ `CredentialError::StaleRoute`（安全失败，retryable=true，经 provider 映射为 UpstreamUnavailable）——新材料永不落入旧 route/旧端点；OAuth 世界内刷新不动 seed_epoch。
  - `management.rs`：凭据先行重播种并盖 `gateway.upcoming_generation()`（单一来源），再换网关快照——两个半开窗口都被关掉（先凭据⇒新 route 不可能遇到旧材料；代次校验⇒旧 route 拿不到新材料）。调换顺序本身不是修复，代次绑定才是。
  - `provider.rs` 改用 `resolve_dispatch` 冻结 compat（消除 compat_hints 跨代次读，F02 入口清单第 4 项）。
- 允许语义：种子未变的旧代次 route 继续用同一材料（C05 在途快照语义保留，`f02_kept_seed_serves_both_generations` 同时覆盖“只换 endpoint/模型”）。

## 3. 同类路径扫描

- `resolve_route` 生产调用者：provider.rs（已改 resolve_dispatch）、provider.rs descriptor()（诊断，无材料）、operations.rs:384（route+凭证背靠背解析，无缓存 route——代次校验天然适用）。
- `compat_hints` 生产调用者：仅 provider.rs:222（已消除）；方法保留并注明仅供诊断。
- worker/辅助：workermodel.rs→AuxiliaryExecutor::complete→next_turn_for_operation（同一咽喉）；aux 辅助回合工具快照为空、不触发 tools 检查；vision 槽位当前全仓无配置/使用（imageInput 检查由本包新增测试钉住）。
- `CredentialService::reload` 调用者：management.rs（生产，唯一）、credentials/mod.rs 内部测试、r05_t02_credentials.rs:1990（均随签名更新）。
- 残余登记（不在本包伪造关闭）：并发管理面 reload 相互竞态（两个 HTTP reload 交错）仍可能造成网关/凭据最后写者不一致——F02 清单未列该矩阵，已在问题矩阵 remaining 登记；operations.rs 将全部凭证错误映射为 Unauthorized/non-retryable（StaleRoute 语义在该层被压平），属 T06 域。

## 4. 既有夹具的合法前置补齐（保留被测故障）

按总控 §3.3（修好 F01 后为其他夹具补齐测试所需合法能力、保留被测故障），为驱动真实工具链的运行级夹具的 chat 绑定补
`"capabilities": {"tools": true}`：r05_t01_model_plane(c02/c04/plane_json_for；c06 ASR 族拒绝测试保持原样，不作能力证据)、r05_t01_binary_wiring(c02)、r05_t02_credentials(plane_api_key/plane_oauth/plane_oauth_plus_static/c01 内联/o1 内联)、r05_t03_protocol_adapters(12 处)、r05_t04_streaming(2)、r05_t06_operations(1)、r05_t06_worker_model(1)、r05_t07_persistence(2)、r05_t07_usage_trace(3)、r05_t08_closed_loop(1)。o1 重钉字符串替换模式随 capabilities 同步更新。辅助槽位/操作绑定不带工具快照，未加声明。

## 5. 自检证据（全部真实运行，退出码见日志）

| 命令 | 结果 |
|---|---|
| `cargo test …-p lingxi-service --test r05_t01_model_plane rr1_f01_f02 -- --test-threads=2`（修复前） | exit 101：4 红 + 对照通过（`old-red-f01-f02.log`） |
| 同上（修复后） | exit 0：16/16 通过（`green-f01-f02.log`） |
| `cargo test …-p lingxi-service --no-fail-fast` | 见 `service-suite-after-fixtures.log`：63 ok +1 FAIL（o1 字符串替换模式，已修并单测通过 exit 0）；r00_management_leaves 在该轮通过（139.6s；此前一轮曾因 macOS 防火墙拦截未签名测试二进制的入站连接在 web-auth/login 停滞——环境偶发，与本改动无关，无模型面参与） |
| `cargo fmt --all -- --check` | 干净 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 干净 |
| `cargo run …-p xtask -- check-contracts` / `check-boundaries` | exit 0 / exit 0 |
| `cargo test …--workspace --no-fail-fast` | `workspace-full.log`：104 个测试二进制 ok，仅 2 项环境偶发失败（`terminal_family_share_cases`：并行负载下 PTY 回显时序；`r00_management…`：macOS 防火墙间歇拦截未签名测试二进制入站连接，其 panic 自述环境原因）——两项均在同一候选上单独复跑通过（见 `workspace-flake-verification.md`：r04_t08 整套 10/10=301.77s、r00 单测通过=156.65s），无未解释产品失败 |

## 6. 共享文件改动记录（供总控仲裁与回归）

- `lingxi-adapters/src/models/provider.rs`（与 T02/T03 共享）：resolve_dispatch 化 + F01 检查 + StaleRoute 映射。回归：lingxi-adapters 全测 + lingxi-service 全测。
- `lingxi-service/src/management.rs`（与 T02 共享）：reload 代次接线。回归：r05_t01 C05/r00 管理叶（通过）。
- `lingxi-service/src/credentials/mod.rs`（T02 F03/F04 主域）：仅 resolve/report 段 + ProviderCell.seed_epoch + reload 签名；未触碰 handle/seed_cell 代次语义（F03 归 WP-T02）。
- `lingxi-adapters/src/models/credentials.rs`：CredentialError 新增 StaleRoute（加法）。
- kernel `model_exchange.rs`：未改（CapabilityUnsupported 既有变体保留未用；避免与 T03/F09 冲突）。

## 7. 未覆盖/移交项（如实）

- 独立验收（INDEPENDENT_PASS）待新审查智能体。
- F01 的 worker 槽位未单列“未声明即拒绝”专测（与辅助槽位共用同一咽喉，由 f01_same_model_id…（title 拒绝）+ 既有 worker 链路测试共同覆盖）；如审查要求可按同模式补一条。
- 正式二进制 closed-loop Python probe（证据包 `audit/closed_loop`）的配置需在 T08 重跑时补 capabilities（其配置不在仓库测试内）。
- R05 交付文档（PROVIDER_SUPPORT_MATRIX/INTERFACE_EVOLUTION 等）的 schema 叙述更新归 F28 收口。
