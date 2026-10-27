# R08 冻结候选独立准入复验（admission-candidate-review-01）

结论：**ADMISSION_PASS_WITH_SCOPE** —— 五个原叶（run_code / run_tools / ast_grep / lsp / security_scan）的逐断言独立复验全部 PASS；R07 准入 FAIL 的 **F01（五工具真实登记+执行缺失）在准入层面闭合**；**F02（149 条原断言错绑）不在本轮职责内，仍 OPEN**。R08 T01—T08 中依赖这五个工具真实执行面的业务可以放行推进；凡是要把 R07 408 叶 PASS 台账整体当可信输入的消费（尤其 T01 多工具兼容矩阵、T07 全功能 A/B 的历史证据复用），仍须等 F02 收口。

审查者：独立准入审查者（`admission_evidence_review` 角色的延续，本轮为第三方复验，未参与任何业务实现）。执行日期：2026-10-22（Asia/Shanghai）。分支 `codex/rust-tauri-migration`，提交基点 `86840d1700397d3dd620a41f2b884df4cddd5cc2`。

## 1. 冻结候选绑定核对

- 本审查开始与结束时各用 python3 复算一次 `git diff --binary HEAD` 的 SHA-256，均为 `cc7f7052cda1b3d0636c1323575cae3a868163b3786ce7d671a56a470acdacdb`，与任务给定期望值一致，**未发生 CANDIDATE_SHIFTED**。
- 与四个既有证据目录回执逐一比对，`candidateDiffSha256` 全部同为 `cc7f7052…`：`artifacts/rust-tauri/R08/security-real-candidate-01`、`admission-public-build-01`、`admission-authority-run-01`、`admission-astlsp-current-01` 的 `receipt.json`。
- 本轮新建的 9 个证据目录（`artifacts/rust-tauri/R08/admission-candidate-review-01/` 下）回执全部 `candidateDiffSha256=cc7f7052…`、`sourceBindingsStable=true`。
- 未修改任何产品源码、测试、脚本、既有文档或历史回执；未 git commit/push；未读写真实用户数据根（全部测试使用合成临时根）。

## 2. 独立执行的命令与结果

每条命令经 `python3 scripts/rust-tauri/r08_run.py --evidence <新目录> -- <命令>` 记录回执。Rust 工具链固定 `/Users/study_superior/.cargo/bin/rustup run 1.98.1`；每次运行 cargo 前确认无并行构建（`pgrep -fl 'cargo|rustc'` 为空）。

| 证据目录 | 命令（要点） | 退出码 | 结果 |
|---|---|---|---|
| `build-01` | `rustup run 1.98.1 cargo test --manifest-path rust/Cargo.toml -p lingxi-service --test r08_admission_run_code --test r08_admission_ptc --test r08_admission_ptc_authority --test r08_admission_ast_lsp --test r08_admission_security --no-run` | 0 | 五目标一次统一编译，产物未重链接（源与 00:51/09:00 既有产物一致） |
| `run-code-run-01` | `rust/target/debug/deps/r08_admission_run_code-465a18f4152e0455 --nocapture --test-threads=1` | 0 | 9 通过 / 0 失败（33.52s） |
| `astlsp-run-01` | `env PATH=/private/tmp/lingxi-r08-ast-tools-0510/node_modules/.bin:$PATH rust/target/debug/deps/r08_admission_ast_lsp-a93a248840f19715 --nocapture --test-threads=1` | 0 | 4 通过 / 0 失败（7.78s） |
| `security-run-01` | `rust/target/debug/deps/r08_admission_security-f2475c2f9d54239a --nocapture --test-threads=1` | 0 | 4 通过 / 0 失败（1.71s） |
| `ptc-authority-run-01` | `rust/target/debug/deps/r08_admission_ptc_authority-23a9c64c6bcaacac --nocapture --test-threads=1` | 0 | 5 通过 / 0 失败（1.03s），拒绝收据在日志中可见（`dispatched: false` 持久 Failed 回执） |
| `ptc-functional-run-01` | `rust/target/debug/deps/r08_admission_ptc-5d41adb521f581e6 --skip real_default_ten_minute_timeout_keeps_completed_action_and_reaps_worker --nocapture --test-threads=1` | 0 | 7 通过 / 0 失败 / 1 明确过滤（1.98s）；过滤项由下一条单独全量执行 |
| `ptc-default-timeout-run-01` | `env TEST_TIME_LIMIT_SEC=660 <同二进制> real_default_ten_minute_timeout_keeps_completed_action_and_reaps_worker --exact --nocapture` | 0 | 1 通过 / 0 失败 / 7 明确过滤（600.25s）——**本轮独立复跑，非仅证据审查** |
| `fence-unit-01` | `cargo test -p lingxi-service --lib fence_verdict_requires_identity_and_no_cancellation -- --exact --nocapture` | 0 | **0 项执行（650 过滤）**：`--exact` 未带模块路径前缀，过滤器落空。保留为无效尝试，不记 PASS |
| `fence-unit-02` | `cargo test -p lingxi-service --lib runs::tests::fence_verdict_requires_identity_and_no_cancellation -- --exact --nocapture` | 0 | 1 通过 / 0 失败（修正过滤器后的有效执行） |

