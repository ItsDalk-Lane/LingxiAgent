# 修复报告：长单轮会话的历史分页、重复投影与「未生成最终回复」误报（F1–F4）

- 基线：`e0afc0dc3b94bc9763d715f981d6f948d1c39c27`（v0.1.37），与任务书审查基线一致，无需改判。
- 工作区另有他人未提交的模型观测/收据 v4 契约补强改动（core/、lib/llm/、tests/model-observability-* 等），本任务未触碰、未清理，保持原样。
- 环境：macOS darwin 25.6.0 arm64 / Node v24.16.0 / vitest。
- 本目录：`evidence/fix-verification.json` 为同夹具前后对比与 T18 规模数据。

## 一、结论总览

| 发现 | 复核结论 | 状态 |
|---|---|---|
| F1 归并显示 ID 被用作分页游标 | 属实，源码逐点确认（`history-builder.ts` 组末条 id → `chat-slice.ts oldestId` → `loadMoreMessages before`；服务端 `id=String(displayIdx)`、`before` 为 display 序号边界） | 已修复并验证 |
| F2 页片段被当成完整 Run，跨页重复产生无回复状态 | 属实（页内归并以 `completed` 进投影，`turn-projector` 局部判空生成 `missing_final_answer`；隐藏输入轮完全无归并时逐条误报更甚） | 已修复并验证 |
| F3 结局裁决未统一 | 属实（projector 判「块存在性」，resolver 判「内容有效性」，两套规则不等价） | 已修复并验证 |
| F4 性能 | F1/F2 的重复读取/解码/投影为实测放大（92 页/4600 条/238s → 3 页/141 条/7s）；自查报告的「无会话缓存/必挂载全部折叠/JSONL 无语义」三条前提经源码复核均不成立，未采纳 | 已消除证实重复，剩余瓶颈未新增优化 |

## 二、根因与修改（逐项）

### F1 游标（谁拥有分页边界）

**根因**：服务端 `id = String(displayIdx)` 是原始记录身份；前端把「归并组第一条显示项的 id」（= 组内**最后**一条记录的序号）当 `before`，导致每页只推进 1 条、其余 49 条重叠，且 `prependItems` 裸拼接不去重。

**修改**：
- `server/routes/sessions.ts`
  - `resolveHistoryPageBounds`：`before>=0` 视为合法边界（`before=0` → 空页 + `hasMore=false`，不再回退最新页）；负数/NaN 才视为未指定。
  - 响应新增 `nextBefore`（`hasMore ? String(startIdx) : null`）——游标由服务端原始页面范围显式下发。
- `desktop/src/react/stores/session-actions.ts`
  - `historyNextCursor()`：优先 `nextBefore`；旧服务端回退**原始首条记录 id**（不是显示项 id）；两者都缺 → `undefined`（保持既有游标）。
  - `loadMoreMessages` 消费 `session.nextBefore ?? session.oldestId`；`hasMore` 但拿不到可推进游标时显式 `console.error` 并终止翻页（可诊断、不死循环、不静默截断）。
- `chat-slice.ts`：`initSession`/`prependItems` 增加游标参数并落盘 `SessionMessages.nextBefore`；`oldestId` 退化为兼容字段（`locate-step.ts` 仍按数值 display 序号消费，语义反而更准）。

### F2 Run 连续性（谁持有整轮结局）

**根因**：Run 跨页时，每个不含 Run 尾部的页片段都以 `completed` 进投影；片段只有过程块 → 各自生成一个 `missing_final_answer`。隐藏输入轮（loop 等 `turnInputEntryId=null`）更退化成逐条记录各自投影。中间失败记录还会把整组放大为 failed。

**修改**：
- 服务端新增 **Run 边界预扫描**：与主循环 `latestTurnInputEntryId` 指针同源（输入事件：user 消息 / custom turn input / loop kickoff 开启新 Run），对每个 displayable assistant 记录下发 `turnStartIndex`/`turnEndIndex`（display 序号）。display 计数与主循环逐字对齐（含被前端过滤的隐藏 user 消息占序）。
- `history-builder.ts`
  - 归组优先按 Run 边界（`turnStartIndex/turnEndIndex` 相等且相邻）——隐藏输入轮也能正确按 Run 归并；旧服务端回退原 `turnInputEntryId` 规则。
  - **组末条记录持有终态**：Run 的 turnStatus 取最后一条记录（中间失败后恢复成功不再放大为失败；T10a）。
  - 片段（不含 Run 尾部，或头部被分页截断）携带 `runFacts`（原始记录事实：displayId/entryId/segments/thinking/toolCalls/turnStatus/inlineBlocks + Run 元数据），`runTerminal=false` 时**不派生任何终态块与结局**。
  - 流活跃时（`loadMessages` 检测本端 stream buffer 有内容 → `openTailRun`）：最新 Run 组在磁盘上的记录必然不完整，不派生终态（T06）；Run 真正结束后由实时收口 `commitLiveRun` 按权威状态落定。
  - 整组无 segments 的旧数据保持 legacy 投影路径（数组位置即展示顺序、不产终态块），兼容旧会话的技能卡顺序与空壳行为。
