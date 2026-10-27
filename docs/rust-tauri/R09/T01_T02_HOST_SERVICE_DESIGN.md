# R09-T01/T02 实现设计调查：薄 Tauri 宿主 + 同一 React 产物 / 服务启动·就绪·退出·远端模式

> 状态：**READ_ONLY_DESIGN_INVESTIGATION**（设计调查，不是实现、不是验收通过）。
> 基线：分支 `codex/rust-tauri-migration`，HEAD `aac8cb9fe6bb`（2026-10-26 21:0x–21:2x 快照）。
> 任务：R09-T01（A01/A02）与 R09-T02（A03/A04）的实现设计取证。
> 纪律：只读生产代码/配置/锁文件/阶段图；本调查只新建本文件与同名 `_EVIDENCE.json`，未构建、未启动、未打包、未修改任何生产文件。
> 结构化版本：`docs/rust-tauri/R09/T01_T02_HOST_SERVICE_EVIDENCE.json`（schema `lingxi.r09.t01t02-design.v1`）。

---

## 0. 调查范围、基线与环境事实（先读）

### 0.1 必读输入与已核实的工作区状态

| 输入 | 结论 |
|---|---|
| `AGENTS.md` | Node>=24.12<25、Electron 42 + Vite 7 + React 19；`desktop/main.cjs` 主进程、preload 桥、`desktop/src/react`；红线=禁止静默降级。 |
| `docs/rust-tauri/R09/START_RECEIPT.md:3-8` | 基点 HEAD `aac8cb9f`；工具链 rustup 1.98.1；`desktop/src-tauri` 仍是 R06 文件资源宿主；完整 `DesktopHost-Tauri` 尚不存在；xtask 原只注册 R02–R08。 |
| `docs/rust-tauri/R09/BLOCKED.md:8-14` | 平台（mac x64/Windows/Linux）、系统权限、签名身份（只有 ad-hoc）为 OPEN；`BLOCKED-TAURI-CLI-OFFLINE` 已 RESOLVED。 |
| `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R09_Tauri桌面宿主、原生能力与跨平台集成.md:42-122` | T01/T02 的“做什么/怎么做/验收”原文（本文逐条对表见 §5）。 |
| `docs/rust-tauri/R00/ACCEPTANCE_MAP.json:6921-7020` | A01–A04 的 given/when/then/evidence/environment；`scenarios` 的 `spec_status` 均为 NOT_STARTED。 |
| `docs/rust-tauri/R08/R08_HANDOFF.json:134-138,165-168` | R09 对照基线 = `artifacts/rust-tauri/R08/verify-r08-closeout-08/SUITES/r08_a16_versions/a16-versions.json`；不得重改模型协议/数据架构。 |
| **调查期间的工作区并发事实**（非本调查所写） | 21:14 前后工作区出现 Lead 的 R09 基础设施：`rust/crates/xtask/src/stage_maps/R09.json`、`R09_INPUTS.json`、`scripts/rust-tauri/r09_{suites,producer,stage_inputs,map_integrity}.py`；`package.json:139` 新增 `"@tauri-apps/cli": "2.12.4"` 且 `package-lock.json` 同步、`node_modules/.bin/tauri` 存在。本文按该实际状态引用，**不修改它们**。 |
| `docs/rust-tauri/R09/PROGRESS.md:12-58,59-78` | Lead 的 R09 实施规格与写作用域纪律：阶段图/`package.json`/锁文件/共享入口由 Lead 唯一维护；任务实现者只改各自任务文件与 `scripts/rust-tauri/r09_t0X.py`；§6 明确“读取三份设计调查并由 Lead 复核成实施规格”，T01 范围为扩展 `desktop/src-tauri` + DesktopHost-Tauri + dev/release 配置 + CLI 构建脚本，T02 为 supervisor/握手/进程所有权。本文结构与之一致。 |

### 0.2 本机环境实测（本次只读命令输出）

| 项 | 实测 | 证据 |
|---|---|---|
| OS/架构 | macOS 27.0.1（Build 26A434）`arm64` | `sw_vers`、`uname -m` |
| Xcode | 仅 CLT 27.0：`xcode-select -p` = `/Library/Developer/CommandLineTools`；`xcodebuild -version` 报 “requires Xcode” | 终端输出 |
| 签名身份 | `security find-identity -v -p codesigning` = **0 valid identities** | 终端输出 |
| 可用签名/打包工具 | `/usr/bin/codesign`、`/usr/bin/hdiutil`、`/usr/bin/osascript`、`/usr/bin/SetFile`、`/usr/bin/xcrun` 均在 | `which` |
| Rust | `~/.cargo/bin/rustup 1.29.1`，active = `1.98.1`（由 `rust-toolchain.toml` 覆盖）；**PATH 上的 `/opt/homebrew/bin/cargo` 是 1.93.0 且不读 toolchain 文件** | `rustup show` |
| Node/npm | node `v24.16.0`；npm registry 可达（`npm view @tauri-apps/cli version` → `2.12.4`；`@tauri-apps/api` → `2.12.4`） | 终端输出 |
| Tauri CLI | 当前工作区已装 `@tauri-apps/cli 2.12.4`（`package.json:139` 精确版本；`node_modules/.bin/tauri` 存在） | 文件/终端输出 |
| Renderer 产物 | `desktop/dist-renderer` 目录树 sha256 = **`3e5339cc92f9f29a30846a101b3e4636d3b4e2d7f896980c57b8f31b129a89ef`**，与 R08-A16 基线 `rendererDist` **完全一致** | `a16-versions.json`、本次复算 |
| Rust debug 二进制 | `rust/target/debug/lingxi-service` sha256 = **`b855e5d638aea6605c1b2d634377f230e1b6118e71522eb98a8be841ed10faa0`**，与 R08-A16 基线一致 | 同上 |
| contract-versions | `shared/contract-versions.json` sha256 `6d869347…6f96`，与 R08-A16 基线一致 | 同上 |
| 未存在的产物 | `dist-rust-service/` 不存在；`dist/mac-arm64/Lingxi.app` 不存在；`desktop/src-tauri/target` 只有 R06 debug host（`lingxi-resource-desktop`，2026-10-19 构建） | `ls` |

> **结论级提醒**：`desktop/dist-renderer` 与 `rust/target/debug/lingxi-service` 当前**逐字节**等于 R08 交接基线，因此 T01/T02 的 A01/A03 证据可以把“固定 R08 React 代码与 Rust 协议”绑定到同一对照基线；一旦 T01 实现为接入 Tauri 而改动 React 侧（新增 host 文件并重建），新 digest 必须重新绑定并在证据里显式记录 diff 白名单（见 §5.1）。

### 0.3 已读但未执行

本调查只做源码/配置/文档/环境读取与摘要复算。**未执行**：`npm run build:renderer`、`npm run build:rust-service`、`npx tauri build`、任何 app 启动、任何 WebDriver/CDP 会话、任何安装/签名/公证。§5 的命令均为**设计草案**，尚未运行；“通过/失败”不由本文声称。

---

## 1. 执行摘要（供 Lead 快速裁决的 8 条）

1. **Electron 的 Rust 预览链已是 T02 的完整行为模板**：显式 env 门（`LINGXI_DESKTOP_SERVER_RUNTIME=rust`）→ 按 dev/packaged 解析二进制 → `--home` spawn → stdout `LINGXI_SERVICE_READY addr=…` 就绪 → `instance.json`/`local-token.json` 私读 + `/lingxi/v1/server/identity` Bearer 校验 → 崩溃最多重试 1 次 → 彻底退出只清自有进程树、远端零接触。Tauri 侧最小实现是**把这条链用 Rust 重写**，不是新协议（锚点见 §2）。
2. **Rust 服务没有 `--port`/`--token` 参数**：默认 `--bind 127.0.0.1:0` 临时端口，per-start token 写入 `{home}/lingxi-service/local-token.json`（0600，`auth.rs:76,2110-2133`），实例记录 `instance.json` 由服务在 listener 绑定后原子写入（`instance.rs:157-177,561-600`）。宿主只能：`--home` + 解析 READY 地址 + 读私有记录 + 验证身份。
3. **协议/数据版本门的正确来源是 Rust 常量，不是 `shared/contract-versions.json`**：Rust `lingxi_protocol::handshake::WIRE_PROTOCOL_{MIN,MAX}_SUPPORTED = 1..=1`（`handshake.rs:21-23`），`ContractVersions::R08_CUTOVER.data_epoch = 2`（`lingxi-protocol/src/lib.rs:408-438`）；而 Node 侧 `shared/contract-versions.json` 的 `DATA_EPOCH` 仍是 `1`（旧 Node 轴），renderer 用 `RUST_ACCEPTED_DATA_EPOCHS=[1,2]` 兼容（`desktop/src/react/services/websocket.ts:58,132,142-153`）。Tauri 宿主若拿 Node JSON 做门禁会**误拒 epoch 2 的 Rust 服务**。另外现有 `build.json` **没有协议字段**（`scripts/build-rust-desktop-service.mjs:133-137`），A03 的“包内协议版本不兼容”必须先扩展构建身份（§4.4）。
4. **T01 的“同一 React 产物”= `desktop/dist-renderer`**：它由根 `vite.config.ts` 产出（`vite.config.ts:299,339,342-357,361`），当前树摘要等于 R08 基线；`desktop/dist-tauri-resources` 是 R06 资源宿主专用的另一套小 UI（`desktop/vite.tauri-resources.config.ts:5-11`，1.7MB vs 84MB），**不能**拿来当 T01 产物。Tauri 通过 release overlay 的 `build.frontendDist="../dist-renderer"` 把同一构建嵌入二进制。
5. **DesktopHost-Tauri 的注册位置**：新增 `desktop/src/react/host/tauri-desktop-host.ts`，在 `desktop-host.ts:18,85-107` 的解析顺序中加入 `window.__TAURI__` 分支（顺序：`window.hana`（Electron）> `window.__TAURI__`（Tauri）> `window.platform` 收编 > Web 受限宿主），`DesktopHostKind` 增 `'tauri'`；此外必须同步 `desktop/src/modules/platform.js:13-22`（data-platform）、`desktop/src/react/stores/session-actions.ts:276-279`（isDesktopShell）。`withGlobalTauri: true` 在现有 `tauri.conf.json:8` 已启用，可零新增 npm 依赖实现桥。
6. **配置分离的最小风险方案**：保留 `desktop/src-tauri/tauri.conf.json` 原样（R06 base，`frontendDist=../dist-tauri-resources`，保证 R06 `tests/desktop_host.rs` 的 `CARGO_BIN_EXE` 行为可复现），新增 `tauri.dev.conf.json`（devUrl=5173）与 `tauri.release.conf.json`（frontendDist=../dist-renderer、productName=Lingxi、externalBin、resources、CSP、bundle.active=true），由新的 Node 构建包装脚本生成并 `tauri build --config`。**overlay 合并语义、CLI 编译期嵌入哪份 config** 必须先用探针验证（Q-05）。
7. **服务二进制分发推荐 `externalBin` sidecar + `resources` 放 `build.json`**：R01-T06 已在本机实测 `npx tauri build --bundles app` 与“打包 .app 内 sidecar 握手成功”（`docs/rust-tauri/R01/R01-T06_REPORT.md:37-38`、`TAURI_SPIKE_REPORT.md:59-77`），且 sidecar 运行时解析 `current_exe()` 同级去 triple 后缀文件；`resources` 布局的嵌套可执行签名未经验证，列为备选（§3.8）。
8. **A01–A04 证据必须对齐 Lead 已落的 R09 门禁契约**：`R09.json:36-97` 已注册 `r09_t01_react_parity` / `r09_t01_release_offline` / `r09_t02_supervisor_ready` / `r09_t02_process_ownership` 四个套件，证据目录 `{EVIDENCE}/SUITES/<suite>/{result.json,output.log}`，逐断言案例走 `r09_producer.py:112-245` 的 `Producer`（`cases.json`，schema `lingxi.leaf-case-results.v1`）。当前 `scripts/rust-tauri/r09_t01.py`、`r09_t02.py` **尚不存在**，按 `r09_suites.py:80-96` 会以 `NOT_IMPLEMENTED` exit 2 真失败——这是“未实现=红”的诚实起点，不是缺陷。

---

## 2. 现状契约表：Electron 行为 → Tauri 最小等价实现

