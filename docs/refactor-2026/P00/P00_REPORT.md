# P00 阶段执行报告 — 基线、范围与验收地图

## 结论

**PASS（基线质量）**——P00 八项任务全部完成，10 个验收场景全部通过，工作树零生产改动、真实数据零读写、零真实账户消耗。P00 的 PASS 只代表基线足够可靠可以实施，**不代表软件没有问题**：本轮登记了 2 个既有基线失败（F1 审计封印滞后 4 个测试红、F2 lint 3 个生产 error）、1 个环境受阻（F3 CI 证据不可取），全部已定级、可复现、有唯一主责阶段，未混入 P00 修复。

## 实际输入

- START_SHA = END_SHA = `92c6646c581883436e026c53898e8bfc34ce4d27`（分支 `docs/knowledge-closeout-2026-09-21`，零 commit）
- 任务书研究 SHA `8037fae7a` = HEAD~1，差异仅 8 个文档文件、0 源码（命令 P00-T01-drift-diff）
- 锁文件 sha256 `a9735825cea1d018c2a42ae04f875368a003c386ca68fc86503bc8acbcd79d3c`；node_modules 与锁一致（npm ls 抽查 4 关键包）
- OS：Darwin 27.0.0（macOS 27.0）arm64；Node v24.16.0（engines >=24.12 <25 ✓）；npm 11.13.0
- 未提交修改保护：进入时 0 已跟踪修改 + 2 个未跟踪任务书目录；退出时原样（终检 P00-T08-final-status）；全程未 reset/clean/切换分支；**未另建分支**（当前分支干净，建分支会扰动用户环境，已在 WORKTREE_SAFETY 记录决策）

## 任务逐项结果

| 任务 | 实现 | 生产入口与源码位置 | 测试与日志 | 旧路径去向 | 状态 |
|---|---|---|---|---|---|
| P00-T01 固定代码/环境/授权边界 | 新增基线记录（无生产改动） | BASELINE.json（drift/env/isolation 节） | P00-T01-git-status/-git-coords/-env/-lockfile/-history-tags/-drift-diff/-npm-ls-spot/-launcher-override-proof/-A03-isolation-proof(-r2) | 无旧路径 | PASS |
| P00-T02 功能清单与排除项 | 新增清单 | FEATURE_MATRIX.md（20 项已采纳+7 项排除/残留）、SCOPE.json | 复用只读源码证据（agent-feature-inventory-raw.md 80 次工具调用） | 无 | PASS |
| P00-T03 调用链与事实所有者 | 新增三份地图 | ENTRYPOINT_MATRIX.md（8 链全收敛判定）、OWNERSHIP_MAP.md（18 类事实：13✅/3🔴/2⚠️）、CALLSITE_MATRIX.json（10 callsite） | 116 次工具调用原始证据 agent-callsite-raw.md + 142 次 agent-ownership-raw.md | 无 | PASS |
| P00-T04 行为规格与回归样本 | 新增验收映射与 fixture | ACCEPTANCE_MAP.json（10 场景全映射） | 既有 1446 测试文件映射（agent-test-inventory-raw.md，31 次工具调用）+ 新 fixture（脱敏样本/必失败用例） | 无 | PASS |
| P00-T05 基线检查 | 全量执行 | BASELINE_TESTS.json / BASELINE_FAILURES.md | typecheck✓ / lint✗(F2) / build:renderer✓ / tool-boundaries✓ / npm test✗(F1) / gh CI 证据 BLOCKED(F3)；失败均保留首次记录与重跑链 | 无 | PASS（基线可用性判定；失败项按 A10 分类移交） |
| P00-T06 性能与提示词预算 | 协议冻结+初始样本 | BENCHMARK_PROTOCOL.md、PROMPT_BASELINE.json | 启动 B1/B2 各10次（median 1479/1431ms，COMPARABLE -3.2%）；历史基准 n=1k/10k（仓库脚本真实路由）；内存初始样本；W3/W4 标实验受限 | 无 | PASS |
| P00-T07 后续改动清单与纵向切片 | 新增清单 | REFACTOR_BACKLOG.md（8 阶段逐项+UNCHANGED_VERIFIED 7 项）、STAGE_DEPENDENCIES.json、NEXT_STAGE_HANDOFF.md | 每项有 P00 事实锚点 | 无 | PASS |
| P00-T08 验收交接 | 本报告+结构化结果 | P00_RESULT.json | 终检（复跑后定稿）：final-status-r2✓ / redaction-final-r2✓ / evidence-check-r2✓（三者顺序运行避免自引用时序） | 无 | PASS |

