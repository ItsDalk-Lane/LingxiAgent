# R09-T01/T02 实施进度（host-core）

状态：**IN_PROGRESS**（T01 证据两套件已绿；T02 套件本文件末尾记录最新 exit code）
负责人：r09-host-core | 分支 `codex/rust-tauri-migration` | 设计依据 `T01_T02_HOST_SERVICE_DESIGN.md` + `IMPLEMENTATION_PLAN.md` D1–D6
证据根：`artifacts/rust-tauri/R09/dev/t01/`、`artifacts/rust-tauri/R09/dev/t02/`（本文件只写摘要；完整回执在证据目录）

## 1. 交付物（工作树，未提交）

### T01｜薄 Tauri 宿主 + 同一 React 产物

| 文件 | 内容 |
|---|---|
| `desktop/src-tauri/tauri.release.conf.json` | release overlay 模板：`frontendDist=../dist-renderer`、`windows:[]`（产品窗由宿主在服务就绪后创建）、release CSP、`bundle.active/targets=app`、`externalBin=binaries/lingxi-service`、`resources` 放 `build.json`、`macOS.signingIdentity="-"`（ad-hoc）。base `tauri.conf.json` **原样保留**（R06 资源宿主可复跑）。 |
| `desktop/src-tauri/tauri.dev.conf.json` | dev overlay：`devUrl=http://localhost:5173`、dev CSP 只放行本机 5173；只用于 `dev:tauri`，不进入 release。 |
| `scripts/tauri-build.mjs` | 构建包装：校验同一 React 产物（`dist-renderer` 树摘要 **+ `lib/theme.js` 摘要**，缺 theme bundle 直接红）、package.json version 注入、host triple 解析、sidecar stage（`binaries/lingxi-service-<triple>` + `resources/rust-service/build.json`）、`tauri build --features r09-host`（release `--bundles app` / debug `--no-bundle`）、输入摘要缓存（crate 源码/renderer/theme/service 清单/config 模板/CLI/工具链）、回执（命令/产物 sha/codesign）。无 feature 的构建不会被证据引用（回执 `hostFeature: r09-host` + 副作用清单）。 |
| `scripts/tauri-dev.mjs` | `dev:tauri`：起 Vite(5173) → `tauri dev --config tauri.dev.conf.json --features r09-host`；任一退出连带清理。 |
| `desktop/src-tauri/src/host.rs` | 产品宿主生命周期：服务先就绪 → 主线程创建隐藏主窗（960×820，min 420×500，title「Lingxi · Rust 预览」）→ 页面 `app_ready` 显示；30s app-ready 兜底；失败先写记录再弹原生 dialog 并 exit(2)；SIGTERM/SIGINT/SIGHUP handler → 统一 `shutdown_and_exit`；`RunEvent::ExitRequested{code:None}` prevent→清理→exit(0)；debug-only 初始化脚本捕获 error/unhandledrejection/console.error/resource；`on_page_load` 诊断。 |
| `desktop/src-tauri/src/commands.rs` | boot 桥命令（`get_server_connection_info/port/token`、`app_ready`（显示窗口 + debug DOM 投影）、`get_platform`（darwin/win32/linux）、`get_app_version`、`get_service_status`、窗口最小/最大/关闭/最大化状态、`window_theme_changed`、`open_external`）；token 类命令只允许 `main` 窗口。 |
| `desktop/src-tauri/src/service/**` | T02 supervisor（见 §2）。 |
| `desktop/src-tauri/capabilities/r09-host.json` | 主窗口 boot 命令 ACL（`core:default` + core:event + 15 条 allow-*）。 |
| `desktop/src/react/host/tauri-desktop-host.ts` | Tauri DesktopHost：`window.__TAURI__` 检测、kind `'tauri'`；boot 基线真实实现；可选成员**真缺失 + capabilityReason**（不用假函数冒充）；未迁移成员抛 `HostCapabilityError` 并在 throw 前记 `window.__r09HostErrors`；`onServerRestarted` 走 `event.listen`。 |
| `desktop/src/react/host/desktop-host.ts` | 解析顺序 `hana > __TAURI__ > platform > web`；桥身份缓存纳入 `__TAURI__`。 |
| `desktop/src/modules/platform.js` | React 前 `data-platform` 引导：Electron 行为逐字保留，新增 Tauri 分支（UA 近似 + `get_platform` 异步纠正）；win32 滚动逻辑抽 `applyPlatform` 不改变语义。 |
| `desktop/src/react/stores/session-actions.ts` | `isDesktopShell()` 纳入 `tauri`。 |
| `desktop/src/react/components/InputArea.tsx` | **唯一业务组件改动**（宿主差异暴露的既有竞态，语义不变）：WKWebView 下 i18n 成功后 placeholder 状态更新可能先于 tiptap `EditorContent` 挂载，旧代码 `editor.view` getter 抛错→区域 ErrorBoundary→首屏输入区消失。修复：view 未就绪时跳过度量 / placeholder refresh 下一帧重试。A01 白名单逐条记录。 |
| `desktop/src/react/__tests__/host/r09-t01-tauri-desktop-host.test.tsx` | 6 例 vitest：解析顺序/缓存、boot 成员透传、`onServerRestarted` listen+unlisten、受限能力抛 `HostCapabilityError`、reset、显式安装优先级。 |
| `package.json` | 仅追加：`build:tauri:renderer`（renderer→theme 固定顺序）、`build:tauri:service`、`build:tauri`、`build:tauri:debug`、`dev:tauri`（无新依赖；`@tauri-apps/cli@2.12.4` 已在 Lead 锁定的 devDependencies）。 |
| `scripts/build-rust-desktop-service.mjs` | `build.json` 增加 `protocol{serverKind,wireProtocolMin,wireProtocolMax,dataEpoch}`（**来源 = 构建产物 `lingxi-service --version`**，不手写常量）；`--verify` 同时核对 stage 公告与二进制 `--version` 一致。 |
| `scripts/rust-tauri/r09_t01.py` / `r09_t02.py` | 四套件真实生产者（构建/启动/信号/进程表/注入），详见 §4。 |

