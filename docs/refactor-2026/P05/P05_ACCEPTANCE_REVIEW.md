# P05 独立验收报告（验收子代理，全新上下文）

日期：2026-09-22｜验收对象：P05 工作区交付（HEAD `1f0537b0865c1f6a4a17c28eb66c6d9ebf4948a6` + 未提交改动）｜角色：只读独立验收，不修改生产代码/测试/文档（本报告与证据日志为模板允许的唯一写入）。全部结论基于本验收独立复跑、源码逐点核对与原始日志审阅，未采信执行者自报。

## 结论

**阶段总状态：PASS（本地验证口径，与执行者判定一致且成立）**。

- 8/8 任务、A01–A16 全部 16 个验收场景的本地证据由本验收独立核实：改动面声称、UNCHANGED_VERIFIED 关键定性、数据零变更、新增测试真实入口、benchmark 数字、全量 F1 基线未扩大、typecheck/门禁/指纹——均复现或逐点对上源码。
- 反例 3 组真实执行：C2（ring 边界精确性/字节上限/大事件压缩可观察）3/3 绿；C3（v2 实时引用磁盘陷阱/会话不匹配/幽灵 callId）绿；**C1（跨流迟到事件）复现出消费端投影污染**——经源码论证当前服务端接线不可达（详见 §5），定性为非阻塞加固项，不构成 P05 场景 FAIL，登记给后续阶段。
- 3 个非阻塞发现（见 §6）：FI 测试文件含机器绝对路径（提交后进 CI 会红）、ACCEPTANCE_MAP A14 一处测试名文件归属错位、f1 patch 全量测试重写副作用在本验收复现并已还原。
- BLOCKED 项核实：仅 P04-T07-2 真供应商冒烟（P04 继承，无凭证/预算授权，非 P05 必需场景），登记属实。

## 1. 改动面与「零生产代码改动」声称

- `git status` / `git diff HEAD --stat`：3 个既有测试文件 **+282 行纯新增、0 删除**（tests/chat-route-switching.test.ts +197、desktop/src/react/__tests__/services/stream-resume.test.ts +41、tests/tool-presentation-history.test.ts +44）+ 2 个未跟踪目录（docs/ artifacts/refactor-2026/P05/）。无任何生产源码文件在 diff 中；纯新增意味着**无断言改弱、无既有用例删除**（diff 逐 hunk 审阅确认）。
- 与 P05_RESULT.json「implementation_note」一致：implementation_performed=true 指测试与文档交付，非生产改动。
- 审计封印面：diff 未触碰 `.sync-audit/`、未触碰 pinned-keyset、未改历史 PASS 证明。

## 2. UNCHANGED_VERIFIED 关键声称抽查（源码级，非采信文档）

| 声称 | 核实结果 |
|---|---|
| canonicalEvents/visibleTextDeltas 批输出唯一消费点 = chat.ts publishNormalizedAssistantBatch | **属实**。全仓 grep：两者仅出现于 server/assistant-event-normalizer.ts（生产）与 server/routes/chat.ts:1563-1577（唯一批消费），diagnostics 同点转 warn 日志。无第二个读取模块。 |
| 无第三套正文解析器 | **属实**。正文语义裁决链=实时 AssistantEventNormalizer + 历史 extractPersistedAssistantSemanticSegments（project-page.ts:372 / history-deferred-content.ts:291 消费）；ThinkTag/MoodParser（core/events.ts，共享 ReservedTagScanner 内核）是保留协议边界。桌面 assistant-block-builder.ts 的 splitThinkResidue 是旧落盘渲染兜底（不判 phase），与 MESSAGE_SEMANTICS §4 清点一致。 |
| legacy text_delta 消费者定性（ChannelsPanel/SubagentSessionPreview 精简视图 + 主聊天回退 + 远程客户端） | **属实**。ChannelsPanel.tsx:540、SubagentSessionPreview.tsx:242 仅消费 text_delta；use-stream-buffer.ts:716-720 canonical 锁定后 legacy 不再驱动正文（canonicalLocked 机制核实）；ws-protocol.ts:24 明示兼容期并行发送。 |
| 旧路径未擅自退役 | **属实**。text_delta 发射链（emitVisibleTextDelta）、history-read legacy 全量回退（fallbackReason 链）、knowledge research 表族删除保护读取（knowledge-store.ts:3688-3701）全部在源码可达。 |
| 数据版本表与源码一致 | **属实**。observability v7（model-observability-schema.ts:30）、knowledge v19（knowledge-store.ts:56）、file-history v1（history-store.ts:20）、session-manifest user_version（store.ts:12）逐一对上；指纹守卫独立复跑 "170 watched; OK"。 |
| P05-2 knowledge 残留决策 | **属实**。P00 SCOPE.json 将 knowledge research pipeline 列 HARD_EXCLUDED（production_residue 注明建表与删除保护）；REFACTOR_BACKLOG「需用户决策」第 2 条原文=「knowledge research 兼容残留的最终去向（保留只读兼容 vs 授权退役）」。P05 保留只读兼容未删未改，与治理要求一致。 |
| P05-3 双身份契约 | SESSION_IDENTITY_CONTRACT 的换算点（ws-scope resolveWsSessionContext）、防误用测试（ws-session-context 13 例等）均在源码/测试集中；全量复跑绿。 |

