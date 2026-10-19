# T08 供应商 HTTP 只读接口清单

2026-10-18：准备状态，未改产品源、未运行 Cargo、未请求真实供应商或账户。T07 明确放行后方可实施。HTTP 适配计划由 `provider_routes.rs` / `provider_probe.rs` 独占；保存、重载、授权探测以 `model_configuration` 唯一发布者为准；注册、权限和界面映射由主控整合。以下是待实现合同，不能当已存在接口或验收通过。

已读 `T08_IMPLEMENTATION_READINESS.md`、`T08_READONLY_HANDOFF.json`；同名 `.md` 不存在。原要求为 D10 67 + D21 42，共109责任，其中39内置供应商定义。下面以当前 `server/routes/providers.ts`、`models.ts`、`core/provider-registry.ts`、`shared/provider-auth.ts`、`secret-custody.ts`、`lib/llm/provider-client.ts` 和39个实际注册插件为准。

## HTTP 请求与返回

新路径以 `/lingxi/v1` 对应旧 `/api`，不要再启一套执行器。已有 models/set、models/switch 和 summary 继续由现有所有者整合，不重复注册。

| 旧入口 | 输入 | 输出与关键行为 |
|---|---|---|
| GET `/providers/summary` | 无 | `{providers:{id:...}}`；已配置目录 + OAuth声明 + 未配置内置定义。字段见下文；不刷新OAuth、不外发 |
| GET `/providers/:name/api-key` | provider路径 | `{api_key:string}`，只显读已保存目录key，不是OAuth access token；`providers.manage` **且** `secrets.write` |
| POST `/providers/fetch-models` | `{name?,base_url?,api?,api_key?,headers?}`，name或base_url必需 | `{models,ignoredModels?}`；OAuth本地目录另带source；失败`{error,models:[]}`。只缓存发现结果，不自动添加可用模型 |
| GET `/providers/:name/discovered-models` | provider路径 | 无缓存`{models:[],fetchedAt:null}`；有缓存返回过滤后的models、fetchedAt、可选ignoredModels；`providers.manage` |
| POST `/providers/test` | `{name?,base_url?,api?,api_key?,headers?,model_id?}` | `{ok,status,skipped?,error?}`；base_url缺失400；有key无api为400；其他探测失败通常HTTP200内`ok:false` |
| PUT `/providers/:name/models/:modelId` | 模型metadata补丁 | `{ok:true}`；是upsert，不存在也会加入；裸字符串升级对象；成功刷新全局模型目录并发`models-changed`，agentId=null |
| DELETE `/providers/:name/models/:modelId` | provider/model路径 | `{ok:true}`；旧行为按ID过滤，模型不存在也成功。新发布者须同时处理有效槽引用，禁止旧槽复活已删除模型 |
| GET `/models` | 无 | `{models,current:id|null,activeModel:{id,provider}|null}`；列表含provider及当前选择、能力/思考/上下文元数据 |
| GET `/models/auxiliary-vision` | 无 | `{auxiliaryVision:{enabled,configured,available,unavailableReason,model}}`；reason可为disabled/not_configured/model_not_found/model_without_image_input |
| POST `/models/health` | `model:{id,provider}`或`modelId+provider`；不能按id猜provider | 成功`{ok:true,status:200,provider}`；Codex`status:0,skipped`；错误`{ok:false,error,code?,reason?}`，参数错误400 |
| POST `/models/set` | `{modelId,provider}` | 只影响后续新会话；旧返回`{ok,model,pendingModel:true,thinkingLevel}`；Rust现已实现选择面，不由provider新模块重写 |
| POST `/models/switch` | 旧`{sessionPath,modelId,provider}`，Rust现接受sessionId | 活跃会话切换，运行/压缩冲突409；旧返回`{ok,model,adaptations,thinkingLevel}`；需主控沿现行会话准入整合 |

providers管理写/探测/发现面要求`providers.manage`，明文秘密写入另要求`secrets.write`。summary旧属settings读；models/list、auxiliary-vision、set/switch旧属chat范围。旧 `models/health` 在 `server/http/route-security.ts` 没有专属scoped分支，落 `/api/*` 的 STUDIO_OWNER，不能无说明扩大为任意chat主体。新路由登记须明确复核实际权限，未知路径继续拒绝。

summary每供应商应保留：type(api-key/oauth)、auth_type、display_name、base_url、api、脱敏api_key/headers、models、custom_models=[]、has_credentials、logged_in、supports_oauth、is_coding_plan、is_configured、can_delete、config_status(ok/needs_setup/invalid)、config_error、missing_fields、media_capability_bindings。API-key模型只投影用户显式目录；首次保存models=[]是正常等待添加，不当缺配置。OAuth目录来自声明，显式[]仍是主动清空；默认项不可删除、存在overlay时can_delete不同。媒体绑定含capability/runtime_provider_id/可选credential_lane_id。

