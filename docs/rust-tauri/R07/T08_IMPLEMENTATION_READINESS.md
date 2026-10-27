# T08 实施前事实与约束

状态：只读准备，尚未开始实施；须在 T07 收口后按顺序推进。不提交或推送，不触真实账号/付费/外发。

## 最大缺口：目录保存不等于实际生效

- `agent_config_readback.rs` 的 load_catalog / registry_save_provider / merged_catalog_providers_with / sync_models_json 可复用，但现只投影 home/models.json（Pi形状 baseUrl/api/apiKey/models），并重读焦点 YAML。
- 真实 Rust 读取 home/lingxi-service/models.json 或明确 --config 内嵌 providers/models，格式为 protocol/endpoint/auth 加 operation 模型绑定。两者路径和形状均不同。现有助手配置 PUT 没有发布到运行网关/凭证，不能把设置读回成功当下一真实调用已生效。
- main.rs 无 plane 时不装配 gateway/credentials/network/provider/operations，RunSupervisor、media、memory 等随后捕获 None；仅补 ServiceState 字段或 reload 无法补齐。

## 可维护方向

仅生产组合根无论配置有无均通过既有路径装配完整共享链，无配置用已校验空种子。增加明确 activation/source 共享状态，保留未配置 reload 的 404/model_plane_unconfigured、聊天 NoProviderConfigured 等原有错误含义；默认 ServiceDeps 与显式注入替身边界不随意改变。所有模型写面及 management reload 共用配置写锁和发布入口：完整读取/单项合并→完整验证→安全持久化唯一Source→credentials.reload(next generation)→gateway.reload同generation→网络/外围执行器允许地址→activation/source→审计。并发不能混 A endpoint/B key。失败后明确 saved_not_applied/可能部分成功，不虚报回滚。

embedded 写回同一 --config，保留 home/workspace/worker 等其他字段；ModelsFile 写回同一真实文件并保留未编辑 provider/slot/network；新用户首次成功写入 runtime models 文件。旧 Pi 投影只为兼容，不能成为第二执行权威。发布必须在外部编辑冲突时拒绝覆盖。不要把全局偏好普通互斥锁跨 await 持有。

Gateway 另缺目录白名单：ProviderConfig 目前仅 protocol/endpoint/auth，显式 provider+model pin 只能从 operation bindings 继承能力，否则 capabilities=None。设置页合法目录模型不必恰好是某槽；桥接需使同一网关快照携带明确目录模型白名单+能力/compat，并维持每个 operation 独立绑定。未知模型不得盗用同 provider 凭证；删除后不能借旧槽继续新调用。

## 凭证与探测

- api-key GET 是已保存目录 api_key，不是 OAuth access token，双权限 providers.manage + secrets.write；默认其他读面/导出全部脱敏。
- 组合 key+多headers 尚不可表达；需在统一 AuthConfig / ApplicableAuth / CredentialService seed / kernel材料无关reference kind / dispatch.apply_auth / 脱敏材料枚举中补齐。头名大小写去重、现有禁头与 CRLF 校验，按旧TS规则自定义同名头显式覆盖，不重复 append。
- freshness 必须走 ProviderCredentialPort.resolve(&route)，resolve_handle 不刷新 OAuth。各请求使用同一网络/时限/配额/真实用量链，不由新路由私发带密钥请求。
- 目录 fetch 和大多数连接 test 是 control-plane GET，不虚增模型用量；Anthropic test 最小 messages 生成才记账；Codex Responses 明示 skipped。15秒预算、草稿凭证优先/掩码回落、成功目录缓存不自动加入模型。
- /models/health 才是指定 provider/model 的真实小文本调用，不能只测当前chat；GatewayedProvider::next_turn_for_operation_with_options 可 pin，AuxiliaryExecutor 尚无该覆盖面。

## 现役UI和安全验证

- ApiKeyCredentials / ProviderModelList 多数保存走 PUT /api/config；ModelEditPanel metadata 走 /providers/{provider}/models/{id}；settings/api.ts 目前只映射 providers/summary。
- 主窗口通用 /api/agents 根路径创建未映射，而 /api/agents/{id} 及正式 /lingxi/v1/agents 已有；真实 T07 页面检查使用正式创建入口，缺口留 T08 修。
- settings_ui::snapshot_route 当前透传 preferences 根对象，search.api_keys 可漏出；T08 首批必须改明确脱敏投影并合成秘密实际HTTP验证。
- T07 A14已证明旧实验字典单键关闭/快照只读/未知字段/重启保留，未验证正式导入/导出入口。T08需查实际存在的入口，不为验收虚构一个原来不存在的按钮；角色卡导出实际解包核对默认无秘密，导入损坏不留半成品。
- 109责任叶(D10 67/D21 42，含39供应商)及A15/A16逐项验证；技能/角色卡/身份设备已有R06真实链可复用，不整轮重跑R06，不凭旧通过直接领R07通过。

