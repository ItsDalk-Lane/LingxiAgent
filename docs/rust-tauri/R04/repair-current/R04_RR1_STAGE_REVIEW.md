# R04 RR1 阶段终审报告 — STAGE-REVIEWER-R04-RR1

- 审查者：**STAGE-REVIEWER-R04-RR1**（全新一次性代理，未参与本轮 RR1 的任何执行、修复、自查或组审；G01–G05 及其 R1 组审均与我无关）。
- 审查日期：2026-10-01（UTC）。平台：macOS darwin 27 arm64 本机；`~/.cargo/bin/cargo` 1.98.1（rustup 锁定），全部命令带 `--manifest-path rust/Cargo.toml --locked`。
- 对象：对抗性复审基线（缺陷存在的旧树）`773d5a6968b87650cd135cc5684707f81d1acda0` → 本轮修复冻结候选 `629e15b85cf54ff20673b24150cc00e4e4366f61`（分支 `codex/rust-tauri-migration`，G01=1692d2314 → G02=da15c4bd9 → G03=614fab1af → G04=1285c3bf6 → G05=629e15b85，提交链亲核）。
- 纪律：主树产品/测试/配置/阶段图/账本零修改（结束时 `git status --porcelain` 仅余我的证据目录 `?? artifacts/rust-tauri/R04/RR1-STAGE-R1/`）；复测输出只写该目录；旧树/候选/负向复跑在 /tmp 三个独立 git worktree（已用后全部 `git worktree remove --force`，自有进程与临时目录逐一清理并复核）。组审报告（G0x-E01/G0x-R1）只作线索，未转抄其任何日志为本报告结果。
- 证据根：`artifacts/rust-tauri/R04/RR1-STAGE-R1/`（`logs/`、`verify-R04/`、`verify-R03/`、`probe-source/`）。

```text
STAGE_VERDICT: PASS
```

（判定依据见 §2–§8；发现的问题全部为非阻塞观察项，见 §9。）

## 1. 实际执行的命令与退出码（全部本人真实运行）

### 1.1 五组反例旧红新绿（A / CLOSE-C02 核心——本人自写反例，非执行者形态）

我编写了一份**两树共用 API** 的反例测试（`probe-source/zz_stage_r1_counterexamples.rs`，5 个集成反例）+ 一个仅追加在 worktree 副本内的 `#[cfg(test)]` 单测模块（`probe-source/zz_stage_r1_f04_mod.rs`）。同一份源码分别编译进旧树 worktree（773d5a696）与候选 worktree（629e15b85）：运行时红＝缺陷实证，编译差异与断言无关。`ToolSuccess.status` 的 G03 装箱差异用本地扩展 trait 兼容（两侧断言逐字相同）。

