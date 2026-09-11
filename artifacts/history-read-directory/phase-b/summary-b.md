# phase B 摘要 — 历史读取目录路径（B06/B07 生产接线）


- 生成：2026-09-10T08:38:50.564Z｜HEAD `1d42b7405c76292f617291e3a01cd2f3ef5efd04`｜分支 `fix/pending-sep10`｜seed 20260910｜页大小 50
- 口径：Hono 内存进程内（app.request）；非网络耗时；OS page cache 未清空；包装层计时开销计入对应分项（readMs/parseMs 为上界）
- 时间模型：互斥分项：identityMs+readMs+parseMs+serializeMs 可加；serverTotalMs 独立实测；差值=residualMs（含投影/路由循环/GC/未归因部分）。jsonlParseMs ⊂ parseMs（嵌套，不重复相加）。directoryMs/hydrateMs/externalStateMs 阶段 A 恒 0（无目录；引擎无外部 store；路由内联 hydrate 无法在不改生产代码情况下单列，归入 residual）
- 夹具：A03 合法长 Run 夹具（1 文件头 + 1 user + N assistant + N-1 toolResult；display=N+1；页数=ceil((N+1)/50)），与 fixture-audit.json sha256 交叉校验：一致

## 关键数字

| 规模 | 冷首页 p50 / p95 / max (ms) | 热页 p50 / p95 / max (ms) | 热样本数 | 完整翻页（页 / wall ms / 累计 serverTotal ms） | 唯一性 |
|---|---|---|---|---|---|
| n=1000 | 9.8 / 11.8 / 11.8 | 0.72 / 1.27 / 2.32 | 20 | 21 / 30 / 27 | overlap=0 missing=0 |
| n=10000 | 73.0 / 80.0 / 80.0 | 0.56 / 0.66 / 3.77 | 200 | 201 / 209 / 186 | overlap=0 missing=0 |

## 每请求工作量（热页，阶段 A 旧路径）

| 规模 | fullFileReadCalls | sessionFileReadBytes | jsonlParseCount | fullHistoryProjectionCount | metadataVisitedCount | readMs p50 | parseMs p50 | identityMs p50 | residualMs p50 |
|---|---|---|---|---|---|---|---|---|---|
| n=1000 | 0 | 0 KiB | 103 | 0 | 103 | 0.00 | 0.09 | 0.000 | 0.58 |
| n=10000 | 0 | 0 KiB | 103 | 0 | 103 | 0.00 | 0.08 | 0.000 | 0.44 |

口径备注：

- `fullFileReadCalls=2`/请求 = repair 同步整文件读（read-api） + SDK loadEntriesFromFile 循环全量装载（fd-episode）；readSessionHeader 4KiB 有界头扫描与 looksLikePiSessionFile 512B 探测不计入整文件读（计入 readCalls/logicalReadBytes）。
- `jsonlParseCount` = JSON.parse 包装层对会话 JSONL 形态（`{"type":…` 开头）行的计数 = repair 行解析 + SDK 行解析 + 头解析；1k 每请求 4004 = 2×2001+2，10k 每请求 40004 = 2×20001+2。
- `fullHistoryProjectionCount` = getBranch 调用数（旧路径 getBranch→projectBranchHistory 1:1 代理口径；fallback 路径不调 getBranch）。
- `metadataVisitedCount` = 被包装的 5 个全数组扫描入口（origin/collab/modelCallRef/toolOutcomes/todos）访问的条目总数（下界；路由内联预扫描未计入，见 A02 read-path-map §1）。
- `heapUsedDelta` 等内存采样为每请求前后 process.memoryUsage 差值（采样点定义：请求发出前 / 响应返回后）。

**失败/异常项**：无
