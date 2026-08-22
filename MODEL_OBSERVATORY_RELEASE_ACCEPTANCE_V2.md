# Model Observatory — Release Acceptance V2（Phase 10.1）

> 本报告取代 `MODEL_OBSERVABILITY_RELEASE_ACCEPTANCE.md` 的当前发布结论，但保留旧报告作为历史证据。
>
> 四态口径：`PASS` = 本轮要求与独立证据闭合；`BOUNDED` = 结果诚实但存在已说明的能力边界；`FAIL` = 已执行且失败；`NOT_EXECUTED` = 没有执行，禁止外推为通过。

## 0. 结论与坐标

| 项 | 结果 |
| --- | --- |
| Phase 10.1 本地代码修复 | **PASS**：AR-01～AR-20 已完成本地修复和回归，当前已知代码 P0=0、P1=0 |
| 当前发布决策 | **NOT_EXECUTED**：不能声明跨平台 production-ready；远端跨平台流水线和本轮源码提交封印均未执行 |
| PHASE10_1_START_SHA / 当前 HEAD | `cda8dbe5931f562a4eef47204ac571d29941035a` / 同值；本轮改动仍在工作树中 |
| Git 拓扑 | detached HEAD；按用户要求未创建或切换分支、未创建新工作树 |
| 本地 main / 远程 main | `e62bb53545c3439cf798a9785f97ada3e1d6a3e1` / `bf3c80b5681c99fc7ff05b9c168898e9ca317587` |
| 旧 VERIFIED_SOURCE_SHA | `3f20b58ebe0648aa4389447913b58642075ff73c`，对象类型为 `commit`，但不覆盖本轮工作树 |
| FUNCTION_COMMIT_SHA | **NOT_EXECUTED**：用户未授权提交或推送 |
| Phase 10.1 audit seal | **NOT_EXECUTED**：没有功能提交就不能伪造新的封印提交 |
| 本机环境 | macOS 26.5.2 arm64；Node 24.16.0；npm 11.13.0 |

## A. MC-01～MC-10 Matrix

| 场景 | 状态 | Phase 10.1 证据与边界 |
| --- | --- | --- |
| MC-01 Pi Chat | PASS | `model-observability-e2e-chat.test.ts`：真实会话、Provider Witness、四层载荷、调用链与毒丸扫描。SDK 尾段来源精度保持 partial，不冒充 exact。 |
| MC-02 AgentRun compaction | BOUNDED | `model-observability-detail-vertical.test.tsx`：真实运行入口，语义输入/响应、15 tokens、Store/Query 一致；上游不暴露 Provider wire，保持 unavailable。 |
| MC-03 native compaction | BOUNDED | 同文件：真实 `isCompacting` 边界、语义请求/响应、调用链与 v3 显式 `not_correlated`；精确用量关联和 Provider wire 结构性不可得。 |
| MC-04 callText 四协议 | PASS | `model-observability-e2e-calltext.test.ts`：四种协议的真实 HTTP、请求正文、映射、用量、持久化、Query 与 HTTP 闭环。 |
| MC-05 Provider probe | PASS | `model-observability-e2e-media-speech.test.ts`：POST 探测记录为调用，GET 模型列表保持控制面零调用；成功响应只记录契约允许的元数据。 |
| MC-06 Image | PASS | 同文件：普通提交与 401 刷新；后者为 1 个逻辑调用、2 次尝试、2 个有序 Provider 请求。 |
| MC-07 external CLI | BOUNDED | `model-observability-e2e-utility.test.ts`：真实外部进程边界，Provider 层保持 OPAQUE，命令参数与输出毒丸不落库。 |
| MC-08 Video | PASS | 同文件：提交是真实调用，轮询是控制面且 0 个新增调用。 |
| MC-09 Speech | PASS | `model-observability-e2e-media-speech.test.ts`：两类协议、音频描述、语言提示、响应和凭证脱敏；无法同步取字节的 Web Blob 诚实保持 externalized。 |
| MC-10 diary direct summary | BOUNDED | `model-observability-e2e-utility.test.ts`：两个临时摘要与终稿共享同一任务调用链；临时摘要的 Provider wire 不可见，保持 unavailable。 |