### T02｜服务启动/就绪/退出/远端

`desktop/src-tauri/src/service/`：
`env`（env 快照）· `resolve`（LINGXI_HOME canonicalize、dev/packaged 二进制、server-info/降级门）· `manifest`（build.json + **协议门用 `lingxi-protocol` Rust 常量** wire 1..=1 / data_epoch 2）· `digest`（Mach-O 签名后内容摘要/PE，签名重写 sha 的放行逻辑）· `instance`（O_NOFOLLOW、owner-only 0600、uid、打开前后 dev/ino、pid/home/instanceId/bindAddr、运行时 wire/epoch 门）· `client`（identity Bearer + health，5s/20×100ms 重试）· `remote`（R08-T03 同形 env 契约、owner-only token 文件、cleartext 门、release 拒绝）· `shutdown`（SIGTERM→17s→SIGKILL(进程组)→5s，residual 报告）· `supervisor`（spawn→READY 90s→instance→health→identity→publish；崩溃最多重启 1 次；reused/remote 结构性不持有 Child）· `diagnostics`（`{home}/diagnostics/tauri-launch/events.jsonl`、失败记录、tauri-crash.log、secret 脱敏）。

**实测中修正的关键点（诚实记录）**：
1. **数据根写入纪律**：Rust 服务 data-epoch 门只接受“可证明是新的” home（空或只含 `lingxi-service/`）。宿主在 spawn 前写 `{home}/diagnostics` 会让全新 home 被服务以 `unstamped-home-with-data` 拒绝。→ 诊断改为惰性：服务 READY（服务已 stamp）后激活并回填启动期缓冲；未 stamp home 的失败记录写 `$TMPDIR/lingxi-tauri-launch/<hash>/failure.json` 并把路径打到 stderr，**绝不污染数据根**。
2. **release 包内二进制命名**：Tauri bundler 保留 cargo bin 名，产物为 `Contents/MacOS/lingxi-resource-desktop`（不是产品名 `Lingxi`），productName 决定 `.app` 名与版本元数据。证据按实际路径记录（设计草案里的 `Contents/MacOS/Lingxi` 不成立；不改 R06 bin 名以保 R06 E2E 可复跑）。
3. **ad-hoc 签名**：`bundle.macOS.signingIdentity="-"` 使 bundler 对 sidecar/主程序/.app 统一 ad-hoc 重签；`codesign --verify --deep --strict` 通过；正式签名/公证仍为 `BLOCKED-SIGNING-IDENTITY`，不冒充。
4. **theme bundle**：`index.html` 先加载 `lib/theme.js`（`vite.config.theme.js` 产物）。只跑 `build:renderer` 会缺该文件：Electron file:// 安静 404，Tauri 自定义协议回退 index.html → `Unexpected token '<'`。构建链固定 `build:renderer → build:theme`，wrapper 把 theme sha256 纳入回执；不删 index.html 引用。
5. **自动化 UI 抑制（用户可见弹窗事故修复）**：宿主失败 dialog 在
   `LINGXI_R09_AUTOMATION=1` 下**不弹 modal**，改为写 `LINGXI_R09_HOST_DIALOG_SINK` 的结构化
   JSON（code/message/expected/actual/detailPath/home/`modalShown:false`）并记诊断事件
   `failure-dialog-suppressed`；无 sink 时只写诊断。生产默认（无 env）行为完全不变。
   同时修复失败 dialog 文案里 `{LINGXI_HOME}` 未插值的问题：现在使用真实绝对路径
   `detailPath`（诊断写者统一脱敏 token）。所有生产者启动 app 前注入该 env 与 sink，
   负例断言 sink code/`modalShown:false`，正例断言 sink 不存在。
   教训：T02 早期套件连续多次启动 GUI 并弹 modal（其中一次 `packaged-exit-graceful`
   等待 45s 后 kill -9，留下 sidecar 孤儿）——已改为单发+finally 进程表清理。
