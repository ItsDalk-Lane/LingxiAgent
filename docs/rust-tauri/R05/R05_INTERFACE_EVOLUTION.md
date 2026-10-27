# R05 接口演进与消费者同步记录（T01 / T02）

日期：2026-10-02。任务：R05-T01 真实模型提供方接入（openai-completions 全链）。
本文档记录 T01 对既有契约的**每一处**接口演进、演进理由、全部消费者的同步方式。
基线 HEAD：`c549ff654508ab951e2cf39cf9d309fc9c6b8656`（改动未提交，按授权边界只落工作树）。

## 1. 设计裁决（实现前锁定）

1. `TurnProviderPort::next_turn(ctx, call, input: &ModelTurnInput)` —— 删除任务书草案的
   `turn: u32, input: &str` 形态；`turn` 语义并入 `ModelTurnInput`/驱动侧（跨 attempt 单调计数），
   自由文本升级为类型化输入。
2. `ProviderTurn::ToolRequests { requests, content: Vec<ContentBlock> }` —— 工具请求轮携带模型
   同轮输出的内容块（T03 混合内容保真的前置），不再只有 requests。
3. `ProviderTurnResult + usage: Option<UsageRecord> + served_by: Option<ProviderDescriptor>`。
   构造器：`of_ctx`（两者皆 None）/ `of_ctx_with_usage` / `with_served_by`。
   - usage：无上报 = 持久化 `None`，绝不编造 0。
   - served_by：**实际服务路由身份**。驱动每轮取 `provider_result.served_by.unwrap_or_else(descriptor)`，
     防止 reload 后把旧路由的调用错标到新身份（驱动不再一次性绑定 descriptor）。
4. `ToolRequest + provider_call_id: Option<String>` + `with_provider_call_id()` —— 协议关联值
   与内核 `ToolCallId` 分层（外部 callId 只是关联值，C10）。
5. 无模型配置走既有 `completed.no_final.no_provider_configured` 早终路径，不新增第二套语义。
6. workspace 只来自 `--config` 文件的 `workspace` 键；不存在隐式 cwd workspace。
7. T01 生产接线只接 file tools（read/write/edit）；process tools 不接（台账如实记录）。
8. 能力拒绝（embedding/rerank/ASR/媒体生成/未实现协议族）在**最早点**响拒，零网络请求。
9. pin 成对规则：provider+model 是一个身份单元；只 pin model 不 pin provider = 响拒
   （`ModelPinRequiresProvider`），绝不从 model id 猜 provider。
10. 凭证材质只在 adapters 侧；kernel 只见 `CredentialReference`（身份+种类）。reload 原子换代次。

## 2. kernel：`model_exchange.rs`（新模块）

`lingxi-kernel/src/model_exchange.rs`（`lib.rs` 增 `pub mod model_exchange;`）：

- `ModelOperation`：Chat / Auxiliary(AuxiliarySlot) / Embedding / Rerank / SpeechRecognition /
  MediaGeneration(MediaGenerationKind)；`describe()`。
- `AuxiliarySlot`：Title/Summarize/Memory/Vision/Approval/Guard；`config_key()`/`describe()`。
- `ProtocolFamily`：7 变体（OpenAiCompletions/AnthropicMessages/GoogleGenerativeAi/
  OpenAiResponses/OpenAiCodexResponses/VolcengineBigAsr/SystemSpeech）；
  `config_name()`/`parse()`。
- `CredentialAuthKind`（ApiKey/None）、`CredentialReference`（provider + auth kind，无材质）。
- `ResolvedModelRoute`：provider+model+operation+protocol+endpoint+credential 引用+`config_generation`。
- `ModelRouteRequest`：operation + 可选 pin 对。
- `ModelGatewayError`：RouteNotConfigured / UnknownProvider / ModelPinRequiresProvider /
  CapabilityUnsupported / OperationUnsupportedByProvider / MissingCredential / ProtocolNotImplemented。
- `ModelGatewayPort`：`resolve_route` + `config_generation`。
- `ModelTurnInput`：`first_turn(submission, tools)`；`submission` + `prior: Vec<ExchangeItem>` +
  `tools: ToolDeclarationSnapshot`。
- `ExchangeItem`：`AssistantTurn { call, content, tool_calls }` / `ToolResult { tool_call_id,
  provider_call_id, outcome }`。
- `RequestedToolCall`：内核 ToolCallId + provider_call_id + target + EffectiveArguments + digest。
- `ToolDeclaration` / `ToolDeclarationSnapshot`：`empty()` / `from_registry` /
  `target_of_wire_name` / `wire_name_of_target`；wire 名冲突响拒
  （`ToolDeclarationError::WireNameConflict`）；同名冲突按序升级为命名空间名，仍冲突即错误。

## 3. kernel：`ports.rs` 演进

- `TurnProviderPort::next_turn` 签名换成 `input: &ModelTurnInput`（裁决 1）。
- `ProviderTurn::ToolRequests` 增 `content: Vec<ContentBlock>`（裁决 2）。
- `ProviderTurnResult` 增 `usage`/`served_by` 字段与三个构造器（裁决 3）。
- `ToolRequest` 增 `provider_call_id: Option<String>` 与 `with_provider_call_id()`（裁决 4）。
- imports：`ContentBlock`、`UsageRecord`（`lingxi-protocol`）、`crate::model_exchange::ModelTurnInput`。

## 4. 消费者同步

### 4.1 生产消费者（手工）

| 文件 | 同步内容 |
|---|---|
| `lingxi-service/src/runs.rs` | 驱动循环持有 `exchange: Vec<ExchangeItem>`；`record_tool_result!(call_id, request, outcome)` 宏记录全部 8 个工具出口（取消路径 drop 点不记录）；ToolRequests 臂先推 AssistantTurn 再逐个执行；Continue 臂推 reasoning AssistantTurn；Failed 重试臂 `exchange.clear()`；每 turn 从 `tool_gateway.registry()` 现建 ToolDeclarationSnapshot（无 gateway = empty）；`persist_model_call` 加 `usage` 参；served_by 每轮取自结果。 |
| `lingxi-service/src/toolgateway.rs` | 内部 `ToolRequest` 字面量补 `provider_call_id: None`。 |
| `lingxi-kernel/src/toolcatalog.rs:1568` | `into_tool_request` 补 `provider_call_id: None`。 |

### 4.2 测试消费者（30 文件 codemod + 手工 5 处）

`docs/rust-tauri/R05/r05_t01_codemod_next_turn.py` 机械改写 30 个测试文件的
`next_turn` 实现/调用点；随后手工修 5 处 codemod 无法安全覆盖的：
`tool_receipt_unknown.rs`（双 content）、`r03_t08_acceptance_matrix.rs`（裸 `ToolRequests{`）、
`recovery_crash_points.rs` / `exit_race_rejections.rs`（`_input` → `input`）、
`late_result_fence.rs`（两个 `ProviderTurnResult` 字面量补 `usage`/`served_by`）。

## 5. adapters：`models/` 模块（新）

- `config.rs`：`ModelPlaneConfig` / `ProviderConfig` / `AuthConfig`（`tag="kind"`：
  `apiKey` / `none`；缺 `auth` 键 = 模式错误，C11）/ `ModelsSection`（chat + 6 辅助槽位，
  `deny_unknown_fields`）/ `RouteBinding` / `ModelConfigError`（7 变体）/
  `parse_and_validate` / `from_value` / `validate`（协议族词汇、端点形状、空 key、
  路由 provider 存在性、空 model，全部装载期响拒）。
- `gateway.rs`：`ConfigModelGateway`（`Arc<RwLock<Arc<GatewaySnapshot>>>`），
  `from_validated`（代次 1）/ `reload`（原子换快照、代次 +1）/ `provider_auth`（材质出口，
  仅 adapters 侧）/ `provider_ids`；`ModelGatewayPort` 实现（能力拒绝对最早、pin 成对规则、
  非 openai-completions → `ProtocolNotImplemented`）。
- `openai_completions.rs`：`OpenAiCompletionsAdapter::execute_chat` + 纯函数
  `render_chat_request` / `parse_chat_response`。HTTP 状态映射：401/403→Unauthorized(不重试)、
  408/5xx→UpstreamUnavailable(重试)、429→BudgetExceeded(重试)、400/404/422→InvalidMessage。
  响应身份字段（`id`/`model`）全忽略（C10）；usage 半报 = None；工具结果/取消/未知四态
  诚实渲染；历史轮目标不在当前快照 = 响拒（不重命名猜测）。
- `provider.rs`：`GatewayedProvider`（生产 `TurnProviderPort`）：发送时解析路由（reload 下一轮
  生效，C05）；解析失败 = 响亮 `ProviderTurn::Failed`（非 retryable）；凭证在解析与取用之间消失
  = retryable 响亮错误；descriptor 在解析不到时返回 `<unconfigured>` 标记而非虚构身份。
- `Cargo.toml`：`reqwest = { version = "0.13.5", features = ["json"] }`（锁文件只增依赖边；
  DEP-08 只禁 kernel，adapters 是 HTTP 的合法归属层）。

## 6. service：配置/组合根/管理面