`models`每模型信息需保留：id/name/provider/isCurrent、input、video/audio及各Transport/Supported、reasoning、thinkingLevels/defaultThinkingLevel、contextWindow/maxTokens、可选outputIncludesThinking/xhigh/toolUse。名称顺序为overrides.displayName→有意义SDK名→known-models→id。不要只返回id/name便称等价。

## 草稿、掩码和凭证

掩码固定为去首尾空白后等于 `********`；所有header值都按潜在秘密脱敏，不仅Authorization。API-key显读是唯一窄出口；其他读面/日志/错误/导出不能暴露秘密。

fetch与test均优先面板草稿。有效明文key，或headers含任一非掩码值（空串也被旧秘密写检测计入），就进入request-draft：不refresh、不混入已保存秘密，只借供应商静态baseUrl/api默认。无明文草稿才fresh-provider：通过唯一凭证服务的fresh resolve，不能用resolve_handle绕过OAuth刷新。只有fresh-provider分支的saved含真实已保存凭证，未给/空/掩码key才沿旧逻辑回落该fresh key。request-draft分支的saved.key与headers固定为空（只借静态api/base）；因此存在任一明文header时，空/未给/掩码key均不得借用真实保存key。草稿一项header存在不代表可暗补其他已保存key/header。

headers字段缺省=沿用saved；显式对象=整份替换，`{}`明确清空；null/非对象在旧helper为{}。对象内空值/null跳过；掩码值按大小写不敏感的同名header找回原值。HTTP名称大小写不敏感去重，后来源覆盖默认key头；新增实现不能append两个Authorization。保留现有禁头/RFC token/CRLF检查。保存时保留未知兼容字段、未编辑模型及当前Source秘密，不能用旧目录投影覆盖新执行Source。

fetch的key只trim；test旧逻辑另外剔除非可打印ASCII（保留0x20–0x7E）后trim。是否继续此清洗应明确冻结并验证，不能两处不经说明改成同样处理。

## 目录发现与探测分路

- fetch：Anthropic GET `append(base,/v1/models?limit=1000)`；其他受支持聊天协议GET `base/models`，15秒。401/403立即错误；API-key的404/网络/空目录不能降级为内置成功。成功发现写`models-cache.json`仅为缓存，不授予模型可调用权限。
- OAuth的models-json/auth-storage目录直接用产品声明目录（source=hana-catalog），不请求ChatGPT `/models`；旧sdk-auth-alias目录来自registry/default，返回source=registry/builtin。Codex与xAI目录不得通过远程数据注入地址/协议/凭证。
- 远端归一化：Anthropic取data[]的id/display_name/max_input_tokens/max_tokens；Google取models[]的baseModelId或剥`models/`名称、displayName/inputTokenLimit/outputTokenLimit；OpenAI族取data[]的id/context_length|context_window|max_context_length/max_completion_tokens|max_output_tokens。仅允许模态与已知type随远端数据进入结果，不透传api/endpoint/auth。
- Ollama在目录后POST无凭证原生`/api/show`补context/capabilities，去`/v1`后缀，最多200唯一模型、并发6、整批15秒；失败保留原模型并显式诊断，私有`_ollamaCapabilities`不得入缓存/返回。此增强也走授权网络出口。
- test：Codex明确skipped，零网络；Anthropic POST `/v1/messages`，固定`.`、max_tokens=1，model_id优先→目录匹配协议首个模型→claude-sonnet-4-6；这是**真实生成和记账**。其他受支持聊天协议只GET目录，**control-plane零模型用量**。两类旧实际网络超时都是10秒；不是15秒。
- health：必须显式合法provider/model，固定`Reply exactly OK.`、maxTokens=128、15秒；真实小文本调用经过同一Gateway/Credential/Network/Quota/Observer/Usage链。singleton trace origin=health_check，source=auxiliary/model_health/settings/user，agentId=null，不归焦点助手。Codex同样明确skipped；不能仅检查当前chat槽或HTTP连通冒充指定模型健康。
- test错误只接受2xx成功，结构化错误截500字符，不把代理HTML整页回传；秘密材料统一清洗。缓存/metadata刷新失败应报告实际已保存/已生效及诊断，不虚构回滚。

