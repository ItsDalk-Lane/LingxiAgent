# P03 阶段执行报告｜工具目录、参数契约与统一执行边界

日期：2026-09-22｜执行：ZCode（用户指定阶段执行）

## 结论

**PASS（8/8 任务、15/15 验收场景全部有真实执行证据；本地 darwin arm64）**。既有网关/目录/权限/生命周期机制以 UNCHANGED_VERIFIED 为主收口；本阶段生产改动极小（死白名单条目移除、镜像集合导出、F2 两处 lint 修复、注释更正），新增测试 10 例 + 2 份负向验证闭环。不代表软件无缺陷：F1（治理 4 红）按 P00 定级保留阻塞 P08，F3（网络环境）继承阻塞四平台 CI 证据。

## 实际输入

- START_SHA = END_SHA 候选 = `5c131896a`（工作区改动零 commit，等待用户授权提交）
- 研究基线 `8037fae7a` 与 HEAD 的 P03 相关路径漂移全部有 P01/P02 记录（gateway/tool-categories/boundary 脚本/lease registry 零漂移；tests/tool-invocation-gateway.test.ts +123 行为 P01-A08 补测）
- 分支 `docs/knowledge-closeout-2026-09-21`（未切换）；Node v24.16.0 / npm 11.13.0 / darwin 27 arm64；lockfile sha256 `a9735825…`（未变，与 P02 交接一致）
- 前置：P02 PASS（P02_RESULT.json），其交接五份契约文件全部读取并复用

## 任务逐项结果

| 任务 | 实现 | 生产入口与源码 | 测试与日志 | 旧路径去向 | 状态 |
|---|---|---|---|---|---|
| T01 目标与路径矩阵 | MODIFIED（导出镜像集合+更正注释；机制 UNCHANGED_VERIFIED） | core/session-permission-mode.ts（导出 FILE_READ_ACTIONS、新增 SESSION_FOLDERS_READ_ACTIONS 常量——行为等价）；shared/tool-categories.ts（导出两个镜像集合、注释指向真实符号） | tests/on-demand-first-party.test.ts +2 例（字面相等+四模式宽松度 floor）；负向验证：注入漂移→2 例红→还原绿（logs P03-T01-*） | TOOL_PATH_MATRIX.json 全矩阵+重复规则清单+已正确项标记 | PASS |
| T02 schema 单一来源 | UNCHANGED_VERIFIED + 补测 | lib/tools/invocation/schema-validator.ts（注册即冻结快照；engine 三处 target.parameters===validator.schema） | tests/engine-tool-defer.test.ts +3 例（对象同一性/目录 describe 同源/装配后改写来源不扩执行契约）；tool-schema-validator +1 例（深度/数量上限） | 无旧 schema 拷贝路径（三处本就同源，测试锁定） | PASS |
| T03 集中执行与授权绑定 | MODIFIED（仅删死白名单条目）；prepared/审计字段 UNCHANGED_VERIFIED | scripts/check-tool-invocation-boundaries.mjs（pluginExecuteTool 白名单→空：原 core/plugin-dev-service.ts 已于 04f90d2b2 删除） | 门禁 2244 文件 0 违规 + boundary 测试 5/5（白名单断言同步） | 旁路删除表见 EXECUTION_BOUNDARY_AUDIT.md §3 | PASS |
| T04 发现/禁用/卸载/升级一致 | UNCHANGED_VERIFIED + 补测 | engine._planDeferredToolAssembly（每会话局部目录）；lazy 合成契约调用期 fail-closed | tests/on-demand-first-party.test.ts +2 例（A07 双 agent 目录隔离；schema 不可消费显式回退常驻） | 无跨会话目录缓存（不存在可退出的旧缓存层） | PASS |
| T05 高权限开发与间接入口 | UNCHANGED_VERIFIED + 补测（HTTP 开发路由已删=既有退出事实） | gateway.prepareAndInvokeForLocalDeveloper（现仅测试消费）；真实 HTTP 工具入口=app-tools call（STUDIO_OWNER） | tests/tool-invocation-gateway.test.ts +1 例（createLocalDeveloperPrincipal 拒 6 组伪造 owner）；复用 http-route-security/server-auth/mcp-routes 既有证据 | HTTP plugin-dev 路由：04f90d2b2 已删除（本阶段核实+清残留白名单） | PASS |
| T06 结果/取消/副作用报告 | UNCHANGED_VERIFIED + 补测 | 网关错误外壳/normalizeResult/EXECUTION_CANCELLED；MCP 传输超时单发 | tests/mcp-http-client.test.ts +1 例（A12 超时不重发：替身计数恰 1、不触发刷新、会话拆≠调用重放） | 无旧结果路径 | PASS |
| T07 组合测试 | 组合矩阵+真实树故障注入（无生产改动） | 全部真实 fixture（真实 engine/McpManager/gateway） | 定向 22 文件 311/311；注入 executeCanonical 直调→门禁 exit1+测试红→还原双绿零残留 | COMBINATION_MATRIX.json（可达/N/A 全标注） | PASS |
| T08 旧路径退出+P06 接口 | MODIFIED（F2 修复）+ 交接 | lib/tools/run-code-tool.ts（ANSI 剥除两处行级 eslint 豁免，仓库先例 tests/cli-data.test.ts:67）；lib/tools/security-scan-tool.ts（恒真条件显式化） | 两工具回归 18/18；全仓 lint 0 error（F2 关闭） | 见 §接线和旧路径 | PASS |

