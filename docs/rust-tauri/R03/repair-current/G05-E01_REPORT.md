# R03 修复轮 G05-E01 执行报告（F06：真实执行输入被静默截断为 2000 字符）

- 执行代理：EXECUTOR-REPAIR-R03-G05-E01（一次性执行/修复代理；本报告为执行者口径，不含独立审查）。
- 日期：2026-09-30。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。
- 候选起点 `CANDIDATE=d56e6883d`（含已通过独立审查的 G01–G04 修复——本轮**未回退、未破坏**：cancel_link_inheritance 7/0、subagent_closeout 8/0、cancel_terminal_race 13/0、tool_receipt_unknown 6/0、admission_dedup_consistency 5/0、admission_dedup_adversarial 5/0 逐套复跑全绿）。本轮无 commit/push（未获授权）。总控账本（`R03_FIX_ISSUES.json`、`R03_FIX_COMMIT_RECEIPTS.json`）未改动（其未提交变更为派单前已存在）。
- 工具链：`~/.cargo/bin/cargo`（rustup 锁定 1.98.1），全部 `--locked`；`rust/Cargo.lock` 与 HEAD 相同（零依赖变化）。
- 证据根：`artifacts/rust-tauri/R03/repair-current/G05-E01/`（`normal-selfcheck/`、`adversarial-selfcheck/`、`logs/`）。
- 结论：**READY_FOR_REVIEW**（workspace 71 条 test-result-ok / **696 passed / 0 failed** ≥ 底线 69/689/0，增量 = 本轮 7 个新用例；fmt 零 diff；clippy `-D warnings` 零告警；check-contracts（626 entries 零漂移）/ check-boundaries exit 0）。

## 1. 实现范围

F06 本体（前台/后台受理链把执行载荷截断为 2000 字符）及其同族路径（日志长度谎报）。不涉及 F07（后台 steering 消费通道）与其他轮次问题，不进入 R04。

## 2. 根因复核（结论：审查属实，红基线实测复现，无反证）

调用链复核与审查清单一致：

1. `sessions.rs` `execute_submission_for`/`execute_background_for` 用 `submission.input.chars().take(2000).collect()` 生成 `recorded_input`（注释自称 "Bound what we record (defense in depth)"）。
2. 该值**就是执行载荷**：前台传给 `drive_run(.., &recorded_input, ..)`，`runs.rs` 内 `turn_input = input.to_string()` 后直接进入 `provider.next_turn(.., &stream_input)`；后台传给 `spawn_background_drive(.., recorded_input, ..)` 再进同一 `drive_run`——不是日志摘要。
3. `dedup.rs` `normalized_request_digest_hex(submission.input)` 覆盖**全量**输入（CRLF→LF 为唯一声明规范化）→ "判定同一请求"的内容与实际执行内容系统性不一致：长请求尾部要求被静默丢弃且无提示；同 id 重放判定与执行内容错位。
4. settle 日志 `input_chars = recorded_input.chars().count()` 上限恒为 2000——同族谎报（长度也被截断值污染）。

**红基线实测**（隔离 git worktree `/tmp/lingxi-r03-g05-redbase`，HEAD=d56e6883d，未含修复，最终版测试）：`input_payload_fidelity` **0 passed / 5 failed**（`adversarial-selfcheck/red-baseline-input_payload_fidelity-full.log`）；`input_budget_refusal` **编译失败**——`InputTooLarge` 与 `MAX_SUBMISSION_INPUT_BYTES` 不存在，证明修复前无任何显式超限拒收（`adversarial-selfcheck/red-baseline-input_budget_refusal-compile.log`）。审查 3 条 source_facts 全部复现。

### 同族路径清单（输入保真全量枚举，逐一定性）

| 路径 | 修复前事实 | 定性 | 处置 |
|---|---|---|---|
| 前台 `execute_submission_for` 执行载荷 | take(2000) → drive_run → provider | **F06 本体** | 传全量 `submission.input` |
| 后台 `execute_background_for` 执行载荷 | take(2000) → spawn_background_drive → drive_run | **F06 本体** | 传全量 `submission.input.to_string()` |
| settle 日志 `input_chars` | 截断值计数（≤2000 谎报） | 同族 | 记全量 `input_chars`/`input_bytes` 计数；日志不再携带内容，"摘要=计数"而非"摘要=截断内容" |
| dedup 摘要 `normalized_request_digest_hex` | 全量（正确方向） | 语义保持 | 不动；C02 钉"摘要对应实际执行内容"（现在两者同源） |
| `steer_for` 追加输入 | 全文透传 + 有界 inbox（满则 loud `SteeringInboxFull`） | 不受影响 | 不动 |
| subagent 任务输入（`subagents.rs` `task_input = request.task.clone()`） | 全文透传 | 不受影响 | 不动 |
| HTTP 传输层 | `DefaultBodyLimit` = 1 MiB（413 `body_limit_exceeded`，先于 handler）；WS 帧 1 MiB；WS 无提交面 | **正式预算取证** | 服务层预算取同一数字（见 §3.1） |
| 受理拒收面 | 不存在（>1 MiB 仅能由传输层拦；进程内调用者可绕过传输且被截断） | **F06 本体（C03）** | 受理头部显式 `InputTooLarge` |

## 3. 修复设计（最小完整）

### 3.1 一条预算、两道执法腿（红线 1/2/5 的落地）