| 反例 | 旧树 773d5a696（红） | 候选 629e15b85（绿） | 证据 |
|---|---|---|---|
| F01 登记满拒绝必须零执行（live_cap=1，第二 one-shot 写哨兵，经真实网关 prepare→execute→真实 supervisor） | **exit 101**：哨兵 sha256 被改写（`7e8f4d94…`≠`678bf03b…`）——被拒命令真实执行；错误词为旧合并形态 `EXEC_SPAWN_FAILED: … registry is full` | **exit 0**：1s 观察窗哨兵恒 BASELINE、注册表仍 1、拒绝含 supervisor 容量词且**不含**网关 `prepared-invocation registry` 词 | `logs/old_red_integration.log`（exit 文件 `.exit`=101）/ `logs/cand_green_integration.log`（exit=0） |
| F02 终态后 settled collector 必须冻结（直接子自然退出，孙进程持 stdout 写端 1.0s 后慢写；stdio_grace=250ms） | **exit 101**：终态后 total_bytes **0→7**（`LATE_A\n` 被脱管泵追加）——`timeout(grace,pump)` 只丢 JoinHandle，泵脱管继续更新已结算采集器 | **exit 0**：两快照逐字节相等（abort+join 观察后冻结） | 同上两文件 |
| F03 清理未确认不得报 exit（孙进程 setsid 逃组持两端；cleanup_timeout=300ms < stdio_grace=1500ms；timeout=1s 触发真实 terminate） | **exit 101**：结果 `ToolRunStatus::Exited{code:137}` 且文本含 "Command exited with code 137"——**从未观察退出却虚构 137** | **exit 0**：status 非 Exited、文本无 "exited with code"（候选映射为 StopUnconfirmed——正向词汇由门禁内 `r04_rr1_f03_stop_honesty` 7 用例钉住） | 同上两文件 |
| F04 混合 chunk 逐字节恰一次（单测：append `[0x61,0xE6,0x97]`→poll→空闲 poll→append `[0xA5]`→poll） | **exit 101**：held-back 2 字节被计入 dropped（left 2/right 0）；（若继续）空闲 poll 将重复投递 `a`、补齐后 `a日` 再整块重投 | **exit 0**：`a`、空、`日`，拼接恰 `a日`，dropped=0 | `logs/old_red_f04_unit.log`（exit=101）/ `logs/cand_green_f04_unit.log`（exit=0） |
| F05a 尾窗丢头不得称完整（150 027B 单行 / 默认 100KiB 滚动窗 / max_output_bytes=200 000，真实 bash+spill） | **exit 101**：`HEAD_MARK_9f3a` 不在返回文本（真头已被淘汰）而 truncated=false | **exit 0**：HEAD+TAIL 可见，truncated=false 时全流字节精确 | 同 1.1 前两文件 |
| F05b 封顶 spill 不得称 Full（100 023B 单行 / spill_cap=64KiB / 预算 30 000） | **exit 101**：文本声明 `Full output: <路径>` 且 ResourceRef 显示名 `exec_command full output`，实际文件 65 536 < 100 023（幽灵完整文件） | **exit 0**：文本为 `Full output: unavailable — … capped`，ref 显示名 `exec_command partial output (… — not the full output)`，无正向 full 声明 | 同上 |

候选稳定性：集成反例另做 2 次复跑（共 3 次全绿，`logs/cand_green_integration_run{2,3}.log`）。过程披露（如实）：我的 F05b 断言第一版用子串匹配 "full output"，误命中候选的**否定式**文案 "not the full output"，属我测试自身缺陷；改为锚定 `exec_command full output` 前缀后，同一份最终源码在两树重跑取证（上表即最终源码结果）。

### 1.2 原合同门禁（B / CLOSE-C01）

| # | 命令 | 退出码 | 结果摘要 | 证据 |
|---|---|---|---|---|
| B1 | `cargo run --locked -p xtask -- verify-stage R04 --evidence artifacts/rust-tauri/R04/RR1-STAGE-R1/verify-R04/` | **0** | overall=**PASS**；candidateSourceBinding **stable=true**，testedSha=`629e15b85…`（与冻结候选逐位一致）；runnerSourceBinding=PASS；worktreeDirty=true（仅我的未跟踪证据文件，tracked 绑定稳定；与 T08/G05 同口径如实记录）。**8/8 命令 exit 0**（workspace 516s / tool_matrix 313s / fmt 13s / clippy 13s / contracts 13s / boundaries 13s / r03_regression_gate 935s / rr1_repair_suites 42s）；**24/24 场景 PASS**（16 A-ID + SUP01/03/05 + 5 个 R04-RR1-F*）；**124 叶 = 55 PASS + 69 DEFERRED_TO_LATER_STAGE**（递延不动）；内嵌 R03 回归 **overall PASS、17/17 场景、15/15 命令**；门禁内 RR1 生产者 **32 运行全绿、22 C-ID 全绿（63 测试）** | `verify-R04/verify-stage-result.json` 及目录 |
| B2 | 负向抽查：隔离副本（/tmp worktree @629e15b85）从 R04.json 删除场景 `R04-RR1-F03`（24→23）→ 同命令完整重跑 | **1** | overall=**FAIL**：rust_test_workspace exit 101（见 §9 OBS-1 的归因注记），结果中该场景消失、23 个场景非 PASS；**删除被钉测点名**：`stage_map::map_tests::r04_production_map_registers_the_rr1_repair_scenarios` 单独运行 **exit 101**，消息 `R04 map dropped the RR1 repair scenario R04-RR1-F03 — the F01-F05 adversarial-repair counterexamples would leave the formal acceptance` | `logs/negative_full_gate.log`、`logs/negative_verify_result.json`、`logs/negative_pin_test.log`（exit=101） |

