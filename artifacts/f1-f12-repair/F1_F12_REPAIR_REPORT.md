# F1–F12 修复报告（LingxiAgent）

- 仓库：ItsDalk-Lane/LingxiAgent · 分支：refactor/dismantle-and-voice-features
- 基线（任务书固定树）：`fc4a6df7714a2a26c423be993147e4772b2883ea`（04f90d2b 后封印提交）
- 执行身份：本任务全部改动位于上述基线之上的**未提交工作区**（P0 记录基线工作区为 clean，
  故当前全部 dirty 文件均为本任务产物；补丁见 patches/）。
- 阶段证据：`artifacts/f1-f12-repair/evidence/p0-*.md … p9-*.md`（逐阶段红/绿记录）。
- 结论先行：**F1–F12 全部实现并通过自动化验证（合成环境）；真实环境验收
  （真机授权/真实云端/跨平台）受凭证与设备限制为 BLOCKED。整体状态 =
  局部实现/验证完成，尚不满足完整合并或发布验收**（缺项见各 F 的剩余限制与
  FACTS.json blockers）。

---

## F1 输入持有与显式派发结果（意图 I1）

- 原缺陷：发送链预检失败/传输失败后，未发送成功的完整输入（正文、附件关联、引用、
  技能、知识库范围）没有可靠的可见持有者；清理时机与结果语义含糊。
- 修改：`components/input/composer-send.ts`（prepare/commit 显式结果
  blocked／failed_before_submit／delivery_unknown／transport_submitted + 完整快照）、
  `InputArea.tsx` submitEditorMessage 快照与清理时机、`services/composer-send-coordinator.ts`
  （发送租约/回执对账）、stores 队列结构。
- 实际行为：失败时编辑器、草稿、附件、引用、技能与知识库引用全部原位保留并可按同一
  逻辑记录重试；清理只在身份匹配且提交成功后发生。
- 测试：S01–S15（tests/…/composer-send.transaction.test.ts、composer-send-coordinator.test.ts）。
- 剩余限制：无独立真机项；传输层真实网络抖动场景由 delivery_unknown 语义与回执超时
  单元覆盖（合成 ws）。

## F2 会话级串行与权威回合结束（意图 I2）

- 原缺陷：同会话消息未按入队顺序串行准备/提交；下一条排队消息可能在上一回合权威结束
  前自动发出；断线/旧回执可致双发或误判回合结束。
- 修改：composer-send-coordinator（会话租约、awaiting_ack、回执释放、断线对账）、
  InputArea 自动续发 effect（只在权威空闲时发调度意图）、ws-message-handler/服务端
  session_user_message 回执衔接。
- 实际行为：同会话按序派发；回执/断线/重复回执不双发、不提前判结束；队列编辑=新快照
  （版本递增，迟到结果丢弃）。
- 测试：S 系列 + coordinator 专项用例（X01/X02/X15/X16 组合映射见 p9 证据）。

## F3 旧版合法记忆完整迁移（意图 I3）

- 原缺陷：旧版（父基线）生效的合法置顶记忆在迁移中可能丢失；迁移失败被标完成。
- 修改：`core/pinned-tenets-migration.ts`（来源分类、逐条归档、失败不标完成）、
  `core/pinned-tenets-recovery.ts` + `scripts/pinned-tenets-recovery.mjs`（无损恢复工具）、
  `lib/memory/tenets.ts` / `lib/tools/pinned-memory.ts` 接线。
- 实际行为：旧 markdown 权威内容按规则完整进入新库；失败可恢复重试，不重复导入、
  不复活已删除内容；恢复指南见 PINNED_TENETS_RECOVERY_GUIDE.md。
- 测试：M01–M20（pinned-tenets-migration + recovery 套件）。
- 剩余限制：真实用户数据迁移未执行（任务书禁止迁移真实数据）；恢复工具在合成数据上验证。

## F9 迁移配额与崩溃恢复（意图 I9）

- 原缺陷：active 的 model_proposed 与 user_direct 上限语义未分离（20/200），
  崩溃中断后的收据/状态可能误判。
- 修改：来源分离计数与 20/200 配额（lib/memory/tenets.ts + migration）、收据幂等与
  崩溃注入恢复（receipt:prepared / commit:before / archive:before / completed:before）。