- **正式预算**：`limits::MAX_SUBMISSION_INPUT_BYTES = DEFAULT_BODY_LIMIT_BYTES`（1 MiB）——与 HTTP body / WS 帧上限同一数字、同一来源（`limits.rs` 常量文档冻结）。不虚构能力：HTTP 路由上 >1 MiB body 仍由框架先 413（既有 `auth_matrix::limits_body_size_and_rate_and_ws_ceiling` 钉），服务层检查是**同一预算**在受理链头部的显式执法腿（覆盖绕过传输的进程内调用者），不是第二个魔法数。
- **受理头部拒收**：`admit_submission` 第一件事（先于 session 读、busy gate、run-id 分配、dedup 预留、任何持久写）检查 `submission.input.len() > MAX_SUBMISSION_INPUT_BYTES` → `SessionExecuteError::InputTooLarge { bytes, limit_bytes }`（两个数字都如实带出）+ warn 日志。前后台共用同一 admission 链，两入口同规则。
- **执行载荷与摘要分离**：前台 `drive_run(.., submission.input, ..)`、后台 `spawn_background_drive(.., submission.input.to_string(), ..)`——全量、字节保真；日志只记 `input_chars`/`input_bytes` 计数（明确"日志摘要=计数，非任务内容"）。
- **HTTP 映射**：`execute_session` handler 新增 `InputTooLarge` → 413 `input_too_large`（`session.input_too_large`），文案明示"resend within the budget — input is never silently truncated"。当前路由不可达（框架 body limit 先拦）但保持面封闭且响亮（与 `BackgroundRegistryFull`/`SteeringInboxFull` 的既有映射策略一致）。

### 3.2 与 G04 的协同（不破坏两阶段绑定）

超限拒收发生在 dedup `admit`（预留）**之前**，因此：不占用 key、不留 Pending/Committed/Unverified 绑定；同 id 随后合法重试 = 全新受理（C03 测试钉：`assert!(!accepted.replayed)` + Provider 真实调用）。G04 的承诺点（durable run-start 后 `commit_durable`）与补偿语义零改动。

### 3.3 禁止项对照（红线 4）

- 不是"2000 换魔法数"：`take(N)` 投影已从 `src/` 消失（仅测试文件文档/构造保留引用）；上限只在拒收分支出现且等于传输层既有预算。
- 未改摘要使不同请求看起来相同：摘要函数零改动，C02 反向钉"同前缀异尾部不合并"。
- 不只测 ASCII 短请求：C01 含 2001–8000；C02 全 Unicode（中文/emoji/组合字符/CRLF）+ 切点横跨组合对。

## 4. 测试（新增 `lingxi-service` 两个集成测试文件，7 用例）

- `tests/input_payload_fidelity.rs`（5）：C01 前台/后台边界长度（1999/2000/2001/3000/8000，尾部 OMEGA 反标记）；C02 Unicode 字节保真（两入口）+ digest 对应实际执行（同 id 重放 / CRLF→LF 声明规范化重放 / 同 id 异尾部 Conflict / 异 id 异尾部各自全量执行 + 摘要互异）+ 组合对横跨历史切点（对抗）。
- `tests/input_budget_refusal.rs`（2）：C03 超限（1 MiB+1）前后台显式拒收、零 run 行/零 started/零模型调用/零工具派发、拒后同 id 合法重试真实受理、预算对齐断言（`MAX_SUBMISSION_INPUT_BYTES == DEFAULT_BODY_LIMIT_BYTES`）；日志摘要超界合法输入不误拒（2001/100_000）。
- 替身纪律：Provider 替身只 push 实收输入全文（唯一观测通道），回复为固定常量；受理链/存储/驱动全部真实组件（与已验收 admission_dedup_consistency 同构的真实组合）。

## 5. 验证汇总

| 项 | 结果 | 证据 |
|---|---|---|
| 红基线（修复前） | input_payload_fidelity 0/5 passed；input_budget_refusal 编译失败 | `adversarial-selfcheck/red-baseline-*.log` |
| G05 用例（修复后） | 7/7 passed（5+2） | `normal-selfcheck/g05-suites-post-fix.log` |
| G01–G04 保护套件 | 6 套全绿（5+5+7+13+8+6，0 failed） | `logs/g01-g04-protected-suites.log` |
| workspace 全量 | 71 条 ok / **696 passed / 0 failed** | `logs/workspace-test-full.log` |
| fmt / clippy -D warnings / Cargo.lock | 零 diff / 零告警 exit 0 / 与 HEAD 相同 | `logs/fmt-check.log`、`logs/clippy-check.log` |
| xtask check-contracts / check-boundaries | 626 entries 零漂移 / OK，均 exit 0 | `logs/xtask-*.log` |

本地结果不代替其他平台、正式打包或真实供应商验证；本轮全部为进程内替身验证（F06 不需要真实供应商）。

## 6. 改动文件

- `rust/crates/lingxi-service/src/limits.rs`：新增 `MAX_SUBMISSION_INPUT_BYTES`（= `DEFAULT_BODY_LIMIT_BYTES`，文档冻结"一条预算两道腿/永不静默截断"）。
- `rust/crates/lingxi-service/src/sessions.rs`：`SessionExecuteError::InputTooLarge`；`admit_submission` 头部预算拒收；前台/后台执行载荷改传全量输入；settle 日志改记全量计数（不再有 `recorded_input` 投影）。
- `rust/crates/lingxi-service/src/lib.rs`：`execute_session` 对 `InputTooLarge` 的 413 `input_too_large` 映射。
- `rust/crates/lingxi-service/tests/input_payload_fidelity.rs`（新）、`rust/crates/lingxi-service/tests/input_budget_refusal.rs`（新）：C01–C03 两层自查用例。

## 7. 对审查结论的反证

无。审查 3 条 source_facts 红基线全部复现，问题属实。
