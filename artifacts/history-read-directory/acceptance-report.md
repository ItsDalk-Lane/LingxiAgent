# 最终验收报告（F06 总报告）

- 生成：2026-09-11｜实际 HEAD `1d42b7405c76292f617291e3a01cd2f3ef5efd04` @ `fix/pending-sep10`（任务改动全部为该提交之上的未提交工作区增量，未提交/未推送/未发布）
- 启动快照（A01）：APFS clone `/tmp/lingxi-baseline-1d42b740` = 纯净 HEAD（porcelain=0）；补丁验证基线 `/tmp/a01-tree`（同 HEAD 干净 clone）
- 状态词表（仅四类）：已修复并验证 / 已实现但指定环境未验证 / 待验证或受阻 / 经证据否定
- 逐项状态：`acceptance-matrix.json` —— P01—P12 + X01—X20 + Y01—Y24 共 **56 项**：55 项已修复并验证、1 项（Y14 真实浏览器跨源 CORS）已实现但指定环境未验证、0 受阻、0 经证据否定。

## 一、阶段判定

| 阶段 | 判定 | 说明 |
|---|---|---|
| A | 已完成（已修复并验证） | 旧路径性能基线 + 可信参考输出 + 差分 harness |
| B | 已完成（已修复并验证） | 目录快路径生产接线；§7.1 硬条件全过；§7.2 四阈值 PASS |
| C | 已完成（已修复并验证） | 可信追加增量：1k/10k 8/8 增量命中、parsedRecords 与规模无关、开放 Run 无 O(Run) 证据 |
| D | 已完成（已修复并验证） | 定向测试 355/355、phase D 复测阈值 4/4 PASS、压力边界 5/5、三配置 typecheck、持久化指纹 repin+guard+tripwire、全量回归失败签名与 HEAD 基线一致 |
| E | 已完成（已修复并验证） | 条件请求（ETag/If-None-Match）+ 概览聚合 + 客户端协议层接线生产路径；E06 调页实验定案 K=100；四组合兼容性全过 |
| F | 已完成（已修复并验证） | F02 协议基准两规模全阈值 PASS + 决定性 304 20/20；F03 barrier 17 用例；F04 定向 406/406 + typecheck 0 + 指纹 guard 0 + 全量失败签名与 A01 基线一致；F05 链路/范围审查（本报告 §四）；F06 交付物齐备（本报告 §七） |

生产路径接通状态：B（目录快路径）已接通——普通分页 `/sessions/messages`（all=0）走 `server/history-read`；C（追加增量）已接通——目录 revision/增量续读为读取主路径；E（协议层）已接通——条件返回与概览路由在 `server/routes/sessions.ts`/`server/index.ts` 生产路由内，客户端 `desktop/src/react/stores/history-protocol-client.ts`、`history-overview-client.ts` 接入真实会话状态流。无测试专用分支、无未启用开关承担上述验收项。

## 二、全量回归对照（D02 + 最终轮）

口径：`npx vitest run`（同 argv/cwd；基线=APFS clone 至 `/tmp/lingxi-baseline-1d42b740` 后 `git restore --source=HEAD --staged --worktree .` + 删除任务未跟踪清单，porcelain=0）。

| 轮次 | 树状态 | 结果 | 退出码 |
|---|---|---|---|
| 基线 | 纯净 HEAD 1d42b740 | 13544 passed / **7 failed** / 7 skipped（1348 文件中 5 文件失败） | 1 |
| D 第 2 轮 | 工作区（闭包/manifest/boundary repin 后） | 13661 passed / **7 failed** | 1 |
| F 最终轮 | 工作区（E/F 全部改动 + 本轮闭包/边界再生成后） | 13735 passed / **7 failed** | 1 |

失败签名比对：最终轮 7 项失败与 A01 基线 7 签名逐条一致（round2×2、round3×1、packaged-desktop-cleanup 90s、audit-seal、ObservabilityDateLine×2），任务引入失败 = 0。逐项签名见 `known-failures-comparison.json`。