## 场景逐项结果

10/10 PASS，逐项明细（test/fixture/命令ID/预期/实际）见 ACCEPTANCE_MAP.json。要点：A01 工作区保护（进入/退出快照）、A03 真实数据隔离（11 断言全绿：隔离写入+哨兵零变化+干净退出）、A05 失败诚实（FAIL 与 BLOCKED 分开记录且 exit 保留）、A06 证据关联（检查器先演示拦截后终检通过）、A08 性能可比较（同输入两批 COMPARABLE+假优化判 INVALID）、A09 脱敏（公开产物零命中）。

## 接线和旧路径

- P00 不新增任何生产接线；新增的 5 个工具（run-logged/isolated-server-proof/evidence-check/redaction-check/bench-*）全部为测试基础设施，位于 artifacts/refactor-2026/P00/tools/，不入生产构建。
- 无旧路径退出需求（零生产改动）；反向确认：未动用 reset/删除任何既有文件。
- 无双写：P00 产物全部为新建未跟踪文件。

## 验证

全部命令 exit code 与原始日志：`artifacts/refactor-2026/P00/logs/command-log.jsonl`（47 条 JSONL，每条含 argv/时间/exit/digest）与同名 .out/.err。基线五命令结果见 BASELINE_TESTS.json；首次失败全部保留（A03 首跑 FAIL、lint 三连、redaction 四连、compare 语法错各留原始+重跑链）。

## 数据、权限与平台

- 数据：零真实用户数据读写（A03 哨兵快照证明）；隔离目录数据为合成。
- 权限：零权限模型变化；零真实凭证使用；LINGXI_TOKEN 仅用合成值。
- 平台：仅 darwin arm64 实测；四平台 CI 证据 BLOCKED（F3，gh 认证失效+代理不通）；不据此声称跨平台通过。

## 差异与限制

1. **批准的行为差异**：无（零生产行为变化）。
2. **执行期间工作区事件（F5/F6，已处置/已登记）**：npm test 的 R10-09 按设计重写跟踪补丁吸收未跟踪文件（已取证并 `git checkout --` 还原至 HEAD，最终零跟踪改动，后续阶段每轮全量测试后须复查 git status）；`Lingxi_Refactor_Taskbooks_2026-09-21_副本/` 目录在会话前后被外部创建/删除（P00 无任何命令触碰，command-log.jsonl 全量 argv 可复核），原任务书目录完好。
3. **受控保留（不进公开打包）**：`artifacts/refactor-2026/P00/logs/P00-A09-redaction-check.err` 与 `-r2.err`（首两版扫描器错误日志按设计引用了受控样本标记）；`isolated/` 下 server-A 原始运行日志。
4. **实验受限**：W3 流式吞吐/W4 取消响应未执行（协议已冻结，P07 执行）；常驻工具 schema 与动态资料的运行时 token 尺寸待 P06 payload 捕获。
5. **未验证环境**：四平台 CI（F3）、Windows/Linux 实机、真供应商（无授权）。
6. **既有非关键问题**：BASELINE_FAILURES.md「既有非关键问题登记」5 项，各有主责阶段。
7. **验收后修正（2026-09-21，用户授权的独立验收修正）**：command-log 条数 37→47（实测 command-log.jsonl 行数）；F2 归因提交 59c131276→6282cf35b（`git log -L` 核实为三行唯一历史提交，涉及本文件、P00_RESULT.json、BASELINE_FAILURES.md）；`deliverables.sha256` 已按原生成命令刷新。原始命令日志与 .out/.err 未改动。

## 回退与下一阶段

- 回退：删除 `docs/refactor-2026/` 与 `artifacts/refactor-2026/` 两个目录即可（无 git 操作、无数据影响）。
- 下一阶段：P01（唯一允许修改范围与必读输入见 NEXT_STAGE_HANDOFF.md）。本报告提交后停止，等待用户明确指定。
