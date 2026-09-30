# R03 对抗性修复轮｜阶段终审派单（STAGE-REVIEWER-R03-RR1，全新）

派单时间：2026-09-30。派单人：R03 修复总控编排器。你是全新一次性独立阶段验收代理 STAGE-REVIEWER-R03-RR1：**从未参与本轮（2026-09-30 对抗性修复轮）任何实现、修复、工作单执行或工作单独立审查**。这是本阶段（修复后）的最终裁决，输出 STAGE_VERDICT。

## 0. 对象与边界

- 仓库 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`，最终候选 `CANDIDATE=ebcbcad1b9fa1c621b315dd33b58f8a2e01ab9eb`（=当前 HEAD=远程 HEAD，工作树应干净；若不干净先如实记录）。
- 修复轮基线：`cd3fb19e651f763afc6c75cb3163064fb54ca3fe`（被质疑的旧接受）；旧接受候选 `1ebb03d9f`（其历史与证据原文保留，不允许伪造历史，也不得以旧 PASS 为反证）。
- 你的复测产物只写 `artifacts/rust-tauri/R03/repair-current/STAGE-REVIEW-RR1/`，报告写 `docs/rust-tauri/R03/repair-current/R03_FIX_FINAL_STAGE_REVIEW.md`。不得修改产品/测试/配置/门禁/账本/既有证据；不得 commit/push。

## 1. 必读

1. 规格：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_对抗性审查_问题清单与修复总控提示词_2026-09-30.md`（全部 F01–F08 节 + §11 阶段复审要求）与 `Lingxi_R03_修复验收清单_2026-09-30.json`（34 C-ID）
2. 原阶段 Gate：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R03_运行状态机、并发、取消与恢复.md`（A01–A16）；`05_验收与性能协议.md`
3. 本轮产物：`docs/rust-tauri/R03/repair-current/`（统一问题账 R03_FIX_ISSUES.json、G01–G07 各 E01 报告+R1 审查、R03_FIX_NORMAL/ADVERSARIAL_SELFCHECK、R03_FIX_ACCEPTANCE_RESULTS.json、R03_FIX_HANDOFF.md、INDEPENDENT_REVIEWS/INDEX.md）与 `artifacts/rust-tauri/R03/repair-current/`（各证据根）
4. 现行交接：`docs/rust-tauri/R03/R03_HANDOFF.json`、`R03_REPORT.md`（REOPENED 节）、`R03_ACCEPTANCE_LEDGER.json`
5. 源码：当前 HEAD 实际实现（cancel.rs/task_supervisor.rs/subagents.rs/runs.rs/sessions.rs/dedup.rs/background.rs/limits.rs/invocations.rs/recovery.rs 等）

## 2. 必须独立实测的七条主反例（每条真实命令+退出码；禁止只引用执行者/组审日志或已知 709 绿）

1. **F01**：真实 CancelRegistry/CancelScope——`run_root_under`/`register_linked` 建链后父取消到达全部子孙；**父先取消再创建子节点**：继承取消或拒绝派发（外部适配器调用计数 0）。
2. **F02**：同进程、不重启、不调用 startup_scan——小并发上限下反复父取消**超过上限次数**后：子线程 busy 清除、active 计数回基线、durable 行合法终态、监督登记回收；最后再派发正常子代理成功。
3. **F03**：Provider 已返 Final、最终持久化处受控停驻时取消 Accepted——释放后完成与取消**只有一个胜者**（不得出现取消后 completed/final message；完成先定则取消得 TooLate/已终态）。
4. **F04**：外部计数器加 1 后 Tool panic——journal 仍 **Unknown**（非 Failed/Success）、恢复扫描两次不重做、计数恒 1。
5. **F05**：占满监督容量（或起始持久化故障）提交带 requestId 的后台任务失败→释放后同 key 重试——**无幽灵 Run**（重试真实启动或显式恢复绑定；不得返回不存在的 Run）。
6. **F06**：>2000 字符（含 Unicode/尾部标记）的前后台请求——预算内**完整到达 Provider**（替身实录输入比对）或明确事前拒绝（零受理/零副作用）。
7. **F07**：后台 run 第一轮停驻时 steer Accepted→释放至下一轮——下一轮**实际消费**该追加输入且仅一次；串扰检查（另一会话/后续新 Run 不收到）。

可以直接编写新的最小复测程序/测试（放你的证据目录，用 cargo 真实运行，走真实 service 入口与存储，Provider/Tool 替身只产生外部响应或受控副作用），或以现有 9 个修复套件的定向用例为骨架**自己重新执行并核对断言内容**（不得盲跑计数）。

## 3. 其余阶段复审义务

1. **原 R03 Gate 独立重跑**：`~/.cargo/bin/cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R03 --evidence artifacts/rust-tauri/R03/repair-current/STAGE-REVIEW-RR1/verify-stage-r03`——核 15/15 命令、17/17 场景（16 A-ID+RP01）、48 叶=17 份额+31 递延、candidateSourceBinding stable、testedShaAtEnd=ebcbcad1b。另实跑 fmt/clippy/workspace（`--locked`）至少一次真实退出码。
2. **剩余变体抽查**：从 34 个 C-ID 的 adversarial_variation 中至少另抽 6 条不同 F-ID 的变体独立复测（barrier/固定交错优先；覆盖 C03 并发取消不倒退、C05 重启安全契约、F06-C03 超限拒绝、F07-C04 容量一致性等高风险面）。
3. **R02 回归**：7 条定向链（认证/事务/事件快照/备份损坏库/恢复/重启链/A16 directed）绿；全量 verify-stage R02 的红集与登记归因一致（GOV-01/02 治理递延族；ustar 清理竞态）——不接受新失败被塞进旧类别。
4. **修复未制造回归**：无双重 finalize、重复计数释放、跨主体 key 串用、未知副作用盲重试、旧 steering 投递新 Run、取消先赢后 completed；G01–G06 套件全绿。
5. **问题账完整性**：34 C-ID 三层状态全 CLOSED_BY_INDEPENDENT_REVIEW；账本与实际证据一致；无 NOT_A_DEFECT 被无依据关闭；递延义务（R04+ ask 档、31 叶、R07 九叶、Windows）保持归属。
6. **范围检查**：无 R04 倒灌（未提前实现工具网关/真实供应商/CLI/UI/Tauri 宿主）；正式签名/真实供应商/完整 R07 UI 不是本轮前置；审计治理残留（seal 坐标/patch-too-large）如实单列未伪造解决。
7. **Git/远程**：HEAD=远程=ebcbcad1b；G01–G07 提交链与 R03_FIX_COMMIT_RECEIPTS.json 一致。

## 4. 裁决标准

全部满足 → STAGE_VERDICT: PASS。任一确认问题未真正关闭/新回归/门禁造假 → FAIL（点名 F-ID/C-ID 与事实）。真实能力缺失阻塞 → BLOCKED（点名阻塞条件）。不能以"组审都过了"代替独立实测；也不能用审查报告的静态结论代替运行。无问题就 PASS，不为对抗制造无关改进。

## 5. 环境

一律 `~/.cargo/bin/cargo`（rust-toolchain.toml 锁定 1.98.1；PATH 中 Homebrew cargo 1.93.0 不读取锁定，禁用）；全部 `--locked`；隔离 /tmp 数据根；无网络外发、无真实供应商、无真实用户数据。

## 6. 输出（报告+最后消息）

```text
STAGE_VERDICT: PASS / FAIL / BLOCKED
候选与远程核对
七条主反例逐条：实测方式 / 命令与退出码 / 结果
抽查变体与结果
原 Gate/门禁/R02 回归结果
问题账 34 C-ID 核对结论
发现（如有）/ 需标 STALE（如有）
审查范围声明
```
