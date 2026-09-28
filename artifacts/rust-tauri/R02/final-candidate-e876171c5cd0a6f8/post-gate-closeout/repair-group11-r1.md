# R02 最终收口 — 修复组 11 R1：WS 外部撤销关闭路径的竞态 RST

- 子代理：`R02-REPAIR-GROUP-11-R1`
- 仓库：`/Users/study_superior/Desktop/Code/LingxiAgent`，HEAD `cdd213078…`（工作区含其他修复组未提交改动，未触碰）
- 范围：仅 `rust/crates/lingxi-service/src/**` 与 `rust/crates/lingxi-service/tests/auth_matrix.rs`。未 commit/push，未动 scripts/、xtask、docs。
- 环境：macOS 27 arm64 loopback；`CARGO_TARGET_DIR=/tmp/rust-target-r02-final`；cargo `--offline --locked`。

## 1. 缺陷与复现

Gate 第 3 轮 step 3 首败（/tmp/r02-final/gate-r3/03-test.log）：
`a05_ws_ticket_and_live_socket_observe_external_revocation` panicked at
`tests/auth_matrix.rs:405` `read server frame: Os { code: 54, kind: ConnectionReset }`。

复现（修复前基线，20 次循环，loopback，命令
`cargo test --manifest-path rust/Cargo.toml --locked --offline -p lingxi-service --test auth_matrix a05_ws_ticket_and_live_socket -- --nocapture`）：

- **pass=18 fail=2（失败率 10%）**，两次失败均为 `auth_matrix.rs:405 ConnectionReset`（run 4、run 16）。
- 日志：`/tmp/r02-final/work/group11/repro-prefix-baseline.txt`、`base-*.log`。
- 取证过程中另有一次同类失败（编辑窗口边界上编译的准修复前代码，run 8，
  `repro-8.log`，23 测试总数、无新测试，确认是旧行为形态）。

结论：行为是竞态/间歇性，与「同轮 verify-stage a05_a06 脚本 PASS」的不确定性一致；
loopback 用例与 LAN 防火墙无关。

## 2. 根因（file:line，均为修复前行号）

服务端撤销检测有两条触发点，但**关闭形态同根**：

- (a) 消息边界被动复查：`rust/crates/lingxi-service/src/lib.rs:2372-2377`
  （`frame_reader.recv()` 分支内 `ensure_ws_principal_current`）；
- (b) 运行中撤销定时复查（T03-R1-F02 语义）：`lib.rs:2247-2248` 的 1s
  `auth_tick` → `lib.rs:2367-2371` 分支调 `ensure_ws_principal_current`。

`ensure_ws_principal_current`（lib.rs:2183-2226）本身形态正确：先写含
`invalid_credential` 的错误文本帧，再写 `close(4401)`。**问题在其后的统一收尾**：
会话循环各分支 `return` 后，`run_ws_session` 末尾（lib.rs:2706-2709）仅
`frame_reader.shutdown().await`（中止读任务）后即丢弃 upgraded IO。两种 RST 形态：

- **A1（close 时入站未读）**：定时复查先于读任务消费客户端 `session_read` 帧触发
  → 写完 close 即 `close(fd)`，接收缓冲仍有未读数据 → 内核发 RST（而非 FIN），
  且可能丢弃发送缓冲中尚未发出的帧；
- **A2（close 后对端仍写）**：服务端已 `close(fd)`，客户端随后写入 → 服务端内核
  回 RST → 客户端 TCP 栈处理 RST 时丢弃其接收队列中已送达、尚未读走的错误帧/close 帧
  → 客户端首次 `read` 即 `ConnectionReset`（即 405 行 panic）。

1s tick 与帧到达同刻时 `tokio::select!` 随机选分支，故间歇性 ~10% 失败。
旁证：hyper 1.11.1 `server/conn/http1.rs:222` —— 101 upgrade 完成后连接 future
立即 `Ready(Ok(()))`，upgraded IO 完全由会话任务独占，丢弃时机即上述 return 之后。

## 3. 修复（统一干净 WS 关闭）

`rust/crates/lingxi-service/src/lib.rs`（+46 行净增）：

- 新增 `drain_ws_after_server_close(frame_reader, shutdown_rx)`（现 lib.rs
  ~2731-2757）+ 常量 `WS_CLOSE_DRAIN_GRACE = 5s`：服务端主动 close 之后不立刻
  丢弃连接，继续消费并丢弃后续入站帧，直到对端回送 close（完成 close 握手）、
  EOF、收到关停广播或 5s 宽限到期。迟到写入被消费 → 内核缓冲不再有未读数据、
  对端也不会写入已关闭的 socket → 不再产生 RST，已写出的错误帧 + close(4401)
  原样可读。
- `run_ws_session` 末尾（现 lib.rs:2713-2715）：非关停退出一律走该排空收尾；
  关停广播分支（两处 shutdown 分支，现 lib.rs:2262/2369 设
  `exited_by_shutdown`）保持原立即返回语义（进程在统一关停预算内退出，不引入
  额外关停延迟）。
