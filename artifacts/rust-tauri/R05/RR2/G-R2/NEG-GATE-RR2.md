# R05 RR2 WP-G（G-NEG）— 负测门禁报告（原 16 项 + RR2 新增 5 项反例交叉复验）

- 执行者：RR2 WP-G 实施智能体（2026-10-06/07）；隔离副本 /tmp（解析路径 /private/tmp）r05-rr2-g/repo＝`git clone --no-hardlinks`（只读自主仓库）+ 未提交 RR2 工作树 overlay（rust/、scripts/、docs/rust-tauri/，--exclude rust/target；交叉复验另加 tests/、.sync-audit/、node_modules），候选＝ad5ec4e98+RR2 改动。主树零注入。
- 命令骨架：`bash scripts/rust-tauri/r05_t08_negative_gate.sh /private/tmp/r05-rr2-g/negative-ev`（原 16 项自动化；证据存档副本＝G-R2/negative-ev/）；RR2 新增反例逐项手工注入（注入方式与原验收不同，见 §2）。
- cargo 一律绝对路径 `/Users/study_superior/.cargo/bin/cargo`；CARGO_NET_OFFLINE=true；未 commit/push。

## 0. 执行事故与处置（如实）

1. **首跑作废**：attempt-1 把证据根放在 `/tmp/...`，而 `/tmp` 是符号链接（→/private/tmp），verify-stage 的候选绑定围栏拒绝跨越符号链接的证据路径（`error: --evidence path ... crosses symlink /tmp`）——N05 因此以错误原因早退（exit 1 但点名文本不同）判 BAD，N06 起将被同因污染。处置：终止首跑（其 N01–N04 四项已 OK，结果一致），证据根改用 `/private/tmp/r05-rr2-g/negative-ev` 全量重跑。首跑证据保留于 /tmp/r05-rr2-g/negative-ev-attempt1-symlink-fence/（未拷入 G-R2）。该围栏行为本身是 xtask 的 fail-closed 设计（不是门禁缺陷），本报告以重跑的 16/16 为准。
2. **N03 锚点过期**：负测脚本 N03 的 sed 锚点 `pin svc:r05_t01_model_plane 7 ` 是 RR1 早期计数；RR1 F26 重登记后该钉为 24，锚点静默失配（变异不会发生，对照组会绿）。处置：仅在隔离副本把锚点更新为 `24 → 23`（降钉语义不变，主树脚本零改动）；镜像测试 `r05_stage_pin_table_matches_the_registered_suites` 的期望值读取仓库真实 TSV，故 24→23 篡改仍精确红。**登记：这是脚本维护债，不是候选缺陷**——建议后续把 N03 锚点改为从 TSV 动态取当前值（未在本轮主树实施，主树写入受限）。

## 1. 原 16 项（R05-GATE-N01…N16，自动化负测）

对照（同环境同副本）：xtask `r05_` 镜像测试全绿 exit 0；`svc:r05_t01_binary_wiring` 全绿 exit 0。

