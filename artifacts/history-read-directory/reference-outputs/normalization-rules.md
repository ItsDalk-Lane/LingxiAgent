# normalization-rules.md — 参考输出归一化规则（A04 §2）

- 状态：**先于参考采集运行制定**（本文件先落盘，采集脚本
  `scripts/collect-history-read-directory-reference.mjs` 按此实现并校验本文件存在，
  不得在采集时临时决定规则）。
- 基准：HEAD `1d42b7405c76292f617291e3a01cd2f3ef5efd04`，A03 修正后合法夹具。
- 目的：同一数据比较必须固定外部 store、语言、时钟与随机标识后，参考输出才能逐字节可比。
- 归一化只作用于**保存到 reference-outputs/ 的响应副本**；发送给生产路由的请求与生产
  代码本身不做任何归一化。

## 1. 非确定性来源清单（在 messages 路由可采集请求集合内）

| # | 来源 | 位置 | 处理 |
|---|---|---|---|
| N1 | `revision = "${stat.size}:${stat.mtimeMs}"` | `core/session-list-projection-cache.ts: sessionFileRevision`（经 `server/routes/sessions.ts: readSessionFileRevision`） | 保留 size，mtime 分量替换为 `<mtimeMs>` 字面量（规则 R1） |
| N2 | 临时根目录绝对路径（os.tmpdir 下的基准夹具目录） | 仅可能出现在任意字符串字段中（采集环境引入，非响应固有内容） | 所有字符串值中的 `<tmpRoot>` 前缀替换为 `<TMPROOT>`（规则 R2） |
| N3 | 随机 UUID | 唯一已知生产来源是 reconciliation 响应的 `snapshotId: randomUUID()`（`sessions.ts:2160`）；本采集**不包含** reconciliation 请求 | 防御性规则 R3：任何字符串值整体匹配 UUID v4 形态则替换为 `<UUID>`，并在该参考条目的 `normalizedFields` 中如实记录；预期为空 |
| N4 | 时钟（响应内时间戳） | 夹具全部条目使用固定时间戳字符串（`2026-09-10T09:00:00Z` / `2026-09-10T10:00:00Z`），消息 `timestamp`/`startedAt`/`endedAt` 均派生自条目 | 无需归一化；断言输出中不得出现采集当刻时间（规则 R4 校验失败即报错，不静默） |
| N5 | 语言 / 外部 store | 路由语言为服务端默认；manifest store 为每次请求全新空库（固定外部 store=空）；deferred/subagent/registry 等 store 在 harness engine 中不存在（路由以 optional-chaining 空值安全跳过） | 固定条件，无需归一化 |
| N6 | 键序 / 数组序 | `JSON.parse` → `JSON.stringify(…, 2)` 保持原键序；响应数组序即业务序 | 保留原序，不排序（规则 R5） |

## 2. 归一化规则（脚本实现口径）

对采集到的 HTTP 响应 JSON（状态码必须为 200，否则该请求按失败记录，不产出参考）执行
深度优先遍历：

- **R1（revision mtime）**：顶层 `revision` 字段为字符串且匹配 `^\d+:\d+(\.\d+)?$` 时，
  改写为 `${size}:<mtimeMs>`（size 原样保留；mtime 分量替换为字面量 `<mtimeMs>`）。
  归一化前的原值只记录 `revisionSize`，不记录 mtime。
- **R2（临时路径）**：每个字符串值中出现的「本次采集临时根目录绝对路径」替换为 `<TMPROOT>`
  （可能多处出现，全部替换）。
- **R3（UUID 防御）**：每个字符串值若整体匹配
  `^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$`（大小写不敏感），
  替换为 `<UUID>`，并在 manifest 的 `normalizedFields` 中记录字段路径。夹具的 SDK 文件头
  UUID `7e5f1a3c-…` 不出现在响应正文（不进入 messages 投影），若出现将被本规则改写并记录。
- **R4（时钟断言）**：归一化后输出中若出现 ISO-8601 UTC 时间戳（形如
  `YYYY-MM-DDTHH:MM:SSZ`），其时分秒必须是夹具固定值 `09:00:00` 或 `10:00:00`（夹具条目
  仅使用这两个固定时间戳，见 A03 构造）；出现其他时分秒即判定采集污染（真实时钟泄漏），
  脚本报错退出（非零），不产出参考。
- **R5（字节稳定序列化）**：归一化后对象以 `JSON.stringify(obj, null, 2) + "\n"`（UTF-8）
  落盘；`sha256` 与 `sha256Normalized` 均按该字节序列计算。

## 3. 不做归一化的字段（比较中不允许删改）

`messages[*].id / sourceIndex / entryId（即 id）/ role / content`、块 `afterIndex`、
`turnStartIndex / turnEndIndex`、deferred 描述符 `id`（base64url 定位器，确定性派生）、
`hasMore / nextBefore / todos / sessionFiles`、Run 边界与工具归属字段。这些字段的任何差异
都是真实语义差异，必须在比较中如实暴露。

## 4. 记录要求

每份参考响应在 `reference-manifest.json` 中记录：请求描述（fixture、before/limit/all）、
HTTP 状态、`sha256Normalized`、关键字段清单（messages 数、首末条 id/sourceIndex/role、
blocks 数、todos、hasMore、nextBefore、sessionFiles 数、revisionSize、deferred 描述符数、
`responseUtf8Bytes`）、读取前后会话文件 sha256 与字节数、`.repair.json` 备份是否产生、
生产读取是否改写文件（`fileUnchanged`）、以及该请求的仪表计数（含
`fullFileReadCalls / jsonlParseCount / fullHistoryProjectionCount / fallbackReason`）。

仪表内存采样（heap/external/arrayBuffers/rss）属于性能证据，不属于表示身份，
**不参与** sha256 计算。
