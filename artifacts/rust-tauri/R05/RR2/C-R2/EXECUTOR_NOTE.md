# RR2 WP-C（F34）实施轮证据 — C-R2

执行者：RR2 工作包 C 实施智能体（全新上下文，2026-10-06）。
候选基线：`codex/rust-tauri-migration` HEAD `ad5ec4e9853a51ed929f1e2e077b97d41c951572`（RR2 总控核实的候选）。
工具链：`/Users/study_superior/.cargo/bin/cargo`（rustup 代理→1.98.1，`rust/rust-toolchain.toml` 锁定）。

## 改动（主树，全部在白名单内）

1. `rust/crates/lingxi-adapters/tests/r05_t04_rr1_batch_terminal.rs`：新增永久测试腿
   `known_stop_reason_with_unclosed_tool_block_is_loud_and_dispatches_nothing`（F34，RR2 WP-C）。
   夹具＝message_start → content_block_start(tool_use id=t1/name=read) → input_json_delta
   （完整合法参数 `{"path":"a.txt"}`）→ message_delta{stop_reason:"tool_use"} → message_stop，
   **只缺 content_block_stop**。断言：finish() Err 且 code==InvalidMessage、不可重试、
   错误点名具体未闭块（`content block(s) [0]` + `content_block_stop`）；经该文件正式断言链
   （match finish() 结果）Ok(ToolRequests) 即 panic——零工具执行。内联阳性对照：同一流仅补
   content_block_stop → Ok(ToolRequests)，恰一请求、provider_call_id=Some("t1")、
   target="tool:first-party:read"、arguments=={"path":"a.txt"}（完整产出）、content 为空。
   生产代码零改动（`anthropic_messages.rs` 未触碰）。
2. `docs/rust-tauri/R05/r05_stage_pins.tsv`：`pin adp:r05_t04_rr1_batch_terminal` 9→10。
3. `docs/rust-tauri/R05/r05_stage_cids.tsv`：新腿名按字母序并入
   `cid R05-T04-C11 adp:r05_t04_rr1_batch_terminal`（归属既有 C-ID，无新 CID；F34 的
   origRefs 即 C09/C11，本腿与既有 unclosed 腿同属 C11 防线语义）。
4. `rust/crates/xtask/src/stage_map.rs`：镜像测试
   `r05_stage_pin_table_matches_the_registered_suites` 的期望计数 9→10（与新腿一致，
   镜像-登记双向锁定）。
   说明：`R05_TEST_MAP.json` 不逐名登记 RR1 电池（registryNote 指向 r05_required_cids.tsv /
   r05_stage_cids.tsv），无需改动；`r05_required_cids.tsv` 的 103 C-ID 集合不变。

## 自检（真实执行；命令以仓库根为 cwd）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --test r05_t04_rr1_batch_terminal` | 0 | 10 passed / 0 failed（既有 9 + 新 F34 腿），green-battery-f34.log |
| 同上（xtask 镜像 pin 表）`... -p xtask --bin xtask -- stage_map::map_tests::r05_stage_pin_table_matches_the_registered_suites --exact` | 0 | 1 passed，green-xtask-mirror-pin.log |
| 同上（xtask 镜像 cid 表）`... -- stage_map::map_tests::r05_stage_cid_table_owns_every_pinned_test_exactly_once --exact` | 0 | 1 passed，green-xtask-mirror-cid.log |
| `/Users/study_superior/.cargo/bin/cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 | green-fmt-check-f34.log（空输出=零差异） |
| `/Users/study_superior/.cargo/bin/cargo clippy --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --tests -- -D warnings` | 0 | green-clippy-f34.log |
| `/Users/study_superior/.cargo/bin/cargo clippy --manifest-path rust/Cargo.toml --locked -p xtask -- -D warnings` | 0 | green-clippy-xtask-f34.log（stage_map.rs 改动无新告警） |

## 变异验证（隔离副本，主树零接触）

- 隔离副本：`/tmp/r05-rr2-c-mut/candidate`（rsync 全仓拷贝，排除 target/node_modules/.git/.mimosa；
  关键文件 diff -q 与被审树逐字节一致后才动手）。
- 基线（变异前）：同套件命令 → exit 0，10/10 绿（mut-baseline.log）。
- 变异：python 脚本把 `anthropic_messages.rs` finish() 的唯一 unclosed 门
  `if !unclosed.is_empty() {` 精确替换为 `if !unclosed.is_empty() && false { // MUT-F34...`
  （只中性化 unclosed 防线；diff 确认单处差异）。
- 红：同套件命令 → exit 101，**恰好新腿红**：`known_stop_reason_..._dispatches_nothing`
  FAILED（panic 于测试文件 :195 正式断言点——被放行为 `ToolRequests { provider_call_id:
  Some("t1"), arguments: {"path":"a.txt"} }`）；其余 9 腿全绿——**含
  `anthropic_open_tool_block_cannot_close_as_a_completed_batch`（复现 F34 事实：旧腿被
  缺-stop_reason 防线吸收）**（mut-red-unclosed-neutralized.log）。
- 还原：`cp` 主树原文件回副本（diff -q 逐字节一致）→ 同命令 exit 0，10/10 绿
  （mut-restored-green.log）。
- 主树生产文件全程未改：`git diff` 仅测试文件+登记三处；副本已留 /tmp 不入主树。

## 结论

F34 缺口（已知 stop_reason+未闭块组合无永久腿）已以最小永久腿关闭：删除/短路 unclosed
防线必被新腿点名红，登记/镜像双向锁定（删登记或漏计数均红），还原后全绿。
