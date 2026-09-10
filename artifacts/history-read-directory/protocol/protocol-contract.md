# 历史分页协议合同 v1（E01 冻结）

- 冻结时间：2026-09-10｜基线 HEAD `1d42b740…`｜来源：TASKBOOK E01/E02（逐条落实）
- 实施状态：E01 本文件、E02 服务端条件 GET（`server/history-read/protocol.ts` + `server/routes/sessions.ts` 成功返回边界）、E02.4 测试（`tests/history-protocol-conditional.test.ts`）。E03 客户端 / E04 概览 / E06 页大小实验为后续批次。
- 原则：先冻结规则再写码（本文档与代码同批交付，规则以本文件为准）；仓库已有同职责接口复用并记录映射（见 protocol-dependency-matrix.md），不并存两套协议。

## 一、最小合同（9 行冻结规则）

| 项目 | 冻结规则 |
|---|---|
| 原消息端点 | `/api/sessions/messages`，现有业务 JSON 字段（messages/blocks/todos/sessionFiles/hasMore/nextBefore/revision）和 nextBefore 含义不变 |
| 版本声明 | 新服务端在普通成功消息响应中声明 `Lingxi-History-Protocol: 1` |
| 条件校验 | `ETag` 响应头、`If-None-Match` 请求头；只验证相同普通页面请求（同 before/limit/模式/作用域） |
| 页大小建议 | `Lingxi-History-Page-Limit: K` 响应头，K 为 E06 实测选出的推荐值（E06.2 决策 **K=100**，单一配置点 `server/history-read/protocol.ts` 的 `HISTORY_PROTOCOL_PAGE_LIMIT`；省略 limit 的旧请求仍由服务端默认 50 承接，声明值是对新客户端的建议） |
| 新概览端点 | `GET /api/sessions/history-overview`，复用 sessionId/path 解析与 sessions.read 授权（E04 实现，本合同先冻结名称与授权边界） |
| 公开 revision | 继续使用原文件 revision（`${size}:${mtimeMs}`），不替换格式、不赋予响应级语义；revision≠ETag，两套职责独立 |
| 兼容默认 | 未传 limit 的消息请求仍默认 50；所有请求上限仍为 200 |
| 304 资格 | 只有成功、可解释的普通页面（目录模式 `mode=directory`）；all=1/reconciliation=1 不使用本协议的条件快路径 |
| 私有数据 | 200/304/概览采用 `Cache-Control: private, no-store`；客户端显式条件请求使用 `cache: 'no-store'`（E03 客户端实施） |

版本与页大小声明是能力/建议，不是访问凭证、分支身份或完成状态。旧客户端不发送条件头时仍得到正常 200 和原 JSON；新客户端看到缺失/不支持的版本或不可读头部时，按现有 50 条请求和 200 处理回退。

E1 第一版采用"当前页面完整投影后生成 ETag"：服务端仍通过 B/C 读取本页和必要依赖、读取现有外部状态，得到当前响应后再比较摘要。不实施"先看 file revision 就直接 304"；不为提前 304 做全仓 store 版本化改造。完整页面必须来自 B/C 路径。

## 二、状态转换的确定行为（全部路径，不只理想路径）

| # | 状态转换 | 服务端行为 | 客户端（合同层面）预期 |
|---|---|---|---|
| 1 | 首次加载 | 无 If-None-Match → 200 + 全响应 + 协议头/ETag/私有策略 | 正常渲染并保存标签 |
| 2 | 同页重复校验 | B/C 读取与外部状态照常执行；表示未变（摘要相等）→ 304 无正文；变了 → 200 新表示新标签 | 复用已持有表示；304/200 都按服务端为准 |
| 3 | 新页加载 | before/limit 进入标签请求身份；跨页标签必然失配 → 200 | 无条件请求新页，不发送旧页标签 |
| 4 | 文件追加 | 追加改变响应（messages/nextBefore 等）→ 摘要不同 → 200；目录路径经增量/重建保证新记录可见（C03） | 200 即重新应用 |
| 5 | 同长度重写 | mutation-epoch 通知（C02）→ 目录失效 → 全量重建 → 响应内容变化 → 200；若重写后内容恰好逐字节相同且外部状态未变，允许 304（表示相同即不变，诚实语义） | 同上 |
| 6 | 分支切换 | head 选择变化 → 分支身份进入摘要 → 标签失配 → 200 新分支页 | 不得复用旧分支表示 |
| 7 | 外部状态改变（todos/sessionFiles/deferred 任务，JSONL 不变） | 外部 store 本次读取结果在响应体内 → 摘要不同 → 200 | 按新表示应用 |
| 8 | 客户端收到 WS | 合同不变：WS 到达后的重新校验仍走本合同；若服务端此前广播使响应变化 → 200，否则 304 | 按 E03 版本护栏应用/丢弃 |
| 9 | 客户端会话被淘汰 | 客户端丢弃校验记录（E03）；淘汰后重进=首次加载（行为 1） | 无标签或旧标签→200 |
| 10 | 鉴权失效 | 授权在条件求值**之前**：403 照常返回，不携带 ETag/协议头，不泄露任何标签 | 重新鉴权后再校验 |
| 11 | 连接切换 | 连接/凭证不参与标签值（作用域含主体/runtime 内部身份与盐）；不同连接同作用域同表示 → 304 合法 | — |
| 12 | 服务器重启 | 进程内随机盐更换 → 旧标签全部失配 → 正常 200（不要求跨重启命中） | 首请求即恢复 |
| 13 | 旧服务器缺少概览/头部 | 旧服务器不返回 `Lingxi-History-Protocol`/概览端点 → 客户端按现有 50 条请求和 200 回退（E03 实施护栏） | 回退路径保持既有语义 |