> 列说明：**事实锚点**均指本仓库当前 HEAD（工作区）；**实现要点**是读代码得到的行为，不是推测；**Tauri 最小设计**是 T01/T02 范围内必须等价实现的最小面，标注模块归属。

### 2.1 启动选择、数据根、二进制与构建身份

| # | 行为 | Electron 事实锚点 | 真实实现要点 | Tauri 最小等价设计 |
|---|---|---|---|---|
| C01 | Rust/Node 运行时可切换（默认 node） | `desktop/src/shared/rust-local-service.cjs:186-192`；`desktop/main.cjs:1510-1514` | 仅 env `LINGXI_DESKTOP_SERVER_RUNTIME`，`node` 默认 / `rust` 开启 / 其他值抛错 | Tauri 宿主**恒为 rust service**，不需要该门；为避免与 Electron 语义混淆，Tauri 不读该 env，文档标注“Tauri 不存在 Node 回退路径”（A03 的“不能启动第二旧服务兜底”成为结构性事实） |
| C02 | `LINGXI_HOME` 解析与导出 | `shared/hana-runtime-paths.cjs:4-17`；`desktop/main.cjs:176-177` | env 覆盖，缺省 `~/.lingxi`，`path.resolve` 后回写 `process.env.LINGXI_HOME` | `service/resolve.rs`：`LINGXI_HOME` 或 `$HOME/.lingxi` → `canonicalize`/绝对化 → spawn env 与私有记录读取共用同一值；启动早期即确定，禁止各处重解析 |
| C03 | 服务 CLI 面 | `rust/crates/lingxi-service/src/main.rs:41-222`（USAGE）、`:236-260` | `--home <ABS>` 必需来源之一；`--bind` 默认 `127.0.0.1:0`；`--network-mode loopback|lan`；`--shutdown-timeout-ms` 默认 10000；无 `--port/--token` | spawn `lingxi-service --home <home>`（与 Electron `main.cjs:1437` 完全一致）；不传 port/token；lan 模式不在 T01/T02 宿主默认启用 |
| C04 | 二进制解析（dev vs packaged） | `rust-local-service.cjs:194-215`；`main.cjs:1374-1380` | dev：`LINGXI_SERVICE_BIN`（必须绝对路径）否则 `<repo>/rust/target/debug/lingxi-service`；packaged：`resources/rust-service/`，env 不能替换 | 同名 env 语义：dev 绝对路径（缺省 repo-relative debug 二进制，转换为绝对路径）；packaged：sidecar 路径（`current_exe()` 同级，见 §3.8）+ `build.json`；packaged 下拒绝 env 替换（与 Electron 一致） |
| C05 | 打包构建身份校验（在 spawn 前） | `rust-local-service.cjs:134-184`（字段/魔数/摘要/签名允许变化），`:43-117`（Mach-O/PE 内容摘要） | 校验 `build.json` schemaVersion=1、platform/arch/target/toolchain/appVersion/binary/sha256/contentSha256/sourceSha256；mac 签名后 sha 变化时要求 contentSha 相等 + `codesign --verify --strict` + `--deep` app bundle 验签；win 允许 PE 校验和/证书表变化并要求同签名者 | `service/verify.rs` 移植同一套（**含签名后内容摘要**；否则 ad-hoc 签名后 sha 必变导致每次拒绝）。新增 `protocol` 字段后在此门禁。失败=启动前明确拒绝，记录期望/实际 |
| C06 | 构建身份生成（打包期） | `scripts/build-rust-desktop-service.mjs:16-21,71-73,90-155`；`package.json:52-53` | 锁定 rustup/工具链、`rustSourceDigest`、`appVersion=package.json version`、`sha256`/`contentSha256`、stage 目录 `dist-rust-service/<mac|win|linux>-<arch>/` | 复用该脚本产物；T01/T02 增加 `protocol` 字段（读 `lingxi-service --version` 输出或 workspace 常量），并新增 Tauri staging 脚本把同一 stage 转成 `binaries/lingxi-service-<triple>` + `resources/rust-service/build.json`（§3.8） |
| C07 | 旧 Node server 记录互斥 | `main.cjs:1349-1353` | 存在 `{home}/server-info.json` 时抛 `RUST_DESKTOP_NODE_SERVER_INFO_PRESENT`，不删他进程线索 | 同语义移植：启动前检查同一 home 的 `server-info.json` → 明确拒绝；Tauri 无 Node 路径，仍拒绝以免误接旧内核 |
| C08 | 禁止数据降级 | `main.cjs:1346-1348` | `LINGXI_ALLOW_DATA_DOWNGRADE=1` 时 Rust 桌面拒绝启动 | 同：拒绝并给出“use explicit recovery workflow”文案 |
| C09 | packaged 下渲染内容版本必须等于壳版本 | `main.cjs:1381-1389` | 走 artifact-boot 激活 renderer，并要求 `_currentContentVersion === app.getVersion()`，否则拒绝 | T01/T02：renderer 以构建期内嵌（无 artifact-boot）。等价门禁改为**构建期绑定**：`frontendDist` 树摘要 + app version 记录进 `build.json`/发布元数据；运行时校验内嵌资源版本标记（由构建脚本生成 `frontend-build.json` 或编译期常量）。若后续 T06/T08 引入 OTA，再补运行时门（R09 后续任务，非 T01/T02） |
| C10 | 现有实例复用策略 | `main.cjs:1390-1405`；`rust-local-service.cjs:298-317` | 读 `instance.json`；pid 死→忽略；pid 活→读 token+identity 验证；dev 允许 `local-reused`（不拥有）；packaged 拒绝 `RUST_DESKTOP_EXISTING_INSTANCE_UNVERIFIED` | 同语义：状态机区分 `LocalOwned | LocalReused | Remote`；packaged 拒绝复用；reused 记 pid 但 `owned=false`（退出零接触） |
| C11 | spawn 参数/环境/脱离 | `main.cjs:1437-1443`、`:1406-1436` | `spawn(binary, ['--home', home], {cwd: dirname(binary), env:{...process.env, LINGXI_HOME, ...bridgeRuntimeEnv}, detached: platform!=='win32', stdio:['ignore','pipe','pipe']})`；unref 后脱离事件循环 | Rust：`Command` 设 `--home`/env/cwd；Unix `process_group(0)`/`setsid` 使自有树可整体收敛；stdout/stderr piped 并有界缓存（崩溃诊断）。bridgeRuntimeEnv 属 Electron artifact 路径，T01/T02 不迁移（显式记录为 GAP，T03/T05 处理） |

### 2.2 就绪握手、身份校验、发布给渲染层

| # | 行为 | Electron 事实锚点 | 真实实现要点 | Tauri 最小等价设计 |
|---|---|---|---|---|
| C12 | READY 行握手（禁止固定 sleep） | `main.cjs:1447-1486`（regex `:1471`，stdout 上限 `:1465`，超时 `:1477`）；服务侧输出 `rust/crates/lingxi-service/src/main.rs:1149-1160` | 逐行解析 stdout `^LINGXI_SERVICE_READY addr=(\S+) home=`；只认 stdout；缓冲上限 64KiB；超时 `SERVER_INFO_FIRST_WAIT_MS=90_000`（`desktop/src/shared/server-readiness.cjs:34`）；child 提前 exit → 带 code/signal 抛错 | `service/supervisor.rs`：async 读 stdout（`BufReader::lines`），同一 regex；`tokio::time::timeout(90s)`；提前 exit/超时 → 失败态 + 杀掉已 spawn 子进程；**证据记录 spawn→READY 时间戳**（A03 证明不是 sleep） |
| C13 | owned instance/token 私读 | `rust-local-service.cjs:217-266`（O_NOFOLLOW/owner-only/Win 受信读器）、`:279-296` | 读 `{home}/lingxi-service/instance.json`（非 secret）与 `local-token.json`（owner-only 0600、uid 本进程）；校验 `record.serverKind==='lingxi-service'`、`record.pid===child.pid`、`homePath===realpath(home)`、instanceId 32hex、token 文件 kind/instanceId 匹配、`token` 32hex、`bindAddr===READY addr` | `service/instance.rs`：Rust 读取同一布局（字段镜像 `instance.rs:157-177`，owner-only 校验 `auth.rs:76,2110-2133`）；Windows 走服务自身 `--read-private-runtime-json`（`main.rs:240-260`）——T01/T02 在 macOS 实现，Windows 分支登记为平台阻塞 |
| C14 | identity 认证校验 | `main.cjs:1310-1334`；服务路由 `rust/crates/lingxi-service/src/management.rs:647,1240-1275` | `GET {baseUrl}/lingxi/v1/server/identity`，Bearer token，5s/请求，仅对 `ECONNREFUSED/ECONNRESET` 重试 20×100ms；要求 `serverNodeKind==='lingxi-service'`、`serverNodeTransport===connection.transport`、非空 `serverId`、非空 `studioId` | `service/client.rs`：同 URL/header/超时/重试/字段断言；另加 T02 版本门（C24）：instance.json 的 `wireProtocolMin/Max`、`data_epoch` 与本机 `lingxi-protocol` 常量相等，否则 RUNTIME_IDENTITY_MISMATCH（A03 负向） |
| C15 | 发布连接给渲染层 | `main.cjs:1336-1343,5646-5655`；渲染侧 `desktop/src/react/app-init.ts:177-206` | 全局 `serverPort/serverToken/serverNodeKind/serverNodeTransport/serverHost`；IPC `get-server-connection-info` 返回 `{port,token,serverNodeKind,serverNodeTransport,host}`；`server-restarted` 事件同形（`main.cjs:2324-2328`） | Tauri state 持有同一结构；commands `get_server_port`/`get_server_token`/`get_server_connection_info` 返回完全相同字段（host 本地为 null）；事件 `server-restarted` 用 `app.emit` 同 payload；渲染侧 app-init 无需改动（这就是 T01 的关键复用点） |
| C16 | 连接记录被外部改写/残留 | `rust-local-service.cjs:298-317`（pid/记录校验）、`instance.rs:19`（记录不是存活权威） | 服务以 OS 文件锁为唯一存活权威；记录仅诊断/身份；宿主不能凭记录杀进程 | 同原则：宿主只读记录做定位；绝不删除/写 `instance.lock`/`instance.json`；pid 复用由“记录 pid + token 身份 HTTP 校验”双重排除（A04 负向 case） |

### 2.3 崩溃、退出、远端与诊断

