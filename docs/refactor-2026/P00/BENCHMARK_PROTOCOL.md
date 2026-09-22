# BENCHMARK_PROTOCOL — 性能与上下文测量方法（P00-T06 冻结版）

版本：1.0｜冻结日期：2026-09-21｜适用阶段：P00 初始样本 + P07 全量执行（后续阶段不得单方面改口径；改动需在阶段报告中说明并重新基线）。

## 1. 工作负载定义（小/中/大）

| ID | 工作负载 | 小 | 中 | 大 | 输入摘要/种子 |
|---|---|---|---|---|---|
| W1 | server 冷启动（spawn→server-info.json 出现） | 1 次全新隔离 HOME | 同左×10 | 同左×30（P07） | 空 HOME+固定入口 `server/main-full.ts`；无随机输入；OS page cache 不清空（首启后为暖缓存，如实标注） |
| W2 | 长历史分页读取（真实 Hono 路由+真实 v3 夹具，`scripts/benchmark-history-read-directory.mjs` phase A） | n=1,000 条 | n=10,000 条 | n=50,000 条（P07 扩） | seed=20260910，page-size=50；工作量硬断言（fullFileReadCalls/请求=0） |
| W3 | 持续流式输出吞吐 | 200 事件 | 2,000 事件 | 20,000 事件 | 固定合成事件流经 session-stream-store 真实 append/trim；**P00 未执行（实验受限：需本地协议 server harness 接线）**，P02/P07 用 `tests/helpers/model-observability-scenario-harness.ts`（本地 HTTP 供应商替身）执行 |
| W4 | 取消响应时延 | prompt 后 500ms 取消 | 流中 5s 取消 | 工具执行中取消 | 测 UI abort→turn_end{aborted} 的墙钟；**P00 未执行（实验受限，同 W3）**；正确性要求：取消后无新增副作用（执行次数/文件/外发零增量） |
| W5 | 内存（空闲/峰值） | 就绪+3s 空闲 | 200 次 API 写后 | 流式高负载峰值（P07） | 整进程树 RSS 聚合（`ps` 按 PPID 链）；P00 已采初始样本（见 §3） |

平台：darwin 27.0 arm64，Node v24.16.0，CPU 数记录于样本 JSON。**不复制生产会话**；全部输入为合成数据。

## 2. 预承诺比较规则（P00-A08 已用工具固化）

工具：`artifacts/refactor-2026/P00/tools/bench-compare.mjs`（digest 工作负载等价性→median 比较）。

1. **正确性零容忍**：任何性能比较前，先过功能正确性门（对应阶段验收场景）；优化不得改变输出/任务量——不等价的报告无效（工具判 INVALID，自检已验证假优化被拒）。
2. **性能比较**：同环境、同工作负载 digest、重复试验；median 差异 ≤10% 视为波动区间（COMPARABLE）；稳定劣化 >10% 即阻塞（REGRESSION）；改善 >10% 记 IMPROVED，仍需正确性确认后才能接受。
3. 该 10% 是本计划初始验收选择，不是行业定律；有既定产品 SLO 时同时满足。
4. 样本量：连续指标预热后每工作负载 ≥30 次；启动 ≥10 次。启动 10 次的高分位稳定性有限，不作严格尾延迟证明；不足次数标"实验受限"。

## 3. P00 初始样本（全部为本机当前实测，非历史数字）

| 工作负载 | 结果 | 样本文件 | 命令ID |
|---|---|---|---|
| W1 启动 B1 | ready 10/10，median 1479ms，p95 1482ms | samples/startup-b1.json | P00-T06-bench-startup-B1 |
| W1 启动 B2（同输入重跑） | ready 10/10，median 1431ms，p95 1481ms | samples/startup-b2.json | P00-T06-bench-startup-B2 |
| W1 两批比较 | COMPARABLE（-3.2%） | — | P00-T06-bench-compare-r2 |
| W2 历史 n=1000 | 冷首页 p50=11.3ms（3次）；热页 p50=1.1ms/p95=1.4ms（20样本）；完整翻页 21 页 38ms | samples/history-baseline/ | P00-T06-bench-history |
| W2 历史 n=10000 | 冷首页 p50=84.1ms（3次）；热页 p50=0.8ms/p95=1.0ms（200样本）；完整翻页 201 页 271ms；fullFileReadCalls/请求=0 | samples/history-baseline/ | 同上 |
| W5 内存初始样本 | 空闲 477,760KB；200 次 PUT 后 479,504KB（ok 200/200） | samples/memory-initial.json | P00-T06-bench-memory |
| W3 流式吞吐 | **实验受限**（P00 未执行，协议已冻结） | — | — |
| W4 取消响应 | **实验受限**（同上） | — | — |

## 4. 提示词与上下文预算（PROMPT_BASELINE.json）

- 组分分解：系统提示词正文（golden zh 5,194B/估1,747 token；en 6,114B/估1,529 token）＋按需目录引导（zh 172B/en 186B）＋PTC 入口描述＋常驻 4 工具 schema（运行时尺寸，P06 捕获）＋动态用户资料（P06 捕获）。
- token 口径：`lib/llm/estimate-text-tokens.ts`（仓库唯一共享估算器，CJK=1.1 token/字、其余 4 chars/token）——**估算值，非精确 tokenizer**（无对应 tokenizer 时明确字节代理）。
- 预算规则：常驻平台开销不得超过本基线；动态用户资料另列；不得删用户内容/关闭记忆凑数字。
- P06 执行方式：同一模型配置+上下文 fixture 下，用既有 model-call-payload 捕获基础设施（tests/model-call-payload-* 同源）截获最终 request，按来源分解计 token/字节。

## 5. 可重放性

- 所有 bench 工具在 `artifacts/refactor-2026/P00/tools/`，输入全部合成、种子固定；重放=同命令重跑。
- 每次运行记录命令 JSONL（command-log.jsonl）与原始 stdout 日志。
- 环境漂移（Node/OS/依赖版本变化）会使历史样本不可直接比较——比较必须同环境成对进行。