| 项 | 注入 diff 摘要（隔离副本内） | 判定命令 | exit | 失败原因行（点名目标） | 判定 |
|---|---|---|---|---|---|
| N01 | R05.json 删除 scenarios[R05-A16] | xtask 单测 filter `r05_production_map_keeps_the_sixteen` | 101 | 镜像单测红并点名 `R05-A16` | OK |
| N02 | r05_stage_pins.tsv：`the_buffer_bound_is_loud`→`..._XYZ`（零匹配筛选） | 完整生产者 r05_t08_stage_suites.sh | 1 | `filter matched 0 tests (running=0, pinned=1)`（gaps.txt） | OK |
| N03 | r05_stage_pins.tsv：`pin svc:r05_t01_model_plane 24`→`23`（降钉数；锚点 7→24 见 §0.2） | xtask 单测 filter `r05_stage_pin_table_matches` | 101 | 断言消息含 `svc:r05_t01_model_plane` 与 `drifted (dropped, added, or re-counted)` | OK |
| N04 | credentials/mod.rs 翻转一条断言（真失败）＋生产者 `RUN_EXIT=$?`→硬编码 0（吞退出码） | 完整生产者 | 1 | `handle_refusal_texts_carry_no_material` 点名＋`passed 0 but the stage pin is 1` 计数核对 | OK |
| N05 | 证据根预置 stale.json（旧证据复用） | verify-stage R05 | 1 | `evidence root ... is not empty; preserving its prior evidence` | OK |
| N06 | 门禁运行中向 kernel/src/lib.rs 追加注释（中途改源码） | verify-stage R02 全矩阵 | 1 | result JSON `overall=FAIL`＋`stable=False`（候选绑定前后不一致） | OK |
| N07 | R05.json 删除 commands[r04_regression_gate]（场景仍引用） | verify-stage R05 | 2 | `references unknown command`（硬解析拒绝） | OK |
| N08 | lingxi-service/src/lib.rs 接线条件 `is_none()`→`is_some()`（移除正常启动接线） | `--test r05_t01_binary_wiring` ＋ `--lib` 对照 | 101 | `c02_real_binary_full_chain_through_authenticated_endpoint` 红、`never reached`；lib 单测仍全绿（exit 0 对照） | OK |
| N09 | tool_render.rs Success 臂固定返回 `"done"` | `--test r05_t03_protocol_adapters c05_runtime_nonce` | 101 | `c05_runtime_nonce_rides_the_next_request_and_follows_changes` 红 | OK |
| N10 | openai_completions.rs tool_call_id→常量 `"call-fixed-mispaired"` | `--test r05_t01_binary_wiring` | 101 | 配对断言红点名 `call_bin_1` | OK |
| N11 | tool_render.rs StopUnconfirmed 臂压平为 `[process exited with code 0]` | adp `--lib` `running_and_stop_unconfirmed_statuses_are_honest --exact` | 101 | 诚实单测红 | OK |
| N12 | runs.rs 删除 `drop(model_permit);`（permit 不释放） | `--test r05_t06_worker_model c06_` | 101 | `c06_main_run_worker_and_callback_complete_under_global_model_concurrency_one`/`c06_a_held_global_model_permit...` 红 | OK |
| N13 | R04.json 某 deferred 叶删除 basisKind（改判形状） | verify-stage R04 | 2 | map/coverage 检查硬拒（deferred_to_later_stage/invalid stage map） | OK |
| N14 | R05.json 增设 live_probe 命令＋R05-LIVE-SNEAK 场景（私开 LIVE lane） | xtask 单测 filter `r05_map_declares_no_live_lane` | 101 | 镜像红点名 `live_probe` | OK |
| N15 | credentials.rs HandleRefused Display 拼入 `material=sk-N15-SYNTHETIC-SECRET` | svc `--lib` `handle_refusal_texts_carry_no_material --exact` | 101 | 无料单测红 | OK |
| N16 | 两次 verify-stage R02 之间改 kernel/src/lib.rs＋复用已用证据根 | verify-stage R02 ×3 | 复用腿 1 | `digestsDiffer=True`＋复用根 `is not empty` 拒收（属性判定不依赖 R02 整体结果，见脚本注释） | OK |

终局机器判定（脚本自写 case-results.json）：`{"cases": 16, "allRefused": true, "controlsGreen": true}`，脚本尾行 `RESULT: every R05 negative case failed closed with the gap named (16/16), controls green`。逐项 exit/gap-named/verdict 与上表逐一核对一致。每项原始 stdout/exit 见 G-R2/negative-ev/ `n01…n16` 子目录＋console.log；还原：`reset_copy` 每例后自 PRISTINE 快照恢复，内层 negcopy 结束于还原态（外层 /tmp 副本全部变异目标 diff -q 与主树逐字节一致，唯一差异＝负测脚本 N03 锚点行本身，见 §0.2）。

## 2. RR2 新增反例交叉复验（每项一次；注入方式与原验收不同）