运行结束后复查：无遗留 `cargo/rustc/r08_admission/r08-ptc-worker` 进程；`git diff --binary HEAD` 摘要仍为 `cc7f7052…`。

对既有 600 秒证据的处理说明：实现者的 `ptc-implementation-01/real-default-timeout-02.log` 与 `final-candidate-01-result.json` 已存在；本审查选择**独立复跑**而非仅审查，结果（退出 0、1 通过、600.25s）与实现者记录（600.30s）一致。

## 3. 每条原断言 → 证据 → 判定

原断言原文取自 `docs/rust-tauri/R00/FEATURE_STAGE_ACCEPTANCE.json`（非摘要）。下列"测试名"均在本轮回执日志中实际执行并通过；行号指向冻结候选中的测试/源码文件。

### 3.1 run_code `R00-T02-LA-C88F29B5114A`（叶判定：PASS）

| 原断言 | 证据 | 判定 |
|---|---|---|
| 两次调用可共享变量 | `run-code-run-01`：`python_and_node_real_kernels_preserve_variables_functions_imports_and_isolate_agent_sessions`（tests/r08_admission_run_code.rs:446-450，同 kernelId 第二次调用以首次的 `x=40` 算出 `42`；Node 含顶层 await/动态 import 跨调用保留）；另默认组合根独立证据 `default-composition-independent-01`（`INDEPENDENT_STATE_RESULT 42`） | PASS |
| 重启后旧变量消失 | 同上测试 :478-483（restart 后新 kernelId，`print(x)` → `not defined`）；超时测试 :707-710 同证 | PASS |
| 取消清理内核 | `cancelling_real_run_stops_persistent_python_kernel_and_its_actual_grandchild`（:611-683：真实取消 → Run Cancelled，内核与真实孙进程两个 PID 全灭，`supervisor.live_handles()` 为空） | PASS |
| 语言无效、无会话、内核死亡或超时显式失败；重启不声称保留旧状态 | `RUN_CODE_INVALID_LANGUAGE`（:551-552）、`RUN_CODE_LANGUAGE_MISSING`+安装指引（:396-399，独立空 PATH 子进程）、`RUN_CODE_KERNEL_EXITED/DEAD` 不静默重建（:590-606）、真实 30 秒 `RUN_CODE_TIMEOUT` 后 restart 新内核且 `print(x)` → `not defined`（:700-710） | PASS（注：`RUN_CODE_NO_SESSION`（coding_run_code.rs:279）为源码级显式拒绝，五套件无独立样本；经真实链调用时会话层先行拒绝，难以从正规入口到达该分支，如实记录为子缺口，不构成阻断） |

### 3.2 run_tools `R00-T02-LA-0818574ABD43`（叶判定：PASS）

