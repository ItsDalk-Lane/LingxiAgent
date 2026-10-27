# R03 修复轮最终阶段终审（R03_FIX_FINAL_STAGE_REVIEW）

- 审查者：**STAGE-REVIEWER-R03-RR1**（2026-09-30，一次性全新阶段验收代理；未参与本轮任何实现、修复、工作单执行或组级审查）。
- 派单：`docs/rust-tauri/R03/dispatches/R03-REPAIR-STAGE-REVIEW-RR1_DISPATCH.md`（全部要求已执行）。
- 候选：`CANDIDATE=ebcbcad1b9fa1c621b315dd33b58f8a2e01ab9eb` = HEAD = `origin/codex/rust-tauri-migration`（`git merge-base --is-ancestor` 逐提交核验，含 385ff2940→ebcbcad1b 全链 8 提交均在远程）。
- 工具链：`~/.cargo/bin/cargo`（rustup 1.98.1，rust-toolchain.toml 锁定），全部 `--locked`；复测数据根 `/tmp`（隔离副本）。
- 证据根：`artifacts/rust-tauri/R03/repair-current/STAGE-REVIEW-RR1/`。审查期间未修改任何产品/测试/配置/门禁/账本/既有证据，未 commit/push。

```text
STAGE_VERDICT: PASS
```

## 0. 工作树如实记录（派单 §0「应干净；若不干净先如实记录」）

`git status`：两个已跟踪文件有未提交修改——`R03_FIX_COMMIT_RECEIPTS.json`（+12 行 G07 回执）与 `R03_FIX_ISSUES.json`（+6 行 F08 commits/remote_receipt）。两者均为**总控补记 ebcbcad1b 自身回执**的账本追加（提交无法包含自己的 SHA，鸡生蛋性质），diff 内容逐行核对仅登记性元数据，无行为面。另有未跟踪的派单文件与本审查证据目录。不影响裁决。

## 1. 七条主反例独立实测（派单 §2；全新自写复测，非引用执行者日志）

方法：审查者从零编写 `stage_review_rr1.rs`（canonical 源在证据根；编译于 `/tmp/stage-review-rr1/ws`——仓库 `rust/` 排除 target 的隔离副本，候选字节等同 HEAD），走**真实 service 入口与存储**（SessionStore 受理/执行/steer/取消、RunSupervisor::drive_run、TaskSupervisor、CancelRegistry、真实 SQLite/RunDatabase、RecoveryCoordinator 的恢复函数），Provider/Tool/Approval 替身只产生外部响应或受控副作用（外部计数器=文件+原子计数，park=信号量握手，无 sleep 碰绿）。

命令（真实，退出码在日志）：

```bash
~/.cargo/bin/cargo test --manifest-path /tmp/stage-review-rr1/ws/Cargo.toml --locked \
  -p lingxi-service --test stage_review_rr1 -- --test-threads=4
# → test result: ok. 16 passed; 0 failed（exit 0；rr1-counterexamples-FINAL.log）
```

首跑 3 个用例失败，逐一归因为**我的测试脚手架错误**（非候选缺陷）：①F02/F05a 忘配 tool_executor——drive 对任何 ToolRequests 轮（含纯委派）要求执行器已配置，否则 ToolExecutorUnavailable（源码 runs.rs 如此，行为正确）；②F04c 审批替身第二问脚本耗尽得 Aborted。修正后 16/16 绿。三轮原始日志均在证据根（run1 含失败原文，未删改）。

