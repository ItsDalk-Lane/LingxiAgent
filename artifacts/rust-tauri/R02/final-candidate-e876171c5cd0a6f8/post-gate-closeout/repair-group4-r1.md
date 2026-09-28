# R02 最终收口修复 — 根因组 4（R02-REPAIR-GROUP-4-R1）

- 日期（UTC）：2026-09-28
- 仓库：`/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`，HEAD `cdd213078`
- 范围：仅 `scripts/rust-tauri/r02_t08_full_chain_probe.py` 与 `scripts/rust-tauri/r02_t08_full_chain_smoke.sh`（未 commit/push，未改服务端，未改断言语义）
- 环境：`PATH="$HOME/.cargo/bin:$PATH"`，`CARGO_TARGET_DIR=/tmp/rust-target-r02-final`，offline

## 修改点

### 1. probe `ws_key()` 违反 RFC6455（transport 拒绝 400）
- 根因：`ws_key()` 返回 `os.urandom(16).hex()`（32 个 hex 字符）。服务端
  `rust/crates/lingxi-service/src/ws.rs`（只读核对，未改）校验 Sec-WebSocket-Key：
  唯一 header 值、STANDARD base64 可解码、解码后字节数 == 16。32 个 hex 字符虽是
  合法 base64 字母表但解码为 24 字节 → `400 transport.invalid_ws_upgrade`
  （`reason=missing or invalid Sec-WebSocket-Key`），探针 WS 握手从未到达 101。
- 修复：`base64.b64encode(os.urandom(16)).decode()` + 顶层 `import base64`
  （`scripts/rust-tauri/r02_t08_full_chain_probe.py`）。
- 一致性核对（只读 `ws.rs`/`transport.rs` 相关代码，未改）：
  - 服务端要求 GET、`Connection` 含 token `upgrade`（大小写不敏感）、
    `Upgrade: websocket`、`Sec-WebSocket-Version: 13`——探针 upgrade() 全部满足；
  - 客户端帧必须掩码（`read_client_ws_frame` 拒绝 unmasked）——探针 `send_frame`
    FIN+opcode、随机 4 字节掩码、长度 126/127 分档，符合；
  - 服务端帧不掩码（RFC6455 服务端规则）——探针 `recv_frame` 按 `header[1] & 0x7F`
    取长度、不剥离掩码位，符合。无需其他改动。

### 2. smoke 脚本 probe 调用点死代码（诊断不可达）
- 根因：`set -euo pipefail` 下 `python3 probe ...`（phase 1，原 202-205 行）与
  `printf | python3 probe`（phase 2，原 224-228 行）非零退出即中止脚本，其后
  `{ cat pass-lines; cat err; } | tee -a summary` 与 `[ -s err ] → fail` 成为
  不可达死代码；失败轮 summary 停在 boot1-ready、命令 stderr 为空。
- 修复（两处同型，保持语义：失败仍 fail 非零）：
  `if ! <probe 调用>; then { cat pass-lines; cat err; } | tee -a summary; cat err >&2; fail "probe phase N failed"; fi`
  之后保留原正常路径（并入 summary + 退出 0 但 stderr 非空的 `[ -s err ] → fail`
  检查，语义不变）。
- 全脚本扫描：`python3 ... r02_t08_full_chain_probe.py` 调用点仅此两处（全 scripts/
  树交叉核对无第三处）。第 199 行 `python3 -c` 为 token 提取非 probe 调用，其
  `[ -n "$OLD_TOKEN" ] || fail` 可达（空 token 路径），不改。

## 验证（真实运行）

### 1. 语法检查
```
python3 -m py_compile scripts/rust-tauri/r02_t08_full_chain_probe.py   → PY_COMPILE_OK（exit 0）
bash -n scripts/rust-tauri/r02_t08_full_chain_smoke.sh                 → BASH_N_OK（exit 0）
2026-09-28T15:32:39Z
```