## 场景逐项结果

A01–A15 全部 PASS，逐条映射（测试文件/用例名/生产入口/命令/证据）见 [ACCEPTANCE_MAP.json](ACCEPTANCE_MAP.json)。安全关键场景（A02/A03/A04/A05/A06/A07/A08/A11/A15）全部展开到实际可达路径并断言执行次数 0/文件未变/替身计数未增；两处 N/A（生产 plugin-dev-chat HTTP 端点、真实远程供应商执行）附不可达/环境证据。

## 接线和旧路径

- **删除**：AST 白名单死条目 `pluginExecuteTool:["core/plugin-dev-service.ts"]`（文件 04f90d2b2 已删；任何生产调用现在都违规）。
- **既有退出核实**：HTTP plugin-dev 路由、桥接 builtinCall/mcpCall 原始引用、engine deferred 原始映射——均无生产消费者。
- **无双写**：schema 三处消费同一对象（对象同一性测试）；executeCanonical 唯一调用点（AST）；无第二取消通道（P02 复验延续）。
- **无测试专用新模块**：所有新用例并入既有测试文件（遵守 P02 交接 §3 spawn 安全范式）。

## 验证

命令全记录：`artifacts/refactor-2026/P03/logs/command-log.jsonl`（40 条 = 原执行期 34 + 复验收修复轮 6；初版本节误记 28 条，已按实测行数更正——见文末复验收修复轮 #2）。既有 34 条为弱格式（argv/exit/status/stdout_path/reason，无时间戳与摘要——登记为 #4，不重建不改写）；修复轮 6 条起为与 P00/P02 同构的全格式（tools/run-logged.mjs）。关键终态：

| 检查 | exit | 结果 |
|---|---|---|
| typecheck ×3 | 0 | 绿（一轮测试类型修正迭代留档） |
| core-contracts / dependency-boundaries / pi-sdk-import(3/3) / tool-invocation-boundaries(2244) | 0 | 全绿 |
| compute-cli-closure / lint:boundary | 0 | 绿（无新生产文件，ratchet 不变） |
| eslint 全仓 | 0 | **0 error——F2 关闭**（29 警告=既有） |
| 定向 22 文件 | 0 | 311/311 |
| 全量 npm test | 1 | 14750 绿 / **4 红 = F1 基线**（seal×1+round2×2+round3×1；与 P00 登记一致未扩大；+10 例为本阶段新增） |

环境受阻：F3 网络继承（build:server:open / 四平台 CI / 真实供应商——P04/P08 取回）。测试全部使用合成数据与临时目录（mkdtemp），未触及真实用户 HOME/会话/凭证。

## 数据、权限与平台

- 零数据格式改动；零迁移。
- 权限面：未放宽任何授权语义（T01 floor 测试显式锁定"延迟永不宽于直载"；审批模式零改动）。
- 实际验证平台：仅本地 darwin arm64；不代替其他平台/正式打包/真实供应商。

