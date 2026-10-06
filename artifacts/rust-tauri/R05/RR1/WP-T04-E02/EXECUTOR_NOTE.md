# WP-T04 R2（F33）证据说明 — R05-T04-修复-r1（2026-10-05）

范围：F33 仅为测试电池补强，**零生产代码改动**（本轮唯一改动文件
`rust/crates/lingxi-adapters/tests/r05_t04_rr1_batch_terminal.rs`，
生产四族 Some(other)/other 响亮分支在登记时与本轮均经亲读确认已响亮）。

## 新增腿（r05_t04_rr1_batch_terminal.rs）

1. `present_but_unmapped_terminal_values_are_loud_and_name_the_value`（缓冲模式，五腿）：
   - openai-completions `finish_reason:"weird"` → Err(InvalidMessage)、不可重试、消息含 `UNKNOWN finish_reason` 且点名 `"weird"`；正对照：同体 `finish_reason:"stop"` → Final。
   - anthropic `stop_reason:"pause_turn"` → Err(InvalidMessage)（真实 Anthropic 值、本 adapter 未映射）；正对照：`end_turn` → Final。
   - google `finishReason:"OTHER"` → Err(InvalidMessage)（真实 Gemini 值、未映射）；正对照：`STOP` → Final。
   - responses `status:"odd"` → Err(InvalidMessage)；正对照：`completed` → Final。
   - responses `status:"incomplete"` + `incomplete_details.reason:"weird_incomplete_reason"` → Err(InvalidMessage)、消息含 `UNMAPPED reason` 且点名该值；正对照：`max_output_tokens` → Failed(BudgetExceeded, retryable=true)。
2. `unmapped_terminal_values_through_the_stream_accumulators_stay_loud`（流式模式，同类路径）：
   chat 流（delta 携带 `finish_reason:"weird"` + [DONE]）与 anthropic 流（message_delta 携带 `stop_reason:"pause_turn"`）都经流 accumulator finish() 重建缓冲体走同一 parser，断言同样的响亮拒绝与值点名。google/responses 流 finish 同样复用缓冲 parser（读码核实：google_generative_ai.rs:1180、openai_responses.rs:941），由缓冲腿钉住的分类器共享覆盖。

## 自检（真实工作树，rustup 代理 /Users/study_superior/.cargo/bin/cargo → 1.98.1）

- green-battery-f33.log：`cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --test r05_t04_rr1_batch_terminal` → exit 0，9/9（既有 7 + 新 2）。
- green-adapters-full-f33.log：`cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters` → exit 0，22 个测试目标、294 通过、0 失败。
- green-service-t04-f33.log：`cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t04_streaming` → exit 0，18/18（共享 parser 的服务级回归）。
- green-clippy-f33.log：`cargo clippy --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --all-targets -- -D warnings` → exit 0。
- green-fmt-check-f33.log：`cargo fmt --manifest-path rust/Cargo.toml --all -- --check` → exit 0（新腿先按 rustfmt 归一后复检）。

## 变异验证（mut1–mut5：证明每条腿真的咬得住）

方法：隔离副本 `/tmp/r05-t04-f33-mutate`（复制 rust/Cargo.toml、Cargo.lock、crates/、
rust-toolchain.toml，独立 target），**候选工作树零接触**；每轮只放宽一个分类分支后跑同一
battery，期望恰好对应腿红、其余绿（exit 101）。

| # | 变异（模拟未来放宽映射的回归） | 结果 |
|---|---|---|
| mut1 | openai_completions.rs `Some(other)`（UNKNOWN finish_reason）→ 静默接受 | 缓冲 openai 腿红（:275）+ 流式 openai 腿红（:431），7 绿 2 红 exit 101 |
| mut2 | anthropic_messages.rs `Some(other)`（UNKNOWN stop_reason）→ 静默接受 | 缓冲 anthropic 腿红（:302）+ 流式 anthropic 腿红（:453），7 绿 2 红 exit 101 |
| mut3 | google_generative_ai.rs `Some(other)`（UNKNOWN finishReason）→ 静默接受 | 缓冲 google 腿红（:332），8 绿 1 红 exit 101 |
| mut4 | openai_responses.rs `Some(other)`（UNKNOWN status）→ 静默接受 | 缓冲 responses status 腿红（:354），8 绿 1 红 exit 101 |
| mut5 | openai_responses.rs incomplete `other`（UNMAPPED reason）→ 静默归类 BudgetExceeded | 缓冲 responses incomplete 腿红（:381），8 绿 1 红 exit 101 |

副本基线（变异前）9/9 绿。变异后工作树未改动生产文件：仓库四族 parser 与隔离副本原始拷贝
`diff -q` 逐字节一致（退出码 0）。/tmp 副本为一次性验证环境，测试后保留日志、副本可弃。