URL规范：Anthropic移除终端`/v1/messages`或`/v1`再由append避免重复v1；completions剥`/chat/completions`，responses剥`/responses`。opencode按协议切根与/v1；官方kimi-coding补/coding/v1；ollama补/v1；官方minimax/minimax-token-plan根或/v1在Anthropic协议转/anthropic。自定义域不因供应商名被强改为官方地址。

## 模型metadata与统一目录

补丁白名单：name/api/context/maxOutput/outputIncludesThinking/image/video/audio/reasoning/xhigh/thinkingLevels/thinkingLevelMap/type/defaultThinkingLevel/web/structuredOutput/operations/operationProtocol/dimensions/groupId，以及规范化inputs/outputs/compat/toolUse/visionCapabilities。

别名：vision→image（只有未给inputs、未给image时）；contextWindow→context；maxTokens/maxOutputTokens→maxOutput（canonical字段优先）。inputs/outputs按text→image→video→audio去重排序，非法明确400；显式inputs清理旧image/video/audio/vision。outputIncludesThinking:null清除覆盖。非正有限上下文/输出、非法toolUse、凭证字段混进模型条目均拒绝；官方DeepSeek的模型id=deepseek拒绝。

**每模型api覆写必须保留**，不能把编辑一个模型的api变成整个provider.protocol更改。CatalogModel/同一Gateway快照需表达目录模型协议覆写、能力、compat、operations/operationProtocol/dimensions/groupId等，且任一operation槽独立。未知或删除模型在拿凭证前拒绝；有目录但没绑槽的模型可合法显式选择。

## 39个内置定义的确定映射

字符串协议与现有 `ProtocolFamily::parse/config_name` 一一对应，不按供应商名猜协议。共29 completions、4 Anthropic、2 responses、1 Google、1 Codex、1 BigASR、1本地语音；认证35 api-key、2 OAuth、2 none。下表均为源码声明，不是联网验证。

| id | auth | api→统一协议 | 默认base_url |
|---|---|---|---|
| agnes | api-key | openai-completions | https://apihub.agnes-ai.com/v1 |
| dashscope | api-key | openai-completions | https://dashscope.aliyuncs.com/compatible-mode/v1 |
| openai | api-key | openai-completions | https://api.openai.com/v1 |
| anthropic | api-key | anthropic-messages | https://api.anthropic.com |
| deepseek | api-key | openai-completions | https://api.deepseek.com |
| deepseek-responses | api-key | openai-responses | https://api.deepseek.com |
| gemini | api-key | google-generative-ai | https://generativelanguage.googleapis.com/v1beta |
| openrouter | api-key | openai-completions | https://openrouter.ai/api/v1 |
| opencode | api-key | anthropic-messages | https://opencode.ai/zen |
| opencode-go | api-key | openai-completions | https://opencode.ai/zen/go/v1 |
| ollama | none | openai-completions | http://localhost:11434/v1 |
| minimax | api-key | anthropic-messages | https://api.minimaxi.com/anthropic |
| minimax-token-plan | api-key | anthropic-messages | https://api.minimaxi.com/anthropic |
| openai-codex-oauth | oauth | openai-codex-responses | https://chatgpt.com/backend-api |
| siliconflow | api-key | openai-completions | https://api.siliconflow.cn/v1 |
| zhipu | api-key | openai-completions | https://open.bigmodel.cn/api/paas/v4 |
| moonshot | api-key | openai-completions | https://api.moonshot.cn/v1 |
| baichuan | api-key | openai-completions | https://api.baichuan-ai.com/v1 |
| stepfun | api-key | openai-completions | https://api.stepfun.com/v1 |
| volcengine | api-key | openai-completions | https://ark.cn-beijing.volces.com/api/v3 |
| volcengine-speech | api-key | volcengine-bigasr | https://openspeech.bytedance.com |
| hunyuan | api-key | openai-completions | https://api.hunyuan.cloud.tencent.com/v1 |
| baidu-cloud | api-key | openai-completions | https://qianfan.baidubce.com/v2 |
| modelscope | api-key | openai-completions | https://api-inference.modelscope.cn/v1 |
| infini | api-key | openai-completions | https://cloud.infini-ai.com/maas/v1 |
| mimo | api-key | openai-completions | https://api.xiaomimimo.com/v1 |
| mimo-token-plan | api-key | openai-completions | https://token-plan-cn.xiaomimimo.com/v1 |
| system-speech | none | system-speech | 空，本地能力，不是HTTP地址 |
| groq | api-key | openai-completions | https://api.groq.com/openai/v1 |
| together | api-key | openai-completions | https://api.together.xyz/v1 |
| fireworks | api-key | openai-completions | https://api.fireworks.ai/inference/v1 |
| mistral | api-key | openai-completions | https://api.mistral.ai/v1 |
| perplexity | api-key | openai-completions | https://api.perplexity.ai |
| xai | api-key | openai-completions | https://api.x.ai/v1 |
| xai-oauth | oauth | openai-responses | https://cli-chat-proxy.grok.com/v1 |
| dashscope-coding | api-key | openai-completions | https://coding.dashscope.aliyuncs.com/v1 |
| kimi-coding | api-key | openai-completions | https://api.kimi.com/coding/v1 |
| volcengine-coding | api-key | openai-completions | https://ark.cn-beijing.volces.com/api/coding/v3 |
| zhipu-coding | api-key | openai-completions | https://api.z.ai/api/coding/paas/v4 |