## 3. 新增测试真实入口核验（非 mock 自证）

- chat-route-switching `P05 stream resume semantics` 4 例：经真实 `createChatRoute` 的 onMessage(resume_stream) → requireWsSessionContext（真实身份链）→ 生产 resumeSessionStream；session-stream-store **未 mock**。mock 边界仅 engine（外部 agent 边界，任务书允许替身）与 hub.send/WS sink（观测点）。独立复跑 4/4 绿。
- stream-resume.test.ts 跨流例：真实 replayStreamResume/dispatchReplayEvent 链（injectHandlers 仅作观测）。复跑绿。
- tool-presentation-history A11：真实磁盘文件陷阱（tmpdir 写-改-删）× 真实 resolveHistoryDeferredContent。复跑绿。
- FI-1/FI-2：vi.mock 仅篡改历史侧段重导出/页边界两处，live 链与 projector 真实；核实在全量套件中真实执行并绿（P05-T08-full-suite.out:4164）。

## 4. benchmark 与全量独立复跑

- benchmark phase B 独立复跑（隔离输出 /tmp/p05-accept-bench，seed 20260910）：n=1000 与 n=10000 热页 **fullFileReadCalls/请求=0、jsonlParseCount/请求=103**，阈值断言全过；输出文件清单与执行者样本目录一致。声称数字完全复现。
- 全量 `npm test`（86.9s）：**14769 绿 / 4 红 / 15 跳过，1456 文件（3 failed | 1450 passed | 3 skipped）**——与执行者 P05-T08-full-suite 完全同数。4 红 = post-verification-audit-seal + round2 R10-03/R10-04 + round3 manifest，与 F1 基线登记同组、未扩大（P04 为 14759 绿/4 红，+10 绿 = 本阶段新增 6 例 + FI 2 例 + 计数漂移，方向一致）。
- typecheck×3 exit 0；typecheck:core-contracts exit 0；指纹守卫 exit 0。
- EVIDENCE_SHA256.txt 73 条逐一重算：全部匹配（manifest 不含自身，无自引用）。
- command-log.jsonl 27 条：command_id 唯一、argv/cwd/SHA/时间/exit_code 齐全，5 组 FAIL→retry 链完整留档（A05 断言口径、消费端 async、FI 三迭代、gates 两次脚本名试错），无「未运行冒充通过」。

## 5. 反例执行（3 组，隔离临时根 /tmp/p05x-accept，已清理；证据 P05-ACCEPTANCE-counterexample.*）

