# Model Observatory Phase 10.1 — Adversarial Finding Ledger

> 状态：Phase 10.1 本地修复复验完成。各条 `Before` 与失败测试保留修复前历史；`After` 记录当前工作树的真实复验结果。远端跨平台流水线与提交封印不在本地修复结论中冒充完成。
>
> 真相源：当前远程功能分支、失败测试、独立 Provider Witness、SQLite 实际行、HTTP DTO、真实 UI 纵向链。

## 审计坐标

| 坐标 | 值 |
|---|---|
| PHASE10_1_START_SHA | `cda8dbe5931f562a4eef47204ac571d29941035a` |
| 远程功能分支 | `feature/model-call-observability` → `cda8dbe5931f562a4eef47204ac571d29941035a` |
| 本地 main | `e62bb53545c3439cf798a9785f97ada3e1d6a3e1` |
| 远程 main | `bf3c80b5681c99fc7ff05b9c168898e9ca317587` |
| 当前封印 | `3f20b58ebe0648aa4389447913b58642075ff73c`（`commit` 对象） |
| 当前工作状态 | detached HEAD；按用户要求不创建、不切换工作树或分支 |

## 严重级别口径

- P0：秘密泄漏、远程越权读取敏感正文、观测改变真实 Provider 请求、调用/调用链身份错绑。
- P1：把错误或未知数据展示为事实、筛选/调用链统计错误、Blob 损坏或悬空引用、在途调用被配置切换截断、发布证据虚假闭合。
- P2：通常不直接制造错误业务事实，但影响恢复能力、性能、维护性或安全纵深。

## AR-01 — 生产 UI 未发送 IANA 时区

- Severity：P1
- 状态：FIXED
- Before：生产聚合请求由界面 helper 发送当前固定时差，跨夏令时历史窗口会错分当地日期。
- Failing test：界面 helper 单测 + `UI → action → Hono → normalize → Query` 跨 2026-03-08 的纵向测试，断言请求使用 `America/Los_Angeles`。
- Root cause：界面只读取当前本地时差，没有读取浏览器解析出的 IANA 时区。
- Fix：新增 browser-safe 本地时区 helper；非空合法时区优先发送，无法取得时才回退固定时差。
- After：`model-observability-vertical.test.tsx` 的“UI 日期分组经 action → route → query 发送浏览器 IANA 时区”通过；真实界面动作经 HTTP 路由到查询层，收到 `America/Los_Angeles`。
- Remaining limitation：浏览器不提供合法时区时只能使用当前固定时差，并需明确属于 fallback。

## AR-02 — DST 算法漏掉偶数次切换

- Severity：P1
- 状态：FIXED
- Before：切换查找只比较区间两端；时差中途变化后恢复时返回“没有切换”。
- Failing test：洛杉矶 2026 全年、卡萨布兰卡同年多次切换、跨 2～3 年范围；同时校验切换边界和当地日期。
- Root cause：端点相等被错误当成区间内从未变化。
- Fix：有界固定步长扫描，发现时差变化后再二分定位边界。
- After：`model-observability-query-truth-integrity.test.ts` 的偶数次切换用例与 `model-observability-e2e-dst.test.ts` 的洛杉矶日期纵向用例通过；窗口端点时差相同也能发现中间切换。
- Remaining limitation：仅依赖本机 `Intl` 时区数据库；不访问网络。

## AR-03 — DST 段数上限静默降级

- Severity：P1
- 状态：FIXED
- Before：段数达到上限后退出循环并继续返回不完整日期表达式。
- Failing test：通过纯函数/正规依赖注入确定性触发段数上限，断言显式 `date_bucket_too_complex`，不得返回 200 + 错分组。
- Root cause：保护上限只有控制流 `break`，没有错误语义。
- Fix：超限抛出内部可识别错误，由查询结果返回稳定原因码。
- After：`model-observability-query-truth-integrity.test.ts` 的复杂度上限用例通过；超限返回稳定 `date_bucket_too_complex`，不再生成外观正常的错误分组。
- Remaining limitation：超过支持复杂度的查询会明确失败，不伪造结果。

## AR-04 — terminalStatus 多选错误使用 AND

- Severity：P1
- 状态：FIXED
- Before：具体状态与 incomplete 各自加入顶层条件；`error + incomplete` 要求同一行同时满足互斥条件。
- Failing test：`model-observability-query-truth-integrity.test.ts` 已 RED（`error+incomplete` 当前返回空集）；后续补 Calls、Trace 选择、Aggregate、Export 复用断言。
- Root cause：同字段候选没有先组成 OR 子句。
- Fix：字段内语义谓词用 OR 包裹后只向顶层加入一次；字段间继续 AND；值保持参数绑定。
- After：`model-observability-query-truth-integrity.test.ts` 的终态并集/跨字段交集用例通过；Calls、Trace、Aggregate 与 Export 继续复用同一过滤契约。
- Remaining limitation：无。