Codex外显id为openai-codex-oauth，authJsonKey/runtimeProviderId/displayProviderId为openai-codex；不要用字符串后缀自动合并其他供应商身份。xai-oauth使用authJsonKey=xai-oauth、models-json目录、OAuth HTTP允许origin仅https://cli-chat-proxy.grok.com，固定协议头及逐模型执行头需保留；fresh OAuth覆盖凭证头，不能被旧目录Authorization/Cookie覆盖。

## 本地语音和BigASR不能硬塞聊天

现有 `ModelPlaneConfig::validate` 无条件要求所有provider为HTTP(S) endpoint；system-speech默认空地址会失败。不能填假HTTP地址，更不能将协议改成completions。其设置保存须由唯一发布者接T06既有Operation/media目录与槽位，并明确校验本地执行能力；system-speech的TTS走 `OperationService` macOS `/usr/bin/say`，非macOS明确受限；system-speech-recognition当前真实服务明确未实现。ASR设置通过adapterAvailable检查，不能把未支持项配置后显示可用。

volcengine-speech不同于Ark的volcengine聊天供应商：`speechRecognition`选bigasr-flash，现役OperationService::transcribe→VolcengineBigAsr计划，POST `/api/v3/auc/bigmodel/recognize/flash`，x-api-key、x-api-resource-id=volc.bigasr.auc_turbo、新request UUID、x-api-sequence=-1；请求user.uid也使用该材料。凭证仍经统一CredentialService，不可交给HTTP路由直接拼请求。该面真实识别计费/记账沿T06现役链，不能用文本health冒充。

旧通用providers/test的buildProviderAuthHeaders仅支持五种聊天协议，对system-speech/volcengine-bigasr会明确unsupportedApiProtocol（本地空地址还会先400）。新实现可以保留明确不适用/不可用并由媒体/本地能力面显示实际可用性，不能把它们送`/models`或为了按钮成功偷偷做真实语音调用。目录也使用媒体/本地声明或明确不支持通用发现，不创建假的chat模型。

## 唯一发布者需要提供的最小接口语义

名称由core最终约定，HTTP模块不能自行写runtime Source或获取裸CredentialService执行器。

1. 脱敏的内置定义/当前有效目录/OAuth状态读取；目录元数据更新与删除均调用 `save(principal,change)`；返回saved/applied/generation/activated/清除引用/审计及兼容投影实际状态。
2. 窄的已保存api-key显读：由发布者核双权限并返回目录保存key，绝不返回OAuth材料。
3. `probe(principal,request)` 为唯一授权出口：明确catalog-read/connectivity-probe/model-health、provider及必要model、request-draft/fresh-provider、当前代际、目标与期限。若使用句柄，它必须为不透明且作用域受限的一次性授权，不是可读key/header的包。网络请求由共享受控出口执行；原始秘密不回到provider_routes。
4. control-plane允许受约束GET及Ollama限定POST；真实生成和health调用复用GatewayedProvider的显式provider/model options及真实观测账，不另造reqwest密钥旁路。草稿不得落盘、不发布临时全局配置，也不能干扰并发真实Run。
5. 网络策略、TLS/代理、响应/时间上限、OAuth fresh、代际、错误脱敏同一边界；未知模型/越权/过期授权必须零外部请求。

实施验证须至少覆盖：39定义与协议不漂移；草稿优先/掩码/双权限/同名header覆盖且不泄漏；GET目录零用量与Anthropic真实记账；health非chat槽显式选择、未知/删除模型零请求；10秒/15秒及错误脱敏；OAuth别名/目录/刷新；speech保存后实际Operation链且本地能力诚实；metadata单项保留其他模型/凭证/未知字段及重启。全部用本机受控外部协议替身，不操作真实账户或付费供应商。

## 开工前 DTO 与纯函数分界补充

