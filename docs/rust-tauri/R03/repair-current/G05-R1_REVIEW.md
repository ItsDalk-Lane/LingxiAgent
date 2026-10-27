# R03 对抗性修复审查 G05-R1（F06：真实执行输入被静默截断为 2000 字符）

- 审查代理：REVIEWER-REPAIR-R03-G05-R1（一次性独立对抗性 Reviewer，未参与 G05 候选的实现或修复）。
- 日期：2026-09-30。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。
- 候选：HEAD `d56e6883d` + 未提交工作树（`limits.rs` / `sessions.rs` / `lib.rs` + 新测试 `input_payload_fidelity.rs`(5) / `input_budget_refusal.rs`(2)）。总控账本未提交变更经核为控制器登记（G04 回执 + G05 注册），按派单 §0 不在审查范围。
- 工具链：一律 `~/.cargo/bin/cargo`（1.98.1，rustup 锁定），全部 `--locked`；`rust/Cargo.lock` 与 HEAD 相同（零依赖变化）。本轮无 commit/push；复测产物只写 `artifacts/rust-tauri/R03/repair-current/G05-R1/`。

## VERDICT: PASS

## 候选摘要

`MAX_SUBMISSION_INPUT_BYTES = DEFAULT_BODY_LIMIT_BYTES`（1 MiB，同一数字同一来源，limits.rs 文档冻结"一条预算两道执法腿"）；`admit_submission` 头部（先于 session 读、busy gate、run-id 分配、dedup 预留与任何持久写）显式拒收 `InputTooLarge { bytes, limit_bytes }`；前台 `drive_run(.., submission.input, ..)`、后台 `spawn_background_drive(.., submission.input.to_string(), ..)` 全量字节保真传递；settle 日志只记 `input_chars`/`input_bytes` 计数不记内容；`execute_session` 对 `InputTooLarge` 映射 413 `input_too_large`。`take(2000` 从 `src/` 全仓消失。

## 逐 C-ID 结果

| C-ID | 对抗性核查 | 独立复测命令（真实） | 退出码 | 证据（本轮，`artifacts/.../G05-R1/`） |
|---|---|---|---|---|
| R03-FIX-F06-C01 | 1999/2000/2001/3000/8000 双入口；尾部反向 OMEGA 标记逐字符断言（不靠回复文本）；红基线旧码断言差异直接显示交付被截为 2000 字符（`...OMEGA_2001_7QXZ_EN` 缺尾 `D`） | `~/.cargo/bin/cargo test --locked --manifest-path rust/Cargo.toml -p lingxi-service --test input_payload_fidelity c01_` → 2 passed / 0 failed | 0 | `c01-independent-rerun.log`；红基线 `red-baseline-input_payload_fidelity.log`（0 passed / 5 failed，exit 101） |
| R03-FIX-F06-C02 | Unicode/emoji/组合字符/CRLF 双入口字节保真；组合对横跨历史 2000 切点；同 id 重放零执行、CRLF→LF 声明规范化重放、同前 2000 异尾部 `DuplicateRequestConflict` 不误合并、异 id 异尾部各自全量执行且实收内容摘要互异；`dedup.rs` 零改动（摘要本就全量） | 同上 `--test input_payload_fidelity c02_` → 3 passed / 0 failed | 0 | `c02-independent-rerun.log` |
| R03-FIX-F06-C03 | 1 MiB+1 字节双入口 `InputTooLarge{bytes=limit+1,limit_bytes=limit}`；零 run 行 / 零 started / 零模型调用 / 零工具派发；同 id 合法重试真实受理（无绑定残留）；对抗变体：2001 与 100_000 字符（仅日志摘要超界）受理且全量执行不误拒；预算对齐断言在测试内钉死 | `~/.cargo/bin/cargo test --locked --manifest-path rust/Cargo.toml -p lingxi-service --test input_budget_refusal` → 2 passed / 0 failed | 0 | `c03-independent-rerun.log`；红基线 `red-baseline-input_budget_refusal.log`（编译失败 5 errors，exit 101：旧码无 `InputTooLarge`/`MAX_SUBMISSION_INPUT_BYTES`，证明修复前无显式拒收） |

## 红线审查（"不是 2000→另一个魔法数"）

