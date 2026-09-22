# P02_REPORT — 运行身份、生命周期、取消与恢复

日期：2026-09-22｜阶段：P02｜执行分支：`docs/knowledge-closeout-2026-09-21`（未切换）。

## 0. 结论

**P02 状态：PASS**（15/15 验收场景全部有真实执行证据；任务 T01-T08 全部完成）。核心改动集中在 P00 所有权图唯一的 🔴 主责缺口（taskId 无统一铸造厂 + attempt 语义缺失），其余子系统经逐项核对为 UNCHANGED_VERIFIED，只补齐证据测试与规格文档。未新增平行 Agent 循环/工具网关/任务总管/凭证体系；未改变用户采纳的权限、人格、MOOD、观测分组、知识来源、文件交付与历史行为。

## 1. 坐标与环境

- 研究基线（任务书）：`8037fae7a6f621549f5b7db4c4d0bba3862ddc07`
- START_SHA = `c3cd52859`（P01 交接后 HEAD：P01 生产提交 `e2392f301` + 2 条 docs 提交，均已获用户授权）
- END_SHA：见 P02_RESULT.json（本轮零 commit，全部为工作区状态，等待授权提交）
- Node v24.16.0；npm 11.13.0；darwin 27.0 arm64；lockfile sha256 `a9735825…`（未变）
- 基线漂移映射：研究 SHA→START 仅 docs/收尾提交（P00-A02 已判定无实现漂移）+ P01 生产提交（其验收即 P01 阶段本体）；对 P02 无未消化影响，行号引用已按当前源码复核。

## 2. 任务执行摘要（逐项状态）

| 任务 | 动作 | 生产改动 | 状态 |
|---|---|---|---|
| T01 身份契约 | MODIFIED | shared/identity-brands.ts（TaskId 品牌+守卫）、lib/tasks/task-identity.ts（新统一铸造厂）、lib/task-registry.ts（attempt 栅栏）、server/task-bus-handlers.ts（attempt 透传）、5 处自铸替换（subagent×1 定义 2 调用点/workflow/rewind/speech×4/media createTaskId）；IDENTITY_CONTRACT.md | PASS |
| T02 状态迁移 | UNCHANGED_VERIFIED（runId/task 状态机零重构）+ 补测 | 零；STATE_TRANSITIONS.md（状态/事件表 R1-R8/T1-T9 全测试映射） | PASS |
| T03 取消与清理 | MODIFIED（最小） | core/session-teardown.ts（返回收集的清理 errors，不改抛错行为）、hub/agent-executor.ts（并入 AggregateError）；CANCELLATION_MAP.md | PASS |
| T04 并发边界 | UNCHANGED_VERIFIED + 补测 | 零；CONCURRENCY_RULES.md（可变当前值审计/容量上限清单） | PASS |
| T05 崩溃恢复 | UNCHANGED_VERIFIED（零持久化结构改动）+ 真实进程证据 | 零；RECOVERY_MATRIX.md、artifacts/.../tools/crash-probe-child.mjs（测试探针，非生产） | PASS |
| T06 入口接通 | UNCHANGED_VERIFIED + 补测 | 零；ENTRY_EXECUTION_MATRIX.md（8 入口 × 正常/取消两类证据） | PASS |
| T07 用户可见结果 | UNCHANGED_VERIFIED | 零（history-run-outcome-edges 既有 6 例即 A13/A14 证据；中间失败不覆盖最终成功/部分正文失败不被掩盖均在其内） | PASS |
| T08 验收清理 | 本报告 + RESULT/ACCEPTANCE_MAP/HANDOFF | — | PASS |

## 3. 关键实现决定

1. **taskId 统一铸造**：新格式 `task_{kind}_{ts36}_{seq36}_{rand6}`，`task_` 前缀与全部旧格式不重叠；旧记录原值读、不迁移（IDENTITY_CONTRACT §4）。替换点零格式消费者受损（grep 复核无前缀解析；3 处测试断言更新为新前缀）。
2. **attempt 栅栏而非禁止重注册**：register 在终态后 = 合法复用（attempt+1 清零终态）；complete/fail/update 可选 expectedAttempt，stale 返回 null 不落盘；终态 first-write-wins。与仓库既有媒体域栅栏（mediaTaskAttempt/settleTask、deferred checkAttempt）语义同构，全链路（task 域/媒体域/交付域）三类迟到回调防护现已齐备。
3. **teardownSessionResources 返回 errors 而非抛错**：其余 8 处调用方（session-coordinator 5 + bridge-session-manager 2 + runAgentPhoneSession 1）行为零变化（显式 warn-only 账本登记于 CANCELLATION_MAP §5，验收修复轮补全为全量 9 处）；只有 runAgentSession 把清理失败并入 AggregateError（A12）。
4. **A15 放进 composition 测试文件**：新文件首次 Write 被安全钩子误判命令注入（固定 argv spawn，P01 已两次登记同类误报），按 P01 先例并入既有 `tests/server-composition-boundary.test.ts` Part 5，零孤立残留。

## 4. 验证与命令日志

