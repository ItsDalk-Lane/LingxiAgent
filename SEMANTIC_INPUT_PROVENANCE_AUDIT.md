# Semantic Input Provenance Audit — Phase 5（第四轮）

> 分支 `feature/model-call-observability`，基线 HEAD `6b93929e`（2026-08-21）。
> 本文是 Phase 5 Semantic Input Provenance 的 **Step 1 Prompt Construction Audit**，
> 覆盖 MC-01～MC-10 全部生产 Model Call 路径的 Semantic Input 构造链。
> 结论以本轮代码事实为准；与历史审计（MODEL_CALL_OBSERVABILITY_AUDIT.md /
> MODEL_CALL_CLOSURE_DELTA.md）冲突处，本文新增 addendum，不回写历史。

## 0. 三个层次的严格区分（本轮第一性原理）

| 层 | 定义 | 本轮是否处理 |
| --- | --- | --- |
| A. Semantic Input | 模型在业务语义层收到的输入（systemPrompt / messages / tools / prompt+reference media / audio+language） | ✅ 建图 |
| B. Provenance | 每部分输入的类别 / 来源 / 位置 / 精确度 | ✅ 本轮全部工作 |
| C. Provider Request | compat + serialization + adapter 变换后的 wire payload | ❌ 下一轮（Phase 6） |

## 1. Prompt Construction Matrix（Step 1 交付）