| 原断言 | 证据 | 判定 |
|---|---|---|
| 每个子调用独立权限判定 | `ptc-functional-run-01`：`real_child_approval_is_separate_from_outer_approval_and_denial_writes_nothing`（外层批准 ≠ 子批准 approvalId，子 write 被拒 → 文件未写）；`ptc-authority-run-01`：批准等待中撤权/禁用/换代均 `dispatched=false` 持久拒绝回执且目标未写 | PASS |
| 外层只显示打印/返回值 | `real_parallel_children_only_return_selected_summary_and_keep_bounded_ui_receipts`（外层正文只含 `selected summary` 与返回值 `2`，`SECRET_CHILD` 不入正文；模型历史恰好 1 条 ToolResult 且无子内容）；默认组合根独立证据：`PTC_PRIVATE_CHILD_DO_NOT_SEND` 未进入实际外部模型请求/历史 | PASS |
| 失败显示已执行子动作 | `later_program_failure_keeps_real_completed_effect_and_exposes_it_in_receipts`（外层 throw 后 `retained.txt` 保留、subcalls[0].ok==true） | PASS |
| 未知工具、自调用、无权限、脚本超时或沙盒异常拒绝；已执行子动作不因外层失败自动回滚 | `unknown_recursive_and_sandbox_failures_are_explicit_and_catchable`（未知名与 `run_tools` 自调用均 `ToolCallError` 且留 2 条 ok=false 持久子回执；沙盒异常 `RUN_TOOLS_EXCEPTION`）；无权限见上行子批准拒绝；`ptc-default-timeout-run-01`：真实 600.25 秒默认预算 `RUN_TOOLS_TIMEOUT`、前缀输出保留、已完成动作保留、worker 实际回收；不回滚见 `later_program_failure…`；另 `actual_vm_escape_cannot_read_write_connect_or_delegate_a_host_write`（vm 逃逸后真实私有读写/子进程写/本机 TCP 全 EPERM/BLOCKED，目标零副作用） | PASS |

### 3.3 ast_grep `R00-T02-LA-196D50D8DD6E`（叶判定：PASS）

| 原断言 | 证据 | 判定 |
|---|---|---|
| 命中结构而非同文字符串 | `astlsp-run-01`：`r08_ast_structure_pagination_no_match_and_errors_use_real_run`（文件含同文字符串 `'greet(99)'`，`greet($A)` 命中数为 2 而非 3——只命中真实调用结构；官方 ast-grep CLI 真实语法树，无文本搜索兜底） | PASS |
| 无匹配不写文件 | 同上（全部操作后 `example.js`/`sgconfig.yml` 逐字节不变，含恶意 sgconfig 未被消费） | PASS |
| 无匹配为空结果；语言/解析失败或路径越权给错误 | 同上（`missing($A)` → matchCount 0 + `no matches`；invalid-language Failed；`Cannot parse query` 真解析器失败 Failed 不冒充空匹配；`../outside.js` 越权 Failed） | PASS |

### 3.4 lsp `R00-T02-LA-2D896560381E`（叶判定：PASS）

| 原断言 | 证据 | 判定 |
|---|---|---|
| 位置与源码一致 | `r08_lsp_real_clangd_positions_preview_rename_and_rejections`（真实 clangd definition/hover/symbols/references；参考列由 UTF-16 偏移（同行含 emoji 😀）计算后命中；broken.cpp 诊断含 `missing`） | PASS |
| rename 仅改返回编辑范围 | 同上（rename applied 后文件恰为 `old_name→new_name` 全替换、收据恰 1 份；错位 prepare 拒绝 Failed） | PASS |
| 不可写时只预览不落盘 | `r08_lsp_read_grant_rename_returns_preview_without_writing`（只读授权下 rename：`applied=false`、`LSP_WRITE_UNAVAILABLE`、preview 含 new_name、文件不变）；同套件 `preview:true` 分支同证 | PASS |
| 服务缺失/崩溃、不可重命名或写入能力缺失明确报错；无编辑时文件不变 | `r08_lsp_missing_and_crashed_server_are_explicit_errors`（missing→`LSP_SERVER_MISSING`；crash→`exited`；空 WorkspaceEdit→changedFiles [] 且文件不变；cancel→真实语言服务进程树全回收；仅外部 LSP 以替身注入，主链真实 clangd） | PASS |

### 3.5 security_scan `R00-T02-LA-CD5D7FC02D8E`（叶判定：PASS）

原断言以 R00 原文为准：发现与实际文件对应 / 范围不足明确 / 无越权读取 / 非Git工作区、路径越权、文件上限或引擎失败标注范围错误不伪称全量通过。