## 三、服务端实现规则（E02 冻结）

1. **接入点**：普通消息路由成功返回边界——授权、B/C 页面读取与快照复核、原 projector、外部状态补齐、lifecycle/rebroadcast（首屏恰一次）全部完成后；不在授权之前、不在目录命中处接入。广播不因最终 304 被取消或重复（广播先于求值，且求值零副作用）。
2. **八步流程**：身份解析/授权/请求校验 → B/C 读取与快照复核 → projector+外部状态 → lifecycle/rebroadcast（仅一次）→ 确定响应体与协商头 → 序列化一次并计算作用域 ETag → 合法匹配：304 无正文；否则 200 发送同一字节串。
3. **单次序列化**：200 与摘要共用同一 `JSON.stringify` 字节串；字节串不写入目录/响应缓存，请求结束释放。不新增时间戳/随机 snapshotId，不删字段制造命中。
4. **摘要输入**（顺序 framing 以 `\0` 连接后 sha256）：协议版本 `hrp1:v1`、进程盐（randomBytes(32)，不落盘）、principalId、serverNodeId、studioId、sessionId、规范 locator（path.resolve）、端点、before（null→`latest`）、实际 limit、模式 `normal`、语言维度（恒空，预留）、分支选择身份（**仅 selectedLeafId**——决定页面展示哪条分支；headResolution 是选择机制而非表示维度，physical tail 变化（弃支追加）不改变当前页表示，两者进摘要只会让标签在表示未变时失配）、revision、响应 UTF-8 字节。只纳入非敏感内部身份或其摘要；标签不可逆，不含 bearer token/路径原文/正文/账号。
5. **响应头**：`Lingxi-History-Protocol: 1`、`Lingxi-History-Page-Limit: K`（K=50，单一配置点）、`ETag: W/"hrp1-<64hex>"`、`Cache-Control: private, no-store`。304 无 Content-Type/无正文/无 Content-Length；私有策略与 ETag 必须在。缓存策略为 no-store，故不设 Vary（无缓存中介参与协商）。
6. **If-None-Match 语义**（RFC 9110）：实体标签列表、弱比较（忽略 W/ 前缀）、`*` = 资源存在即匹配（已先授权+确认资源存在）；官方客户端只回显单个服务器标签，绝不发送 `*`。语法无效（无任何合法元素）→ 忽略条件头正常 200（记录测试）。超长头按既有 HTTP 头部限制，不新造宽松入口。
7. **绝不 304 的情形**：403/404/500、读取失败、分支不确定（未走 directory 模式）、revision=null、all=1/reconciliation=1、普通读取降级（legacy 全量兜底 `mode=full`）。缺条件头/标签过期/跨页/跨作用域 → 正常 200。
8. **降级不签发**：`mode=full`（legacy 兜底）、revision=null、身份不可靠（无规范路径）→ 不签发标签、不携带协议头，仍 200 + 私有策略。
9. **all/reconciliation**：客户端不发送条件头；服务端不让其进入条件求值函数，按原完整模式处理（回归测试覆盖）。
10. **日志**：只含 requestId、结果、截短不可逆摘要（tag 前 16 字符）；不记录 Authorization、完整路径查询或响应正文。

## 四、验收锚点

- E02.4：`tests/history-protocol-conditional.test.ts`（每个 304 以同请求无条件 200 为 oracle 比较实际当前 JSON，不只比 hash）。
- 语义失败数=0；既有差分 21/21 逐字节一致（200 字节等价）。
- 性能与内存预算见 `protocol-acceptance-budget.json`（E06.2 冻结，D 基线锚点 `protocol/baseline-d/`）。