- **C1（跨流迟到事件，RED——登记为非阻塞加固项）**：向真实 streamBufferManager 喂 Run A（streamA/segA 正文）→ run_end → Run B（streamB/segB 正文）→ **迟到旧流 delta（streamId=streamA, seq=9, segmentId=segA）**：该尾巴被作为第二个 answer 块拼进 Run B 的消息与 turnProjection（answerBlockIds 含 segA）。源码定性：`updateCanonicalSegment`（use-stream-buffer.ts:413-460）按 segmentId+seq 防重，**无 streamId 身份闸门**；ws-message-handler 的 updateSessionStreamMeta 遇 streamId 翻转会清空 consumedSeqs（合法换流语义），迟到事件可直达 buffer。**可达性论证：当前服务端不可产生此输入**——emitStreamEvent（chat.ts:776-786）在发送时同步盖「当前」streamId/seq，同会话 Run 串行，resume 重放只含当前流（A04 服务端语义正确且有测试）。故定性=纵深防御缺口（消费端假设服务端不越轨），非 P05-A04 场景失败。最小修复建议（后续 hardening）：updateCanonicalSegment 或 buffer 层对 canonical 段事件校验 streamId 与 activeRunKey 归属，错配丢弃+诊断。涉及场景：A03/A04 消费端面；源码位置：desktop/src/react/hooks/use-stream-buffer.ts:413-460、desktop/src/react/services/stream-resume.ts:160-183。
- **C2（ring 边界/字节/压缩，3/3 GREEN）**：sinceSeq=firstSeq-1 → truncated=false、firstSeq-2 → truncated=true 且补发自 firstSeq（边界精确）；maxBytes 超限触发 trim 且 resume 显式 truncated、nextSeq 不失真；单事件超 256KiB → compacted:true + originalByteLength，压缩占位不含原正文（不把压缩结果当原始完整）。真实 store 生产实现直接驱动。
- **C3（v2 实时引用，GREEN）**：kind=tool_output 的 v2 locator 展开读持久 toolResult 记录（磁盘同名文件内容分叉时记录为准）；locator 会话 ≠ 授权会话 → null（引用非授权凭证）；幽灵 toolCallId → null（不回退读磁盘）。首版反例曾误用非法 kind=tool_file_content（LiveToolContentKind 仅 tool_output|tool_search）与错误 details 字段，修正后通过——该曲折不涉及生产缺陷。

## 6. 非阻塞发现（需编排层知悉，均不构成本阶段 FAIL）

1. **FI 测试文件可移植性**：artifacts/refactor-2026/P05/logs/P05-FAULT-INJECTION.test.ts 的 vi.mock/import 使用本机绝对路径（/Users/study_superior/…）。该文件匹配 vitest 默认 include（npm test 的 --exclude 列表不含 artifacts/），本机全量已执行且绿；但**提交进仓库后 CI/其他机器将无法解析绝对路径而红**。最小修复：改为相对导入（文件已在仓库根下）或迁入 tests/ 并用相对路径，同时更新其头部「不进入默认测试集」的过时注释（与 NEXT_STAGE_HANDOFF §3「已并入默认 vitest 集」矛盾）。属 P05 交付物自身缺陷，建议在编排层提交前处理。
2. **ACCEPTANCE_MAP A14 归属错位**：「requires a checkpoint receipt after the prepared phase」实际位于 tests/data-epoch.test.ts:109，map 记在 tests/data-epoch-coordinator.test.ts 名下。测试存在且绿（40/40 复跑），证据真实，仅文件归属字段错。
3. **f1 patch 重写副作用复现**：本验收全量运行后 89bc0b64-to-r01-r10-source.patch 被测试重写（+127140/-17715，与执行者 stat.txt 同象），验收侧已 `git checkout` 还原。执行者交付终态该文件与 HEAD 一致（本验收起点 git status 佐证），但其 P05_REPORT 未明说全量运行曾重写后还原（HANDOFF §8 有提示、stat.txt 有证据）。提醒编排层：**任何 `npm test` 全量后提交前须再次检查/还原该文件**。

## 7. 边界与未验证项

- 未验证（如实登记）：四平台 CI / open server 冒烟（F3，P08 范畴）；真供应商冒烟（P04-T07-2 继承 BLOCKED，无凭证/预算授权）；正式打包产物。本地 darwin arm64 单平台结论不外推。
- C1 反例仅证明消费端投影层行为与当前服务端不可达性论证；未穷举所有假设性服务端缺陷输入（不承诺绝对无缺陷）。
- 本验收写入的文件：docs/refactor-2026/P05/P05_ACCEPTANCE_REVIEW.md + artifacts/refactor-2026/P05/logs/P05-ACCEPTANCE-{counterexample.test.ts,counterexample.log,rerun.log,full-suite.log,typecheck.log}。执行者 EVIDENCE_SHA256.txt 未重生成（属编排层提交前动作，按 HANDOFF §8 任何日志追加后须重新生成清单）。