基线 7 项全部为 HEAD 既有（详见 D 初版记录与 `known-failures-comparison.json`）；round2 证据测试会随全量运行在位再生成 `artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch`（HEAD 既有行为），已还原，不计入任务 diff。

## 三、持久化指纹门禁（D03 + E/F 复核）

- D03：触及集 `core/session-jsonl-file.ts`、`lib/session-files/session-file-registry.ts`、`server/routes/sessions.ts`；`--classification compatible` repin → guard exit 0；tripwire 15/15。
- E/F 复核：E 协议与客户端渐进能力改动后 4 个受护源 repin → guard exit 0，无 DATA_EPOCH 变更；`tests/persistence-schema-tripwire.test.ts` 15/15。无新持久化文件/表，无数据形状变化。

## 四、F05 真实链路与范围审查

四链路逐环节核对（全部为生产代码路径，未发现停在未调用函数/测试专用分支/未启用开关的环节）：

1. 普通分页：`server/routes/sessions.ts` GET `/sessions/messages` → `server/history-read/index.ts`（目录缓存命中/定点读取）→ `project-page.ts` 同源投影 → `hydrate.ts` 物化当前外部状态 → `protocol.ts` 条件求值 → 200/304。既有语义测试 `history-read-shared-functions`/`sessions-route` 覆盖；回归无失败。
2. 客户端重校验：`history-protocol-client.ts` revalidate 入口携带 receipt/If-None-Match → 304 分支保留已加载 items/nextBefore/Run 状态，200 分支整页替换；任何 200 接收前到达的事件与 304 返回前的失效注入按 F03 barrier 语义处理（`desktop/src/react/__tests__/history-f03-barrier.test.ts`，17 用例全绿）。
3. 概览：`server/history-read/overview.ts` 复用同目录聚合（0 文件读/0 JSONL 解析/不 hydrate，超预算→directory_unavailable）→ 路由挂载于 `server/index.ts` → `history-overview-client.ts` 拉取 → `HistoryOverviewBadge.tsx` 非阻塞展示（失败/401 不吞、stale 丢弃、慢响应不阻塞消息区）。
4. 能力协商：客户端显式 `limit`（HISTORY_PROTOCOL_PAGE_LIMIT=100）→ 服务端校验（省略 limit 旧请求仍 50、最大 200、非法/超限保守处理）→ 翻页仍依 `nextBefore` 原游标语义推进（Y20—Y22）。

范围审查结论：

- 仅一套分支/Run/关联语义（严格层唯一入口，未新增第二套投影）；无服务器响应正文缓存；无客户端第二份正文缓存（客户端仅持有 receipt/etag 等 stamp 与有界统计，32 条/256KiB 上限，Y23）；未为 ETag 增加全历史重读（条件求值基于目录增量 revision）；概览未全扫描 task/run（同目录聚合）。
- 页策略：省略 limit 仍 50、最大 200；未强制 all；最终推荐 limit=100（E06 决策，见 `protocol/page-size-decision.md`）。
- 无新持久化文件/表；无 SDK 核心改动；无发布操作。
- A01 保护清单复核：他人修改未回退/覆盖（`protected-changes.json`；round2 patch 自动再生成为 HEAD 既有行为，已还原）。
- 新增前端变更限于 E 允许的请求/状态/统计/页策略范围：协议客户端、概览客户端、徽标组件、ChatMessageSurface 接线、CSS token 化、i18n 文案、file-refs 选择器；无布局重构、无 Run 合并重写。

## 五、E 协议收益与调页决策（摘要，详见 protocol/）

- 304 收益与命中范围：决定性 304 场景 1k/10k 各 20/20 全 304（bodyBytes=0、rebuilds=0）；同页重复校验响应体 30,753B→0B；ETag 随 limit 变化（etagVariesByLimit=true）；命中范围=普通分页路由（all/reconciliation/find 不进条件快路径，Y11）。
- 概览：准确性 oracle 逐项验证（counts/三桶合计/去重/零与未知区分，Y16）；旧方式逐页探底 201 请求/6095KiB → 概览 1 请求/≤4KiB；热路径 0 文件读；冷路径复用同目录单次扫描。
- 调页：四候选（50/100/150/200）实验后定案 K=100（10k 全翻 201 请求/31.7s → 101 请求/16.5s）；候选对照与取舍理由见 `protocol/page-size-decision.md` 与 `protocol/page-size-experiments/`。
- 新旧四组合（旧/新服务端 × 旧/新客户端）全部通过，见 `protocol/compatibility-matrix.json` fourCombos。

