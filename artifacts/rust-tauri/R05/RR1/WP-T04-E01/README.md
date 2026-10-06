# WP-T04-E01 — R05 RR1 工作包 R05-T04 执行者自检证据（R1，2026-10-05）

- 执行者：R05-T04-执行者（首任，全新上下文）。分支 codex/rust-tauri-migration；
  工具链 rustup 代理 `/Users/study_superior/.cargo/bin/cargo`（解析仓库根 rust-toolchain.toml 锁定的 1.98.1）。
- F-ID：F11（整批准入先于副作用）、F12（传输结束≠协议完成；仅过程内容不得产生 final）、
  F13（规范化结果成为实时/final/快照/持久化读取的共同来源）。
- 旧行为反例（先红）：断言原文迁移自证据包 audit/protocol/src/lib.rs（PF05/PF06 腿）与
  audit/closed_loop（CL-02/CL-03 形状），仅补合法前置，未削弱行为断言。
  - 修复前（当前候选树，本会话真实执行）：
    `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --test r05_t04_rr1_batch_terminal`
    → 6 failed / 1 passed（`duplicate_provider_ids_with_conflicting_arguments_reject_whole_batch`、
    `identical_resends_collapse_but_distinct_ids_stay_independent`、
    `anthropic_open_tool_block_cannot_close_as_a_completed_batch`、
    `openai_done_without_finish_reason_does_not_make_final`、
    `thinking_only_and_opaque_only_do_not_form_final_answers`、
    `buffered_bodies_without_a_normal_stop_reason_are_loud` 红；正面对照
    `positive_controls_normal_terminals_and_legal_batches_still_work` 绿）。
    `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t04_streaming rr1_`
    → 8 failed / 1 passed（F11 零副作用/同 ID 冲突/重发去重、F12 reasoning-only/mood-only/
    unresolved 文本、F13 同源投影红；对照 `rr1_f11_distinct_ids_with_equal_arguments_both_execute` 绿）。
    上述输出为本会话命令原始回显（01-old-red-*.log 为捕获摘录，完整 stdout 未逐字节落盘，以本说明为准）。
- 修复后证据（本目录日志为本会话真实输出）：
  - `03-postfix-adapters.log`：五套 adapters 测试（rr1_batch_terminal 7、t04_streaming 18、
    t03_goldens 3、t03_rr1_replay 33、t07_usage_families 14），exit 0。
  - `04-postfix-service-affected.log`：受影响 service 套件（t04_streaming 18、t08_closed_loop 11
    含新增正式二进制重启同源腿、t03_protocol_adapters 12、r04_t01 6、r04_t02 13、r04_t03 10、
    r04_t04 11），exit 0。
  - `02-full-suite-no-fail-fast.log`：`cargo test -p lingxi-kernel -p lingxi-adapters -p lingxi-service
    --no-fail-fast` → 90 result 行、1221 通过、1 失败 = `r00_management_leaves` 的
    LAN 浏览器腿（测试自身输出明示本机 macOS 防火墙拦截非环回自地址入站连接，环境失败；
    与 WP-T03 R2 验收记录的同一环境项，非本包缺陷；本会话隔离复跑一次同样停滞）。
    注意：该全量运行编译于最后两处纯清洁性修改（dead-assignment 移除/文档缩进）之前；
    修改后受影响套件见 03/04 号日志与 05 号门禁，均为最新树。
  - `05-fmt-clippy-contracts-boundaries.log`：fmt --check exit 0、clippy --workspace
    --all-targets -D warnings exit 0、xtask check-contracts exit 0、check-boundaries exit 0。
- 正式二进制层证据：`r05_t08_closed_loop.rs::rr1_f13_normalized_final_survives_restart_on_the_real_binary`
  （真实 lingxi-service 二进制 + 认证 HTTP + SIGTERM 重启；见 04 号日志）。
- 共享文件改动与回归范围、接口演进：见 RR1_ISSUE_MATRIX.json F11/F12/F13 的
  candidateSummary 与 docs/rust-tauri/R05/R05_INTERFACE_EVOLUTION.md 第 5/6/12/13 条。
- 状态：F11/F12/F13 = SELF_CHECKED（待新的独立验收智能体亲跑复核）。