## B. Usage Correlation Matrix

| 情形 | 状态 | 真值结果 |
| --- | --- | --- |
| 有明确调用标识的普通路径 | PASS | Ledger → v3 usage projection → Call detail/aggregate 保持 `present`，token 与 Witness fixture 一致。 |
| 普通调用缺 usage row | PASS | 返回 `unknown`，不再由缺行猜成 `not_correlated`。 |
| MC-03 native compaction | BOUNDED | 运行时显式写 `usage_correlation_state=not_correlated`；Query 只读取事实，不再启发式推断。 |
| v1 accounting projection 不存在 | PASS | `projection_unavailable`，所有不可计算数字为 null，不显示 0。 |
| 部分覆盖 | PASS | `partial` 与覆盖调用数同时返回；已知和不冒充完整总量。 |
| 数字字段损坏 | PASS | Call、Trace、Aggregate 与 Export 均保留 `corrupt`，不把损坏数字合成 0。 |

## C. Payload Truth Matrix

| 情形 | 状态 | 真值结果 |
| --- | --- | --- |
| FULL / metadata-only 生产路径 | PASS | 运行时可见度原样进入持久化、Query、HTTP 与界面；正文只在详情阶段按需读取。 |
| SDK Provider wire 不可见 | BOUNDED | MC-02/03/10 持久化为 unavailable + null，不升级为空对象或 FULL。 |
| 外部进程 wire | BOUNDED | MC-07 保持 OPAQUE，界面和导出都不升级。 |
| present 与 dropped 同时存在 | PASS | Call 状态以 dropped 为准，但已经成功保存的正文仍可查看；界面显示降级告警。 |
| JSON / provenance 损坏 | PASS | 显式 `corrupt`，不伪装成空正文、无来源或正常类别。 |
| Export | PASS | 默认 metadata-only；显式正文导出也只含受控清洗内容，不含 Blob 字节。 |

## D. Trace Truth Matrix

| 场景 | 状态 | 独立判断 |
| --- | --- | --- |
| simple / multi-turn tool loop | PASS | 真实会话边界；子调用父标识等于根调用标识，不等于工具调用标识。 |
| parallel tools | PASS | 真实工具转换边界；C2、C3 同链且均以 C1 为父，Store、Query、Trace UI 一致。 |
| subagent | PASS | 真实 spawn 工具边界；跨会话子调用与父调用同链，父子边在 Store、Query 与 Trace UI 一致。 |
| detached/background | PASS | 父调用结束后创建的延迟任务不再错误继承旧调用链。 |
| concurrent sessions | PASS | 两个会话并行时调用标识、载荷和用量不串线。 |
| Trace filter | PASS | 过滤只选择调用链；选中后仍以完整调用集合计算数量和终态。 |
| crash/restart | PASS | 无终态的已持久调用恢复为 incomplete + interrupted，不伪造成 Error。 |

## E. Query Truth Matrix

| 语义 | 状态 | 结果 |
| --- | --- | --- |
| terminalStatus 多值 | PASS | 同字段 OR、跨字段 AND，且继续使用参数绑定。 |
| payloadAvailability 多值 | PASS | 同字段 OR、跨字段 AND，Calls/Trace/Aggregate/Export 共享契约。 |
| SQL NULL / 0 / 小数 | PASS | NULL 保持 null；真实 0 与小数成本保持原值。 |
| IANA timeZone / DST | PASS | 洛杉矶历史分桶、窗口中两次切换、固定时差兼容和 UI 纵向传递均通过。 |
| transition cap | PASS | 超出复杂度返回 `date_bucket_too_complex`，不返回错误 200 数据。 |
| pagination / cursor / SQL injection | PASS | 稳定翻页、篡改游标拒绝、维度闭集和参数绑定测试通过。 |
| Trace filtered metrics | PASS | 过滤命中任一调用即可选链，但链内统计不被筛选截断。 |