| 原断言 | 证据 | 判定 |
|---|---|---|
| 发现与实际文件对应 | `security-run-01`：`r08_security_changes_git_readonly_and_subdirectory_never_enumerates_parent`（changes 模式 `filesScanned==1` 恰为被改的 tracked.ts）；`r08_security_real_baseline_never_returns_secret_and_audits_counts`（credential.ts 命中 findingCount>0）；源码强制 finding 必须映射回已授权快照文件否则 `SECURITY_FINDING_OUT_OF_SCOPE`；默认组合根独立证据（真实文件 + `secret_api_key_literal`、filesScanned=1、审计计数事件） | PASS |
| 范围不足明确 | baseline 测试：`filesSkipped>=2`（binary/超限）+ 正文含 skip 说明；引擎状态逐项显式（semgrep/gitleaks missing/failed/completed）；子根无 .git 时明确 `SECURITY_GIT_SCOPE_UNAVAILABLE` 并建议 path 模式 | PASS |
| 无越权读取 | `r08_security_outside_root_and_symlink_refused_and_sources_unchanged`（根外路径与符号链接均拒；扫描输出不含外部哨兵，外部文件不变）；`r08_security_external_scanner_cannot_read_or_write_outside_snapshots_and_leaves_no_child`（外部扫描器进程在快照沙盒内：外部读/写失败、孙进程被回收）；changes 子根不枚举父（无 parent-only-sentinel）；`.git/index` 前后逐字节一致（`--no-optional-locks`，worker 写根为空）；默认组合根：模型投影不含命中正文 | PASS |
| 非Git工作区、路径越权、文件上限或引擎失败标注范围/错误，不伪称全量通过 | 非Git（子根 changes）与路径越权直接实测 Failed；文件上限：源码机制完整（`fileCapReached` 字段 + `file cap … reached — narrow the path` 正文行 + `max_files` 截断，coding_security.rs:154-158/205-208/304/322-324）但五套件无直接触发样本；引擎失败：worker 明确区分 `missing`/`failed(SCANNER_EXECUTION_FAILED)`/`completed`（r08-security-worker.mjs:65-83），默认组合根以 engines=[baseline] 如实呈现缺引擎，但无"引擎执行失败"直接样本；摘要行恒带 skip 计数、引擎状态与审计状态，未见任何冒充全量通过的路径 | PASS（如实记录两个子缺口：文件上限与引擎失败分支为机制级核对，缺直接样本；不构成阻断，见 §6 追加项） |

## 4. 源码机制核对（只读，逐项确认存在且被测试断言）

1. `runs.rs:188` `refuse_child!` 宏：早拒绝先落 `record_invocation_intent` + `persist_tool_event` + 子收据后返回，`dispatched=false` 可持久化。被 `unknown_recursive_and_sandbox_failures…`（2 条 ok=false 子回执）与 `ptc_authority` 三项拒绝测试（`assert_not_dispatched`：journal phase=Failed、receipt Failed、dispatched=false、detail 非空）实际断言。
2. `runs.rs:301` 每个子调用经 `review_and_validate_from_request_pinned`（biased select 取消优先）；`runs.rs:400-424` 复核递归（`PTC_RECURSION_FORBIDDEN`）、phone 域（`PHONE_SCOPE_FORBIDDEN`）、workflow 工具过滤、delegation（`PTC_DELEGATION_REQUIRES_DIRECT_DRIVER`）。worker（r08-ptc-worker.mjs）把每个 `tools.name(args)` 原样发给 Rust driver，无 JS 侧预过滤。
3. `runs.rs:2332-2348` `fence_verdict`（identity 不符→FenceMismatch；取消先到→`CancelledBeforeWrite`）、`runs.rs:2359` `audit_late_result`（拒写留审计）、`runs.rs:517` PTC 子结果主链接线（stale → `ToolOutcome::Unknown` + audit）。单元测试 `runs::tests::fence_verdict_requires_identity_and_no_cancellation` 本轮独立执行通过（`fence-unit-02`）。
4. `coding_run_code.rs:288-293` 非受限动作遇真实 `workflow_scope` → `RUN_CODE_WORKFLOW_UNSUPPORTED`（status 保持只读）；被 `real_workflow_write_roots_cannot_be_bypassed_by_persistent_interpreter_run_or_restart` 实际断言（兄弟文件未创建、监督器无内核）。
5. `coding_security.rs`：changes 模式仅接受当前授权根内实体 `.git`（:120-131），git 经 worker 以 `--no-optional-locks -c core.fsmonitor=false -c core.untrackedCache=false status --porcelain=v1` 只读列举（liveGit 模式写根为空）；外部扫描器只在快照沙盒运行（读限 snapshotRoot，写限 reportRoot）；测试断言 `.git/index` 前后逐字节一致、子根不枚举父、外部扫描器越界读写失败。
6. 默认装配路径：`lib.rs:1735`（run_code）、`:2063`（run_tools，`wired_real_model_chain` 下）、`:2576/:2588`（ast_grep+lsp、security_scan）。与默认组合根独立 PASS_LOCAL（五工具默认 resolve+prepare，三工具默认模型入口实际执行）互证。