| # | 行为 | Electron 事实锚点 | 真实实现要点 | Tauri 最小等价设计 |
|---|---|---|---|---|
| C17 | 崩溃监控与有限重试 | `main.cjs:2298-2349` | 非主动停止时 `exit` → 自动重启**最多 1 次**（`:2315`）；成功→重挂监控 + 向 main/settings 广播 `server-restarted`；再失败→写 crash log + 错误对话框（含 version/reason） | `service/supervisor.rs`：watch child exit → 若非 quitting/非 intentional → `restart_attempts<1` 则重启并 emit 事件；否则写失败记录 + 原生 dialog；**禁止无限重启** |
| C18 | owned 退出清理 | `main.cjs:6854-6940`（`SERVER_SHUTDOWN_GRACE_MS=17_000`:725，`SERVER_FORCE_KILL_WAIT_MS=5_000`:726，poll=200:728） | SIGTERM → 等 17s → SIGKILL → 等 5s；未确认退出则保留线索并报告 PID（不假称清理完成） | `service/shutdown.rs`：Unix `libc::kill(pid,SIGTERM)`（服务自身信号处理见 `main.rs:17-23,1300-1424`，graceful 预算默认 10s）→ 17s 等待 → SIGKILL → 5s；写结构化退出报告 `{ownedPids,residual,graceMs,timeout}`；残留 pid 显式上报 |
| C19 | reused/远端退出零接触 | `main.cjs:6900-6930`；`docs/rust-tauri/R08/T03_FREEZE_MATRIX.md:17,73-74,84` | reused 且非 owned → “detached from external server”，返回 `{confirmed:false}`；remote 分支结构性 `serverProcess=null; reusedServerPid=null` → before-quit `allow` | 状态机在 remote/reused 下**不持有 Child 句柄**，退出路径无杀进程代码（结构性零接触，不靠分支纪律）；退出报告写 `mode=remote|reused, touchedRemote=false` |
| C20 | 退出触发面与重入保护 | `main.cjs:6942-6988`（before-quit 状态机）、`:6966-6975` | `isQuitting` 置位 → 隐藏窗口 → 计算 `hasActiveOwnedServer` → `resolveBeforeQuitServerAction`：allow/wait/run；run 后第二次 quit 放行；`_isUpdating` 时跳过 | Tauri：`Builder::build().run(|app,event| RunEvent::ExitRequested{api,..})`；自装 SIGINT/SIGTERM/SIGHUP handler（tokio signal）→ `app.quit()`（无 handler 时 SIGTERM 直接终止，绕过清理，A04 会红）；`prevent_exit` + async shutdown + 再次 `app.exit()`；重入用 `Mutex<QuitState>` |
| C21 | remote 模式形状与红线 | `rust-local-service.cjs:319-375`；`main.cjs:1354-1373` | `LINGXI_DESKTOP_RUST_REMOTE_URL`（http(s)、显式端口、禁 userinfo/path/query/hash）+ 绝对路径 `..._TOKEN_FILE`（`{"token":"^[0-9a-f]{32}$"}`，owner-only）；非 loopback 明文 http 需 `..._ALLOW_CLEARTEXT=1`；`REMOTE_URL` 需 `LINGXI_DESKTOP_SERVER_RUNTIME=rust`（矛盾即抛）；**packaged 拒绝 remote**（`:1359-1361`）；远端不读本地 instance pid、不 spawn | Tauri 复用同名 env 契约（便于 R08 夹具平移）；`app.is_dev()`/`cfg!(debug_assertions)` 下才允许 remote；packaged → 明确拒绝 `REMOTE_REQUIRES_DEV_BUILD`；远端 token 文件 owner-only 校验；退出零接触（C19）。A04 因此使用**debug/dev 构建**验证远端列，成文记录“远端=dev 预览”边界（与 R08-T03 一致） |
| C22 | 诊断/崩溃日志 | `desktop/main.cjs:444,2425-2480,2624`；`desktop/src/shared/desktop-launch-diagnostics.cjs:1-50` | `{LINGXI_HOME}/diagnostics/desktop-launch/renderer.log` JSONL + `{LINGXI_HOME}/crash.log`；诊断含 HOME、runtime、connection mode、spawn 记录/exit | `diagnostics.rs`：`{LINGXI_HOME}/diagnostics/tauri-launch/events.jsonl`（事件：resolve/spawn/ready/identity/reject/shutdown/page-load）+ `{LINGXI_HOME}/crash.log`（共用文件名会与 Electron 安装并存时冲突→建议 `crash.log` 兼容读取、写入 `tauri-crash.log`，Q-09）。A02/A03 证据直接消费该 JSONL |
| C23 | Node 残留 server-info / 外来同宅探测 | `main.cjs:1516-1604`；`desktop/src/shared/stale-server-info.cjs:14-19`；`shared/server-info-probe.cjs:82-139` | 仅 Node 路径：PID 存活+token 探测（alive-same-home/unauthorized 阻塞；not-hana/dead 清理）；Rust 路径完全不读 server-info 的复用逻辑，只做 C07 互斥拒绝 | Tauri 无 Node 路径 → 只实现 C07 检查；`stale-server-info.cjs`/`server-info-probe.cjs` **不移植**（避免把 Node 的 PID 复用启发式带进 Rust 服务锁语义）；在证据中记录“不适用”理由 |
| C24 | 协议/数据版本三轴与健康面 | 服务：`lib.rs:536-543,3646-3657,8251`；公开 health `auth.rs:752`；identity `management.rs:1240-1275`；`handshake.rs:21-23,99-112`；`lingxi-protocol/src/lib.rs:408-438`；Node 轴 `shared/contract-versions.json` + `main.cjs:1095` | `/lingxi/v1/health` 公开，返回 `{status,serverKind,serverVersion,wireProtocolMin,wireProtocolMax,dataEpoch}`；identity 认证后返回 kind/transport/serverId/studioId/version；instance.json 另带 wire/epoch。`serverVersion()` 仍是 **0.0.0 占位**（R02 handoff 已知缺口） | T02 三层门：①构建期 `build.json.protocol` == 本机编译常量（`lingxi-protocol`，tauri-host 允许依赖，`DEPENDENCY_RULES.json:180-190`）；②spawn 前 manifest 校验；③READY 后 instance/health/identity 校验。**不用** `shared/contract-versions.json` 做门；`serverVersion` 不参与门（0.0.0 无鉴别力）；`appVersion`（package.json 版本）走构建身份 |
| C25 | 服务单写者/双启动 | `main.rs:6-23`（exit 3 + `LINGXI_SERVICE_SINGLE_WRITER_BLOCKED`）、`instance.rs:432+` | 同一 home 第二实例拒启 exit 3；陈旧记录自动归档 `instance.stale.json` 并接管；锁是存活权威 | 宿主不自行做单写者，交给服务；宿主解析 stderr marker → 错误对话框/记录（含 home/recordedPid/instanceId/addr）；**不删除记录、不强杀** |
| C26 | Windows guardian（仅 Node 路径） | `main.cjs:6858,6870,6878`（`serverNodeKind!=='lingxi-service'` 才用 guardian）；`windows-server-guardian.cjs` 引用 `main.cjs:97-103` | Rust owned 路径在 Windows 上也只 `child.kill()`，没有 Job 对象 guardian | Tauri Windows：应使用 Job Object（KILL_ON_JOB_CLOSE）保证树收敛；本机无 Windows（BLOCKED-PLATFORM-WINDOWS-X64），设计登记为 T02 未实现/未验证分支 |
| C27 | R06 宿主可复用资产 | `desktop/src-tauri/tauri.conf.json:1-15`、`Cargo.toml:14-33`、`build.rs:2-3`、`lib.rs:530-596`、`tests/desktop_host.rs:137-160`；`desktop/src-tauri/README.md:23-26` | 独立 manifest/锁树；tauri 2.12.1/tauri-build 2.7.2；`withGlobalTauri:true`；命令在 build.rs `AppManifest::commands` 注册；`e2e-test` feature 注入 wdio WebDriver（默认可选不编译）；E2E = spawn `CARGO_BIN_EXE_...` → 连 `http://127.0.0.1:$TAURI_WEBDRIVER_PORT/session` | T01/T02 直接扩展该工程：新增命令必须同时加 `build.rs` 命令表与 `capabilities/*.json` allow 项；E2E 沿用 wdio WebDriver；保留 base config 使 R06 测试仍可复跑（§3.3） |
| C28 | 渲染层契约（T01 的“零改业务组件”边界） | `desktop/src/react/types.ts:547-701`（PlatformApi 109+ 成员）、`app-init.ts:110-247`、`desktop/src/react/host/desktop-host.ts:18,85-107` | 业务只经 `getDesktopHost()/usePlatform()` + ServiceClient；宿主桥成员多数 optional；boot 只强依赖 `getServerConnectionInfo|getServerPort|getServerToken` + `appReady` + `onServerRestarted` | T01 的 DesktopHost-Tauri 先实现 **boot 基线**（§3.4），其余成员登记 `capabilityReason` + `HostCapabilityError`（禁止静默 no-op）；T03 再逐项补齐并在 A05 验收 |

---

## 3. R09-T01 前端接线设计（同一 React 产物 + 薄 Tauri 宿主）

### 3.1 产物加载：`desktop/dist-renderer` 是唯一产品产物

| 事实 | 锚点 | 结论 |
|---|---|---|
| 产品 renderer 构建 | `vite.config.ts:299`（`base:'./'`）、`:339`（`outDir:'../dist-renderer'`）、`:342-355`（index/settings/quick-chat/onboarding/browser-viewer/viewer-window 六入口 + mobile；splash 独立） | `desktop/dist-renderer` 是 T01 的目标产物；当前 453 assets / 84MB |
| 旧文件复制插件 | `vite.config.ts:267-292`（lib/modules/themes/assets/locales + styles/icon 等） | 产物包含 `modules/connection-csp.js`、`modules/platform.js` 等非模块脚本，Tauri 必须按同目录结构加载（`base:'./'` 相对路径天然兼容自定义协议） |
| index 入口 | `desktop/src/index.html:6,14-17` | 先 `connection-csp.js` → i18n/theme/platform → `main.tsx`；无内联业务脚本（利于 Tauri CSP） |
| Tauri 嵌入方式 | Tauri v2 `build.frontendDist` 指向目录 → 资产编译进 app 二进制（无运行时目录依赖） | release overlay 设 `frontendDist:"../dist-renderer"`；宿主无需解析外部目录，A02 的“无 dev server”是结构性的 |
| R06 资源 UI（**不可复用**） | `desktop/vite.tauri-resources.config.ts:5-11`（root=desktop/src/tauri-resources，outDir=desktop/dist-tauri-resources，1.7MB）；`tauri.conf.json:6` 指向它 | 这是 R06 的 `data-testid=resource-desktop` 文件页；与产品 React 无关。若误用，A01 直接失败（两壳产物不同） |

**同一产物的证据锚**：R08-A16 基线 `rendererDist=3e5339cc…`（`artifacts/rust-tauri/R08/verify-r08-closeout-08/SUITES/r08_a16_versions/a16-versions.json`）已被本次复算证实；T01 实现后重建的新摘要必须写入 A01 证据并在两侧（Electron `loadFile(dist-renderer/index.html)` / Tauri 内嵌 dist）共同绑定。

### 3.2 生产不依赖开发服务器的技术手段（A02 设计要点）

1. **构建期固定**：release overlay 不含 `devUrl`；`frontendDist` 是唯一页面来源；base config（R06）也没有 `devUrl`。Tauri `tauri build` 只嵌入 `frontendDist`。
2. **运行期无读取点**：Tauri 宿主不读 `VITE_DEV_URL`（Electron 的 dev 分支在 `main.cjs:543,615-631`、`scripts/launch.js:27`；Tauri 没有等价 env）。证据中故意注入 `VITE_DEV_URL=http://127.0.0.1:5173` 并证明无效。
3. **页面的真实来源可观测**：宿主在 `webview` `on_page_load` 记录 URL 到诊断 events.jsonl（A02 断言为 `tauri://localhost/index.html`，不是 `http://localhost:5173`）。
4. **网络探针**：5173 上放 canary listener（记录连接数=0）+ `lsof -a -p <app-pid> -iTCP`（无 5173）+ 发布二进制 `strings` 扫描 `localhost:5173`/`devUrl`（应 0，R01 的“release 无测试插件 strings 探测”先例 `TAURI_SPIKE_REPORT.md:84-85`）。
5. **release 构建不含 WebDriver 测试插件**：与 R06/R01 一致用 `e2e-test` feature 门控（`desktop/src-tauri/Cargo.toml:27,33`；`lib.rs:594-595`）；A02 使用**不带 feature** 的 release 二进制，A01 的 DOM 对照才用带 feature 的证据构建（两者都记录 digest/命令）。

### 3.3 dev / release 配置分离（保留 R06 base，新增 overlay）

| 文件 | 建议内容 | 理由 |
|---|---|---|
| `desktop/src-tauri/tauri.conf.json`（**保持原样**） | R06 resource host：`frontendDist=../dist-tauri-resources`、`productName="Lingxi Resources"`、`bundle.active=false` | `cargo test --test desktop_host` 用编译期 base config（`env!("CARGO_BIN_EXE_lingxi-resource-desktop")` 来自该 config 嵌入）；改 base 会破坏 R06 证据可复跑性 |
| `desktop/src-tauri/tauri.dev.conf.json`（新） | `build.devUrl="http://localhost:5173"`、`build.beforeDevCommand=null`（由 wrapper 起 Vite）、`app.windows` 覆盖为 R09 main 窗（label main、url index.html、visible:false）、dev CSP 追加 `ws://localhost:5173 http://localhost:5173` | dev 只读 Vite；不进入 release；限定 dev 来源（仅 localhost:5173） |
| `desktop/src-tauri/tauri.release.conf.json`（新） | `build.frontendDist="../dist-renderer"`、`productName="Lingxi"`、`identifier="com.lingxi.app"`（与 Electron appId 对齐；Q-06 待确认并行安装影响）、`bundle.active=true,targets=["app"]`、`externalBin`、`resources`、`app.windows`（main, 宽度/高度对齐 Electron main，终值 T03 定）、release CSP（§3.6）、`bundle.macOS.signingIdentity` 不设（ad-hoc；Q-02） | release 与 dev 彻底分离；`version` 由 wrapper 从 `package.json:3` 注入（避免 config 手工漂移） |
| `desktop/src-tauri/.tauri-build/`（生成物，gitignore） | wrapper 生成的最终 overlay（含 version、stage 绝对路径、target triple） | 避免模板里写死平台路径 |

