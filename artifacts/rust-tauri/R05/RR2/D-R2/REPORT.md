# RR2 WP-D（F43）执行报告 — 2026-10-06

工具链：`/Users/study_superior/.cargo/bin/cargo`（rustup 1.98.1）。证据目录：本目录。

## (a) r00_management_leaves 非回环访问失败 — 环境根因复证（不改测试文件）

复跑与差分探针全记录见 `a-rootcause-conclusion.md` 与 `probe/probe-suite.log`、`rerun-current-binary.log`。

| 命令 | 结果 |
|---|---|
| `cargo test -p lingxi-service --test r00_management_leaves -- --nocapture`（当前树二进制 294787929158efb0，无重编译） | exit 1；逐字复现 RR1 签名（192.168.3.5 stalled during write/read exchange, 0 bytes read），51.78s |
| 差分探针 clang adhoc 二进制 × {127.0.0.1 / 192.168.3.5 / 192.168.3.10 / fe80 v6} | 回环 ok；三个非回环全部 stalled（connect+write 成功、accept 永不触发） |
| 同二进制 `codesign --force --sign -` 后 | 仍 stalled（adhoc 重签无效） |
| Apple 签名 `/usr/bin/python3` 同地址对照 | ok（同机同刻） |
| ALF 判定表（只读） | 63 条；`294787929158efb0` 路径有历史 Allow 但重链接后仍被拦 → 判定按(路径,cdhash)，不跨 relink 存活 |

结论：**ALF 按二进制实例拦截未授权应用的非回环入站流**（环境项 R05-ENV-ALF-UNSIGNED-TEST-BINARY，本轮完整复证）。正当测试基建（adhoc 签名/换地址族/复用带判定路径）均无效（已逐项实证）；非回环语义不可降级。**处置：不修改 r00_management_leaves.rs**；解除需用户动作（对当前二进制点 ALF Allow / `sudo socketfilterfw --add` / 关防火墙 / Developer ID 签名）。

## (b) r04_a09 pid 文件 TOCTOU — 等完整可解析内容

改动：`tests/r04_t05_process_tools.rs` — `read_pid_file`（存在性+过滤解析）替换为 `complete_pid_file`（非空、逐 token 可解析、数量齐 3 才返回），屏障由 `pid_file.exists()` 改为轮询读完整内容（20s 有界）；`assert_eq!(pids.len(), 3)` 与后续子孙回收/sentinel 存活/审计断言逐字保留。

| 命令 | 结果 |
|---|---|
| 隔离正例 `cargo test -p lingxi-service --test r04_t05_process_tools r04_a09_managed_tree… -- --nocapture` | exit 0（1 passed） |
| 负例（临时改 printf 只发布 2 个 pid）| exit 101：`timed out waiting for complete tree pid file (3 parseable pids)`（20.13s）——屏障拒绝不完整内容，不空转放行 |
| 还原后全套件 `cargo test -p lingxi-service --test r04_t05_process_tools` | exit 0（14 passed） |

## (c) f24_sigterm worker 就绪 — 真实就绪屏障 + 真实 purpose + 有界 0/6 契约

真实根因（非负载）：原测试把运行期 nonce 当 callback purpose（`workers_section("ask_model", &tag, &tag)`）。宿主 `AuxiliarySlot::from_purpose`（lingxi-kernel/model_exchange.rs）只映射六个固定插槽名，nonce purpose 被防御性拒绝（`model_purpose_not_granted`）→ 工具瞬时失败 32 次 → run `failed.turn_budget_exceeded`，worker 从未停驻。RR1 的"30s 就绪期限"表象即此风暴；历史绿为巧合 pgrep 命中风暴中 5ms 寿命进程的**虚假通过**（未真正测过 SIGTERM 清理）。证据：`isolated-c-green.log`（32×dispatch、turn_budget_exceeded、`model_purpose_not_granted`）与残留 runs.db 查询。

改动：
1. `src/bin/r04_t07_fixture.rs`（测试支撑件，非生产入口）：新增 `ask_model_tagged` 模式——purpose 固定真实插槽 "summarize"，唯一 tag 走 argv/prompt。回归范围：fixture 消费方 6 个测试文件（additive 新模式，既有模式零改动；workspace 全测覆盖）。
2. `tests/r05_t08_production_tools.rs`：
   - 就绪屏障：等待 stub 记录到 `summarize-model-f24` 回调请求 **且** pgrep 见 worker 进程（90s 诚实上界，非固定生成期限）；超时诊断带 stub hits/进程数/服务 stderr。
   - 断言语义更新为生产真实契约：SIGTERM 后进程有界退出（exit 0 或 shutdown.rs 文档化的 code 6 transport-drain 溢出——前台 run 停驻在永不应答的回调上，drain 必然耗尽预算），code 6 必须钉住 `transport_drain_timed_out=true` 标记；**worker 子进程不得在任一路径下存活**（15s 有界回收断言，原语义保留并真正生效）。无盲 sleep、未放宽超时上限（8000ms 预算、30s stop、15s reap 全未动）。

| 命令 | 结果 |
|---|---|
| 隔离正例（屏障+tagged 模式） | exit 0（1 passed，9.38s：真实停驻→SIGTERM→有界退出→回收） |
| 中间证：只修屏障不改 purpose | exit 101：屏障后 SIGTERM 退出码 6（transport drain 耗尽 8000ms）——证明停驻真实发生且暴露原断言 `exit.success()` 与生产契约不符 |
| 负例（临时还原 nonce purpose） | exit 101：`the worker child never reached its parked callback wait … stub hits: 32, worker processes: 0`（90.86s 有界） |
| 还原后全套件 `cargo test -p lingxi-service --test r05_t08_production_tools` | exit 0（6 passed） |
| 孤儿核查 `pgrep -fl r04_t07_fixture` | 空（code-6 路径亦无孤儿） |

## 门禁/工作区

| 命令 | 结果 |
|---|---|
| `cargo build --manifest-path rust/Cargo.toml --locked --workspace --all-targets` | exit 0（6m26s，`prebuild-workspace.log`） |
| `cargo clippy --manifest-path rust/Cargo.toml --locked --workspace --all-targets -- -D warnings` | exit 0（`clippy-selfcheck.log`） |
| `cargo test --manifest-path rust/Cargo.toml --locked --workspace` | 见 `workspace-test-full.log`（真实记录） |

## E 工作包交叉项（OAuth 404）

`r00_management_leaves.rs` 无 oauth/models 引用（grep 核对）；本轮未改其任何断言；其失败集合保持环境性单一 stall（与 E 登记一致）。
