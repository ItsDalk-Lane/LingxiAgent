# R03 修复轮 G05-E01 普通自查（F06：真实执行输入被静默截断为 2000 字符）

- 执行代理：EXECUTOR-REPAIR-R03-G05-E01。日期：2026-09-30。
- 工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`，候选起点 `d56e6883d`（含 G01–G04）。
- 命令一律 `~/.cargo/bin/cargo`（1.98.1），全部 `--locked`。
- 证据根：`artifacts/rust-tauri/R03/repair-current/G05-E01/`。

## 逐 C-ID 普通自查

### R03-FIX-F06-C01｜边界长度和尾部要求完整 — PASS

- 命令：`~/.cargo/bin/cargo test --locked -p lingxi-service --test input_payload_fidelity c01`（退出码 0）。
- 用例（真实受理链：真实 SQLite、真实 gate/dedup/supervisor；Provider 替身**只记录实际收到的输入**，回复为与输入无关的固定文本）：
  - `c01_foreground_boundary_lengths_deliver_the_full_input`：1999/2000/2001/3000/8000 字符，前台 `execute_for`，逐条断言 Provider 收到**逐字符等于原请求**的输入（尾部 OMEGA 标记完整），5 条 durable run、0 次工具派发。
  - `c01_background_boundary_lengths_deliver_the_full_input`：同长度集合，后台 `execute_background_for`（显式 requestId），等待 durable 终态 `completed` 后断言同样内容。
- 证据：`normal-selfcheck/g05-suites-post-fix.log`。
- 红基线（修复前 HEAD=d56e6883d，隔离 worktree）：同两用例 FAIL（recorded input 被截为 2000 字符）——`adversarial-selfcheck/red-baseline-input_payload_fidelity-full.log`。

### R03-FIX-F06-C02｜Unicode 与规范化一致 — PASS

- 命令：`~/.cargo/bin/cargo test --locked -p lingxi-service --test input_payload_fidelity c02`（退出码 0）。
- 用例：
  - `c02_unicode_payloads_byte_honest_on_both_entries`：中文 + 非 BMP emoji（🦊🚀💡）+ 组合字符（e+U+0301、a+U+0308）+ CRLF 混合、总长 >2000 字符；前台与后台两条入口的 Provider 记录输入与原请求**字节相等**（无隐式规范化：CRLF 保持、组合序列保持分解形、emoji 完整）。
  - `c02_dedup_digest_corresponds_to_the_content_actually_executed`：
    - 同 id + 字节相同内容 → idempotent Replay（Provider 计数不变）；
    - 同 id + 仅 CRLF→LF 差异 → Replay（**契约声明的唯一规范化**，dedup.rs 既有语义保持）；
    - 同 id + 同前 2000 字符异尾部 → `DuplicateRequestConflict`（不合并、不执行）；
    - 异 id + 同前 2000 字符异尾部 → 两次**全量各自**执行（两条 run），`normalized_request_digest_hex(calls[0]) != (calls[1])`——去重摘要对应实际执行内容。
  - `c02_adversarial_combining_pair_straddling_the_historical_cut`：组合字符对精确横跨第 2000/2001 字符位（旧 `take(2000)` 会丢重音符），修复后完整到达。
- 证据：`normal-selfcheck/g05-suites-post-fix.log`；红基线同用例 FAIL。

### R03-FIX-F06-C03｜超过正式支持上限明确拒绝 — PASS

- 命令：`~/.cargo/bin/cargo test --locked -p lingxi-service --test input_budget_refusal`（退出码 0）。
- 正式预算取证（配置来源）：
  - HTTP 路由层 `DefaultBodyLimit::max(limits::DEFAULT_BODY_LIMIT_BYTES)`（lib.rs 路由组装）+ 裸 body 读取 `to_bytes(body, DEFAULT_BODY_LIMIT_BYTES)`，超限由框架先 413（`body_limit_exceeded`；auth_matrix `limits_body_size_and_rate_and_ws_ceiling` 既有钉）。`DEFAULT_BODY_LIMIT_BYTES = 1 MiB`。
  - WS 帧上限 `WS_FRAME_LIMIT_BYTES = 1 MiB`（limits.rs）；WS 无 execute 提交面（只读）。
  - 本轮新增服务层受理预算 `MAX_SUBMISSION_INPUT_BYTES = DEFAULT_BODY_LIMIT_BYTES`（同一数字、一条预算两道执法腿；测试内 `assert_eq!` 钉死对齐）。
- 用例：
  - `c03_over_budget_refused_loudly_with_zero_side_effects`：1 MiB+1 字节输入，前台与后台均返回 `InputTooLarge { bytes: limit+1, limit_bytes: limit }`；**零** run 行、**零** started 写入（`status != 'queued'` 计数为 0）、**零** Provider 调用、**零**工具派发；且超限拒绝不留 id→run 绑定——同 id 合法重试真实受理（G04 语义保持）。
  - `c03_log_summary_oversized_legal_input_is_not_misrefused`：2001 字符（超历史日志摘要界）与 100_000 字符合法输入均受理并全量执行——日志摘要层面的"超限"不误拒合法输入。
- 证据：`normal-selfcheck/g05-suites-post-fix.log`；红基线：`adversarial-selfcheck/red-baseline-input_budget_refusal-compile.log`（修复前 `InputTooLarge`/`MAX_SUBMISSION_INPUT_BYTES` 不存在，无法编译——旧代码无任何显式拒收）。

## 回归

- workspace 全量：`--workspace` 71 条 `test result: ok`（含新增 2 个测试二进制）/ **696 passed / 0 failed**（底线 ≥69 suites/689/0；增量 = 本轮 7 个新用例）。证据 `logs/workspace-test-full.log`。
- G01–G04 保护套件逐套复跑全绿：admission_dedup_adversarial 5/0、admission_dedup_consistency 5/0、cancel_link_inheritance 7/0、cancel_terminal_race 13/0、subagent_closeout 8/0、tool_receipt_unknown 6/0。证据 `logs/g01-g04-protected-suites.log`。
- `cargo fmt --all --check` 零 diff（exit 0）；`cargo clippy --workspace --all-targets -- -D warnings` exit 0 零告警；`Cargo.lock` 与 HEAD 相同（零依赖变化）。
- xtask check-contracts（626 entries 零漂移）与 check-boundaries 均 exit 0。证据 `logs/xtask-check-contracts.log`、`logs/xtask-check-boundaries.log`。

## 结论

三个 C-ID 普通自查全部 PASS，无已知未执行项。