**必须实测的合并语义（Q-05）**：`tauri build --config <overlay>` 以 base 为底、overlay 覆盖；数组字段（`app.windows`、`bundle.externalBin`、`bundle.resources`）是否整体替换、还是深度合并，直接决定 overlay 是否要写全量。探针：构建后读取宿主 `get_build_info`/诊断里的 config 摘要 + `strings <binary>` 查 `dist-renderer`/`dist-tauri-resources` 与 `5173` 是否出现；并用 `cargo test --test desktop_host` 证明 base config 路径未变。

### 3.4 DesktopHost-Tauri：注册位置、解析规则、T01 基线成员

**注册位置（现有三实现之外新增第四个）**

```
desktop/src/react/host/
  desktop-host.ts          # 注册表/解析（修改：kind 增 'tauri'，解析顺序插入 __TAURI__ 分支）
  electron-desktop-host.ts # 现有：唯一直读 window.hana 的文件
  web-desktop-host.ts      # 现有：受限宿主 + capabilityReason
  host-capability-error.ts # 现有：显式受限错误
  tauri-desktop-host.ts    # 新增：读 window.__TAURI__（withGlobalTauri 全局），invoke 适配
```

**解析规则改动**（`desktop-host.ts:85-107` 附近）：现状为 `hana → platform 别名 → web`；改为 `hana → __TAURI__ → platform 别名 → web`，并把 `window.__TAURI__` 纳入桥身份快照（`lastHana/lastPlatform` 同款机制，`desktop-host.ts:27-33,46-54`），避免测试逐用例替换全局时命中旧缓存。`DesktopHostKind`（`:18`）扩展 `'tauri'`；`explainHostCapability`（`:113-120`）的“Electron 全能力例外”保持不动——Tauri 缺成员时必须走 capabilityReason/`HostCapabilityError`，不得冒充全能力。

**T01 基线成员（只实现 boot 与核心页所需，其余显式受限）**

| 成员 | T01 实现 | 命令/事件 |
|---|---|---|
| `getServerConnectionInfo` | ✅ | `invoke("get_server_connection_info")`（返回 `{port,token,serverNodeKind,serverNodeTransport,host}`） |
| `getServerPort` / `getServerToken` | ✅（非 optional，boot 回退路径需要） | `get_server_port` / `get_server_token` |
| `appReady` | ✅（显示主窗口；splash 关闭逻辑 T03） | `app_ready` |
| `getPlatform` | ✅（`platform.js` 早于 React 需要 → 见 §3.5；React 侧异步版） | `get_platform` |
| `onServerRestarted` | ✅ | `listen("server-restarted")`，返回 unlisten（`app-init.ts:110-174` 消费同形 payload） |
| `settingsChanged/onSettingsChanged` | 最小显式实现或受限登记（主窗核心页可能不触发；T03 补齐） | 事件 `settings-changed` |
| 其余 100+ 成员 | ❌ 不假装实现：`capabilityReason="R09-T03 迁移中"`，调用抛 `HostCapabilityError`（`host-capability-error.ts:5-21`） | — |
| `openSettings`（非 optional，void） | T01 允许显式抛 `HostCapabilityError`（不是空 no-op）；T03 实现窗口 | — |

**测试**：新增 `desktop/src/react/__tests__/host/r09-tauri-desktop-host.test.tsx`（jsdom 注入假 `window.__TAURI__`），断言：解析顺序、kind、payload 透传、受限能力显式理由、`resetDesktopHostForTests` 清缓存；不得要求 jsdom 里真 Tauri。

**类型**：`window.__TAURI__` 的最小 ambient 声明放在 `desktop/src/react/types.ts` 或新 d.ts（仅 `core.invoke`、`event.listen` 等 T01 实际使用的成员，禁止 any 扩散）。

### 3.5 必须同步的宿主探测点（否则 UI/交互会“看起来像 Web”）

| 点 | 现状 | T01 改动 |
|---|---|---|
| `desktop/src/modules/platform.js:13-22` | 只有 `window.hana` 分支；否则 `data-platform="web"` | 增 `window.__TAURI__` 分支：同步设置 `data-platform`（macOS 下 `darwin`，Windows/Linux 可由 UA 或后续 command 异步纠正），保留 Electron 抢先别名与 win32 滚动逻辑逐字不变 |
| `desktop/src/react/stores/session-actions.ts:276-279` | `isDesktopShell()` 只认 `kind==='electron'` | 改为 `kind==='electron' || kind==='tauri'`（或抽 `isNativeDesktopHost()`），否则 Tauri 下输入焦点恢复等桌面行为静默失效（属 A01 核心页面行为差异） |
| `desktop/src/modules/connection-csp.js:1-68` | 运行时 `document.write` 注入 meta CSP，`connect-src` 只含 127.0.0.1/localhost 与持久化远端连接；**没有 `ipc:` / `http://ipc.localhost`** | 若探针确认 Tauri IPC 被该 meta 拦截：additive 追加 `ipc:`/`http://ipc.localhost` 到 connect-src（Electron 无副作用）；保留 `document.write` 行为以最小化风险 |
| `desktop/src/react/types.ts:547-552,610,613` | `getServerConnectionInfo?` 已 optional；`onServerRestarted?` 已 optional；`appReady`/`getServerPort`/`getServerToken` 必填 | 无需改类型（Tauri host 直接满足必填三项 + 可选面） |

### 3.6 CSP / `withGlobalTauri` 决策

- **桥方式推荐**：用现有 `withGlobalTauri: true`（`tauri.conf.json:8`）+ `window.__TAURI__.core.invoke`，**不**新增 `@tauri-apps/api` 依赖。理由：①同一 React 产物同时给 Electron/Tauri，静态 import Tauri API 会把 Tauri 运行时假设打进 Electron 产物；②避免改 package 依赖影响 R08 基线 bundle；③R06/R01 已验证全局桥可用。代价：`__TAURI__` 暴露面更大，T06 必须在能力矩阵/CSP 收口时复核（`withGlobalTauri` 只暴露 **ACL 允许**的命令，不是绕过 ACL；R01 §3 已实测未授权窗口全拒）。
- **release CSP 草案**（在 R06 `tauri.conf.json:11` 基础上扩到产品需求）：`default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' ipc: http://ipc.localhost http://127.0.0.1:* ws://127.0.0.1:*; img-src 'self' data: file: http://127.0.0.1:*; media-src 'self' data: blob: file: http://127.0.0.1:*; font-src 'self' data:; frame-src 'self' blob: data: file: http://127.0.0.1:* http://localhost:*; object-src 'none'`。参考 `vite.csp-profiles.ts:15-18` 的 index profile（Electron 版）逐指令对齐。
- **双 CSP 叠加风险**：Tauri 通过响应头注入 config CSP，页面内 `connection-csp.js` 又写 meta；两者取交集。必须用 WebDriver 探针验证：`invoke` 可用、WS 到 `ws://127.0.0.1:<port>` 可连、无 CSP violation 控制台错误（A01 证据里记录 console 列表）。

### 3.7 构建命令与版本锁定（T01 建议 npm scripts）

当前 `package.json` 已有 `dev:renderer`（`:34`）、`build:renderer`（`:35`）、`build:rust-service`（`:52`）、`verify:rust-service-package`（`:53`），并已装 `@tauri-apps/cli 2.12.4`（`:139`）。建议新增（实现阶段才写）：

```jsonc
"build:tauri:renderer": "npm run build:renderer",                       // 与 Electron 完全同一产物
"build:tauri:service":  "npm run build:rust-service",                   // 同一 dist-rust-service 身份
"stage:tauri:service":  "node scripts/stage-tauri-service.mjs",         // dist-rust-service/<tag>-<arch> → binaries/+resources/，triple 由 rustup rustc -vV host 解析
"build:tauri":          "node scripts/build-tauri-host.mjs",            // 检查 dist 摘要 → stage → 生成 release overlay（version/路径）→ tauri build --bundles app
"build:tauri:e2e":      "node scripts/build-tauri-host.mjs --features e2e-test --out desktop/src-tauri/target/e2e", // 仅 A01 证据构建
"dev:tauri":            "node scripts/dev-tauri.mjs"                    // 起 Vite(5173) → tauri dev --config tauri.dev.conf.json；不进 release
```

约束：
- **CLI 版本固定** `@tauri-apps/cli@2.12.4`（`package.json:139` 已精确锁定；与 `tauri` crate 2.12.1 同 2.12 线，构建前由 wrapper 记录 `tauri --version` + `cargo metadata` 到证据）。
- Rust 命令一律经 `rustup run 1.98.1`（`rust-toolchain.toml`；`r09_producer.py:64-92` 已提供封装），不直接依赖 PATH 的 Homebrew rust。
- build wrapper 必须在同一进程内校验：`desktop/dist-renderer` 存在且树摘要记录、`dist-rust-service/<tag>-<arch>/build.json` 的 appVersion 等于 `package.json:3` 的版本、目标 triple 等于 `rustup run 1.98.1 rustc -vV` 的 host。

### 3.8 安装包内服务二进制/清单布局：候选与推荐

| 方案 | 布局 | 优点 | 缺点/风险 | 结论 |
|---|---|---|---|---|
| **L1（推荐）externalBin sidecar + resources 清单** | Tauri `bundle.externalBin:["binaries/lingxi-service"]`（构建前 stage 成 `binaries/lingxi-service-<triple>`，由 Tauri 装到 `Contents/MacOS/lingxi-service`）；`bundle.resources` 把 `build.json` 放到 `Resources/rust-service/build.json` | R01-T06 已在本机实测“`npx tauri build --bundles app` exit 0 + 打包 .app 内 sidecar 握手成功”且 sidecar 路径解析规则已知（`TAURI_SPIKE_REPORT.md:59-77`）；Tauri 负责把 sidecar 放进签名范围；无需 `tauri-plugin-shell` 前端权限 | 需要 stage 脚本产出 triple 后缀文件；`build.json` 与可执行分处两目录（宿主分别解析）；Windows/Linux 打包未验证（平台阻塞） | **推荐**；T02 需新增 stage 脚本 + externalBin 配置 + 宿主 `current_exe()` 同级解析 |
| L2 resources 目录 | `bundle.resources:{"<stage dir>":"rust-service/"}` → `Resources/rust-service/{lingxi-service,build.json}` | 两文件同目录、路径直观；无需 triple 命名 | 嵌套可执行是否被 Tauri/Apple 正确签名未验证；ad-hoc 签名后签名范围/摘要策略需自行处理；R01 未验证 | 备选；先用 `codesign --verify --deep --strict` 探针确认（Q-02） |
| L3 把服务编进宿主二进制 | 单进程内嵌 service | 无分发问题 | 违反 R01 ADR-001（service 独立进程、host 不嵌业务内核）与 TB-01/02 所有权；工作量大 | 否决 |

**Electron 对照**：Electron 用 `extraResources` 把 `dist-rust-service/${os}-${arch}/` 整套拷到 `Resources/rust-service/`（`package.json:191-199`），并在 mac `binaries` 明示签名 `Contents/Resources/rust-service/lingxi-service`（`package.json` mac 段）。Tauri 若走 L2 需要等价的自定义签名步骤；L1 由 bundler 处理 sidecar 签名。

### 3.9 T01 交付范围与非目标（防范围膨胀）

- 交付：Tauri overlay 工程配置（dev/release）、DesktopHost-Tauri（boot 基线 + 显式受限）、同一 `dist-renderer` 嵌入、service sidecar stage + **最小** supervisor 骨架（真正启动/握手在 T02 完成）、构建 wrapper、A01/A02 证据套件。
- 非目标（T03+）：托盘/快捷键/通知/对话框/剪贴板/深链、settings/quick-chat/onboarding 多窗口、文件/预览能力、权限矩阵、Windows/Linux 打包、签名/公证、renderer OTA。
- T01 验收边界：A01 的“核心页面与服务行为一致”用**主窗口 rust-preview 路径**证明；T03 的完整宿主能力矩阵不在 T01 声称。