- `config.rs`：闭集扩为 `{home(必), workspace?, providers?, models?}`；
  `ServiceConfigFile{home, workspace, model_plane}` + `read_service_config`；
  `read_config_home` 成为其包装（既有 6 个严格性钉死测试全绿，只断言变体）；
  `ModelPlaneSource{EmbeddedInConfigFile, ModelsFile}` + `resolve_model_plane(config, runtime_dir)`
  —— models.json = `runtime_dir/models.json`，缺失 = None（C03 显式未配置态），存在但坏 =
  响亮 ConfigFileUnreadable/Invalid；两源同在 = 响亮歧义错误（绝不猜优先级）。
- `lib.rs`：`ServiceDeps + model_gateway / model_plane_source / workspace_root`
  （Default 全 None）；`ServiceState + model_gateway / model_plane_source` 及访问器；
  `bootstrap_with_instance_identity` 在 `RunSupervisor::new` 前：当 `turn_provider.is_none()
  && model_gateway.is_some()` 时建 ToolRegistry + ApprovalService + ToolInvocationGateway
  （DEFAULT_PREPARED_TTL_MS / DEFAULT_LIVE_PREPARED_CAP）+ 有 workspace 则
  `ResourceAccess::new` + `filetools::register_core_file_tools` + `GatewayedProvider`，
  并替换 `recovery_capabilities = RegistryCapabilities(registry)`、approval_gate、
  tool_gateway、turn_provider。注入的测试替身永远优先；无模型平面时逐字节保持 R03/R04 默认形态。
- `main.rs`：layout 之后 `resolve_model_plane(cli.config, &layout.runtime_dir)`，Some 时填 deps
  三字段（workspace 由 `read_service_config` 重读 --config 取得）；USAGE 文本同步闭集。
- `management.rs`：`POST /lingxi/v1/models/reload`（LocalUser 限定）—— 按登记的 source
  重读重校验，`gateway.reload` 原子换代次返回 `{ok, generation}`；源失效/非法 = 409 响亮错误
  （运行中快照绝不被清空或半换）；无平面 = 404 `models.unconfigured`；审计日志记
  `models.reload` + 代次。
- `redaction.rs`：**修复 R02 时代真实缺陷**——`replace_each_ci` 的直接函数传参把小写化 haystack
  与原文对调，导致混合大小写键（`apiKey`/`Password`/`TOKEN`）逃出赋值/查询/头扫描（且被替换头
  变成小写）。R05 复核（任务书 divergence-#1 复核义务）发现并按闭包约定修复全部 5 处调用点；
  新增 `r05_secret_key_words_match_case_insensitively` 与 `r05_provider_api_key_shapes_are_redacted`
  钉死（含 base64 带 `/`/`=` 的密钥经赋值规则覆盖）。

## 7. 刻意未做（诚实清单）

- process tools（exec_command/write_stdin）不进生产接线（裁决 7；T01 只接 file tools）。
- 流式解析、取消/重试细化、辅助/Worker 实际调用入口、用量聚合面：T04/T05/T06/T07 范围。
- 协议族 anthropic-messages 等 6 族：词汇已冻结、路由可解析、派发响亮 `ProtocolNotImplemented`
  （T03 实现真实编码解码）。
- `models.json` 独立源与内嵌源同时存在 = 错误（不合并、不猜优先级）。

---

# R05-T02 接口演进与消费者同步记录

日期：2026-10-02。任务：R05-T02 凭证、刷新、撤销与秘密保护。
基线：T01 工作树之上（仍未提交，同一授权边界）。

## 8. 设计裁决（实现前锁定）

1. **AuthConfig 四形态**：`apiKey` / `authHeader`（静态种子，config 为权威）/ `oauth`（流程描述符，
   token 材料只进凭证存储）/ `none`。
2. **OAuth 描述符词汇**：`{flow: "authorizationCodePkce"|"deviceCode", clientId, tokenEndpoint,
   authorizeEndpoint?, deviceAuthorizationEndpoint?, scopes?}`；endpoint 校验拒 userinfo。
3. **凭证服务** `CredentialService`（service crate `src/credentials/{mod,store}.rs`）是唯一材料出口；
   adapters 侧 port 为 `ProviderCredentialPort`（`resolve` + `report_unauthorized`）。
   gateway 的 `provider_auth()` 材料出口**删除**（C01）。
4. **401 协调**：每 provider 单 flight（cell: generation + material + Option<flight>）；flight 是
   spawned task（最后离开者不 abort transport，30s 请求超时收束）；waiter 用
   `watch::Receiver`，cancel-safe（C04）；写回双重 fence（map 现势 + cell generation，C05）；
   先持久化后换内存，persist 失败 → `Refreshed{persisted:false}` 如实上报（C06）；
   现任优先（used ≠ current → 直接 Refreshed 不刷新）。
5. **撤销**：先删 store 行（失败则 PersistenceFailed、内存不动）再翻内存 Revoked + generation bump；
   静态 key 重启后从 config 重播种（文档化语义），OAuth 重启后 not_logged_in。
6. **存储**：`{runtime_dir}/credentials.json` v1；未知字段（顶层+每 provider）经 `#[serde(flatten)]`
   原样保留；版本不符响亮拒绝；`atomic_write_private`；`StoreIo` trait 供故障注入；锁序恒 cell→store。
7. **handle（C12）**：128-bit getrandom id 绑定 (provider, generation, 60s expiry)，registry 为权威
   （伪造/跨 provider/过期/stale generation/篡改 expiry 全拒）；`resolve_handle` 不触发刷新。
8. **状态码**：401→Unauthorized（可触发一次刷新重试），403→Forbidden（不刷新）；chat 与 OAuth
   client 均 `redirect::Policy::none()`，3xx 响亮失败且不回显 Location 目标（C10）。

## 9. kernel 演进

- `model_exchange.rs`：`CredentialAuthKind` 扩 `AuthHeader` / `OAuth`（原 ApiKey/None 保留）。
  kernel 仍然只见 `CredentialReference`（身份+种类），不见材料。

## 10. adapters 演进

- `models/credentials.rs`（新）：`ApplicableAuth`（None/Bearer/Header）、`CredentialError`（8 变体
  → REV-T02 R01 F-04 删除零构造的 `Cancelled` 后 7 变体，见 §13；
  文本不含材料）、`RefreshVerdict`（Refreshed{generation,persisted}/Revoked/ReauthorizationRequired/
  Transient/NotRefreshable）、`ProviderCredentialPort`、`scrub_materials`（精确匹配首选防线）。
- `models/config.rs`：`AuthConfig` 四形态 + 校验（header RFC7230+framing 黑名单、oauth 描述符、
  endpoint userinfo 拒收）；补 `PartialEq, Eq`（reload 的 seed 比较）。
- `models/oauth.rs`（新）：`OAuthHttp`（no-redirect）、`FlowClock`（Send+Sync 注入）、
  device-code 状态机（pending/slow_down/期限/取消）、authorization-code+PKCE（S256、一次性 state、
  回调 listener）、`refresh` 分类（invalid_grant/401→Reauth；429/5xx/超时→Transient；
  协议违反→Reauth 不自动重试）；错误文本经 `scrub_materials`；`CallbackGrant` 手工 Debug 不渲材料。
- `models/gateway.rs`：删除 `provider_auth()`；auth_kind 映射补 AuthHeader/OAuth。
- `models/openai_completions.rs`：`execute_chat` 签名改 `auth: &ApplicableAuth`；no-redirect；
  错误 excerpt 先 scrub 在-play 材料；401/403 分码；3xx 响亮拒绝。
- `models/provider.rs`（重写）：`GatewayedProvider::new(gateway, credentials, budget)`；
  401 且 auth≠None → report_unauthorized → Refreshed 时恰好重试一次（C07 无环）。

## 11. service 演进

- `src/credentials/store.rs`（新）：`CredentialStore`（单写者、persist-first、`StoreIo`/`FsStoreIo`、
  版本冲突/格式错误响亮、未知字段保留）。
- `src/credentials/mod.rs`（新）：`CredentialService`（resolve/report_unauthorized/revoke/reload/
  status/mint_handle/resolve_handle + `ProviderCredentialPort` impl）、`ProductionRefreshDriver`、
  `CredentialHandle`、`ProviderCredentialStatus`。
- `lib.rs`：`ServiceDeps`/`ServiceState` 加 `credential_service`；bootstrap 不变量（model_gateway 有且
  turn_provider 未注入 → credential_service 必须存在，否则响亮启动错误）。
- `main.rs`：model plane 存在时 `CredentialService::bootstrap`，失败 exit 2。
- `management.rs`：`GET /lingxi/v1/models/credentials`（无材料状态）、
  `POST /lingxi/v1/models/credentials/{provider}/revoke`，仅 LocalUser；`reload_models` 先
  `credentials.reload(&plane)` 后 `gateway.reload(plane)`；revoke 审计进 management.json。
- `redaction.rs`：`is_token_char` 补 `/` 和 `=`（对齐 incumbent `[A-Za-z0-9+/_=-]`；
  RR-R05-REDACTION-BOUNDARY 修复），pin 测试 `r05_t02_base64_tokens_with_slash_and_padding_are_redacted`。

## 12. 消费者同步与遗留

- `GatewayedProvider::new` 签名变更的唯一生产调用点是 `lib.rs` bootstrap；T01 测试套件
  （`r05_t01_model_plane.rs`）的 boot helper 补 `credential_service`（真实 bootstrap）。
- kernel `ports.rs` 的 `CredentialPort` / `CredentialHandle`（约 1164 行）为 R00 时代遗留 trait，
  现役无人实现/调用；T02 的新凭证语义不走该 trait（它是同步形态、无刷新协调），保持不动，
  后续阶段如需统一再行裁定。
