# R03 对抗性修复派单｜G05-E01（执行代理）

派单时间：2026-09-30。派单人：R03 修复总控编排器。派单性质：一次性执行代理（EXECUTOR-REPAIR-R03-G05-E01）。

## 0. 你是谁、只做什么

你是一次性执行/修复代理，只处理 **G05 = F06**（真实执行输入被静默截断为 2000 字符）及其同根因路径。基线 `FIX_BASE_SHA=cd3fb19e651f763afc6c75cb3163064fb54ca3fe`，当前候选 `CANDIDATE=d56e6883df1969a05830a3053afabdc693583a62`（含已通过独立审查的 G01–G04 修复——**不得回退或破坏**）。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。

**你没有 commit/push 权限。** 总控账本文件为未提交更新——不要改动或还原。

## 1. 必读

1. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_对抗性审查_问题清单与修复总控提示词_2026-09-30.md`（F06 节全文 + 总控规程）
2. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_修复验收清单_2026-09-30.json`（F06 的 3 个 case：R03-FIX-F06-C01..C03）
3. R03 任务书 T01/T08（A15 真实入口驱动状态；输入保真）+ 05 验收协议（替身边界）
4. 现行 `docs/rust-tauri/R03/repair-current/` G01–G04 报告
5. 源码：`rust/crates/lingxi-service/src/sessions.rs`（execute_submission_for/execute_background_for 的 recorded_input 生成，当前 :551/:628 附近 `submission.input.chars().take(2000).collect()`——注意 G04 已重写此区域，以当前 HEAD 为准）、`dedup.rs`（normalized_request_digest_hex 覆盖完整输入）、HTTP 入口对请求体的实际上限（lingxi-service/src/lib.rs 及 http 层）、既有测试（session_serialization、background_*、request_dedup、r03_t08_acceptance_matrix 中涉及输入长度的断言）

## 2. 问题与你的 3 个 C-ID

**F06（P2）**：`execute_submission_for`/`execute_background_for` 用 `chars().take(2000)` 生成 recorded_input 并传给 drive_run/后台驱动（不只是日志摘要）；normalized_request_digest_hex 反而覆盖完整输入——"判定同一请求"的内容与真正执行内容不一致。合法长请求尾部要求被静默丢弃，用户无提示。

- R03-FIX-F06-C01 边界长度和尾部要求完整（1999/2000/2001 及更长但在支持预算内的请求，前台及后台真实受理，Provider 替身只记录实际输入：预算内内容完整；若上限拒绝则显式且零受理/零副作用。对抗：尾部放与前缀相反的明确测试标记，不能靠回复文本猜内容）
- R03-FIX-F06-C02 Unicode 与规范化一致（中文、emoji、组合字符、CRLF 混合长请求，两条入口执行比对：仅允许契约声明的规范化、无隐式字符尾部丢失、去重摘要对应实际执行内容。对抗：同前 2000 字符不同尾部的两请求不被误合并）
- R03-FIX-F06-C03 超过正式支持上限明确拒绝（明确记录的输入/请求体预算已确定后提交超限载荷：返回准确超限错误、不分配假 Run、不写 started、不派工具。对抗：超限只发生在日志摘要时不得误拒合法输入）

## 3. 修复红线

1. **将日志/展示摘要与执行载荷分离**；支持预算内完整输入传到执行接口。
2. 确需上限则在**受理及任何副作用前明确拒绝**并说明限制，不能静默截断；不要求无限接受任意大小输入。
3. 统一合法规范化与摘要规则，覆盖 Unicode、换行和前后台路径；明确哪些是日志摘要而非任务内容。
4. **禁止只把 2000 改成另一个魔法数字**；禁止改摘要让截断后两条不同请求看起来相同；禁止只测 ASCII 短请求。
5. 与 G04 摘要链协同（dedup 摘要覆盖完整输入的语义保持）；不破坏 G01–G04 套件；HTTP 层既有请求体上限如实取证（如果 HTTP 层已有更小上限，须如实报告并以其为正式预算的一部分，不得虚构不存在的能力）。

若认为审查有误：给当前源码调用链或可复现反证，交总控转全新 Reviewer。

## 4. 环境与产出

- 一律 `~/.cargo/bin/cargo`（锁定 1.98.1；PATH 中 Homebrew cargo 1.93.0 禁用）；全部 `--locked`；隔离 /tmp 数据根；无网络外发。
- 证据：`artifacts/rust-tauri/R03/repair-current/G05-E01/`（normal-selfcheck/、adversarial-selfcheck/、logs/）；报告 `docs/rust-tauri/R03/repair-current/G05-E01_REPORT.md` + `G05-E01_NORMAL_SELFCHECK.md` + `G05-E01_ADVERSARIAL_SELFCHECK.md`（逐 C-ID：攻击窗口→观测→是否推翻→命令/退出码→证据）。
- 回归底线：workspace ≥ 69 suites/689 passed/0 failed（允许增加）；fmt 零 diff；clippy -D warnings 零告警；Cargo.lock 不变；G01–G04 套件保持绿。
- Provider 替身只记录实际收到的输入（不得由回复文本推断）；不得 mock 被测的受理链/真实存储。

## 5. 返回格式

结论（READY_FOR_REVIEW / FAIL / BLOCKED）、改动文件清单、逐 C-ID 两层自查状态表（含证据路径）、workspace 测试统计、对审查结论的反证（如有）。
