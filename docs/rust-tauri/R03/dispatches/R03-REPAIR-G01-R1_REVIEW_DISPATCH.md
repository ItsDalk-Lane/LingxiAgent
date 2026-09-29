# R03 对抗性修复派单｜G01-R1（独立 Reviewer，第 1 轮）

派单时间：2026-09-30。派单人：R03 修复总控编排器。你是一次性独立对抗性 Reviewer：REVIEWER-REPAIR-R03-G01-R1，未参与 G01 候选的实现或修复。

## 0. 范围与对象

只验 G01（F01＋F02，10 个 C-ID：R03-FIX-F01-C01..C05、R03-FIX-F02-C01..C05），第 1 轮。

- 候选：分支 `codex/rust-tauri-migration`，HEAD `cd3fb19e651f763afc6c75cb3163064fb54ca3fe` + **未提交工作树**（`git status --porcelain` 见 5 个产品源文件修改：cancel.rs/task_supervisor.rs/subagents.rs/runs.rs/background.rs；4 个测试文件：cancellation_tree.rs、r03_t08_acceptance_matrix.rs 修改 + cancel_link_inheritance.rs、subagent_closeout.rs 新增；另有总控账本/标注与 G01-E01 报告产物，不在审查范围）。审查期间候选冻结：任何人不改工作树。
- 执行者证据根：`artifacts/rust-tauri/R03/repair-current/G01-E01/`（normal-selfcheck/、adversarial-selfcheck/、logs/、commands.json）；报告 `docs/rust-tauri/R03/repair-current/G01-E01_REPORT.md` 与两个 SELFCHECK 文档。
- 你的复测产物**只**写入 `artifacts/rust-tauri/R03/repair-current/G01-R1/`，报告写 `docs/rust-tauri/R03/repair-current/G01-R1_REVIEW.md`。**不得修改产品、测试、配置、门禁、账本（R03_FIX_ISSUES.json）与 G01-E01 证据**。

## 1. 必读

1. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_对抗性审查_问题清单与修复总控提示词_2026-09-30.md`（F01/F02 节 + 总控规程 §6 第三层要求）
2. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_修复验收清单_2026-09-30.json`（10 个 case 的 given/when/then/adversarial_variation/evidence_required）
3. R03 任务书（A01–A16 语义，特别是 A05/A06/A15）与 05 验收协议
4. 真实 diff（`git diff` 全量 + 2 个新测试文件全文）、调用链与执行者三层报告
5. 现行 R03_REPORT/R03_HANDOFF（理解原 FINDING-1 两阶段收口为何被重开）

## 2. 独立核查要求（不能只复述执行者日志或相信其结论）

1. **逐 C-ID**：核源码与证据绑定；实际重跑每个 F-ID 最关键反例（至少 F01-C01/C03、F02-C01/C03 各一次独立运行，其余可复核执行者证据+定向抽查）；涉及竞态用固定调度/barrier 复测。
2. **F02 红线专项**：普通父取消必须在数据库健康、服务不重启的情形下，同进程核 DB 终态、thread.busy、active 计数、TaskSupervisor 登记四者同时收口；**不得把重启后恢复当取消通过**（检查测试代码里无 startup_scan/重启辅助普通取消路径）；真正进程崩溃的 startup recovery 测试保留且仍绿。
3. **测试语义修改审查**：执行者改了 `r03_t08_acceptance_matrix.rs`（父取消用例）与 `cancellation_tree.rs`（a06 演示子）。判断这是把旧的错误预期（重启收口）更正为正确语义（同进程收口），还是削弱了原 A06/A15 保护——对照原验收断言逐条比对：监督/隔离/配额/无 final message 断言是否保留；崩溃恢复链（recovery_* 套件）是否原样。
4. **修复没有制造**：双重 finalize、重复计数释放、跨 child_run_id 误清 busy、幽灵 Running/无句柄条目、取消先赢后仍 completed、无关树被连带取消、首次取消原因被覆盖、`note_child_finished` 改 pub 是否引入生产面风险。
5. **红基线真实性**：执行者的 RED 日志声称来自旧代码。可用 `git worktree add /tmp/r03-g01-redcheck cd3fb19e6`（隔离副本，用后 `git worktree remove`）把新测试文件拷入旧代码复跑 1–2 个关键反例，验证旧代码确实红（或按日志/提交差异给出充分静态依据）。不得在当前工作树上制造红灯。
6. **门禁复跑**：`~/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --workspace --locked`（期望 65/648/0）+ fmt/clippy（-D warnings）至少各一次真实退出码。
7. 允许以独立证据判 NOT_A_DEFECT（须给源码/实测反证）；无依据不得删除问题或弱化要求；没有问题就 PASS，不为对抗制造无关改进；后续阶段（R04+）独立功能缺失不计本轮 FAIL。

## 3. 环境

一律 `~/.cargo/bin/cargo`（rustup 锁定 1.98.1；PATH 中 Homebrew cargo 1.93.0 不读取锁定，禁用）；全部 `--locked`；隔离 /tmp 数据根；无网络外发。

## 4. 输出格式（报告+最后消息）

```text
VERDICT: PASS / FAIL / BLOCKED
候选：HEAD+工作树摘要（diff --stat）
逐 C-ID：正常自查核对 / 对抗性变体 / 独立复测命令与退出码 / 证据路径
finding（如有）：位置、重现、违反要求、影响、根因、同族范围
误判反证（如有）：事实与裁决建议
需标 STALE 的旧证据（如有）
审查范围声明：未修改产品/测试/配置/门禁
```

PASS 后结束本次代理，不参加下一轮；下一轮必须新建 Reviewer。
