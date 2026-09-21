# NEXT_STAGE_HANDOFF — P03 输入（P02 → P03）

日期：2026-09-22｜P02 验收基线：见 P02_RESULT.json。

## 1. 已验收坐标与环境

- 工作区 START = `c3cd52859`；END = 工作区状态（零生产 commit，等待用户授权提交——P02 改动清单见 P02_REPORT §5）
- 分支：`docs/knowledge-closeout-2026-09-21`（未切换）
- Node v24.16.0；npm 11.13.0；darwin 27.0 arm64；lockfile sha256 `a9735825cea1d018…`（未变）
- 全量 npm test 终态：见 P02_RESULT.json commands 字段（预期红 = F1 治理 4 例，与 P01 基线一致则未扩大）

## 2. P03 开工前必读（本阶段新建契约）

| 文件 | 对 P03 的用途 |
|---|---|
| docs/refactor-2026/P02/IDENTITY_CONTRACT.md | taskId 铸造厂（lib/tasks/task-identity.ts）与 attempt 语义——P03 工具目录/参数契约的新工具若需后台任务身份，用 mintTaskId，勿再自铸；终态回调带 expectedAttempt |
| docs/refactor-2026/P02/STATE_TRANSITIONS.md | runId/task 状态机与迟到回调栅栏总表（含工具网关 lifecycleGeneration 行）——P03 工具生命周期测试的参照 |
| docs/refactor-2026/P02/CANCELLATION_MAP.md | 工具取消的真实链路（SessionExecutionRegistry/ConfirmStore/killTree）与清理期限——P03 统一执行边界不得另建取消通道 |
| docs/refactor-2026/P02/ENTRY_EXECUTION_MATRIX.md | 每入口正常+取消证据坐标——P03 入口接线测试直接复用 |
| docs/refactor-2026/P02/RECOVERY_MATRIX.md | durable 接受 vs best-effort 可见性的分工——P03 若新增工具持久化，按 §2 选择响应语义 |

## 3. 本阶段建立的门禁（P03 不得削弱）

1. P01 五项门禁全部复验绿（typecheck×3 / core-contracts / dependency-boundaries / tool-invocation-boundaries / pi-sdk-import-boundary）。P03 新增核心契约文件进 strict 范围的规则不变（tsconfig.core-contracts.json include + 零诊断）。
2. **改动 cli/shared/core/lib 导入图后必须重跑 `scripts/compute-cli-closure.mjs`**；新增生产 TS 文件必须进 export-manifest.json 白名单（本轮实证：漏加 → open-boundary-lint 红，补入后绿）。
3. 测试新增 spawn 固定 argv 范式：优先扩展现有测试文件（composition/teardown 等），避免新文件被安全钩子误判命令注入（P01/P02 各登记一次）。

## 4. P03 主责输入（P00 所有权图归 P03 项 + 本阶段新登记）

- F2（lint 3 生产 error：lib/tools/run-code-tool.ts、lib/tools/security-scan-tool.ts）主责仍 P03。
- P00 REFACTOR_BACKLOG P03 行：工具目录/schema/权限/生命周期一致的真实工具执行。
- 本阶段移交：工具网关 prepared invocation 的 lifecycleGeneration 复核已在 STATE_TRANSITIONS §5 登记（17 例既有证据，P01-A08）；工具取消经 SessionExecutionRegistry 包装（engine.ts:4624）是唯一工具级取消面，P03 统一边界时沿用。

## 5. 已执行验证与遗留

- 已绿：typecheck×3、core-contracts（identity-brands 改动后零诊断）、dependency/tool-invocation boundaries、pi-sdk 族、任务书 §7 定向（teardown+history-run-outcome-edges）、本阶段新增 6 文件+受影响回归 171/171、composition 全文件 10/10（含 A15 真实 server 重启循环与 P01 纵向链回归）、全量 npm test（见 RESULT）。
- BLOCKED（环境，继承 P01）：build:server:open / smoke:server:open（F3 网络）；四平台 CI；Windows/Linux 实机——P08 取回。
- 旧路径清退核实：taskId 5 类自铸全部收口（P00 枚举的 TaskRegistry-facing 面）；无平行 TaskRegistry/取消通道/状态机。例外账本：会话 fork 克隆路径三处 store 域自铸克隆 ID（`media-fork-`/`subagent-fork-run-`/`workflow-fork-run-`，见 IDENTITY_CONTRACT §4.5）——P03 新增克隆/派生身份时**不得**再新增自铸格式，既有三处是否归入 task-identity 属范围决策，登记待后续阶段。

## 6. 必保留兼容（P03 不可改变）

- 本阶段全部兼容规则：IDENTITY_CONTRACT §4（旧格式 taskId 原值读、缺 attempt 按 1、媒体三域栅栏分工）与 P01 交接 §6 全部条目。
- TaskRegistry attempt 栅栏语义：不禁止终态后重注册；不带 expectedAttempt 的旧调用语义不变——P03/P04 扩展调用方时按需启用 expectedAttempt（捕获 register 返回的 task.attempt）。
- teardownSessionResources 的 warn-not-throw 契约与 {errors} 返回值：除 runAgentSession（AggregateError，A12）外的 8 处调用方依赖不抛错行为（session-coordinator 5 + bridge-session-manager 2 + runAgentPhoneSession 1，全量账本见 CANCELLATION_MAP §5）。

## 7. 工作区卫生提醒（继承并补充）

- 全量 npm test 的 stdout 不要重定向到仓库扫描范围内的文件（R10-09 快照撕裂；本轮按 P01 指引先落 /tmp 后拷贝）。
- 全量后检查 `git status`：`artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch` 若被 R10-09 重写，取证后 `git checkout --` 还原。
- 本轮新增未跟踪目录：docs/refactor-2026/P02/、artifacts/refactor-2026/P02/、lib/tasks/、tests/{task-identity-attempts,p02-run-finalize-edges,p02-cancellation-edges,p02-concurrency-edges,p02-recovery-restart,p02-entry-lifecycle-edges}.test.ts。
