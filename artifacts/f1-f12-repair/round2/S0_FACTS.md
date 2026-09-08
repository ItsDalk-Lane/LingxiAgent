# S0：基线、调用链与反例入口

记录日期：2026-09-08。性质：只读源码核实与执行前范围事实，不是 S1–S11 验收报告。
所有仓库相对路径以隔离工作树为根；本文件不包含真实用户 home、凭证或记忆正文。

## 源码与环境

- 固定基线及隔离树 HEAD：`89bc0b64bf0a9b84ef3532efaa66c23213affb70`；detached HEAD（`git branch --show-current` 无输出）。没有提交、推送、合并或发布。
- 最近提交依次为 `89bc0b64`、`fc4a6df7`、`04f90d2b`，不回滚到旧基线。
- 主代理执行前记录原工作区 clean；隔离树后续出现的 migration、tenets、新增 round2 测试及本目录变化属于正在进行的本轮工作。本文的基线事实不把这些未提交变化宣称为基线内容。
- 实际运行环境 Darwin / arm64、Node `v24.16.0`、npm `11.13.0`；符合 package.json 的 Node 24 范围。当前 package.json 无 `build:packages`，不套用其他检出的旧命令。
- 已有命令执行事实以 `COMMAND_RESULTS.json` 为准：`npm ci` exit 0，有独立原始日志和源摘要。后续测试已有主代理记录；本文不代替其结果，也不把命令名称带 green 当作通过。
- 任务书完整内容已由主代理读取；此子任务逐段核实 S0、S3–S6 及相关真实调用链。任务书规定目标，源码只说明当前行为。

## 约束与未决材料

“聊天工具栏上没有增加语音输入的组件是用户要求的”。不恢复旧输入框系统听写探测、转写回填或替代听写 UI；保留 macOS Speech 后端、宿主授权桥、原生音频附件、快捷键、上传转写和 TTS。已修 F 项只回归。

只在隔离本地工作树和合成数据中修改、构建与测试。不触碰真实用户数据，不触发真实权限窗、TCC 重置或付费服务，不 commit/push/PR/merge/release。缺平台、凭证或明确授权的真实环境验收逐项 BLOCKED。

`LINGXI_89BC0B64_REVIEW_COUNTEREXAMPLES.zip` 未在 Downloads 枚举结果中找到，不能读取或验证其 MANIFEST。旧 ZIP 原始反例与完整性核验为 **BLOCKED**；下表的 E 映射来自任务书，不冒充已核验 ZIP 内容。继续用任务书描述建立真实模块反例。旧摘录即使退出 0 也不代表修复通过。

## 真实调用链