## F. Missing-State Matrix

| 状态 | 状态 | Runtime → Store → Query → UI → Export |
| --- | --- | --- |
| 真实 0 | PASS | 保持数值 0，与 null/unknown 分开。 |
| NULL / Unknown | PASS | 数字显示破折号或 Unknown，导出为 null/unknown，不改写成 0。 |
| Not Correlated | PASS | 只有明确运行时事实可产生；普通缺行仍是 unknown。 |
| Projection Unavailable | PASS | v1/无投影单独呈现，不生成假 token。 |
| Not Captured / Dropped / Expired | PASS | 闭集优先级一致；dropped 不被已有正文掩盖。 |
| Opaque / Unavailable | BOUNDED | 结构性不可见内容保持 null；这是能力边界，不是失败。 |
| Corrupt | PASS | 正文、来源、类别、用量和完整性损坏均显式降级。 |
| Incomplete | PASS | 无终态调用与 Error 分开，重启事实单独保留。 |

## G. Persistence Retry Matrix

| 场景 | 状态 | 结果 |
| --- | --- | --- |
| DB 首次失败、第二次成功 | PASS | Blob 文件阶段只执行一次；重试只重放数据库阶段。 |
| Blob 原始字节 | PASS | 1MB pattern 在重试后逐字节一致，不被空字节覆盖。 |
| 健康计数 | PASS | 事务局部 delta 仅在成功后应用一次，回滚重试不重复累计。 |
| serialization drop | PASS | 全局计数与逐调用 dropped 状态同时落下。 |
| Blob 写失败 | PASS | 描述降级为 store_failed，无标识、无悬空引用。 |
| 最终事务失败 | PASS | 运行时持续 degraded；无新业务事件时 `close` 仍补记持久失败回执。 |
| 外置器所有权 | PASS | 关闭时恢复先前对象，但不覆盖后来接管者。 |

## H. Dynamic Reconfigure Matrix

| 在途切换 | 状态 | 结果 |
| --- | --- | --- |
| metadata → payload | PASS | 旧调用完整落旧代，新调用落新代，无重复或 incomplete。 |
| payload → metadata | PASS | 旧调用仍保存旧代正文，新调用只写元数据。 |
| enabled → disabled | PASS | 在途旧调用写完；切换后新调用不再入库。 |
| enabled policy A → B | PASS | 两代各自完整，设置只影响明确的新调用集合。 |
| 永不结束的旧调用 | BOUNDED | 退役代使用有界排空超时，避免永久资源泄漏；不会静默宣称已完整排空。 |
| 真实设置入口 + 延迟 Provider | PASS | 生产设置入口使用代际管理；响应期间换代不截断真实 `callText` 生命周期。 |

## I. Security Matrix

| 边界 | 状态 | 结果 |
| --- | --- | --- |
| Payload / Blob 本地访问 | PASS | LOCAL_ONLY；远端 owner 与匿名访问正文/字节均拒绝。 |
| 标识、路径与数据库 relative_path | PASS | 标识白名单、受控目录重算、包含性检查；数据库路径不再拥有文件读删权限。 |
| SSRF / 任意本地文件读取 | PASS | Blob 路径只接受闭集标识，不访问 URL，不接受任意磁盘路径。 |
| XSS / 损坏 JSON | PASS | 界面按文本结构渲染，不执行正文 HTML；损坏内容显示 corrupt。 |
| 凭证与毒丸 | PASS | Witness 能看到真实请求凭证，但 Observer、SQLite、WAL/SHM、Query 与 Export 扫描为零出现。 |
| Observability ON/OFF 等价 | PASS | 开关前后 Provider 请求正文和业务返回一致。 |
| Blob GET / HEAD | PASS | GET 流式读取，HEAD 只检查元数据；安全响应头和 64MB 限制不回归。 |
| SQL injection | PASS | 查询维度闭集，用户值始终参数绑定。 |

