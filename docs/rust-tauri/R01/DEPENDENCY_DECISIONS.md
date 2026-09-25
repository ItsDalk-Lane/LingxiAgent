# R01-T03 DEPENDENCY_DECISIONS — 依赖锁定决策记录

日期 2026-09-25｜执行者 ZCode:R01-T03-exec-r1｜基线 HEAD `9705fe0fbaa4c8898dbede0a62a5c97465ca64e3`（分支 codex/rust-tauri-migration）｜证据目录 `artifacts/rust-tauri/R01/T03/`。

ADR 格式依据任务书 06 §1。本文件每条决策含：问题与用户目标、基线证据、不可违反契约、候选及实际验证、选择与理由、依赖版本、性能/安全/兼容影响、退出条件、受影响任务/测试。

**总锁表（rust/Cargo.lock 解析值，证据 `locked-versions.txt`）：**

| 组件 | 锁定版本 | 备注 |
|---|---|---|
| Rust 工具链 | 1.98.1 (48a229cea 2026-09-01) | rust-toolchain.toml channel="1.98.1"，组件 rustfmt+clippy |
| tokio | 1.53.1 | features: macros/rt-multi-thread/sync/net/io-util/time |
| axum | 0.8.9 | 默认特性 |
| reqwest | 0.13.5 | 默认特性（default-tls/charset/http2/system-proxy）+ json |
| rustls（经 reqwest） | 0.23.45 + aws-lc-rs 1.18.1 + rustls-platform-verifier 0.7.1 | 平台原生根证书 |
| rusqlite | 0.40.2（libsqlite3-sys 0.38.2，bundled SQLite 3.53.2） | features: bundled |
| tracing / tracing-subscriber | 0.1.44 / 0.3.23 | fmt + env-filter |
| schemars（T02 既有） | 1.2.2 | schema 生成侧 |
| jsonschema | 0.57.0 | schema 验证侧 |
| rmcp | 3.4.1 | default-features=false + client,server |
| serde / serde_json（T02 既有） | 1.0.229 / 1.0.151 | derive |
| sha1 / sha2 / base64（T02 既有） | 0.11.0 / 0.11.0 / 0.23.1 | 握手与摘要 |

锁树总计 279 个 crate（含平台条件依赖条目）。锁定机制 = Cargo.lock（精确版本）+ rust-toolchain.toml（精确工具链）；manifest 中保持 semver 兼容界（与 T02 先例一致），不以 `=` 钉死 manifest 造成双向事实源。

---

## D-01 工具链：rustup + rust-toolchain.toml 固定 1.98.1

**问题与用户目标：**核心与桌面用同一固定 stable 工具链（02 §2）；锁定必须可被真实验证而非写在文档里。

**基线证据：**本机 Homebrew rust 1.93.0（`/opt/homebrew/bin/rustc`，不读 rust-toolchain.toml）。已安装 rustup 1.29.1（官方脚本 sh.rustup.rs，2026-09-25，`--default-toolchain stable --profile minimal`，后补 rustfmt/clippy 组件），安装方式与版本记录于 `toolchain-pin.log`。这是本任务授权范围内的唯一系统级变更。

**不可违反契约：**禁止伪造锁定验证；fmt/clippy 语义随工具链版本变化，同一仓库必须只有一套。

**候选及实际验证：**
- Homebrew rust 直用：无法读 rust-toolchain.toml，锁定不可验证——否决。
- rustup 官方脚本：采用。实测 repo 内 `rustc --version` 经 rustup 代理解析为 1.98.1（toolchain-pin.log）。
- 实测发现：T02 代码是在 1.93.0 rustfmt 下格式化的，1.98.1 rustfmt 对 10 处换行判定不同；本任务已按 1.98.1 重排并复跑 T02 全部生成门禁（两棵生成树零 diff，见 t02-check-generated.log [1/2] 与 t02-roundtrip.log）。这是"必须固定工具链"的直接证据。

**选择与理由：**rustup + 根目录 rust-toolchain.toml（channel="1.98.1"）。核心/桌面共用同一文件；桌面 src-tauri 独立 manifest（R09）在同一仓库根之下，自动继承同一锁定。

**依赖版本：**rustc/cargo 1.98.1；rustup 1.29.1。

**性能/安全/兼容影响：**工具链升级改为显式 PR 动作（改 channel + 全量回归）；rustfmt/clippy 版本锁定消除了"谁跑 fmt 谁改代码"的漂移。已知操作要求：PATH 中 `~/.cargo/bin` 必须先于 `/opt/homebrew/bin`，否则 Homebrew cargo 绕过锁定。

