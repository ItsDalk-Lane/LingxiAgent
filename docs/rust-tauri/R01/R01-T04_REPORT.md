# R01-T04 执行报告（浏览器宿主替换原型）

- 任务：R01-T04；验收 R01-A07 / R01-A08（REQUIRED）
- 基线 HEAD：`08b9f075032455be5f57f8fc882bf366b25b4269`；分支 `codex/rust-tauri-migration`
- 平台：macOS arm64 实测；Windows/Linux UNVERIFIED（R09/R10 门禁）
- 日期：2026-09-25

## 1. 交付清单

| 类别 | 路径 |
|------|------|
| Spike 报告 | `docs/rust-tauri/R01/BROWSER_SPIKE_REPORT.md` |
| ADR | `docs/rust-tauri/R01/ADR-002-browser-host.md` |
| 本报告 | `docs/rust-tauri/R01/R01-T04_REPORT.md` |
| 可重放原型（Rust crate） | `rust/crates/lingxi-browser-spike/`（已注册 DEPENDENCY_RULES.json，DEP-07 生效） |
| WKWebView 原型 | `tests/migration/r01-t04/wkwebview_spike.swift` |
| Electron 对照 harness | `tests/migration/r01-t04/electron_harness.cjs` |
| 测试站/代理/窗口枚举辅助 | `tests/migration/r01-t04/{testsite.mjs,proxy.mjs,list_windows.swift}` |
| 一键重放 | `scripts/rust-tauri/r01-t04-replay.sh` |
| 证据 | `artifacts/rust-tauri/R01/T04/`（chromium×4 阶段、wkwebview、electron、wry-api-surface、reverify、host-cost.txt） |
| 决策登记 | DEPENDENCY_DECISIONS.md D-09（libc 传递→直接，版本不变） |

## 2. 逐场景：命令 / 预期 / 实际 / 退出码

统一前置：`env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY`（代理已死）；
`CARGO_TARGET_DIR=/tmp/r01t04/cargo-target`（构建隔离）；浏览器数据目录 `/tmp/r01t04/profiles-*`；
测试站 127.0.0.1:18281、代理 127.0.0.1:18282（仅 loopback 转发）。

### Chromium 主阶段（A1-A14，20 步）
- 命令：`spike_browser run --site http://127.0.0.1:18281 --repo <repo> --evidence artifacts/rust-tauri/R01/T04/chromium --profile-root /tmp/r01t04/profiles --phases main`
- 预期：启动无 TCP 调试口；导航/快照/中文输入/select/点击/提交/滚动/整页截图/上传/下载/弹窗/对话框/登录/挂起/冷重启 全过
- 实际：`chromium/transcript.jsonl` 20×VERIFIED（A10 弹窗首击停滞、重试一次成功，actual 已注明）
- 退出码：**0**

### Chromium 隔离阶段（A08，B1-B6）
- 命令：同上 `--phases isolation --evidence …/chromium-iso`
- 预期：双 BrowserContext localStorage/cookie 互不串；探针页拿不到 hana/process/CDP；dispose 后查询被拒
- 实际：6×VERIFIED（`STORAGE_READ local=ALPHA_A` vs `BRAVO_B` 无交叉；probe 全 BLOCKED/undefined）
- 退出码：**0**

### Chromium 代理阶段（C1/C2）
- 命令：同上 `--phases proxy --proxy-port 18282 --evidence …/chromium-proxy`
- 预期：死代理导航失败；活代理流量经代理（日志为证）
- 实际：C1 `ERR_PROXY_CONNECTION_FAILED`（须 `--proxy-bypass-list=<-loopback>` 关闭隐式绕过）；C2 代理日志 `HTTP GET http://127.0.0.1:18281/form`；修补后代理日志显示全部非 loopback 请求 `REFUSED`
- 退出码：**0**（修补后重跑；事件见 §4）

### Chromium 用户接管阶段（D1-D4）
- 命令：同上 `--phases takeover --evidence …/chromium-takeover`
- 预期：窗口可见；独立 OS 通道（osascript keystroke）键入落页面且 CDP 读回；整屏截图留证
- 实际：4×VERIFIED（CGWindowList onscreen=1 1280x860；`HUMAN-TAKEOVER` 读回；d3-screen.png 1.88MB）
- 退出码：**0**

