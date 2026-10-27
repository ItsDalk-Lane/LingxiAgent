# R01-T03 REPORT — 锁定依赖与构建原型

执行者：ZCode:R01-T03-exec-r1（全新独立执行代理，非验收代理）
日期：2026-09-25｜分支 codex/rust-tauri-migration｜基线 HEAD `9705fe0fbaa4c8898dbede0a62a5c97465ca64e3`
状态：**READY_FOR_REVIEW**（判定归独立验收代理）

## 0. 基线留证

`git rev-parse HEAD` = 9705fe0fbaa4c8898dbede0a62a5c97465ca64e3；`git status --porcelain` 开工时仅 `M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`（总控账本，全程未触碰）。环境记录：`artifacts/rust-tauri/R01/T03/baseline-env.txt`（macOS 27.0 arm64 Mac15,14；Node v24.16.0；Homebrew rust 1.93.0；代理变量指向已死的 127.0.0.1:7890，本任务一切联网命令均剥离代理前缀执行，直连可用）。

## 1. 环境变更声明（授权范围内）

- 安装 rustup 1.29.1（官方脚本 https://sh.rustup.rs，profile minimal + 组件 rustfmt/clippy），装 stable=1.98.1 与精确 toolchain 1.98.1-aarch64-apple-darwin。证据：toolchain-pin.log。
- 安装 cargo-audit 0.22.2（开发工具，~/.cargo/bin）。
- 除此之外无系统级变更；Homebrew rust 保留未动（被 rustup 代理绕过，非删除）。

## 2. 锁定结果（逐项版本见 DEPENDENCY_DECISIONS.md 总锁表）

- 工具链：仓库根 `rust-toolchain.toml`，channel = "1.98.1"（rustc 1.98.1 48a229cea / cargo 1.98.1 797e8a9bc），rustfmt+clippy。实测 repo 内 rustup 代理解析生效（toolchain-pin.log）。
- 依赖：tokio 1.53.1 / axum 0.8.9 / reqwest 0.13.5（rustls 0.23.45 + aws-lc-rs 1.18.1 + rustls-platform-verifier 0.7.1）/ rusqlite 0.40.2 bundled（SQLite 3.53.2）/ tracing 0.1.44 / tracing-subscriber 0.3.23 / jsonschema 0.57.0 / rmcp 3.4.1；T02 既有 serde 1.0.229 / serde_json 1.0.151 / schemars 1.2.2 / sha1 0.11.0 / sha2 0.11.0 / base64 0.23.1 不变。`rust/Cargo.lock` 已更新（279 包，含平台条件条目），锁树中 tauri/electron/webview2-com/gtk/webkit2gtk/openssl-sys/native-tls 全部 absent。
- 原型工程：新 crate `rust/crates/lingxi-spike`（6 个 bin：health / sqlite_queue / mcp_handshake / schema_validate / check_deps / tls_probe；lib 含 queue.rs 有界队列与 schema.rs 验证助手），已在 DEPENDENCY_RULES.json 注册为 kind=prototype 并纳入 DEP-07（D5-reverse 契约要求每个 workspace member 注册；该校验器正向通过）。

## 3. 验收场景执行登记

### R01-A05 锁文件可复现

| 项 | 内容 |
|---|---|
| 命令 | ① 全新 CARGO_HOME=/tmp/r01t03-a05/cargo-home 下 `cargo fetch --locked --manifest-path rust/Cargo.toml`（联网、剥离代理）② 全新 CARGO_HOME + 全新 CARGO_TARGET_DIR=/tmp/r01t03-a05/target 下 `cargo build --workspace --locked --offline` |
| 预期 | 构建成功，lockfile 不被修改 |
| 实际 | fetch exit=0；build exit=0；Cargo.lock SHA-256 构建前后一致（值见下） |
| 退出码 | 0 / 0 |
| 证据 | artifacts/rust-tauri/R01/T03/a05-clean-build.log（含版本清单来源 locked-versions.txt） |