全部命令经 `artifacts/refactor-2026/P02/tools/run-logged.mjs` 记录（命令行/cwd/exit code/stdout/stderr 落 `artifacts/refactor-2026/P02/logs/`，索引 command-log.jsonl）。摘要：

| 命令 ID | 内容 | exit |
|---|---|---|
| P02-T08-typecheck / -r2 / -r3 | npm run typecheck（tsc×3） | 2（测试类型错，已修）→2→**0** |
| P02-T08-core-contracts | typecheck:core-contracts（含改动后的 identity-brands） | **0** |
| P02-T08-tool-invocation-boundaries | check:tool-invocation-boundaries | **0** |
| P02-T08-dependency-boundaries | check:dependency-boundaries（P01 门禁未削弱） | **0** |
| P02-T08-pi-sdk-import-boundary | tests/pi-sdk-import-boundary.test.ts | **0** |
| P02-T08-compute-cli-closure | 三件套基线再生成（task-identity 入闭包图） | **0** |
| P02-T08-contract-tests | contract-tests+cli-closure-census | 1（open-boundary 违例：新文件未入 manifest）|
| P02-T08-open-boundary-lint-r2 | manifest 补入 lib/tasks/task-identity.ts 后复跑 | **0** |
| P02-T08-taskbook-targeted | 任务书 §7：teardown+history-run-outcome-edges | **0** |
| P02-T08-new-p02-tests | 全部新增测试+受影响回归 17 文件 | **0**（171/171） |
| P02-T08-composition-full | composition 全文件（A15+纵向链） | **0**（10/10） |
| P02-T08-full-suite | npm test 全量 | 见 RESULT（首次失败迭代保留日志） |

全量 stdout 按 P01 交接 §7 先落 /tmp（防 R10-09 快照撕裂），完成后拷贝入证据目录。

## 5. 变更/保留/删除清单

**生产修改（11 文件）**：shared/identity-brands.ts、lib/tasks/task-identity.ts（新）、lib/task-registry.ts、server/task-bus-handlers.ts、lib/tools/subagent-tool.ts、lib/tools/workflow-tool.ts、lib/tools/rewind-tool.ts、core/media-adapters/speech.ts、core/media/image-task-runner.ts、core/session-teardown.ts、hub/agent-executor.ts。
**基线再生成（1）**：build/cli-runtime-closure.json（task-identity 入图）；export-manifest.json 补 1 行（新文件入白名单）。open-boundary-baseline.json 无变化。
**测试（7 文件改动/新增）**：新 tests/task-identity-attempts.test.ts、p02-run-finalize-edges.test.ts、p02-cancellation-edges.test.ts、p02-concurrency-edges.test.ts、p02-recovery-restart.test.ts、p02-entry-lifecycle-edges.test.ts；改 tests/agent-executor-teardown.test.ts（+A12）、server-composition-boundary.test.ts（+Part 5）、subagent-tool.test.ts / workflow-tool.test.ts（taskId 前缀断言）。
**删除**：无（旧 taskId 自铸路径由 mint 调用就地替换，无双写期；旧格式数据无迁移）。
**文档（docs/refactor-2026/P02/）**：IDENTITY_CONTRACT.md、STATE_TRANSITIONS.md、CANCELLATION_MAP.md、CONCURRENCY_RULES.md、RECOVERY_MATRIX.md、ENTRY_EXECUTION_MATRIX.md、ACCEPTANCE_MAP.json、P02_RESULT.json、本报告、NEXT_STAGE_HANDOFF.md。

## 6. 旧路径去向

- 5 类 taskId 调用方自铸：**全部收口**至 mintTaskId（P00 所有权图枚举的 TaskRegistry-facing 自铸零残留；无第二 `task_` 格式铸造点）。会话 fork 克隆路径另有三处 store 域自铸克隆 ID（`media-fork-`/`subagent-fork-run-`/`workflow-fork-run-`，randomUUID、不进 TaskRegistry 铸造面），为显式登记例外而非遗漏，见 IDENTITY_CONTRACT §4.5（验收修复轮修正本节原"零残留自铸格式生产代码"的过强表述）。
- teardownSessionResources 吞错路径：runAgentSession 侧升级为显式上报；其余调用方显式登记 warn-only 账本（非静默——有 warn 日志与文档）。
- TaskRegistry 无 attempt 的旧回调路径：保留（向后兼容，不带 expectedAttempt 不栅栏）；总线与调用方按需启用。
- 新旧双写检查：attempt 字段新增即唯一事实（无第二登记处）；task:register 返回值扩展为加字段（原 ok:true 消费者不受影响，媒体/回压调用方已核实不读返回值）。

## 7. 未完成/BLOCKED

| 项 | 原因 | 处理 |
|---|---|---|
| 无阶段内 BLOCKED 项 | 本阶段验收全部为本地可执行（真实子进程/真实 server 均本地达成） | — |
| （继承，非本阶段）build:server:open / 四平台 CI | F3 网络环境（P01 登记） | P08 处理 |
| （继承，非本阶段）F1 seal 4 例 / F2 lint 3 error | 治理流程 / 主责 P03 | 状态不变 |