| # | 主反例 | 实测方式（我的测试函数） | 关键断言 | 结果 |
|---|---|---|---|---|
| ① | F01 树链接+父先取消再建子 | `rr1_main_f01_…`：真实 CancelRegistry/`register_linked`/`run_root_under`/`child` 三入口建链，取消父；再"先取消后建"三入口；最后在已取消树上经真实 `TaskSupervisor::spawn_linked` 派发计数适配器 | 全部子孙取消+等待者有界醒；晚建节点继承标志/首因/首时刻，registry 项起始 Requested；**适配器调用计数=0**，exit=Aborted | PASS |
| ② | F02 同进程反复父取消超上限 | `rr1_main_f02_…`：真实 ServiceState，per_session=global=2，**4 轮**（>上限）"派发停驻子代理→取消父"；全程无重启、无 startup_scan | 每轮：子线程 busy 清除、active_counts==(0,0)、父/子 durable 行均 `cancelled`、监督登记回收；第 4 轮后正常子代理派发成功（completed） | PASS |
| ③ | F03 最终持久化停驻取消 Accepted | `rr1_main_f03_…` 双臂，自制 `GatedStore` 直通真实存储、在真实持久化边界 park（到达信号→测试行动→释放，写入仍走真实单写者链） | A 臂（mc0001 模型事件持久化中取消）：Accepted→释放后**无 completed、无 final message**、provider 恰 1 次调用；B 臂（finalize 事务中取消）：**TooLate**→completed+final message 在，事后取消 AlreadyTerminal——只有一个胜者 | PASS |
| ④ | F04 外部+1 后 panic | `rr1_main_f04_…`：Tool 真实写外部计数文件后 panic；`recover_run_invocations` 真实跑两次 | 外部计数=1；journal `receipt_outcome='unknown'`、`dispatched=1`（非 Failed/Success）；两次恢复 decision=`needs_attention`（保守能力下绝不盲重试）、计数恒 1 | PASS |
| ⑤ | F05 容量满/起始持久化故障同 key 重试 | `rr1_main_f05a_…`（cap=4 被 2 个停驻子代理占满→后台 requestId 提交被拒）+ `rr1_main_f05b_…`（首个 `record_run_started` 注入 Io 失败） | 首次：BackgroundRegistryFull/Storage 错误**响亮**、runs 行数不变、provider 0 调用（无受理无副作用）；释放后同 key 重试：真实新 Run 完成，provider 恰 1 次——**无幽灵 Replay** | PASS |
| ⑥ | F06 >2000 字符完整到达 | `rr1_main_f06_…`：1999/2000/2001/2600 字符（中文+emoji+CRLF 混合，尾部放决定性标记），前台 `execute_for` 与后台 `execute_background_for` 双入口 | Provider 实录输入与提交输入**逐字节相等**（String 全等）——尾部标记完整到达，无静默截断 | PASS |
| ⑦ | F07 后台 steer 下一轮消费恰一次 | `rr1_main_f07_…`：双后台会话停驻第 1 轮；alpha steer→Accepted→释放 | alpha 第 2 轮输入含追加文本且 `matches()==1`；beta 全程 0 次；alpha 完成后新 Run 不再收到（全库出现次数恒 1）——不串扰 | PASS |

## 2. 变体抽查（派单 §3.2：≥6 条不同 F-ID）

8 条变体（覆盖 7 个 F-ID，含点名的 C03/C05/F06-C03/F07-C04 四个高风险面），同一命令同一二进制内：

| 变体 | 测试函数 | 攻击面 | 结果 |
|---|---|---|---|
| F01-C03 | `rr1_var_f01c_…` | barrier 起跑：6 racer×3 子节点+孙节点 注册竞态取消；遍历后加入者；首因保持 | 18/18 节点全取消、首因不丢、晚加入继承（PASS） |
| F02-C02 | `rr1_var_f02b_…` | current_thread 确定性：树在 wrapper 首次 poll 前 fire（never-first-poll 窗口） | exit=Aborted、适配器 0 调用（PASS） |
| F03-C03 | `rr1_var_f03c_…` | 双线程 barrier 并发异因 fire；Cleaning 后迟到 fire；之后 claim | 恰一个 Fired、scope 首因==phase 因、无回退、claim 以首因 CancelledBy（PASS） |
| F04-C03 | `rr1_var_f04c_…` | 授权拒绝 vs 可信外部失败 vs panic 三类负面事实 | refused：failed+dispatched=0；trusted：failed+dispatched=1；与主④unknown 区分（PASS） |
| F05-C03 | `rr1_var_f05c_…` | 同 key 异内容冲突；同 key 同内容跨会话 | Conflict 响亮零新 Run 零执行；beta 独立命名空间真实新受理（PASS） |
| F05-C05 | `rr1_var_f05e_…` | 同一数据 home 上真实二次 bootstrap（进程级注册表清空=重启） | 同 key→`RequestIdBoundToEarlierRun` 点名旧 Run；新 key 正常受理（PASS） |
| F06-C03 | `rr1_var_f06c_…` | >1MiB（MAX_SUBMISSION_INPUT_BYTES）超限，前后台双入口 | `InputTooLarge{bytes,limit}` 精确；runs 行数不变、provider 0 进入（PASS） |
| F07-C04 | `rr1_var_f07d_…` | 容量 2 的 inbox，前台+后台同契约 | 容量内真消费（两文本均达下一轮）；第 3 条响亮拒绝且不污染队列（PASS） |

## 3. 原 R03 Gate 独立重跑（派单 §3.1）

```bash
~/.cargo/bin/cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- \
  verify-stage R03 --evidence artifacts/rust-tauri/R03/repair-current/STAGE-REVIEW-RR1/verify-stage-r03
# → overall PASS, EXIT_CODE=0
```