## 5. 七个边界项裁决

1. **PTC-A02 delegation**：`OUT_OF_ORIGINAL_SCOPE`（对原叶而言），显式拒绝满足诚实性要求。`request_delegation` 只匹配 `subagent|subagent_reply|subagent_close`（subagents.rs:274-281）；PTC 子调用到这些目标被 `PTC_DELEGATION_REQUIRES_DIRECT_DRIVER` 显式拒绝（runs.rs:419-424），`run_tools` 自调用另由 `PTC_RECURSION_FORBIDDEN` 拒绝且该拒绝已有持久回执实测。原叶四断言不要求 delegation 语义；"run_tools 可作为 delegation 目标"属 R07 all-tools 集成义务，超出本准入范围。缺口如实记录：`PTC_DELEGATION_REQUIRES_DIRECT_DRIVER` 拒绝收据目前无专项测试样本（仅源码机制）。
2. **RC-A02 目录一致性**：`GAP_NON_BLOCKING`（保持独立测试缺口，不阻断原叶）。当前源码 `coding_run_code.rs:513-521` 会在每次执行时把 `kernel.cwd` 相对当前网关工作区（`actual_cwd`）做组件级重授权，越出授权根即 `RUN_CODE_RESOURCE_DENIED` 显式拒绝——不会"展示新目录批准下静默写旧目录"的越权情形已由机制阻止；但仍无 kernel.cwd 与冻结 cwd 的一致性比较（旧目录仍在授权内时继续用旧目录），且五套件无工作区切换到达路径样本。原叶未要求目录一致性；缺口保留为 R08 集成期补样项。
3. **PTC-A03 phone/cron/heartbeat 自动审核身份矩阵**：`MECHANISM_CLOSED_SAMPLE_GAP`。每个子调用经 `review_and_validate_from_request_pinned`（runs.rs:301），权限上下文由 `authority.phone_scoped` 构造（:292-299），另有逐子 `PHONE_SCOPE_FORBIDDEN`（:403-407）；通用逐子审核已被 ptc_authority 五项实测（撤权/禁用/换代/配额/重放）。phone_scoped 专项样本（phone/cron/heartbeat 身份下逐子动作）仍缺——这是授权独审的扩展要求，不是原叶断言；不阻断本准入，登记为 R08 义务。
4. **PTC-A05 取消先到/迟到结果围栏**：`SUFFICIENT_FOR_LEAF`。机制存在且被独立验证：`fence_verdict`/`CancelledBeforeWrite`/`audit_late_result` 主链接线于 runs.rs:517，单元测试本轮独立执行通过（fence-unit-02：Current/FenceMismatch/CancelledBeforeWrite 三分支）；biased select（:300/:444）使取消在竞态中先到；真实取消测试（`cancelling_actual_program_reaps_its_real_node_process`：活程序取消后 worker+孙进程回收、Run Cancelled）与 ptc_authority 重放项（同请求运行中/完成后幂等、零重复副作用）覆盖原叶"取消清理/不重复副作用"语义。"取消恰在结果就绪与写状态之间"的端到端竞态直接样本仍缺，作为非阻断残留登记。
5. **PTC-A07 崩溃中点重放**：`NOT_ORIGINAL_LEAF_REQUIREMENT`。默认组合根已独立证明关库重开不重复执行（历史读回无重放、文件副作用保持）；ptc_authority 的 `same_explicit_request_replay_and_real_event_resume_never_repeat_children` 证同请求重放幂等（journal 反重放 + `PTC_PARENT_INACTIVE_OR_CHILD_REPLAY`，runs.rs:170-178）。原叶只要求"外层失败不自动回滚已执行子动作"（已实测）；"崩溃恰在中点（子效果已落、结算未写）后重放"是授权独审/R08 恢复义务的扩展，不阻断本准入。
6. **660 秒机器保护器余量差异**：`ACCEPTED`（可接受，且本轮已独立复跑，不要求再次复跑）。差异仅作用于测试进程的机器保护器（lingxi-test-guard 整进程墙钟，默认 600s；内存 8GiB 保护保留），产品 600 秒预算由测试内部断言（`started.elapsed() >= 600s`、`RUN_TOOLS_TIMEOUT`）独立保证且未改、无假钟。实现者将全套 8 项拆为 7+1 两组全量执行（过滤是执行分Partition，不是 skip）；本审查独立复跑单项：退出 0、1 通过、600.25s，与实现者 600.30s 记录一致。
7. **F01 整体**：`CLOSED_FOR_ADMISSION`。默认组合根独立 PASS_LOCAL 已证五工具默认登记+resolve+prepare（run_code/run_tools/security_scan 另经真实默认模型入口执行、关库重开读回）；本轮五套件在同一冻结候选上全部独立复验通过，且逐断言映射成立（见 §3）。R07 准入 FAIL 的 F01 缺口（五工具 TargetNotFound）已消除。附注：ast_grep/lsp 经"默认模型入口"的执行未单独抽样（其行为执行在本套件经真实 gateway/Run 完成，登记+prepare 在默认根独立证明）——该附注不重开 F01，因为 F01 的发现是登记表缺失，不是触发入口。