- 测试：M10/M11/M14 崩溃恢复 + tenets-source-quota.test.ts（配额互不占用）。
- 已知非缺陷：满载全量跑时 M10 个别子用例偶发失败（单跑 25/25 通过，未触碰该模块的
  轮次同样出现）——按 P9.3 规则记录为疑似既有 flaky，最终全量以全绿运行计。

## F4 宿主 Speech 授权状态机（意图 I4）

- 原缺陷：没有真实宿主内的 Speech 授权请求链；同步/裸进程方式无法弹授权或会崩。
- 修改：`desktop/speech-permissions.cjs`（状态机工厂+单例：not_determined/authorized/
  denied/restricted/unsupported/bridge_unavailable、单飞、dispose 语义）、
  `desktop/native/LingxiSpeechPermissions/`（ObjC++ Node-API 桥，TSFN 一次性回调）、
  main.cjs/preload.cjs/types.ts IPC（speech-permission-status/-request）、
  `scripts/build-speech-permissions.mjs` + CI + ad-hoc 重签名接线。
- 实际行为：桥在真实 Electron 运行时加载成功（exports 可枚举、状态可读）；未授权时
  helper 显式 PERMISSION_REQUIRED fail-closed（合成 WAV 冒烟）。
- 测试：A01–A16（speech-permission-bridge + lifecycle 套件）+ P9 Electron 加载补验。
- 剩余限制（BLOCKED）：真实 TCC 首次授权/拒绝/恢复流转需真机授权环境。

## F5 Swift 识别核心异步重写（意图 I5）

- 原缺陷：识别回调依赖默认主队列/信号量阻塞，成功/错误/超时/取消可能不结算或重复结算。
- 修改：`desktop/native/LingxiSpeechHelper/`（RecognitionCoordinator 单次结算状态机：
  created→running→succeeded/failed/timedOut/cancelled，task.cancelRecognition 永不
  finishRecognition；CheckedContinuation + TaskGroup 超时竞争；独立 OperationQueue；
  协议 2 JSON 输出；始终要求设备内识别，不支持时显式 recognizerUnavailable——F7/P5.4）。
- 测试：A 系列 + Swift runner 16/16（Package 无 XCTest target，CLT 工具链限制，
  以可执行 runner 为准；swift test 如实 exit 1）。

## F6 Node 适配器异步化（意图 I6）

- 原缺陷：helper 调用同步阻塞事件循环；取消/超时/终止无界；错误表达丢失。
- 修改：`core/speech-recognition/system-speech-adapter.ts`（异步 spawn、SIGTERM→宽限→
  SIGKILL 有界终止、'close' 定界、4MB stdout 上限、AbortSignal 贯穿 REST→service→adapter、
  SYSTEM_SPEECH_* 九类错误码、协议 2 单行 JSON 解析）。
- 测试：A06–A12 生命周期（假 helper：超时/取消/大输出/迟到一个不结算）+ X05 并发不阻塞。

## F7 文本模型听写与完整前端接线（意图 I7）

- 原缺陷：听写 UI 是死代码（无按钮、死 toggle、文本模型被门禁与自动丢弃效应拒绝）；
  转写解包 `String(object)` 产生 `[object Object]`；零配置 system-speech 被生效模型规则阻断。
- 修改：InputArea 能力选择器（canSendNativeAudio / canUseSystemDictation=宿主桥+后端探测）、
  ComposerToolbar 真实语音/听写按钮、七态生命周期、modeAtStart 快照、修正自动丢弃效应、
  按钮与快捷键同一门禁、类型化解包+错误码区分文案、保真插入、迟到结果守卫（会话/连接代次/
  取消）、服务端 authType=none 目录直通 + 错误码前缀透出、locale 五语言文案。
- 测试：D01–D12（InputArea.system-dictation.test.tsx 19 用例）+ 服务端 3 用例 + X04 组合。
- 剩余限制（BLOCKED）：真机「首次授权→文本模型录音→文字插入→发送」全流程。

## F8 通用形状规则与正常标记保护（意图 I8）

- 原缺陷：词表外孤儿闭标签形状规则没有未知开启栈，正常配对标记（</details>、</summary>、
  带属性标签的闭标签）被当残渣误删；转义字面量在链头消费后于链尾被删；CDATA 内标签被
  解释；历史净化器在无 leading 块时 return 原文抵消清理。
