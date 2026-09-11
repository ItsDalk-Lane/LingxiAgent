# 实现报告（implementation-report）

- HEAD 基线：`1d42b7405c76292f617291e3a01cd2f3ef5efd04` @ `fix/pending-sep10`（A01 启动快照一致）
- 阶段状态：A/B/C/E/F01–F04 已修复并验证；D07 完成（真实传输基线）；F05/F06 本报告。
- 详细分阶段日志：`PROGRESS.md`；服务端阶段对照：`server-stage-report.md`。

## 一、生产路径接线（全部真实链路，非测试专用分支）

| 链路 | 接线位置 |
|---|---|
| 普通分页 | `/api/sessions/messages`（all=0）→ `server/history-read/index.ts` readSessionHistoryPage → probe（C01 判定序）→ 目录命中/增量（C03）/重建 → 定点窗口读取（window-reader）→ 同源 projector（project-page/projection-context）→ 当前外部状态补齐（hydrate）→ E02 条件返回（成功返回边界） |
| 条件 GET | `server/history-read/protocol.ts`：ETag（W/"hrp1-<sha256>"，作用域=盐+主体/runtime/studio/会话/locator+请求身份+分支选择身份+响应字节）→ RFC 9110 弱比较/`*`/非法忽略 → 304 无正文 |
| 概览 | `GET /api/sessions/history-overview` → 同目录聚合（overview.ts 计数器 O(1) 读取）→ E04 三 reason 不可用结构 |
| 客户端 | `desktop/src/react/stores/history-protocol-client.ts`（receipt/请求键/条件包装）+ `history-overview-client.ts`（概览非阻塞客户端）+ HistoryOverviewBadge（最小展示） |

## 二、已实现功能清单

- B：目录快路径（probe 四条件/single-flight/8 槽 LRU/64MiB 驻留/16MiB 单目录准入/版本化发布）；定点窗口读取；失败链 尝试1→invalidate+重建→尝试2→legacy。
- C：C01 判定序 + C02 mutation-epoch（插桩全部受支持重写路径）+ C03 增量扫描（SegmentedList/MapOverlay 版本隔离、Run 小表、off-by-one 修复）+ C04 受影响关系 + C05 命中率验收。
- E：条件 GET（ETag/304/私有头）+ 概览端点 + K=100 协商落地 + 客户端条件校验/失效（E03）+ 概览接线（E05）。

## 三、真实环境验证

- 定向：406/406（协议/概览/兼容/F03 barrier/history-read 十文件/P08 语义/session-actions store）。
- 全量：13735 passed / 7 failed（与 A01 纯净树基线签名逐条一致，任务引入 0）。
- 差分：三模式 21/21 逐字节一致（×3 复验）。
- 性能：F 复测四阈值 PASS；决定性 304 两规模 20/20。
- 指纹：compatible repin + guard 0 + tripwire 15/15。

## 四、已知局限

- retired 旧版本释放依赖 LIMIT=2/invalidate（`cache.release()` 无调用方，B04 既有，有界）。
- 真实浏览器跨源（Y14）留待指定环境；Node loopback smoke 已覆盖头契约。
- 外部进程 in-place 改写未读前缀不可与纯追加区分（C02 信任边界）。
- 冷首页 10k 存在运行方差（73–93ms，阈值内）。

## 五、被测量否定的性能假设

- 「需要把推荐 50 调到更大」：E06.1 四候选实测——K=100 取得主要收益（往返 −50%、全翻 −48%），K≥150 首屏 +22~26ms/档且载荷翻倍，边际收益不抵成本 → 选定 K=100 而非 200。
- 「冷页阈值需放宽」：B 冷页优化后 73–93ms 达标（阈值 123.84），未放宽。