## 6. F01/F02 状态与放行建议

- **F01：闭合（准入层面）**。依据：默认组合根独立复验（334 件来源稳定的候选，五工具默认可见可备、三工具默认入口执行+关库重开）+ 本轮五套件逐断言独立复验（全部绑定 cc7f7052、sourceBindingsStable）。
- **F02：仍 OPEN，不在本轮职责内。** 149 条错绑复核集合的收口顺序由主集成在独审结论后另行决定；本报告不代领、不修改其范围重建结论（ADMISSION_F02_SCOPE_REVIEW.md）。
- **R08 T01—T08 放行建议**：T01—T08 中消费这五个工具真实执行面（UI 卡片、文件任务、A/B 中涉及 run_code/run_tools/ast_grep/lsp/security_scan 的链路）的业务可以放行推进；但（a）凡把 R07 408 叶 PASS 台账整体当可信输入的验收，须等 F02 收口或逐项重新绑定；（b）下列登记缺口应作为 R08 期内的跟踪项，不阻断开工：RC-A02 工作区切换样本、phone_scoped 专项矩阵、delegation 拒绝收据专项样本、取消-迟到结果端到端竞态样本、崩溃中点重放、security 文件上限与引擎失败直接样本、run_code 无会话分支样本说明。
- 平台边界：本轮全部为 macOS（darwin 27.0.0 arm64）实测；Linux bwrap/Windows 拒绝路径未实测，沿既有声明保留。

## 7. 诚实性声明

- `fence-unit-01` 是一次过滤器落空的无效执行（0 项运行），原样保留并标注，不记 PASS；有效结果以 `fence-unit-02` 为准。
- 本轮无任何测试失败需要报告；若后续候选变化，本报告全部结论须重新绑定复验，不得沿用。
- 本轮未执行：真实用户数据迁移、非 macOS 平台、正式打包、R08 T01—T08 业务验收、F02 149 条收口、正式 `verify-stage R08` 门禁。
- 证据目录清单与逐命令退出码见配套 `ADMISSION_CANDIDATE_INDEPENDENT_REVIEW.json`。