## AR-05 — payloadAvailability 多选错误使用 AND

- Severity：P1
- 状态：FIXED
- Before：present/unknown/列状态各自加入顶层条件，多值结果互相排斥。
- Failing test：`model-observability-query-truth-integrity.test.ts` 已 RED（`present+unknown` 当前返回空集）；后续补四个查询消费者复用断言。
- Root cause：同字段语义谓词分散实现，且读侧优先级与 SQL 语义不共享。
- Fix：建立 canonical 语义谓词，同字段 OR、跨字段 AND。
- After：`model-observability-query-truth-integrity.test.ts` 的载荷可用性并集/跨字段交集用例通过，参数继续绑定，没有引入字符串拼接。
- Remaining limitation：无。

## AR-06 — 数据库 NULL 被数字转换为 0

- Severity：P1
- 状态：FIXED
- Before：数字 helper 对所有值调用数字转换，`NULL` 与空字符串变为 0；多个读取字段受影响。
- Failing test：`model-observability-query-truth-integrity.test.ts` 已 RED（数据库 NULL 当前读成 0；真实 0 与 0.001 同时锁定）。
- Root cause：整数、有限数和缺失值共用行为含混的 helper。
- Fix：拆分严格有限数与严格有限整数 helper，先排除 null/undefined/空字符串。
- After：`model-observability-query-truth-integrity.test.ts` 的 `NULL`、真实 0 与小数成本用例通过；损坏用量数字另由 `corrupt` 用例锁定，不再伪装为 0。
- Remaining limitation：损坏的非数值字段表达为 null，并由查询降级状态说明。

## AR-07 — 聚合投影不可用被显示成 0

- Severity：P1
- 状态：FIXED
- Before：v1 无用量表时选择零常量；聚合 DTO 要求 token 必为数字；界面与导出显示真实零。
- Failing test：无调用、10/0、10/5、10/9+1 显式未关联四类聚合，覆盖整体、分组、界面和导出。
- Root cause：聚合契约缺少覆盖状态，数值字段不能为 null。
- Fix：新增 complete/partial/projection_unavailable/unknown 聚合状态；已知和与覆盖度并列，无法计算时数值为 null。
- After：`model-observability-query-truth-integrity.test.ts` 的聚合用例通过，明确区分 `unknown`、`partial` 与 `projection_unavailable`；`model-observability-schema-v2.test.ts` 锁定 v1 投影不可用时 token 为 null。
- Remaining limitation：partial 只表示已知和，不宣称完整总量。

## AR-08 — Data Completeness 读取失败被显示成 0

- Severity：P1
- 状态：FIXED
- Before：SQL 错误、JSON 损坏、非法数值和表不可读都返回四个零。
- Failing test：`model-observability-query-truth-integrity.test.ts` 已 RED（缺 key 无 known 状态；损坏 JSON 待修复后继续执行断言）；后续补 SQL/table unreadable 注入。
- Root cause：所有异常与真正缺 key 共用零兜底。
- Fix：契约增加 known/unknown；只有明确缺 key 按 schema 解释为 0，其他失败返回 null 并使查询健康降级。
- After：`model-observability-query-truth-integrity.test.ts` 的完整性损坏用例通过；缺键为已知零，损坏 JSON 为 `unknown/degraded` 且计数为 null。
- Remaining limitation：数据库完全不可读时只能报告 unknown，不能恢复真实计数。

## AR-09 — 缺用量行被自动标成 not_correlated

- Severity：P1
- 状态：FIXED
- Before：有用量表但无对应行时一律标明确未关联。
- Failing test：`model-observability-query-truth-integrity.test.ts` 已 RED（普通调用缺行当前被标 not_correlated）；显式 MC-03 与 v3 纵向测试待补。
- Root cause：数据库没有显式运行时关联事实，查询用缺行启发式代替事实。
- Fix：由 MC-03 真实接点写闭集 `usageCorrelation=not_correlated`；升级 v3 正式列，其他缺行返回 unknown。
- After：普通缺行用例返回 `unknown`；MC-03 纵向用例从真实压缩边界写入 v3 显式 `not_correlated`，`model-observability-schema-v2.test.ts` 同时覆盖 v1/v2/v3 决策。
- Remaining limitation：旧 v1/v2 行没有该事实，只能 unknown，禁止回填猜测。

## AR-10 — Trace 筛选截断 Trace 统计