### WKWebView spike（W0-W16）
- 命令：`swiftc -O -o /tmp/r01t04/wkwebview_spike tests/migration/r01-t04/wkwebview_spike.swift && /tmp/r01t04/wkwebview_spike --site … --repo … --evidence artifacts/rust-tauri/R01/T04/wkwebview`
- 预期：逐项覆盖契约；缺口如实 UNVERIFIED
- 实际：15 VERIFIED / 0 FAILED / 3 UNVERIFIED（W6 上传无 API、W13 代理 loopback 绕过不可负验、W16 未签名 CLI 不可激活）
- 退出码：**0**

### Electron 对照（E0-E16 + P1/P2）
- 命令：`node_modules/.bin/electron tests/migration/r01-t04/electron_harness.cjs -- --site … --repo … --evidence …/electron --user-data /tmp/r01t04/electron-profile [--mode login-write|login-verify]`
- 预期：复刻生产语义逐项过；登录跨进程持久化
- 实际：main 17 VERIFIED / 1 UNVERIFIED（E6 上传=生产自身缺口）/ 0 FAILED；P1/P2 均 VERIFIED
- 退出码：**0 / 0 / 0**

### 校验器复跑（改 rust/ 后强制）
| 校验 | 退出码 | 证据 |
|------|--------|------|
| `r01_t01_check_ownership.py`（正向） | 0 | reverify/t01-check-positive.log |
| 同 `--self-test`（N1-N15） | 0 | reverify/t01-self-test-N1-N15.log |
| `r01_t01_build_ownership.py --check` | 0 | reverify/t01-build-check.log（736 features/69 stores up-to-date） |
| `r01-t02-roundtrip.sh` | 0 | reverify/t02-roundtrip.log |
| `r01-t02-handshake.sh` | 0 | reverify/t02-handshake.log + handshake/transcript.jsonl |
| `r01-t02-check-generated.sh` | 0 | reverify/t02-check-generated.log（624 条无漂移） |
| `cargo test --workspace --offline` | 0 | reverify/cargo-test-workspace.log |
| `cargo test -p lingxi-browser-spike`（fmt 后） | 0（7 passed） | reverify/cargo-test-spike-postfmt.log |
| `cargo fmt --all --check` | 0 | reverify/cargo-fmt.log |
| `cargo clippy --locked --workspace --all-targets --offline -- -D warnings` | 0 | reverify/cargo-clippy.log |

## 3. 红线合规自查

- 未 commit/push/PR/tag/release；未改任务书目录、`.sync-audit`、`ORCHESTRATOR_PROGRESS.json`。
- 生产目录零改动（desktop/ server/ core/ lib/ shared/ cli/ hub/ plugins/ skills2set/ package*.json 均未触碰；对 `desktop/main.cjs` 仅只读提取 SNAPSHOT_SCRIPT）。
- 新增代码隔离：`rust/crates/lingxi-browser-spike`（workspace 成员，prototype kind）、`tests/migration/r01-t04/`、`scripts/rust-tauri/r01-t04-replay.sh`、`docs/rust-tauri/R01/` 三文档；`rust/Cargo.toml` 仅增 members 条目+注释。
- Cargo.lock 零新增第三方版本（base64/libc/serde/serde_json/sha2 全复用锁定版本）。
- 测试目标全部 127.0.0.1；无真实账号/用户数据；浏览器自带后台遥测外联（非 proxy 阶段 GCM 注册）见 §4 末段补充披露（docfix R1）。

## 4. 红线事件披露（必须上报）

活代理验证期间，初版 `proxy.mjs` 只记录不限制目标，被测 Chrome 实例的后台流量经代理真实外发。预修补日志（保留为 `chromium-proxy/proxy-server-prepatch-LEAK-EVIDENCE.log`）计：**1 条明文 HTTP GET `clients2.google.com/time/1/current` + 7 条 CONNECT 隧道**（www.google.com×2、content-autofill.googleapis.com×2、www.gstatic.com、update.googleapis.com、accounts.google.com）。另有一次修补后重启失败（EADDRINUSE，旧进程仍监听），导致 proxy 阶段 run2 实际仍跑在未修补代理上。

处置（全部完成并留证）：
1. 修补 `proxy.mjs`：HTTP 与 CONNECT 双路径拒绝一切非 loopback 目标（403），拒绝也落日志；
2. 按 PID 杀掉旧代理（72895）后重启修补版，`curl` 双验：example.com→403、127.0.0.1→200；
3. 重跑 proxy 阶段（exit=0，C1/C2 VERIFIED），新日志 `chromium-proxy/proxy-server.log` 显示 Chrome 后台对 Google 的全部请求均被 `REFUSED`，loopback 正常放行。

