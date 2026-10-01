# R04 RR1 修复轮 G05-E01 — 新增验收场景接入正式门禁与门禁完整执行报告

- 工作单：G05-E01（CLOSE-C01 执行部分 + CLOSE-C03 的 R03/R02 回归执行部分）
- 日期：2026-10-01/02
- 基线：HEAD 1285c3bf6（G01–G04 四个修复已合入），分支 codex/rust-tauri-migration，工作区唯一写者；Cargo.lock 零改动
- 角色：一次性执行代理；本报告不更新接受记录/交接/账本（总控收口职责），不自行宣布独立 PASS
- 输入：/tmp/r04_repair_package/Lingxi_R04_Review/ 总控（§8 门禁与候选纪律、CLOSE-C01..C04）与 R04_RR1_修复验收清单.json closure_checks
- 工具链：~/.cargo/bin/cargo（rustup 1.98.1 锁定，PATH 前置；Homebrew cargo 1.93.0 不读取锁定）

## 0. 结论摘要

26 项验收检查中的 22 个五 F C-ID（F01×4 + F02×4 + F03×5 + F04×4 + F05×5）已全部作为正式场景/案例接入 R04 阶段门禁；其余 4 项（CLOSE-C01..C04）为收口检查，按总控口径不入阶段图（C01 执行部分与 C03 回归执行部分即本工作单）。七条门禁命令（a–g）在最终候选树上全部真实执行、退出码全 0 落盘；既有负向门禁 9/9 失败关闭复验通过；新增 RR1 负向 4/4 失败关闭（含删 RR1 场景后的完整 verify-stage R04 FAIL）。原 16 A-ID、SUP01/03/05、124 叶（55 share + 69 deferred）零删除零削弱，只增不减；递延叶不动。

**READY_FOR_REVIEW**（独立阶段重审与接受记录修订由总控派单；本代理未 commit/push）。

## 1. 接线清单（新增场景 → 命令 → 案例映射）

### 1.1 生产者命令（R04.json 新注册，经生成器）

| 命令 key | argv | timeout | 证据 |
|---|---|---|---|
| r04_rr1_repair_suites | bash scripts/rust-tauri/r04_rr1_g05_repair_suites.sh {EVIDENCE}/R04_RR1_REPAIR | 2400s | {EVIDENCE}/R04_RR1_REPAIR/rr1-cases.json、summary.txt |

生产者内嵌两张机器可核对表（同时落盘 pin-table.txt / cid-table.txt 为证据）：

- **pin 表（32 运行）**＝6 个集成套件（37 测试）+ 26 个 lib 精确名单测（`cargo test --lib <全名> -- --exact`）。逐运行核对：过滤器 0 匹配、`running N`≠pin、`passed`≠pin、任一失败/忽略＝点名进 gaps.txt 后 exit 1。
- **cid 表（22 C-ID / 63 测试名）**：每个测试名必须在其 run 日志中有字面 `test <名> ... ok` 行；每名恰归属一个 C-ID（双重归属=红）；归属并集==执行并集（孤儿执行/幽灵归属=红）；cid 引用的 run 必须在 pin 表且其 F-ID 与 C-ID 前缀吻合。

### 1.2 新场景（R04.json：19 → 24，全部 REQUIRED，只增不减）

| 场景 | commandRefs | 覆盖 C-ID（各自测试数） |
|---|---|---|
| R04-RR1-F01 | [r04_rr1_repair_suites, rust_test_workspace] | C01(3) C02(3) C03(5) C04(1) |
| R04-RR1-F02 | 同上 | C01(4) C02(5) C03(3) C04(1) |
| R04-RR1-F03 | 同上 | C01(2) C02(1) C03(1) C04(3) C05(2) |
| R04-RR1-F04 | 同上 | C01(2) C02(1) C03(7) C04(1) |
| R04-RR1-F05 | 同上 | C01(5) C02(5) C03(4) C04(2) C05(2) |

### 1.3 C-ID → 真实测试映射（63 测试，与 G01–G04 各报告的 C-ID 证据映射逐一对应）