| 链 | 文件、函数与调用者 | S0 发现及执行边界 |
|---|---|---|
| pins 启动迁移 | `core/engine.ts` 启动调用 `migratePinnedMemoryToTenets` → `core/pinned-tenets-migration.ts` 的 `migrateAgentPinnedTenets` → `runFreshMigration/resumeMigration` → `verifyTargetCommitted/archiveSources`；`lib/memory/tenets.ts` 的 `planLegacyPinnedImport/readTenetsFileStrict/serializeTenetsFile` | 收据、目标、归档必须真实文件系统验证；S1 后改动不反写为基线事实 |
| pins 批准恢复 | `scripts/pinned-tenets-recovery.mjs` main → `scanPinnedTenetsRecovery/applyPinnedTenetsRecovery` → `lib/memory/tenets.ts` 的 `importLegacyPinnedItems/planLegacyPinnedImport` | 基线 apply 独立写目标后写全局完成收据，不能声称已有 S1 共享事务 |
| 队列编辑与发送 | `InputArea.tsx` 的 `handleQueuedEditStart/Save`、自动 effect → `requestQueueFlush` → `flushQueuedHeadNow` → `dispatchQueuedItem/acquireSendLease/sendWithLease` → `prepareComposerSend` 和提交校验 | timer 更新 existing.deps 却读取初次闭包 deps；开始编辑只更新 React state，保存才取消 lease；effect 无撤销 cleanup，自动取得租约未检查聊天页/当前 SessionRef |
| WS 与历史 | `services/websocket.ts` onopen/onclose → `noteComposerConnectionOpened/Closed`；`ws-message-handler.ts` 消费接收事件；`stores/session-actions.ts` 的 sessionMessagesUrl/loadMessages/loadMoreMessages → `/api/sessions/messages` → `loadSessionHistoryMessages/resolveHistoryPageBounds` | 重连仅恢复 streamingSessions；loadMessages 有 UI hydration 副作用；分页已有 before/hasMore，不能用第一页未命中证明未接收 |
| canonical user 提交 | `core/desktop-session-submit.ts` 的 submit → `session-coordinator.ts` promptSession/_promptSessionWithinTrace → SDK AgentSession.prompt → SDK SessionManager.appendMessage | 现有 clientMessageId 在 session_user_message 事件；presentation 不写确切关联。afterInputAccepted 是 preflight 回执，不是 entry 已落盘回调 |
| assistant 表示 | raw 协议 → Think/Mood 的 ReservedTagScanner → canonical source → `components/chat/StreamingMarkdownContent.tsx` → `utils/markdown.ts` 的 renderMarkdown/renderStreamingMarkdown → markdown-it；history-builder 还原历史、format 提供代码复制 | StreamingMarkdownContent 是 stripTagEscapes 唯一生产调用者，整串预处理会二次删除代码反斜杠；直接 scanner 通过不足以证明 DOM/复制通过 |
| editor 正文 | `components/input/input-editor-extensions.ts` createInputEditorExtensions → InputArea useEditor/update/draft hydration → `utils/editor-serializer.ts` serializeEditor → composer bundle → WS → desktop submit → session history | serializer 尾部循环剥空段。StarterKit 实际默认 TrailingNode 会补无标记尾 paragraph，必须在来源位置证明 synthetic，旧无标记草稿保留 |
| ASR | `core/speech-recognition/system-speech-adapter.ts` 异步 spawn → stdout data/onStdout → JSON 事件 → SpeechRecognitionService | Buffer 每块 toString('utf-8')；需真模块子进程或 spawn 边界夹具拆分多字节，保留 close/非零退出/取消/字节上限与并发 |
| TTS | `core/media/universal-media-manager.ts` _submitSpeechWithinTrace/_resolveSpeechTarget → `media-parameters.ts` resolveSpeechParameters → `core/media-adapters/speech.ts` resolveSpeechBody → TaskStore completeSynchronousSpeechTask；启动 recoverSynchronousSpeechTasks | 参数二次归一化与输出 exists 验证须分别反例；网络仅边界替身，任务和真实文件校验不能 mock 掉 |

## canonical entry 与 TipTap 的依赖复核

隔离树 npm ci 后已再次读取其依赖源码，不再仅依赖主检出安装物：

- Pi `node_modules/@earendil-works/pi-coding-agent/dist/core/agent-session.js` 的 `_handleAgentEvent` 先通知 listener，再 `appendMessage(event.message)`。同步 message_end listener 读取 leaf 尚非本条 entry。
- 同包 `session-manager.js` 的 appendMessage 保留传入 message 对象到 entry.message，返回真实 entry.id。允许本项目窄包装层在提交后按同一对象证明 entry，不能按正文、时间、最后 user 猜测。尚未实施该回调。
- `core/session-jsonl-file.ts` 的 schedulePreAssistantSessionManagerFlush 已使用 microtask 等 append 后再刷新；这是现有时序参考，不是关联已实现或磁盘 fsync 保证。
- `node_modules/@tiptap/starter-kit/src/starter-kit.ts` 默认注册 TrailingNode；`@tiptap/extensions/src/trailing-node/trailing-node.ts` 的 appendTransaction 对非 paragraph 尾部插入 type.create()，无 synthetic 标记。本地 StarterKit.configure 未关闭此扩展。

## home 写入所有权：BLOCKED