6. **A01 对照方式**：Tauri 侧用 debug-only 宿主 DOM 投影（`__TAURI__` 页内 JS，禁用 release 注入），Electron 侧用 `--remote-debugging-port` CDP 页内投影；两壳同窗对照（Electron 先启动并保持存活，再启动 Tauri），比较归一化标题/平台/React 根/正文 hash/长度。不是 WebDriver 全矩阵（T07 范围）。

## 2. 真实运行链（实测摘录）

隔离 `LINGXI_HOME=/private/tmp/...`，debug 产品构建 + 真实 `rust/target/debug/lingxi-service`：

```
host-start → resolve → spawn(pid,pgid) → ready(addr, 119–124ms) → instance(protocol wire 1..=1 epoch 2)
→ identity(serverId/studioId, transport=http) → service-ready(local-owned) → window-created(≈0.36–0.6s)
→ page-load tauri://localhost(Started/Finished) → app-ready(url/title, windowVisible=true) → dom-projection
SIGTERM → signal → shutdown{confirmed:true, residual:[], ownedPids:[…]} → shutdown-complete → exit 0
```

- 崩溃重试实测（误杀服务触发）：`service-crashed{willRetry:true}` → 新 pid READY 3.07s → `service-ready{restarted:true}` → 退出仍只清自有进程组。
- release `.app` 实测：sidecar 被 ad-hoc 重签后 `sha256≠manifest.sha256`，`contentSha256` 相同 + `codesign` 通过 → `signedMutation:true` 放行；`strings` 中 `devUrl/localhost:5173/127.0.0.1:5173/dist-tauri-resources/wdio/webdriver` 计数全 0；注入 `VITE_DEV_URL` 后页面仍为 `tauri://localhost`，5173 canary 连接 0；SIGTERM exit 0。
- 页面 origin 接缝（D6）：Tauri 页 origin 下 identity/health fetch、WS 均可用（`已连接` 状态实测；服务 Origin 接缝由 Lead 探针 13/13 覆盖）。

## 3. 门禁契约与命令