---

## 4. R09-T02 服务 supervisor 设计

### 4.1 模块布局（`desktop/src-tauri`，扩展 R06 工程）

```
desktop/src-tauri/src/
  lib.rs                 # run(): manage(ServiceRuntime), invoke_handler(commands), RunEvent 退出钩子
  service/mod.rs         # 对外：start_and_wait / shutdown / status / republish
  service/resolve.rs     # LINGXI_HOME、dev/packaged 二进制、remote env 形状与红线
  service/manifest.rs    # build.json 校验（移植 rust-local-service.cjs:134-184）
  service/digest.rs      # Mach-O/PE 内容摘要（移植 :43-117，签名后 sha 变化的允许逻辑）
  service/instance.rs    # instance.json/local-token.json 私读 + 字段校验（:217-317 语义）
  service/client.rs      # identity/health HTTP（reqwest；超时/重试/字段断言）
  service/supervisor.rs  # 状态机：spawn→READY→identity→publish；崩溃监控；单次重启
  service/shutdown.rs    # SIGTERM/SIGKILL 梯度、残留报告、reused/remote 零接触
  service/remote.rs      # R08-T03 远端规则（token 文件、cleartext 门、dev-only）
  diagnostics.rs         # tauri-launch/events.jsonl + crash log
  commands.rs            # Tauri commands → state/DTO
```

依赖新增（全部复用已锁版本，按 R01 D-11 纪律）：`libc`（Unix 信号/进程组；版本从 `desktop/src-tauri/Cargo.lock` 已存在的传递版本取，若不存在需 Lead 批准新增边）、`chrono` 或 `std::time`（时间戳，优先 std 零新增）、`tauri-plugin-shell` **不需要**（宿主内部 `std::process::Command` + 自解析路径；避免前端 shell 权限面，R01 §3 已确立该边界）。

> **与 Lead 实施规格的对齐**：`docs/rust-tauri/R09/PROGRESS.md:71-78` 计划以 `desktop/src-tauri/src/supervisor.rs` 单文件承载 supervisor。上表的 `service/*` 只是建议拆分，**可在实现时压平为 `supervisor.rs`（+ 少量 helper 文件）而语义/常量/命令/事件不变**；以 Lead 复核后的实施规格为准。

### 4.2 状态机（最小）

```
Idle
 → Resolving       (LINGXI_HOME / dev-or-packaged / remote env / old server-info 检查)
 → VerifyingBinary (packaged: build.json + digest; dev: 普通文件/可执行/绝对路径)
 → Reusing?        (instance.json + pid alive + token + identity)
      └─ dev: LocalReused (publish, owned=false, no Child)
      └─ packaged: Reject RUST_DESKTOP_EXISTING_INSTANCE_UNVERIFIED
 → Remote?         (dev only; token 文件 + cleartext 门 + identity; publish; owned=false)
 → Spawning        (Command --home <home>, piped stdio, unref/detach)
 → WaitingReady    (stdout READY ≤90s; 提前 exit/超时 → Failed)
 → VerifyingIdentity(instance/token/pid/bindAddr + identity HTTP + wire/epoch 门)
 → Ready           (publish DTO; monitor child)
 → Crashed         (emit; attempts<1 → Spawning; else Failed)
 → ShuttingDown    (SIGTERM→17s→SIGKILL→5s; 报告 residual)
 → Stopped
失败态统一：写入 diagnostics + failure record（reason/expected/actual/pids）→ 原生 dialog → 退出码非 0
```

### 4.3 启动时序（T02 最小伪码）

```rust
// setup()（主线程只做状态初始化；服务启动放到 async task）
let runtime = Arc::new(ServiceRuntime::new(...));
app.manage(runtime.clone());
tauri::async_runtime::spawn(async move {
    match runtime.start_and_wait().await {          // 内含 4.2 全链
        Ok(info) => {
            runtime.log("service-ready", &info.redacted());
            create_main_window(&app, /*visible=*/false); // 页面加载后 app_ready 再 show
        }
        Err(err) => {
            runtime.write_failure(&err);             // 先落盘，再弹窗（证据可读）
            show_native_error(&app, &err);           // 自动化可用 osascript 关闭（R01 先例）
            app.exit(2);                             // 明确非零；无任何 Node 回退路径
        }
    }
});
```

> 与 Electron 的差异（显式记录）：Electron 先起 splash、再 `await startServer()`、再建主窗口（`main.cjs:6676-6734`）。T01/T02 无 splash（T03 补），故选择“服务就绪后才建主窗口”；若 T02 评审要求先见窗口，则改为预建隐藏窗 + 宿主 boot-gate command（需同时防“首帧无连接”的 UI 假状态），列为 Q-14 待裁决。

### 4.4 版本/协议门（A03 的核心）

**三层门 + 一个结构事实**：

1. **构建期身份（新增）**：`build.json` 增加
   ```json
   "protocol": { "serverKind": "lingxi-service", "wireProtocolMin": 1, "wireProtocolMax": 1, "dataEpoch": 2 }
   ```
   （`scripts/build-rust-desktop-service.mjs:133-137` 处生成；来源必须是服务自身权威 `lingxi-service --version` 输出的 `wire-protocol a..=b data-epoch c`（`main.rs:224-233`），不得手写常量）。Tauri stage 脚本原样搬运；Node 侧 verifier 对多余字段宽容（`rust-local-service.cjs:150-160` 只校验已知字段，新增不会破坏 Electron）。
2. **spawn 前 manifest 门**：`protocol` 字段与本机编译期常量（`lingxi_protocol::handshake::WIRE_PROTOCOL_*`、`ContractVersions::R08_CUTOVER.data_epoch`）逐项相等；不等 → `RUST_SERVICE_PROTOCOL_INCOMPATIBLE expected=… actual=…`，**不 spawn**。
3. **READY 后运行时门**：`instance.json` 的 `wireProtocolMin/Max`、`data_epoch` 与编译常量相等；`/lingxi/v1/health` 的 `serverKind/status/wire*/dataEpoch` 同值；identity 的 `serverNodeKind==='lingxi-service'`、transport 匹配、`serverId/studioId` 非空。任一不符 → `RUNTIME_IDENTITY_MISMATCH`，SIGTERM→SIGKILL 回收本轮 child，失败退出。
4. **无回退的结构事实**：Tauri release 二进制不含 Node 运行时、不含 `hana-server`/`server/bootstrap` 路径（`strings` 证据），宿主代码不存在“其他服务二进制”分支；A03 证据同时断言进程表无 node/hana、home 下无 `server-info.json`。

**版本门不使用的输入**：`shared/contract-versions.json`（DATA_EPOCH=1，旧 Node 轴，会误拒 Rust epoch 2；见 §1 第 3 条）与 `serverVersion()`（0.0.0 占位，无鉴别力）。

**超时/重试常量（与 Electron 对齐）**：

| 常量 | 值 | Electron 锚点 | Tauri 用法 |
|---|---|---|---|
| READY 首等待 | 90_000 ms | `desktop/src/shared/server-readiness.cjs:34`；`main.cjs:1477` | spawn→READY 总预算；超时杀 child |
| identity 请求超时 | 5_000 ms | `main.cjs:1316` | 每请求 |
| identity 连接拒绝重试 | 20 次 ×100 ms | `main.cjs:1312-1325` | 仅 ECONNREFUSED/ECONNRESET |
| 关停宽限 | 17_000 ms | `main.cjs:725` | SIGTERM→SIGKILL 之间 |
| 强杀确认 | 5_000 ms | `main.cjs:726` | SIGKILL→确认 |
| 轮询粒度 | 200 ms | `main.cjs:728` | 等待循环 |
| 崩溃自动重启 | ≤1 次 | `main.cjs:2315` | monitor |
| 服务自身 graceful 预算 | 10_000 ms 默认 | `rust/crates/lingxi-service/src/main.rs:86-95` | 记录进证据（17s 宽限覆盖它） |

### 4.5 Tauri state / commands / events

**state**（`ServiceRuntime`，`Arc` 托管）：
```rust
struct ServiceRuntime {
  mode: RwLock<ServiceMode>,            // LocalOwned | LocalReused | Remote
  conn: RwLock<Option<ConnectionInfo>>, // port, token(secret), kind, transport, host, instanceId, servicePid
  child: Mutex<Option<OwnedChild>>,     // 仅 LocalOwned 持有
  start: Mutex<StartState>,            // lifecycle + failure record
  diagnostics: DiagnosticsWriter,
}
```

| command | 返回 | 说明 |
|---|---|---|
| `get_server_connection_info` | `{port,token,serverNodeKind,serverNodeTransport,host}` | 与 `main.cjs:5646-5655` 逐字段一致；host 本地 null |
| `get_server_port` / `get_server_token` | `number|null` / `string|null` | 兼容渲染回退路径 |
| `get_service_status` | 脱敏诊断：`{mode,pid,readyAddr,manifest{protocol,appVersion,sha256},lastError}` | T01/T02 证据/A02/A03 读取；**token 永不入状态** |
| `app_ready` | `()` | 显示 main 窗；记录诊断事件 |
| `get_platform` | `string` | `platform.js`/React 用 |

命令注册：`desktop/src-tauri/build.rs:2-3` 的 `AppManifest::commands` 列表追加；`capabilities/main.json` 增 `allow-get-server-connection-info` 等（R06 权限生成先例；最终能力表 T06 收口）。命令实现接收 `WebviewWindow`/`AppHandle` 校验 sender label（`main` 及其后 T03 白名单），token 类命令仅 `main`。

| event | payload | 消费者 |
|---|---|---|
| `server-restarted` | 同 `main.cjs:2324-2328`：`{port,token,serverNodeKind,serverNodeTransport,host}` | `app-init.ts:110-174`（Tauri host `onServerRestarted` 桥接） |
| `server-crashed` | `{reason,willRetry,crashLog}` | T01 先记录；UI 呈现 T03/T07 |
| `settings-changed` | 同 Electron IPC 语义 | T03 补齐 |

### 4.6 崩溃策略

- 非主动停止的 child exit → 记诊断 → 若 `attempts<1`：重启一次，成功后 emit `server-restarted`；失败/第二次崩溃：写 crash log + 原生 dialog（含 host version、reason、log path），不进入循环。
- 主动停止集合：退出中（`QuitState`）、update/apply 期间、shutdown 已发起；用与 Electron 相同的“intentional stop 标记”防竞态（`main.cjs:2305-2311` 的语义）。
- 服务自身单写者冲突（exit 3 / marker）不算崩溃重启：明确报错（C25）。

### 4.7 退出清理矩阵（T02 必须成文+证据）

| 触发 | LocalOwned | LocalReused | Remote |
|---|---|---|---|
| 关窗（T03 托盘前：无托盘） | 应用退出 → 清自有树 | detach，零接触 | 零接触 |
| 彻底退出（Cmd+Q / `app.exit` / SIGTERM 经宿主 handler） | SIGTERM→17s→SIGKILL；未确认→报告 residual | `detached from external server`，不做任何 kill | 结构性零接触（无 Child 句柄） |
| OS 登出（Tauri `ExitRequested`） | 同彻底退出 | 同 | 同 |
| 启动失败在 publish 之前 | 杀掉本轮已 spawn child（SIGTERM→17s→SIGKILL→5s）并确认；未确认→失败信息带 pid | 不适用 | 不适用 |

退出报告（写 `{LINGXI_HOME}/diagnostics/tauri-launch/events.jsonl`）：`{event:"shutdown", mode, ownedPids, signaled, residual:[pid], graceMs, confirmed}`；A04 直接断言。

### 4.8 远端模式规则（T02 “本地/远端模式”）

