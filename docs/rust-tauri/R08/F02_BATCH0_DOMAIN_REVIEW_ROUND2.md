# F02 批次0 域级独立审查复验（Round 2）

状态：**PASS_WITH_NOTES**（2026-10-22）。审查者延续 F02_BATCH0_DOMAIN_REVIEW 的独立审查角色，未参与修复实现；本轮只读复验（未修改产品文件、未运行 cargo），仅新建本报告。
复验对象：[F02_BATCH0_DOMAIN_REVIEW.md](F02_BATCH0_DOMAIN_REVIEW.md) 问题清单（F02-B0-1…6）的修复处置。

**结论一行：前轮 FAIL 的 D17 两叶已修复成立 —— 探针 `bridge_settings_receipt_and_rich_streaming_toggles_persist` 两开关双向写入回读、非法权限模式 400 后经真实端点读回且 richStreamingEnabled=false（非默认值）在拒后存活，『拒绝时目标状态不变』不再是超claim；lsp/run_code A3 补列案例的函数体确实断言对应分支；suites-03 8 套件 exit 0、25 个 pinned 案例全绿；候选 812f053c… 与工作区逐字节一致。问题1—5 全部处置成立，批次0 整体 8/8 叶通过；遗留 NOTE 为前轮问题6（security_scan，未在本轮修复范围、已披露）与下文新发现 N1—N3（均不构成虚claim）。**

## 1. 前轮问题逐项处置判定

### F02-B0-1（FAIL｜09D567E99524、D5514BBDBF88｜400 后无状态回读、rationale 超claim）— 已修复，成立

- **探针函数体**（rust/crates/lingxi-service/tests/r07_probe_access.rs:576-632）：
  - 双向写入＋逐次回读：`:581-596` 对 `receiptEnabled` 写 false→true、`richStreamingEnabled` 写 true→false，每次写入后断言 `status==200` 且同响应 `body[field]==value`（`:593-594`）。两字段各覆盖两个方向。
  - 400 后真实读回：`:609-616` POST `permissionMode:"god"` → 断言 400；`:617-624` 随即 POST `permissionMode:"auto"`（合法无关写）→ 200；`:625-630` 断言 `receiptEnabled==true`、`richStreamingEnabled==false`，与写入序列终值一致（receipt 第二次写 true，rich 第二次写 false）。
  - 关键语义核（读回载体本身是否改写所核字段）：读 `settings_route` 写路径（rust/crates/lingxi-service/src/bridge_routes.rs:470-508）——`permissionMode` 分支只写 `root["bridge"]["permissionMode"]`（`:486-491`），receipt/rich 仅在各自字段 `Some` 时写入（`:492-497`），POST `permissionMode=auto` 不会触碰两开关，回读值即持久化状态。且非法 mode 在 `:487-489` 先校验返回 Err，`write_preferences_root`（`:498`）不可达，400 路径零写入（源码机制与读回证据互相印证）。
  - 证据强度：`permission()` 渲染规则（bridge_routes.rs:200-206，`:204`）为 `!=Some(&Value::Bool(false))` 即 true，缺省即 true——rich=false 是唯一可与默认区分的方向，其经无关写（`:607`）与 400 拒绝（`:626-630`）两轮读回仍为 false，构成『拒绝时目标状态不变』的可区分真实证据（receipt=true 与默认同值，单看该字段不可区分，见 N2；rich 字段已足以立据）。
- **candidate.diff 取证**：f02-batch0-binding-check-02/candidate.diff 中该文件 hunk 的 pre-image 与前轮失败版逐行一致（单向写入、`kept` 断言 false/true、400 无回读），post-image 与当前工作区一致——确证改写即为本轮所审版本。
- **rationale**（R07_LEAF_BINDINGS.json 两叶同文）：如实描述『首轮只断言 400 未回读、rich 只测默认方向 true』的失败史与修复内容，结尾『待域审复验』——无超claim。
- **真实执行**：suites-03 r07_probe_access output.log:3 `bridge_settings_receipt_and_rich_streaming_toggles_persist ... ok`（21 passed / 0 failed）。

### F02-B0-2（MAJOR_NOTE｜D5514BBDBF88｜rich 仅测默认方向 true）— 已修复，成立

rich 双向（true→false）写入回读（r07_probe_access.rs:583, :594），终值 false 为非默认方向，且经无关写与 400 拒绝后读回仍 false（`:607`、`:626-630`）。A0『richStreamingEnabled true/false 保存并返回实际值』两个方向均有真实样本。