- Severity：P1
- 状态：FIXED
- Before：筛选直接作用于参与聚合的调用连接，命中一个 provider 后调用数被缩成 1。
- Failing test：`model-observability-query-truth-integrity.test.ts` 已 RED（T1 当前 callCount=1、ok=1、error=0，而真值为 3/2/1）。
- Root cause：Trace 选择集合和 Trace 内完整统计集合共用同一个已过滤别名。
- Fix：用 EXISTS/候选 CTE 只选择 Trace，再连接完整调用集合统计。
- After：`model-observability-query-truth-integrity.test.ts` 的 Trace 过滤用例通过：过滤只选择调用链，3 个调用的 2 成功/1 失败统计保持完整。
- Remaining limitation：无。

## AR-11 — 部分载荷丢失被 present 掩盖

- Severity：P1
- 状态：FIXED
- Before：只要存在任一载荷记录，读侧立即返回 present；写侧序列化 drop 只记全局计数。
- Failing test：`model-observability-query-truth-integrity.test.ts` 已 RED（已有 payload row 时 dropped 当前映射为 present）；三条纵向 UI/Export 用例待补。
- Root cause：状态优先级和记录存在性顺序错误；序列化失败未留下逐调用事实。
- Fix：canonical 优先级 dropped > expired > not_captured > present > unknown；插入失败把 callId 标 dropped。
- After：部分正文已保存且另有丢失时，查询保持 `dropped` 且仍可查看已有正文；持久化真值测试同时证明序列化丢弃留下逐调用状态，导出保留该语义。
- Remaining limitation：dropped 表示预期载荷至少部分丢失，不等于没有任何可查看载荷。

## AR-12 — Blob 数据库重试会以空字节覆盖成功文件

- Severity：P1
- 状态：FIXED
- Before：文件成功写入后暂存字节被清空；事务首次失败时整个提交函数重跑，第二次写入 0 字节。
- Failing test：确定性 1MB pattern；首次文件成功、首次事务失败、第二次事务成功；断言文件/元数据/描述/ref 原样，且每个 blobId 文件写一次。
- Root cause：文件阶段与数据库阶段未分离，重试粒度过大。
- Fix：prepare 文件一次得到不可变回执；数据库事务可独立重试；成功后才释放字节。
- After：`model-observability-persistence-truth-integrity.test.ts` 首次事务回滚/第二次成功用例通过；1MB pattern 文件只写一次，字节、元数据、描述和引用一致。
- Remaining limitation：数据库最终失败后的孤儿文件由既有宽限期恢复处理。

## AR-13 — 失败 Blob 可产生悬空引用

- Severity：P1
- 状态：FIXED
- Before：失败标识在失败判断前已加入引用数组；描述虽删除标识，引用仍可能插入。
- Failing test：文件写失败后检查描述为 store_failed 且无 blobId，并执行全表 LEFT JOIN invariant，悬空引用必须为 0。
- Root cause：收集的是“出现过的 staged 标识”，不是“已持久的标识”。
- Fix：只收集 durableBlobIds，引用写入前再次确认准备回执/持久元数据。
- After：文件写失败用例通过：描述降级为 `store_failed` 且不保留标识，全表引用完整性检查为 0 个悬空引用。
- Remaining limitation：无。

## AR-14 — 回滚会重复增加内存健康计数

- Severity：P1
- 状态：FIXED
- Before：事务回调直接修改 JavaScript 健康对象，SQLite 回滚无法撤销；重试再次累计。
- Failing test：首次事务在处理载荷后失败、第二次成功；真实 drop 只计一次；最终失败、待补记与 close flush 另测。
- Root cause：数据库事务副作用与进程内副作用未隔离。
- Fix：事务只产生局部 delta；成功后应用一次；最终失败另生成 health dirty receipt，由 timer/maintenance/close 补记。
- After：回滚重试只累计一次真实丢失；连续失败保持运行时降级，并在无新业务事件的 `close` 中补记持久健康回执。
- Remaining limitation：数据库永久不可写时只保留运行时 degraded，不能伪造持久成功。

## AR-15 — 在途调用被动态重配截断

- Severity：P1
- 状态：FIXED
- Before：引擎先关闭旧句柄再安装新句柄；已开始调用固定持有旧接收者，晚到响应/终态被 closed 丢弃。
- Failing test：四类中途切换（元数据→载荷、载荷→元数据、启用→禁用、策略甲→乙），延迟 Provider；最终恰好一条完整调用，无重复事件/载荷。
- Root cause：持久化生命周期按设置切换，而非按调用代际切换。
- Fix：Generation + Drain；旧代先退出全局入口、继续接收已绑定调用，活跃集合归零后关闭；设置只影响新调用。
- After：`model-observability-generation.test.ts` 七个用例通过，覆盖四类在途切换、排空超时、真实设置入口和延迟 Provider；旧调用完整落旧代，新调用按新策略处理，无截断或重复。
- Remaining limitation：永不结束的调用在有界退役超时后按现有 incomplete/crash 语义收口，报告 BOUNDED。