- **环境契约与 Electron 同形**（便于 R08 夹具平移）：`LINGXI_DESKTOP_RUST_REMOTE_URL` + `LINGXI_DESKTOP_RUST_REMOTE_TOKEN_FILE`（绝对路径，owner-only，`{"token":"^[0-9a-f]{32}$"}`）+ 非 loopback 明文 http 需 `LINGXI_DESKTOP_RUST_REMOTE_ALLOW_CLEARTEXT=1`；URL 形状/端口/禁 userinfo·path·query·hash 全部照搬（`rust-local-service.cjs:328-352`）。
- **dev-only**：`tauri::is_dev()`/`cfg!(debug_assertions)` 为假时拒绝 `REMOTE_REQUIRES_DEV_BUILD`（Electron `:1359-1361` 的等价）。T04/T05/T06 若要正式化远端（TLS、凭据 UI、凭据 scope）另行裁决。
- **远端零接触**：不读本地 `instance.json`/不解析本地二进制/不 spawn/不 kill；退出路径无 Child 句柄（结构性，不靠分支纪律）。
- **渲染层**：`get_server_connection_info` 返回 `host=<真实 hostname>`，`app-init.ts`/`server-connection.ts` 的 R08-T03 host 传播链已存在（`docs/rust-tauri/R08/T03_DESIGN.md:90-120`），Tauri 侧无需改业务；标题由 renderer 依据 `serverNodeKind==='lingxi-service'` 自行设置（`app-init.ts:189-191`），A01 对照时两侧应同为“Lingxi · Rust 预览”。

### 4.9 跨平台差异

| 平台 | T02 状态 |
|---|---|
| macOS arm64 | 本机唯一可验证平台；信号/进程组/codesign 均有路径 |
| macOS x64 | `BLOCKED-PLATFORM-MACOS-X64`；`rustup target list --installed` 当前只有 aarch64 + windows-gnu，无 x86_64-apple-darwin |
| Windows x64 | `BLOCKED-PLATFORM-WINDOWS-X64`；需 Job Object 收敛自有树、Tauri 签名与 `lingxi-service.exe` sidecar 布局、`--read-private-runtime-json` 私读；设计登记，未实现未验证 |
| Linux x64 | `BLOCKED-PLATFORM-LINUX-X64`；AppImage/deb 内 sidecar 路径、WebKitGTK 依赖、无 `codesign` 等价物 |

---

## 5. 证据计划（A01–A04）

### 5.0 与已存在 R09 门禁的对齐（必须先看）

Lead 已落地的门禁契约（工作区未提交）：

| 事实 | 锚点 |
|---|---|
| 四个命令 → 四个套件 | `rust/crates/xtask/src/stage_maps/R09.json:36-97`（argv = `python3 -B scripts/rust-tauri/r09_suites.py --suite <name> --evidence {EVIDENCE}/SUITES/<name>`，timeout 3600s） |
| 任务→套件映射 | `scripts/rust-tauri/r09_stage_inputs.py:44-45`（T01=react_parity+release_offline；T02=supervisor_ready+process_ownership） |
| 场景→命令引用 | `R09.json` scenarios `R09-A01..A04`（A01 引 inputs_check+react_parity+release_offline；A02 引同样两个；A03/A04 引 T02 两个）——命令全绿**仍需逐断言合同**（scenario note 原文） |
| 套件 dispatcher / 未实现=红 | `scripts/rust-tauri/r09_suites.py:24-33,80-96`：按前缀分派到 `r09_t01.py`/`r09_t02.py`；模块缺失 → `result.json status=NOT_IMPLEMENTED` + exit 2 |
| 生产者工具 | `scripts/rust-tauri/r09_producer.py:112-245`：隔离 runtime（`/private/tmp/lingxi-r09-runtime/<suite>-…`）、`LINGXI_HOME/TMPDIR` 隔离、真实 `~/.lingxi` 前后摘要、`run_step`（真实 argv/exit/耗时/log）、`check/require`、`finish`（`cases.json` schema `lingxi.leaf-case-results.v1` + `result.json` schema `lingxi.r09.suite-receipt.v1`） |
| 期望证据文件 | `{EVIDENCE}/SUITES/<suite>/result.json`、`output.log`；Producer 自动再加 `cases.json`。**不要**把大二进制拷进 artifacts（R08 已有 69GB 教训），只放 hash/清单/截图/日志 |

**实现落点（T01/T02 实现阶段，不在本次只读范围）**：新建 `scripts/rust-tauri/r09_t01.py`、`r09_t02.py`，各自 `from r09_producer import Producer` 并导出 `run_suite(suite, evidence)`；本节的 case 名即建议写入 `Producer.check()` 的 ID。

### 5.1 A01｜同一 React 产物可运行（套件 `r09_t01_react_parity`）

**目标**：证明 Electron 与 Tauri 启动的是**同一份 React 构建产物**，核心页面与服务行为一致，未重写第二套组件。

**前置/夹具**：
- 两个隔离 home（Producer.runtime/home 派生两个子目录，各自 `LINGXI_HOME`；服务单写者要求不同 home）。
- Rust 服务：`rust/target/debug/lingxi-service`（当前 sha=b855…，记录进证据）或 `dist-rust-service` release；外部协议用确定性替身（本 A01 不需要模型，只需服务身份/健康面）。
- Electron 侧：`require("electron")` 二进制 + `cwd=ROOT` + `['.', '--remote-debugging-port=<port>']`，env：`LINGXI_HOME=<iso1>`、`LINGXI_DESKTOP_SERVER_RUNTIME=rust`、`LINGXI_SERVICE_BIN=<abs>`，删除 `ELECTRON_RUN_AS_NODE`（R08 A05 drill 已验证此路径，`scripts/rust-tauri/r08_t03_a05_drill.mjs:552-566`）。非打包时 `desktop/bootstrap.cjs:187-195` 直接 `require("./main.cjs")`，不需要 `main.bundle.cjs`；但必须先确保 `desktop/preload.bundle.cjs` 存在（`main.cjs:134-142` 缺失即硬崩，必要时套件先跑 `npm run build:preload`）。
- Tauri 侧：`e2e-test` feature 证据构建（`cargo build/tauri build --features e2e-test`，wdio WebDriver 由 `lib.rs:594-595` 注入），env 同上（`LINGXI_HOME=<iso2>`、`LINGXI_SERVICE_BIN=<abs>`）；启动后连 `http://127.0.0.1:$TAURI_WEBDRIVER_PORT/session`（R06 先例 `tests/desktop_host.rs:137-160`）。

**断言/案例（Producer.check ID 建议）**：

| case | 断言 |
|---|---|
| `a01::renderer-dist-digest-recorded` | 记录 `desktop/dist-renderer` 树摘要；若 T01 改动了 React 文件，记录**改动白名单**（host 注册/platform.js/session-actions/可选 connection-csp + 新 host 文件）并断言 `git diff --name-only` 只含白名单（业务组件 0 改动） |
| `a01::same-artifact-both-shells` | Electron `loadFile` 的目录 = Tauri release overlay `frontendDist` = 同一路径；构建记录中两侧摘要相等 |
| `a01::electron-boots` | CDP `/json` 找到主窗；`document.title==='Lingxi · Rust 预览'`；`#react-root` 已挂载；`window.hana` 存在 |
| `a01::tauri-boots` | WebDriver session 建立；同 title/root；`window.__TAURI__` 存在且 `window.hana` 不存在（宿主识别正确） |
| `a01::dom-snapshot-parity` | 归一化快照相等：`title`、`data-platform`、`#react-root` 顶层子节点路径、当前状态文案（rust 连接下 `app-init.ts:238-247` 的 rustCoreUnavailable 状态）、可见主要区块标记；快照写 `{EVIDENCE}/.../dom-{electron,tauri}.json` |
| `a01::service-identity-parity` | 两侧 `getServerConnectionInfo`（Electron 走 preload IPC；Tauri 走 `invoke`）字段结构一致；`serverNodeKind==='lingxi-service'`；各自服务 `/lingxi/v1/health` 的 `{status,serverKind,wireProtocolMin=1,wireProtocolMax=1,dataEpoch=2}` 相等（port/pid 归一化） |
| `a01::no-second-react-entry` | `desktop/dist-tauri-resources` 摘要不被两侧任一加载（Electron 启动日志/CDP location、Tauri 页面 URL 均指向 dist-renderer 产物）；`dist-renderer` 中不存在 tauri-resources 的 `data-testid=resource-desktop` |
| `a01::console-clean` | 两侧 console error 列表为空/同一集合（CSP/资源 404 会在这里暴露） |
| `a01::isolated-homes` | 两 home 各 1 个 `lingxi-service`、无 `server-info.json`、真实 `~/.lingxi` 摘要前后一致（Producer 内置） |

**证据路径**：`{EVIDENCE}/SUITES/r09_t01_react_parity/{result.json,output.log,cases.json,dom-electron.json,dom-tauri.json,renderer-digest.json,console-*.json}`。
**必须真机构建**：Electron 侧以 `electron .` 直接跑源码 `desktop/main.cjs`（`bootstrap.cjs:187-195`；`preload.bundle.cjs` 需在位，`main.cjs:134-142`），无需打包构建；Tauri **必须真构建**（e2e feature + release overlay，产物是 A01 的被测二进制）；Rust 服务可用已存在 debug 产物（记录 sha），release stage 可选。

### 5.2 A02｜生产不依赖开发服务器（套件 `r09_t01_release_offline`）

**前置**：确认无 Vite 进程、5173 无监听（无则起 canary listener 记录连接数）；release 构建（**不带 e2e-test feature**）：`python3 -B scripts/... → node scripts/build-tauri-host.mjs`（内部 `tauri build --config tauri.release.conf.json --bundles app`）；把 `.app` 拷到 **realpath** 目录再运行（R01 实测：`/tmp` 符号链接会让 Tauri `current_exe()` 拒绝，`TAURI_SPIKE_REPORT.md:97-102`）。

**断言/案例**：

| case | 断言 |
|---|---|
| `a02::release-binary-built` | `Contents/MacOS/Lingxi` 存在；记录 sha256、`tauri --version`、overlay 摘要；`codesign -dv` 记录 ad-hoc 身份（不声称正式签名） |
| `a02::no-devurl-in-release-binary` | `strings` 扫描 `<binary>`：`devUrl`=0、`localhost:5173`=0、`127.0.0.1:5173`=0；可选扫 `dist-tauri-resources`=0（防止 overlay 合并错） |
| `a02::no-vite` | 启动前 `lsof -nP -iTCP:5173 -sTCP:LISTEN` 为空；canary listener 在 5173 记录连接数=0（整个启动窗口） |
| `a02::webview-loads-embedded` | 宿主诊断 `page-load` 事件 URL 为 `tauri://localhost/index.html`（macOS WKWebView）或 Tauri 自定义协议；非 `http(s)://localhost:5173` |
| `a02::ui-boots` | 诊断收到 `app_ready`（React `appReady()` 走到宿主）→ 主窗口可见；`service-ready` 事件存在 |
| `a02::env-cannot-redirect` | 注入 `VITE_DEV_URL=http://127.0.0.1:5173` 后重启，页面来源仍为内嵌；证明 release 无 dev 入口 |
| `a02::no-5173-sockets` | `lsof -a -p <pid> -iTCP`（含 WebKit helper）无 5173 连接；记录快照 |
| `a02::graceful-exit` | 退出码 0；`shutdown` 事件 confirmed=true；无残留 `Lingxi`/helper/`lingxi-service` |
| `a02::isolated-homes` | 真实 `~/.lingxi` 未被触达 |

**证据路径**：`{EVIDENCE}/SUITES/r09_t01_release_offline/{result.json,output.log,cases.json,strings-scan.txt,lsof-before.txt,lsof-after.txt,canary.log,diagnostics.jsonl}`。
**必须真机构建**：release `.app`（`tauri build --bundles app`）；本机 CLT-only 可行性依据 R01 事实，但 2.12.4 CLI/2.12.1 crate 组合需本次重证（§6 Q-01）。DMG/正式安装器不在 A02 声称范围（签名阻塞）。

### 5.3 A03｜错误二进制明确拒绝 + 不兜底（套件 `r09_t02_supervisor_ready`）

**准备**：真实构建 release app（带 sidecar+build.json）；负向包通过 **staging 期注入后重新构建**（见下表与下方说明，保证 bundle 自身签名一致）；另备“确定性替身服务”（Rust/Python 任意可执行，行为由夹具控制：打印指定 READY 行、写指定 instance.json、对 `/lingxi/v1/server/identity` 返回指定 JSON）。

**断言/案例**：

