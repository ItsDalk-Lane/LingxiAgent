# NEXT_STAGE_HANDOFF — P04 输入（P03 → P04，含 P06 工具能力接口）

日期：2026-09-22｜P03 验收基线：见 P03_RESULT.json。

## 1. 已验收坐标与环境

- 工作区 START = END 候选 = `5c131896a`；零生产 commit，全部改动为工作区状态（12 文件清单见 P03_REPORT 实际输入节），等待用户授权提交
- 分支：`docs/knowledge-closeout-2026-09-21`（未切换）；Node v24.16.0 / npm 11.13.0 / darwin arm64；lockfile sha256 `a9735825…`（未变）
- 全量 npm test 终态：14750 绿 / 4 红 = F1 基线（未扩大）；**F2 已关闭（eslint 全仓 0 error）**

## 2. P04 开工前必读（本阶段新建契约）

| 文件 | 对 P04 的用途 |
|---|---|
| docs/refactor-2026/P03/SCHEMA_CONTRACT.md | 工具参数 schema 单一来源与输入上限——P04 若为模型调用引入新参数面（如观测 payload），沿用同一快照/脱敏纪律 |
| docs/refactor-2026/P03/RESULT_CONTRACT.md §3/§4 | MCP 传输超时/401 单次重试语义与网关诊断日志字段——P04 流式请求与观测记录复用同一"稳定 code + 安全归因字段"模式，勿另造日志面 |
| docs/refactor-2026/P03/TOOL_PATH_MATRIX.json | 全部工具执行路径与执行权威——P04 的模型调用观测若需关联工具调用，targetId/generation 是关联键 |
| docs/refactor-2026/P03/EXECUTION_BOUNDARY_AUDIT.md §2 | prepared 绑定事实清单——P04 不得在模型/凭证层再造一份身份绑定 |

## 3. 本阶段建立的门禁（P04 不得削弱）

1. P01/P02 五门禁 + compute-cli-closure + open-boundary-lint 全部复验绿（见 command-log.jsonl）；工具边界白名单 **pluginExecuteTool 已清零**——P04 若需插件执行调试入口，必须先在边界脚本恢复精确白名单并给真实文件，不得绕过。
2. 新增漂移防线：`tests/on-demand-first-party.test.ts` P03-T01 双例（shared 镜像集合 ↔ core classifier 字面相等 + 四模式宽松度 floor）。P04 改权限分类器或延迟契约表必须两侧同改。
3. F2 已关闭：全仓 eslint 0 error 是新基线；run-code-tool 两处行级 `no-control-regex` 豁免是刻意的（ESC/BEL 即匹配目标），勿删。

## 4. P04 主责输入（移交与确认）

- P00 REFACTOR_BACKLOG P04 行：旁路观测完备性核查（embedding/rerank、call-text、summarizeTitle 是否全部经 observed-model-call 包装）+ mc_/ma_/mt_ 铸造回归 + 真供应商验证（BLOCKED 项管理）。
- 本阶段移交：工具网关诊断日志（route/origin/targetId/sourceId/generation/code）是 P04 观测关联的现成事实源；A12 证明传输层无自动重发，P04 的请求重试策略若引入须另行设计并测试。
- F3 网络环境继承：真供应商验证无授权/环境时标 BLOCKED，先完成可独立部分（P00 已核实的观测包装核查与单元/协议替身测试）。

## 5. P06 工具能力接口约定（T08-2 交付）

P06（上下文/提示词/记忆/能力集成）消费工具能力时使用以下接口，不再手写另一份目录：

1. **能力目录读取**：`ToolTargetRegistry.listEligible()/listDeferredCandidates()`（会话持有 `buildTools` 结果中的 `toolTargetRegistry`）；目录清单快照 `toolCatalogManifest`（含 tier/text/fingerprint/names）。
2. **schema 版本**：目标 `parameters`（=注册期冻结的 `validator.schema`，对象同一性由 T02 测试锁定）；MCP 行经 `schemaRef()` 读来源原件但执行校验恒用冻结快照。
3. **失败反馈**：`ToolInvocationError`（稳定 code + details.issues 安全字段路径，无值回显）；错误码表见 docs/architecture/tool-invocation-path-invariance.md。
4. **工具可用性读取**：目标 `isCurrentlyAvailable(runtimeContext)`（运行时复核）与 `availability`（装配时决策）；提醒通道的不可用名单经 session reminder（compactionRevision 收据）。
5. **缓存失效约定**：目录/manifest 为每会话局部对象，无跨会话缓存；插件热装载/MCP 刷新推进 generation 使旧 prepared 失效（TARGET_REVOKED）；漂移播报走 reminder 广播（diffCatalogNames），不重建旧会话目录。

## 6. 已执行验证与遗留

- 已绿：typecheck×3、core-contracts、dependency/tool-invocation/pi-sdk-import 边界、cli-closure、open-boundary-lint、eslint 全仓（F2 关闭）、定向 22 文件 311/311、全量（F1 基线 4 红）。
- 负向验证 2 组留档：镜像漂移注入、真实树 executeCanonical 直调注入（COMBINATION_MATRIX fault_injection_log）。
- BLOCKED（环境，继承）：build:server:open / 四平台 CI / Windows/Linux 实机 / 真实供应商——P08/授权后取回。
- 观察事实（非缺陷登记）：run_code REPL 内核无 OS 沙盒（用户启用、权限审批约束；改写属后续独立决策）。

## 7. 必保留兼容（P04 不可改变）

- P02 交接 §6 全部条目继续有效；本阶段新增：`prepareAndInvokeForLocalDeveloper` 与 route 类型（plugin-dev-http/chat）保留（消费面为测试与未来开发入口，删除属另行决策）；shared/tool-categories.ts 镜像集合导出名（FILE_TOOL_READ_ACTIONS/SESSION_FOLDERS_TOOL_READ_ACTIONS）为测试依赖。
- 审批模式、常驻四工具规则、目录预算语义零变化。

## 8. 工作区卫生提醒（继承）

- 全量 npm test 的 stdout 先落 /tmp 后拷贝（R10-09 快照撕裂）；跑完全量检查 `git status`，若 `artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch` 被重写，取证后 `git checkout --` 还原（本阶段再次发生并已处理，stat 留档 P03-T08-f5-patch-side-effect.stat.txt）。
- 本轮新增未跟踪目录：docs/refactor-2026/P03/、artifacts/refactor-2026/P03/；新增测试并入 6 个既有文件（无新测试文件）。