**退出条件：**若某平台 1.98.1 出现阻塞性编译器缺陷，升级需同时更新 rust-toolchain.toml、重跑 fmt/clippy/test 全套并在报告记录。

**受影响任务/测试：**R01-T01/T02 全部回归（本任务已重跑，见报告 §5）；05 §3 的三条标准 cargo 命令自此对锁定工具链生效。

## D-02 HTTP 服务：axum 0.8.9 + tokio 1.53.1

**问题与用户目标：**R02 的 Rust 独立服务需要异步运行时与 HTTP 框架候选，须先证明可编译可运行。

**基线证据：**R00 现役服务为 Node（server/）。R01-T02 已用 tokio-free 的原型服务器证明协议语义；HTTP 框架选型留给本任务。

**不可违反契约：**headless workspace 不依赖任何桌面/WebView 栈（DEP-07）；Rust 服务无需 Tauri 即可启动。

**候选及实际验证：**axum（W09）0.8.9 在锁定工具链编译；spike_health 真实运行：axum 服务 127.0.0.1 临时端口 + reqwest 客户端自检查 /hello、/health 均 200（run-spike_health.log）。tower/hyper 裸用为备选（更底层、代码量大），未采用。

**选择与理由：**axum——tokio 生态标准、rmcp 3.4.1 的 HTTP transport 也以 axum 0.8 实现（版本对齐减少重复依赖）。

**依赖版本：**tokio 1.53.1、axum 0.8.9（hyper 1.11.1 传递）。

**性能/安全/兼容影响：**多线程运行时；取消语义经 tokio CancellationToken（R03 将基于此）。无平台特定代码。

**退出条件：**若 R02 服务形态要求 HTTP/2 流式之外的能力不足，可退回 hyper 直用；axum 非协议层依赖，替换不影响 wire 契约。

**受影响任务/测试：**R02（service 组合根）、R05（模型流式）、05 §3 标准命令。

## D-03 HTTP 客户端与 TLS：reqwest 0.13.5（rustls + 平台原生根证书）

**问题与用户目标：**模型 provider/MCP/更新检查需要 HTTP 客户端；TLS 根证书与代理行为跨平台一致可验证。

**基线证据：**开发机代理 127.0.0.1:7890 已死（baseline-env.txt）；任务书红线禁止关闭 TLS 校验解决连通性。

**不可违反契约：**TLS 证书校验永不关闭；system-proxy 行为保留（用户环境代理必须可用）。

**候选及实际验证：**
- reqwest 0.13.5 默认 default-tls（0.13 起默认即 rustls + aws-lc-rs + rustls-platform-verifier，读平台原生根证书）：spike_tls_probe 对 https://index.crates.io/config.json 实测 HTTPS 200、校验开启（run-spike_tls_probe.log）。
- native-tls 候选：会把 Linux 平台绑到系统 OpenSSL 动态库（交叉/打包负担），未采用。
- aws-lc-sys 在本机无 cmake 条件下用 Apple clang 21 构建成功（a05-clean-build.log）；Windows MSVC 是否需 NASM 未验证 → 平台风险 PR-WIN-01（PLATFORM_BUILD_MATRIX.json）。

**选择与理由：**reqwest 默认 rustls 栈：锁树无 openssl-sys（实测 absent），Linux 免 OpenSSL 开发库，根证书行为经 rustls-platform-verifier 与平台一致。

**依赖版本：**reqwest 0.13.5、rustls 0.23.45、rustls-platform-verifier 0.7.1、aws-lc-rs 1.18.1。

**性能/安全/兼容影响：**aws-lc-rs 是 C 构建（增加编译时间与 C 工具链要求）；FIPS 诉求未评估（无此需求记录）。Linux 精简容器缺 ca-certificates 时平台验证器回退行为未实测 → PR-LNX-01。

**退出条件：**若 Windows MSVC 构建被 aws-lc-sys 阻塞且不愿引入 NASM，退回 native-tls（Schannel）并重测本条。

**受影响任务/测试：**R05（模型协议）、R09（更新）、R01-T03 TLS probe 证据。

## D-04 SQLite 访问：rusqlite 0.40.2 bundled + 独立有界工作队列

**问题与用户目标：**同步 SQLite 访问必须隔离在独立有界工作队列后（02 §2 / R01-T03 步骤 2），不得依赖系统 libsqlite3 造成平台差异。

