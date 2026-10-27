# R05 RR2 基线简报（RR2_BRIEF）

维护：RR2 收口总控（本会话）。所有工作包智能体开工前必读本文件 + `RR2_MASTER_PROMPT_2026-10-06.md`。

## 候选与环境（2026-10-06 总控核实）

- 分支 `codex/rust-tauri-migration`，HEAD=`ad5ec4e9853a51ed929f1e2e077b97d41c951572`（=origin，已推送）。
- 异常记录：总控接手时主工作区一度处于 detached HEAD（指向同一提交，无任何丢失），已重新挂回分支，原因不明，登记备查。
- 工具链强制：所有 cargo/rustc 调用必须使用绝对路径 `/Users/study_superior/.cargo/bin/cargo`（rustup 代理→1.98.1，符合 `rust/rust-toolchain.toml`）。PATH 里的 Homebrew cargo 是 1.93.0，禁止使用；禁止无理由升降级或改锁。
- 修改白名单（RR2 总控 §二）：`rust/`、`scripts/rust-tauri/`、`.gitignore`、`docs/rust-tauri/R05/**`、`docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`、`docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json`。越界先登记到 `RR2_PROGRESS.md` 并核对授权，其余工作继续。
- 新证据放 `artifacts/rust-tauri/R05/RR2/<WP>-<轮>/`（如 `A-R2/`）；`FINAL-01/` 保留给阶段终审，任何工作包不得写入或预创建。
- `E/RR2/` 下既有 `A-I1`、`D-I1`、`G-I1`、`G-I2`（mtime 2026-10-06 14:23）为**前次中断会话残留**：A-I1=两个 raw `runs.db`，D-I1=9 个 r05f24 测试库，G-I1=完整隔离源码副本（含 .git），G-I2=synthetic-archive-fixture。无台账、主树零代码改动，**不纳入本轮证据链**；可只读参考，不得删除或覆盖。
- 提交纪律：子智能体一律禁止 `git commit/push/branch/tag`；收口由总控在阶段终审后统一精确暂存、提交、推送（已有有效授权）。
- 安全钩子：写代码若被 Mimosa 拦截（典型：动态 `new RegExp(变量).exec()`、shell/命令字符串拼接），按其建议改为参数列表、纯字符串解析等安全写法，不得绕过。服务端/测试代码涉及 URL 时仅 http/https 且校验 host、拒绝环回/私网（受控测试替身的显式例外按既有测试模式声明）；SQL 一律参数绑定，不得拼接。
- 测试并发注意：本轮多个工作包共用同一 cargo target 目录，遇到 `Blocking waiting for file lock` 属正常，等待即可，不得杀其他 cargo 进程。

## 必读材料（按序）

1. `RR2_MASTER_PROMPT_2026-10-06.md`（本轮权威，特别是 §四 中本工作包条目）。
2. `RR1_MASTER_PROMPT_2026-10-04.md`（完整继承：§3 执行纪律、§5 四层证据与防漏规则、§6 放行公式）。
3. `RR1_FINAL_REPORT.md`、`RR1_ISSUE_MATRIX.json`（本工作包 F-ID 行的完整字段）、`RR1_PROGRESS.md`、`RR1_LEAF_DEVIATIONS.md`。
4. 工作包对应原始证据（见矩阵行 `evidenceDir`；多为 `E/RR1/INDEPENDENT-9/` 与 `E/RR1/INDEPENDENT-9-supplementary/`）。

## 工作包与文件所有权

| WP | 问题 | 主要文件 | 所有权说明 |
|---|---|---|---|
| A | F41 存储登记 | `docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json`、`scripts/rust-tauri/r02_t04_storage_tx.sh`、登记一致性自检挂点 | 独占上述文件；`migrations.rs` 只读核对，不改动 |
| B | F42 活跃日志绑定 | `.gitignore`、`scripts/rust-tauri/r02_t08_legacy_entry_regression.sh` | 独占 |
| C | F34 永久测试腿 | `rust/crates/lingxi-adapters/tests/r05_t04_rr1_batch_terminal.rs` 及 producer/映射挂点 | 独占该测试文件；映射表（stage_map/TEST_MAP/cids tsv）改动须在进度表登记（与 A/D 可能相邻，先到先改+登记） |
| D | F43 workspace 三测 | `r00_management_leaves.rs`、`r04_t05_process_tools.rs`、`r05_t08_production_tools.rs` | 独占三个测试文件与必要测试基建；生产代码尽量不动，确需动先登记原因与回归范围 |
| E | F31 恢复 404 | `management.rs`（NotOAuth 列表面分支）、`r05_t02_credentials.rs` 钉住测试、`RR1_LEAF_DEVIATIONS.md`、叶证据描述 | 独占上述；F25 侧表（`r05_leaf_case_map.tsv`/`R05_SCOPE_MATRIX.json` 等）仅在其消费的测试语义变化时最小同步并登记 |
| F | F40/文档收口/HANDOFF | `RR2_*`、`R05_*.md/json`、`ORCHESTRATOR_PROGRESS.json`、`R05_HANDOFF.json` | 最后执行，独占 |
| G | I01–I11 映射 + 负测 | `E/RR2/G-*/`、隔离副本 | 主树只读；变异只在隔离副本；发现真实缺口→登记新 F-ID（F44+）走修复轮 |

## 状态与返回纪律

- 每个 F-ID 状态：OPEN→IMPLEMENTED→INDEPENDENT_PASS→CLOSED；BLOCKED 必须具名环境归因（命令、退出码、缺口）。
- 每步更新 `RR2_ISSUE_MATRIX.json` 对应行与 `RR2_PROGRESS.md`。
- 执行者返回（≤60 行，中文）：`workPackage / status(IMPLEMENTED|PARTIAL|BLOCKED) / summary / changedPaths / selfChecks[{command,exitCode}] / counterexamples / evidencePaths / blockers`。
- 审查者返回：逐 F-ID `PASS|FAIL|BLOCKED` + `mustFix`（文件:行、现象、期望）+ `commandsRun`；亲跑正负例，不得修改被验对象。
- 诚实底线：没跑过的命令不写跑过；失败如实报；无法达成的门禁走升级/如实 BLOCKED，不绕过、不伪造。
