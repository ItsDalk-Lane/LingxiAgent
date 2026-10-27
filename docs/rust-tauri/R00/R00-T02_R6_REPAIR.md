# R00-T02 第六轮修复候选：分支、最终消费者与 UI 动作

**状态：修复候选，交未参与修复者独立复审；本报告不判 R00-A03/A04 或 R00-T02 PASS。** Task Base/HEAD 均为 `16aeb380d58d68ff1a38bb46f5cc5d18f985f084`，分支 `codex/rust-tauri-migration`。本轮没有修改产品代码、总控账本、任务书、R1—R5 独立报告和根因报告，没有提交或推送。原工作区的旧任务书删除、新任务书和总控账本修改保留原样。

## R5-F01：新增选择值不能靠同步两层摘要放行

`R00-T02_BRANCH_SEMANTIC_CONTRACT.json` 是人工逐值契约，列 `ask_user` 从 `decision?.action` 得到的 `confirmed`、`timeout`、`aborted` 和兜底 `dismissed`，逐值记录源码条件、当前可达来源、同效/分裂裁决、工具结果标志与现有子场景断言。`r00_t02_source_gates.py` 另从现役 `ask-user-tool.ts` 的赋值、显式比较、switch case、工具返回标志和默认返回独立抽取实际集合，再与契约双向比较；统一审查行还冻结 `ConfirmStore` 与确认路由上游。现行 HTTP 只允许 `confirmed/rejected`，不能因此把未来工具内新分支自动判不可达。新增 `skipped` 时先报新分支，后续须给上游可达或不可达证据、结果与场景。

完整 `build()` 内存反例在新增 `skipped → toolError` 后**同步了逐叶源码摘要、统一行源码摘要及逐叶记录摘要**，保持旧人工契约和旧断言，现报 `ask_user 源码分支与人工语义契约差集: 新增=['skipped']`。原确认 POST、Git、slash、权限与工具负例仍保留。源码抽取只判候选值/结果标志，不自动判不同分支是否同效；该语义须人工裁决。

## R5-F02：从组合根追到最终结果

生成器从 `SettingsContent.tsx` 的页签映射和 `AppPages.tsx` 的实际 JSX 入口，找到每个页面的具体组件，沿本地静态 import/export、hook、store、API 与子组件递归取保守消费闭包。矩阵另列跨前端/服务端与桌面桥的必要文件和每条动作的调用链。每个 UI 统一审查行必须包含**源码抽取闭包、矩阵人工链和全部动作调用链**的文件签名；矩阵自身另核原始文件 SHA、调用点有效性、目标叶的源码交点。26 个 UI 行的最终 `source_scope` 合计与其他入口共享 **1017** 个唯一源码文件。

用量链现可追到 `UsageTab → ModelObservabilitySection → use-observability-query-state → ObservabilityUsagePanel → model-observability-actions → server/routes/model-observability.ts`；成功聚合的 `setAggregate(result)` 供指标卡读取。四张图各自另请求数据，因此 `setAggregate(null)` 影响指标卡，不能断言四图必然清空。完整 `build()` 中只在内存把该赋值改为 `setAggregate(null)`，不改报告及摘要，现报 `ObservabilityUsagePanel.tsx` 源码签名变化。

静态无法穷举的 5 个 UI 动态边界在矩阵逐项列调用点、原因和实施 Task；统一审查行再列未知值、当前人工结论及复审触发条件。供应商、Bridge 与动态 MCP 真实运行结果也按各入口逐条标记 MANUAL，不把静态目录存在写成真实平台通过。

## R5-F03：26 页 161 个动作重建支持关系

只读支持矩阵 `R00-T02_UI_ACTION_MATRIX.json` 覆盖全部 **26** 个 `kind=ui` 页面/面板和 **161** 个自动加载或可见动作，含 **281** 个同动作控件/效果变体；每条记录实际触发、调用链、真实 F-ID/子场景，以及成功、空态或未初始化、失败或无权限的页面结果。当前 **0 个未映射 GAP**、5 个单项 MANUAL。生成器双向核动作目标与 `supported_feature_ids/supported_scenario_ids`，并要求目标源码与调用链相交。把用量页恢复为只支持一个旧导出叶、同时同步统一审查记录摘要的完整 `build()` 内存反例，现报支持关系不一致。

具体修正：用量页支持聚合/洞察、调用台账、轨迹、详情、设置、清理和导出等真实效果；模型页连辅助模型读写/测试及四类媒体默认值；供应商页连供应商配置、凭证/OAuth 登录生命周期；技能页连安装/删除/组合/助手启停/外部目录及 goal/autolearn 偏好；自动化面板连增、删、启停、更新和读取。其余 21 页也逐项列动作，而非沿用任意单一代表叶。所有 UI 叶与补充场景都附 `ui_action_checks`，其断言逐动作保留三类结果。

此前 `/model-observability/health` 被误并入 settings GET，现有独立 `route_behavior` F-ID `F-D22-ROUTE_BEHAVIOR-BEHAVIOR-MODEL-OBSERVABILITY-MODEL-OBSERVABILITY-204546` 和场景 `R00-T02-LA-2045462C0D98`。健康状态、未初始化、无权限和网络失败是页面 bootstrap 结果；与“读取持久化设置”不同。旧 settings F-ID 到新叶的拆分映射保留。Sharing 页旧 `service-connectivity` 错链已删除；配色、宽度、字体、分段上限是独立本地偏好结果，四项默认值、写入与重开读回、不可写边界分别进入该 UI 叶场景。OAuth 的 start/callback/poll 经源码确认是一次登录生命周期的协议步骤，G3 审查与场景现分别断言 URL/设备码/手输码、pending/done/error 和最终登录状态，而不把取得 sessionId 称为登录成功。

本轮按现役事实记录局限：观测设置留存 PUT 和清理删除的 UI `catch` 目前静默，失败不能被写成显示错误 toast；其场景要求不虚报成功、状态未改。观测导出中断仍可能留部分文件。外部供应商、跨平台能力和后续 Rust/Tauri 的真实执行未在本轮验证。

## 清单与验证结果

- 生产登记 **832**，保留叶 **736**（HTTP 491、非 HTTP 245），D01—D24 全覆盖；四项结构差集均为空。736 个补充子场景全部仍为 `SPECIFIED_NOT_EXECUTED`，交 R00-T07 纳账及对应实施 Task 执行。
- 原普通路由 G1/G2/G3 **128/127/127**，原非 HTTP 工具/UI/核心 **111/82/47**、文件操作拆分 **5** 均保留；72 组多方法、文件预览 R09-A09/A10、IPC/slash/别名、A04 现役/撤回分类及旧 F-ID 映射保留。
- `python3 -B docs/rust-tauri/R00/r00_t02_inventory.py --write`、默认复算、`--negative-checks`、`py_compile` 退出码均为 0。三个 R5 完整构建负例及既有确认/Git/slash/文件预览等反例均报预期失败。
- 定向现役 Vitest：**12 文件、95 项通过**，覆盖提问、地图读写、观测设置、设置页、分享本地偏好、OAuth、观测动作/指标/图表、自动化与技能页。它验证当前 TypeScript 路径，不能代替迁移后的 736 个子场景。

**独立复审重点：**从源码反向检查分支契约、最终消费者与 26 页动作矩阵，复算三类同步/错链负例；逐项查看 MANUAL 的结论边界。R5 对 R00-A04 的当前源码静态分类 PASS 可作为输入，本报告不更新独立验收结论。
