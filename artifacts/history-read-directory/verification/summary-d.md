# phase D 摘要 — 服务端阶段复测与压力边界样本（D01，生产实现与 B/C 相同）

- 生成：2026-09-10T12:19:43.937Z｜HEAD `1d42b7405c76292f617291e3a01cd2f3ef5efd04`｜分支 `fix/pending-sep10`｜seed 20260910｜页大小 50
- 口径：Hono 内存进程内（app.request）；非网络耗时；OS page cache 未清空；包装层计时开销计入对应分项（readMs/parseMs 为上界）
- 时间模型：互斥分项：identityMs+readMs+parseMs+serializeMs 可加；serverTotalMs 独立实测；差值=residualMs（含投影/目录定位/合并/GC/未归因部分）。jsonlParseMs ⊂ parseMs（嵌套，不重复相加）。目录仪表（buildCount/appendUpdateCount/cacheStateNote）按 cache stats delta 逐请求记录；增量请求的 parsedRecords 只含新增/续读记录
- 夹具：A03 合法长 Run 夹具（1 文件头 + 1 user + N assistant + N-1 toolResult；display=N+1；页数=ceil((N+1)/50)），与 fixture-audit.json sha256 交叉校验：一致

## 关键数字

| 规模 | 冷首页 p50 / p95 / max (ms) | 热页 p50 / p95 / max (ms) | 热样本数 | 完整翻页（页 / wall ms / 累计 serverTotal ms） | 唯一性 |
|---|---|---|---|---|---|
| n=1000 | 11.2 / 13.8 / 13.8 | 0.98 / 1.48 / 3.08 | 20 | 21 / 37 / 32 | overlap=0 missing=0 |
| n=10000 | 93.2 / 96.0 / 96.0 | 0.67 / 0.83 / 3.57 | 200 | 201 / 255 / 228 | overlap=0 missing=0 |

## 每请求工作量（热页，目录路径）

| 规模 | fullFileReadCalls | sessionFileReadBytes | jsonlParseCount | fullHistoryProjectionCount | metadataVisitedCount | readMs p50 | parseMs p50 | identityMs p50 | residualMs p50 |
|---|---|---|---|---|---|---|---|---|---|
| n=1000 | 0 | 0 KiB | 103 | 0 | 103 | 0.00 | 0.09 | 0.001 | 0.83 |
| n=10000 | 0 | 0 KiB | 103 | 0 | 103 | 0.00 | 0.08 | 0.000 | 0.54 |

## D01 压力边界样本（真实路由，冷构建 + 首/中/末热页）

| 场景 | 首页记录数 | 冷构建 ms | 冷解析数 | 热页解析数 | 热页整文件读 | 热页回退 | 目录驻留 MiB |
|---|---|---|---|---|---|---|---|
| big-tool-result | 50 | 1.97 | 172 | 50 / 52 / 21 | 0 | 0 | 0.06 |
| many-customs | 50 | 2.78 | 531 | 50 / 51 / 51 | 0 | 0 | 0.15 |
| multi-run | 50 | 2.51 | 451 | 50 / 51 / 51 | 0 | 0 | 0.22 |
| many-discarded | 50 | 2.45 | 651 | 50 / 51 / 51 | 0 | 0 | 0.18 |
| large-output | 50 | 4.31 | 251 | 50 / 51 / 51 | 0 | 0 | 0.10 |

场景说明：big-tool-result=单条 200KiB 工具结果正文（目录无正文驻留）；many-customs=assistant+custom 交错；multi-run=200 组 user→assistant（200 个 Run 边界）；many-discarded=300 主链+300 抛弃支链（head=主链叶）；large-output=每条 ~4KiB 输出（页体量大）。

口径备注：

- 热命中请求 `fullFileReadCalls=0`、`jsonlParseCount` ≈ 2×页大小+依赖（目录定位 + 窗口读取，与历史总规模无关）；冷构建/降级请求含整文件扫描与全量解析，如实按请求记录。
- 每请求记录 `cacheStateNote`/`buildCount`/`appendUpdateCount`（cache stats delta）：append-update = 可信追加走增量更新；cold-build = 目录构建（含增量失败后的全量重建）；legacy-fallback = 目录不可用回退旧路径；hot-hit = 目录命中。
- `appendUpdateCount>0` 时 `parsedRecords` 只含增量续读/新增记录（O(新增)；尾记录重读允许），与规模无关是 C05 验收口径。
- `heapUsedDelta` 等内存采样为每请求前后 process.memoryUsage 差值（采样点定义：请求发出前 / 响应返回后）。

**失败/异常项**：无