```bash
# 套件（真实构建/启动/信号；argv/evidence 由 R09.json 固定，不改 map）
python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t01_react_parity   --evidence <新目录>
python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t01_release_offline --evidence <新目录>
python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t02_supervisor_ready --evidence <新目录>
python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t02_process_ownership --evidence <新目录>
# 单元/集成（真实服务）
rustup run 1.98.1 cargo test --manifest-path desktop/src-tauri/Cargo.toml --lib --features r09-host
rustup run 1.98.1 cargo test --manifest-path desktop/src-tauri/Cargo.toml --locked --features r09-host --test service_supervisor -- --nocapture
# 构建
node scripts/tauri-build.mjs --debug --receipt <evidence>/tauri-build-debug.json
node scripts/tauri-build.mjs --receipt <evidence>/tauri-build-release.json
# 静态能力一致性
python3 -B scripts/rust-tauri/r09_capability_audit.py --evidence <新目录>
```

## 3.9 自动化阶段 1 单发验证（Lead 限定步骤，2026-10-26T22:27）

- 命令：单次 debug 宿主（`LINGXI_R09_AUTOMATION=1` + `LINGXI_R09_HOST_DIALOG_SINK`，隔离 home/tmp，
  真实服务），随后对 owned pid 连续两次 SIGKILL（第 1 次触发唯一一次重启，第 2 次失败）。
- 证据：`artifacts/rust-tauri/R09/dev/t01/phase1-automation-01/`（sink-failure-dialog.json、
  events.jsonl、app-stderr.log、tauri-build-debug.json、phase1-automation-summary.json）。
- 结果：app exit **2**；sink code=`RUST_SERVICE_CRASH_RESTART_EXHAUSTED`、`modalShown:false`、
  `detailPath` 为真实绝对路径；窗口 `visible=false`（`app-ready-window-suppressed`
  + `app-ready.windowVisible=false` + `Accessory` 激活策略），隐藏窗口 DOM eval 正常
  （bodyTextLength=121、errors=[]）；进程表 final 计数 **0**；无 modal 路径。
- 说明：agent 无法看屏幕；“无可见窗口/弹窗”= 未调用 show + visible=false + 未走 dialog 分支 +
  sink 替代 + 进程 0 的结构性证据。

## 3.10 renderer 构建树摘要非确定性（证据口径与 R10 已知项）

- **事实**：同一源码连续执行 `npm run build:renderer && npm run build:theme`，
  `desktop/dist-renderer` 树摘要会变化（实测 `d09359…` / `d5a24a…` / `afcdc1…`；也有连续两次完全一致的情形）。
- **根因（已定位）**：`settings.html` 注入的 CSS `<link>` 顺序非确定（其余 643/644 文件逐字节相同；
  JS/CSS 资产内容哈希稳定）。属 Vite 多入口 CSS 依赖排序的实现细节，不是本任务源码改动。
- **证据口径（Lead 裁决，已执行）**：**单次构建内 build-once-bind-once**：
  `scripts/tauri-build.mjs` 内部固定 `build:renderer → build:theme` 并记录
  `rendererDigest` + `themeBundleSha256` + `rendererBuild[]`；`tauri build` 嵌入同一目录；
  Electron 侧在启动前/后核对同一摘要；**跨两次构建摘要相等不是不变量**，不得用作 A01/A02 绑定。
- **复现/复核**：`tmp=$(mktemp -d); npm run build:renderer && npm run build:theme` 各两次，比较
  `settings.html` 中 `<link>` 顺序；其余文件 sha256 相同。证据脚本 `scripts/rust-tauri/r09_t01.py`
  的 `build_consistent_host` 已按“构建后磁盘摘要 == 回执摘要”判定。
- **R10 发布复现性已知项**：发布资产目录树摘要不可重复（settings.html 排序）；不影响功能/签名，
  但影响“同源重建摘要相等”类发布断言。可另立小项让构建后对 HTML `link` 做确定性排序再落盘
  （不改产物语义），不在 T01/T02 阻断范围。

## 4. 证据状态（本文件最后更新时）

