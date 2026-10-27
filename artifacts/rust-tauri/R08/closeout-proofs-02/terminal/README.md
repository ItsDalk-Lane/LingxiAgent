# D13 终端子系统 A/B 证据（R08 收尾第六轮）

三叶（snapshot / tail / close）的双内核真实正路径证据，替代 earlier recorded。
叶子：

- `R00-T02-LA-CE69063550AA` F-D13-WS_IN-WS-IN-TERMINAL-SNAPSHOT-REQUEST
- `R00-T02-LA-4C35E6AEC6F7` F-D13-WS_IN-WS-IN-TERMINAL-TAIL-REQUEST
- `R00-T02-LA-CFD9F02BC6AA` F-D13-WS_IN-WS-IN-TERMINAL-CLOSE-REQUEST

## 证据构成

- `EVIDENCE.json`：命令、exit code、逐断言→case 映射、双腿 case 结果、证据探针、
  Rust 改动清单、旧任务 recorded 判定面影响、限制说明。
- `r08_t07_ab-terminal-dev-01/out/`、`r08_t07_ab-terminal-dev-02/out/`：两次独立开发的
  双腿演练产物（`ab-compare.json`、`ab-node-0.json`、`ab-rust-0.json`、双方 vitest 日志、
  内核日志）。两次均 `tasks=1 parity=1 differs=0 recorded=0 failedA=0 failedB=0`。
- `r08_t07_ab-terminal-legacy-01/out/`：既有主套件任务 `leaf.ws.terminal` 的聚焦复跑，
  确认新实现下仍判 `recorded`（判定面不变）。
- `terminal-*.log`：drill 控制台输出（含 `AB_RESULT` / `T07 AB OK`）。

## 复现命令

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --manifest-path rust/Cargo.toml -p lingxi-service --locked
LINGXI_R08_AB_TEST_FILTER='leaf.terminal.positive' \
  node scripts/rust-tauri/r08_t07_ab_drill.mjs \
    --bin rust/target/debug/lingxi-service \
    --work /private/tmp/lingxi-r08-verify/r08_t07_ab-terminal-dev-<唯一后缀> \
    --suite-files desktop/src/react/__tests__/r08/r08-t07-terminal-ab.test.ts
# exit 0；AB_RESULT tasks=1 parity=1 differs=0 recorded=0 failedA=0 failedB=0
```

正式门由 Lead 以 `--suite-files <主套件>,desktop/src/react/__tests__/r08/r08-t07-terminal-ab.test.ts`
合并，`A13` 合同按 `leaf.terminal.positive` 的 cases 派生图钉。

## 实现要点（Rust）

- `procsupervisor.rs`：PTY record 增加 `cwd`；新增**只读终端日志** `TerminalLog`
  （有界 ring、按 UTF-8 边界解码、结束 flush U+FFFD），与 `write_stdin` 的交付游标
  完全独立（tail 重复读不消费交付；单测断言 `pty_deliver` 仍拿到输出）；
  新增 `persistent_terminals()` / `persistent_terminal()` / `terminal_tail()` 只读查询。
- `terminals.rs`（新）：三帧的现役形状、会话身份链
  （`session_locator::resolve` → `sessions.get_for` → record owner 过滤）、
  close 走 `supervisor.terminate` → `killed|already_stopped|rejected`
  （`CleanupTimedOut` 如实报 `rejected/termination_unconfirmed`，绝不谎报已停止）。
- `lib.rs`：WS handler 接线；`native_processes` 缺席时显式 `terminal_unavailable`。

## 限制（如实披露）

- 「终端管理器不可用」分支在 A/B 双腿均不可达（两内核都接线管理器）；Rust 由单测
  `terminals::a_wire_without_a_supervisor_is_an_explicit_capability_error` 覆盖，
  旧栈由 `chat.ts` 的 `terminal_unavailable` 分支覆盖（观测字段
  `probes.managerUnavailableReachable=false`）。
- 驱动层级 = WS 绑定层 + 真实工具回合，不是像素级 DOM（与主演习同口径）。
- 旧栈 foreign-terminal 的 tail 拒绝是通用 error frame（无 code），Rust 带
  `terminal_not_found_or_mismatch` code；语义同为显式拒绝，入探针、不在 case 判定面。