**基线证据：**W14 指名 rusqlite 为候选；R00 现役存储为 Node better-sqlite3（盘点事实，本任务不迁移）。

**不可违反契约：**禁止静默降级——队列满、worker 死、SQL 错都必须显式报错；单写者；WAL 语义（W06/W07）。

**候选及实际验证：**
- rusqlite 0.40.2 bundled：实现 `BoundedQueue`（专用 std::thread 独占 Connection + tokio mpsc 有界通道）。实测：put/get/count 往返（含中文值）、200 任务过 capacity=8 队列零丢失、capacity=1 + 屏障驻留 worker 时 try_submit 显式返回 Full、journal_mode 实测为 wal、SQLite 版本 3.53.2（cargo test 7 项 + run-spike_sqlite_queue.log）。
- sqlx 候选：异步优先、连接池模型与"单写者有界队列"设计不契合（池即多写者），且宏模式增加构建复杂度；未采用。

**选择与理由：**rusqlite 同步语义 + 显式队列正好实现"隔离同步访问"；bundled 消除系统库差异（otool 实测无 libsqlite3 动态依赖，run-spike_check_deps.log）。

**依赖版本：**rusqlite 0.40.2、libsqlite3-sys 0.38.2（bundled SQLite 3.53.2）。

**性能/安全/兼容影响：**bundled 每次全量构建编译 SQLite C 源（约 10-20s，可接受）；SQLite 版本随 crate 固定，四平台一致。备份/WAL checkpoint 语义归 R02/R06（W06/W07）。

**退出条件：**若需 SQLCipher 或扩展加载，重评 feature 组合；队列模式不变。

**受影响任务/测试：**R02（运行/消息库）、R06（记忆/知识）、R01-T07（存储切换）。

## D-05 JSON Schema 工具：schemars 生成 + jsonschema 0.57.0 验证

**问题与用户目标：**T02 已用 schemars 从 Rust 类型生成 schema；本任务评估验证侧候选（W10/W11）。

**基线证据：**contracts/generated 56 文件（T02，零 diff 门禁在控）。

**不可违反契约：**单一协议源在 Rust serde 类型；生成侧不换成 TS 手写。

**候选及实际验证：**jsonschema 0.57.0 用 T02 生成的 ClientHello.schema.json 验证 golden client-hello 通过；篡改 protocolMin 为字符串被拒（"one" is not of type "integer"），负向断言真实触发（run-spike_schema_validate.log + schema.rs 单测 3 项）。

**选择与理由：**schemars（生成）+ jsonschema（验证）分工成立；验证侧用于 R04 第三方工具 schema 边界校验。

**依赖版本：**jsonschema 0.57.0。

**性能/安全/兼容影响：**jsonschema 拉入 icu/regex 等较大依赖树（锁树 279 总量含此）；外部 dialect/$ref 的资源预算限制需 R04 自定义（W11 警告已记录）。

**退出条件：**若验证性能不足，可限制为启动时编译 + 缓存 validator；不替换生成侧。

**受影响任务/测试：**R04 工具 schema 边界、xtask check-contracts（R02）。

## D-06 MCP：官方 rmcp 3.4.1，不自写协议栈

**问题与用户目标：**验证官方 MCP Rust SDK（W12）可编译可用，协议协商真实发生。

**基线证据：**rmcp 3.4.1（crates.io 2026-09 当前版）；已知最新 MCP 修订 2026-07-28 起 initialize 语义变化（SDK 测试证实）。

**不可违反契约：**不自写 MCP 协议栈；版本协商必须真实。

**候选及实际验证：**rmcp 3.4.1（default-features=false + client,server）：进程内 tokio duplex transport 完成 initialize 握手，协商协议 2025-11-25，server 身份（lingxi-spike-mcp）回传校验通过，list_tools 成功，连接有序关闭（run-spike_mcp_handshake.log）。自写 JSON-RPC 栈：违反任务书，未考虑。

**选择与理由：**官方 SDK 覆盖 stdio/child-process/streamable-HTTP transport 与协商；功能集按 feature 最小化引入。

**依赖版本：**rmcp 3.4.1。

**性能/安全/兼容影响：**`server` feature 拉入 schemars/uuid；HTTP transport（server-side-http/streamable-http-client）本阶段未启用，R04/R07 按需开启并重测。MCP 2026-07-28 修订（per-request metadata 取代 initialize）属 R04 协商策略输入。