| 套件 | 证据目录 | 结果 |
|---|---|---|
| `r09_t01_react_parity` | `artifacts/rust-tauri/R09/dev/t01/run-08` | **PASS exit 0 / 18 cases**（renderer 预览 `2c29…`→theme 后新摘要；两侧同摘要绑定、DOM 投影一致） |
| `r09_t01_release_offline` | `artifacts/rust-tauri/R09/dev/t01/run-release-03` | **PASS exit 0 / 16 cases**（release `.app`、codesign 校验、strings 0、canary 0、SIGTERM exit 0） |
| 自动化阶段 1 | `artifacts/rust-tauri/R09/dev/t01/phase1-automation-01` | **PASS（单发）**：sink `modalShown:false`、窗口 hidden、exit 2、ps 0 |
| `r09_t02_supervisor_ready` | `artifacts/rust-tauri/R09/dev/t02/phase2-supervisor-03` | **PASS exit 0 / 27 cases**（5 发单发：debug 正/release 正/注入协议/注入摘要/fake；逐发 ps-clean、sink/无 sink 断言、窗口 hidden、code 匹配） |
| `r09_t02_process_ownership` | `artifacts/rust-tauri/R09/dev/t02/phase2-ownership-02` | **PASS exit 0 / 16 cases**（5 发：owned+remote/reused/remote/拒远端/pid 复用；逐发 ps-clean、no-orphan、remote-untouched 修正后绿） |
| 历史红→绿 | `phase2-supervisor-01/02`、`phase2-ownership-01` | 保留：生产者 NameError ×2、ppid 断言取值 bug、SIGTERM 未发导致超时等；均已在修复后新目录复跑 |
| Rust lib tests | `cargo test --lib --features r09-host` | 56 passed（service 模块单测 + SQLite 等既有） |
| Rust supervisor 集成 | `--test service_supervisor` | 8 passed（真实服务 roundtrip/超时/运行时协议门/reused/remote/拒绝远端/server-info/降级） |
| Vitest host | `r09-t01-tauri-desktop-host.test.tsx` + R08 web host | 6 + 5 passed |
| `npx tsc --noEmit` | 全仓 | exit 0 |
| capability audit（T01+T05 命令面） | `artifacts/rust-tauri/R09/dev/capability-audit-01` | 44 命令 registered==manifest==permissions，`acl-manifest-covers-registered` 绿；仅 `policy-present` PENDING（T06 负责 `R09_COMMAND_POLICY.json`） |

> 说明：`run-01…07` 等历史目录保留红/中间态（含并发写 renderer 导致的 digest 漂移、首屏 ErrorBoundary 归因过程），便于审计“红→绿”证据；**最终登记以最新 PASS 目录为准**。

## 5. A01–A04 → case 建议映射（Lead 统一登记 FULL_BINDINGS）

A01（`r09_t01_react_parity/cases.json`）：
`a01::renderer-dist-digest-recorded`、`a01::business-components-untouched`、`a01::tauri-build-uses-same-renderer`、`a01::tauri-debug-artifact-built`、`a01::electron-boots`、`a01::electron-service-identity`、`a01::tauri-boots`、`a01::tauri-loads-embedded-page`、`a01::dom-snapshot-parity`、`a01::service-identity-parity`、`a01::no-second-react-entry`、`a01::renderer-has-no-resource-ui`、`a01::isolated-homes`、`a01::electron-exit`、`a01::tauri-graceful-exit` + Producer 内建 `isolation::real-home-untouched`、`suite::all-cases-hold`。

A02（`r09_t01_release_offline/cases.json`）：
`a02::release-binary-built`、`a02::no-devurl-in-release-binary`、`a02::no-test-plugins-in-release-binary`、`a02::adhoc-signature-verified`、`a02::no-vite-5173-listener`、`a02::canary-zero-connections`、`a02::webview-loads-embedded`、`a02::ui-boots`、`a02::env-cannot-redirect`、`a02::no-5173-sockets`、`a02::service-ready-manifest`、`a02::graceful-exit`、`a02::home-stamped`、`a02::no-node-server-info`。

A03 自动化/清理新增：`t02::automation-no-modal-positive`、`t02::automation-no-modal-packaged`、`t02::failure-dialog-suppressed-protocol`、`t02::failure-dialog-suppressed-digest`、`t02::failure-dialog-suppressed-all`、`t02::ready-timeout-bounded`（headless 集成测试）、`isolation::no-residual-processes`。