主控和core现已约定：`ServiceState::model_configuration()`返回共享协调器（是否Option由core最终装配决定）；`ModelConfiguration::save(&Principal,&[ModelSettingsChange])`、`reload`、`probe(&Principal,ProviderProbeRequest)`均由core定义和实现，错误统一`ModelApplyError`，禁止路由本地直接保存或发布。ProbeRequest含kind(Catalog/Connection/ModelHealth)、可选provider/model/draft，draft含endpoint/protocol/api_key/headers。**来源由core根据字段实际存在性和掩码决定，客户端不能提交request-draft/fresh-provider选择器。**

`headers:Option<Value>`若直接derive反序列化会把null折叠为None；实现须显式保留“字段未给”和“给了null”，例如路由从原Value的contains_key构造Some(Null)，或由core使用专门的present/nullable补丁类型。前者沿用、后者清空，不能混同。health的model还兼容`provider/id`字符串（首个/拆分）；另给provider冲突时400，最终送core的必须是完整复合身份。

已向core提交、待其精确函数名冻结的纯函数分界：`provider_probe`负责`build_probe_plan(kind,provider,protocol,endpoint,model)`，输出ControlPlane(method/url/body/timeout)、Generation(prompt/max_tokens/timeout/purpose)、Skipped或Unavailable之一；计划不含auth或秘密header，也不执行请求。core内部PreparedProviderProbe决定fresh/草稿、授权、代际和network，Generation走同一真实Gateway/usage。另提供目录归一化/过滤、连接结果解析、Ollama detail请求计划与结果合并纯函数。Ollama整批期限/并发/数量及请求执行由core掌握。不要复用含已应用凭证的`OperationRequestPlan`作为可见授权出口。

操作用途固定：Catalog/普通Connection为control-plane；Anthropic Connection为`.`/1 token/10秒的provider_probe真实生成；ModelHealth为`Reply exactly OK.`/128 token/15秒的model_health真实生成。provider/model目录校验、真实trace/usage身份与null Agent归属由core装配，路由不能构造一个固定成功的usage结果。


## 本次已实现边界（待主控统一编译和实链验证）

- `provider_routes.rs` 已接唯一发布者，原12面由既有路由与新增路由合并；`provider_probe.rs` 只做公开协议计划及解析，不持秘密或创建网络客户端。39定义有逐协议/期限矩阵检查；目录可信字段白名单、DeepSeek过滤、Ollama旧模式与模型凭证字段拒绝均有检查。
- `extract_legacy_provider_changes(partial,focus_config)` 返回 `{changes,agent_patch}`：全局providers剥除；内联api/embedding_api秘密迁至明确目标provider并清空助手旧秘密残留；目标提供不足报错；每助手聊天选择保留原agent配置，不生成全局Chat槽变更。headers未给/null/掩码保留差异。
- summary的媒体绑定来自T06现役七家声明；两个独立语音供应商按现役声明补展示绑定，不能据此声称当前平台可执行。Codex/xAI OAuth实际登录状态仍来自发布者，不凭内置目录宣称已登录。
- `settings_tools.rs` 已实现旧字典14项：备份、语言、时区、桥接公开地址、电脑操作启用偏好、会话思考档位、记忆开关、可编辑记忆、经验开关、助手名、用户称呼、工作目录、主题、每助手默认聊天模型。元数据明确区分“旧tool可调字典”与shared schema仅scope声明。
- 实际工具调用只消费网关注入的冻结上下文；本机LocalUser且UserSession且非ReadOnly才由AuthService签发窄settings.write，非owner，无供应商或密钥权限。其它来源在原scope证据不足时明确拒绝apply。模型参数不能伪造该上下文。只读search与apply按Read/Review区分。
- 全局仅四项安全偏好复用原setter；目标助手严格检查后写现役MemoryService/配置；主题、桥接、电脑偏好分别更新原共享消费者；电脑偏好不批准应用或提升OS授权；助手模型独立保存且下一Run按目标助手读取，不修改全局Chat。
- `r07_t08_provider_http.rs` 使用真实HTTP服务、发布者、凭证、网络、网关、用量账、工具授权、Run与journal；仅供应商响应为loopback替身。覆盖草稿不落盘、双权限、远程目录字段不获执行权、health/Anthropic真实用量、模型更新删除及14项设置实际Run、目标助手隔离、下一Run模型消费、只读/安全拒绝。

当前仅完成源码接线与格式检查，不把未运行测试、其它平台、真实OAuth/供应商、正式R07整体验收记为通过。Cargo由主控唯一时槽执行；失败记录须保留并按真实结果修复。