- 修改：shared/reserved-tag-stream.ts 三概念分离（协议块/未知标记栈/转义与代码保护；
  引号属性内 > 不截断；注释/CDATA/自闭合不入栈；深度与挂起缓冲有界保守透传）、转义反斜杠
  链上保留 + 显示层 stripTagEscapes 一次性消费（StreamingMarkdownContent，live/历史同口）、
  events.ts 契约注释、history-segment-sanitizer T16 修复。
- 测试：T01–T16（reserved-tag-text-preservation，完整 Think→Mood 链 + 实时/历史双路径）
  + orphan-closer 套件更新；5 个旧转义断言按新契约升级（非弱化）。
- 声明：不能凭形状可靠识别所有未知成对思考内容——未知成对标记一律保守保留（不隐藏正文）。

## F10 粘贴到发送文本保真（意图 I10）

- 原缺陷：serializeEditor 对最终输出 trim 并丢空段；粘贴插入整段 paragraph 致选区替换/
  段中粘贴多生换行；历史用户正文解析存在边界 trim。
- 修改：editor-serializer（去 trim、空段=可见空行、徽章段例外、文档尾 schema 垫尾剥离、
  列表续行缩进、insertFaithfulPasteAtSelection 行内保真插入原语）、InputArea 粘贴接线、
  message-parser（正文边界 trim 移除 + 协议块分隔空行摘除）。
- 测试：P01–P12（serializer + 真实 TipTap editor + composer-send.transaction P07/P08/P10）；
  S13 tripwire 断言按预留注释升级。
- 取舍：文档尾真空段（TipTap 垫尾）剥离；粘贴保真走段内 hardBreak 模型不受影响。

## F11 TTS 参数域隔离（意图 I11）

- 原缺陷：语音适配器读取图片域 ctx.config 的 providerDefaults，且按 credentialProviderId
  索引（与设置页保存键错位）；默认参数可改写模型；Number(null)=0 吞掉缺失。
- 修改：media-parameters.resolveSpeechParameters（显式>语音默认>协议默认；null/非法/零值
  显式语义；逻辑 provider 索引）、manager 注入有效参数并同源记录 TaskStore/语义观测、
  speech.ts 三条云端路径删除全部 config 读取与模型回退。
- 测试：V01–V06/V03b（网络 stub，断言 URL/body/凭证来源）。

## F12 同步 TTS 任务终态（意图 I12）

- 原缺陷：response 投递的同步语音任务提交后永远 pending；文件缺失仍 ok=true；无重启恢复。
- 修改：task-store.completeSynchronousSpeechTask（文件非空+目录内+存在校验、done/completed/
  completedAt、幂等、终态不倒退）+ recoverSynchronousSpeechTasks（仅 speech+response+pending；
  文件在→done 缺→failed；不重合成不通知不新增用量）、manager response 收尾与 start 恢复钩子。
- 测试：V07–V13（含持久化 flushSync→重启读回）。

---

## 退出检查（P10.4 逐条）

- F1–F12 均有状态/证据/测试 ID：✓（见 FACTS.json）。
- 缺真机/凭证未写为通过：✓（BLOCKED 项单列；真实环境验证一栏如实 NOT_RUN/BLOCKED）。
- 未恢复废弃功能：✓（邀请码/插件市场/社区插件/研究模式无复活路径；P1 证据）。
- 未破坏既有工具与 knowledge 范围：✓（X11–X13 全量绿；门禁 closure/tripwire/seal 绿）。
- 输入、迁移、识别、任务均有失败终态：✓（S/V/T/D 各失败路径显式断言）。
- 新增原生桥与收据有构建/持久化登记：✓（CI 工作流步骤、resign-adhoc 1c 块、pack 干净
  构建；迁移收据幂等由 M 系列覆盖；export-manifest 未扩大）。
- 日志无敏感内容：✓（logs/ 均为脱敏合成输出；一次性签名密钥仅存 /tmp 未入库）。
- Git/数据操作在授权内：✓（零 commit/push/PR；真实用户数据未触碰；VERIFIED_SOURCE_SHA 未推进）。