F01（r04_t05_registry_capacity 9 集成 + 3 lib 单测 live_slot_*）：
- C01：repro_rr1_f01_registry_full_refusal_must_not_execute_the_command、rr1_f01_c01_registry_full_refuses_with_zero_dispatch + lib live_slot_reservation_enforces_the_cap_atomically
- C02：rr1_f01_c02_{two_one_shots, two_ptys, one_shot_and_pty}_race_the_last_slot_through_a_barrier（屏障固定交错，非随机重跑）
- C03：rr1_f01_c03_{one_shot_group_verify_failure_compensates_exactly_once, pty_group_verify_failure_closes_the_pty_and_returns_the_slot, pty_open_failure_is_zero_dispatch_and_returns_the_slot} + lib live_slot_commit_transfers_release_to_settle_exactly_once、live_slot_drop_after_panic_style_abandon_still_releases
- C04：rr1_f01_c04_same_process_usable_after_repeated_refusals_and_releases

F02（r04_rr1_f02_reaper_cleanup 11 集成 + 2 lib 单测 group_signal_*）：
- C01：rr1_f02_repro_grandchild_pump_outlives_the_grace_and_mutates_the_settled_collector、rr1_f02_c01_grandchild_holding_both_ends_freezes_the_result_and_closes_the_read_ends、rr1_f02_c01_pty_family_grandchild_holding_the_slave_closes_the_master、rr1_f02_c01_adversarial_slow_writer_cannot_mutate_the_settled_collector
- C02：rr1_f02_fast_exiting_child_is_not_refused_by_the_reap_race、rr1_f02_c02_terminate_in_the_reaped_undrained_window_signals_nothing、rr1_f02_c02_adversarial_window_offsets_never_signal_a_stale_group + lib group_signal_is_skipped_{once_the_child_reap_is_published, when_the_kernel_identity_disagrees}
- C03：rr1_f02_c03_double_stuck_pumps_complete_within_the_single_budget、rr1_f02_c03_adversarial_panicking_pump_is_observed_and_honest、rr1_f02_c03_adversarial_exit_and_cancel_racing_never_signals_unprovably
- C04：rr1_f02_c04_stress_cycles_return_to_the_declared_steady_state

F03（r04_rr1_f03_stop_honesty 7 集成 + 2 lib 单测）：
- C01：rr1_f03_c01_timeout_cleanup_unconfirmed_never_reports_an_exit + lib terminal_facts_never_fabricate_an_exit_for_unconfirmed_or_live_phases
- C02：rr1_f03_c02_pty_cleanup_unconfirmed_poll_agrees_with_the_text
- C03：rr1_f03_c03_repeated_terminate_does_not_upgrade_unconfirmed_to_confirmed
- C04：rr1_f03_c04_control_group_real_states_stay_accurate、rr1_f03_c04_adversarial_cancel_racing_natural_exit_stays_real + lib exit_fact_status_codes_follow_the_shell_convention
- C05：rr1_f03_c05_run_cancel_keeps_control_flow_and_external_unconfirmed_separate、rr1_f03_c05_adversarial_dropped_tool_future_keeps_the_honest_chain

F04（r04_rr1_f04_pty_consumption 1 集成 + 10 lib transcript 单测；C01–C03 按总控"真实 Rust 纯逻辑测试"口径即 lib 单测）：
- C01：lib transcript_mixed_chunk_prefix_is_consumed_byte_exactly_r04_rr1_f04_c01 + 旧分块控制组 transcript_delivers_split_multibyte_characters_intact
- C02：lib transcript_idle_polls_after_a_partial_delivery_hold_back_without_loss
- C03：lib transcript_property_{valid_input_reassembles_exactly, invalid_bytes_replaced_exactly_once, eviction_accounting_counts_only_real_loss}、transcript_ring_overflow_while_holding_back_counts_only_real_evictions、transcript_boundary_across_many_chunks_with_interleaved_polls、transcript_force_delivery_flushes_a_dangling_partial、transcript_ring_drop_of_undelivered_is_counted_honestly
- C04：f04_c04_real_pty_handshake_delivers_mixed_bytes_exactly_once（真实 posix_openpt+bash+网关全链握手，无 sleep 碰绿）

