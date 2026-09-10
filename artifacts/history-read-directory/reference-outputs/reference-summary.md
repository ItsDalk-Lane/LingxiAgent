# reference-outputs 摘要（A04）

- 生成时间：2026-09-10T05:00:20.370Z；HEAD `1d42b7405c76292f617291e3a01cd2f3ef5efd04`；seed 20260910
- 口径：Hono 内存进程内（`app.request`），非网络耗时；旧读取路径（无目录缓存）。
- 防污染：每请求独立夹具副本；见各条目 `antiPollution`（含前后 sha256）。

| 规模 | 请求 | messages | hasMore | nextBefore | fullFileReadCalls | jsonlParseCount | fullHistoryProjection | 文件未变 | sha256(归一化) 前 12 |
|---|---|---|---|---|---|---|---|---|---|
| n=1000 | first-page | 50 | true | 951 | 2 | 4004 | 1 | true | 920642f7b514 |
| n=1000 | middle-page | 50 | true | 451 | 2 | 4004 | 1 | true | 7ee7e21b0fa0 |
| n=1000 | last-page | 50 | false | null | 2 | 4004 | 1 | true | 8a0c833642c9 |
| n=1000 | all | 1001 | false | null | 2 | 4004 | 1 | true | 84d9ae39aefb |
| n=10000 | first-page | 50 | true | 9951 | 2 | 40004 | 1 | true | 45f1c96d9e1b |
| n=10000 | middle-page | 50 | true | 4951 | 2 | 40004 | 1 | true | 5a3c90dc1780 |
| n=10000 | last-page | 50 | false | null | 2 | 40004 | 1 | true | 167edeefb956 |

## n1000 完整翻页

- 页数 21（理论 21），记录 1001 条，唯一 1001，重叠 0，与 all=1 等价：true

## n10000 完整翻页

- 页数 201（理论 201），记录 10001 条，唯一 10001，重叠 0，与 all=1 等价：true

字段级明细见 `reference-manifest.json`；归一化规则见 `normalization-rules.md`。
