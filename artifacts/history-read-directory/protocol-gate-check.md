# 协议技术闸门（D07）

- 生成：2026-09-10｜基线 HEAD `1d42b7405c76292f617291e3a01cd2f3ef5efd04` @ `fix/pending-sep10`
- 基线产物：`protocol/baseline-d/`（requests-d.jsonl 1582 条逐请求记录、summary-d.{json,md}）
- 命令：`node scripts/benchmark-history-protocol.mjs --phase D --sizes 1000,10000 --seed 20260910 --page-sizes 50 --rtt-ms 0,50,150 --bandwidth-mbps 10,50 --output artifacts/history-read-directory/protocol/baseline-d`（exit 0，failures=0，全部 status=200）
- 脚本能力：真实 HTTP loopback（@hono/node-server 挂 D01 同一 Hono app）+ 本地 TCP 代理确定性整形（rtt=每请求双向首字节各 rtt/2 合计一次往返；带宽=每方向 64KiB 桶 token bucket，小页走预存令牌即时放行、持续吞吐受限速）；未知参数 exit 2、phase E/F 预留 exit 2、校验失败 exit 1。三口径分列（Hono 内存 span / HTTP loopback / 受控链路），不代表公网质量。

| 闸门 | 判定 | 依据 |
|---|---|---|
| G1 服务端基础可靠 | **成立** | P/X 32/32「已修复并验证」（acceptance-matrix.json）；D01 定向三段 355/355 无新增失败；全量回归失败签名与纯净 HEAD 基线逐条一致（diff=空，7 项均 HEAD 既有）；phase D 性能阈值 4/4 PASS（verification/summary-d.json）。本链路必测通过；第 5.2 节保证边界（跨请求前端一致性）如实单列，不写 P05 全面通过。 |
| G2 已有协议基线 | **成立** | 四行为 ×（2 规模 × 7 链路格）数据齐全：a 首屏、b 重复校验、c 完整翻页（1k=21 页/10k=201 页逐格断言通过）、d 总量/任务=现状无端点（requests=0 如实记录）。每请求分列：honoSpanMs、请求/响应头与正文字节、客户端读取/JSON 解码/状态应用、RTT/服务端/客户端测量域分开；HTTP 真实请求（非 Hono 内存替代）。客户端成本复现边界（buildItemsFromHistory 完整链/React 提交渲染不可 Node 复现）逐请求 gaps 字段注明。 |
| G3 现有语义与表示边界可解释 | **成立** | 见下清单。 |
| 授权状态（非待决闸门） | 已获得，本任务内继续 | — |

## G3 依赖与边界清单

**消息响应依赖（`/api/sessions/messages` 普通成功页）**：`messages`（含 id/sourceIndex/entryId/role/内容与 blocks 引用坐标）、`blocks`、`todos`（外部 store 现值）、`sessionFiles`（registry 可达文件）、`hasMore`、`nextBefore`、`revision`（文件 stat 签名，无响应级语义）。另有 **deferred 凭证**：重内容以 content-id 形式内嵌于 messages，经独立 content 端点凭 sourceIndex/entryId 语义展开（server/history-deferred-content.ts）——条件快照必须覆盖这些字段，不能只 hash 正文。

**既有重载语义**：`all=1` 强制全量返回（不进目录、无窗口断言，保留旧路径）；`reconciliation`（独立路由，严格对账不在普通页协议内）；`find` 路由仅复用同源页边界 `resolveHistoryPageBounds`。E 合同的 304 资格只限普通成功页面，与上述重载互斥。

**客户端失效入口清单**（desktop/src/react/stores/）：
1. `invalidateSessionCache(path)`（chat-slice.ts:6，调用点 :484 流式块消费、:517 interlude、:601 本地/乐观消息、:634/:677 registry set/upsert、:614 registry 更新）；
2. `invalidateStreamBuffer` / `invalidateStreamResumeMeta`（stream-invalidator）；
3. LRU 淘汰（chat-slice.ts:150–162：淘汰会话时同步清 FileRef 缓存与流缓冲）；
4. `_loadMessagesVersion` 竞态护栏（session-actions.ts loadMessages：rapid switch/并发 load 时 stale 响应丢弃）；
5. `messageLiveVersionBySession` / `todosLiveVersionBySession` mid-flight live 更新早退；
6. SessionFilesFlight（begin/consume + resetSeen：branch reset 全量权威替换 vs HTTP 快照竞态，upsert 重放）；
7. 发送失败重试：`markOptimisticUserMessageFailed(retryable)` 与 queued turn input 状态机（ready/blocked/failed）；
8. WS/bridge live 事件经上述 upsert/invalidate 原语进入（无独立缓存通道）。

**模式排除清单**：all/reconciliation/content(deferred)/find 均不参与条件快路径（E01 合同冻结）；概览（E2）与页大小建议（E3）为 E 阶段功能，本基线未实现。

**权限边界保持**：sessionId/path 解析（SessionManifestStore）与 sessions.read 授权不变；协议头（ETag/版本/页大小建议）仅为能力/建议，不是访问凭证、分支身份或完成状态。

## 关键基线数字（10k 最劣格 rtt=150ms/bw=10Mbps；完整矩阵见 summary-d.md）

| 行为 | 请求数 | wall | 总字节 |
|---|---|---|---|
| a 新进入首屏 | 1 | 158.7ms | 31.2KiB（正文 30.8KiB） |
| b 同页重复校验（现状=重新完整 GET） | 1 | 156.0ms | 31.2KiB（与首屏相同字节——E1 目标收益所在） |
| c 完整向前翻页 | 201 | 31,738ms | 6,061KiB |
| d 只取总量/任务分布 | 0（无端点，现状缺口如实记录） | — | — |

客户端单页成本（Node 可复现部分）：JSON 解析 ≈0.09ms、状态应用骨架 ≈0.01ms（30KB 页）；渲染/完整 buildItems 链不可 Node 复现（gaps 字段注明）。服务端冷构建 span：1k 21.2ms / 10k 121.3ms（含目录构建；后续请求热路径 Hono span 亚毫秒级，见 phase D 复测）。

## 备注

- 本步骤只建基线与闸门文档，未实现任何 E 功能（无 ETag/概览/页大小头/协议版本头）。
- 大基准独占运行（无并发高负载任务）。
