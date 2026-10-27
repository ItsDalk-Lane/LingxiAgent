# R00-T01 独立验收：第 1 轮

**VERDICT: FAIL**。`R00-A02` 有真实 PASS 证据；`R00-A01` 的真实目录保护前置条件及目录内容哈希没有满足，不能将现有替代证据算作 PASS。本结论仅针对 R00-T01，不评价 R00 其他任务。

## 审查范围与复跑

- 候选 HEAD 为 `7d1a0c6bc28062ff455adcf8f68c3a80b117e90d`，分支为 `codex/rust-tauri-migration`。审查前 R00-T01 候选为 28 个文件；检查了两份场景结果引用的全部证据文件哈希，均与记录一致。相关源码、`AGENTS.md`、`CONTRIBUTING.md`、`tests/README.md` 和依赖锁摘要也与 `BASELINE.json` 相符。
- 已逐项核对 T01 五个步骤及三项交付。规则文件、分支/HEAD/工具链、远端默认分支、研究与实施 SHA 对照和工作树差异已有记录；两 SHA 相同，`BASELINE_DELTA.md` 的“无已提交源码差异”属实。`BASELINE.json`、`BASELINE_DELTA.md` 和实际隔离路径/启动探针均存在。第 4 步的真实目录保护证据不足，详见 F01。
- 源码链为 `desktop/main.cjs` 开发态派生 `server/bootstrap.ts` → `server/main-full.ts` → `server/index.ts:startServer` → `ensureFirstRun` / `LingxiEngine.init`。候选探针直接走该真实服务组合入口；它没有 mock 待测服务。`scripts/launch.js` 经 `scripts/dev-env.js` 会覆盖传入的 `LINGXI_HOME`，候选已识别此行为，并同时设置临时 `HOME` 与 `LINGXI_HOME` 绕开该启动器。日志、进程参数、隔离目录中的 `server-info.json` 和带令牌的健康检查共同证明这次服务实际启动，退出码为 0，关闭后该文件消失。
- 我独立重跑 `vitest run tests/startup-contract.test.ts tests/hana-runtime-paths.test.ts tests/server-composition-boundary.test.ts`，使用新的临时 `HOME`、`TMPDIR`、`LINGXI_HOME`；退出码 0，3 个文件、26 个测试通过。另单独复跑真实 full composition 的 HTTP 场景，退出码 0；该场景确实派生真实服务，验证未带令牌为 403，带令牌的健康、代理列表和日记接口为 200。单独复跑使用名称筛选，其余 9 个测试显示 skipped，不计为通过；完整复跑的 26 个测试均通过。
- 全局拒写尝试的错误指向临时 `LINGXI_HOME` 的 `mkdir` 被沙盒规则拒绝，退出码 1；这是试验策略阻断服务，不应记为产品回归。最终探针的沙盒配置拒绝写入真实用户 HOME，负向拒写探针报 `EPERM`。这些证据支持**受沙盒约束的该服务进程**无法写入真实用户 HOME；并未证明整个真实目录在有并发应用时保持字节不变。

## REQUIRED 场景

| 场景 | 独立判断 | 理由 |
|---|---|---|
| R00-A01 隔离目录可证明 | **未通过** | 服务隔离和关闭链路通过，但真实目录未设置只读哨兵，且真实 `~/.lingxi`、`~/.lingxi-dev` 只核对路径、权限、大小、修改时间等元数据清单；并发运行的真实应用使真实目录内容不变这一要求缺少可归因的证据。见 F01。 |
| R00-A02 脏工作区不丢失 | **PASS** | `worktree-before.json` 与 `worktree-after.json` 中原有 17 个已跟踪删除项的 diff 摘要相同；原有新任务书 33 个未跟踪文件的逐文件 SHA-256 前后及审查时均相同。审查时其他已跟踪和暂存差异为空；任务书改动未混入 T01 交付。 |

## Findings

### F01 — BLOCKING｜真实目录保护条件被替代证据当作 PASS

- **位置：**`artifacts/rust-tauri/R00/T01/startup_probe.py` 的 `directory_inventory`、`before`/`after` 与 `acceptance_checks`；`artifacts/rust-tauri/R00/T01/startup-probe.json`；`artifacts/rust-tauri/R00/T01/R00-A01.result.json`；`docs/rust-tauri/R00/R00-T01_REPORT.md` 的 A01 结论。
- **证据：**探针把只读哨兵放在 `/tmp/.../protected-production`，而不是验收前置条件中的真实目录。对真实 `~/.lingxi` 和 `~/.lingxi-dev` 只计算相对路径、类型、权限、大小和 `mtime_ns` 的摘要；只有现成的 `~/.lingxi/data-epoch.json` 单文件计算内容哈希。候选明确记录真实 Lingxi.app 同时运行，且声明未读取真实目录全部文件内容。元数据相同无法排除等长内容变化或扫描间写入后恢复。
- **违反要求：**R00-A01 要求“真实目录设只读哨兵，另建空测试目录”，并以“生产目录哈希和哨兵未变，日志与数据只出现在测试目录”为通过条件。通用约束要求 REQUIRED 场景有真实 PASS 证据，不得把未满足的前置条件或推断记为通过。
- **后果：**现有材料可证明本次服务在临时目录启动，并且沙盒阻断其写入真实 HOME；不能按原验收条件宣称真实生产目录及只读哨兵完整无变化。后续报告若继承 A01 的 PASS，会把未完成的数据保护证明带入迁移基线。未发现本次探针实际改写真实用户数据。
- **根因及同类路径：**为了避免干扰正在运行的真实应用，用受控替代目录和元数据清单替换了真实目录的前置保护与内容核对；同一缺口覆盖 `~/.lingxi` 和 `~/.lingxi-dev`。
- **修复要求：**先将 A01 改为未通过或 BLOCKED。若要按原规格通过，应在不干扰用户数据、取得必要授权的安静窗口中，建立真实目录的只读哨兵及可复核的前后内容摘要，并记录并发应用状态、沙盒配置、进程参数、退出码与目录内外写入；任何新增或权限改动不能未经授权施加于真实用户目录。若选择维持当前安全的替代方案，须由规格负责人正式确认等价验收条件后再重测，不能由实施报告自行替换。
- **必须重跑：**R00-A01 的完整真实启动/关闭与拒写负向探针；真实目录和哨兵前后核对；`startup-contract`、`hana-runtime-paths`、`server-composition-boundary` 三组受影响回归；重新计算该场景引用的证据哈希和最终候选摘要。A02 只有在工作树或取证脚本因此变化时才需重跑。

本轮只新增这份审查报告；未修改产品源码、测试实现、配置、T01 实现文件或总控账本，未提交或推送。
