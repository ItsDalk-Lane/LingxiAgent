# R03 对抗性修复派单｜G06-E01（执行代理）

派单时间：2026-09-30。派单人：R03 修复总控编排器。派单性质：一次性执行代理（EXECUTOR-REPAIR-R03-G06-E01）。

## 0. 你是谁、只做什么

你是一次性执行/修复代理，只处理 **G06 = F07**（后台运行接受 steering 却没有传入消费通道）及其同根因路径。基线 `FIX_BASE_SHA=cd3fb19e651f763afc6c75cb3163064fb54ca3fe`，当前候选 `CANDIDATE=8883923a5b30a2a6c0face0c42a9ea143db9a605`（含已通过独立审查的 G01–G05 修复——**不得回退或破坏**）。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。

**你没有 commit/push 权限。** 总控账本文件为未提交更新——不要改动或还原。

## 1. 必读

1. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_对抗性审查_问题清单与修复总控提示词_2026-09-30.md`（F07 节全文 + 总控规程）
2. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_修复验收清单_2026-09-30.json`（F07 的 4 个 case：R03-FIX-F07-C01..C04）
3. R03 任务书 T02（A03 steering/follow-up 语义区分）+ T06（A12 断线≠取消）+ 02 目标契约 §4
4. 现行 `docs/rust-tauri/R03/repair-current/` G01–G05 报告（当前受理/绑定/取消/裁决语义）
5. 源码：`rust/crates/lingxi-service/src/background.rs`（spawn_background_drive 当前形态——G04 后已改，drive_run steering 参数仍传 None）、`sessions.rs`（steer_for/steering_inbox/session lease）、`runs.rs`（drive_run 的 Some(inbox) 每轮 drain 逻辑）、既有测试（session_serialization、background_*、r03_t08_acceptance_matrix 中 steering 相关断言）

## 2. 问题与你的 4 个 C-ID

**F07（P2）**：`steer_for` 依据真实 session gate 接受忙会话追加输入；前台 `drive_run` 使用 `Some(lease.steering_inbox())`，但 `spawn_background_drive` 虽持有 lease 却给同一参数传 `None`；`drive_run` 只在 `Some(inbox)` 时每轮 drain——已接受的后台追加输入不会到下一模型轮。未消费文本的后续归属也需检查（防残留进入另一个 Run）。

- R03-FIX-F07-C01 后台 Accepted 后下一轮收到（真实后台 drive 第一轮等待、session 仍 busy；steer 返回 Accepted 后释放 Provider 至下一轮：下一轮准确且只收到一次追加内容。对抗：多次追加、不同顺序、循环多轮，不得重复 drain 同一内容）
- R03-FIX-F07-C02 跨会话和跨运行不串用（两个后台会话并行、其中一个追加要求：要求只到授权目标运行，不进另一会话或无关下一 Run。对抗：同一会话取消后快速开始新 Run，旧回调不能投递错归属）
- R03-FIX-F07-C03 取消和终态边界可解释（steering 已接受但未到消费点时取消/完成当前 Run：按明确契约保留/退回/记录未消费，不假称已用于执行。对抗：错过消费时点不得悄悄触发新任务）
- R03-FIX-F07-C04 容量和前后台一致性（有界 inbox 容量很小、前台/后台均运行：容量内真实消费；超限显式拒绝且不污染队列。对抗：前后台使用同样契约和授权，不因入口改变结果）

## 3. 修复红线

1. 后台驱动接入**同一被授权、属于当前会话/运行的追加输入通道**，保持下一安全模型轮消费规则（与前台同源，不是另造一套）。
2. 保证一次消费、身份隔离、容量限制、取消/终止后的明确处置；**不得把旧追加内容悄悄带入不相关新任务**。
3. 修运行层接线即可，不提前迁移 R07 整个客户端，**不通过禁止所有后台 steering 改变原契约**。
4. 禁止：把 Accepted 换成假成功空响应；全部后台 steering 直接丢弃或禁用；只补前台测试。
5. 不破坏 G01–G05 语义与套件（cancel_link_inheritance/subagent_closeout/cancel_terminal_race/tool_receipt_unknown/admission_dedup_*/input_* 保持绿）；A03/A12 既有断言保持。

若认为审查有误：给当前源码调用链或可复现反证，交总控转全新 Reviewer。

## 4. 环境与产出

- 一律 `~/.cargo/bin/cargo`（锁定 1.98.1；PATH 中 Homebrew cargo 1.93.0 禁用）；全部 `--locked`；隔离 /tmp 数据根；无网络外发。
- 证据：`artifacts/rust-tauri/R03/repair-current/G06-E01/`（normal-selfcheck/、adversarial-selfcheck/、logs/）；报告 `docs/rust-tauri/R03/repair-current/G06-E01_REPORT.md` + `G06-E01_NORMAL_SELFCHECK.md` + `G06-E01_ADVERSARIAL_SELFCHECK.md`（逐 C-ID：攻击窗口→观测→是否推翻→命令/退出码→证据）。
- 回归底线：workspace ≥ 71 suites/696 passed/0 failed（允许增加）；fmt 零 diff；clippy -D warnings 零告警；Cargo.lock 不变。
- Provider 替身只记录每轮实际输入；不得 mock 被测的 session gate/受理链/驱动。

## 5. 返回格式

结论（READY_FOR_REVIEW / FAIL / BLOCKED）、改动文件清单、逐 C-ID 两层自查状态表（含证据路径）、workspace 测试统计、对审查结论的反证（如有）。