F05（r04_rr1_f05_output_integrity 7 + r04_rr1_f05_spill_failure 2 集成 + 9 lib exectools 单测；镜像 G04-E01 报告 §C-ID 表）：
- C01：f05_c01_hidden_window_prefix_loss_is_flagged_not_hidden、f05_c01_control_small_stream_big_budget_distinguishes_facts、f05_c01_big_stream_marks_eviction_and_keeps_true_head_and_tail + lib assemble_retained_output_{small_stream_is_whole_and_exact(对照), evicted_middle_is_counted_and_marked}
- C02：f05_c02_single_multibyte_line_keeps_head_and_tail_content + lib truncate_head_tail_{single_huge_ascii_line_keeps_head_and_tail, multibyte_line_cuts_on_char_boundaries, newline_only_at_end_keeps_both_markers, small_output_stays_whole(对照)}
- C03：f05_c03_capped_spill_is_partial_and_honest + lib transcript_spill_claim_vocabulary_is_state_exclusive、full_output_claim_vocabulary_is_state_exclusive、spill_resource_ref_never_claims_full_when_capped_or_failed
- C04：f05_c04_spill_write_failure_diagnoses_honestly_no_ghost_full_file、f05_c04_unwritable_spill_dir_reports_no_spill_kept
- C05：f05_c05_high_output_and_many_pty_polls_stay_bounded、f05_c05_cancelling_mid_output_closes_the_spill_and_bounds_everything

### 1.4 xtask 侧同步（stage_map.rs，只增不减）

新增 3 个钉测（随 rust_test_workspace＝门禁自身命令运行）：
1. `r04_production_map_registers_the_rr1_repair_scenarios` — 逐 id 钉 5 场景（删除即点名 "R04 map dropped the RR1 repair scenario <id>"）→ 再钉总场景数=24（多发明的场景同样红）→ 钉命令 argv/evidencePaths 与生产者脚本存在。
2. `r04_rr1_producer_pin_table_matches_the_registered_suites` — R04_RR1_PIN_TABLE（32 项）与脚本 pin 表逐行精确相等。
3. `r04_rr1_producer_case_table_owns_every_check_id_exactly` — R04_RR1_CASE_TABLE（22 C-ID+计数）精确相等；run↔F-ID 交叉；无测试名双重归属；owned 63 == 执行 63。

既有 R03/R04 钉测（16 A-ID 逐字、SUP 场景、124 叶 split、56 矩阵案例、案例期望值、R03 十套件 pin 表）零改动，全部保持绿。

### 1.5 生成器幂等复现

- r04_t08_generate_stage_map.py 追加命令+5 场景+coverage note 一句（沿用 T08 既有机制，无第二套生成器）。全部编辑结束后连续两次运行输出与登记 R04.json 逐字节一致（sha256 35888cd868f067f8f87c15a9410217b30040db29866e7e439194cf5e716cae86，git diff 对基线仅 +58/-1 行：5 场景+1 命令+note；16 A-ID/SUP/124 叶内容零变化）。

### 1.6 登记文档同步

- R04_TEST_MAP.json 追加 "R04-RR1" 任务条目（acceptance_map 五场景→测试、对抗项、门禁指针；只增不减，T01–T08 条目零改动）。
- R04_SCOPE_MATRIX / R04_PLATFORM_CAPABILITIES 未动（无范围变化）。
- r04_t08_gate_negative_tests.sh 唯一维护性改动：隔离副本 overlay 清单追加新生产者脚本（xtask 钉测会读该脚本；不带它副本即缺注册文件，破坏电池"每案只篡改一件事"语义）。

## 2. 门禁命令与退出码（日志 artifacts/rust-tauri/R04/RR1-G05-E01/logs/）

| # | 命令 | 退出码（最终候选） | 日志 |
|---|---|---|---|
| a | cargo fmt --manifest-path rust/Cargo.toml --all -- --check | 0 | a_fmt_check.log |
| b | cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings | 0 | b_clippy.log |
| c | cargo test --manifest-path rust/Cargo.toml --workspace --locked | 0（88 绿二进制 / 954 passed / 0 failed / 0 ignored；r00_management_leaves 通过） | c_workspace_test.log |
| d | cargo run … -p xtask -- check-contracts | 0 | d_check_contracts.log |
| e | cargo run … -p xtask -- check-boundaries | 0 | e_check_boundaries.log |
| f | cargo run … -p xtask -- verify-stage R04 --evidence …/RR1-G05-E01/verify-R04/ | 0 | f_verify_stage_R04.log |
| g | cargo run … -p xtask -- verify-stage R03 --evidence …/RR1-G05-E01/verify-R03/ | 0 | g_verify_stage_R03.log |