## J. Cross-platform Matrix

| 平台/发布环节 | 状态 | 真实执行结果 |
| --- | --- | --- |
| macOS 26.5.2 arm64 | PASS | Node 24.16.0 下 typecheck、全量测试、三个构建和本地 package smoke 均通过。 |
| Windows | NOT_EXECUTED | 本轮工作树没有功能提交/推送，未对本轮树触发远端 Windows 流水线。 |
| Linux | NOT_EXECUTED | 本轮工作树没有功能提交/推送，未对本轮树触发远端 Linux 流水线。 |
| Remote macOS CI | NOT_EXECUTED | 只有本机 macOS 证据，没有本轮远端运行记录。 |
| Electron notarization | NOT_EXECUTED | 本地打包使用 `SKIP_NOTARIZE=true`；不得将目录打包冒充公证完成。 |
| Phase 10.1 source seal | NOT_EXECUTED | 未创建 `FUNCTION_COMMIT_SHA`，旧封印只证明旧提交。 |

Verified platforms：**macOS 26.5.2 arm64（本地）**。没有证据支持写“Windows/Linux 已验证”或“跨平台 production-ready”。

## 11. 本地门禁实录

| 门禁 | 状态 | 结果 |
| --- | --- | --- |
| targeted observability | PASS | 51 files / 449 tests；补充审批、记忆和详情纵向复跑 2 files / 17 tests |
| typecheck | PASS | `npm run typecheck`，三个 TypeScript 配置均 exit 0 |
| lint | PASS | `npm run lint`，0 error；警告不改写为错误或通过数量 |
| lint:boundary | PASS | 1 条既有基线边，未新增 |
| persistence scanner | PASS | 61 stores / 721 sites |
| persistence fingerprint | PASS | `sha256:f4cfa1e85848f7b621a87f5cc638a3d0c9229d71acae9f662e34b438358a3d49` |
| CLI closure | PASS | 10609 files：713 source graph、11 runtime assets、9885 NFT；1 条既有基线边 |
| i18n parity/coverage | PASS | 7 files / 14 tests |
| full npm test | PASS | 1201 files passed / 1 skipped；12135 tests passed / 7 skipped；0 failed；134.40s |
| build:server | PASS | 原生/运行时 smoke 与签名 seed 验证通过；本地临时密钥仅用于构建并已删除 |
| build:server:open | PASS | exit 0 |
| build:client | PASS | exit 0 |
| package smoke | PASS | `dist/mac-arm64/Lingxi.app` 成功生成并完成本地签名验证；notarization 未执行 |

构建过程保留两次真实前置失败：干净依赖状态先缺少界面产物；补建界面后又因没有正式签名密钥而停止。随后只使用临时本地密钥完成构建/打包验证，密钥目录已删除，没有写入仓库或报告。

## 12. 剩余边界与发布阻断项

- MC-02、MC-03、MC-10 的 Provider wire 受上游接口限制，只能诚实标为 unavailable；MC-07 的外部进程 wire 只能标 OPAQUE。
- 浏览器无法提供合法 IANA 时区时仍会回退当前固定时差；这是明确 fallback，不代表完整历史 DST 能力。
- 永不结束的在途调用使用有界排空超时，结果为 BOUNDED。
- 旧 `blobs/mb/` 文件保持兼容但不主动搬迁；新文件已使用真实随机分片。
- `npm ci` 报告 1 low + 7 moderate 依赖审计项；本轮未运行越界的自动或强制升级。
- Windows、Linux、远端 macOS、notarization、功能提交和 Phase 10.1 seal 均为 **NOT_EXECUTED**。在这些项有真实证据前，发布结论仍是“本地修复通过，但不可声明跨平台 production-ready”。