- `history-run-merge.ts`（新）：`mergePrependedHistoryItems` ——
  - **缝合**：incoming 末尾片段与既有首个同 `runKey` 的 Run 项合并 facts（按 displayId 并集）→ 重投影 → 原位替换（显示项 id = Run 尾部记录 id，React key 稳定）；Run 头部到达（最小 displayId == turnStartIndex）即丢弃 facts（内存有界）。
  - **幂等**：重复页按显示身份（消息 id / interlude id）与记录 displayId 去重，不增加消息/工具/告警数量；不同 Run、不同身份不混入。
  - 未受影响的项保持对象引用不变（增量渲染）。

### F3 结局唯一裁决

**修改**（`turn-projector.ts`）：
- 删除 `answerBlocks.length===0 && resultBlocks.length===0 && controlBlocks.length===0` 的局部判空；分区完成后统一调用 `resolveAssistantTurnOutcome` 决定是否生成 `turn_status`（含 `empty_final_answer`/`only_process_blocks` 原因），并把 `outcome`/`missingFinalAnswerReason` 写进 projection。
- 输入中残留的派生 `turn_status`（旧投影产物）一律剔除、由本次投影按稳定 id 重新生成——派生状态永不作为「已有结果」的证据（T17）。
- `runTerminal=false` 时不裁决（未知 ≠ 没有输出）。实时路径不传该字段，行为不变（live-history parity 149 项测试全绿佐证）。

### F4 性能

- 先消除证实重复：见 evidence（-96.9% 记录传输、-97.0% 耗时，同夹具）。
- 复核并**否定**自查报告三条前提：会话缓存存在（`MAX_CACHED_SESSIONS=8` + 修订点校验）；`Collapse` 关闭不挂载子内容 + 服务端已有延迟读取；JSONL 已有 `stopReason`/语义段。未新增平行缓存系统。
- 1000 过程单元（T18）：21 页、1001 唯一记录、无失控；单页耗时主要来自服务端每页全文件解析（既有读取模型），未在本次扩大范围优化。

## 三、生产接线清单

| 层 | 文件 | 接线 |
|---|---|---|
| 服务端 | `server/routes/sessions.ts` | `before>=0` 边界、Run 边界预扫描、每条 assistant 消息携带 `turnStartIndex/turnEndIndex`、响应携带 `nextBefore` |
| 类型 | `chat-types.ts` | `HistoryRunRecordFact`/`HistoryRunFacts`/`ChatMessage.runFacts`/`SessionMessages.nextBefore` |
| 投影 | `history-builder.ts` | Run 边界归组、`projectHistoryRunRecords` 共用核心（页内归并与跨页缝合同一坐标系）、`projectHistoryRunFromFacts` 缝合重投影、`openTailRun` |
| 投影 | `turn-projector.ts` | 结局唯一裁决、派生 turn_status 过滤、`runTerminal` |
| 合并 | `history-run-merge.ts`（新） | 同 Run 缝合 + 幂等去重 |
| 状态 | `chat-slice.ts` | `initSession`/`prependItems` 游标参数 + 缝合接线 + `history_run_stitch` 性能事件 |
| 动作 | `session-actions.ts` | `loadMessages`（空页不截断 hasMore、openTailRun、游标落盘）、`loadMoreMessages`（nextBefore 消费、空页推进、游标缺失可诊断） |

## 四、验收矩阵覆盖