### 2.1 verify-stage R04（f）结果摘要

- overall=**PASS**；candidateSourceBinding stable=true（testedSha 1285c3bf6…，worktreeDirty=true 如实记录——工作区含本次未提交接线文件，与 T08 验收同口径）；runnerSourceBinding=PASS。
- 命令 8/8 PASS：rust_test_workspace 518s、r04_tool_matrix 313s、rust_fmt 12s、rust_clippy 13s、check_contracts 12s、check_boundaries 12s、r03_regression_gate 923s、r04_rr1_repair_suites 41s。
- 场景 **24/24 PASS**（16 A-ID + SUP01/03/05 + 5 个 R04-RR1-F*）。
- 叶 **124**：55 stage_share_satisfied 全 PASS + 69 deferred_to_later_stage（递延不动）。
- **内嵌 R03 回归整体 PASS**（R03_REGRESSION/verify-stage-result.json：overall PASS、17/17 场景、15/15 命令、48 叶=17 PASS+31 DEFERRED）。
- 门禁内 RR1 生产者输出：32 运行全绿（allSuitesOk=true）、22 C-ID 全绿（allCasesOk=true）、63/63 测试。

### 2.2 verify-stage R03（g，独立重跑）结果摘要

- overall=**PASS**；stable=true；testedSha 1285c3bf6…；15/15 命令 PASS；**17/17 场景 PASS**；48 叶＝17 PASS + 31 DEFERRED。
- **R02 定向链 7/7 绿**（r02_auth_matrix / r02_storage_tx / r02_events_matrix / r02_backup_restore / r02_recovery_drill / r02_full_chain / r02_legacy_regression）——CLOSE-C03 的受影响 R02 定向链执行部分。

## 3. 负向门禁验证（CLOSE-C01 对抗项）

### 3.1 既有负向电池（scripts/rust-tauri/r04_t08_gate_negative_tests.sh，证据 negative/）

9/9 全部失败关闭且缺口点名（case-results.json allRefused=true）：

| 用例 | 退出码 | 点名 |
|---|---|---|
| n-unknown-stage（R99） | 2 | unknown stage |
| n-empty-scenario-set | 2 | must not be EMPTY |
| n1a-delete-leaf-mapping | 1 | drops 1 REQUIRED_SUPPLEMENTAL leaf … +叶 id |
| n1b-delete-command-mapping | 2 | references unknown command r04_tool_matrix |
| c03a-stale-evidence-root | 1 | is not empty |
| n4-delete-sup-scenario | 101 | R04-SUP01（xtask 钉测红） |
| n2-zero-match-filter（全门禁） | 1 | no case fragments were produced |
| n3-missing-evidence-file（全门禁） | 1 | missing-evidence-demo.json |
| c03b-midrun-input-change（全门禁） | 1 | Candidate file bytes or HEAD changed + stable=false |

执行注记：电池进程在 8/9 用例完成后被外部 SIGKILL（非门禁失败）；C03b 由逐字复刻该用例的续跑器（/tmp/run_t08_c03b_continuation.sh，同隔离副本机制、同检查、同证据目录）补齐，最终 verdict python 对 9 行完整表重跑输出 allRefused=true。被杀电池遗留的半途 C03b 证据存档为 negative/c03b-killed-battery-partial。

### 3.2 新增 RR1 负向（scripts/rust-tauri/r04_rr1_g05_negative_tests.sh，证据 negative-rr1/）

4/4 全部失败关闭且缺口点名（case-results.json allRefused=true）：

| 用例 | 退出码 | 点名 | 证明 |
|---|---|---|---|
| rr1-n1 删 R04-RR1-F03 场景 | 101 | "R04 map dropped the RR1 repair scenario R04-RR1-F03" | xtask 钉测红 |
| rr1-n2 pin 9→8 篡改 | 101 | pin table drifted + r04_t05_registry_capacity | 镜像钉测红 |
| rr1-n3 删 R04-RR1-F05-C05 cid 行 | 101 | R04-RR1-F05-C05 + cid table drifted | C-ID 覆盖钉测红 |
| rr1-n4 删场景→完整 verify-stage R04 | 1 | overall=FAIL + rust_test_workspace FAIL + 日志含 R04-RR1-F03 | 真实全门禁非零退出 |