**退出条件：**若 SDK 在 R04 真实 MCP server 互联中暴露缺陷，按上游 issue 升级/降级锁定版本；仍不自写。

**受影响任务/测试：**R04（工具网关 MCP）、R07（外围接入）。

## D-07 Node/npm 锁文件保留定位

**问题与用户目标：**保留现有 Node/npm 锁文件供前端/测试/受控 worker；明确区分"开发机需要 Node"≠"生产 Agent 依赖 Node"。

**基线证据：**R00 交接 dependency_locks：package-lock.json sha256 e54a16fe…（未改动）。R00 RUNTIME_DEPENDENCIES 盘点现役 Node 生产链。

**不可违反契约：**本任务零改动 package*.json 与生产目录；目标架构中生产 Agent 内核不依赖 Node。

**候选及实际验证：**不适用（保留决策，非选型）。

**选择与理由：**Node 保留角色：(1) 开发/构建机：Vite 前端构建、vitest、TS 生成消费端校验（tests/migration/r01-t02）；(2) 受控 worker 候选运行时（文档解析等，R07 裁决）。生产 Rust 内核/service/CLI 运行不需要 Node——本任务的 5 个原型全部为纯 Rust 二进制即为证据。

**依赖版本：**Node v24.16.0 / npm 11.13.0（开发机现状，非生产依赖）。

**性能/安全/兼容影响：**无变更。

**退出条件：**R11 旧路径退出时重估 Node 是否仅剩开发用途。

**受影响任务/测试：**R02/R07 worker 边界、R11 退出验收。

## D-08 安全与许可检查

**问题与用户目标：**基于锁定依赖做安全与许可检查；高风险项出 ADR。

**基线证据：**rust/Cargo.lock 279 crate。

**不可违反契约：**不得因工具不可得而跳过检查后宣称安全；工具不可得须如实记录并给替代。

**候选及实际验证：**
- cargo audit 0.22.2（本任务安装，~/.cargo/bin，代理剥离后安装成功）：RustSec advisory-db 1269 条，扫描结果 **0 漏洞、0 警告**（cargo-audit.log / cargo-audit.json）。
- 许可盘点（cargo metadata 全树）：全部宽松许可；注意项两条——r-efi 5.3.0/6.0.0 为 MIT OR Apache-2.0 OR LGPL-2.1-or-later（可选 MIT/Apache，无 copyleft 义务）；webpki-root-certs 1.0.9 为 CDLA-Permissive-2.0（数据许可，宽松）。unicode 依赖为 Unicode-3.0。无 GPL/AGPL/SSPL（license-inventory.txt）。

**选择与理由：**cargo audit 可用且通过；无高风险项需要单独 ADR。许可无阻断项。

**依赖版本：**cargo-audit 0.22.2（开发工具，非产品依赖）。

**性能/安全/兼容影响：**建议 CI 定期重跑 audit（advisory DB 随时间变化）——归 xtask/CI（R02+）登记。

**退出条件：**出现新公告时按版本升级锁定并重测。

**受影响任务/测试：**R02 xtask check 编排、R10 终验。

## D-09 R01-T04 原型依赖：libc 提升为直接依赖
**不可违反契约：**原型 crate 不得引入未锁定的第三方版本；不接入生产入口。

**候选及实际验证：**
- lingxi-browser-spike（R01-T04 浏览器宿主原型）需要 `libc::pipe/dup2` 实现 CDP `--remote-debugging-pipe` 的 fd3/fd4 传递。libc 0.2.189 已是锁定传递依赖（cargo tree 取证），本决策仅将其**提升为直接依赖**，版本不变。
- base64 0.23.1、serde 1.0.229、serde_json 1.0.151、sha2 0.11.0 同样全部复用已锁定版本，无新增第三方版本（Cargo.lock diff 仅新增 lingxi-browser-spike 自身条目，可由 git diff 取证）。

**选择与理由：**复用锁定版本满足「无新增依赖版本」约束；libc 为 fd 级进程拉起的最低充分选择（nix 等更厚封装会引入新版本，不允许）。

**依赖版本：**libc 0.2.189（传递→直接，版本不变）。

**性能/安全/兼容影响：**prototype kind，DEP-07 桌面禁令对其生效；不进入生产依赖图。

**退出条件：**R09 正式宿主实现确定后，若 CDP 机制保留则 libc 随正式模块登记，否则随原型下线。

**受影响任务/测试：**R01-T04 spike 本体与校验器（T01 正向/N1-N15、T02 roundtrip/handshake）。
