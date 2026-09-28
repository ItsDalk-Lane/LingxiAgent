# R02 最终收口修复 — 根因组 9 / R1：`r00_management_leaves.rs` 有界等待加固

- UTC 时间窗：2026-09-28T17:14Z – 17:20Z
- 唯一修改文件：`rust/crates/lingxi-service/tests/r00_management_leaves.rs`（+66/−26 行，`git diff` 已核对仅此文件）
- 未 commit / 未 push；未改任何断言语义、用例体（test body 逐字未动）、LAN 用例仍要求真实非 loopback IPv4 网卡地址。

## 修改点

1. 新增 `fn request_stalled(addr, method, path, phase, read_bytes) -> !` 统一超时失败出口：
   panic 消息含目标 addr、phase、method/path、已读字节数，及规定原文的环境诊断句
   （"request to {addr} stalled … — if addr is a non-loopback self-address, the macOS
   application firewall / proxy TUN may be blocking inbound connections to this unsigned
   test binary; this is an environment failure surfaced honestly, not a skipped assertion"）。
   超时即 panic（测试 FAIL），不返回假响应、不跳过断言。
2. `request()` 连接段：`tokio::net::TcpStream::connect` 包 `tokio::time::timeout(10s)`；
   连接失败仍走原 `expect("connect to real service")`，超时走 `request_stalled(…, "connect", 0)`。
3. `request()` 交换段：写请求 + 读循环整体包 `tokio::time::timeout(20s)`（读循环原逻辑
   `Ok(0)`/ConnectionReset/BrokenPipe break、其他错误 panic 逐字保留，仅移入 async 块）；
   超时走 `request_stalled(…, "write/read exchange", raw.len())`。
4. `start_server_with_config()` 的 `ready_rx.await`（同文件另一处无界 await，同型加固）：
   包 `tokio::time::timeout(10s)`；`Ok(Err)` 原分支逐字保留；超时 panic 给出同族环境诊断句。
5. 扫描结论（其余 await 核查后不需改）：`TestServer::stop` 已有 10s timeout；spawn 内
   `stop_rx.await` 由有界 `stop()` 终结；ready-Err 分支内 `handle.await` 仅在 run 已结束时可达；
   `bootstrap_with_deps` 为本地初始化库调用，非网络等待家族，保持原样。

## 验证

环境：`PATH="$HOME/.cargo/bin:$PATH"`，`CARGO_TARGET_DIR=/tmp/rust-target-r02-final`。

| # | 命令 | UTC | exit code | 结果摘录 |
|---|------|-----|-----------|----------|
| 1 | `rustfmt --edition 2021 --check`（仅本文件） | 17:16Z | 0 | FMT_OK（先格式化一次后零 diff） |
| 2 | `cargo fmt --manifest-path rust/Cargo.toml -p lingxi-service -- --check` | 17:18:05Z | 0 | 零 diff |
| 3 | `cargo clippy --manifest-path rust/Cargo.toml -p lingxi-service --all-targets --locked --offline -- -D warnings` | 17:18:06Z | 0 | `Finished dev profile … in 2.39s`，零告警 |
| 4 | `perl -e 'alarm 150; exec @ARGV' cargo test --manifest-path rust/Cargo.toml --locked --offline -p lingxi-service --test r00_management_leaves -- --nocapture` | 17:18:12Z | 0（通过） | `test r00_management_positive_and_negative_branches_on_real_service ... ok` / `test result: ok. 1 passed; 0 failed; … finished in 34.49s`；LAN 自连（remote=192.168.3.5:6227x，bad_origin 403 用例）真实通过 |
| 5 | 同上复跑（精确取 exit code + 间歇性第二样本） | 17:19:01Z | 0（通过） | `test result: ok. 1 passed; 0 failed; … finished in 34.76s` |

- 本轮两次实测均为「环境放行」分支：全绿 1 passed（34.49s / 34.76s，远低于 150s alarm）。
  与背景描述的间歇性一致（当日 16:58Z 过、16:20Z/17:1xZ 挂）。本轮未复现拦截窗口，
  故超时 panic 路径未在真实拦截下触发——该路径为纯 `tokio::time::timeout` 包裹 + panic，
  语义上把无限挂起转为 ≤20s 快速 FAIL，无跳过/伪造响应分支。
- 本地结果不代替其他平台/正式打包验证；未执行 commit。
- 工作区其余 `M` 文件（auth_matrix.rs、xtask 等）为其他修复组既有改动，本次未触碰。