结果 JSON 核验（`verify-stage-r03/verify-stage-result.json`）：**15/15 命令 PASS**（rust_test_workspace、A15 矩阵、7 条 R02 定向链、A16 seed、fmt/clippy、check-contracts/check-boundaries、**repair_suites**、r02_events_matrix）；**17/17 场景 PASS**（16 A-ID+R03-RP01）；**48 叶 = 17 stage_share_satisfied + 31 deferred**（控制台逐条核对：17 个 PASS 叶 ID 与 G07 记录一致）；`candidateSourceBinding.stable=true`（15 个逐命令 checkpoint 全稳定、before==after digest）；`testedShaAtEnd=ebcbcad1b…`（=HEAD=远程）；runnerSourceBinding PASS。

**如实记录（反而成为 F08-C03 的现场证明）**：第一次运行 overall **FAIL**（exit 1）——`finalChangedPathBytesHex` 解码为 `…/STAGE-REVIEW-RR1/stage_review_rr1.rs`，即**我在门禁运行期间写入证据目录的复测源文件**触发了候选稳定性守护（reason："Candidate file bytes or HEAD changed during the registered stage checks…stage PASS is forbidden"）。这不是候选缺陷，而是绑定机制正确拒绝运行中变更的实证；该次运行完整保留于 `verify-stage-r03-attempt1-dr-contaminated/`（15 命令本身全 PASS，仅 overall 被稳定性强制 FAIL）。清空证据根后干净重跑得上述 PASS。本轮干净运行的候选 digest 含我静态存在的复测源文件（稳定的非执行性文件）与 worktreeDirty=true（§0 所述账本补记），按规格「元数据与行为分开核对」不影响 testedSha 与 15 命令的行为面结论。

独立命令实跑（我的退出码，证据 `gate-commands/`）：

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | 零 diff |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | 零告警（1m26s） |
| `cargo test --workspace --locked` | 0 | **72 suites / 709 passed / 0 failed**（与 G07 记录一致） |

## 4. R02 回归（派单 §3.3）

- **7 条定向链**：在本审查的 verify-stage R03 内全绿（r02_auth_matrix / r02_storage_tx / r02_events_matrix / r02_backup_restore / r02_recovery_drill / r02_full_chain / r02_legacy_regression(directed)——控制台逐条 PASS）。
- **全量红集归因核对**：G07 的 `verify-stage-r02-regression`（exit 1，19/20）：唯一红 a16 的 4 个失败文件＝3 个注册 SEAL_FAMILY（post-verification-audit-seal/round2/round3-delivery-evidence＝R03-GOV-01/02 治理递延）+1 条 `artifact-core-ustar.test.ts` 环境闪红。**本审查独立复核**：①隔离重跑 `npx vitest run tests/artifact-core-ustar.test.ts`＝10/10 绿（exit 0，与登记归因一致：afterEach fs.rmSync ENOTEMPTY 临时清理竞态，该文件与 shared/artifact-core 在 cd3fb19e6..ebcbcad1b 零触碰）；②seal 红我实跑复现同一形态（"VERIFIED_SOURCE_SHA 之后出现了非审计文件改动"——坐标滞后守护，GOV-01/02 登记，属 seal 工作流，非本轮范围且按项目规则不得由我"修绿"）。红集与登记类别**逐条一致，无新失败塞进旧类别**（ustar 被如实单列，未套 seal 族）。

## 5. 修复未制造回归（派单 §3.4）

- 取消后 completed：主③A 臂证伪存在（Accepted 后无 completed/final）。
- 双重 finalize：主③B 臂事后取消 AlreadyTerminal；workspace 709 绿含 r03_t08 矩阵终态幂等用例。
- 重复计数释放/迟到回调误清：主②4 轮 counts 精确回 (0,0) 且 thread 快照的 last_run_status 正确；源码 `note_child_finished` 的 exactly-once 环+身份栅栏（child_run_id 匹配才清 busy）在案；套件 `late_and_duplicate_completions_cannot_corrupt_the_closeout` 存在且在其位。
- 跨主体 key 串用：V5 跨会话命名空间独立受理证伪。
- 未知副作用盲重试：主④ needs_attention 两遍恢复零重做。
- steering 串新 Run：主⑦全库出现次数恒 1。
- G01–G06 九套件：门禁 repair_suites PASS（逐套件精确计数）+ workspace 709/0。

## 6. 问题账与交付物（派单 §3.5–3.7）