- `OAuthFlowTransport` trait 未引入（已向总控如实汇报）：OAuth 走标准 reqwest，独立会话/非共享
  client 归 T03。

## 13. REV-T02 R01 修复轮（2026-10-02）

独立审查 REV-T02 R01（`artifacts/rust-tauri/R05/REVIEW-T02/R01_REVIEW.md`）判 PENDING，
四项发现的修复：

- **F-01（必修）`resolve_impl` 刷新循环无界**：`Refreshed => continue` 无计数，token 端持续
  签发立即过期 token 时 resolve 不终止（审查探针实测 3 秒 265,137 次驱动调用）。修复：每次
  resolve 至多等待一个刷新 flight——`flight_awaited` 守卫，刷新后重读仍过期 →
  `CredentialError::Transient`（"did not converge"），绝不静默再刷。全 workspace 同类
  `continue` 路径已排查：scrub 数组遍历、device 轮询（期限收束）、PKCE accept（state 过期
  收束）、消息遍历均有界；只此一处缺陷。回归测试：
  `resolve_refreshes_at_most_once_when_every_mint_is_already_expired`（expires_in 截断为 0
  形态）、`resolve_refreshes_at_most_once_when_a_one_second_grant_dies_in_transit`
  （expires_in:1 往返超期形态，驱动内推钟确定性构造）；mint 侧语义 pin 在
  `r05_t02_oauth_flows::sub_second_expires_in_mints_an_immediately_expired_token`（0.4s →
  expires_at == now）。
- **F-02（同修）`install_tokens` fence-1 TOCTOU**：map 现势检查原在 std 读锁内完成即释放、
  cell 锁后才获取，reload 换 cell 可穿插。修复：fence-1 移入 cell 锁内（锁内复查 map 现势，
  再查代次）；死锁安全性注释论证（任何路径都不会持 map 锁等 cell 锁：reload 先快照即释放，
  写段纯换 map 不碰 cell）。回归测试 `a_reload_replacing_the_cell_discards_the_late_writeback`
  （确定性终态 pin：迟到写回被 Discarded、store 保持旧值、新 cell 无刷新痕迹；TOCTOU 窗口
  本身为调度竞态，以锁内复查根除，源码论证在 `install_tokens` 注释）。
- **F-03（文档）** kernel `ports.rs` 遗留 `CredentialPort` 加 `#[deprecated(note)]` +
  归属指针注释（指向 `lingxi-adapters` 的 `ProviderCredentialPort`）；`CredentialHandle`
  加指针注释（其 struct 本体不标 deprecated——trait 定义体签名引用会触发警告，以 trait 的
  `allow(deprecated)` 注释说明）。不删，属 T08 裁定。
- **F-04（清理）** `CredentialError::Cancelled` 从未构造（取消语义由 future drop 表达，
  C04 已验证）——删除变体及其 `provider()`/`Display`/`credential_failure` 三个匹配腿，
  `credential_failure` 注释同步更新。`CredentialError` 现为 7 变体。补
  `an_expired_handle_is_refused`（resolve_handle 时间过期腿，C12 补齐）。

测试面：service lib 330→334、credentials 套件 16/16 不变、oauth 套件 19→20。

---

# R05-T03 接口演进与消费者同步记录

日期：2026-10-02。任务：R05-T03 实现实际使用的文本协议——anthropic-messages /
google-generative-ai / openai-responses / openai-codex-responses 四族真实适配，
openai-completions 补流式解码与 C07 全态工具结果渲染，驱动重试语义按 C12 修正。
基线：T01+T02 工作树之上（仍未提交，同一授权边界）。无新增第三方依赖。

## 14. 设计裁决（实现前锁定）

1. **生产路径一律 buffered**：四个新族的生产 `execute_chat` 全部走非流式请求
   （anthropic 体 `stream:false`、google `:generateContent`、openai-responses `stream:false`）。
   SSE 解码器（`streaming.rs`）与 openai-completions / openai-codex-responses 的流式聚合
   已实现并被 golden 钉死（golden 以 `wire_mode: "sse"` 标记），但生产切流式归 T04。
2. **`ModelTurnInput.system_prompt: Option<String>`**（kernel 新增字段）：host
   （`runs.rs`）现恒 `None`——现役 `callText` 从不声明 system，不为协议族虚构人格文本；
   golden/单测走显式 Some。codex 族在 `None` 时回落到现役常量 instructions
   （`openai_codex_responses.rs` 内 "You are Hana's utility model.…"，与 incumbent 逐字一致）。
3. **anthropic `max_tokens` 是协议必填**：适配器 pin `DEFAULT_MAX_OUTPUT_TOKENS = 16384`
   （常量注释说明理由）。每模型上下文/输出上限的注册缺口已在 PROTOCOL_WIRE_MATRIX 登记，
   归 T05/T07 裁定。
4. **google 族无 callId**：functionResponse 按 ExchangeItem 的名字+顺序配对
   （`tool_roundtrip_pairs_function_responses_by_exchange_name`）；对不上 = 响亮渲染失败
   （`an_unpairable_tool_result_is_a_loud_render_failure`），绝不按猜测配对。
5. **responses/codex 无服务端会话状态**：请求从不携带 `previous_response_id` 等服务端引用，
   codex pin `store:false, stream:true`。跨 provider 的旧引用按构造无处可发；
   不支持服务端状态恢复的族明确失败而非伪造引用（C11）。
6. **opaque 原位往返**：anthropic thinking/redacted_thinking 签名、google thoughtSignature、
   responses 加密 reasoning 项 → `ContentBlock::Opaque{provider, data}` 字节级保真；
   渲染回写本族原协议位置与顺序；**异族 opaque 一律不回显**（渲染时跳过，C10），
   从不渲成正文。
7. **C12 重试语义修正**：`runs.rs` 可重试模型失败臂不再 `exchange.clear()`——重试从
   已确认交换继续，已确认工具绝不重做（原 T01 行为会在重试后从原始用户输入重建交换，
   违反 C12）。不在 adapter 新建私有 loop。
8. **错误分类与 HTTP 公共件共享**：`dispatch.rs` 提供
   `build_client`（no-redirect）/ `apply_auth` / `append_provider_api_path` /
   `scrubbed_excerpt` / `classify_error_status`（401→Unauthorized 不重试、403→Forbidden、
   408/5xx→UpstreamUnavailable 可重试、429→BudgetExceeded 可重试、400/404/422→InvalidMessage、
   3xx 响亮拒绝），四族+openai-completions 共用，不各写一份。
9. **凭证头映射**：apiKey→`Authorization: Bearer`；authHeader→逐字 `{name}: {value}`；
   none→零凭证头（ollama 形状，service 套件 c07 钉死）；oauth 材质仍只经 T02
   `ProviderCredentialPort` 出口，本任务不动凭证语义。

## 15. kernel 演进

- `model_exchange.rs`：`ModelTurnInput` 增 `system_prompt: Option<String>`（裁决 2）；
  `first_turn` 构造器置 None。无其他 kernel 变更。

## 16. adapters 演进（`models/` 模块）

- `streaming.rs`（新）：`SseDecoder` 增量 SSE 解码（任意分块、多行 data 拼接、注释/id/retry
  忽略、EOF 未终结事件丢弃并如实上报、非法 UTF-8 响亮、缓冲上限响亮、首个 BOM 剥离一次）。
- `tool_render.rs`（新）：四态工具结果共享渲染——成功（resource_refs、`ToolRunStatus`
  Exited 非零码/Running/StopUnconfirmed、truncated 尾注全携带）/Failed/Cancelled/Unknown，
  T01 词汇保持不变（C07）。
- `anthropic_messages.rs`（新）：`render_chat_request`（system/tools/messages 形状、
  tool_use/tool_result 配对、thinking+签名与 redacted_thinking 原位回写、并行调用合并进
  一条 user tool_result 消息）/ `parse_chat_response`（内容块保序、stop_reason→Empty detail、
  usage 半报=None）；未知工具名/畸形块响亮。
- `google_generative_ai.rs`（新）：`systemInstruction`/`functionDeclarations` 形状、
  model id 百分号编码进路径、functionCall/functionResponse 按 exchange 名字配对、
  thoughtSignature/thought 保序解析。
- `openai_responses.rs`（新）：`instructions`/`input`/`tools` 形状、function_call 配对、
  reasoning 项逐字往返（summary→Reasoning 内容块）、`status:"failed"`=诚实失败绝不空轮、
  流式终态事件携带权威聚合。
- `openai_codex_responses.rs`（新）：现役 URL 规则逐字（`resolve_codex_responses_url`）、
  JWT `account_id` 提取或响亮失败、`store:false`/`stream:true`/常量 instructions 钉死、
  `parse_codex_response`/`parse_codex_stream` 公开供 golden 与服务套件复用。
- `openai_completions.rs`（演进）：换用 `tool_render` 共享渲染（C07 全态）；
  `system_prompt` 渲为首条 system 消息；流式解码+聚合（文本/usage/tool_call 片段/reasoning
  保序、multi-choice 与缺 id 响亮、必须见 [DONE]）；既有 T01/T02 语义不动。
- `gateway.rs`：四族放行真实适配（volcengine-bigasr/system-speech 仍响拒归 T06）；
  `provider.rs`：五族持有+分发（裁决 1 的 buffered 生产路径）。

## 17. service 演进