### 1.3 R03/R02 回归（C / CLOSE-C03）

| # | 命令 | 退出码 | 结果摘要 | 证据 |
|---|---|---|---|---|
| C1 | `cargo run --locked -p xtask -- verify-stage R03 --evidence artifacts/rust-tauri/R04/RR1-STAGE-R1/verify-R03/` | **0** | overall=**PASS**；stable=true（testedSha=629e15b85）；**15/15 命令 exit 0**（含 **R02 定向链 7/7**：auth_matrix / storage_tx / events_matrix / backup_restore / recovery_drill / full_chain / legacy_regression）；**17/17 场景**；48 叶 = 17 PASS + 31 DEFERRED | `verify-R03/verify-stage-result.json` |
| C2 | R03 保护抽验（逐测试二进制直接运行） | 全 **0** | cancel_terminal_race 13/13；request_dedup 4/4；request_id_canonicalization 7/7；tool_receipt_unknown 6/6（取消/终态统一裁决、请求去重/规范化、Unknown 不盲重试均不退化） | `logs/r03_protection_spot_checks.log` |

（R02 定向链另在 B1 的内嵌 R03 回归中二度过绿：`verify-R04/R03_REGRESSION/` 下 7 条 r02 命令全 exit 0。）

### 1.4 间歇项第三方观测（E）

| 项 | 我的观测 | 证据 |
|---|---|---|
| A10（PTY ^C 观察）孤立运行 | **10/10 绿**（0.57–0.61s/次）；且在 B1/C1 两次全量门禁的 workspace 腿中随套件绿（r04_t05_process_tools 14/14） | `logs/a10_isolated_observations.log` |
| terminal-snapshot（`terminal_family_share_cases` 内 `terminal-snapshot-current-transcript`） | 孤立 **5/5 绿**；B1 全门禁 r04_tool_matrix exit 0，该 case `recordedOk=true, actual=1`（矩阵 56/56 allCasesOk） | `logs/terminal_snapshot_flake_probes.log`、`verify-R04/R04_MATRIX/` |

### 1.5 范围与纪律核验（F，只读 git 取证）

