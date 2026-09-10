# phase F 摘要undefined

- 生成：2026-09-10T16:27:30.501Z｜HEAD `1d42b7405c76292f617291e3a01cd2f3ef5efd04`｜分支 `fix/pending-sep10`｜seed 20260910｜页大小 50
- 口径：Hono 内存进程内（app.request）；非网络耗时；OS page cache 未清空；包装层计时开销计入对应分项（readMs/parseMs 为上界）
- 时间模型：互斥分项：identityMs+readMs+parseMs+serializeMs 可加；serverTotalMs 独立实测；差值=residualMs（含投影/目录定位/合并/GC/未归因部分）。jsonlParseMs ⊂ parseMs（嵌套，不重复相加）。目录仪表（buildCount/appendUpdateCount/cacheStateNote）按 cache stats delta 逐请求记录；增量请求的 parsedRecords 只含新增/续读记录
- 夹具：A03 合法长 Run 夹具（1 文件头 + 1 user + N assistant + N-1 toolResult；display=N+1；页数=ceil((N+1)/50)），与 fixture-audit.json sha256 交叉校验：一致

## 关键数字

| 规模 | 冷首页 p50 / p95 / max (ms) | 热页 p50 / p95 / max (ms) | 热样本数 | 完整翻页（页 / wall ms / 累计 serverTotal ms） | 唯一性 |
|---|---|---|---|---|---|
| n=1000 | 10.6 / 12.5 / 12.5 | 0.76 / 1.37 / 2.26 | 20 | 21 / 31 / 28 | overlap=0 missing=0 |
| n=10000 | 78.9 / 81.8 / 81.8 | 0.61 / 0.74 / 3.54 | 200 | 201 / 226 / 203 | overlap=0 missing=0 |

## 每请求工作量（热页，目录路径）

| 规模 | fullFileReadCalls | sessionFileReadBytes | jsonlParseCount | fullHistoryProjectionCount | metadataVisitedCount | readMs p50 | parseMs p50 | identityMs p50 | residualMs p50 |
|---|---|---|---|---|---|---|---|---|---|
| n=1000 | 0 | 0 KiB | 103 | 0 | 103 | 0.00 | 0.08 | 0.000 | 0.63 |
| n=10000 | 0 | 0 KiB | 103 | 0 | 103 | 0.00 | 0.08 | 0.000 | 0.49 |

口径备注：

- 热命中请求 `fullFileReadCalls=0`、`jsonlParseCount` ≈ 2×页大小+依赖（目录定位 + 窗口读取，与历史总规模无关）；冷构建/降级请求含整文件扫描与全量解析，如实按请求记录。
- 每请求记录 `cacheStateNote`/`buildCount`/`appendUpdateCount`（cache stats delta）：append-update = 可信追加走增量更新；cold-build = 目录构建（含增量失败后的全量重建）；legacy-fallback = 目录不可用回退旧路径；hot-hit = 目录命中。
- `appendUpdateCount>0` 时 `parsedRecords` 只含增量续读/新增记录（O(新增)；尾记录重读允许），与规模无关是 C05 验收口径。
- `heapUsedDelta` 等内存采样为每请求前后 process.memoryUsage 差值（采样点定义：请求发出前 / 响应返回后）。

**失败/异常项**：无