| case | 场景 | 断言 |
|---|---|---|
| `a03::manifest-protocol-match` | 正确包 | manifest.protocol 与本机常量相等 → 启动成功；记录 `service-ready` 时间戳（spawn→ready >0，且 READY 之前 instance.json 不存在） |
| `a03::no-fixed-sleep-handshake` | 正确包 | 事件序列严格 `spawn → READY(addr) → instance/token 出现 → identity 200 → published`；诊断里 READY 时间戳早于 published，且失败包不会“等待后照样成功” |
| `a03::manifest-protocol-mismatch-rejected` | **stage 期注入**：构建前把 `resources/rust-service/build.json` 的 `protocol.wireProtocolMax=2` 写入 staging 目录再 `tauri build`（bundle 签名覆盖注入后清单） | 启动**在 spawn 前**失败：failure record `reason=RUST_SERVICE_PROTOCOL_INCOMPATIBLE`、expected=1..=1/epoch2、actual=…；进程表无 `lingxi-service`；退出码非 0 |
| `a03::manifest-digest-mismatch-rejected` | **stage 期注入**：stage 与 `build.json.sha256` 不符的 sidecar 后构建（签名后 sha 变化的正常情形另由 contentSha 校验覆盖） | `reason=RUST_SERVICE_DIGEST_MISMATCH`；不 spawn；错误记录含两侧 hash 前缀 |
| `a03::runtime-identity-mismatch-rejected` | 用替身服务：READY 格式正确，但 instance.json `wireProtocolMax=3`（或 identity 返回 `serverNodeKind:'node'`） | READY 之后被拒：`RUNTIME_IDENTITY_MISMATCH`；宿主 SIGTERM/SIGKILL 回收本轮 child（替身 pid 不再存活）；失败记录含实际字段值 |
| `a03::no-node-fallback` | 上述任一失败 | 进程表无 `hana-server`/`node … server/bootstrap.js`/`bundle/index.js`；home 下无 `server-info.json`；release 二进制 strings 无 Node 服务入口路径；failure record `fallbackAttempts=0` |
| `a03::error-surfaced` | 任一失败 | `tauri-launch/events.jsonl` 有 `service-launch-failed` + crash log；原生 dialog 出现（osascript 关闭并截图/记录 window 标题；R01 dialog dismiss 先例） |
| `a03::timeout-is-bounded` | （Rust 单元测试，非 E2E） | `supervisor` 的超时路径用注入 `Duration` 测试：READY 超时→失败→kill 已 spawn child；不跑 90s 真实等待 |

**证据路径**：`{EVIDENCE}/SUITES/r09_t02_supervisor_ready/{result.json,output.log,cases.json,injected/*.json,process-tree-*.txt,failure-record-*.json,dialog-*.png}`。
**必须真机构建**：release `.app`（真构建）；负向注入优先在 `tauri build` 之前的 staging 目录完成（bundle 签名覆盖注入后内容，避免“改动已签名 bundle 被 OS 拒启”这一混杂变量），post-build 篡改只作可选第二形态并记录 `codesign --verify` 失效状态，不作为主证据；所有注入只发生在 `/private/tmp` 副本（**不碰**工作区构建产物）；替身服务为夹具二进制（可由 `rustc` 一次编译或 Python 脚本，不进产品路径）。

### 5.4 A04｜退出清理只影响自有进程（套件 `r09_t02_process_ownership`）

**夹具**：
1. 远端服务：harness 直接起 `lingxi-service --home <remoteHome>`（loopback + 临时端口；R08-T03 的 LAN 条款不复活），记录 pid/README addr/token、`instance.json` 摘要；属于 harness，不允许客户端杀。
2. 本地客户端：Tauri 构建（release 或 debug）+ `LINGXI_HOME=<localHome>`；宿主 spawn 自有服务。
3. 进程树记录：复用/扩展 `scripts/rust-tauri/r08_t03_process_tree.mjs`（分类 `rust-kernel`：`:77,79-86`），新增 `tauri-host` 分类（comm=`Lingxi` 或构建产物名）；快照 S1（ready 后）、S2（退出中）、S3（退出后）。

**断言/案例**：

| case | 断言 |
|---|---|
| `a04::two-services-coexist` | S1：恰好 2 个 `lingxi-service`（remoteHome/localHome 各 1）；远端 ppid 非宿主；本地服务在宿主子树内 |
| `a04::graceful-trigger` | 触发**彻底退出**：向宿主发 SIGTERM（设计要求宿主安装信号 handler → `app.quit()` → graceful shutdown）；若实现未达，则记录 fallback AppleScript Cmd+Q 路径（R08 A05 先例）且断言同一退出语义 |
| `a04::owned-tree-cleaned` | S3：local service pid 消失；其 worker（如有）消失；宿主进程 exit 0；`shutdown` 事件 `{mode:"local-owned",ownedPids:[pid],residual:[]}` |
| `a04::remote-untouched` | 远端 pid 存活；`/lingxi/v1/health`+identity 仍 200；`instance.json` sha 不变；远端日志无 shutdown marker；S3 中远端 ppid 未变、未被重新 parent |
| `a04::no-stray-spawn` | 退出窗口期无第二个 `lingxi-service` 出现（监控 2s 采样）；localHome 无残留 `instance.json`（服务优雅退出会自删，`instance.rs:600+`） |
| `a04::reused-is-detached` | dev 场景：先手动起服务在 localHome，再起 Tauri（dev，允许 reused）→ 退出：宿主 `mode=local-reused`、报告 `detached`、服务 pid 仍存活（与 Electron `main.cjs:6900-6908` 对齐）；随后 harness 自行清理 |
| `a04::remote-mode-zero-touch` | debug 构建 + `LINGXI_DESKTOP_RUST_REMOTE_URL/TOKEN_FILE`（loopback、显式端口、cleartext 豁免仅 loopback）：启动后 home 无 `instance.json`（未 spawn 本地）；退出后远端 pid/instance 摘要不变；宿主报告 `mode=remote,touchedRemote=false` |
| `a04::packaged-refuses-remote` | release 构建同样 env → 明确拒绝 `REMOTE_REQUIRES_DEV_BUILD`，不连接/不 spawn；记录 evidence（边界声明） |
| `a04::pid-reuse-safe` | 伪造 `instance.json` 指向一个无害存活进程（如 `sleep 60` 的 pid）但 token/identity 不匹配：packaged → 拒绝复用、**不杀**该 pid；dev → 不误杀，继续走 spawn 或被明确拒绝；结束后无害进程仍存活（harness 自清） |

**证据路径**：`{EVIDENCE}/SUITES/r09_t02_process_ownership/{result.json,output.log,cases.json,tree-s1.json,tree-s2.json,tree-s3.json,remote-*.json,shutdown-report.json}`。
**必须真机构建**：release/debug Tauri 构建 + 真实 `lingxi-service`；远端夹具进程为本机真实进程（非 mock）。

### 5.5 证据纪律汇总

- 所有套件经 `r09_producer.Producer`（隔离 home/tmp、真实 `~/.lingxi` 前后摘要、真实 argv/exit）。
- `result.json` 必须让门能机器读：`status/exitCode/caseCount/commands[]`（Producer `finish()` 已实现）；失败时 suite exit 2 并保留 `cases.json` 中失败 case（诚实红）。
- 大文件：`.app` 副本、进程快照、截图放 `/private/tmp/lingxi-r09-runtime/...` 或 `LINGXI_R09_RAW`，artifacts 只放摘要/清单/日志（R08 69GB 教训）。
- 平台声明：全部证据标注 `macos-arm64`；A01–A04 不跨平台外推；Windows/Linux/mac-x64 按 `BLOCKED-PLATFORM-*` 记录。
- 不把“命令 exit 0”写成“场景 PASS”；A01–A04 的 PASS 必须逐 case 绑定（scenario note 已明确）。

---

## 6. OpenQuestions（含可执行实测步骤）

| ID | 问题 | 现有事实/依据 | 实测步骤（待实现阶段执行） | 影响 |
|---|---|---|---|---|
| Q-01 | **CLT 27.0/无 Xcode.app 下 `tauri build` 是否可行（2.12 线）** | R01-T06 在 Tauri 2.11.6 上 `npx tauri build --bundles app` exit 0（`docs/rust-tauri/R01/R01-T06_REPORT.md:37-38`），打包 .app 冒烟过；本机 CLT-only、无 Xcode.app | ①`xcode-select -p`、`xcodebuild -version`、`codesign`/`hdiutil` 版本入证据；②最小 overlay 构建 `npx tauri build --config <release overlay> --bundles app`；③`codesign --verify --deep --strict <app>`、`spctl -a -vv`（预期 ad-hoc 失败/提示）；④`--bundles dmg` 单独试（`osascript` Finder 布局在无交互会话可能失败，失败即记录，不阻塞 T01/T02） | A02/A03/A04 的“release 安装产物”范围；DMG/正式安装器是否能在 R09 声称 |
| Q-02 | **签名/摘要：ad-hoc 签名会改可执行字节，构建清单 sha256 必变** | Node 已有签名后内容摘要逻辑（`rust-local-service.cjs:43-117`）与 `contentSha256` 字段（`build-rust-desktop-service.mjs:132`）；本机 0 身份=只能 ad-hoc | 构建后比较 `sha256(lingxi-service)` 与 `build.json.sha256`；预期不等；验证 `contentSha256` 计算一致；`codesign --verify --strict` + `--deep` app bundle；决定 T02 的 `service/manifest.rs` 是否必须移植内容摘要（初判：**必须**） | T02 启动门（否则每次启动都被拒）；A03 digest case 设计 |
| Q-03 | **服务二进制分发：externalBin 的 triple 后缀、签名与 `current_exe()` 解析** | R01 sidecar 实测“打包随包、握手成功”与 triple 规则（`TAURI_SPIKE_REPORT.md:59-77`）；`current_exe` 拒绝符号链接路径 | 用 `rustc --print host-tuple`（或 `rustup run 1.98.1 rustc -vV` host）stage `lingxi-service-aarch64-apple-darwin`；构建后检查 `Contents/MacOS/lingxi-service` 存在且可执行；宿主按 `current_exe()` 同级查找；`codesign -dvvv` 记录 sidecar 签名状态 | 推荐布局 L1 是否成立；Windows/Linux 留阻塞 |
| Q-04 | **协议/数据版本唯一来源** | Rust 常量 wire 1..=1 / epoch 2（`handshake.rs:21-23`、`lingxi-protocol/lib.rs:408-438`）vs Node JSON epoch=1（`shared/contract-versions.json`）；`build.json` 无协议字段；R08 API_READINESS §3 明确要求“版本与 epoch 须和 T03 就绪链统一” | 扩展 `build-rust-desktop-service.mjs` 增加 `protocol`（来源=服务 `--version` 输出）；加一个 `xtask`/契约测试核对 `shared/contract-versions.json` 与 Rust `ContractVersions` 的对应关系（Node 轴 vs Rust 轴的显式映射，不强行相等）；A03 用篡改 manifest 验证门生效 | T02 版本门/A03；跨阶段契约一致性 |
| Q-05 | **tauri config overlay 合并语义 + 编译期嵌入哪份 config** | base 为 R06 config（`tauri.conf.json:1-15`）；Tauri 2 支持 `--config` overlay 与平台级 config 文件；数组字段合并行为需实测 | ①用 overlay 构建；②`strings <binary>` 查 `dist-renderer`/`dist-tauri-resources`/`5173`；③宿主诊断里回读生效 config 摘要（或 `tauri::generate_context!` 的 config 字段）；④`cargo test --test desktop_host` 复跑证 base 未变 | §3.3 方案成立与否；若 overlay 不能替换数组，则改用**独立 overlay 全量写 windows/bundle** 或改 base+单独 R06 overlay（需 R06 迁移记录） |
| Q-06 | **identifier 与并行安装**：Tauri release 用 `com.lingxi.app`（Electron appId，`package.json` build.appId）还是新 id | Electron 现役 `com.lingxi.app`；并行安装/LaunchServices/单实例插件行为受影响 | 用两个不同 `.app` 名 + 相同/不同 identifier 各构一次，观察 LaunchServices/默认协议/单实例（T03 才实现）冲突；evidence 只做记录 | R09-T03 单实例/默认应用；A01 并行启动 |
| Q-07 | **CSP 双注入**（Tauri config CSP + `connection-csp.js` meta）是否阻断 `invoke`/WS | `connection-csp.js:6-18` 的 connect-src 无 `ipc:`；Tauri IPC 无 fetch 时可能不受限；无实测 | WebDriver 在 Tauri 页面执行 `window.__TAURI__.core.invoke('get_platform')` + `ws = new WebSocket(wsUrl)`，收集 console CSP violation；若阻断→additive 修改 `connection-csp.js` 并复跑 Electron 侧 | T01 桥可用性；T06 CSP 收口输入 |
| Q-08 | **远端模式是否保持 dev-only** | Electron packaged 拒绝 remote（`main.cjs:1359-1361`）；R08-T03 明文“预览/演练”；T02 任务书只要求“本地/远端模式” | 按 dev-only 实现并让 A04 用 debug 构建；正式化（TLS/凭据 UI/权限）作为 T04/T05/T06 裁决项登记 | A04 证据形态；安全面 |
| Q-09 | **诊断目录命名/共存**：`crash.log` 与 Electron 同为 `LINGXI_HOME/crash.log` 会互相覆盖 | Electron `main.cjs:2624` 写 `{home}/crash.log`；Tauri 若同 home（迁移期）会冲突 | 建议 `{home}/diagnostics/tauri-launch/events.jsonl` + `{home}/tauri-crash.log`；找到/切换 home 不影响 Electron 默认；evidence 记录 | T02 诊断/A02/A03 证据 |
| Q-10 | **T01/T02 是否要求 renderer OTA/签名 seed 链** | Electron 生产走 artifact-boot 签名 seed（`main.cjs:1661-1761`）；任务书 T01 只要求“同一 React 产物/生产不依赖 dev server” | 本设计按“内嵌 dist-renderer”实现 A01/A02；OTA/seed 兼容列为 T06/T08 后续范围并在 R09_HANDOFF 登记，不在 T01/T02 声称 | 生产升级链路完整性；R09 后续任务 |
| Q-11 | **Windows 树收敛与签名** | 无 Windows 机（BLOCKED-PLATFORM-WINDOWS-X64）；Electron Rust 路径没有 guardian（`main.cjs:6858` 条件） | 设计 Job Object（KILL_ON_JOB_CLOSE）+ `--read-private-runtime-json` 私读 + PE 签名验证；真机验证留阻塞 | T02 跨平台完整性声明 |
| Q-12 | **token 暴露面**：`get_server_token` 返回给渲染层是否保留 | Electron 保留（`main.cjs:5647`），renderer 用它连 HTTP/WS；T06 要收口能力面 | T01/T02 保留同语义、命令仅 main 窗可调；T06 决定是否收敛为“宿主代理 HTTP”方案（会大改 renderer，不属 T01/T02） | 安全面/T06 输入 |
| Q-13 | **服务启动与窗口时序**：先 ready 再建窗（本设计）vs 预建隐藏窗+boot gate | Electron 先 splash 后 startServer 再主窗（`main.cjs:6676-6734`）；T01/T02 无 splash | 本设计选“ready 后建窗”；若评审要求先见窗，需 boot-gate command + renderer boot 重试（改共享 React 需评估 A01 范围） | UX/超时体验；T03 splash |
| Q-14 | **R06 测试回归**：扩展同一 crate 后 `tests/desktop_host.rs` 是否仍绿 | 该 E2E 依赖 base config 嵌入的 dist-tauri-resources（`tauri.conf.json:6`）与 CARGO_BIN_EXE（`tests/desktop_host.rs:139`） | 若 base 保持 R06 则理论不受影响；需实跑 `cargo test --features e2e-test --test desktop_host`（真实 GUI，可能受本机权限影响，R06 已有该条件） | Q-05 决策输入；R09 不得静默破坏 R06 |

