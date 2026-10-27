# R03 修复轮交接（R03_FIX_HANDOFF）

- 生成：2026-09-30，G07-E01（EXECUTOR-REPAIR-R03-G07-E01；一次性执行代理，无 commit/push 授权）。本文件为执行者口径，不自称阶段 PASS；阶段重开状态的解除归总控另派的全新阶段 Reviewer。
- 基线 `FIX_BASE_SHA=cd3fb19e651f763afc6c75cb3163064fb54ca3fe`；修复轮候选 = HEAD `8a6303bcd477cda891d64fddd6e96fa35017f7ef`（G01–G06 已推送提交的顶端）+ G07 本轮未提交工作树（门禁接入/负向测试/报告更正）。分支 `codex/rust-tauri-migration`。
- 权威逐项状态：`docs/rust-tauri/R03/repair-current/R03_FIX_ISSUES.json`（F01–F07 = CLOSED_BY_INDEPENDENT_REVIEW_R1；F08 = G07 完成执行与两层自查，待阶段终审）。本文件引用真实提交，不写自身 SHA。

## 1. 本轮真实提交链（总控推送，远程包含回执在 `R03_FIX_COMMIT_RECEIPTS.json`）

| 提交 | 组/内容 |
|---|---|
| `385ff2940` | 总控：重开 R03 + 统一问题账登记（docs） |
| `520bb75b9` | G01：取消树链接（F01）+ 进程内子代理收尾（F02）——`cancel.rs`/`task_supervisor.rs`/`subagents.rs`/`runs.rs`/`background.rs` + 新套件 `cancel_link_inheritance.rs`(7)/`subagent_closeout.rs`(8) |
| `ccb09fde6` | G02：取消 vs 完成统一裁决（F03）——`cancel.rs` FireOutcome/裁决、`runs.rs`/`sessions.rs` + `cancel_terminal_race.rs`(13) |
| `198e0da1e` | G03：未观测工具退出归类 Unknown（F04）——`runs.rs`/恢复分类 + `tool_receipt_unknown.rs`(6) |
| `d56e6883d` | G04：两阶段受理绑定、无幽灵 Replay（F05）——`dedup.rs`/`sessions.rs`/`background.rs`/`runs.rs`/`subagents.rs`/`lib.rs`/`run_store.rs` + `admission_dedup_consistency.rs`(5)/`admission_dedup_adversarial.rs`(5) |
| `8883923a5` | G05：完整输入载荷保真+显式预算拒绝（F06）——`limits.rs`/`sessions.rs`/`lib.rs` + `input_payload_fidelity.rs`(5)/`input_budget_refusal.rs`(2) |
| `8a6303bcd` | G06：后台 steering 接入授权收件箱（F07）——`background.rs` 一行接线 + `background_steering.rs`(8) |

G07 本轮**未提交**改动（留总控按授权收口）：`rust/crates/xtask/src/stage_maps/R03.json`（+repair_suites 命令+R03-RP01 场景+图注）、`scripts/rust-tauri/r03_t08_generate_stage_map.py`（同步生成）、`rust/crates/xtask/src/stage_map.rs`（5 个钉图测试）、`rust/crates/xtask/src/verify/runner_tests.rs`（原 16 场景钉更新为 16+RP01）、新脚本 `scripts/rust-tauri/r03_g07_repair_suites.sh` 与 `scripts/rust-tauri/r03_g07_gate_negative_tests.sh`、现行报告更正（`R03_REPORT.md`/`R03_HANDOFF.json`/`R03_ACCEPTANCE_LEDGER.json`，均为追加不改原文）、`repair-current/` 轮次交付物与 `artifacts/.../G07-E01/` 证据。`rust/Cargo.lock` 与 `package-lock.json` 零改动。

## 2. 接口与语义变化（R04+ 消费面，详见 `R03_HANDOFF.json` interfaces，已同步更新）