## 差异与限制

1. F1（治理 4 红）与 F3（网络）按 P00 定级保留：F1 阻塞 P08 发布门禁、F3 阻塞平台矩阵证据——均非本阶段验收项。
2. run_code REPL 内核以用户会话身份运行（终端管理器直启，无 OS 沙盒）：既有设计事实，本阶段登记为观察（DEVELOPER_ROUTE_SECURITY §2），未改写（任务书 §3 禁止重写工具业务实现）；如需内核级隔离属后续独立决策。
3. T04 事实更正：注释所写"无契约→常驻"实际不可达（合成契约为惰性构造、调用期 fail-closed）；可达的常驻回退是 schema 不可消费分支——已按实际行为落测试与文档（TOOL_LIFECYCLE §5）。
4. A08 的"实际 HTTP 开发入口"已不存在：场景在现存三层（认证/路由/网关）闭合，映射理由见 ACCEPTANCE_MAP。

## 回退与下一阶段

回退步骤（按序）：①`git checkout -- core/ lib/ scripts/ shared/ tests/ docs/architecture/`（12 文件，见 git status；全部为工作区改动未提交）；②删除未跟踪 docs/refactor-2026/P03/ 与 artifacts/refactor-2026/P03/；③若已授权提交则 revert 对应 commit。无数据需要回滚（零持久化改动）。旧 prepared/目录 generation 无需失效动作（本阶段未改任何注册/生命周期语义）。

下一阶段交接：[NEXT_STAGE_HANDOFF.md](NEXT_STAGE_HANDOFF.md)（P04 输入 + P06 工具能力接口约定）。

## 复验收修复轮（2026-09-22，全阶段重验收发现，四项）

独立复验收（P00–P03 全量重验）对本阶段提出 4 项发现，全部以最小改动闭合：

| # | 发现 | 修复 | 复验 |
|---|---|---|---|
| 1 | **EVIDENCE_SHA256.txt 双重缺陷**：条目混用两种路径基准（docs 相对裸名 + 仓库根相对），任何单一 cwd 下 `shasum -c` 都无法全验；且清单内 command-log.jsonl 哈希在清单生成后又有条目追加而陈旧 | 统一为仓库根相对路径；清单改为**证据链最终步**（在全部日志追加完成后最后重生成，日志哈希随清单版本固定；后续任何追加须同步再生成清单） | 重生成后从仓库根单次 `shasum -c` 全过（含修复轮 6 条新日志与 tools/run-logged.mjs） |
| 2 | **报告/RESULT 命令计数失实**：两处写"28 条"，实测 34 条（同 P00 曾出现的计数类缺陷） | 按实测更正为 40（34+修复轮 6）并在验证节标注初版误记 | `wc -l` 实测一致 |
| 3 | **EXECUTION_BOUNDARY_AUDIT 未声明静态分析边界**："executeCanonical 唯一调用点（AST）"未限定为成员调用形态；别名/脱钩调用、engine 定义面、测试面引用不在规则匹配面 | 审计文档新增 §5 静态边界声明（含可利用性评估与升级条件） | 文档审查 |
| 4 | **command-log 条目格式退化**：既有条目缺时间戳/摘要/cwd/source_sha（低于 P01 修复轮确立的 P00 同构标准；P01 曾因同类退化被判 FAIL） | 不重建不改写历史条目（如实登记）；新增 tools/run-logged.mjs（P00 同构），修复轮起新条目全格式 | 修复轮 6 条含完整字段且摘要与文件实测一致 |

修复轮证据（P03-FIXR1-*，6 条全格式）：依赖边界门禁 2026 文件 0 误报（含模板折叠加固）；检查器自测 7/7（新含 electron-template/sdk-template/runtime-template 三反例）；p02-recovery 主检出 3/3 + 独立 worktree 3/3（crash-probe 可移植性）；定向 10 文件复跑 148/148。涉及跨阶段修复（P01 检查器与账本、P02 清单与探针）的证据统一记于本阶段日志，P02 自身 command-log 零追加以保其清单哈希稳定。
