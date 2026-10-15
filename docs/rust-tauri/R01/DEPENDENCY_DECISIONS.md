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
| ctor / dtor（R06-T06 新增，测试守卫） | 0.8.0 / 0.3.0 | 预 main 初始化钩子；ctor-proc-macro 0.0.7 + dtor-proc-macro 0.0.6 传递；仅经 dev-dependencies 的 test-guard feature 边进入测试构建（详见 D-12） |

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

## D-10 R01-T05 原型依赖：零新增（spike_pdf 复用 lingxi-browser-spike 依赖面）
**不可违反契约：**原型 crate 不得引入未锁定的第三方版本；不接入生产入口；候选后端必须逐项实测而非以「支持 PDF 的 Rust 库」充当等价证据。

**候选及实际验证：**
- 候选后端 = 受控 Chromium（Chrome 153.0.8010.52 本机实测）经 CDP `Page.printToPDF`，复用 R01-T04 已验证的 pipe 传输与 launcher；spike_pdf 以 bin target 加入既有 lingxi-browser-spike crate，**无新 crate、无新第三方依赖、Cargo.lock 零变化**（可 `git diff rust/Cargo.lock` 取证）。
- 纯 Rust HTML→PDF 库（printpdf/wkhtml 系/headless_chrome 包装等）未进入实测：任务书明确禁止以库能力声明充当等价证据，且它们无法满足 @page/preferCSSPageSize/Chromium 打印管线逐语义对齐；受控 Chromium 是与现役 Electron printToPDF 同引擎族的唯一候选，直接实测。
- 字体验证/文本提取/位图渲染工具链使用 macOS 系统框架（PDFKit/Quartz/CryptoKit，swiftc 编译），不进入 Rust 依赖图。

**选择与理由：**同引擎族（Chromium printToPDF）是唯一能逐项对齐现役 HTML 打印语义的候选；复用 T04 依赖面满足零新增约束。

**依赖版本：**无变化（base64 0.23.1 / libc 0.2.189 / serde 1.0.229 / serde_json 1.0.151 / sha2 0.11.0 沿用）。

**性能/安全/兼容影响：**prototype kind；DEP-07 生效；渲染宿主为外部 Chrome 二进制（非 cargo 依赖），分发成本与许可评估见 ADR-003。

**退出条件：**R09 正式宿主定型后 spike_pdf 随原型下线或被正式 renderer 模块吸收。

**受影响任务/测试：**R01-T05 spike 本体与校验器（T01 正向/N1-N15、T02 roundtrip/handshake/check-generated、cargo test --workspace）。

## D-11 R01-T06 桌面壳原型依赖：Tauri 2.11.6 + 官方插件组（独立于 rust/ workspace 的 spike 锁树）

**问题与用户目标：**桌面壳候选（W01/W02/W13）必须真实选定并锁定版本；原型要覆盖窗口/托盘/快捷键/通知/剪贴板/对话框/自启动/更新器/sidecar/ACL 负向/E2E 测试路径，且不得污染 headless rust/ workspace（DEP-07）。

**基线证据：**R01-T03 锁表仅覆盖 headless 栈（279 crates）；桌面栈此前零锁定。现役生产壳为 Electron 42（desktop/main.cjs），本任务零改动生产目录。

**不可违反契约：**Tauri 原型放独立 manifest 目录、不进 rust/ workspace；不接入生产入口；测试插件（wdio-webdriver）必须 feature 门控、发布构建可证明缺席（A12）；updater 测试私钥不入库。

**候选及实际验证：**
- Tauri v2 当前稳定线 2.x：采用并实测。锁树 603 crates（spike/tauri-shell/app/src-tauri/Cargo.lock）。W03 测试路径实测：tauri-driver 仅 Windows/Linux（macOS 无 WKWebView 驱动，不可用，如实记录）；macOS 路径 = tauri-plugin-wdio-webdriver 1.4.0 嵌入式 WebDriver（optional 依赖 + `e2e-test` feature，release 构建二进制 strings 探测 wdio/webdriver 计数为 0，端口 ECONNREFUSED）。
- Electron 保留：现役生产壳不动（TB-03），本决策只覆盖迁移目标候选。
- 选 Tauri 而非继续 Electron 的架构理由归任务书 02（壳瘦身、Rust 单一技术栈、capabilities ACL 默认拒绝模型）；本任务以 A11 负向实测确认 ACL 模型真实生效（未授权 webview 11/11 拒绝，远程 URL 零权限）。