| Path | Semantic Request Boundary | System Builder | Message Builder | Tool Builder | Media/Audio Input | Flattening Point | Source Still Known At | Provenance Potential |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| MC-01 Pi AgentSession | streamFunction 包装（`lib/pi-sdk/model-call-stream-observer.ts:101`；SDK 侧对应 agent-loop.js:194 组装 `{systemPrompt, messages, tools}` 后调用） | `core/agent.ts:1268 buildSystemPrompt`（parts.join("\n")）→ `core/session-coordinator.ts:2011-2018` freeze snapshot → resourceLoader proxy(:2096-2120) → SDK `system-prompt.js` customPrompt 分支末尾追加 append/skills/cwd | SDK sessionManager JSONL → agent.state.messages → `convertToLlm`（agent-loop.js:185）；role 语义：`user`/`assistant`/`toolResult`（toolResult 独立 role，非 user 包装） | `getToolsSnapshot` → `lib/pi-sdk/session-options.ts:172` 名字 allowlist → SDK 注册 → `agent.state.tools`（AgentTool，标识字段 `name`） | N/A | buildSystemPrompt 的 `parts.join("\n")`（多段一次拍平）；SDK 层再拼 append+skills+cwd | buildSystemPrompt 各 part 构造点；session snapshot freeze 点；streamFn 边界（结构化三元组仍完整） | **FULL**（system 前缀可 runtime 前缀证明；SDK 尾部 structural） |
| MC-02 cache-preserving AgentRun | `lib/llm/cache-preserving-compaction-agent-run.ts:348-353` AgentContext 构造 + 各 `runTurn(instruction)`（:498 strict / :524 repair） | 原样取 session 最终 system prompt（`core/session-compactor.ts:1763` / compaction-guard-ext.ts:312） | `session-compactor.ts:1555-1564` normalize 后 liveMessages + instruction（:377 buildCompactionInstructionValue）+ repair（:138 createRepairInstruction） | placeholderTools（runner :118-131，保留 name/schema，execute 换占位） | N/A | instruction 文本内部多段一次拼接；recovery/repair 由 SDK loop 追加 assistant/toolResult | runner 构造 AgentContext 与 instruction 时 | **FULL**（system 整段 structural，messages/tools/instruction exact） |
| MC-03 Pi native compaction | SDK `compaction.js:447-449 completeSummarization` 内 streamFn 调用（Lingxi 经同一包装可见） | SDK 生成 `SUMMARIZATION_SYSTEM_PROMPT`（Lingxi 镜像于 `lib/pi-sdk/compaction-request-shape.ts:6`；**镜像为手工副本，SDK 不从包出口 re-export**，`dist/core/compaction/utils.js:139` 未进 index） | SDK `serializeConversation` 拍平为单条 user 消息（compaction.js:469-483，`<conversation>`/`<previous-summary>` 标签包裹） | 无 tools | N/A | Lingxi 只传 `customInstructions`（session.compact 入参）；其余全部 SDK 内部派生 | streamFn 边界（systemPrompt 与单条 user 消息可见，内部组成不可拆） | **PARTIAL**（system=structural（镜像非同源数据结构）；messages[0]=structural；customInstructions identity-only） |
| MC-04 callText | `core/llm-client.ts:509-522`（system 消息 merge 完成 + normalizedMessages 形成；provider body 构造之前） | caller 传入 `systemPrompt` + messages 中 system role 合并（"\n" 连接） | caller 构造 messages（约 22 个生产 call site，见 §3） | 无 | multimodal content 数组（vision/appearance） | 各 caller 的 userContent 多段拼接（详见 §3）；merge system（llm-client.ts:509-522）；codex 空系统注入 `DEFAULT_CODEX_UTILITY_INSTRUCTIONS`（:566，adapter_injected） | caller 构造处（显式 provenance）；callText 边界（fallback） | **FULL**（显式迁移后；未迁移 caller 走 structural fallback） |
| MC-05 Anthropic probe | `lib/llm/provider-client.ts:327-336` 请求体构造 | 无 system | 固定 `[{role:"user", content:"."}]` | 无 | N/A | N/A（常量） | 请求体构造点 | **FULL**（exact；不保存 "." 值） |
| MC-06 Image HTTP | 各 adapter body 构造（`core/media-adapters/*.ts`）；语义边界在 `core/media/image-task-runner.ts:404-478` 的 params | codex 用固定 instructions；其余无 | codex input 数组 | codex image_generation tool | `params.prompt` + `params.image[]`（`core/media/submit-image.ts:44-61`） | prompt/reference 进 wire body 后不可逆 | submit-image params 构造点 | **FULL**（media_prompt/media_reference locator 指向 params 位置，不含值） |
| MC-07 Dreamina/Jimeng CLI | `plugins/jimeng-cli/adapters/dreamina.ts:521-546` submit + argv 构造（:593-656） | 无 | 无 | 无 | argv：`--prompt` + `--images/--image`（本地路径，禁止进 provenance） | CLI argv 即终点；进程内 wire 不可见 | buildImageSubmitArgs/buildVideoSubmitArgs 构造点 | **PARTIAL**（CLI 内部 OPAQUE；argv 语义输入 exact） |
| MC-08 Video HTTP | `core/media/universal-media-manager.ts:735-837` `_submitVideoWithinTrace` + adapter body | 无 system | agnes body | 无 | `prompt` + `image`（参考图）；duration/resolution/fps 为 config 非语义输入 | 同 MC-06 | params 构造点（:758-771） | **FULL**（同 MC-06；config 不进 provenance） |
| MC-09 Speech Recognition | `core/speech-recognition-service.ts:392-476` `_transcribeWithAccounting` + `core/speech-recognition/adapters.ts` 各 body | 无 | mimo/dashscope 用 messages 数组包 audio | 无 | audio 字节（Blob/base64/dataURL 四形态）+ `language` hint | audio 进 wire 后不可逆 | service 层 file/language 解析点（:224-304） | **FULL**（audio_input/language_hint locator，不进内容） |
| MC-10 Pi direct summary | `lib/llm/observed-pi-direct-summary.ts:58` observePiDirectSummary（facade `lib/pi-sdk/index.ts:229-259`，无 streamFn 分支） | SDK SUMMARIZATION_SYSTEM_PROMPT（同 MC-03） | `currentMessages`（Lingxi 拥有：diary `lib/diary/diary-writer.ts:355-402`）+ `customInstructions`（:366-368 固定文案）+ `previousSummary` | 无 | N/A | SDK 内部 serializeConversation | facade 参数边界（三元组结构化可见） | **FULL**（messages/customInstructions/previousSummary 三段 exact） |

## 2. MC-01 system prompt 构造链（重点审计）

**Lingxi 侧（customPrompt，来源逐段可知）**——`core/agent.ts:1310-1576` `parts.join("\n")`，按序：

| # | part | 来源 | 拟定 category |
| --- | --- | --- | --- |
| 1 | 平台行「你运行在灵犀平台上」 | runtime 常量 | platform_instruction |
| 2 | `# 执行环境`（getPlatformPromptNote） | runtime 平台探测 | platform_instruction |
| 3 | `# 用户档案`（resolveUserName + user.md） | user profile 文件 | user_profile |
| 4 | agentsMd（identity + yuan + AGENTS.md 模板，`this.personality`） | persona 文件链 | persona |
| 5 | 样貌 prompt（appearance summary，条件注入） | agent appearance 资源 | persona（source=agent.appearance） |
| 6 | 输出展示纪律行 | runtime 常量 | platform_instruction |
| 7-9 | 记忆规则 + `# 置顶记忆` + `# 记忆`（memoryBlock，条件注入，位于 cache 分界线后） | memory 规则模板 / pinned.md / memory.md | memory_context |
| 10-17 | 工具纪律 / Session 文件 / UI 上下文 / subagent 协作 / 本机应用控制 / 行动纪律 / 网页优先级 / 主动技能（各条件注入） | runtime 指令块 | platform_instruction（各带独立 source.id） |
| 18 | `# 团队` roster（条件注入） | agent roster | agent_roster |
| 19-21 | Session started at 时间快照 + 快照说明 + 04:00 日界说明 | runtime 每次构建 | session_instruction |