- `git diff --name-only 773d5a696..629e15b85`：非 artifacts/docs 的改动**恰为 19 个文件**（lingxi-kernel/ports.rs、lingxi-service/{exectools,procsupervisor,runs,lib}.rs、6 个新 RR1 测试、4 个既有测试的等价适配、xtask stage_map.rs + R04.json、scripts/rust-tauri 的 2 个新脚本 + 生成器 + 负向电池 overlay 一行）；docs 侧仅 ORCHESTRATOR_PROGRESS.json（总控重开记录）、R04_TEST_MAP.json（RR1 条目）与 repair-current 报告；**无 desktop/、无 R05、无 tauri 壳、无 skills2set/ 改动**。
- **Cargo.lock 零变化**（diff 0 行）。
- **旧测试零删除**：r04_t05_process_tools.rs 41→41 个 fn、r04_t08_tool_matrix.rs 39→39（comm 集合差为空）；全 tests/*.rs 文件清单差为空；src 内被移除的函数仅 `register(`（F01 修复本体：拆为 reserve_live_slot+LiveSlot::commit）与 `status_code(`（F03 修复本体：改 `observed_status_code()->Option<i64>`，类型层杜绝虚构，消费测试等强度适配）。
- **递延不动**：R04_HANDOFF.json 未被触碰（deferred_registrations 3 条含 t05-obs2-registryfull-tail 原样保留为历史）；两次 R04 门禁均 69 叶 DEFERRED；stage_map 124 叶 split 钉测绿。

## 2. A｜五组反例旧红新绿 — 成立

见 §1.1。五组反例（F01 容量、F02 泵监督、F03 未确认、F04 混合 chunk、F05 窗口/封顶）全部由**我自写的、两树同源的反例**在旧树取得运行时红（各退出码 101 与失败原文存证）、在候选全绿（exit 0，3 次稳定）。反例面向真实链路：F01/F03/F05 经真实 ToolInvocationGateway（prepare→execute）→真实 ProcessTools→真实 ProcessSupervisor；F02 直驱真实 supervisor；F04 为纯逻辑单测（总控允许的证据级别），其真实 PTY 链由门禁内 `r04_rr1_f04_pty_consumption` 覆盖。**CLOSE-C02：满足。**

## 3. B｜原 R04 门禁不降标 — 成立

- 正向（B1）：24/24 场景（原 16 A-ID + SUP + 5 RR1）、8/8 命令、stable 绑定到冻结候选；RR1 生产者 32 运行/22 C-ID/63 测试全绿且逐 C-ID 归属核验通过（生产者脚本 `scripts/rust-tauri/r04_rr1_g05_repair_suites.sh` 亲读：pin 表精确计数、0 匹配拒绝、C-ID 唯一归属、缺口点名 exit 1）。
- 负向（B2）：删一个 RR1 场景 → 完整 verify-stage **exit 1 / overall FAIL**，且 xtask 钉测按名点出被删场景——门禁非空壳。**CLOSE-C01：满足**（16 原合同义务经 tool_matrix 56 案例 + workspace 88 套件在本次全新证据根真实重跑）。

## 4. C｜R03/R02 回归 — 成立

独立 R03 门禁 PASS（17/17、15/15 含 R02 七链），加内嵌回归二度过绿与四条保护抽验全绿。**CLOSE-C03：满足。**

## 5. D｜七项接受依据纠正核验 — 全部有事实支撑

（旧依据原文保留不改；下列"旧树行号"均指 773d5a696 版本，候选行号指 629e15b85。）

| # | 旧依据（错误） | 本轮事实 + 证据指针 |
|---|---|---|
| 1 | 进程登记满被当作网关 `PreparedRegistryFull`（R04_FINAL_STAGE_REVIEW_R1.md O-1 引 T05-R1-OBS-2） | 两个容量：旧树 `register()` 在 `command.spawn()` **之后**才查 live_cap（procsupervisor.rs:824 定义、:978 one-shot / :1144 PTY 调用）；网关 prepared cap（DEFAULT_LIVE_PREPARED_CAP=1024）无关。候选词汇三分：`EXEC_PROCESS_REGISTRY_FULL`（exectools.rs:118-122 注释明示区分）/`EXEC_SPAWN_FAILED`/网关 `PreparedRegistryFull`；我的 F01 反例双重断言（是前者、非后两者）。 |
| 2 | "属罕见路径"（同上 O-1） | 满容量常规可触发：live_cap=1 + 一个运行中受控进程即可**确定性**触发旧树"派发后拒绝"，无需竞态——我的 F01 旧红（哨兵被覆写）即为无竞态实证。 |
| 3 | `timeout(grace, pump)` 被描述为"超限 abort 泵任务"（R04-T05_REPORT.md §2） | tokio 语义：丢弃 JoinHandle＝detach。旧树源码 `timeout(grace, pump).await.is_err()`（:1000）消费并丢弃句柄，泵继续运行——F02 旧红（终态后 collector 0→7）为行为实证；候选 `drain_output_tasks`（procsupervisor.rs:2469-2567）abort 后 join 观察实际结束，EndUnconfirmed 如实记录。 |
| 4 | "记录非终态 ⇔ 子进程未 reap ⇔ PID 不可复用"（R04-T05_REPORT.md §2） | 旧树 reaper 先 `child.wait()`（reap 发生）再进 drain 宽限，期间记录仍 Running/Terminating，`mark_terminating`（旧 :1479-1508）与 Drop（旧 :1463）仍按陈旧 pgid 发 killpg。候选以 `child_reaped` 分离建模（note_child_reaped :2336，wait 解析后、grace 前发布）+ 发送时所有权门 `prove_group_ownership`（:2369，锁内 child_reaped+实时 getpgid），门内才发信号、否则审计 `group_signal_skipped`；门禁内 `rr1_f02_c02_*` 三腿+2 单测钉住。 |
| 5 | CleanupTimedOut 被折算为已退出 137（旧 exectools.rs:718 `_ => ExitFact::Signal(9)`、:744 结果映射；write_stdin 旧 :948 `CleanupTimedOut => Some(ToolRunStatus::Exited{code:137})`） | 我的 F03 旧红：ExitFact 虚构链在结果面产出 `Exited{137}`+文本 "exited with code 137"，而退出从未观察。候选 `terminal_facts`（:2656）对 terminal-without-fact 返回 None、`await_termination` 不升级（:2154-2176 unconfirmed_echo）、kernel `ToolRunStatus::StopUnconfirmed`（ports.rs:1063）、exec/write_stdin/journal 三面诚实映射。 |
| 6 | 逐 chunk seq 推进被当作字节消费（旧 deliver_since_cursor :626-680，cursor 只跨完整消费 chunk，:665） | 我的 F04 旧红：部分消费 chunk 整块留队列→前缀重投+暂存计 dropped（旧 :681 `dropped + (joined.len()-consumed)`）。候选 `TranscriptChunk{consumed}` 字节偏移（:893-899）——同一字节至多消费一次；四态分立（pending/replaced/dropped/consumed）。 |
| 7 | 尾窗未再次截断＝原输出完整（旧 exectools.rs:759 `truncated = head_tail.truncated`，:740 仅呈现 `decode_window(&snapshot.window)`） | 我的 F05a 旧红：真头已淘汰而 truncated=false。候选 `truncated = head_tail.truncated \|\| evicted_bytes > 0`（exectools.rs:980）+真头窗（head_cap=window_cap=100KiB/进程，内存有界）+`lost_middle_bytes` 派生事实；Full 声明四态（:610-629），封顶/失败/缺文件不得称 Full。 |

## 6. E｜不一致观察裁决（如实记录）

- **A10**：G01-E02 称孤立确定性红（3/3–5/5）；G02-E01（16/16+20/20 绿）、G02-R1（10/10 绿）与我（**10/10 孤立绿 + 两次全量门禁随套件绿**）三方观测一致反对。裁决：G01 的红主张在本机当前条件**不可复现**，无缺陷实证；其归因方向（macOS readline 在 SIGINT 后丢弃 type-ahead 的负载相关时序）不被排除，但不足以构成候选阻塞。维持"未复现、不虚构缺陷、不做无据修复"的登记。
- **terminal-snapshot flake（G05 首跑）**：G05 归因为重载下 PTY 快照轮询错过窗口（该次 `observed 0`，已存档 attempt1）。我的复核：孤立 5/5 绿 + 本次全门禁该 case recordedOk=true——**未复现**，归因（负载相关间歇）与本机观测相容；该测试非本轮改动对象，维持既有间歇项登记，非候选阻塞。

## 7. 我抽验的 26 C-ID 覆盖说明

- **本人自写反例直接复验**：F01-C01、F02-C01、F03-C01、F04-C01+C02（同一反例覆盖两查）、F05-C01、F05-C03（共 7 个 C-ID 的核心反例形态）。
- **本人门禁内真实重跑覆盖**：B1 门禁的 `r04_rr1_repair_suites` 生产者在本轮全新证据根执行 32 运行/63 测试，**22 个 C-ID 全部 ok**（`verify-R04/R04_RR1_REPAIR/rr1-cases.json`：allSuitesOk=allCasesOk=true）——即 26 C-ID 中的全部 22 个五 F 检查由我在本次审查中亲自重跑过；其余 4 项为收口检查 CLOSE-C01..C04，状态见 §2-§4 与 §8。
- 剩余深入对抗面（如 F01-C02 屏障并发、F02-C04 压力稳态、F03-C05 Run 取消链）由上述门禁运行中的对应测试承载，我未逐个单独复跑——非缺口，属抽验边界，如实声明。

## 8. 四项收口检查状态

| ID | 状态 | 依据 |
|---|---|---|
| CLOSE-C01 原合同不降标 | **满足**（我执行） | §1.2-B1 正向 + B2 负向（删除→FAIL+点名） |
| CLOSE-C02 独立反例旧红新绿 | **满足**（我执行） | §1.1（自写反例，两树同源） |
| CLOSE-C03 R03 回归及范围 | **满足**（我执行） | §1.3-C1/C2 + 内嵌二度过绿 |
| CLOSE-C04 纠正接受依据并可追溯 | **事实核验完成**：七项纠正全部有本轮事实与证据支撑（§5），历史报告原样保留；现行接受记录/交接/回执的**追加**属总控收口职责（本审查报告即其输入），不由本审查者代写 | §5、§1.5 |

## 9. 发现的问题（全部非阻塞观察项，无 FAIL 级）

| ID | 严重度 | 定位/现象 | 说明与建议 |
|---|---|---|---|
| SR-OBS-1 | Info | `r04_t06_sandbox.rs` A12（`r04_a12_filesystem_write_isolation_holds_with_real_sentinels`，本轮未改动的既有测试）在我的 /tmp worktree 副本中失败（"the write must fail: (no output)"），同一测试在主树本次所有运行（含两次全量门禁）均绿 | 副本位于 /tmp（/private/tmp）时 sandbox 腿环境敏感；不影响候选判定（主树全绿、与修复无关），但提示**负向/隔离副本验证尽量不用 /tmp 路径**，或后续为 A12 增加路径鲁棒性。与 G05 电池在 /tmp 副本上偶发挂起的现象同族 |
| SR-OBS-2 | Info | verify-stage 拒绝跨越 /tmp 符号链接的 evidence 根（"crosses symlink /tmp"）——负向首跑因此未启动，改用副本内相对路径后正常 | 预期内的 fail-closed 绑定保护，记录供后续审查者少走弯路：隔离副本的 evidence 必须放副本内 |
| SR-OBS-3 | Info | 我的门禁运行 worktreeDirty=true | 仅我的未跟踪证据文件所致；tracked 候选绑定 stable=true（testedSha=629e15b85），与 T08/G05 已接受口径一致 |
| SR-OBS-4 | Info | 我自写 F05b 反例的首版断言子串误匹配候选否定式文案 | 我方测试缺陷，已修正并以最终源码两树重跑取证（§1.1 披露）；候选无涉 |
| SR-OBS-5 | Info | 平台边界 | 本审查全部在 macOS arm64 真机执行；Windows/Linux 真机、正式打包与真实供应商验证维持既有平台递延（69 叶不动），本地结果不替代 |

## 10. 结论

- 五组反例旧红新绿**成立**（自写反例、两树同源、退出码与失败原文存证）。
- 原 R04 门禁与 R03/R02 回归**真实通过**（stable 绑定冻结候选 629e15b85；负向删除抽查 FAIL+点名，门禁非空壳）。
- 七项接受依据纠正**全部有事实支撑**（旧依据错误定位到行，候选修复与行为证据对应）。
- 范围与纪律核验通过：无产品范围扩张、无 R05 倒灌、无旧测试删除、Cargo.lock 零变化、递延登记未消费未删除。
- 不一致观察（A10、terminal-snapshot）经第三方观测均未复现缺陷，如实登记，不构成阻塞。
- 无未修复的阶段级阻塞项。

**STAGE_VERDICT: PASS**