**选择与理由：**Tauri 2.11.6 + 下列官方插件全锁（见下表）。插件全部来自 tauri-apps 官方 org、semver 界写在 manifest、精确版本锁在 spike 自有 Cargo.lock（与 T02/T03 先例一致的双层锁定）。

| 依赖 | 锁定版本 | 用途 |
|---|---|---|
| tauri / tauri-build | 2.11.6 / 2.6.3 | 壳核心（features: image-png, tray-icon） |
| wry / tao（传递） | 0.55.1 / 0.35.3 | WebView/窗口运行时（macOS=WKWebView） |
| tauri-plugin-shell | 2.3.6 | sidecar spawn/事件泵（**仅 Rust 侧，capabilities 零 shell 权限**） |
| tauri-plugin-notification | 2.4.0 | 系统通知 |
| tauri-plugin-dialog | 2.7.3 | 文件对话框（实测：用 callback API，blocking 变体有外部取消不返回缺陷，见 TAURI_SPIKE_REPORT §7-R1） |
| tauri-plugin-clipboard-manager | 2.3.3 | 剪贴板 |
| tauri-plugin-global-shortcut | 2.3.2 | 全局快捷键 |
| tauri-plugin-autostart | 2.5.1 | 登录自启动 |
| tauri-plugin-updater | 2.12.0 | 更新（minisign 验签实测正负向） |
| tauri-plugin-process | 2.3.1 | 进程退出/重启 |
| tauri-plugin-wdio-webdriver | 1.4.0 | E2E WebDriver（**optional，feature e2e-test 门控**） |
| serde / serde_json / tokio | 1.0.229 / 1.0.151 / 1.53.1 | 与 rust/ workspace 锁表同版本对齐 |

**性能/安全/兼容影响：**prototype 定位；DEP-07 对 rust/ workspace 的桌面禁令不受本锁树影响（spike 为独立 manifest——无显式 `[workspace]` 段、无祖先 workspace 可归入，T01 校验器 D5-reverse 不受影响——spike 不是 workspace member，也未在 DEPENDENCY_RULES.json module_registry 登记，登记会触发"exists 但缺席 workspace"误报；隔离决策在此显式记录）。603 crates 的许可/安全扫描（cargo audit 等）未跑——spike 锁树不是生产依赖图，正式扫描归 R09 tauri-host 建立时对其自有锁树执行。

**退出条件：**R09 tauri-host（desktop/src-tauri，planned）建立时以本锁表为起点重新评估最新 2.x；若官方插件在 R09 真实业务面暴露缺陷，按上游 issue 升/降锁并复测本任务的能力矩阵。

**受影响任务/测试：**R09（宿主集成）、R10（跨平台产物）；本任务证据 artifacts/rust-tauri/R01/T06/ 与 docs/rust-tauri/R01/{SHELL_CAPABILITY_MATRIX.json,TAURI_SPIKE_REPORT.md,R01-T06_REPORT.md}。

## D-12 R06-T06 测试守卫：新增 lingxi-test-guard crate（唯一新第三方 crate = ctor 0.8.0）

**问题与用户目标：**R06-T06 事故——`r06_t06_memory` 测试进程因 `urldecode` 无终止循环在 ~61 分钟里把常驻内存涨到 ~176GB，耗尽整机 96GB 内存被看门狗重启。测试面缺进程级内存/时间上限。目标：默认随 `cargo test` 对所有测试生效的守卫，且生产/发布构建零开销、零全局副作用。

**不可违反契约：**第三方 crate 只能经 rust/ workspace 引入并锁定（锁表纪律）；headless workspace 无桌面依赖（DEP-07）；守卫只在测试构建启用；不使用来路不明依赖；非必要不用 unsafe（必要处逐处注明 sound 理由）。