本说明汇集只读源码调查，事实需以实施时最新源码核对；不代替完成结果或运行证据。

## 核心落地细化（仍未实施）

以下是 T07 收口期间的只读设计补充。没有修改运行代码，没有运行 T08 Cargo、真实供应商请求或付费请求；函数签名为待实施的共享约定，不是现有接口，也不构成验收通过。

### 启动装配与业务可用状态必须分开

`main.rs` 目前只在 `resolve_model_plane` 返回 Some 时设置 gateway、credentials、network、source。`lib.rs` 根据这些依赖一次性装配真实 turn provider、辅助执行器、operation service、记忆等消费者，RunSupervisor 随后捕获 provider。仅在保存设置时把 ServiceState 中某个 None 改成 Some，不能修复已经捕获 None 的对象。

建议生产 main 在两种启动情况下都装配相同共享对象：已有配置使用已验证种子；全新无配置使用已验证空种子，并额外持有明确的未激活状态。首次成功发布后切换同一状态和同一批对象，不替换服务、会话或供应商执行链。默认 ServiceDeps 和显式注入 turn_provider 的测试入口保持原义；不能仅凭 `source == None` 推断所有注入实例都未激活，因为已有测试会直接注入 gateway。

可采用以下最小依赖约定（名称可随实现调整，语义须保持）：

```rust
// 仅生产组合根设置；None 保留既有依赖注入的行为。
ServiceDeps::model_plane_activation: Option<Arc<AtomicBool>>

// 组合根使用真实共享对象；业务接口另外检查激活状态。
ServiceState::wired_model_gateway(&self) -> Option<&Arc<ConfigModelGateway>>
ServiceState::wired_credential_service(&self) -> Option<&Arc<CredentialService>>
ServiceState::wired_operations(&self) -> Option<&Arc<OperationService>>
ServiceState::wired_network_plane(&self) -> Option<&Arc<NetworkPlane>>
```

不能简单把当前 getter 在未激活时全部隐藏、再继续用它们装配：`MediaService::new(&state)` 缓存 operations；`ConversationApi::from_state` 缓存 gateway；自动模型审阅器只在 bootstrap 的 gateway/credentials 均存在时安装。以上位置必须使用装配 getter，或明确传入共享对象。office、辅助模型、压缩、guard、worker 回调、memory、operation service 同样需要保留同一批共享句柄。运行时 getter 才按 activation 保留当前未配置错误；RunSupervisor 还须在真实模型调用入口读取同一 activation，继续产生 `NoProviderConfigured`，不能改成已有 provider 但请求失败。

已核对使用面包括 `main`、`lib`、`management`、`chat_settings/chat_wire`、`settings_ui`、`automation/automation_activity`、`diary/persona/dream_service`、`conversations/session_collab`、`git_panel`、`web_tools/browser`、`media/media_admin/media_generate`。这些调用方有的是启动捕获，有的是每次请求读取；实施时须逐一分清，不能仅搜索 getter 并机械替换。`management::reload_models` 在未配置时的 404 / `details.reason=model_plane_unconfigured` 已有 R05 真实测试，应继续保留；其他凭证管理端点同理。thinking 的既有 503 / `model_state_unavailable` 也不应无说明改变。

### 唯一保存和发布入口

建议在 service 中新增小型 `model_configuration` 协调模块，持有原有 gateway/credentials/network/operations 共享句柄、activation、当前 Source 和一个 async 写锁；不要增加另一套供应商执行器。Source 读取返回拥有所有权的快照，避免持有普通互斥锁跨 await。所有设置写面和 management reload 进入该协调模块。

建议对调用方暴露的接口如下；调用方不能传任意文件路径、裸凭证执行器、网关或发布代号：

```rust
async fn save(
    &self,
    principal: &Principal,
    change: ModelSettingsChange,
) -> Result<ModelApplyReport, ModelApplyError>;

async fn reload(
    &self,
    principal: &Principal,
) -> Result<ModelApplyReport, ModelApplyError>;

fn source(&self) -> Option<ModelPlaneSource>;
```

`ModelSettingsChange` 应表达供应商/目录模型增删改、独立槽位设置/清除等具体动作，沿用现有权限检查。`ModelApplyReport` 至少表达 `saved`、`applied`、`generation`、`activated`、被清除的失效槽位，以及审计或兼容投影未确认；错误也携带这些结果事实，均不携带秘密。管理 reload 不新增保存动作。首个 Source 的目标由已知 runtime layout 决定，不接受 HTTP 提供目标路径。