执行注记（如实）：本电池共跑三次。attempt1（N1 点名顺序缺陷——计数断言先于点名循环，已改为先点名后计数）与 attempt2（我并发启动两个电池导致 CPU 争用、subagent_closeout 一腿挂起 40 分钟超时；门禁仍失败关闭 exit 1）均已存档（negative-rr1-attempt1 / -attempt2-concurrent-hang）；上表为干净串行重跑的最终结果。attempt2 的超时再次如实暴露：负向 N4 的全门禁腿在并发负载下存在挂起敏感面（该测试单跑与各正式门禁均绿），不影响正式门禁结论。

## 4. 份额/结构变化核对

- 场景：19 → 24（+5 RR1-F）。原 16 A-ID 与 SUP01/03/05 的 id/requirement/commandRefs 逐字未动（xtask 钉测 + 生成器 diff 双重核对）。
- 叶：124 = 55 share + 69 deferred 逐叶未动（r04_production_map_keeps_the_124_leaf_split 绿；生成器 diff 叶区块零变化）。
- 命令：7 → 8（+r04_rr1_repair_suites）。
- 测试数量随实际增加同步更新（workspace 954 passed；RR1 生产者 63 测试），未死守旧数（888 为历史口径），未删任何旧测试。

## 5. 间歇项与未验证项（如实）

1. **terminal-snapshot 间歇 flake（既有）**：f 的第一次运行（verify-R04-attempt1-flaky-terminal-snapshot，已存档）中，内嵌 R03 回归的 workspace 腿里 r04_t08_tool_matrix 的 `terminal_family_share_cases` 报 "terminal-snapshot-current-transcript: pinned expectation 1 did not hold (observed 0)"——PTY 快照轮询在重载下错过窗口。隔离复跑（--exact 单测）0.21s 绿；同一矩阵随后在正式 f/g 的多条腿全绿。非本次接线引入（本单未触碰该测试），登记为既有间歇项。
2. **r00_management_leaves**：本单所有运行（c、f×2、g×2、内嵌 R03×2）均通过，未触发已知防火墙间歇拦截项。
3. **负向 N4 并发挂起**（见 §3.2 注记）：subagent_closeout 的 parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap 在两门禁并发时挂起超时；单跑与正式串行门禁均绿。
4. **平台边界**：RR1 五套件与门禁仅在 macOS/arm64 真机验证（与整个 R04 阶段口径一致）；Windows/Linux 真机验证仍按既有平台递延，本单无平台范围变化。
5. CLOSE-C01 的"接受提交绑定/清单映射回执"部分、CLOSE-C02（独立反例旧红新绿）与 CLOSE-C04（接受依据修订）不在本单职责内，属总控收口。

## 6. 交付物清单（基线 1285c3bf6 之上的全部改动；未 commit）

- scripts/rust-tauri/r04_rr1_g05_repair_suites.sh（新，注册生产者）
- scripts/rust-tauri/r04_rr1_g05_negative_tests.sh（新，RR1 负向电池）
- scripts/rust-tauri/r04_t08_generate_stage_map.py（+命令 +5 场景 +note；幂等复现两次验证）
- rust/crates/xtask/src/stage_maps/R04.json（生成器输出，+58/-1）
- rust/crates/xtask/src/stage_map.rs（+3 钉测 +R04_RR1_PIN_TABLE/R04_RR1_CASE_TABLE 两常量）
- scripts/rust-tauri/r04_t08_gate_negative_tests.sh（overlay 追加生产者脚本，唯一维护性改动）
- docs/rust-tauri/R04/R04_TEST_MAP.json（+R04-RR1 条目，只增不减）
- artifacts/rust-tauri/R04/RR1-G05-E01/：logs/（a–g 与负向日志+exit-codes.txt）、verify-R04/、verify-R03/（正式记录）、verify-R04-attempt1-flaky-terminal-snapshot/、verify-R04-prereorder/、verify-R03-prereorder/、negative/（T08 9/9）、negative-rr1/（RR1 4/4）、negative-rr1-attempt1/、negative-rr1-attempt2-concurrent-hang/（存档）
- 本报告（docs/rust-tauri/R04/repair-current/G05-E01_REPORT.md，非执行性记录，最后写入）