1. **取消**：`CancelRunOutcome` 新增 `TooLate{run_id, detail}`（G02/F03）——驱动已不可撤销认领终态结算时取消请求诚实反馈"过晚"，不谎报 Accepted；`CancelRegistry::fire` → `FireOutcome::{Cancelled,TooLate}` first-wins。父取消路径在**当前进程内**完成 child 收尾/终态/线程与配额回收（G01/F02）；普通取消不得再以重启为通过步骤（F08 红线），startup recovery 仅留作真正进程崩溃兜底（DanglingActive→同一 finalize 事务）。
2. **受理/去重**：`SessionExecuteError` 新增 `AdmissionInFlight{request_id}`（预留中重试，绝不半提交假结果）与 `RequestIdBoundToEarlierRun{request_id, run_id}`（跨重启同 key 显式拒绝并点名既有 run，不盲重做）（G04/F05）。
3. **输入**：新增 `InputTooLarge{bytes, limit_bytes}`（超 `MAX_SUBMISSION_INPUT_BYTES`（=传输 1MiB）受理期响亮拒绝零副作用）；预算内输入逐字节完整（执行载荷与日志摘要分离；去重摘要覆盖完整输入）（G05/F06）。
4. **Steering**：前台/后台驱动消费同一授权收件箱（`lease.steering_inbox()` 同一 Arc）；后台 Accepted 的追加输入到下一模型轮且恰一次；取消先赢不 drain；子代理 child run 保持隔离 lane（G06/F07）。
5. **工具收据**：派发后无可信回执的 panic/中止/通道丢失/超时归类 `ToolOutcome::Unknown`（外部已执行但无回执绝不报成功/确认失败，恢复面禁盲重试）（G03/F04）。
6. **门禁机制**：R03 阶段图新增 `repair_suites` 命令与 `R03-RP01` 场景（F08）；xtask 钉图测试镜像注册与 9 套件计数钉表；门禁负向测试可重跑脚本入仓。

## 3. 剩余后续义务与边界

- **阶段终审（F08-C04）**：全新 STAGE-REVIEWER 须重读全部 F-ID/C-ID、独立实测七条主反例、复核原 16 场景/48 叶/7 条 R02 定向链与 R02 回归、检查 G07 门禁接入与负向测试证据；PASS 前不放行 R04。
- **既有治理递延（单列，不因本轮变化）**：a16/E5 封印族坐标滞后（R03-GOV-01）与 round3 patch-too-large（R03-GOV-02，git MAX_APPLY_SIZE 硬限）归 seal 工作流；本轮 R02 全量回归唯一命令红仍为 a16（E5），另有 1 条 `tests/artifact-core-ustar.test.ts` ENOTEMPTY 临时目录清理环境闪红（隔离重跑 10/10 绿、修复轮零触碰该面；归因记录 `verify-stage-r02-regression/A16/legacy-entry/g07-ustar-isolated-rerun-attribution.txt`）——**不套用旧 seal 类别**，如实登记待复核。
- **R04+ 递延保持原要求与归属**：R02 图 9 项 deferred_to_r07、R03 图 31 项 deferred_to_later_stage、T06-O1 ask 档审批面（R04）、Windows/打包（R09/R10）。
- **非本轮前置（明示）**：正式签名/公证、真实供应商、完整 R07 UI、Tauri 宿主与发布流程。

## 4. 验收面（供阶段 Reviewer 直达）

- 组合门禁（overall PASS）：`artifacts/rust-tauri/R03/repair-current/G07-E01/verify-stage-r03/verify-stage-result.json`（15/15 命令、17/17 场景、48 叶 17 pass+31 deferred、candidateSourceBinding stable、testedSha=8a6303bcd4）。
- 独立命令实跑：`artifacts/rust-tauri/R03/repair-current/G07-E01/gate-commands/`（fmt/clippy/workspace 72 suites·709 passed·0 failed/check-contracts/check-boundaries 全 exit 0）。
- R02 全量回归：`artifacts/rust-tauri/R03/repair-current/G07-E01/verify-stage-r02-regression/`（19/20，红逐条归因见 `R03_HANDOFF.json` verify_stage_r02_full_gate_g07_rerun）。
- 门禁负向测试与 C03 受控演示：`artifacts/rust-tauri/R03/repair-current/G07-E01/negative-tests/`（脚本 `scripts/rust-tauri/r03_g07_gate_negative_tests.sh` 可重跑）。
- 逐 F-ID 红绿矩阵与三层状态：`R03_FIX_ACCEPTANCE_RESULTS.json`。
- 六份组级独立审查索引：`R03_FIX_INDEPENDENT_REVIEWS/INDEX.md`。
