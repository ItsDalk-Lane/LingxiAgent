# LIVE_HISTORY_REPLAY_REPORT — 真实路由到真实 store 的等价测试（P05-T07）

日期：2026-09-22｜基线 HEAD：`1f0537b08`。

## 1. 等价测试矩阵（S16 模式复用 + 本阶段扩展）

任务书 T07-1/2 要求：真实 Hono 路由 / chat 路由 + history builder + chat store，
加入真实 WS 事件驱动的 live 结果，分别在首屏/翻页/断线恢复/重启后采样，比较
正文/推理/MOOD/工具/文件/状态与身份归属。现状映射：

| 采样面 | 锁定测试（本轮全绿） | 比较内容 |
|---|---|---|
| 首屏（live vs 历史重进） | tests/live-history-reserved-tag-parity.test.ts 场景一/二：真实 createChatRoute 事件链 → WS payloads → 真实 streamBufferManager 实时投影；同语义持久数据 → 真实 buildItemsFromHistory | mood 内容序列、answer 文本、工具名、turn status、块序列、保留标签零泄漏 |
| 正文/推理/合法标签 | tests/reserved-tag-text-preservation.test.ts T13（同源 raw 两路一致）、T14/T15/T16；assistant-event-normalizer.test 9 例 | 正文逐字、转义字面量、用户输入不动 |
| 工具/文件/大结果详情 | tests/tool-presentation-history.test.ts（含本阶段新增 A11 例） | 实时终态 vs 历史短详情逐字段；大结果首包预览 vs 展开全文 |
| 翻页 | tests/history-pagination-run-continuity.test.ts T01/T03/T04（1k/10k 真实夹具、真实路由、真实 manifest store、真实前端 store 驱动） | 分页串联 vs 全量恢复同序同集合、不重叠不缺失 |
| 断线恢复 | 本阶段新增：chat-route-switching `P05 stream resume semantics` A03–A06（真实路由 × 真实 stream store）+ stream-resume.test（含新增跨流 seq 例）+ stream-resume-gate 5 例 + use-stream-buffer-dedupe | 补发不重复、跨流不污染、截断可见、流结束走历史 |
| 重启后 | history-run-outcome-edges T06（openTailRun → 流结束恢复终态）、data-epoch 系列（A14/A15 机制） | 终态恰好一次、不早终结、数据不半迁移 |
| 导出 | conversation-export、export-open-tree、model-observability-export | 导出范围/显式失败/观测导出字段 |

外部 provider 替身：live 路径的"供应商"是测试注入的 assistantMessageEvent 序列
（Pi adapter 下游边界），模型本身不参与——符合任务书"外部 provider 可替身"。

## 2. 负向证明（T07-4：让测试真的会挡错）

`artifacts/refactor-2026/P05/logs/P05-FAULT-INJECTION.test.ts`（2 例，本轮多次迭代后
全绿；首次失败与修正过程全部留档 command-log P05-T07-fault-injection 至 -r4）：

- **FI-1（commentary 被错当 final）**：vi.mock 篡改
  `shared/assistant-semantic-segments.ts extractPersistedAssistantSemanticSegments`
  （历史侧段重导出一律输出 final_answer），live 侧走真实链。结果：历史投影把
  process 段升为 answer，语义 profile 与 live 不再相等——正是
  live-history-reserved-tag-parity 的 `toEqual(liveProfile)` 断言点，该故障必然红。
  同时验证基线：未篡改 live 真值为 process/answer 两段。
- **FI-2（分页游标使用展示/源下标）**：vi.mock 篡改
  `server/history-read/page.ts resolveHistoryPageBounds`（beforeId 解释为源数组下标、
  忽略 displayable 过滤）。在含 5 条隐藏记录的 30 记录夹具上：最新页欠填
  （<5 条，正确语义必须满页）、总页数偏离 display 页数学（6≠5）——正是
  history-pagination-run-continuity T01/T12 的断言口径，该故障必然红。

注：P04 反例曾用 /tmp 隔离根执行；本阶段 FI 文件用 vi.mock 隔离（vitest 文件级
worker 隔离，同套件其他文件不受影响——P05-T07-parity-and-fi 将 FI 文件与 parity
套件同进程运行验证互不污染，全绿）。

## 3. 旧客户端兼容 / 越权 / 截断 / 磁盘失败 / 索引损坏（T07-3）

- 旧客户端兼容：history-protocol-compat / history-protocol-conditional（T04 套件）+
  ws text_delta 兼容链（CONSUMER_MIGRATION §3）。
- 资源越权：resource-access-service（denied→403）、http-route-security（scoped
  device）、sessions/content/:id 每请求授权（RESOURCE_HISTORY_CONTRACT §3）。
- 大结果截断：sessions-route "defers large command output and legacy screenshot/artifact
  payloads"（首包不含尾部标记、展开完整恢复）。
- 磁盘失败：history-read-window-reader（注入短读/异常 EOF → 显式 short_read /
  file_identity_changed，不返回混合页面）、persistence 系列。
- 索引损坏：history-read-directory-incremental X05（增量读失败→legacy 全量等价输出）、
  C01（文件缩短→重建不混页）；原始会话文件不被修改（派生目录可重建，T04 §3）。

## 4. 结论

T07 全部要求由"既有等价测试 + 本阶段新增恢复/资源/故障注入测试"覆盖，未发现
只比快照文字的假等价；生产函数未被全 mock（路由、store、builder、projector 均真实）。
能够捕获"退出后恢复正常"类不一致的机制已由 FI-1/FI-2 反向证明。