A03（`r09_t02_supervisor_ready/cases.json`）：
`t02::ready-handshake-real`、`t02::no-fixed-sleep-handshake`、`t02::debug-exit-graceful`、`t02::manifest-protocol-match`、`t02::packaged-exit-graceful`、`t02::manifest-protocol-mismatch-rejected`、`t02::injected-protocol-no-service-spawn`、`t02::injected-protocol-no-node-fallback`、`t02::manifest-digest-mismatch-rejected`、`t02::injected-digest-no-service-spawn`、`t02::runtime-identity-mismatch-rejected`、`t02::error-surfaced-records`、`t02::dialog-observation-boundary`、`t02::ready-timeout-bounded`、`t02::no-node-fallback-in-release-binary`。

A04（`r09_t02_process_ownership/cases.json`）：
`t02::no-orphan-owned-services`、`t02::two-services-coexist`、`t02::exit-owned-tree-only`、`t02::remote-untouched`、`t02::reused-is-detached`、`t02::remote-mode-zero-touch`、`t02::packaged-refuses-remote`、`t02::pid-reuse-safe`。

## 5.1 自动化纪律（本轮事故后的硬规则）

- 任何 GUI 证据：**每套件一次性串行启动，失败即中止**，不重试、不循环。
- 每个 Tauri 启动注入 `LINGXI_R09_AUTOMATION=1` + `LINGXI_R09_HOST_DIALOG_SINK`；
  系统可见动作（modal/通知/托盘，native 合并后同样走该门）只记录 “would show”，不弹。
- `run_suite` 的异常路径与每个套件正常结束都调用 `cleanup_runtime_processes`
  （SIGTERM→SIGKILL 并做进程表断言 `isolation::no-residual-processes`）；
  A04 另有 `t02::no-orphan-owned-services`。
- 构建缓存不再只看“文件存在”：命中前核对回执记录的可执行文件 sha256
  （防外部 `cargo build` 覆盖 target 二进制导致版本/配置错位）。

## 5.9 native（T03/T04）合并状态（host-core，2026-10-26T22:5x）

已完成（代码在工作树，编译/测试绿，**未启动 GUI**）：
1. `desktop/src-tauri/Cargo.toml`：`tauri` features `["image-png","tray-icon"]`；新增
   `tauri-plugin-single-instance=2.5.4`、`global-shortcut=2.4.1`、`notification=2.7.0`、
   `autostart=2.7.0`、mac `objc2-foundation=0.3.2`、`objc2-user-notifications=0.3.2`、
   `block2=0.6.2`；`[lints.rust] unexpected_cfgs` 允许 `r09_standalone_test`。
2. `src/lib.rs`：`pub mod native;`；Builder 首行 `native::install::with_plugins(...)`（single-instance
   最先）；setup 中 `host::setup` 之后 `native::install::install(app)?`（manage NativeState + 托盘 +
   quick-chat 快捷键 + mac 通知点击 delegate + 权限探测）。
3. 验证（无 GUI）：`cargo build --locked --features r09-host` **exit 0**；`cargo test --lib --features
   r09-host` **238 passed**（含 native 182 + service 56）；`cargo test --test service_supervisor`
   **9 passed**；无 feature 的 R06 base `cargo check` **exit 0**。

**未完成（交 T06/下一 cleared 批次）**：`native/install.rs::INVOKE_COMMANDS` 的 28 条命令 +
`DEBUG_INVOKE_COMMANDS` 2 条尚未注册（命令 glue 函数、`invoke_handler`、`build.rs`、
`capabilities/{main,settings,quick-chat,splash,onboarding}.json` 按窗口 allow-*、Info.plist 语音/麦克风
描述、主窗 close/move-resize 生命周期接线）。审计（T06 policy 已存在）当前红项与归属：
- `caps::window-plugin-permissions::{main,browser-viewer,viewer-*,skill-viewer-*}` 与
  `caps::no-dangerous-plugin-permissions`：现有 capabilities 仍用 `core:default`（展开含
  menu/tray/webview devtools 叶子），policy 期望窗口级只给精确 leaf（如 `core:event:default`）——
  涉及 R06 `capabilities/main.json`（R06 兼容资产）与 T05 `browser-viewer.json`、T06 既有
  `viewer/skill-viewer` 文件，属 T06 收口面。