### 2. 完整真实跑（ALL GREEN）
- 命令：`bash scripts/rust-tauri/r02_t08_full_chain_smoke.sh /tmp/r02-final/g4-verify/A15`
- 开始 2026-09-28T15:32:47Z（本地 23:32:47+0800），EXIT=0，命令 stderr 为空。
- summary 关键行摘录（`/tmp/r02-final/g4-verify/A15/full-chain/summary.txt`）：
  ```
  PASS build
  PASS boot1-ready addr=127.0.0.1:54711 home=/var/folders/.../lingxi-r02t08-a15-home.sT1k7q
  PASS health-200-minimal
  PASS unauthenticated-me-401
  PASS loopback-token-me-200
  PASS execute-write-1-committed (runId=run_000001a0e8a5b494_000001)
  PASS ws-subscribed-with-snapshot-boundary (snapshotSeq=2)
  PASS execute-write-2-committed (runId=run_000001a0e8a5b497_000002)
  PASS ws-live-event-after-write (seq=3 eventId=run_000001a0e8a5b4…)
  PASS http-events-page-contiguous (head=4 count=4)
  PASS ws-future-cursor-explicit-reject (invalid_message/future_cursor close)
  PASS phase-boot-write-subscribe-complete (head=4)
  PASS phase1 (health/auth-negative/auth/write/subscribe/live-event/read-your-writes/future-cursor)
  PASS close1-exit-0 (service exit code 0)
  PASS close1-no-leftover-processes
  PASS close1-port-closed (health probe on 127.0.0.1:54711 refused)
  PASS boot2-ready addr=127.0.0.1:54722
  PASS pre-restart-token-rejected-401
  PASS post-restart-new-token-me-200
  PASS post-restart-session-readback (runCount=2)
  PASS post-restart-events-preserved (head=4 count=4)
  PASS post-restart-health-200
  PASS phase2 (old-token-rejected/new-token auth / session readback / events preserved / health)
  PASS close2-exit-0 (service exit code 0)
  PASS close2-no-leftover-processes
  PASS close2-port-closed (health probe on 127.0.0.1:54722 refused)
  PASS instance-record-removed (single-writer record cleaned on stop)
  == R02-T08 / R02-A15 full chain: ALL GREEN ==
  ```
- probe1.err / probe2.err 均为 0 字节（探测零 stderr 通过）。
- 服务真实二进制：`/tmp/rust-target-r02-final/debug/lingxi-service`，toolchain 1.98.1，`--locked` offline 构建。

### 3. 负向测试（临时副本，/tmp，不入仓库）
方法：smoke 脚本副本（仅改 `cd` 行与 probe 路径指向 /tmp 下的假探针）。
- phase 1 注入失败（`/tmp/r02-final/g4-verify/neg/smoke_neg_phase1.sh`）：
  exit 1；summary 出现 `PASS fake-pass-line-before-injected-failure` 与
  `FAIL injected failure for negative test (phase1)`（诊断成功并入 summary）；
  stderr 为该 FAIL 行 + `FAIL: probe phase 1 failed`。清理 trap 正常收尾。
- phase 2 注入失败（`.../smoke_neg_phase2.sh`，boot-and-write 假通过、readback
  经 stdin 管道失败）：exit 1；summary 含 fake PASS 行与 phase2 FAIL 行；stderr
  为 FAIL 行 + `FAIL: probe phase 2 failed`；boot2 服务被清理。
- 两轮负向后核查：`pgrep -fl "lingxi-service --home"` 无匹配，合成 home 目录
  全部被 cleanup 移除（无遗留进程/端口/数据根）。

## 结论
组 4 两个缺陷均修复并通过真实运行验证：A15 全链 summary ALL GREEN（exit 0）；
失败路径诊断不再丢失（并入 summary + stderr + 非零退出）。改动仅限任务指定的
两个文件（`git diff --stat`：probe +6/-1 行、smoke +21/-4 行）。