- `runs.rs`：C12 修正（裁决 7）——retry 臂保留 `exchange`；`turn_input` 构造补
  `system_prompt: None` 并注释说明（host 不虚构 system）。无其他 service 生产改动。

## 18. 消费者同步与遗留

- golden 15 个（5 族 × forward/tool_roundtrip/error）+ `r05_t03_goldens.rs`（3 测试）+
  service 级 `r05_t03_protocol_adapters.rs`（10 测试，in-process 真实 bootstrap +
  loopback HTTP stub，即"外部世界"边界）；全部离线，NOT_REAL_API 标注于台账。
- 媒体/ASR 38 个 D12 叶在 PROTOCOL_WIRE_MATRIX 注册 wire 级事实并注明归属 T06（R5 规则）；
  parity_guard 已核：唯一非 R5 的 D12 叶是 R2 规则延期叶（桌面壳 open 入口，r05_share=null，
  归 R07），无"挂在 T06 的非媒体 R5 叶"，未触发挂起条件。
- anthropic `max_tokens` 固定值缺口、生产流式切换、辅助槽位/Worker 调用入口、
  用量聚合面：归 T04/T05/T06/T07（与 T01 §7 清单一致）。

## 19. REV-T03 R01 修复轮与观察项（2026-10-03）

独立审查 REV-T03 R01（`artifacts/rust-tauri/R05/REVIEW-T03/R01_REVIEW.md`）判**有条件通过**：
实现层 go，两项台账层必修（F-01/F-02，均非产品代码缺陷）+ 三个观察项。

- **F-01（台账张冠李戴）**：T03-C04 条目误挂取消语义（任务书 C04 是「相同 callId 跨会话隔离」，
  取消语义不属于 T03 任何检查点）。修复：移植审查者 scratch 测试为正式测试
  `c04_same_provider_call_id_in_two_sessions_stays_isolated`（两会话收相同 `toolu_SHARED`，
  各自续发各自真实文件内容，journal 各一条，互不串扰），台账 C04 条目重写；
  C02 条目 expected 中混入的「跨会话同名」字样清除（C02 本是外部/内部调用 ID 区分）。
- **F-02（证据形式）**：T03-C05 原为编译期常量单次运行（防预置强度中）。修复：移植
  `c05_runtime_nonce_rides_the_next_request_and_follows_changes`——stub 脚本先于 nonce 固定，
  运行时 nonce（pid/nanos/counter）逐字节上线，两腿不同值跟随变化，构成任务书强形式。
- **N-01（已修复，R05 RR1 F10，2026-10-04）**：openai-responses 族渲染器把 opaque（encrypted reasoning item）
  立即入列、文本累积到最后统一入列；assistant 内容为 text-在前的非标顺序时，重渲染后 opaque
  位置前移。RR1 WP-T03 修复：渲染按内容原相对顺序重放——文本段在原位成 message item（相邻段
  `\n` 连接），reasoning item 原位 verbatim，function_call 经解析期 `function_call_item` 锚点回到原位
  （无锚点的手工交换保持 content-first）；Responses 与 Codex 同步（共享 render_input_items）。同族
  google 的 functionCall Part 也改为原位锚点重放（R05 RR1 F07）。永久测试
  `lingxi-adapters/tests/r05_t03_rr1_replay.rs`（rr1_f10_*）。
- **N-02（观察，产品策略）**：write 工具结果的 wire 文本含 `[resource: out.txt <file://…绝对路径…>]`，
  即工作区绝对路径经 file:// URI 上送模型。这是 resource_refs 的保真上送（非压平），行为正确；
  是否对模型暴露绝对路径归 T06 资源引用处理时裁定（已登记 PROGRESS_LEDGER 后续义务）。
- **N-03（观察，声明）**：openai-completions SSE 聚合把 reasoning 与 text 各自累积后按
  reasoning-在前重建，reasoning/text 之间的逐块交错顺序不保留（text↔tool_calls 交错严格正确）。
  reasoning 模型的实际 wire 顺序几乎必然 reasoning 在前，实际影响为零；此处如实声明。

## 20. T04 设计裁决（流式解码/规范化/部分结果，实现前锁定）

1. **生产五族全切流式**：`GatewayedProvider` 生产路径统一走 `execute_chat_stream`
   （`execute_chat` 保留为带 sink 的委托）。五族终态标记：openai-completions 以
   `data: [DONE]` 闭合；anthropic-messages 以 `message_stop` 闭合；google-generative-ai
   以携带 candidate `finishReason` 或 `promptFeedback.blockReason` 的终帧闭合
   （`:streamGenerateContent?alt=sse`）；openai-responses / openai-codex-responses 以
   `response.completed` / `response.failed` / `response.incomplete` 终态事件闭合
   （终态事件携带权威聚合）。裸 EOF 一律是截断流，绝不算成功。
2. **kernel 端口演进**：`ModelProviderPort::next_turn(ctx, call, input, sink)` +
   `ModelTurnDelta` / `TurnDeltaSink`。live delta 从网络到达节奏直接流出，不攒到结尾重切。
3. **runs.rs 流式桥**：模型调用在 supervised child 内执行，delta 经有界 channel
   （`TURN_STREAM_LIVE_DELTA_CAP = 1024`，D7：满则背压到 provider 读取侧，不丢不无限缓冲）；
   `model_call_started` 先于任何 delta 持久化（D6：一调用的 durable 顺序恒为
   started → deltas → completed）；terminal 先 persist 后 publish。
4. **规范化链同源**：`streaming_norm.rs` = `ReservedTagScanner`
   （`shared/reserved-tag-stream.ts` 字节级 port：openTag/unknownStack/code 三态、转义与
   代码围栏保护、pending-tag 与栈深守卫保守回退）→ think 层（`THINK_TAGS`）→ mood 层
   （`MOOD_TAGS`）→ `DeltaNormalizer`。live delta 与 history 投影
   （`split_reserved_tag_segments`）共用同一 scanner——同一来源，不会长歪（C13 钉死）。
5. **D5 事件词汇决策**（R05 RR1 F12/F13 修订）：live text 片段 = `unresolved`
   ——在调用的 terminal 分类前，任何族都不知道片段文本是否最终答案（文本之后仍可能跟
   工具调用），wire 契约（`AssistantPhase::Unresolved`）禁止静默猜成 `final_answer`；
   text segment 的 END 事件在调用 terminal 处解析：仅当 driver 判定该轮为携带可见
   答案文本的真 `Final` 时为 `final_answer`，否则保持 `unresolved`（incumbent
   `phaseKnownAtEnd` 形状：段以 unresolved 开、在 end 处解析）。reasoning 片段
   （provider 原生 reasoning + think 块文本）= `reasoning`；mood 块从事件流剥离
   （冻结词汇无 mood 事件）。segment id 形如
   `assistant:{turn}:reasoning:default` / `assistant:{turn}:text:default`，开启后驻留到
   `finish`。
6. **final 消息规范化投影（R05 RR1 F13）**：`normalize_final_message` 是
   `ProviderTurn::Final` 消息进入 `final_message_committed` 事件 / messages 行 /
   历史读取的唯一投影——Text 块经同一 scanner 重切（think 族 → Reasoning 块、
   mood 族内容丢弃、围栏/转义字面量保持文本），非文本块原样保留；原始协议文本仅
   存在于 run 的 typed exchange（供应商重放所需），绝不混回可展示正文。规范化后
   无任何可见文本（仅 mood / 仅过程块）的 final 不提交，run 以
   `completed.no_final.process_only` 结算。
7. **idle 超时**：`DEFAULT_STREAM_IDLE_TIMEOUT_MS = 60_000` = R05_BASELINE 预登记
   `http_idle_stream_timeout_ms`。流停转（无 delta 无 terminal 超 60s）即取消 call scope
   （D8：await 点 drop，释放 provider socket），按可重试 upstream 失败结算；一个执行点
   覆盖 connect → 首字节 → 帧间隔全链路。
8. **解码器界限（C04）**：`SSE_BUFFER_LIMIT = 8 MiB` 兼作整流总读上界——比预登记
   `stream_total_buffer_max_bytes`（16 MiB）更紧，§8 允许收紧（预登记值是上限不是目标）；
   `SSE_FRAME_MAX_BYTES = 1 MiB` = 预登记 `sse_single_frame_max_bytes`；
   `TOOL_ARGUMENTS_MAX_BYTES = 1 MiB` = 预登记 `tool_arguments_max_bytes`。全部响亮拒绝，
   绝不静默截断。
9. **C02 严格 UTF-8**：非法 UTF-8 在任意分片下产生同一 `InvalidMessage`；已交付的事件恒为
   完好前缀，erring feed 内已完成但尚未交付的事件随错误一并丢弃（不带着污染缓冲继续交付）。
10. **C07 canonical 参数规则**：非对象参数响亮拒绝；重复键 last-wins 且 digest 覆盖
    canonical 值；浮点与超安全整数一律响亮拒绝（`ArgumentsNotSafeInteger`，与 TS
    incumbent 逐点 parity——这是冻结规则，不是舍入策略）。