- `caps::test-control-env-not-in-release-paths`：`LINGXI_R09_AUTOMATION`/`LINGXI_R09_HOST_DIALOG_SINK`
  在非 `cfg(test)` 源码被引用（release 也读）——需要 T06 决定用 feature（如 `r09-test-hooks`）门控
  证据构建 vs 交付构建；A03 release 负向证据依赖该 env 抑制 modal，需在证据构建中显式启用并记录。
- 命令面缺口（28+2 条）导致 T03/T04 producer 仍 blocked（真实系统动作待注册）。

## 6. 未完成 / 边界（如实）

- **native/（T03/T04）注册合并**：本轮未完成；文件已由 r09-native-caps 交付，注册面（Cargo 依赖/first-plugin/install/invoke_handler/build.rs/capabilities 八窗口）由 host-core 合并，属于下一步。
- **A01 对照范围**：Electron 侧以 `electron .` 源码 + CDP 投影，Tauri 侧 debug 构建 + 宿主投影；非 WebDriver 全量交互矩阵（T07）。Electron 夹具 home 需由真实服务先 stamp（Electron 会在 spawn 前写 user-data/diagnostics；未 stamp 新 home 会被 Rust epoch 门拒绝）——这是夹具事实，已写入证据。
- **theme.js**：仓库 `index.html` 引用 `lib/theme.js`，构建链必须 `build:theme`；缺文件时 Tauri 自定义协议回退 index.html 报 `Unexpected token '<'`（A01 诊断已捕获，构建链已修）。
- **可选成员**（`trainUpdateStatus`/`onTrainUpdateAvailable`/`onMaximizeChange`/`onTrainFallbackNotice`/`onTrainUpdateProgress`/`setBrowserConnection`）在 Tauri host 中**真缺失 + capabilityReason**；T03/T05/R10 逐项补齐，不伪造状态。
- **A03 native dialog 视觉观察**：失败记录/崩溃日志/diagnostics 已取证；dialog 像素级观察需 Accessibility/Screen Recording（`BLOCKED-SYSTEM-PERMISSIONS`），不冒充。
- **宿主被 SIGKILL 的孤儿边界**：macOS 无父死子亡原语（Electron 同样如此）。优雅退出路径
  （SIGTERM/关窗/Cmd+Q/OS 登出）由 supervisor SIGTERM→17s→SIGKILL 与 `shutdown` 报告
  `residual:[]` 覆盖；宿主被 `kill -9` 时 sidecar 会成为孤儿——T02 登记为**已知边界**，
  生产者 finally 必须清理并按进程表断言零残留；不声称已实现父死子亡。
- **平台**：全部证据 `macos-arm64`；mac x64/Windows/Linux `BLOCKED-PLATFORM-*`（Windows 的 Job Object 收敛、`--read-private-runtime-json` 私读未实现/未验证）。

## 7. v2 冻结期只读改造（2026-10-26T23:3x）

- `r09_t01.py`：删除 renderer/tauri 重建路径，改为只读消费：
  `LINGXI_R09_T01_TAURI_APP`（v2 wrapper/evidence 可执行）+
  `LINGXI_R09_T01_TAURI_RECEIPT`（回执，校验 rendererDigest == 磁盘）；
  `LINGXI_R09_T01_RELEASE_APP`（v2 delivery .app）+ `..._RELEASE_RECEIPT`。
  无 env → exit 2 `CANDIDATE_REQUIRED`；renderer 摘要漂移 → 同错误（不重建）。
  新增 `LINGXI_R09_T01_VISIBLE=1`（可见窗口批次，不注入 automation/sink）+ `screencapture` 证据点。
- `r09_t02.py`：同模式 `LINGXI_R09_T02_DEBUG_APP`/`..._DEBUG_RECEIPT`、
  `LINGXI_R09_T02_RELEASE_APP`/`..._RELEASE_RECEIPT`；`ensure_service_stage` 改为只读 `--verify`（失败即 CANDIDATE_REQUIRED，不重建）；
  新增 `LINGXI_R09_T02_HIDDEN_ONLY=1`：只跑 debug hidden 子集，release/可见用例登记
  `blocked::t02::visible::*`（整轮 FAIL，不冒充）。
- 验证（无 env、无 GUI）：T01/T02 均 exit 2 `CANDIDATE_REQUIRED`，日志无任何 build/tauri 命令、无新进程。