无 experience 段（当前代码事实：buildSystemPrompt 无该 part；不制造不存在项）。

**SDK 侧尾部（append + skills + cwd）**——`system-prompt.js:7-34` customPrompt 分支：
`customPrompt + "\n\n" + appendSystemPrompt + <project_context> + formatSkillsForPrompt(skills) + "Current working directory: cwd"`。

**可证明性**：最终 `context.systemPrompt.startsWith(customPrompt)` 可在 streamFn 边界用
**真实冻结快照对象**做 runtime 前缀验证（非模板重建）——验证通过则快照 provenance 的
span 平移 0 后对最终 prompt 成立（exact）；验证失败 → 整段降级 structural。SDK 尾部
（append/skills/cwd 混合）不可拆分 → 单一 structural 尾段 + skills/agents file
identity-only 段（§一百一十二纪律：不猜 span）。

**Snapshot 冻结链**：`session-coordinator.ts:2011-2054`（build/restore）→ `core/session-prompt-snapshot.ts`（v1）→ proxy loader(:2096-2120) → restore 时 `finalSystemPrompt` 直接覆盖（:2549-2551，`_applyFinalPromptSnapshot` :7189-7199）。
**结论**：snapshot 需要附带安全 provenance metadata（不含内容副本），否则 restart 后只能按当前 persona/memory 重建来源（违反 §四十七/八十六）。旧 snapshot（无 provenance 字段）恢复时诚实降级 structural。

## 3. MC-04 caller 迁移清单（Step 10 输入）

多段拼接被提前 flatten 的重点 caller（改造核心）：

| caller | file:line | 组成段 | 拟定 categories |
| --- | --- | --- | --- |
| memory fact extraction | `lib/memory/deep-memory.ts:261-270` | timeContextBlock + previousSnapshot + currentSummary | task_input / previous_summary / task_input |
| memory compile today/daily/longterm/editable | `lib/memory/compile.ts:360-366,417-437,666-672,991-997` | prevDraft/prevLongterm/prevFacts + delta/newContent/newFacts | previous_summary / task_input |
| rolling summary 初次 | `lib/memory/session-summary.ts:928-932`（system 侧 794-920 内嵌 5 段） | prevSummary + convText + budgetText；system 内嵌 persona/userProfile/memory/roster/format | previous_summary / task_input / format_constraint；system 侧 persona/user_profile/memory_context/agent_roster/format_constraint |
| rolling summary repair | `lib/memory/rolling-summary-format.ts:114-131` | issues + `<draft-summary>` 草稿 | format_constraint / task_input |
| dream 全阶段 + 两类 repair | `lib/memory/dream/model-runner.ts:89,105,120` | JSON.stringify(payload) 单串 + repair 追加段 | task_input / format_constraint |
| diary 最终生成 | `lib/diary/diary-writer.ts:692-750` | summaryText + activitiesText + memory + 写作指导 + 约束 + 日期（≥6 段双重 flatten） | task_input / memory_context / task_instruction / format_constraint / session_instruction |
| title / activity×2 | `core/llm-utils.ts:253,384,446` | user 段 + assistant 段（+工具段） | task_input（各段） |
| rc-summary | `core/slash-commands/rc-summary.ts:184-190` | 标题 + userText + assistantText + toolStr | task_input |
| approval 首调 / 二调 format repair | `lib/approval-gateway.ts:641-649` | JSON payload；二调追加 FORMAT_CORRECTION_PROMPT 为**独立 user message** | task_input；format_constraint |
| vision 两形态 | `core/vision-bridge.ts:897-911,934-957` | 指令 text block（userRequest 内嵌末行）+ image block | task_instruction / current_user_input（task_input）/ media_reference |
| install-skill guard | `lib/tools/install-skill.ts:103-129` | 审查指令 + skillContent 同一 user 消息 | task_instruction / task_input |
| agent appearance | `lib/agent-appearance-summary.ts:296-309` | 指令+agentName text block + avatar image block | task_instruction / media_reference |
| health check | `server/routes/models.ts:24,237` | 常量单段 | task_instruction |
| translate_skill_names / generate_agent_id / generate_description | `core/llm-utils.ts:301,554,607` | 单段 | task_input |