- **34 C-ID 三层状态**：`R03_FIX_ISSUES.json` 顶层 `cases` 数 34；逐项 normal/adversarial=PASS_EXECUTOR、independent=PASS，reviewer_id 与所属组一一对应（G01×10、G02×4、G03×4、G04×5、G05×3、G06×4、G07×4）；8 F-ID 均 CLOSED_BY_INDEPENDENT_REVIEW_R1。**无 NOT_A_DEFECT 条目**（不存在无依据关闭问题）；34 项审查证据引用齐全且指向的证据根（G0x-E01/G0x-R1/G07-E01）实测全部存在。
- 红绿可追溯（F08-C01）：`R03_FIX_ACCEPTANCE_RESULTS.json` fIdMatrix 7 组（9 套件 59 用例）＋各组红基线日志在档。
- 门禁负向/绑定（F08-C02/C03）：G07-E01 negative-tests 7/7 在档＋本审查的 attempt1 现场复证（§3）。
- **无 R04 倒灌**：`git diff --name-only cd3fb19e6..ebcbcad1b -- rust/crates`（非测试）仅 13 个 F 域文件（cancel/task_supervisor/subagents/runs/sessions/dedup/background/limits/lib/run_store/xtask×3）；无工具网关、真实供应商、CLI/UI、Tauri 宿主、发布流程。
- 提交链：8 提交与 `R03_FIX_COMMIT_RECEIPTS.json` 一致（G07 条目在工作树补记版中，指向 ebcbcad1b=远程，见 §0）；远程包含逐提交核验通过。
- 递延保持：R02 图 9 叶 deferred_to_r07、R03 图 31 叶、T06-O1 ask 档（R04）、Windows/打包（R09/R10）登记未动；治理残留（GOV-01/02 seal 坐标/patch-too-large）如实单列未伪造解决。
- 现行报告：R03_REPORT.md REOPENED 节+修复轮进度小节（历史原文保留）、R03_HANDOFF.json status=REOPENED_PENDING_REPAIR+FINDING-1 关闭注记、R03_ACCEPTANCE_LEDGER repair_rounds/R03-RP01 追加——**本 PASS 裁决后该 REOPENED 状态应由总控按授权流转为再接受**（见 §8）。

## 7. 发现（非阻塞，如实）

1. **无阻塞发现**。七条主反例、变体抽查、原 Gate、命令面、R02 回归、账本/范围/Git 全部通过。
2. 观察项（不构成 FAIL）：①G07 回执两文件的未提交补记（§0，鸡生蛋性质，建议总控随下次授权提交收口）；②全量 verify-stage R02 的 ustar 环境闪红仍会在偶发条件下复现（测试基建 `afterEach fs.rmSync` 的 ENOTEMPTY 竞态），归属测试基建治理而非 R03 行为面；③本审查 attempt1 展示了「审查者自己在运行中写证据」会触发候选稳定性强制 FAIL——后续阶段 Reviewer 宜先备好静态证据文件再启动门禁。
4. 需标 STALE：**无**（G07-E01 证据绑定 testedSha=8a6303bcd4→其提交 ebcbcad1b 后行为输入零变化——本审查在同一内容上独立重跑 15/15+709/0 全等，未发现任何陈旧依赖）。

## 8. 审查范围声明

- 只读产品/测试/配置/门禁/账本；复测产物仅写 `artifacts/rust-tauri/R03/repair-current/STAGE-REVIEW-RR1/` 与本报告；未 commit/push；未动真实用户数据；无网络外发；Provider/Tool 替身只产生外部响应/受控副作用。
- 实测环境：macOS darwin 27.0.0 arm64，rustup cargo 1.98.1（rust-toolchain.toml 锁定；PATH 中 Homebrew cargo 未使用），`--locked`（Cargo.lock 零变化），/tmp 隔离数据根与 /tmp 工作区副本。本地结果不替代其他平台/正式打包/真实供应商验证。
- 未覆盖（依派单明确非本轮前置）：正式签名/公证、真实供应商、完整 R07 UI、Tauri 宿主、Windows；审计治理 seal 坐标与 patch-too-large 归既有授权流程。
- 裁决仅对本候选 `ebcbcad1b` 有效；其后任何行为代码/测试输入变更须新建阶段 Reviewer。

## 9. 结论

七条主反例全部由本审查**独立实测复证**（16/16 自写用例 exit 0），8 条变体覆盖 7 个 F-ID（含点名的四个高风险面）通过；原 R03 阶段 Gate 全新证据根重跑 overall PASS（15/15、17/17、48=17+31、binding stable、testedShaAtEnd=ebcbcad1b）；fmt/clippy/workspace 独立实跑 0/0/0+709；R02 七定向链绿、全量红集与登记归因逐条一致；34 C-ID 三层全 CLOSED 且证据根齐备；无 R04 倒灌；提交链与远程一致。**STAGE_VERDICT: PASS**——F01–F08 全部确认关闭，R03 对抗性修复轮可由总控按既有授权流转为 RE-ACCEPTED（含 §0 两文件账本补记的收口提交与 R03_HANDOFF/REPORT/LEDGER 状态流转）。