---

## 7. 风险登记（实现阶段必须跟踪）

| ID | 风险 | 等级 | 缓解 |
|---|---|---|---|
| R-01 | A03/A02 依赖真实 `tauri build`；CLT-only + 新 CLI/crate 组合未验证，构建可能失败 | 高 | Q-01 探针先做；失败即记录 R09_BLOCKED，不拿旧证据冒充；保留构建日志 |
| R-02 | ad-hoc 签名改变二进制摘要，若 T02 未移植内容摘要逻辑，packaged 启动全部失败 | 高 | Q-02；把 `service/digest.rs` 列为 T02 必交付 |
| R-03 | T01 改 React host 后 `dist-renderer` 摘要变化，A01 若仍引用 R08 基线 digest 会变成假证据 | 高 | A01 的“同一产物”=两侧同新 digest；旧基线只作 diff 说明；白名单检查业务组件 0 改动 |
| R-04 | overlay 合并语义不清导致 release 嵌错 UI（`dist-tauri-resources`）或残留 devUrl | 高 | Q-05 探针 + A02 strings/URL 断言 |
| R-05 | 用 Node `contract-versions.json` 做 Rust 门 → epoch 1 vs 2 误拒；反之漏检 | 高 | §4.4 明确来源；A03 负向覆盖 |
| R-06 | 宿主启动失败弹原生 dialog 阻塞自动化/验收 | 中 | 先写 failure record 再弹窗；自动化用 `osascript` 关闭（R01 先例）；证据以记录为准并记录 dialog 观察 |
| R-07 | Tauri IPC 被双 CSP 拦截 / WS 连不上 | 中 | Q-07 WebDriver 探针；必要时 additive 改 `connection-csp.js` 并回归 Electron |
| R-08 | 退出信号处理缺失 → SIGTERM 直接终止，A04 变成“假清理/或残留” | 高 | T02 必须装 SIGTERM/SIGINT handler 走 `app.quit()`；A04 首选信号触发并验证 shutdown 事件 |
| R-09 | 证据目录膨胀（R08 已有 69GB artifacts） | 中 | 大产物只进 `/private/tmp`；artifacts 只放摘要/清单/小日志 |
| R-10 | 并发 Lead/其他 teammate 正在改 R09 基础设施/门禁，引用会漂移 | 中 | 本文所有 R09 门禁锚点标注“21:14 快照”；实现时以当时 `R09.json`/`r09_stage_inputs.py` 为准；只读不写 |
| R-11 | R06 E2E/资源 UI 因 base config 或 capability 变动回归 | 中 | Q-14；保留 base；新命令 additive；必要时把 R06 验证写进 T01 收口清单 |
| R-12 | 平台声明被夸大（Windows/Linux/x64 无真机） | 高 | 证据全部标 `macos-arm64`；其余登记 BLOCKED-PLATFORM，不写成“平台不支持” |
| R-13 | 90s READY 超时在失败负向用例中拖慢/阻塞证据 | 低 | 超时用 Rust 单测注入短 Duration；E2E 只覆盖立即失败（manifest/digest/runtime） |

---

## 8. 未执行与限制（诚实声明）

- 未构建、未启动、未打包、未安装、未调用 WebDriver/CDP、未运行任何 vitest/cargo 测试；§5 全部命令为草案。
- 平台仅 macOS 27.0.1 arm64；macOS x64/Windows/Linux 未验证（BLOCKED-PLATFORM-*）。
- 签名身份 0，正式签名/公证/分发不可声称（BLOCKED-SIGNING-IDENTITY）。
- 真实 provider 未使用（A01–A04 均可用确定性替身，符合场景 environment 声明）。
- 本文引用的 Lead R09 门禁文件为工作区未提交快照（21:14 前后），可能在实现时继续演进。
- 只读：本调查只新建 `docs/rust-tauri/R09/T01_T02_HOST_SERVICE_DESIGN.md` 与 `T01_T02_HOST_SERVICE_EVIDENCE.json`；未修改 AGENTS.md/package*.json/rust/**/scripts/**/docs 其他文件/阶段图/锁文件。

---

## 9. 关键文件:行索引（写作依据）

**Electron 壳 / Node 共享模块**
- `desktop/main.cjs:176-177`（LINGXI_HOME）、`:543,615-631`（dev 页面加载）、`:703-728`（server 全局态 + 超时常量）、`:990-992`（rust 下禁用 legacy 迁移端点）、`:1004-1055`（进程存活/等待/关停请求）、`:1063-1065`（readiness 常量导入）、`:1104-1158`（Node poll）、`:1231-1304`（可复用校验）、`:1310-1334`（Rust identity）、`:1336-1343`（publish）、`:1345-1508`（startRustServer）、`:1510-1674`（startServer 选择/双启动/stale/reuse）、`:2040-2252`（_spawnServerOnce）、`:2298-2349`（monitorServer）、`:2425-2480`（崩溃诊断）、`:2624`（writeCrashLog）、`:5646-5655`（连接信息 IPC）、`:6661-6822`（whenReady 启动时序）、`:6854-6940`（shutdownServer）、`:6942-6988`（before-quit）。
- `desktop/src/shared/rust-local-service.cjs:6-15,17-41,43-117,134-184,186-192,194-215,217-266,268-277,279-296,298-317,319-375`。
- `desktop/src/shared/stale-server-info.cjs:14-19`；`shared/server-info-probe.cjs:72-139`。
- `desktop/src/shared/desktop-launch-diagnostics.cjs:1-50`；`desktop/src/shared/server-readiness.cjs:34,38,39`。
- `shared/hana-runtime-paths.cjs:4-17`；`shared/contract-versions.json` / `.cjs`。
- `desktop/src/react/app-init.ts:110-174,177-206,238-247`；`desktop/src/react/types.ts:547-701`；`desktop/src/react/host/desktop-host.ts:18,27-33,46-54,85-120`；`desktop/src/react/host/electron-desktop-host.ts:1-21`；`desktop/src/react/host/web-desktop-host.ts:72-…`；`desktop/src/react/host/host-capability-error.ts:5-21`。
- `desktop/src/react/stores/session-actions.ts:276-279`；`desktop/src/react/services/websocket.ts:58,132,142-153`。
- `desktop/src/modules/platform.js:13-22`；`desktop/src/modules/connection-csp.js:1-68`。
- `vite.config.ts:267-292,299,339,342-357,361`；`desktop/vite.tauri-resources.config.ts:5-11`；`vite.csp-profiles.ts:15-18`。
- `package.json:2-3,52-53,56-60,137-139,191-199`（含 mac `binaries` 段）。

**Rust 服务**
- `rust/crates/lingxi-service/src/main.rs:1-23,41-222,224-233,236-260,671-712,1080-1160,1200-1300+`。
- `rust/crates/lingxi-service/src/instance.rs:19,58,157-177,220-243,432+,561-600+`；`paths.rs:8-27,38-39,117-127`；`auth.rs:76,2110-2133`。
- `rust/crates/lingxi-service/src/lib.rs:536-543,3646-3657,8251`；`management.rs:647,1240-1275`。
- `rust/crates/lingxi-protocol/src/handshake.rs:21-23,99-112`；`rust/crates/lingxi-protocol/src/lib.rs:408-438`。
- `scripts/build-rust-desktop-service.mjs:16-21,23-25,39-59,71-73,90-155`。

**Tauri / R06/R01/R08/R09 文档与证据**
- `desktop/src-tauri/tauri.conf.json:1-15`；`Cargo.toml:14-33`；`build.rs:2-3`；`src/lib.rs:530-596`；`tests/desktop_host.rs:137-160`；`README.md:23-26`。
- `docs/rust-tauri/R01/ADR-001-ownership.md:77`；`DEPENDENCY_DECISIONS.md:234-260,294+`；`DEPENDENCY_RULES.json:86-96,180-190`；`R01-T06_REPORT.md:37-38`；`TAURI_SPIKE_REPORT.md:59-77,79-87,97-102,121-123`。
- `docs/rust-tauri/R08/API_READINESS.md:16,40-46`；`T03_DESIGN.md:14,24,65-120,128-145`；`T03_FREEZE_MATRIX.md:17,33,73-84`；`R08_HANDOFF.json:134-138,165-168`。
- `artifacts/rust-tauri/R08/verify-r08-closeout-08/SUITES/r08_a16_versions/a16-versions.json`。
- `rust/crates/xtask/src/stage_maps/R09.json:36-97`；`R09_INPUTS.json:1641-1644`；`scripts/rust-tauri/r09_suites.py:24-33,80-96`；`r09_producer.py:112-245`；`r09_stage_inputs.py:34-46`。
- `docs/rust-tauri/R09/PROGRESS.md:12-58,59-78`（Lead 的 R09 实施规格/写作用域；§6 明确本文属三份设计调查之一、T02 以 `desktop/src-tauri/src/supervisor.rs` 为实施落点）。
- `docs/rust-tauri/R00/ACCEPTANCE_MAP.json:6921-7020,25041-25044`；`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R09_…md:42-122`。