- `server/index.ts` 现有同宅闸使用 `shared/server-info-probe.cjs` 的 `probeServerInfo/isForeignServerBlocking/describeForeignServerBlock`，读取 server-info.json 并认证探测在跑 server；源码明确保留两个内核同时冷启动的竞态。它不是可取得/释放的原子跨进程 home 写入锁。
- `desktop/src/shared/single-instance-lock.cjs` 的 `configureClientSingleInstance` 调 Electron requestSingleInstanceLock，约束客户端 userData，不覆盖独立 CLI 恢复进程。
- 恢复 CLI 基线直接 apply，没有取得上述 server 所有权；未找到可供迁移/恢复复用的 acquire/release home-lock API。不能把端口探测或 Electron 单例宣传为事务所有权。
- 因此任务书要求的既有单一 home 写入所有权接线仍 **BLOCKED / 未证明**。合成 home 单进程故障测试可以继续，但不证明跨进程互斥。不得另建通用事务平台或默许真实 home 写入。

## E → R → 真实测试入口

下表是测试建设映射；除 COMMAND_RESULTS.json 独立记录的实际运行外，不能由此表推导测试已执行或通过。建议新增测试标题/文件不是已有实现。

| 旧反例 | R | 已有真实测试文件/应补断言 | 本表状态 |
|---|---|---|---|
| E01 | R03 | `desktop/src/react/__tests__/services/composer-send-coordinator.test.ts`；更新 timer deps、编辑立即取消慢准备、前台切页；配 `components/composer-send.transaction.test.ts` | 待阶段真实 RED |
| E02 | R01 | `tests/pinned-tenets-migration.test.ts`；duplicate-only 目标删除必须 conflict | 实际新增与运行见 round2-pinned-migration 和命令台账 |
| E03 | R01 | 同上；新增 ID 存在但正文/active 状态变化不认证、不复活 | 同上 |
| E04 | R01 | 同上及 `tests/pinned-tenets-migration-recovery.test.ts`；归档已完成/收据失败恢复真实磁盘窗口 | 同上 |
| E05 | R05 | `desktop/src/react/__tests__/components/StreamingMarkdownContent.test.tsx`；围栏/inline code DOM textContent 保留反斜杠 | 待阶段真实 RED |
| E06 | R05 | 同上；连续反斜杠与重复渲染/历史/复制；`tests/live-history-reserved-tag-parity.test.ts` 回归 | 待阶段真实 RED |
| E07 | R06 | `desktop/src/react/__tests__/utils/editor-serializer.test.ts`；hello/empty/empty→hello\n\n；真实 TipTap 自动尾段与用户空段分开 | 待阶段真实 RED |
| E08 | R07 | `tests/system-speech-adapter.lifecycle.test.ts`；UTF-8 每字节分块真实 adapter；`tests/x05-speech-concurrency.test.ts` 回归 | 待阶段真实 RED |
| E09 | R09 | `tests/speech-response-lifecycle.test.ts`、`tests/media-task-store.test.ts`；目录/外链/多文件无效真实 FS 与重启 | 待阶段真实 RED |
| E10 | R08 | `tests/speech-parameters.test.ts`、`tests/model-call-payload-speech.test.ts`；actual request/task/observer 参数同源 | 待阶段真实 RED |
| E11 | R02 | `tests/pinned-tenets-migration-recovery.test.ts`、`tests/memory-tenets.test.ts`；pending 在 active 前、重复 apply、批准快照与操作收据 | 待阶段真实 RED |
| 另补 | R04 | 真实 desktop submit → entry 关联 → sessions route → history → coordinator；ACK 前断线、离线完成、分页、跨 origin、新 run 与旧响应竞态 | NOT_EXECUTED，本文件未实施 |
| 另补 | R10 | 本轮证据工具/manifest/补丁 fresh apply；新增文件覆盖、摘要变更拒绝旧结果、全部日志引用哈希 | NOT_EXECUTED，本文件未实施 |

原生授权、实际识别、真实云端 TTS、其他平台运行以及最终全量门禁不在本 S0 子任务执行范围；不能记 PASSED。F7 原前端听写依赖测试需按最终产品范围标 SUPERSEDED_BY_PRODUCT_CHANGE，仍保留后端与原生能力独立验收。