11. **C09/C11 截断与停止原因的决策记录**：截断（length / max_tokens /
    model_context_window_exceeded / MAX_TOKENS / incomplete(max_output_tokens)）=
    **可重试** `BudgetExceeded`——该调用零副作用（工具批次整批不派发），partial 内容留在
    delta 事件里，重试安全；拒绝类（content_filter / refusal / SAFETY / RECITATION /
    BLOCKLIST / PROHIBITED_CONTENT / SPII / IMAGE_SAFETY）= **不可重试** `Forbidden`；
    google `MALFORMED_FUNCTION_CALL` = 不可重试 `InvalidMessage`（族自带的终态诚实信号：
    模型自己的 function call 畸形）。usage 在失败臂照常保留（真实账单不丢）。
12. **R05 RR1 F12 终结严格化**：传输结束 ≠ 协议完成。四族 buffered/stream 分类仅凭
    已知的正常终态理由——openai `finish_reason` 缺失或未知值、anthropic `stop_reason`
    缺失或未知值、未闭合 content block（仅见 `message_stop`）、google `finishReason`
    缺失或未知值、responses `status` 缺失/未知值或 incomplete 未知原因，一律响亮
    `InvalidMessage`，绝不猜成 Final / ToolRequests；正常停止但全部内容为过程块
    （reasoning/opaque、无答案文本）的轮 → `ProviderTurn::Empty`（过程语义、
    `content` 随行供重放），run 以 no_final 终结算，不产生 `final_message_committed`。
13. **R05 RR1 F11 整批准入**：`admit_provider_call_ids`（四族共享）——同 provider
    call id 同形（target+digest）重发折叠为一次，异形冲突整批响亮拒绝；
    driver 侧在首个副作用前对整批做静态准入（digest 门 + 网关纯校验
    `validate_from_request`：目标/可用性/代次/CURRENT schema/策略裁决），任一失败
    → 整批零派发、逐项结构化拒绝回传模型；执行时仍逐项重跑完整 prepare 与
    权限/批准/取消/代次复核（预验证不取代即时授权）。

## 21. T04 kernel 演进

- `ports.rs`：`ModelTurnDelta`（Text / Reasoning 两变体；工具参数增量与 opaque 块
  不经 delta 通道——批次准入后由终态 `ProviderTurn` 承载，REV-T04 F-01 校正）与
  `TurnDeltaSink`（`emit` 异步、Err = 调用方关闭，fail-closed）；
  `ModelProviderPort::next_turn` 替换原一次性 `chat_turn` 形态。30 个既有 test double
  全部迁移到新签名（行为不变，仅端口形状）。

## 22. T04 adapters 演进

- `streaming.rs`：`SseDecoder` 从攒批解析演进为**增量**解码（feed 任意字节片，完成事件
  按序返回；EOF 未终结帧响亮上报；BOM 只剥首个；CRLF 与注释心跳腿合法）。三个界限常量见
  裁决 7。
- `dispatch.rs`：`drive_sse_stream` 增量驱动——每个 chunk 随网络到达即喂 decoder，每个完成
  事件立即交 handler；decoder 或 handler 拒绝即放弃整条流（连接读取被取消，错误上达，
  HTTP 200 绝不掩盖坏流，C15）；传输层读错误 = 可重试 `UpstreamUnavailable`。
- 三族流式 accumulator：openai `ChatStreamAccumulator`（[DONE] 闭合）、anthropic
  `MessagesStreamAccumulator`（message_stop 闭合）、google `GenerateStreamAccumulator`
  （finishReason/blockReason 终帧闭合）；responses/codex 走终态事件权威聚合。闭合批次经
  与 buffered 模式**同一**解析器校验——流式不是第二套语义。
- `provider.rs`：五族 `execute_chat` 全部委托 `execute_chat_stream`（T03 的 buffered
  生产路径仅存在于 git 历史）。

## 23. T04 service 演进

- `runs.rs`：流式桥（裁决 3/6）——child 内 `ChannelDeltaSink`，driver drain/normalize/
  批量持久化 delta（`persist_delta_batch`），terminal 先 persist 后 publish；cancel 臂
  不补 `model_call_completed`（被取消的调用诚实不闭合）。
- `streaming_norm.rs`（新）：裁决 4/5 的规范化链，14 个单测。

## 24. T04 消费者同步与遗留

- adapters 级 `r05_t04_streaming.rs`（18 测试：C01 任意字节分片等价——逐字节/全二切点/
  固定 seed 随机 200 轮；C02 严格 UTF-8；C04 预登记界限响亮——整流缓冲/单帧/工具参数/
  病态嵌套；C05 半截 JSON；C06 可解析≠完成；C07 canonical
  参数；C08 交错片段；C09 截断批次准入；C10 重复/冲突响亮；C11 停止原因表；C14 opaque
  保真；C15 解码半）+ service 级 `r05_t04_streaming.rs`（9 测试：C12 反假流式屏障——gated
  stub 刷首帧后 hold，真实订阅者在屏障释放前收到首 delta；C05/C06/C09/C11/C13/C15 服务面；
  C16+A09 流中取消释放连接并恰一次结算；A08 中断流绝不伪造 final）。唯一 double 是
  loopback stub HTTP 服务（导线对端）；进程内全链路为生产代码。全部离线，
  NOT_REAL_API 标注于台账。
- 既有套件适配（消费者同步，全部保持原断言语义）：
  - `cancellation_tree.rs` 的 stream_read 取消腿 durable 序更新——D6 下
    被取消的中途调用留下诚实的 `model_call_started`（绝不伪造 completed）。
  - `late_result_fence.rs` 的 mc0002 零写入断言排除诚实的 started 事实
    （D6：attempt2 的调用真实发生过，started 是其唯一合法事实；被 fence
    的结果仍零写入）。
  - `r05_t01_model_plane.rs` / `r05_t01_binary_wiring.rs` /
    `r05_t02_credentials.rs` 的 loopback stub 从 buffered JSON 应答改答
    SSE（生产已切流式；请求断言 `stream:false`→`true`；流式 tool_call 片段
    必须携带 `index`，buffered 形状无此字段）。
- live provider 验证保持 BLOCKED_NOT_AUTHORIZED（未获真实外网授权）；生产流式切换已在
  T04 落地，遗留项收敛为 T05 渲染层穿插、T06 媒体/资源引用、T07 槽位裁定。

## 25. T05 设计裁决（网络纪律/重试/compat 移植，实现后登记）

1. **三段位超时 + 单调总预算**（C01）：`dispatch::HttpTimeouts{connect 10s, first_byte 30s}`
   （预登记值）+ `ModelTurnInput.deadline_unix_ms`（每次逻辑调用建立一次，`now+300s`，
   跨该调用的全部重试共享；ToolRequests/Continue 臂重置）。deadline 走真实时钟，
   不搭注入的 `now_ms` 测试钟。命中总预算 = 不可重试 `BudgetExceeded`（发送前/首片窗内/
   流中段三点判定，窗口命中与期限命中用新鲜时钟读区分，绝不混淆）。
2. **A10 精确化：重试类别 = 429/5xx（有界）+ 连接期失败（可证明未接受）**。
   发送后失败、首片超时、流中段断读、408 全部**不可重试**——请求可能已被服务端持有，
   无幂等键即绝不盲重发。与现役对齐：`core/model-operation-client.ts:448`
   只把 429/≥500 标 retryable，`core/llm-client.ts:838` 一次调用=一个真实网络 attempt。
   **本裁决修正了 T03/T04 两处较宽分类**（首片超时与流中段读错误原标可重试；无既有
   断言依赖旧值，T03 golden 与 T04 套件在修正后全绿）。
3. **Retry-After 契约**：429 的 hint（RFC 9110 两种形态；垃圾值忽略）进
   `details.retryAfterMs`；driver 侧 hint **覆盖**计算退避，计算退避
   `base×2^(n-1)` 封顶（500ms/8s 默认），两者都被共享 deadline **否决**——
   睡不进预算的重试按原失败结算，绝不睡向注定的过期。退避睡眠是 cancel-aware
   select 臂（取消立即按四阶段结算），且退避期间不持有模型配额 permit。
4. **尝试身份**：Run 固定、重试开新 attempt（#a2…），默认 `max_attempts=3`
   （预登记 `transport_retry_max_attempts`）。重试保留已确认交换（T03-C12），
   工具副作用绝不重做（C07）。
5. **代理直连裁决（C12 范围决策；REVIEW-T05 R01 修订）**：所有外发 client
   `no_proxy()`——dispatch 共享 client 与 OAuth client 同一纪律（R01 发现
   `oauth.rs` 漏了该行（F-02）：token/refresh 请求携带 client_secret/refresh_token，
   reqwest 默认 `system-proxy` 会读 HTTP(S)_PROXY 与 macOS 系统代理库，属对现役
   pi-sdk 裸 fetch 的真实行为回退；已修并以计数代理回归测试钉住）。现役 Node
   fetch 从不受理环境代理（审查者 node-fetch-proxy-probe 实测直连 200、计数代理
   0 命中）；环境代理会合成自己的应答（拒绝/断连被掩盖成 502/挂起），直接破坏
   A10 的接受证明，并无声暴露凭证材料。系统/手动/NO_PROXY 配置面登记为遗留
   （依据：现役模型路径零代理处理代码，`lib/net/outbound-proxy.ts` 的 dispatcher
   只影响 MCP/bridge 面——NOT_APPLICABLE 成立，附审查者来源证据）。
   **私有 CA 腿是真实能力差距（F-01 更正：原「无现役对应物」理由事实错误）**：
   现役内建 fetch 尊重 `NODE_EXTRA_CA_CERTS`（审查者 node-extra-ca-probe 实测
   私有 CA 签名端点 200 OK），另有 Windows 系统 CA 合并通道
   （`desktop/src/shared/windows-system-ca.cjs`，main.cjs:95 接线）；Rust 侧
   rustls-platform-verifier 读平台根（Windows 系统 CA 腿由此覆盖），但**无
   NODE_EXTRA_CA_CERTS 等价物**（全平台操作者附加 CA 文件通道）——显式登记为
   后续网络加固阶段任务。无效证书拒绝腿由审查者 tls-probe 补证 PASS（生产
   `dispatch::build_client()` 拒自签证书，macOS 信任评估错误码 -67843；全仓无
   `danger_accept_invalid_certs` 调用点）。