| # | 注入（隔离副本 /tmp/r05-rr2-g/repo） | 命令 | 红（exit/点名） | 还原后 | 判定 |
|---|---|---|---|---|---|
| F41 删登记册 v6 | R02-T04_STORAGE_REGISTRY.json 删除 version=6 条目（7→6；原验收同注入，本轮为交叉复验） | `bash scripts/rust-tauri/r02_t04_storage_tx.sh <ev>` | exit 1：S0 `count mismatch: MIGRATIONS has 7 entries, the registry has 6`＋`v6 (model_call_usage_rr1_f21) ... MISSING from the registry`；S4 等式（真实 service 建 v7 库＋inspector migrations 对 6 条登记册）`AssertionError: userVersion 7 / supportedVersion 7 disagree with the registry's 6 migrations`（F41 原签名） | 登记册逐字节还原后 S0 `PASS: ... == registry (7 entries ...)` exit 0；完整 S0–S4 门禁绿见 §3 | PASS |
| F34 短路 unclosed | anthropic_messages.rs finish() 的 `if !unclosed.is_empty()`→`if false { let _ = &unclosed; ... }`（区别于执行者 `&& false` 与验收者整块删除/filter false 两法） | `cargo test -p lingxi-adapters --test r05_t04_rr1_batch_terminal` | exit 101：**恰新腿** `known_stop_reason_with_unclosed_tool_block_is_loud_and_dispatches_nothing` FAILED（panic 于 r05_t04_rr1_batch_terminal.rs:195，放行形态 `ToolRequests { provider_call_id: Some("t1"), {"path":"a.txt"} }`）；其余 9 腿绿（含旧腿——复现 F34 事实） | cp 还原（diff -q 一致）→ 10/10 绿 exit 0 | PASS |
| F31 列表改回 409 | management.rs list_oauth_models 合并臂 `EndpointError::not_found()`→`EndpointError::new(StatusCode::CONFLICT, ..., "oauth_only_surface")`（直接复现 RR1 409 形态；区别于 E-R2 验收的臂移除注入） | `cargo test -p lingxi-service --test r05_t02_credentials rr1_f04::rr1_f04_oauth_model_listing_and_non_oauth_rejection -- --exact` | exit 101：具名测试 FAILED `left: 409 / right: 404`（r05_t02_credentials.rs:3842） | cp 还原（diff -q 一致）→ exit 0（1 passed） | PASS |
| F42-N1 改源码 | 门禁主绑定 NOTE 出现后 3 秒向 `lingxi-adapters/src/models/credentials.rs` 追加注释（区别于 B-R2 的 lib.rs 追加；directed 模式） | `R02_LEGACY_REGRESSION_MODE=directed-no-seal-family bash r02_t08_legacy_entry_regression.sh <ev>` | exit 1：`FAIL: candidate copy does not mirror the invoking worktree (tracked+untracked content binding differs)` | 移除追加行（diff -q 与主树一致）→ directed 门禁 E0–E4.5 ALL GREEN exit 0 | PASS |
| F42-illegal（F42 附加项） | `R02_A16_RUN_OUTPUT_ROOTS=rust`（另加第二形状 `=scripts`，均区别于 B-R2 的 rust/artifacts/artifacts-rust-tauri 三形状） | 同上门禁 | 两形状各 exit 1，绑定前拒绝：`not a dedicated run-output root strictly inside artifacts/ ... forbidden`＋`declared run-output root 'rust'('scripts') is illegal (F42)` | 无需还原（纯环境变量，未改文件）；正常根绿色继承 [B-R2-5] s5-full-5 | PASS |
| F44 maxBuffer 还原默认 | tests/post-verification-audit-seal.test.ts `diffNamesSinceVerified` 移除 `maxBuffer: GIT_LISTING_MAX_BUFFER` 选项（回 node 默认 1MB） | `npx vitest run tests/post-verification-audit-seal.test.ts` | exit 1：`Error: spawnSync git ENOBUFS`（1 failed｜2 passed）——F44 原始崩溃形态复现；**分类器重放**（/tmp/ptl-verify/cls-lib.sh＝自 RR2 门禁脚本逐字提取的 extract_blocks＋classify_file）：该块判 `UNRECOGNIZED` → E5 fail-closed（非登记类，不转绿） | cp 还原（diff -q 一致）→ exit 1 但 0 处 ENOBUFS、恢复完整 `AssertionError: VERIFIED_SOURCE_SHA 之后出现了非审计文件改动...` ＋路径清单（登记 seal-coordinate-lag 形态，真实诊断可达） | PASS |

证据：/tmp/r05-rr2-g/cross/{f41,f34,f31,f44}/、/tmp/r05-rr2-g/cross/f42/（n1/、n1-green/、illegal-rust/、illegal-scripts/）；关键日志拷贝存档于 G-R2/cross/（red/green/restored 日志与 exit 码）。

## 3. 补充绿控（交叉复验配套）

- F41 完整门禁绿：登记册还原后 `CARGO_TARGET_DIR=<隔离 target> bash scripts/rust-tauri/r02_t04_storage_tx.sh <新 ev>` S0–S4 全绿 exit 0（本机隔离副本亲跑；与 [A-R2] REVIEW 的主树亲跑互为独立复验）。
- F42 完整链绿：[B-R2-5] `B-R2/R2/s5-full-5/`（仓库根完整 r02_t08，最终行 GREEN，exit 0；本轮未重复完整跑，静默窗口成本高且已有独立绿记录；directed 全绿为本轮亲跑）。

## 4. 统计与结论

- 原 16 项：**16/16 fail-closed 且点名（allRefused=true），对照绿（controlsGreen=true）**；异常 0。
- RR2 新增反例（F41/F34/F31/F42-N1+F42-illegal/F44）：**6/6 符合预期（红→还原→绿/复现登记形态）**；异常 0。
- **新 F-ID：无（未登记 F45+）**。两处环境/维护性事实如实登记于 §0（/tmp 符号链接围栏＝xtask 设计行为；N03 锚点过期＝负测脚本维护债，隔离副本内已按当前登记值等效执行，主树脚本未动）。

## 5. I-MAPPING.md 的 I11 行以此报告为准

I11（门禁自证）的有效运行＝本报告 §1 的 16/16＋§2 的 6/6＋恢复后对照绿；F25/F26/F27 的 RR1 期新增反例证据见 I-MAPPING.md §1 I11 行（叶段致命 gaps、registry 恒等预检、采样器负对照），本轮未重复注入。