新增测试（全部调用真实生产代码）：
- `tests/history-pagination-run-continuity.test.ts`：T01（3 页/141 唯一记录/1 逻辑 Run/0 伪无回复/与 all=1 逐块等价/facts 头部到达即丢弃）、T03（limit=7 任意切页等价）、T04（重复页幂等）、T11（before=0 空页终结 + nextBefore 契约）、T11b（整页被过滤仍推进、更早历史可达）。
- `tests/history-run-outcome-edges.test.ts`：T06（openTailRun 不误终结/流结束恢复终态）、T07（真无输出恰一个提示、不因工具豁免）、T10a/T10b（中间失败恢复/真失败带部分正文）、T12（相邻 Run 不误合并）、T14（文件 blocks 跨页归属、无重复）。
- `desktop/src/react/__tests__/chat-semantics/turn-outcome-unification.test.ts`：F3 五态（空白答案/pending 控制卡/已确认卡/only-process/派生状态不作证据/失败中止同步）。
- 既有严格形状断言随新契约更新：`tests/sessions-route.test.ts`（响应携带 run 边界）、`history-builder.test.ts`（projection.outcome）——均为字段追加，未削弱断言。

阶段 A 修复前失败实录（同夹具）：T01/T04 直接 60s 超时（92 页真实请求放大）、T11 返回最新页、T11b 历史被静默截断、T03 游标链断裂、F3 单测 5 红。

## 五、验证记录

| 项 | 命令 | 结果 |
|---|---|---|
| 定向 | `npx vitest run tests/history-pagination-run-continuity.test.ts tests/history-run-outcome-edges.test.ts desktop/src/react/__tests__/chat-semantics/` | 22/22 绿 |
| 相关回归 | live-history parity、reserved-tag、turn-outcome-patterns、chat-turn-lifecycle、ws-message-handler（85）、use-stream-buffer（34）、process-fold、block-renderers、AssistantMessage.interlude、session-find-route、sessions-route（106）、history-builder（28） | 全绿 |
| 类型 | `npm run typecheck`（root + node + test 三份 tsconfig） | 通过 |
| 指纹门禁 | `node scripts/check-persistence-schema-fingerprint.mjs` | 通过（本任务文件不在受守护集合；他人未提交的 repin 原样保留） |
| 全量 | `npx vitest run` | 1344 文件通过；4 失败均为**基线预存红**（见下），非本任务引入 |

基线预存红（与本任务无关，均有独立证据）：
1–3. `tests/round2-delivery-evidence.test.ts` R10-03/R10-04、`tests/round3-delivery-evidence.test.ts` 源码 manifest——失败 diff 为知识库测试文件等非本任务文件的工作区漂移（他人未提交改动所致，与本记忆中「全量仅 4 个基线预存红」一致）。
4. `tests/packaged-desktop-cleanup.test.ts`「持续未就绪仍在原有 90 秒期限失败」——VM 切片锚点随 v0.1.37 打包冒烟加固（acde46f1，已提交）漂移，单独重跑稳定复现，与聊天链路无文件交集。

## 六、兼容、剩余风险与未验证项

- **兼容**：旧服务端（无 `nextBefore`/run 边界）→ 游标回退原始首条记录 id（正确值）、归组回退 `turnInputEntryId` 规则、无 segments 组保持 legacy 投影；`initSession` 新参数可选，QuickChat/mobile/ws-message-handler 的三参调用不受影响。
- **已知行为变化（有意）**：带 segments 的历史 Run 现在由统一裁决产生结局（空白答案/纯过程会显示恰一个「未生成最终回复」，与实时路径一致）；跨页 Run 在补页瞬间与已有项缝合，显示项 id 全程稳定。
- **剩余风险/未验证**：
  - 运行中冷加载若恰好跨模型轮落盘窗口，历史侧最新 Run 组（无终态）与实时 inflight 项可能短暂并存部分过程内容——这是既有实时/历史交接形态，本次仅保证不误终结（T06），未重构该交接。
  - Electron 桌面真机切回/滚动锚定/复制交互未实测（无桌面环境），已用真实路由+真实 store+真实投影链覆盖数据层。
  - Windows/Linux 平台行为未单独验证（改动均为平台无关纯逻辑）。

## 七、文件清单

修改：`server/routes/sessions.ts`、`desktop/src/react/utils/history-builder.ts`、`desktop/src/react/utils/turn-projector.ts`、`desktop/src/react/utils/chat-performance.ts`、`desktop/src/react/stores/chat-slice.ts`、`desktop/src/react/stores/chat-types.ts`、`desktop/src/react/stores/session-actions.ts`、`desktop/src/react/utils/history-run-merge.ts`（新增）、`tests/sessions-route.test.ts`、`desktop/src/react/__tests__/utils/history-builder.test.ts`
新增测试：`tests/history-pagination-run-continuity.test.ts`、`tests/history-run-outcome-edges.test.ts`、`desktop/src/react/__tests__/chat-semantics/turn-outcome-unification.test.ts`
