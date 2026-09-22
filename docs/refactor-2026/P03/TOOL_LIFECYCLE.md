# P03-T04｜工具发现、禁用、卸载与升级一致性

日期：2026-09-22。生产机制 UNCHANGED_VERIFIED；本阶段补目录隔离（A07）与显式常驻回退（T04-3）测试。

## 1. 常驻与按需（A09）

- 常驻面 = `RESIDENT_CORE_TOOL_NAMES`（read/write/edit/exec_command）+ 桥接三工具（`mcp_search_tools/mcp_describe_tool/mcp_call`）+ 无法安全延迟的回驻工具。其余分类内置、内置插件、MCP 连接器工具全部按需目录化。
- 「四常驻」不等于模型请求只带四个 schema：目录模式下请求还带桥接三工具；全部延迟 schema 留在目录内，经 describe 按需暴露（`tests/on-demand-first-party.test.ts` "keeps only the four Pi primitives on the direct surface"/"exposes the schema through describe without loading anything"；`tests/engine-tool-defer.test.ts` "switches to catalog mode"）。
- 阈值与开关：MCP 源按 `deferEnabled/deferThreshold` 全体一致延迟（不做部分连接器混排）；内置/插件源按 `getBuiltinToolDeferEnabled` 主开关（`tests/engine-tool-defer.test.ts` 阈值/开关/全员/固定四组用例 + `tests/tool-defer-config.test.ts`）。

## 2. 缓存与目录隔离（A07）

- **没有跨会话目录缓存**：`createToolCatalog` 与 deferPlan 都是 `buildTools` 调用内局部对象；会话各持自己的注册表/网关/目录。目录行在装配时冻结。
- 会话 manifest 快照（`buildToolCatalogManifestSnapshot`）记在会话 entry；漂移指纹只覆盖运行期可变来源（MCP+插件名），第一方行不参与（避免应用升级被误报为目录丢失）。
- 新增测试 `tests/on-demand-first-party.test.ts` "isolates per-agent catalogs…"：A/B 两 agent（B 禁用 office）各自构建——B 的目录/清单无 office、注册表 `TARGET_NOT_FOUND`；先 A→B→A 交替查询无串扰；B 即使持有同名 capability 授权也执行 0 次。
- 历史会话不受无关变更破坏：旧会话保留自己的 schema/描述快照（`getLiveToolCatalogNames` 只做增减播报，`tests/session-manifest-delivery.test.ts` catalog change broadcasts）。

## 3. 禁用、卸载与升级失效（A05/A06）

- **审批后禁用**：`invoke` 执行前 `isCurrentlyAvailable` 复核 → `TARGET_DISABLED_FOR_AGENT`/`TARGET_REVOKED`，0 副作用（`tests/tool-invocation-path-parity.test.ts` "revokes %s after approval when availability or generation changes"，direct/deferred/plugin-dev-chat 三路由 × 可用性/代次两场景；`tests/tool-deferred-mcp-parity.test.ts` "Agent 禁用后直接与延迟调用返回同一稳定错误"）。
- **卸载插件**：代次推进 → 旧 prepared/旧调用 `TARGET_REVOKED`，不执行旧对象（`tests/tool-lifecycle-revocation.test.ts` "审批后卸载插件…"）。
- **升级/刷新同名工具**：保留旧描述快照但拒绝执行旧目标；临时断线不推进代次（不算撤销）、用户停机才是传输失败（同文件 "工具列表变化后…"、"临时断线按需可重启…"；`tests/tool-deferred-mcp-parity.test.ts:243`）。
- **回退路径**（T04-3）：
  - 权限契约不可归一（如 resolver 缺失）→ 惰性合成契约：工具照常延迟，**调用期 fail-closed**（`"defers a garbage resolver and fails closed at call time, never at build"`）；合成契约表受 `assertOnDemandCoreToolNamesSound` 约束（每个名字必须指向真实按需工具）。
  - schema 无法安全消费（注册期与 JSON 往返后都抛）→ **显式回退常驻 + warn**，绝不带病延迟（本阶段新增 `"keeps a tool resident with a warning when its schema cannot be consumed safely"`）。该回驻不违反已采纳目录预算：仅单个异常工具回驻，仍受启动断言（`assertAllBuiltInToolsPermissionCovered`/`assertAllToolsCategorized`）约束，无静默新增常驻类别。
  - 目录行 schemaRef 读取失败 → describe 显式提示"参数定义暂不可用…不要当作无参数工具直接调用"（`core/tool-catalog-bridge.ts` describe 分支），不猜 schema。

## 4. 压缩、恢复与模型切换（A10）

- 桥接三工具在**工具前缀**（请求工具面）而非提示词内，压缩不触及；压缩/睡眠后目录清单经 reminder 通道按 `compactionRevision` 重投（`tests/session-manifest-delivery.test.ts` manifest delivery 族 + receipt 字段含 compactionRevision）。
- 压缩或重载后继续调用：describe 随时可用（目录冻结快照），参数契约不猜；授权语义不变（真实目标 capability），无提权。
- 模型切换按当前会话契约重建工具面（session-coordinator 会话重建路径），属 P05/P06 范围，本阶段不改。

## 5. 本阶段改动

- 测试：+2（A07 目录隔离、schema 回驻）+ makeEngine 增加 agentConfig 选项；生产代码零改动（UNCHANGED_VERIFIED）。
- 事实更正记录：曾按注释字面理解"无契约→常驻"；实测确认合成契约为惰性构造（构造期必成功、调用期 fail-closed），真正可达的常驻回退是 schema 不可消费分支——测试与本文按实际行为落笔。
