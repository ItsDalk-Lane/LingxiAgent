# P0.3 调用图与缺陷实锤（2026-09-07，基于 fc4a6df7 源码）

## CG1 发送链（F1/F2/F10）

编辑器 → `submitEditorMessage`（InputArea.tsx:1790）→ `dispatchComposerSend`（composer-send.ts:100）→ `ws.send`（composer-send.ts:406）→ `server/routes/chat.ts:2444 onMessage` → `hub.send`/`submitDesktopSessionMessage`（chat.ts:2643 → desktop-session-submit.ts:282）→ `session_user_message`（desktop-session-submit.ts:552-572，带 clientMessageId）→ `assistant_run_start/end`（chat.ts:1659/1962 → beginAssistantRun 735 / finishAssistantRun 783「唯一正常 finalize 入口」）。

实锤：
- **F10**：`InputArea.tsx:1795 const text = rawText.trim()`；`editor-serializer.ts:184 .replace(/\n+$/,'').trim()`；队列编辑 `InputArea.tsx:2090 editQueuedText.trim()`。服务端不 trim（chat.ts:2507）。
- **F1**：清理发生在派发前（InputArea.tsx:1997-2016），而派发器有 6 个静默 return 分支（composer-send.ts:155/160/260/293/397/422），每个都丢已清空正文；`dispatchComposerSend: Promise<void>` 不区分 blocked/failed/submitted；ws null 分支（397）乐观消息已建。
- **F2**：自动续发 effect `InputArea.tsx:2040-2064`：先 `removeQueuedTurnInput`（2053）再 400ms 后异步派发（2059），无会话租约；`handleQueuedInsertNow`（2066-2077）用 streamingSessions 外观猜 interject。
- 回执关联：`clientMessageId` 由 composer-send.ts:321 生成，服务端透传（chat.ts:2646），`session_user_message` 携带（desktop-session-submit.ts:554），前端 `confirmOptimisticUserMessage`（ws-message-handler.ts:1009）。
- `applyStreamingStatus`（ws-message-handler.ts:403-443）有身份栅栏（identitiesMatch）；run 生命周期归 StreamBufferManager。
- 测试缺口：无 composer-send 专测、无 400ms effect 测试、无队列 slice 单测。

## CG2 迁移链（F3/F9）

旧 pins（agents/<id>/pinned.md + pinned-memory.json）→ `migratePinnedMemoryToTenets`（core/pinned-tenets-migration.ts:109，engine.ts:3319 启动接线，先于 server 写入开放）→ `addTenetDirect` 逐条（受 300 字符/200 条限制，逐条 catch 继续）→ 无条件 rename .migrated → `activeTenets` → `buildTenetsPromptSection`（tenets.ts:281）→ core/agent.ts:1537 注入 system prompt（memory.tenets 块，cache 分界线后）。

实锤：
- **F9**：`decideTenet`（tenets.ts:254）与 `addTenetDirect`（tenets.ts:222）都按 active **总数**算配额；`isTenetError`（64-67）传 code 仍匹配任意 tenet 错误；pin_memory（pinned-memory.ts:70-75）LIMIT_REACHED 返回假成功文案。
- **F3**：迁移 JSON 优先（不沿旧 mtime 权威规则，旧规则见父基线 pinned-memory-store.ts:111-120 `mtimeMs > +1`）；空 JSON items+旧 md → 回退 md（复活删除语义）；损坏 tenets.json 被 `readTenetsFile` 吞成空库（97-110）；无收据/备份/状态机/恢复工具。
- 旧版合法内容：仅非空约束，无长度/条数限制（父基线 serializeItems）。
- `readTenetsFile` 归一化 dedupKey（小写+去句尾标点）比任务书 P1.5 允许的迁移去重规则更宽——迁移须改用「换行+边界空白归一后精确比较」。
- 设置页 AgentTenets.tsx 无数值配额显示（仅 409 错误内联）；工具失败契约 = `toolError(text, {errorCode})` + isError（lib/tools/tool-result.ts:23）。

## CG3 听写链（F4/F5/F6/F7）