## AR-16 — Blob Externalizer 未恢复先前注册对象

- Severity：P2
- 状态：FIXED
- Before：安装前旧对象被硬编码为空，关闭时无所有权检查。
- Failing test：A→安装 B→关闭 B 恢复 A；A→B→外部 C→关闭 B 保持 C。
- Root cause：未读取 registry 当前值，卸载实现与 observer/sink 不一致。
- Fix：安装前读取 prior；仅当前仍为 B 时恢复 prior。
- After：`model-observability-persistence.test.ts` 的两种所有权用例通过：关闭自身时恢复先前对象，已有后来接管者时保持后来对象。
- Remaining limitation：无。

## AR-17 — BlobStore 内部信任数据库 relative_path

- Severity：P1
- 状态：FIXED
- Before：内部读、删、缺失恢复直接拼接数据库路径，包含性只限制到整个观测目录。
- Failing test：把 relative_path 篡改为 `observability.sqlite` 后执行读/删/恢复；数据库文件必须原样，非法 blobId 也不得触盘。
- Root cause：历史元数据被当作文件系统权限凭证。
- Fix：校验 blobId，重算 Blob 根目录内 canonical/legacy 路径；数据库路径仅作历史信息。
- After：`model-observability-blob.test.ts` 的数据库路径篡改与历史布局用例通过；读、删、缺失恢复只使用合法标识推导出的受控候选路径。
- Remaining limitation：历史路径只兼容已知旧 scheme，不接受任意数据库值。

## AR-18 — Blob 分片树实际恒为同一目录

- Severity：P2
- 状态：FIXED
- Before：标识统一以 `mb_` 开头，取前两位分片恒为 `mb`。
- Failing test：不同随机 token 产生不同分片；旧 `blobs/mb/` 文件仍可读、可安全删除。
- Root cause：分片选择了固定前缀而不是随机部分。
- Fix：新文件使用随机 token 前两位；受控 legacy lookup 保持历史兼容。
- After：新文件按随机 token 分散到真实分片；`blobs/mb/` 历史文件仍可安全读取和清理，且不会信任任意数据库路径。
- Remaining limitation：旧文件不会主动搬迁，仍可能集中在 legacy 目录。

## AR-19 — Blob GET 同步读取整个 64MB 文件

- Severity：P1
- 状态：FIXED
- Before：查询服务 `readFileSync` 返回整块 Buffer，GET 期间会阻塞事件循环并产生额外复制。
- Failing test：64MB GET 并发 timer 能持续推进；HEAD 只 stat；长度、媒体类型、安全响应头、missing/traversal/LOCAL_ONLY 反向回归。
- Root cause：查询 DTO 把文件内容建模为 Buffer，而非验证后的服务端文件描述/流。
- Fix：查询只做精确标识、数据库状态、canonical path 与 stat；GET 使用文件流桥接 Web stream，HEAD 不打开内容流。
- After：`model-observability-e2e-security-blob.test.ts` 的 64MB 用例通过：HEAD 只做元数据检查，GET 以流读取且并发计时器继续推进；本地访问、标识校验和安全响应头回归通过。
- Remaining limitation：仍受 64MB 写入上限约束；不访问 URL 或任意本地路径。

## AR-20 — 发布证据与真实完成度不闭合

- Severity：P1
- 状态：FIXED
- Before：旧报告声称 P0/P1 归零和可发布，但进度清单仍有独立场景未执行，跨平台明确未执行，且本轮发现多项 P1。
- Failing test / oracle：报告矩阵与具体测试文件、用例名、命令退出码、CI run/head SHA 逐项对照；seal guard 验证提交对象与允许差异。
- Root cause：历史结论把后续/局部测试和未执行项外推成完整验收。
- Fix：保留旧报告并标注由 Phase 10.1 取代；新建 V2 十类四态矩阵；未执行与有边界项绝不升级。
- After：旧报告已加 `SUPERSEDED BY PHASE 10.1` 提示，新建 V2 十矩阵并逐项链接本地证据；Windows/Linux、远端流水线与新提交封印保持 `NOT_EXECUTED`，不再外推成发布完成。
- Remaining limitation：本机不能执行的平台只通过真实远程 CI 证明；没有流水线则保持 NOT_EXECUTED。

## 当前总计

| Severity | OPEN | FIXED | BOUNDED | NOT_EXECUTED |
|---|---:|---:|---:|---:|
| P0 | 0 | 0 | 0 | 0 |
| P1 | 0 | 18 | 0 | 0 |
| P2 | 0 | 2 | 0 | 0 |

> 该总计只表示当前工作树内 AR-01～AR-20 的代码与本地证据状态。是否可发布仍由 V2 的远端跨平台流水线和源码提交封印两个独立阻断项决定；它们未执行，因此不能据此声明发布完成。