锁内顺序为：读取当前真实 Source 及文件版本 → 只合并本次编辑字段 → 规范化目录和引用 → 完整验证配置、凭证、允许地址 → 检查外部编辑冲突并原子保存 → 记录已保存 Source/摘要 → 同代更新 credentials 和 gateway → 发布网络和外围允许地址 → 激活 → 审计。`OperationService::refresh_egress_origins` 当前可能因地址解析失败而返回错误，可先生成已验证的地址集合，再在发布阶段无失败替换，避免网关已经切换后才发现地址非法。`NetworkPlane::apply` 本身不需要新异步发布机制。

现有 credentials.reload 最后替换映射，gateway.reload 切换快照；公共写锁可避免两次设置/重载互相穿插而产生 A 地址配 B 密钥。实施时还需验证在途请求的代际检查；不能宣称几个独立对象天然具有跨对象原子性。保存文件之后取消 HTTP、进程中断或发布失败，均可能成为“已保存但尚未发布”，应明确保存摘要与已发布摘要并允许同一入口重试。首个文件已经保存但未激活时，也应保留 Source，便于 reload 恢复。审计在生效后失败，应报 `saved=true/applied=true/auditRecorded=false`，不能虚构回滚或恢复旧凭证。

### Source 和旧格式兼容

三种情况分别验证：已有 embedded 仍写回同一 `--config`；已有 models-file 仍写回同一 runtime 文件；全新无配置首次保存建立 runtime models-file。两种真实 Source 同时存在继续按原规则拒绝，不能静默挑一个。已有但损坏或不可读继续明确失败，不能变成空配置启动。

embedded 的现行严格顶层仅接受 `home/workspace/providers/models/workers`，目前不接受 `network`。写回必须保留其他顶层字段，不能把含默认 network 的整个 ModelPlaneConfig 序列化进去，否则重启将拒绝文件。独立文件则保留未编辑的 network、provider、slot。未识别兼容元数据按既有格式边界保留；不能以编辑一个字段为由丢弃其他设置。

旧 `home/models.json` 的 Pi 形状及目录登记文件仅保留为兼容投影，真实 runtime Source 是执行权威。不得全量读取旧目录后覆盖新 Source 的秘密：例如掩码 api_key 表示“保留当前 Source 的值”，不能把目录中较旧的 key 重放。只应用明确修改字段。兼容投影失败但真实配置已发布时，要报实际生效并附投影失败，而不是让用户误以为保存未执行。现有 `models/chat` 助手 YAML 与 `chat_settings` 的 session/global 选择不是同一读链，需由明确的设置动作连接，不隐式重绑全部 operation。

### 目录进入同一网关快照

建议扩展每个 ProviderConfig 的可选目录 `models: Option<Vec<CatalogModel>>`。`None` 代表旧格式，加载时一次性从已有 operation bindings 推导明确目录；`Some([])` 是明确空目录，不能回退槽位把删除模型复活。CatalogModel 需携带模型 id、能力、compat 和界面需要的非秘密分组元数据；所有字段与既有 metadata 的映射应显式定义。

在 gateway Snapshot 中建立 `(provider, model)` 索引，route、capabilities、compat 必须从同一个快照解析。当前显式双 pin 若未在 13 个槽位中找到模型，会构造 metadata=None 的 route；普通文本场景仍可能取同供应商凭证并发出请求，故需要在取凭证之前拒绝未知组合，不能只依赖“缺能力”间接拒绝。

建议提供脱敏目录读取接口，供设置和会话选择共用：

```rust
fn catalog_models(&self) -> Vec<PublishedModelDescriptor>;
fn describe_model(
    &self, provider: &str, model: &str,
) -> Result<PublishedModelDescriptor, ModelRouteError>;
```

13 个 operation 的绑定各自保留。一个已列入目录但未绑定任何槽的模型，可以被合法显式选择并获得正确能力；未知或已删除模型不能借旧槽位获得权限。删除目录模型时，在同一变更中明确清除相应槽位并回报结果，或拒绝引用冲突；不能保留悬空引用继续新调用。旧会话选中已删除模型应明确报不可用，不静默换成其他模型。`settings_ui` 当前把目录和槽位取并集，须改为以已发布目录为有效选择范围，避免旧槽“复活”模型。

`SessionCollabTools` 目前持有启动时的 `models: Vec<String>`，`session_collab::production` 只调用一次 `available_models`。生产构造应改持共享 gateway，每次列出/校验模型时读取当前已发布目录；单元测试现有静态列表入口可以保留。否则新增、删除模型虽已发布，协作工具仍会使用旧列表。另需逐项核对 UI 的 knowledge 槽与 Rust 六个 AuxiliarySlot 的映射，不能凭名称把它静默映射到 summarize。