**候选及实际验证：**
- 自定义 test harness（`harness = false`）：需改写 115+ 个测试文件，侵入过大——否决。
- nextest setup/脚本：无法在测试进程内安装监控（且仓库当前未用 nextest）——否决。
- `ctor` 预 main 钩子（采用）：`crates/lingxi-test-guard` 以 `#[ctor::ctor]` 在 main 前安装采样线程；经 `test-guard` feature 边由 dev-dependencies 开启，`use lingxi_test_guard as _;` 置于 kernel（feature 门控）与各无 kernel 边的 crate 的 `#[cfg(test)]` 项，使所有链接被测 crate 的测试二进制自动覆盖。实测（本机 macOS）：服务/适配器/内核集成与单元测试二进制均装载（nm 计数 + `TEST_GUARD_VERBOSE=1` armed 行），synthetic 泄漏用例在阈值处 abort（exit 134=SIGABRT，报告含测试线程名）。
- macOS RSS 采样经 `libc::task_info(MACH_TASK_BASIC_INFO).resident_size`；Linux 经 `/proc/self/statm`。libc 0.2.189 已是锁定直接依赖（D-09），版本不变。
- ctor 0.8.0（mmastrac，成熟维护）：`Cargo.lock` 新增 ctor/ctor-proc-macro/dtor/dtor-proc-macro 四个条目 + lingxi-test-guard workspace member；无其它新增；`cargo build`/release 图不含该边（feature 只由 dev-dependencies 开启）。
- unsafe 使用面：仅 guard crate 内 task_info/task_threads/pthread_getname_np/内联 `libc::write` 四处，均逐处注明 SAFETY/原因（见 crate 源码）；`#[allow(deprecated)]` 两处沿用 libc 绑定而非引入 mach2 新 crate。

**选择与理由：**ctor + feature 边是"默认随 cargo test 生效、零测试文件改动、release 零开销"三者同时满足的最小侵入方案；守卫自身有 11 个单元测试（阈值解析、越限判定、RSS 采样、线程名枚举、默认安装证明）。

**依赖版本：**新增 ctor 0.8.0（+ 传递 dtor 0.3.0、ctor-proc-macro 0.0.7、dtor-proc-macro 0.0.6）；libc 0.2.189 复用。锁树 320 个 crate（本次决策时 `grep -c '^name = ' rust/Cargo.lock`；T03 基线 279 的历史语句不改写）。

**性能/安全/兼容影响：**测试构建多一个 50ms 周期采样线程（task_info/statm 微秒级），生产图无变化；默认阈值 8GiB / 600s（时间默认上市时为 300s，收尾验证发现恰好卡在既有最慢合法用例 `approval_face_share_cases` 的固有完成线 ~300.5s 之下而误伤，上调为已知最慢合法用例 ~2 倍富余，见 `artifacts/rust-tauri/R06/T06/09_test_guard/guard_time_default_incident.txt`），`TEST_MEM_LIMIT_MB`、`TEST_TIME_LIMIT_SEC`（0=关闭）、`LINGXI_TEST_GUARD=0`（总开关）可调；`cargo test` 期间被 spawn 的真实服务子进程同样被覆盖（默认静默、阈值宽松）。已知边界：不链接任何被测 crate 的纯 synthetic 测试二进制不会被装载（rustc 不链接未引用 crate）；该边界在 crate 文档中写明。

**退出条件：**若 Rust 未来提供稳定的 test-harness 级进程守卫（或 nextest 提供 in-process 资源限制），迁移并退役本 crate；退役须在 DEPENDENCY_RULES.json 同步登记。

**受影响任务/测试：**全部 `cargo test` 面（kernel/adapters/service/protocol/spike/browser-spike/xtask 的测试二进制）；ownership 校验器（D5-reverse 新成员登记、DEP-07 生效）。

## D-13 R06 资源桌面宿主依赖兼容与独立边界登记

2026-10-13：已有 `desktop/src-tauri` 资源宿主使用独立 manifest 和锁树，不加入 headless workspace。将 Tauri 从 2.11.6 更新为 2.12.1、tauri-build 从 2.6.3 更新为 2.7.2，解决原锁树中 runtime/runtime-wry 2.12.1 与旧 Tauri API 不兼容的实际编译错误；dialog 2.7.3 不变。

