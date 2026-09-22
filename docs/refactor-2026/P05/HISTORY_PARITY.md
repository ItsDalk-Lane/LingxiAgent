# HISTORY_PARITY — 跨页 Run 归并与有界读取（P05-T04）

日期：2026-09-22｜基线 HEAD：`1f0537b08`。结论先行：**生产实现 UNCHANGED_VERIFIED**，
本阶段交付 = 全量回归 + phase B 有界读取实测 + 本契约文档；未发现需要修改的缺口。

## 1. 读取栈现状（P00 已定位，本阶段复核接线未变）

- 编排入口 `server/history-read/index.ts readSessionHistoryPage`：目录快路径
  （captureReadContext → probe → resolveHistoryPage → 窗口定位读取 → 同一 projector
  → 读后复核 stat/head/locator）→ 失败链目录 2 次 → legacy 全量（记 fallbackReason，
  不返回旧页、不伪造空历史）。
- 三种模式（全量 / 冷构建 / 热命中）共用同一 projector 与事实实现（I04 不变量）。
- 目录缓存 `cache.ts`（single-flight、预算、失效分类）、增量 `incremental.ts`
  （可信追加 + 受影响关系更新）、窗口读取 `window-reader.ts`（定位读 + 身份校验 +
  短读补齐 + 异常 EOF）。

## 2. 分页游标语义（T04-2）

`before` 是 **display 序号边界**（读取位置），不是 UI 展示 ID：
`resolveHistoryPageBounds`（page.ts:38-49）返回 `[max(0,end-limit), end)`，
0 是合法边界（翻到开头 → 空页终结分页），负数/NaN = 最新页，`all=1` 强制全量
（B07：仅显式请求，无窗口性能断言，不得用 all=1 逃避跨页问题）。
"整页被过滤仍推进"由前端消费侧落实：服务端分页按 display 边界推进，
tests/history-pagination-run-continuity.test.ts T11b（整页隐藏后台通知 → 进度仍推进、
更早历史可达）锁定；服务端同界语义 T11（before=0 空页 + 显式下一页游标）。

## 3. 跨页 Run 归并（T04-2/3 逐场景 → 现有测试）

| 任务书场景 | 锁定测试（本轮全绿，见 §5 命令） |
|---|---|
| 同 Run 跨页不拆、无伪无回复、不重复 | history-pagination-run-continuity T01（1k/10k 夹具分页 vs 全量恢复逐块等价）、T03（任意页大小串联同序同集合）、T04（重复页幂等） |
| 相邻不同 Run 不误并、各自结局 | history-run-outcome-edges T12 |
| 活跃末尾（openTailRun）不误终结 | history-run-outcome-edges T06 |
| 中间失败/部分失败不放大 | history-run-outcome-edges T10a/T10b |
| 无交付/只有过程 | history-run-outcome-edges T07（missing_final_answer 恰一个） |
| 文件 blocks 跨页缝合不重复 | history-run-outcome-edges T14 |
| before=0 / 死循环保护 | T11 / T12c（超页数显式报错） |
| 索引损坏/失效可重建、原始数据不动 | history-read-directory-incremental X05（增量失败→legacy 全量承接，输出等价）、C01（文件缩短→全量重建不混页）、history-read-window-reader X02-X05（读取途中截断/身份变化不返回混合页面） |
| 分支/重试、乱序页 | history-read-retry-rewind-mixed、history-read-directory-incremental C03/C04 |
| 隐藏输入/附件跨页 | history-run-outcome-edges T12 + chat-route-switching 隐藏后台 turn input 系列 |

## 4. 有界读取实测（T04-4，本阶段新增证据）

运行仓库基准 `scripts/benchmark-history-read-directory.mjs --phase B`（真实 Hono 路由
内存进程 + 真实 A03 夹具 1k/10k + sha256 交叉校验；输出
`artifacts/refactor-2026/P05/samples/history-bench-b/`）：

| 指标（热页 p50，页大小 50） | n=1000 | n=10000 | 断言 |
|---|---|---|---|
| fullFileReadCalls | **0** | **0** | 热页零整文件读 |
| sessionFileReadBytes | 0 KiB | 0 KiB | 零字节窗口外读取 |
| fullHistoryProjectionCount | **0** | **0** | 热页零全量投影 |
| jsonlParseCount | 103 | 103 | ≈2×页大小+依赖，**与总规模无关** |
| 冷首页 p50 | 11.0 ms | 89.1 ms | 首建目录成本单列（buildCount 按请求记录），不与后续分页混算 |
| 完整翻页 | 21 页 / 唯一性 overlap=0 missing=0 | 201 页 / overlap=0 missing=0 | 全程无重叠无缺失 |

→ "固定 page size 不得退化成每页全读全 parse"有实测数字支撑：热页工作量 O(K+|Dpage|)
（jsonlParseCount 恒 103），不是 O(总规模)。冷构建/降级请求的整文件扫描如实按请求
记录在 requests-b-*.jsonl（cacheStateNote: cold-build / append-update / hot-hit）。

注：summary-b.md 头部引用的 HEAD/分支来自夹具 environment.json（夹具生成时点记录，
内容 sha256 校验一致）；本次执行的实际 HEAD/分支以 command-log.jsonl 为准。

## 5. 本阶段执行的验证命令（exit code 见 command-log.jsonl）

1. `npx vitest run tests/history-run-outcome-edges.test.ts tests/history-pagination-run-continuity.test.ts`
   → 16/16 绿（P05-T04-history-parity）。
2. `npx vitest run tests/history-read-directory-{build,cache,counters,incremental,memory,scanner,semantics}.test.ts tests/history-read-window-reader.test.ts tests/history-read-cache-auth.test.ts tests/history-read-route-fallback.test.ts tests/history-protocol-compat.test.ts tests/history-protocol-conditional.test.ts tests/history-pagination-invalid-fixture.test.ts`
   → 117 绿 / 4 跳过（1 文件按其自身条件跳过）（P05-T04-directory-suite）。
3. `node scripts/benchmark-history-read-directory.mjs --phase B …`
   → exit 0，全部工作量断言通过（P05-T04-bench-phase-b）。

## 6. 结论

P00 冻结的旧历史/分页契约在本阶段全部复核为真且由测试锁定；本阶段零生产改动。
大历史分页输出与全量恢复逐块等价（T01/T03 既断言），读取有界（§4 实测）。
无 N/A 场景。