- 语义保持：撤销检测时机与错误帧/close(4401) 内容不变（撤销仍在有界时间内
  断开，客户端可见关闭是即时的）；只有最终 TCP 丢弃延后到 close 握手完成或
  ≤5s 宽限，连接槽保持有界。消息边界复查与定时复查两条路径统一收口到同一
  干净关闭。

`rust/crates/lingxi-service/tests/auth_matrix.rs`（+ 新测试）：

- `a05_ws_late_client_write_after_server_close_keeps_frames_intact`：钉住线上
  契约（close 后迟到写入不破坏已送达帧）。**如实说明**：该测试在本机 macOS 上
  修复前也通过（实测 3/3；macOS 对「已排队未读数据 + RST」保留队列数据，
  Linux 等平台则会丢弃），因此它在本机不构成修复前判别器——判别证据是循环
  失败率与下列确定性单测；其注释已如实改写，未夸大。

## 4. 回归测试（确定性部分）

黑盒对端无法控制服务端读调度，故「close 时入站未读」无法从集成层确定性构造；
排空机制本身以内存双工管道在 lib.rs 单元测试中确定性验证（`src/lib.rs`
tests 模块）：

- `ws_drain_consumes_late_frames_and_ends_on_client_close`：迟到 text+ping 被消费
  丢弃、排空不提前结束；对端 close → 立即结束。
- `ws_drain_ends_on_shutdown_broadcast_and_peer_eof`：关停广播立即让出；
  对端 EOF 立即结束。

另加稳定性循环证明（见下）。

## 5. 验证结果（UTC，本机）

| 项 | 命令 | 结果 | exit |
|---|---|---|---|
| 修复前基线 20x | `--test auth_matrix a05_ws_ticket_and_live_socket` 循环 | pass=18 fail=2（均 405 ConnectionReset） | — |
| 最终代码 30x | 同上循环（`a05_ws_` 过滤，含新契约测试，每轮 2 测试） | **pass=30 fail=0** | 0 |
| 全量（crate） | `cargo test -p lingxi-service --offline --locked` | lib 184 过（含 2 新单测）、auth_matrix 24 过（含 1 新测试）、其余 13 目标全绿 | 0 |
| fmt | `cargo fmt --all -- --check`（workspace 根） | 干净 | 0 |
| clippy | `cargo clippy --manifest-path rust/Cargo.toml -p lingxi-service --all-targets --locked --offline -- -D warnings` | 零告警 | 0 |
| workspace | `cargo test --workspace --offline --locked`（2 次） | 除 1 个环境性失败外全绿（见下） | — |

- 30x 循环日志：`/tmp/r02-final/work/group11/loop30-final.txt`（另有一轮功能
  等价代码的 30x：`loop30-postfix.txt`）。
- workspace 两轮中唯一失败均为 `r00_management_positive_and_negative_branches_on_real_service`
  （r00_management_leaves.rs:49）：测试自身诊断明确写明「request to
  **192.168.3.5**（非 loopback 自地址）stalled … macOS application firewall /
  proxy TUN 可能拦截 —— environment failure」。属任务书预告的 LAN 间歇拦截，
  与本组改动无关（该测试无 WS 路径，卡在 `POST /lingxi/v1/web-auth/login`
  HTTP 交换，0 bytes read）；隔离重跑该测试 PASS（35.69s），且本组改动前后的
  `-p lingxi-service` 全量（18:35Z）中它也 PASS。除此之外 24 个
  `test result: ok` 全绿。
- 时间戳：修复前基线 18:1x–18:2x Z；最终 30x 18:2x–18:3x Z；全量
  18:35:40Z–18:36Z；workspace 18:36:59Z、18:39:20Z；fmt 18:4x Z。

## 6. 修改文件

- `rust/crates/lingxi-service/src/lib.rs`：+46 行（排空收尾、关停分支标记、
  2 个确定性单测）。
- `rust/crates/lingxi-service/tests/auth_matrix.rs`：+1 契约测试（+注释如实
  标注本机非判别器）。

中间产物：`/tmp/r02-final/work/group11/`（循环日志、pre/post 固定副本
`lib.rs.fixed`、`auth_matrix.rs.fixed`）。

## 7. 边界与如实声明

- 未 commit/push；未触碰其他修复组的未提交改动（auth_matrix.rs 中
  registry-failure 语义等他组改动原样保留）。
- 关停广播分支保持原「写 close(1001) 后立即丢弃」语义（进程退出预算内），
  该路径在进程退出时理论上仍可能被对端迟到写入 RST 化——不在本缺陷组范围，
  未改、未扩大验证。
- 黑盒确定性复现「close 时入站未读」不可行（服务端读任务调度不可从对端
  控制），判别证据为：修复前 2/20 失败、修复后 60/60（两轮 30x）全绿 +
  排空机制确定性单测。
- 本地结果不代替其他平台（RST 与接收队列交互的平台差异已如实记录于新测试
  注释）或正式打包验证。