lockfile SHA-256（构建前=构建后，完整值）：`480cb513de317235cf11de31af6db26a9b1e937effbcd521a077c7eddd1f3560`。干净构建产物 spike_check_deps 实测运行 OK。构建目标：aarch64-apple-darwin。

### R01-A06 依赖缺失不掩盖

| 项 | 内容 |
|---|---|
| 前置 | /tmp/r01t03-a06/work 隔离副本（rust/ + contracts/generated + rust-toolchain.toml），全新 CARGO_HOME=/tmp/r01t03-a06/cargo-home |
| 操作 | fetch 全部锁定 crate 后删除 rmcp 3.4.1 的 registry 源码目录与 .crate 缓存；运行原型安装检查 `cargo run --locked --offline -p lingxi-spike --bin spike_check_deps` |
| 预期 | 明确报缺失，退出非零 |
| 实际 | `error: failed to download 'rmcp v3.4.1' — attempting to make an HTTP request, but --offline was specified`，exit=101；隔离 CARGO_HOME 下开发机全局 ~/.cargo registry 未被掩盖使用 |
| 对照 | 重新 fetch 恢复后同一命令 SPIKE_CHECK_DEPS_OK，exit=0 |
| 证据 | artifacts/rust-tauri/R01/T03/a06-missing-dependency.log |

此外 check_deps 本体即安装检查实现：枚举二进制动态链接库（macOS otool -L）并逐一验证存在性，缺失即 exit 3 报 `MISSING_DEPENDENCY: <path>`；实测输出证明 bundled SQLite 无系统 libsqlite3 依赖（run-spike_check_deps.log）。

## 4. 原型真实运行记录（全部 exit=0，日志在 artifacts/rust-tauri/R01/T03/run-*.log）

| 原型 | 关键结果 |
|---|---|
| spike_health（axum+tokio+reqwest loopback） | SPIKE_HEALTH_OK /hello=200 /health=200 |
| spike_sqlite_queue（rusqlite bundled 有界队列） | SPIKE_SQLITE_QUEUE_OK sqlite=3.53.2 journal=wal jobs=201 capacity=8 |
| spike_mcp_handshake（官方 rmcp 3.4.1） | SPIKE_MCP_OK 协商协议 2025-11-25，server 身份回传校验通过，list_tools 成功 |
| spike_schema_validate（jsonschema 0.57.0 × T02 生成物） | 正向 valid；负向 protocolMin 篡改被拒 |
| spike_tls_probe（reqwest rustls 平台根证书） | HTTPS 200，证书校验开启（红线：未关闭校验） |
| spike_check_deps（安装检查） | SPIKE_CHECK_DEPS_OK |

cargo test --workspace（锁定 1.98.1）：kernel 7 + protocol 19 + spike 7，全过（cargo-test-workspace.log）。fmt --check 与 clippy --locked --all-targets -D warnings 均干净（cargo-fmt-check.log / cargo-clippy.log）。

## 5. T01/T02 回归（改 rust/ 后重跑，全部留证）

| 门禁 | 结果 | 日志 |
|---|---|---|
| r01_t01_check_ownership.py 正向 | exit=0（含 lingxi-spike 注册后的 D5/D5-reverse/DEP-07） | t01-check-positive.log |
| r01_t01_check_ownership.py --self-test（N1–N15 负向弹药） | exit=0 | t01-check-selftest-n1-n15.log |
| r01_t01_build_ownership.py --check | exit=0（736 features/69 stores 无漂移） | t01-generator-check.log |
| cargo test --workspace --offline | exit=0 | cargo-test-workspace.log |
| r01-t02-check-generated.sh | **exit=1，仅 headSha 预存漂移**（见 §7 问题 1）；协议 56 文件零 diff | t02-check-generated.log |
| r01-t02-roundtrip.sh | exit=0（12 golden Rust→TS→Rust 字节一致 + 负向 N1–N3） | t02-roundtrip.log |
| r01-t02-handshake.sh | exit=0（版本协商 + 不兼容诊断） | t02-handshake.log + handshake/transcript.jsonl |