影响评估：CONNECT 为盲隧道（代理不见 TLS 内容），明文 GET 仅是 Chrome 时间校验；无任务数据、无账号、无测试 payload 外发。属测试基础设施事故，已修复、复验并双向留证。

**补充披露（docfix R1，应验收 R1 F2 要求）：非 proxy 阶段的 Chrome 后台外联真实发生。**

上文披露覆盖 proxy 阶段经代理的外发。验收 R1（`R01-T04_REVIEW_R1.md` §8 F2）指出：非 proxy 阶段（main / isolation / takeover）浏览器直连运行、不经 proxy.mjs，Chrome 自带后台网络活动会真实外联，而原证据未留存这些阶段的浏览器外联记录。本修正代理（docfix-r1）以与 `launcher.rs` 默认旗标组完全一致的参数（含 `--disable-background-networking --disable-component-update --disable-sync --metrics-recording-only`，全新 profile、about:blank）独立复跑证实：

- Chrome stderr 出现 `google_apis/gcm/engine/registration_request.cc` 的 `Registration response error message: DEPRECATED_ENDPOINT`（及后续 `QUOTA_EXCEEDED`）——收到服务端响应码即证明完成了一次到 Google GCM 注册服务的真实往返；
- 同一运行 lsof 采样观测到 Chrome Helper 进程持有到外部地址 `:443` 与 `:5228`（GCM/mtalk 常用端口）的已建立 TCP 连接；
- 证据落盘 `artifacts/rust-tauri/R01/T04/docfix-r1-gcm/`（baseline / mitigation / endpoints 三组的 stderr 与 lsof 采样，含 SHA256SUMS.txt）。

性质与区分：与上文已披露事故**同类**——GCM 注册是 Chrome 浏览器自身的后台遥测/保活机制，载荷为 Chrome 实例的设备级注册信息，**不含任何任务数据、测试 payload、账号或凭证**；区别仅在于它发生在不经代理的阶段，因而既不经由也不见于 proxy.mjs 日志。本机实测网络路径经一个活跃的 TUN 代理转发（fake-IP 198.18.0.0/15，utun1024；shell 代理环境变量虽死，TUN 层仍生效），该路径是机器环境特性；无论出口路径如何，外联由被测浏览器进程自发发起、越出 loopback 测试边界、并收到 Google 侧响应，这一事实与路径无关。

缓解措施评估：

- **实测无效（本次验证，留证）**：launcher 已有的 `--disable-background-networking` 等旗标组对 GCM 不完全有效（与验收 R1 观察一致）；追加 `--disable-features=PushMessaging` 后 GCM 注册仍发生（mitigation 组 stderr 同现 DEPRECATED_ENDPOINT×2 与 QUOTA_EXCEEDED）。故不声称任何命令行旗标组合能消除该外联。
- **已验证有效（沿用 proxy 阶段现有证据）**：后续复跑/重放应对**全部阶段**套用修补后的 loopback-only `proxy.mjs`（`--proxy-server=127.0.0.1:18282 --proxy-bypass-list=<-loopback>`）；`chromium-proxy/proxy-server.log` 显示经此代理时 Chrome 对 Google 的全部后台请求（含 CONNECT 隧道）均被 REFUSED，loopback 正常放行。
- **未验证项**：host 级防火墙/DNS 阻断、其他 `--disable-features` 组合未实测，不作有效性声明。

对既有结论的影响：零。全部能力断言针对 loopback 测试站内容，不依赖、也不受该后台外联影响；`BROWSER_SPIKE_REPORT.md` 方法段的「无真实外发」blanket 表述已更正为精确表述。

## 5. 已知限制 / 阻塞

- 无「无候选可用」级阻塞；候选 B 全绿。
- WKWebView W16（未签名 CLI 接管）与 W13（loopback 代理负验）为环境/红线限制，如实 UNVERIFIED，不影响选型（该候选已因 W6+wry 缺口被拒）。
- Windows/Linux 全部 UNVERIFIED → R09/R10 强制门禁，不得视为已覆盖。
- 预存失败未追修：全量 npm test 审计封印 4 项 FAIL（任务书明示不追修，本任务未跑全量 npm test）。