## 8. 回退步骤

1. 停止本阶段拥有的执行：无常驻进程（全部测试进程已退出；无服务变更）。
2. 代码回退：`git checkout -- <上述生产/测试/基线文件>` 并删除新增文件（lib/tasks/、tests/p02-*.test.ts、tests/task-identity-attempts.test.ts、docs+artifacts/refactor-2026/P02/）。零 commit，回退即恢复 START 工作区。
3. 持久化字段回退条件：新增 attempt 字段仅写入 plugin-tasks.json（追加字段）。旧版本确认可读（load 忽略未知字段）且不会重放副作用（恢复只置 recovering，不执行）；满足任务书 §9 的回退前提。若选择旧版本快照回滚，切换后新增记录不得被旧快照覆盖（P01 既有规则）。
4. 禁止用旧快照覆盖切换后的新增用户记录。

## 9. 安全/数据/功能缺口声明

- 安全：无权限面变化（取消链全部走既有入口与登记处）；A06 证明不按进程名杀用户进程。
- 数据：零持久化结构变更（无 data epoch/迁移）；attempt 为追加字段且旧记录兼容已测。
- 功能：无用户可见行为变化（唯一外部可观察差异=新铸造 taskId 的字符串格式；断连宽限/审批取消/重启语义均为既有行为的固化测试）。
- 「界面已停但后台继续」：A04/A06/A15 分别从终态幂等、进程树退出、端口释放三面证明不存在该情况被报告为完成。

## 10. 独立验收修复轮（2026-09-22）

独立验收轮 1 结论 PASS（15/15 场景与 8/8 任务全部独立复现），另提出 3 项低严重度发现，本修复轮全部以**文档更正**闭合（零生产代码改动，验收轮判定均为声明精度/账本完整性问题而非功能缺陷）：

| # | 发现 | 修复 |
|---|---|---|
| 1 | "无第二铸造厂/零残留自铸格式"声明过强：会话 fork 克隆路径仍有 3 种 store 域自铸克隆 ID（task-store.ts:222、subagent-run-store.ts:214/218，randomUUID、P00 未枚举、无格式消费者） | IDENTITY_CONTRACT 新增 §4.5 例外账本登记三处并界定收口范围；本报告 §6、RESULT legacy_paths、HANDOFF §5 同步更正为精确表述 |
| 2 | CANCELLATION_MAP §5 账本只列 coordinator 5 处 warn-only 调用方，漏 bridge-session-manager×2（:1470/:1879）与 runAgentPhoneSession（:721） | 账本补全为全量 9 处调用方（含 1 处已升级的 runAgentSession 与各 warn-only 场景的保留理由） |
| 3 | attempt 栅栏"就绪但在库消费为 0"的状态未显式标注（expectedAttempt 仅总线面，仓库内无 task:complete/fail 生产发送方） | IDENTITY_CONTRACT §3 增加"当前消费状态"标注与 cancel/abort 显式排除边界；P03+ 接线要求指向 HANDOFF §6 |

验收轮 1 的独立复跑证据：typecheck×3/core-contracts/boundaries/pi-sdk 门禁全绿、新增+受影响测试 129/129、assistant-run-lifecycle+history-run-outcome-edges+tripwire 26/26、composition 10/10（含 A15）、全量 npm test 4 红（=F1 基线）/14740 绿，与 §4 命令日志逐位一致；EVIDENCE_SHA256 全部 44 条重算匹配。反例探针（隔离于 /tmp，仓库零污染）证实：重复 complete first-write-wins、迟到栅栏拒绝且不落盘、默认工厂 2 万 ID 零碰撞、非法 expectedAttempt 防御性抛错；同时确认无栅栏迟到终态（向后兼容路径）与迟到 cancel 的行为边界与文档声明一致。未验证边界（Windows/Linux 实机、四平台 CI、真实供应商）维持 F3/环境受限登记，不属本阶段缺陷。

## 11. 复验收修复轮（2026-09-22，全阶段重验收发现，两项，零生产代码改动）

| # | 发现 | 修复 |
|---|---|---|
| 4 | **EVIDENCE_SHA256.txt 自引用条目**：第 51 行列入了清单自身哈希（fb3d22af…），逻辑上永不可匹配，`shasum -c` 恒报 1 失败，削弱清单作为门禁的可用性 | 移除自引用条目并重生成全清单（含本报告更新后的新哈希；command-log.jsonl 本轮零追加、哈希不变；重生成后 56/56 单次全过） |
| 5 | **crash-probe-child.mjs 硬编码绝对路径**：`import … from "/Users/study_superior/Desktop/Code/LingxiAgent/lib/task-registry.ts"` 使 tests/p02-recovery-restart.test.ts 只能在唯一机器路径下通过（任何其他 clone/工作树必红：exit 1≠70） | 改为相对本文件的 `../../../../lib/task-registry.ts`；主检出复跑 3/3 绿 + 独立 worktree 复跑 3/3 绿（可移植性证明；证据记于 P03 阶段日志 P03-FIXR1-*，P02 自身 command-log 不追加以保清单哈希稳定） |