## 8. final-01 门后修复（2026-10-27，Lead 指派 A–E）

- **A｜`desktop/src-tauri/src/commands.rs`（hooks 门控，production 默认不变）**
  - L91-125 新增纯决策：`AppReadyWindowAction`/`app_ready_window_action(automation, already_ready, visible)`
    （automation 下**每次**调用都 `Suppress{hide_if_visible}`，与 already_ready 无关）+
    `AutomationWindowControl`/`automation_window_control_action` + `suppress_window_control_in_automation`
    （automation 下 window_minimize/window_maximize 不改变可见性，若已可见则 hide，并写
    `window-control-suppressed` 诊断）。
  - L227 `app_ready`：改用纯决策；automation 分支若窗口已可见则 `hide()`，诊断记录
    `visibleBefore/visibleAfter/alreadyReady`；非 automation 保持每次 `show()`（原语义）。
  - L336/L344 `window_minimize`/`window_maximize` 加 automation 守卫。
  - L410+ 新增 3 个单测：automation×already_ready×visible 全矩阵均 suppress；连续两次调用均 suppress；
    automation=false 才 Show；window control 守卫决策。
  - 验证：`cargo check --features r09-host` exit 0；`cargo test --lib commands::tests` **3 passed**。
- **B｜`scripts/rust-tauri/r09_t01.py`**
  - `RENDERER_DIFF_WHITELIST` 增加 `desktop/src/modules/connection-csp.js`（v3.2 页面 meta CSP asset 修复，
    audit `caps::page-meta-csp-allows-asset` 回归；其余 v3/v3.2 变更已被 host/ 目录与既有条目覆盖）。
  - 验证（无 GUI）：对当前 `git status -- desktop/src/react desktop/src/modules` 应用 whitelist →
    `outsideWhitelist: []`（changed 8 个文件全部登记）。
- **C｜`scripts/rust-tauri/r09_t02.py`**
  - 新增 `launch_hidden_tauri()`（L67-79）：T02 所有 debug 启动**显式**注入
    `LINGXI_R09_AUTOMATION=1` + `LINGXI_R09_HOST_DIALOG_SINK`，覆盖全局 `LINGXI_R09_T01_VISIBLE=1`
    泄漏；5 处启动改用（L463/618/771/840/880）。修复 final-01
    `dialog-tauri-home-runtime-mismatch` 缺失（fake 服务 launch 未注入 sink）。
  - `cargo-supervisor-tests` 改为 **隔离 `CARGO_TARGET_DIR=<runtime>/t02-supervisor-cargo-target`** +
    `--features r09-host,e2e-test`：final-01/H3 期间该 `cargo test`（旧 `--features r09-host`）编译
    `CARGO_BIN_EXE` 覆盖了 `desktop/src-tauri/target/debug/lingxi-resource-desktop`（污染证据：sha
    `99e76032…`/`7b116ea4…`，55,384,664B，wdio strings=0，mtime 与 T02 suite 结束 00:10:04 同秒）。
  - 验证：`py_compile` 通过；无 GUI 自检（无 env）两套件 exit 2 `CANDIDATE_REQUIRED`，无启动/无构建。
- **D｜`scripts/rust-tauri/r09_t07.py`**：确认 release_smoke **零构建调用**
  （`grep -E "tauri-build|npm run build|tauri build|cargo build"` = 0；final-01 output.log 仅
  strings/codesign/read-only；file-task 保持显式红）。
- **E｜恢复**：`target/debug` ← `/private/tmp/lingxi-r09-evidence/lingxi-resource-desktop-030607ee`
  （sha `030607ee…`）；receipt ← `513aba98…`；delivery exe `8e74aa15…` 未变；renderer `b0a20250…` 未重建。
- **T06 重建建议**：commands.rs 属产品宿主源 → v3.3 应重建 **G1 evidence + delivery release +
  release-evidence**（全部 `--no-renderer-build`，renderer 不变）；仅重建 G1 会造成源码/交付候选不一致。
  T01/T02/T07 脚本改动不影响候选二进制。v3.3 后 wrapper 需从新 G1 稳定副本重建。