6. **compat 移植范围**：12 个注册表模块全部移植（deepseekResponses/deepseek/kimi/
   mimo/qwen/zhipu/volcengine/agnes/openrouter/anthropic/codexResponses/ollama），
   first-match-wins 顺序一致；TS throw → 不可重试 `InvalidMessage`，拒绝文案逐字。
   golden 对齐：`r05_t05_generate_compat_goldens.mjs` 直跑现役 TS 模块生成
   49 fixture，`r05_t05_compat.rs` 断言推导链/派发/payload/拒绝文案结构相等
   （serde_json Value，key 序不敏感；拒绝文案为字符串逐字相等）——fixture 文件
   逐字节一致由生成器幂等重跑证明（R01 审查者复跑 49 fixture shasum diff 为空）。
   不移植登记（附源码依据，
   见 PROTOCOL_WIRE_MATRIX.json `shared.provider_compat_port.not_applicable`）：
   longcat（本管道上恒为恒等函数——所有触发条件在 Rust 侧不可达）、
   openai-input-audio（input_audio 部件在 T06 媒体范围，渲染器从不产生）、
   openai-video-url（不在注册表，中心化处理）、中心层通用补丁（对 Rust 渲染输出
   恒等）。google 族不接 compat 层（现役无针对 google 信封的 compat 模块，
   登记偏差而非缺口）。deepseek roleplay 触发器无 Rust 侧来源（persona 层，
   TS 默认 OFF 即此处行为）。`thinkingProfile` 别名与 `maxOutput` 回退在配置
   compat 块不可表达（只声明 `maxTokens`）；JS 非对象输入退化分支不移植
   （Rust 类型层已排除）；kimi 的 utility 0.6 温度经 serde 浮点精确表示
   （0.6 在两侧同为 IEEE754 最近值，一致性由 golden fixture 证明）。

## 26. T05 adapters 演进

- `dispatch.rs`：`send_with_timeouts`（发送前预算查、首片窗、429 细节）、
  `drive_sse_stream_within_budget`（流中段预算）、`parse_retry_after`、
  `RETRY_AFTER_MS_DETAIL`；共享 client 加 `no_proxy()`（裁决 5）；中段读错误与
  408 改不可重试（裁决 2）。
- `oauth.rs`（REVIEW-T05 R01 F-02 修复）：`OAuthHttp::new()` 补 `.no_proxy()`——
  token/refresh 请求携带 client_secret/refresh_token，与裁决 5 同一纪律；回归测试
  `r05_t02_oauth_flows.rs::the_credential_bearing_client_never_consults_the_ambient_proxy`
  以计数代理监听器钉住（pre-fix 失败、post-fix 通过，辨别力已实证）。
- `compat.rs`（新，约 2900 行）：`ModelView`/`CompatOptions`/`CompatCall`、
  推导链（thinkingFormat/reasoningProfile/replay 契约/outputIncludesThinking/
  max-effort/video）+ 12 模块移植 + 注册表派发；7 个单测。
- `config.rs`：`RouteBinding.compat: Option<RouteCompatHints>`（10 字段）+ 封闭
  校验（4 词表 + 值域；未知 thinkingFormat 响拒——TS 静默丢弃正是本平面拒绝的
  静默降级）。
- 四信封适配器 `execute_chat(+_stream)` 增 `compat` 参数：渲染后、派发前应用；
  拒绝 → 不可重试 Failed。`provider.rs` 经 `gateway.compat_hints` 接线
  （chat binding 在六个 aux 槽顺序找同 (provider,model) 对的 compat 提示）。

## 27. T05 service 演进

- `runs.rs`：`RunDriveLimits::DEFAULT_MAX_ATTEMPTS=3`；`ModelCallTuning`
  （total_budget 300s / backoff base 500ms / cap 8s）+ 响亮校验；
  `with_model_call_tuning` builder；drive_run 的 call_deadline 建立/共享/重置、
  acquire 前预算耗尽响拒（budget_exceeded 不可重试）、Retry-After 覆盖、
  deadline 否决、cancel-aware 退避臂。
- `lib.rs`：`ServiceDeps.model_call_tuning`（默认 + bootstrap 校验）；
  `ServiceDeps.stream_idle_timeout`（REVIEW-T05 R01 F-03 测试缝：`None` 保持
  预登记生产值 60s，组合根接 `RunSupervisor::with_stream_idle_timeout`）。

## 28. T05 消费者同步与遗留

- 适配器级 `r05_t05_timeouts.rs`（13 测试：C01 三段位+预算三点、C05/A10
  接受后断连/首片超时/流中段各恰一次外发且不可重试、连接期失败可重试、
  C03 429 双形态 hint/垃圾忽略/5xx 无捏造 hint、parse_retry_after 单元钉）；
  服务级 `r05_t05_timeouts.rs`（9 测试：退避表纯函数钉、默认 3 次、hint 覆盖、
  无 hint 计算退避下限、预算否决不盲睡、退避中取消立即结算、退避不占 permit——
  全局模型配额 1 下 B 在 A 退避中被准入并完成、C07 已确认工具副作用后重试不重做、
  C01 空闲流界限缝测试（REVIEW-T05 R01 F-03：经 `ServiceDeps.stream_idle_timeout`
  注入 200ms，「一个 delta 后永久挂起」的 double 在注入界限被切断，3 次尝试后
  诚实失败、每尝试的 partial delta 持久不倒带；生产 60s 值的计时签名另由审查者
  idle-probe 以 181.531s/3 attempts 补证））。compat golden 套件
  `r05_t05_compat.rs`（49 fixture：生成器幂等重跑逐字节一致，套件断言结构相等）。
- 唯一 double：loopback raw-TCP stub（导线对端，故障注入式脚本）与
  TurnProviderPort 脚本 double（只产出外部响应；状态决策全在生产代码）。
  全部离线，NOT_REAL_API。
- 既有套件适配（消费者同步，断言语义保持或按新真相收紧）：
  - `r05_t03_protocol_adapters.rs::c02_anthropic_tool_roundtrip_with_the_real_file_tool`
    的 tool_result 断言补 `cache_control: ephemeral`——anthropic compat 补丁对尾部
    可缓存 user 消息的标记是现任逐字行为（T05 compat golden 钉住），T03 断言
    先于 compat 层存在。
  - `r05_t02_credentials.rs::c07_a_transient_refresh_failure_is_bounded_by_the_attempt_policy`
    的命中计数 2→3：默认尝试预算从 2 抬到预登记值 3
    （`transport_retry_max_attempts`），测试同步钉住新默认的有界性
    （chat/refresh 各恰 3 次后停止）。
  - `r05_t02_oauth_flows.rs` 增 `the_credential_bearing_client_never_consults_the_ambient_proxy`
    （REVIEW-T05 R01 F-02；套件 20→21 测试）。
- 已知环境限制：本机系统代理曾拦截 loopback 流量并合成应答（裁决 5 后
  测试与生产同走直连）；`r00_management_leaves` 唯一失败为既知 ALF 环境项
  （macOS 防火墙拦非回环自地址），与本阶段无关。
- 遗留（REVIEW-T05 R01 修订后）：C11 的 SSRF/内网地址策略腿与附件 URL 取数
  授权腿（现役模型路径无对应策略层，台账拆腿为 R05-T05-C11B NOT_RUN）、
  C12 的代理配置面腿（NOT_APPLICABLE，现役无代理处理代码路径）与私有 CA 腿
  （真实能力差距：Rust 侧无 NODE_EXTRA_CA_CERTS 等价物，F-01 更正登记）——
  均归后续网络加固阶段；C12 无效证书拒绝腿已有审查者 tls-probe 证据，固化为
  常驻测试是廉价的后续项（rcgen 已在 lingxi-service dev-deps）；live provider
  验证保持 BLOCKED_NOT_AUTHORIZED；session thinking level / structured output /
  replay 策略源归 T06/T07（CompatOptions 对应字段生产端恒 None）。

### 28a. T05 RR1 修订（F14/F15/F16，2026-10-05，实现后登记）

上节遗留与裁决 5 的「后续网络加固阶段」延期被 RR1 审查（gate G03）判定为
**不合法延期**（原任务书 T05 步骤 1 是本阶段必需义务），已由 RR1 WP-T05 修复关闭：