### F02-B0-3（NOTE｜09D567E99524｜receipt 仅测 false 方向）— 已修复，成立

receipt 双向（false→true）写入回读（r07_probe_access.rs:582, :594），A0 两个方向均有样本（true 方向与默认同值，由逐次回读立据；见 N2 的强度说明）。

### F02-B0-4（NOTE｜C88F29B5114A｜A3 套件内绿样本未绑定）— 已修复，成立（含披露）

- 绑定：run_code 叶 A3 组（R07_LEAF_BINDINGS.json，第 4 组）补列 `real_gateway_status_remains_read_only_and_code_never_inherits_service_secrets`。
- 函数体（rust/crates/lingxi-service/tests/r08_admission_run_code.rs:489-569）确实断言对应分支：`:550-552` 未支持语言名 ruby → Failed 且正文含 `RUN_CODE_INVALID_LANGUAGE`；`:553-567` 对 `no-such-session` 的 `execute_background_for` 返回 Err（无会话分支）。
- rationale 如实披露『无会话分支为会话层先行拒绝』——与前轮 NOTE 口径一致，无虚claim（残留说明：该分支证据在会话层而非 run_code 工具网关自身，属已披露的层级差异）。
- 真实执行：suites-03 r08_admission_run_code output.log:9 该案例 `ok`（9 passed / 0 failed）。

### F02-B0-5（NOTE｜2D896560381E｜A3 两分支未列入本组）— 已修复，成立

- 绑定：lsp 叶 A3 组（R07_LEAF_BINDINGS.json，第 4 组）补列 `r08_lsp_real_clangd_positions_preview_rename_and_rejections` 与 `r08_lsp_read_grant_rename_returns_preview_without_writing`（连同既有的 missing/crashed 案例）。
- 函数体确实断言对应分支：错位 prepare 拒绝——rename 于 line 1 column 1（错位位置）→ `ToolOutcome::Failed`（r08_admission_ast_lsp.rs:526-530）；写能力缺失——只读授权 rename → `applied==false`、`errorCode=="LSP_WRITE_UNAVAILABLE"`、preview 含新名、文件逐字节不变（`:551-565`）。
- 真实执行：suites-03 r08_admission_ast_lsp output.log:5-6 两案例 `ok`（4 passed / 0 failed）。

### F02-B0-6（NOTE｜CD5D7FC02D8E｜security_scan 间接证据/无直接样本）— 未处置（不在本轮修复范围）

前轮已判定为已披露 NOTE（finding.path 间接成立；文件上限/引擎失败无直接样本、登记 R08 跟踪缺口），本轮修复未触及 security_scan 绑定与案例，维持原披露状态，不构成虚claim，不阻塞。

## 2. 证据链复核 — 全部成立

- **suites-03**：8/8 result.json `exitCode==0`（r06_skills_completion、r07_probe_access、r07_t12_closure、r08_admission_{ast_lsp,ptc,ptc_authority,run_code,security}）；8 份 output.log 的 sha256 与 result.json `logSha256` 逐一实测相符；r08_admission_ptc `durationSecs=602.389`（真实 600 秒单项，test result: 8 passed / 0 failed，602.19s）。
- **leaf-cases**：60/60 案例 `ok=true、actual==expect==1、suiteExitCode==0`；由 8 叶绑定 assertionCases 展开去重恰 25 个 pinned 案例（与前轮 24 相比新增 `real_gateway_status_remains_read_only…`，与补列一致），25/25 均在 leaf-cases 中且全绿、无缺失。
- **绑定↔注入一致性**：stage_maps/R07.json 中 8 叶的 `originalAssertionCases` 与 R07_LEAF_BINDINGS.json `assertionCases` 逐叶组级全等，`assertionContract.cases` 与去重 pinned 集（25）逐叶全等；`suiteRefs` 与 `evidenceCommandRefs` 一致（后者仅多生产者命令 `r07_leaf_cases`，非差异）。8 叶无组内重复案例。
- **绑定回执 f02-batch0-binding-check-02**：`exitCode==0`，`sourceSha=86840d17…` 与当前 git HEAD 相同；`candidateDiffSha256=812f053c…` 与现场重算 `git diff HEAD` 逐字节一致（sha256 实测相同）——本审查所读工作区即回执绑定的候选，无『审非所绑』缺口。
- **候选边界**（binding-check-01 → -02 两份 candidate.diff 逐文件逐 hunk 比对）：新增仅 `rust/crates/lingxi-service/tests/r07_probe_access.rs`（探针重写）；变更仅 `docs/rust-tauri/R07/R07_LEAF_BINDINGS.json`（D17 两叶 rationale 重写、lsp A3 +2 案例、run_code A3 +1 案例）与派生的 `rust/crates/xtask/src/stage_maps/R07.json`；其余 11 个文件（含 lingxi-kernel/lingxi-service 源码 hunk）与 -01 完全相同——本轮修复未改任何产品源码。
- **时间链**：探针/绑定最后修改 10:39:42/10:39:59 → 8 套件完成 10:41:58—10:50:37 → leaf-cases 生成 10:52:12 → 绑定回执 10:52:24，证据晚于代码、顺序自洽。