`buildUtilityPromptLayout`（`lib/llm/prompt-layout.ts:14`）使用者仅 4 文件（deep-memory / compile / session-summary / dream），全部在 memory 域，`cacheGroup`+`templateVersion` 即 template identity（§二十三：source.id=cacheGroup、source.version=templateVersion，不重算 hash，不动 `cachePrefixHash`）。

## 4. 已知 Opaque / Structural 源（Known Gaps，诚实清单）

| 源 | 位置 | 精度 | 原因 |
| --- | --- | --- | --- |
| Pi native summarizer system prompt | MC-03 | structural | SDK 常量不随包出口，Lingxi 镜像为手工副本（compaction-request-shape.ts:6），非「同一数据结构」（§三十二例外不成立）；runtime 等值验证可作旁证但不升 exact |
| Pi native summarizer messages[0] 内部组成 | MC-03 | structural | `<conversation>`/`<previous-summary>` 序列化发生在 SDK 内；按标签解析最终字符串 = 禁止的反推 |
| SDK system prompt 尾部（append+project_context+skills+cwd 混合段） | MC-01 | structural（identity-only 子段） | SDK 拼装，内部 span 不可定位；skills/agentsFiles identity 来自冻结快照 |
| MC-02 system prompt 整段 | MC-02 | structural | runner 取 session 最终 prompt，不做前缀证明（诚实降级；messages/tools/instruction 均 exact） |
| Dreamina/Jimeng CLI 内部 wire | MC-07 | opaque | 外部进程边界；argv 语义输入仍 exact |
| MC-03 无 provider_request_prepared | 既有事实 | — | summarizer options 无 onPayload（事件缺失即真相，沿用） |
| provider-compat normalizeProviderPayload 的字段改名/搬移 | 全路径 | 不建模 | §八十一：Provider Request Capture（Phase 6）处理；唯一例外：codex 空系统注入 DEFAULT_CODEX_UTILITY_INSTRUCTIONS 在注入点标记 adapter_injected |

## 5. 复用与不重复建设（§十/§十三/§二十三）

- `prompt-layout.ts` 的 cacheGroup/templateVersion/cachePrefixHash = template identity，直接复用为 source.id/source.version；不新建 utilityTemplateName/Revision，不动 cachePrefixHash。
- `SessionPromptSnapshot`（v1）以**附加可选字段**方式携带安全 provenance metadata（仅 category/locator/sourceId/templateVersion 形状，无内容副本、无 sections[].content）；不建 PromptSnapshotV2 平行体系。版本不升级（additive-optional，旧 snapshot normalize 后无该字段 → structural，诚实降级）；持久化指纹按仓库规程 repin（compatible）。
- 复用 ModelCallRecorder/ModelCallScope/TraceScope 管线：provenance 为 recorder 持有的 per-call sidecar（随 call GC，无全局 Map）；Observer 事件仅携带 summary（§三十九）。

## 6. 决策记录（audit 阶段）

1. **MC-01 current_user_input 判定**：以「trace ingress（session.prompt 包装）+ 首 turn 内最后一条 role=user 消息」判定，依据 pi-ai Message 形状事实：toolResult 是独立 role（非 user 包装），loop 只追加 assistant/toolResult，故 turn 内最后 user 消息即触发输入——这是 runtime 可证不变量而非「数组最后一项」启发式（§五十一）。turn 标记缺失（agent.continue、无 ingress 场景）→ conversation_history，不猜。
2. **MC-03 精度**：见 §4 第 1 行；customInstructions（Lingxi 显式传入 SDK 的唯一语义输入）以 identity-only structural 段记录。
3. **MC-06/08 config 参数**（duration/resolution/fps/size）：非 Semantic Model Input，不进 provenance（§七十三）。
4. **MC-09 四种 audio 形态**（Blob/base64/dataURL/文件）：一律 audio_input + locator（root=input path=["audio"]），不记形态值。
5. **Section 数上限**：per-call sections 以消息级/工具级为粒度（§一百一十），上限 1024，超限尾段折叠为单段（记录折叠事实），防病理膨胀。