## 六、A/D/F 可比数字（固定 50，同 seed/夹具/硬件）

数据源：`performance-comparison.json`（A/D/F）与 `protocol/protocol-performance-comparison.json`（协议四表）。要点：

| 指标 | A | D | F |
|---|---|---|---|
| 热页 1k p50 (ms) | 7.72 | 0.98 | 0.76 |
| 热页 10k p50 (ms) | 75.53 | 0.67 | 0.61 |
| 冷首页 1k/10k p50 (ms) | 9.27/79.08 | 11.19/93.20 | 10.60/78.94 |
| 完整翻页 1k/10k (ms) | 176.6/15341.3 | 36.8/254.7 | 30.8/226.1 |
| 热页 fullFileReadCalls/请求 | 2 | 0 | 0 |
| 热页 jsonlParseCount (1k/10k) | 4004/40004 | 103 | 103 |

被测量否定的性能假设（按词表仅用于明确假设）：「需要把推荐 50 调到更大」经 E06 部分否定——50→100 在 10k 全翻收益明确（31.7s→16.5s），150/200 无进一步规模收益且放大单页成本，故定案 100 而非上限值。

## 七、最终交付物与补丁验证（F06）

交付目录 `artifacts/history-read-directory/` 按 TASKBOOK F06 树齐备（含 protocol/、verification/、verification-f/、baseline/、reference-outputs/、logs/、patches/）；无空文件占位。

补丁三件（`patches/`）与验证结果（2026-09-11，应用基线=/tmp/a01-tree 干净 HEAD clone）：

| 补丁 | 内容 | `git apply --check` | 应用 | 树级等价验证 |
|---|---|---|---|---|
| a-to-d-server-stage.patch | A01→D 检查点（47 文件，git patch） | 通过 | 成功 | —（既有轮次已验） |
| d-to-final-protocol-stage.patch | D→最终（38 文件；源码/测试/脚本/基线，排除 artifacts 与环境噪音） | 通过 | 成功 | a-to-d→d-to-final 应用后树 vs 工作区 diff=0（exit 0） |
| task-only.patch | A01→最终全量任务专属（74 文件；文件集与 git status 任务清单逐项一致，排除他人既有差异与本地忽略文件） | 通过 | 成功 | 应用后树 vs 工作区 diff=0（exit 0） |

补丁生成方式：`diff -ruN` 统一排除集（.git/node_modules/artifacts/dist*/构建产物/本地忽略文件等），表头归一为 `a/`、`b/` 相对路径；排除补丁与校验清单自身，避免自引用。唯一警告为 a-to-d 既有「EOF 多一空行」空白提示（D 阶段内容，非本轮引入）。

`checksums.sha256` 于全部交付文件定稿后最后生成，覆盖交付目录除自身外全部文件（相对路径）。

## 八、剩余问题、未验证环境与他人修改保护

- 验证覆盖：定向 406/406 exit 0、typecheck 0、差分 21/21 exit 0、tripwire 15/15、guard 0、F03 17 用例、四组合全过；全量 13735 passed / 7 failed（签名=A01 既有）。本地结果不代替其他平台：Windows/Linux/macOS x64 未在本任务执行；打包/发布未执行（无授权要求）；Y14 真实浏览器跨源 CORS 仅 Node loopback 真实 HTTP 层验证，指定环境留待补测。
- 已知边界（非受阻，均在案）：P09 `cache.release()` 未接线（HEAD/B04 既有，有界）；E04.3 无通知外部分支修改的信任边界（Y05 如实标注，不表述为全局保证）。
- 剩余风险清单：`remaining-risks.md`；阻塞记录：`BLOCKED.md`（无未说明阻塞）。
- 他人修改保护：`protected-changes.json`；A01 保护清单逐项复核未发现回退/覆盖。