听写（仅快捷键 Cmd+Shift+M，InputArea.tsx:1411-1423，且要求 showAudioInput=原生音频模型）→ getUserMedia（1329）→ stopAudioRecording（1201）→ dictate 分支 POST /api/media/asr/transcribe providerId/modelId=system-speech（1264）→ media.ts:107 → UniversalMediaManager.transcribeAudio（1820，normalize 包 `{ok,transcription}`）→ SpeechRecognitionService（243-340）→ system-speech-adapter.transcribe（63-121，**execFileSync 90-94，timeout 150s 阻塞事件循环，无 AbortSignal**）→ helper transcribe → 返回 `{text}` → 前端 1273 `String(asrData?.transcription ?? asrData?.text)`（**对象被 String 成 [object Object]**）。

实锤：
- **F4**：main.cjs/preload.cjs/PlatformApi 零 speech IPC；repo 零 askForMediaAccess；helper 永不自请授权（main.swift:66-79，裸 CLI 会 SIGABRT）；需新建 ObjC++ Node-API 授权桥。
- **F5**：main.swift:121-125 主线程 `semaphore.wait`；超时用 `task.finish()` 冒充取消；空文本 ok:true 返回；durationMs 恒 0。
- **F6**：execFileSync 阻塞；resolveHelperCommand（23-45）候选2假设 process.execPath=Electron，但打包形态 server 是独立 node（dist-server/<arch>/node）→ 解析不到 Resources；pack/dist 均未接 build:speech-helper（extraResources 210-219 已映射 dist-speech→speech/macos）；resign-adhoc.cjs:99-102 与 sign-local.cjs:74-76 已覆盖 helper 签名。
- **F7**：dictateMode/toggleDictateMode/isMacDictateAvailable（InputArea.tsx:505-570）**定义了但 JSX 未接线**（无麦克风按钮）；录音卡只在 `showAudioInput` 时渲染（2403）；自动丢弃 effect（1433-1438）在文本模型下丢弃录音；快捷键门（1397-1409）绑 showAudioInput。

## CG4 标签链（F8）

原始 delta → ThinkTagParser（core/events.ts:112，词表 think/thinking/mm:think，无孤儿规则）→ MoodParser（events.ts:92，词表 mood/pulse/reflect，**dropUnknownOrphanClosers: true = 链尾**）→ normalizer → canonical segments（server/routes/chat.ts:1298-1346 feedReservedTagText/flush）。历史：`splitReservedTagSegments`（shared/reserved-tag-stream.ts:271-305，**恒定开孤儿规则**）被 message-parser.ts:51 / format.ts:112 / history-segment-sanitizer.ts:28 / assistant-block-builder.ts:34 消费。

实锤：
- **F8**：scanner 只跟踪词表内 openTag；未知配对闭标签（如 `</details>` 配 `<details>`）在链尾/最终边界被 CLOSE_TAG_SHAPE（reserved-tag-stream.ts:51）当孤儿吞掉；转义 `\</tag>` 在链头被消费成字面量后，链尾无法区分（无保护传递）。
- sanitizer `!changed → return source`（history-segment-sanitizer.ts:47）：无 leading block 时返回原文——抵消孤儿清理的既有行为须保留语义但修复 T16 场景（仅孤儿残渣、有 leading block 被删时重建会丢孤儿闭标签）。
- 词表外闭标签形状规则是 04f90d2b 新加的；mm:think 在 llm-client/message-utils/session-coordinator 各有正则副本。

## CG5 TTS 链（F11/F12）

TTS 配置（PUT /media/speech/config → setSpeechConfig → preferences.speechGeneration.providerDefaults）→ `_submitSpeechWithinTrace`（universal-media-manager.ts:829-1011）→ `_resolveSpeechTarget`（1013-1062，credentialProviderId/credentialLaneId 来自 media-execution-target-resolver）→ `observedSubmitCtx`（882-885，含 `config: _createConfigBridge()`=**图片配置** + `speechConfig: _createSpeechConfigBridge()` 存在但**无 adapter 读**）→ speech.ts 三云适配器读 `ctx.config.get("providerDefaults")[credentialProviderId]`（72/148/239=**读图片域**）→ TaskStore.add（945-966，**status:pending**）→ response 投递跳过 poller/deferred/task:register（968-998）→ 任务**永远 pending**。

实锤：
- **F11**：语音 providerDefaults 是死配置；适配器读图片配置域，按 credentialProviderId 索引（与 UI 写入的 runtimeProviderId 键不一致）。
- **F12**：response 同步成功后任务不落 done；poller fake-async 快路径（poller.ts:335-360）只覆盖被轮询任务；`destroy()` 不 flush（task-store.ts:626-631）。
- 朗读前端 AssistantMessage.tsx:214-244 用 delivery:response + tasks[].files 播放。