- **裁决 5 修订**：固定 `no_proxy()` 不再是生产行为——`build_client_with_timeouts`
  退化为 Direct 策略的遗留构造器（隔离测试保持原语义）；生产走
  `models/network.rs` 的统一网络策略（`NetworkPlane` 代次化）：system（load 时
  HTTP(S)_PROXY/ALL_PROXY/NO_PROXY 环境快照——现役 `shared/network-proxy.ts` 契约
  镜像，含强制环回 bypass 与完整 NO_PROXY 语法）/manual（http/https/socks/socks5，
  校验拒绝 userinfo/path/query）/direct。显式 `trustedCaPem`（PEM bundle）叠加于
  rustls-platform-verifier 之上＝NODE_EXTRA_CA_CERTS 等价物；TLS 链/主机名/有效期
  校验永不放宽（无 accept-invalid 路径）。chat 五族/OAuth/辅助/worker callback/
  operations/egress 下载全部消费同一 plane；`POST /lingxi/v1/models/reload` 原子
  发布新策略代次。消费面正反对照（计数代理、生成 CA 的 TLS 授权/未授权/错主机名/
  过期/错链）＝`r05_t05_network.rs` mod rr1_f14。
- **C11B 边界修订**：原「DNS rebinding 离线不可判」声明缺口撤销——egress 对非
  same-origin 的 https 主机名先解析并判定**全部候选**（任一守卫候选即拒，混合
  公私记录同拒），然后经 `resolve_to_addrs` **钉住**已判定候选拨打（SNI/Host/证书
  校验仍按域名）；代理解析边界与认证头不带同样有钉（mod rr1_f15）。
- **预算边界修订（F16）**：错误 body（chat/operations 分类路径）读入纳入剩余绝对
  预算+64KiB 上限且保留超时/截断/读失败事实；operation admit 与 run 驱动模型 permit
  排队、401 协调刷新等待全部包裹剩余预算（`BudgetExceeded` 诚实结算，不再各段重置
  或等满配额窗）；OAuth 端点 body 读入加 256KiB 上限。
- live provider 验证仍为 BLOCKED_NOT_AUTHORIZED（本修订只关离线义务）。

## 29. T06 设计裁决（Worker 回调异步化 / 媒体与辅助操作平面，实现前锁定）

1. **`WorkerModelPort::complete` 演进为 boxed-future 异步端口**（形如
   `TurnProviderPort::next_turn`）：签名新增 `invocation: &str`（宿主铸造的 CSPRNG
   请求 id，一次 execute() 一个）与 `cb_id: &str`——预算/去重的真实调用身份由宿主
   传入，端口绝不从 run/worker 字符串推导。workerrpc.rs 读循环内的同步调用点改为
   `timeout_at(deadline, port.complete(..)).await`：callback 等待与行读取共享同一
   invocation deadline——到期即协议取消 + 进程组有界 kill + Unknown 结算（C10）。
   async 读循环内无 block_on、无持锁等网络（C05 由单 worker-thread 运行时测试钉死）。
2. **预算键 = invocation id，计数随 invocation 回收**：`BoundedWorkerModel` 的
   counters 以宿主 invocation id 为键；`begin_invocation` 在 execute() 铸 id 后调用，
   `end_invocation` 在每条结算路径（含 future 被 drop 的取消路径，经 RAII guard）回收
   计数——R04 形状以 `run_id/worker` 为键会跨调用误扣、且 counters 只增不减（泄漏），
   两者本裁决一并消除（C07）。同 invocation 内重复 `cb_id` 由 invocation 级回执缓存
   应答：**同一 cb_id 绝不外发第二次**，重放不消耗回调预算（C07 的「重复回调」腿）。
3. **C08 宿主预算校验前置**：`max_output_tokens == 0` 或超过宿主上限（预登记
   4096）、prompt 字节超过宿主上限（预登记 64 KiB）→ `BudgetExceeded`，在 inner 端口
   被咨询之前拒绝，绝不静默截断或溢出。生产预算值按 R05_BASELINE 预登记：
   `WORKER_MAX_CALLBACKS_PER_CALL` 4→**8**、`worker_callback_max_output_tokens`=4096、
   cancel_settle_window_ms=10000（WorkerLimits::default 同步对齐 8）。
4. **C09 载荷不是权限**：callback 线格式只认 `kind/cb_id/op/purpose/prompt/
   max_output_tokens`；出现 `provider`/`model`/`endpoint`/`auth`/`run_id`/
   `parent_run_id` 等身份主张字段 → 该 callback 以 `worker_identity_not_negotiable`
   响亮拒绝（不计成功、不派发、计入回调预算）。`purpose` 必须命中该 worker 注册时宿主
   批准的用途白名单（`WorkerToolSpec.allowed_model_purposes`，purpose→AuxiliarySlot 的
   宿主侧映射）；未授权用途 → `model_purpose_not_granted`。路由（provider/model/
   endpoint/凭证）一律由宿主 ModelGateway 按槽位解析。
5. **`GatewayWorkerModel`（service 侧新模块 `workermodel.rs`）**：gateway（aux 槽
   路由）+ ProviderCredentialPort + `AuxiliaryExecutor`（裁决 8）+ QuotaManager
   （每次 callback 获取 Model permit——C06 的结构性前提：runs.rs 在主模型 HTTP 轮次
   解出后、工具执行前释放模型 permit，callback 的 acquire 才能成功；负向证明用有界
   超时捕获「不释放即死锁」）。凭证只在 CredentialService；错误串一律过
   `scrub_materials`（C10-凭证腿）。usage 归 T07 账本；T06 在 callback 回执携带
   （provider/model/usage/父 invocation+cb_id）关联事实，测试以记录型 trace 端口钉住
   父子 ID（C04）。
6. **kernel `ProtocolFamily` 扩词表**：新增 operation 方言——embedding
   （openai-embeddings/ollama-embed/gemini-embed/voyage-embeddings/minimax-embeddings）、
   rerank（cohere-rerank/siliconflow-rerank/voyage-rerank/dashscope-rerank）、image
   （openai-images/openai-codex-responses-image/volcengine-images/minimax-images/
   dashscope-images/gemini-generate-content-image/agnes-images）、video（agnes-videos）、
   speech（openai-audio-speech/minimax-tts/dashscope-qwen-tts；system-speech 已在）、
   asr（openai-audio-transcriptions/mimo-chat-completions-asr/dashscope-qwen-asr-chat；
   volcengine-bigasr 已在）+ system-speech-recognition。每族声明可服务操作集；gateway
   对「族不服务该操作」响亮 `OperationUnsupportedByProvider`（嵌路由到
   openai-completions 提供者的 embedding 永远响拒，绝不静默改道）。
7. **config `ModelsSection` 增六个操作绑定键**：`embedding`/`rerank`/`image`/`video`/
   `speech`/`speechRecognition`（各 RouteBinding，与 chat/aux 同一独立解析纪律；
   未知键仍 `deny_unknown_fields` 响拒——config 测试同步演进）。每操作一个槽，
   不发明每供应商模型目录（catalog/管理面归 R07）。
8. **`AuxiliaryExecutor`（adapters 侧 `models/auxiliary.rs`）**：复用五族 turn 渲染/
   解析/compat/401 单次刷新——`ModelTurnInput` 增 `images: Vec<InputImage>`（宿主授权
   读取的字节+MIME，有界）与 `max_output_tokens: Option<u32>` 两字段（chat 路径恒空，
   现役行为不变）；`GatewayedProvider` 增 `next_turn_for_operation`，aux 槽以
   turn=1/无工具/无 prior 的最小交换走真实族适配器。approval/guard 的 fail-closed
   由「槽位独立解析 + 无 fallback」构造性保持（未配置=响亮 RouteNotConfigured）。
9. **operation 执行平面分层**：`lingxi-adapters::models::operations`（纯方言渲染/解析
   + HTTP 执行，复用 dispatch.rs 纪律）+ `lingxi-service::operations`
   （`OperationService`：QuotaManager Model permit、deadline、授权资源读取、产物
   落盘登记、SSRF 出口守卫、system-speech 平台分发）。文件编码/格式转换可留受控
   执行器，模型选择/凭证/网络生命周期/预算由 Rust 持有（规格 §4-T06）。
10. **异步媒体诚实状态机**：`submit_video` 只证明 job 被接受
    （`VideoJobAccepted{provider_job_id}`≠产物完成）；`query_video` 返回
    Generating/Completed/Failed——Completed 仅在产物字节下载+校验+登记之后（C12）。
    agnes 族无远端取消 API：本地取消只停本地轮询并把本地记录置 cancelled，
    `remote_cancel: "unsupported"` 事实显式登记，绝不冒充远端撤销。媒体任务持久
    存储/管理 HTTP 面（D12 tasks READ/RETRY 叶）归 R07 业务面，本阶段操作级证据=
    状态机与线协议测试。
11. **C11B 出口守卫（`models/egress.rs`）**：媒体产物/参考图 URL 取数纪律——
    仅 http(s)、无 userinfo、字面 IP 的私网/回环/link-local/unspecified 一律拒绝
    （唯一例外：URL origin 与已配置 provider endpoint origin 完全一致——回环 stub
    与自托管同源资产情形）；https 公网主机名放行（DNS rebinding 类离线不可判定，
    登记为已声明缺口）；http 仅同源；重定向不跟随（build_client no_redirect）；
    **下载绝不携带任何 provider 凭证**；字节上限（64 MiB）读中强制；Content-Type
    必须是 image//video//audio/ 前缀否则响拒。参考图本地读走 ResourceAccess 授权
    + 大小上限。凭证转发策略：凭证只发往已配置 endpoint 原点，跨原点零转发。