1. **一条预算两道执法腿，取证属实**：HTTP 腿 `DefaultBodyLimit::max(limits::DEFAULT_BODY_LIMIT_BYTES)` 于 `lib.rs:3230`（既有 413 钉：`tests/auth_matrix.rs:1700-1713` `limits_body_size_and_rate_and_ws_ceiling` 断言 >1 MiB body → 413）；WS 腿 `WS_FRAME_LIMIT_BYTES`（1 MiB）于 `ws.rs:441`，且 `ws.rs:417` 拒绝分片帧（无分片绕过；另核实 WS 无提交面）。服务层腿 `MAX_SUBMISSION_INPUT_BYTES = DEFAULT_BODY_LIMIT_BYTES`（`limits.rs:33`）在 `admit_submission` 头部执法，覆盖绕过传输的进程内调用者。预算数字取自既有传输常量，非新造。
2. **执行载荷与日志摘要分离**：载荷=全量 `submission.input`（前台 `sessions.rs:682`、后台 `sessions.rs:803`；`runs.rs:701→760→770` 证实 `input` → `turn_input` → `provider.next_turn`，替身观测即真实载荷）；日志只记计数（`sessions.rs:730-731` `input_chars`/`input_bytes`，无内容字段）。
3. **受理与副作用前拒收**：预算检查位于 `admit_submission` 第一件事（`sessions.rs:872-885`），先于 `get_session_erased`、intake gate、busy gate、run-id 分配、dedup 预留与任何持久写——测试钉零 run 行/零 started/零模型调用/零工具派发。
4. **不误拒**：拒收阈值 1 MiB（字节），与"日志摘要界（2000 字符）"无关；2001/100_000 字符输入受理且全量执行（`c03_log_summary_oversized_legal_input_is_not_misrefused`）。
5. **`take(2000` 在 src/ 消失**：`grep -rn "take(2000" rust/crates/*/src/` exit 1（无匹配）；残留 `take(` 均为无关（`Option::take`、测试 mock 分页 `take(limit)`、`subagents.rs:1049` 输出侧结果摘要，见 finding-2）。
6. **G04 协同**：拒收先于 dedup `admit` 预留，不留任何绑定；同 id 合法重试 fresh 受理（c03 测试钉 `!replayed` + Provider 真实调用）。G01–G04 保护套件复跑：`--test cancel_link_inheritance --test subagent_closeout --test cancel_terminal_race --test tool_receipt_unknown --test admission_dedup_consistency --test admission_dedup_adversarial` → 44 passed / 0 failed，exit 0（`g01-g04-protected-suites-rerun.log`）。

## 门禁（真实命令与退出码）

| 门禁 | 命令 | 结果 | 退出码 | 证据 |
|---|---|---|---|---|
| workspace 全量 | `~/.cargo/bin/cargo test --locked --manifest-path rust/Cargo.toml --workspace` | **71 suites ok / 696 passed / 0 failed**（与期望 71/696/0 一致） | 0 | `workspace-test-full.log` |
| fmt | `~/.cargo/bin/cargo fmt --check --manifest-path rust/Cargo.toml --all` | 零 diff | 0 | `fmt-check.log` |
| clippy | `~/.cargo/bin/cargo clippy --locked --manifest-path rust/Cargo.toml --workspace --all-targets -- -D warnings` | 零告警 | 0 | `clippy-check.log` |

注：`cargo fmt --check` 不带 `--all` 时在 workspace 根报 "Failed to find targets"（调用方式怪癖，非代码问题），带 `--all` 后零 diff。

## 红基线真实性（本轮独立重做）

`git worktree add /tmp/r03-g05-redcheck d56e6883d`，拷入**当前工作区最终版**两个新测试后复跑，用后 remove（已确认清出 worktree 列表）：

- `input_payload_fidelity`：**0 passed / 5 failed，exit 101**——交付载荷为前 2000 字符投影，尾部 OMEGA 标记被切断（直接反例文本在日志中）。
- `input_budget_refusal`：**编译失败，exit 101**（5 errors：`InputTooLarge` 变体与 `MAX_SUBMISSION_INPUT_BYTES` 不存在）——修复前无任何显式超限拒收路径。

## Finding 清单

无阻断性 finding。两条范围外观察（均不构成 G05 缺陷）：

1. **证据形式小瑕疵（不影响结论）**：执行者红基线日志中 panic 行号与最终版测试文件行号不完全对应（红基线用了早期版本测试）。本轮已用最终版测试独立重做红基线并复现红（见上），结论不受影响。
2. **范围外观察（NOT_A_DEFECT for G05/F06）**：`subagents.rs:1049` `delivery_text` 把子运行结果回送父线程的摘要截为 200 字符——这是**输出侧**结果摘要（代码注释明示既有 block_update 语义），不是提交输入路径；F06 范围是提交执行输入。如未来审查范围扩展到"子结果回送保真"可另行立项。

## 误判反证 / 需标 STALE

- 误判反证：无需——本轮未发现针对修复的反例观测。
- 需标 STALE：无。

## 审查范围声明

仅审查 G05（F06，R03-FIX-F06-C01..C03）：规格（`Lingxi_R03_对抗性审查_问题清单与修复总控提示词_2026-09-30.md` F06 节 + `Lingxi_R03_修复验收清单_2026-09-30.json` 3 case）、候选真实 diff 与调用链（limits/sessions/lib/runs/background/dedup/ws）、执行者三层报告与证据、独立复测（C01/C03 全量重跑、C02 定向抽查、红基线 worktree、G01–G04 保护套件、workspace/fmt/clippy 门禁、grep 复核）。不涉及 F07/F08 及其他工作单；总控账本未提交变更（控制器登记）不在审查范围。本地进程内替身结果不代替其他平台、正式打包或真实供应商验证。