过程记录：首次 T01 正向运行因新增锁条目的平台条件 crate（如 android_system_properties）未入本地缓存而失败（cargo metadata --offline 拒绝联网）；以 `cargo fetch --locked` 补齐后通过——此现象本身即"锁定后 fetch 再离线"流程的必要性证据，已并入 A05 流程。

**对 T02 源码的触碰披露：**rustfmt 1.98.1（锁定工具链）与 T02 时所用的 1.93.0 对 10 处换行判定不同；clippy 1.98 新增/收紧 4 个 lint。为让 05 §3 标准门禁在锁定工具链下通过，本任务对 lingxi-protocol 做了纯格式化重排 + 4 处行为不变等价改写（map_or→is_some_and ×2、`<= (1<<53)-1`→`< 1<<53`、同体 if 分支合并、另一处 map_or）。行为不变由以下门禁共同证明：cargo test 19 项协议测试、roundtrip 字节一致、check-generated [1/2] 56 生成文件零 diff、handshake 转录一致。

## 6. 安全与许可（基于锁定依赖）

- cargo audit 0.22.2（RustSec advisory-db 1269 条）：**0 漏洞、0 警告**（cargo-audit.log / cargo-audit.json）。
- 许可全树盘点：全部宽松许可；无 GPL/AGPL/SSPL。注意项：r-efi（MIT OR Apache-2.0 OR LGPL-2.1-or-later，可选宽松支）、webpki-root-certs（CDLA-Permissive-2.0 数据许可）（license-inventory.txt）。
- 无高风险项需单独 ADR；TLS 校验全程开启。

## 7. 问题与阻塞（如实）

1. **T02 check-generated 预存漂移（非本任务引入）**：API_COMPAT_MATRIX.json 内嵌 headSha=a98d2487（T02 执行时点 HEAD），基线 HEAD=9705fe0f（T02 提交后账本提交）。实测完整 diff 仅第 6 行 headSha 一处，矩阵内容逐项一致；提取器输入面（desktop/server/preload）本任务零改动。修复 = 重新生成一行 SHA，属 T02 交付物，本任务未代为改写，留待总控/T02 责任人处置。
2. **跨平台 UNVERIFIED**：macOS x64 / Windows x64 / Linux x64 全部标 UNVERIFIED + 负责人（总控指派）+ 最迟验证阶段 R09/R10，见 PLATFORM_BUILD_MATRIX.json。登记平台风险：PR-WIN-01（aws-lc-sys 在 MSVC 或需 NASM）、PR-LNX-01（精简容器 ca-certificates 缺失时平台验证器回退行为未实测）、PR-LNX-02（glibc 下限未测定）。
3. **rmcp HTTP transport 未启用**：本阶段只验证 client+server+async-rw；streamable-HTTP/SSE transport 归 R04/R07 按需开启。
4. **PATH 要求**：锁定仅对经 rustup 代理的调用生效；Homebrew cargo（1.93.0）不读 rust-toolchain.toml。CI/开发者须知已写入 DEPENDENCY_DECISIONS.md D-01 与 rust-toolchain.toml 注释。
5. 未跑全量 npm test（任务说明明确不必；预存 4 审计封印 FAIL 与 round2/3 夹具重写为已知预存问题，未触碰）。

## 8. 交付清单与 SHA-256

见 artifacts/rust-tauri/R01/T03/SHASUMS.txt（生成于证据封存时）。

交付物：rust-toolchain.toml；rust/ workspace 更新（Cargo.toml、Cargo.lock、crates/lingxi-spike/、lingxi-protocol 纯格式化+lint 等价改写）；docs/rust-tauri/R01/{DEPENDENCY_DECISIONS.md, PLATFORM_BUILD_MATRIX.json, DEPENDENCY_RULES.json（注册 lingxi-spike）}；artifacts/rust-tauri/R01/T03/ 证据目录。

## 9. 未授权事项确认

无 commit/push/PR/tag/release；未改任务书目录、.sync-audit、ORCHESTRATOR_PROGRESS.json；desktop/ server/ core/ lib/ shared/ cli/ hub/ plugins/ skills2set/ package*.json 零改动。