12. **N-02 裁定（REV-T03 R01 观察项，本任务落地）**：模型面向渲染
    （`tool_render.rs`）不再外发 `file://` 绝对路径——`[resource: {display_name|
    resource_id}]`，仅非 file:// 的远端 URI 保留渲染。宿主侧 ToolOutcome/journal
    保留完整 URI（审计保真不丢）。裁定依据：模型只需可对工具回引的资源身份；
    工作区绝对路径对模型无协议价值、对远端是主机布局泄漏。受影响 golden/断言按
    新真相收紧并在 §33 登记。
13. **平台诚实**：system-speech（TTS）生产实现 = spawn `/usr/bin/say`
    （argv 成形、无 shell、120s 进程上限、SIGTERM→1.5s→SIGKILL、产物非空校验）——
    仅 macOS；非 macOS 响亮 `ProtocolNotImplemented`。system-speech-recognition：
    Swift helper 是桌面 TCC 授权面（权限归属 Lingxi.app 宿主，裸服务进程无法持有
    授权），Rust service 本阶段对该族响亮 `ProtocolNotImplemented`（不 spawn），
    登记平台归属理由——显式拒绝不是占位：ASR 操作本身由其余四族真实落地。

## 30. T06 实现登记（实现后固化，2026-10-03）

按 §29 的裁决逐项登记实际落地的接口与文件：

1. **adapters `models/egress.rs`（新）**：C11B 出口守卫。策略即 §29.11：
   仅 http(s)、无 userinfo、字面 IP 私网/回环/link-local/unspecified/组播
   /RFC6598 共享段拒绝（唯一例外：URL origin 与已配置 provider endpoint
   origin 完全一致——`parse_url` 产出与 `origin_of` 同形的 origin，默认端口
   隐去）；https 公网主机名放行（DNS rebinding 为已声明离线不可判定缺口）；
   plain http 仅同源；no-redirect client；下载零凭证；64 MiB 读中上限
   （`EGRESS_DOWNLOAD_MAX_BYTES`）；Content-Type 必须 image//video//audio/
   前缀。`set_provider_endpoints` 支持原子换源（management reload 后同步）。
   单测 5 项钉死各拒绝/放行腿。
2. **adapters `models/tool_render.rs`（N-02 落地）**：`file://` URI 不再进
   模型渲染——resource_refs 渲染为 `[resource: {name}]`（仅非 file:// 的
   远端 URI 保留 `<{uri}>` 内联）；非文本内容块的 JSON 投影同步移除
   `resource.uri`（file:// 时）。宿主侧 ToolOutcome/journal 保留完整 URI
   （审计保真不丢）。受影响 golden 收紧：openai-completions /
   anthropic-messages / openai-responses 的 tool_roundtrip.json（渲染侧
   `[resource: out.txt <file:///tmp/out.txt>]` → `[resource: out.txt]`；
   输入侧 resource_refs 夹具的 uri 保留——那是宿主侧事实）。
3. **adapters `models/operations/`（既有方言层的补全）**：
   `speech.rs` 增 `effective_openai_speech_format` /
   `effective_minimax_speech_format`（产品 MIME 推导输入——MIME 跟随 wire
   FORMAT，不跟随响应 content-type）；空文本守卫修正为 trim 语义
   （`textOrNull`：空白串也是 "prompt is required"——原 `is_empty()` 是
   保真缺口，本任务修复并有 c12 测试钉住零请求）。
4. **service `operations.rs`（新，§29.9 的 service 半层）**：
   `OperationService` = ConfigModelGateway 路由（每操作自有绑定，未配置
   即响亮 `RouteNotConfigured`，绝不落回 chat 路由）+ CredentialService
   凭证（唯一材料出口）+ QuotaManager Model permit（与主循环/worker 回调
   同一配额管理器）+ OperationDispatcher（共享 dispatch 纪律）+ EgressGuard
   （URL 产物下载）。方法面：`embed` / `rerank` / `generate_image`
   （openai 族同步结算；dashscope Pending→`query_media_task` 单轮询）/
   `submit_video`+`query_video`+`cancel_media_task`（§29.10 诚实状态机）/
   `synthesize_speech`（openai 字节体、minimax hex、dashscope URL→egress、
   system-speech 平台分发）/ `transcribe`（bigasr 状态头校验 + 四族文本
   阶梯）。产物登记 `RegisteredProduct`：写盘 + sha256 摘要，登记记录只在
   文件存在后产生（C03 幽灵产物拒绝）；空产物拒绝。
   `read_image_reference` / `read_host_audio`：经 `ResourceAccess` 授权读
   （工作区外 Forbidden；20 MiB / 25 MiB 上限响亮拒绝，绝不截断）。
   system-speech：`/usr/bin/say` argv（-o out [-v voice] input）、无 shell、
   120s 上限、SIGTERM→1.5s→SIGKILL（libc::kill 阶梯）、产物非空校验、
   非 macOS 响亮拒绝；system-speech-recognition 在 `resolve()` 即响亮拒绝
   （§29.13 平台归属）。
5. **service `lib.rs`/`management.rs` 接线**：bootstrap 在真实模型链接线
   （`wired_real_model_chain`，注入 turn_provider 时保持测试所有）时构建
   `OperationService` 挂到 `ServiceState::operations`（产物根
   `runtime_dir/media-products`）；models.reload 先换 gateway 快照，再以
   当前 provider endpoints 原子刷新 egress allowlist。
   `ConfigModelGateway::provider_endpoints()` 新增（端点是配置不是凭证
   材料；gateway 仍无材料访问器）。
6. **媒体状态机语义（§29.10 落地）**：`MediaJobAccepted` 只证明 job 被接受
   （`remote_cancel: "unsupported"` 是 agnes 的供应方事实）；查询返回
   Generating / Completed（仅下载+校验+登记之后）/ Failed /
   CancelledLocally{remote_cancel}（本地取消不冒充远端撤销）；未知 task id
   响亮拒绝（"not tracked"——不编造可轮询事实）。
7. **测试登记**：adapters `r05_t06_operations.rs` 27 测试（C01 全方言
   wire/解析逐字断言 + C02 行为诚实：bytes-out=bytes-in、minimax 信封、
   bigasr 状态头、32 MiB+1 读中上限单请求、agnes legacy 回退精确条件、
   codex SSE 聚合）；service `r05_t06_operations.rs` 7 测试（C02 三供应
   商零交叉 + 线账本、C03 授权读/上限/幽灵产物/守卫下载零凭证/跨源拒绝、
   C12 job≠产物/本地取消事实/挂起中取消零登记/空文本零请求）；
   `r05_t06_worker_model.rs` 9 测试（C04-C11，前一会话完成，本次复核全绿）。
8. **本节未做与后续**：usage 聚合归 T07；媒体任务持久化/管理面归 R07；
   egress 的 DNS-rebinding 判定缺口维持已声明状态（离线不可判定）；
   T05 登记的 C11B NOT_RUN 由本任务的 egress.rs + 测试解除（T05 台账
   更新见 R05_ACCEPTANCE_LEDGER）。

## 31. RR1 F38 实现登记（R05-T07 修复 r1，2026-10-06）

取消后（请求可能已发出）的对账义务落地，涉及三处接口演进：

1. **`WorkerModelPort` 新增 `abandoned(fact: AbandonedWorkerCallback)`**
   （默认 no-op；`BoundedWorkerModel` 转发 inner）：一个在结算前被 drop 的
   回调（invocation deadline 到期 `timeout_at` drop，或 run 取消 drop 整个
   execute future）的记账入口。`fact` 为**自有数据**（RunContext/invocation/
   cb_id/parent_tool_call/purpose/started_at 全 owned）——run 取消路径只有
   Drop 代码运行，行必须能脱离到 detached task。生产实现
   `GatewayWorkerModel::abandoned` 经既有 `LedgerWorkerCallbackTrace` 写行：
   outcome=`cancelled`、usage unknown、`transport_attempts=None`（尝试数
   未知）、身份诚实 `unreported`；写失败响亮记日志（future 已不存在，行是
   最后的诚实见证，不是控制流事实）。
2. **usage 台账 `transport_attempts` nullable 化（migration v7 表重建）**：
   `ModelCallUsageRecord.transport_attempts: Option<u32>`——`Some(0)`=not-sent、
   `Some(n)`=观测到的物理请求数、`None`=drop 后尝试数未知（绝不以 0/1 冒充）。
   v6 库开库即升 v7，旧行原值保留（`r05_t07_persistence.rs::
   migration_v6_to_v7_makes_attempts_nullable_and_keeps_rows`）。
3. **run driver 取消臂写行不写事件**：`root.cancelled()` 竞态臂与 fence
   Stale+cancelled 臂在 `settle_cancellation` 之前为该 call 落台账行
   （新私有入口 `persist_model_call_cancelled_in_flight`，与完成路径共用单一
   record 构造点 `model_call_usage_record_of`）；**不写**
   `model_call_completed` 事件——A09/C16 的「取消的 call 不以伪造 completed
   事件收尾」纪律原样保持（cancellation_tree r03_a05 与 r05_t04 c16 两条
   既有反例零改动通过）。
4. **五族 stream parse-Err 路径拾回已观测 usage**（F21「意外工具响应有
   usage」自检腿揭出的同类缺口）：各 accumulator 新增
   `observed_usage_report()`（openai-completions 原始 usage JSON、responses/
   codex terminal 聚合、anthropic/google fold+violation），execute 的
   finish-Err 分支把它附到 Failed 结果（`with_usage_report`，同一严格解码，
   无任何放宽）——回合照常响亮失败，计费事实不随 parse 错误消失。