## 3. 新发现（均为 NOTE，不构成 FAIL/PASS 障碍）

| 编号 | 级别 | 发现 |
|---|---|---|
| R2-N1 | NOTE | 『suites-03 重新串行全绿』的『串行』应理解为套件内串行（r07_stage_inputs.py:100-107 对 r08_admission_* 注入 `RUST_TEST_THREADS=1`、ptc 加 `TEST_TIME_LIMIT_SEC=660`）。跨套件并非逐一串行：8 套件 result.json 完成时间跨度 10:41:58—10:50:37 约 519 秒，小于各套件测试时长总和 682.4 秒，算术上必然存在跨套件并发（非 ptc 各套件在 ptc 的 602 秒窗口内完成）。前轮 ptc 红失败模式是套件内并行导致同父 PID 的兄弟 worker 误命中（ppid 过滤），跨套件进程父 PID 不同，不会命中该断言；本轮该测试绿。证据有效性不受影响，仅措辞需按此口径理解。 |
| R2-N2 | NOTE | 400 拒后读回中 `receiptEnabled==true` 与默认渲染值（缺省即 true，bridge_routes.rs:204）同值，单看 receipt 字段无法区分『保持』与『被重置回默认』；『拒绝时目标状态不变』的可区分证据由 `richStreamingEnabled==false`（非默认）承担，并与源码先校验后写（bridge_routes.rs:487-498）互证。探针注释与 rationale 均未对此超claim。 |
| R2-N3 | NOTE | D17 两叶的两组原断言仍各自绑定同一探针案例（跨组复用），为前轮已复核放宽规则所允许（组内重复仍禁，8 叶均无组内重复）。 |

## 4. 更新判定

| 叶 | 前轮 | 本轮 |
|---|---|---|
| run_code C88F29B5114A | PASS_WITH_NOTES | PASS（问题4 处置，残留仅会话层披露口径，rationale 已如实标注） |
| run_tools 0818574ABD43 | PASS | PASS（未变更） |
| ast_grep 196D50D8DD6E | PASS | PASS（未变更） |
| lsp 2D896560381E | PASS_WITH_NOTES | PASS（问题5 处置） |
| security_scan CD5D7FC02D8E | PASS_WITH_NOTES | PASS_WITH_NOTES（问题6 维持披露，R08 跟踪缺口不变） |
| learn_lesson 771296A194E0 | PASS | PASS（未变更） |
| D17 09D567E99524 | **FAIL** | **PASS**（问题1/3 处置，见 §1） |
| D17 D5514BBDBF88 | **FAIL** | **PASS**（问题1/2 处置，见 §1） |

批次0 整体：**PASS_WITH_NOTES**（8/8 叶通过；遗留为 security_scan 既披露 NOTE 与本报告 N1—N3 措辞/强度说明）。

## 5. 诚实性声明

- 已执行：探针与两处 A3 案例函数体逐行读取；bridge_routes.rs 写路径/渲染源码核对；R07_LEAF_BINDINGS.json 四叶条目与 stage_maps/R07.json 逐叶全等比对；suites-03 全部 result.json/logSha256/输出日志核验；leaf-cases 60 条与 25 个 pinned 全绿核验；binding-check-02 回执与 `git diff HEAD` sha256 逐字节比对；两轮 candidate.diff 逐文件逐 hunk 差异分析；文件时间戳与执行时长交叉核对。
- 未执行：cargo/测试运行（任务约束）；批次0 之外 400 叶的既有绑定复核；非 macOS 平台；问题6 相关新样本要求（未在修复范围）。
- 本结论绑定候选 `sourceSha 86840d17…`＋`candidateDiffSha256 812f053c…`；候选变化后须重新复验，不得沿用。