`DEPENDENCY_RULES.json` 现按实际包名 `lingxi-resource-desktop` 登记 `tauri-host` 为 exists。边界检查读取并合并独立 manifest 的锁定 Cargo metadata，立即执行 DEP-05 的依赖规则，同时保持 DEP-07 对 headless workspace 的桌面禁令。未忽略独立模块、未扩大其允许业务依赖；新增负控验证宿主直接依赖 kernel、包路径偏离登记均被拒绝。

本机通过 native_resources 两项、renderer 六项、TypeScript 类型检查、独立前端及宿主构建。真实 GUI picker 流程受 macOS 辅助访问拒绝阻塞，未观察完成的窗口、默认程序或系统废纸篓行为不计通过。此登记只反映已存在的 R06 资源子集，不声明 R06/R09 或跨平台验收完成。证据见 `artifacts/rust-tauri/R06/continuation-20261013/desktop-handoff.json` 与 `boundary-registry-selftest-r2.log`。

## D-14 R06 助手角色卡 ZIP 与标准文件上传

2026-10-14：原叶 206785535A40 要求从真实桌面助手设置导入、预览和导出角色卡。已有 Rust 技能安装器只支持目录，显式拒绝 ZIP；原锁树也没有可复用的压缩库或 multipart 提取器。选用 zip-rs 官方 `zip =9.0.3`，关闭默认特性，只启用 `deflate-flate2-zlib-rs`；复用 axum `=0.8.9`，只新增 multipart 特性。依赖在 `rust/` workspace 声明并锁定，headless 图不引入桌面依赖。

锁定新增包：zip 9.0.3、typed-path 0.12.3、nix 0.31.3、flate2 1.1.10、zlib-rs 0.6.8、crc32fast 1.5.2、multer 3.1.0、spin 0.9.9。版本、许可和 API 依据 Cargo registry 中官方下载源码及 `cargo info zip --registry crates-io`；zip 9.0.3 为 MIT、最低 Rust 1.88，本次指定工具链 1.98.1。锁文件生成是实现准备，其余构建与验证均使用 `--locked`。

替代方案：只支持 JSON 会遗漏原 ZIP 操作；调用系统解压工具不可跨平台稳定使用；手写压缩/多段请求解析增加不必要边界风险。读取严格限制总展开大小、条目数及相对路径，拒绝链接、逃逸和重复路径；上传只用 axum 标准提取器。导出仅写指定工作区（不存在工作区时为本助手资料目录下 exports），不写当前进程仓库目录。验证结果与运行坐标随本次独立交付证据提供，不改变原依赖治理规则或验收白名单。

## D-15 R06 EX09 搜索派生层：纯 Rust Jieba 与 NFKC

**问题与用户目标：**现役 `lib/search/search-text.ts` 使用 NFKC → 小写 → 空白折叠、Jieba 搜索模式（HMM=true）、ASCII 整词、CJK 2/3-grams 与最多96个 FTS 字面词。旧 Rust 实际 SQLite 查询把 alpha 拆字母，误命中 beta；全角 Quartz 漏召回；仅固定 n-gram 没有长江大桥词级切分。本次补真实分词，不修改原文、引用偏移、权威事实或历史身份金样。