### 多 header 与 key 的统一凭证出口

沿已有 `AuthConfig → CredentialAuthKind/reference → CredentialService → ApplicableAuth → dispatch.apply_auth` 扩展组合材料。kernel reference 仅增加必要种类，不放原始秘密；所有请求仍通过 `ProviderCredentialPort.resolve(&route)`，沿用 OAuth 刷新、网络策略、时限、配额、用量和错误脱敏。新保存/探测路由不能自行用 reqwest 带密钥旁路请求。

header 名按大小写无关处理，沿用现有禁头、token 名、空值和 CRLF 检查；旧 TS 的自定义同名头覆盖默认 key 头的规则应显式保留。不能用重复 `.header` 追加两个 Authorization。所有组合材料都要纳入错误清洗和导出脱敏；header 值按潜在秘密处理，编辑时沿用 secrets.write 检查。目前秘密补丁路径只覆盖 api_key，不能照搬为多 header 新增无权限出口。

### 建议分工与最小验证顺序

可分开编辑的边界如下，先约定 AuthConfig/CatalogModel 公共结构，避免多个代理同时改同一文件：

1. adapters 的配置验证、目录索引、网关解析和能力快照；此项拥有 `models/config.rs` 的新类型定义。
2. service 的协调模块、Source 保存、main/lib 装配、management reload、RunSupervisor activation；此项拥有唯一保存发布接口。
3. 凭证种类、组合材料、dispatch、材料脱敏；与第一项先约定枚举，随后独占凭证相关文件。
4. 现有设置路由/兼容投影/前端接口适配，统一调用第二项，不自行写 runtime Source；协作模型实时列表同步更新。
5. 实际服务集成验证，仅替换外部平台响应，不替换网关、存储、凭证或真实调用链。

验证顺序应从一条闭环开始：全新无配置启动 → 保留旧未配置错误 → 保存一个供应商和明确目录/聊天绑定 → 不重启进行下一次真实调用 → 重启仍读同一 Source。随后覆盖 embedded 和已有 models-file、未绑定槽的合法模型、未知/删除模型零外部请求、operation 独立性、长期持有消费者即时生效。再验证单项编辑保留其他字段、掩码保留当前秘密、多 header 不重复且不泄露、损坏/不可写/外部冲突时拒绝覆盖、并发保存与 reload 不混密钥、已保存未发布和审计失败的诚实回执。以上均为待执行清单，不能用 T07 或 R05 的旧通过记录代替。


## 独立只读核对：技能、卡片与身份设备

109责任原文和本轮14个读取源文件摘要保存在 T08_READONLY_HANDOFF.json（D10 67、D21 42，包含39供应商定义）。未改变原责任清单或当前NOT_RUN。

技能目录、启停、组合、安装及运行提示词已有真实服务链和R06检查。外部路径保存仍需最小修补：skills::persist_external_paths 必须使用 preferences_locked 与严格根读取；只有缺失才建立空对象，损坏/不可读保持原字节并报错。PUT paths应逐项拒绝非字符串，失败时不改盘、不改内存、不发变更事件；合法空列表仍可明确清空。验证用真实HTTP分别证明损坏输入、并发单项保存、未知字段保留和重启回读。

互动卡片与角色卡应分清实际入口：character-cards 已有预览令牌、素材验证、重复确认保护、实际包与导入后部分失败诚实说明；InteractiveCard 对 show_card 结果的 /api/cards/{id} PUT/GET 未接Rust。后者需最小有界LRU缓存、受权GET/PUT、禁止联网/导航的沙箱HTML/CSP、统一接口及当前连接路径适配，并从实际Run工具结果经真实页面加载验证，不能只检查工具返回cardId。

现役技能选择和查看窗口已有Tauri实现及platform桥；r06_skills_completion::original_native_skill_viewer_current_tauri_window_and_read_boundaries会实际构建资源、跑读取边界与skill_viewer_host并要求receipt.json。早期“递延R09”不再作为当前未实现证据，本轮未运行则明确待核验。身份设备面已有SettingsContent/RustAgentTab到server/identity、AccessTab到management的接线，真实auth注册簿与WS每帧/空闲重验可复用；未见需要凭空新增一套管理链的理由，仍需当前候选回执。

通用配置import/export按钮/路径未在本轮检索的设置General/WorkTab及config路由找到，现役明确存在记忆和角色卡包导入导出。A14联验应先明确实际产品入口，不能把快照读取或直接改合成文件称作导入已完成。上述补充均未修改运行代码、运行Cargo或外发请求。