**选择与依据：**workspace 精确声明 `jieba-rs =0.6.8`（仅 default-dict；MIT）与 `unicode-normalization =0.1.25`（MIT OR Apache-2.0），adapter通过workspace边引用。Jieba调用 `Jieba::new().cut_for_search(normalized, true)`，默认词典直接内嵌、无运行期联网、无桌面/模型依赖。Unicode脚本、标点和ASCII匹配复用已锁定regex，不新增script包。官方API/源码：[Jieba 0.6.8](https://docs.rs/jieba-rs/0.6.8/jieba_rs/struct.Jieba.html)、[该版Cargo源码](https://raw.githubusercontent.com/messense/jieba-rs/v0.6.8/Cargo.toml)、[NFKC API](https://docs.rs/unicode-normalization/0.1.25/unicode_normalization/trait.UnicodeNormalization.html)。

**替代方案：**jieba-rs 0.11.0默认内嵌词典实际通过include-flate的默认zstd特性引入zstd-sys，未满足本次纯Rust增量要求，故未进入最终依赖图；0.7.4仍同样依赖include-flate 0.3。手写中文词典或只有n-gram不能承担现役Jieba语义；外部词典文件和网络下载增加部署/失效面，未采用。

**精确新锁版本与许可：**jieba-rs0.6.8/MIT、unicode-normalization0.1.25/MIT OR Apache-2.0；传递新增byteorder1.5.0/Unlicense OR MIT、cedarwood0.4.6/BSD-2-Clause、fxhash0.2.1/Apache-2.0 OR MIT、hashbrown0.14.5/MIT OR Apache-2.0、phf/phf_codegen/phf_generator/phf_shared均0.11.3/MIT、rand0.8.8/rand_core0.6.4/siphasher1.0.4均MIT OR Apache-2.0。合计13个新(name,version)条目，受控 `cargo update --workspace` 保留全部原锁版本，无全量升级；最终没有新增*-sys/C/C++/桌面栈。现有SQLite/TLS的原生依赖未扩大为本分词依赖。

**缓存兼容：**独立可重建元数据记录Jieba/NFKC/CJK与反向overlap算法代际。旧无元数据且application_id=0的已识别缓存明确标为LegacyStale；旧chunks、原文和旧向量保留，不进行模型调用或自动计费。未知/损坏元数据拒绝并保留，不能当合法旧算法降级。显式重建才原子写新search_text/tag。`current_cache_profile_hash` 仅命名空间化派生缓存，原profile/civ/viv金样算法保持；服务层应在显式重建/查询一致接入，避免旧向量身份冲突和误复用，整体EX09接线由主控负责。

**验证与限制：**新adapter测试直接执行SQLite写入/FTS查询、原旧生产cache重建、事实快照/引用原字节检查、损坏元数据拒绝及旧schema未来版本保护；不mock被验收业务核心。词典只初始化一次，但会增加首用内存和时间，实际受现有测试资源守卫约束。此次本机验证不代替其它平台/正式包；仅为EX09明确适配器份额，不改依赖规则或宣称整体阶段完成。

## D-16 R06 Office 只读转换：受限 ZIP/XML

原两叶2F2B6E173C1B/3617DF5B23D5要求真实Word HTML与首张工作表HTML；当前Rust fsread原先没有转换入口。复用workspace既有zip9.0.3，不启动Node业务进程、不手写XML语法。workspace新增精确quick-xml=0.42.0，service通过workspace边引用，关闭额外特性；MIT，最低Rust1.86。依据当前Cargo registry官方包源码Cargo.toml与events/reader接口；该版本已在独立desktop锁树使用。原锁树所有(name,version)保留，仅新增quick-xml0.42.0，传递memchr复用原版本；完整锁定metadata由Cargo --locked检查。

转换只读取授权文件的受限快照和归档内XML，不写出成员、不解析外部关系、不执行宏或公式、不加载外链。输入20MiB、每XML20MiB、累计展开80MiB、4096成员、200000节点/单元格及256层深度均明确拒绝超限；XML外部实体/DOCTYPE拒绝。Word提取正文/标题与表格并HTML转义；Excel按workbook第一张关系定位，保留共享/富文本、稀疏列、数值和布尔缓存值并转义，不把第二张或计算公式当首张结果。日期按工作簿样式和1900/1904年历转换为明确的ISO8601 UTC时刻，不把Excel序号作为日期显示；与原ExcelJS同一时刻，展示格式不依赖宿主地区英文时区长名。仅做内容转换，不声明排版、字体和公式重算等价于Office软件。

选择理由：已存在zip负责受限解压，成熟XML解析器负责结构与实体；避免手写XML占位、系统命令解压或新增桌面/模型依赖。所有构建/检查继续固定工具链并使用--locked，原依赖边界、断言、阶段规则均不变。实际HTTP、生产预览组件与失败显示的验证随本批专属证据记录；其他平台、完整应用和全阶段不由依赖登记宣称完成。
