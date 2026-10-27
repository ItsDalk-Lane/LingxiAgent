# R08-T03 设计文档：Electron 启动 Rust 服务的预览模式（+ A05/A06）

> 状态：**待 Lead 评审**。任务书：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R08_旧Electron客户端接入与数据迁移演练.md` L125-165。
> 预研基线：`docs/rust-tauri/R08/T03_SCOPING.md`（Lead，2026-10-22），本文所有「现状」均已在源码逐条复核。
> 执行纪律：合成 HOME（`lingxi-r08-t03-*`）、证据 `t03-*` 前缀、`r08_run.py` 包裹、红证据保留、真 `~/.lingxi` 永不触碰、不改 `rust/` 产品源码、不改生产默认启动路径。

## 1. 目标与非目标

### 目标
1. **显式预览启动选择 + 可见性**：沿用既有 env 门 `LINGXI_DESKTOP_SERVER_RUNTIME=rust`（不改生产默认），
   补上「预览」语义的可见面：主窗口标题标记 + 崩溃诊断条目。生产打包默认仍为 node（现状已满足，证据化）。
2. **本地托管 / 远端两种模式分开**：新增显式远端连接路径——不 spawn、不读本地 instance pid、
   不杀远端进程；身份/token 校验**不放宽**。
3. **冻结策略矩阵**：盘点 close-window / 退托盘 / 彻底退出 × 本地owned / 本地复用 / 远端 的行为，
   退出超时报告具体残留 PID，矩阵成文并经集成证据核验。
4. **完整进程树记录**：外部记录器产出进程树快照，区分「Electron 仅 UI」与「旧 Node 内核仍在执行」。
5. **A05**：禁用旧 Node 服务 + 预览开启下完成真实文件任务（合成 HOME + 确定性替身 Provider），
   证据 = 进程树 + 执行轨迹。
6. **A06**：两个客户端连同一测试服务，关闭其一，另一个仍工作、任务继续、无本地服务偷启动。

### 非目标（明确不做）
- 不重建 READY 握手 / instance+token 校验 / 失败清理（已存在，见 §2）。
- 不改 `rust/` 产品源码；fs 等路由若未映射 → 显式 FAIL/BLOCKED 上报 Lead，不静默降级、不绕道实现。
- 不做正式版远端模式的 TLS 终端/UI 配置面；远端模式本阶段定位 dev 演练（见 §3.2 红线）。
- 不做 R07 新宿主切换实现；只对仍走 Electron 对照的原生浏览器/PDF 路径做**显式标记**（§4）。
- 不新增渲染层 badge 组件（标题 + 诊断即可见性闭环；About 页标记列为可选延展，由评审裁决）。

## 2. 现状核查结论（2026-10-22 复核，行号锚定）

| 能力 | 现状 | 锚点 |
|---|---|---|
| 选择门 | `rustDesktopEnabled()`：env `LINGXI_DESKTOP_SERVER_RUNTIME`，`'node'`（默认）/`'rust'`，其他值抛错 | `desktop/src/shared/rust-local-service.cjs:186-192`；`desktop/main.cjs:1482-1485` |
| 二进制解析 | dev：`LINGXI_SERVICE_BIN` 或 `rust/target/debug/lingxi-service`（绝对路径/普通文件/可执行）；packaged：`resources/rust-service/` 清单+魔数+sha256+签名验签 | `rust-local-service.cjs:194-213` |
| 启动+握手 | `startRustServer()`：`spawn(binary,['--home',home])` → 解析 stdout `LINGXI_SERVICE_READY addr=…`（有界 65536，超时 `SERVER_INFO_FIRST_WAIT_MS`）→ `readOwnedRustConnection` → `verifyRustDesktopIdentity` → `publishRustDesktopConnection` | `main.cjs:1338-1464` |
| 实例复用 | `readExistingRustConnection`：pid 存活 + instance.json/local-token.json owner-only 校验 + READY 地址一致；复用时 `reusedServerPid=pid; reusedServerOwned=false; serverProcess=null` | `rust-local-service.cjs:298-318`；`main.cjs:1363-1376` |
| 地址约束 | `localAddress()` 强制 127.0.0.1/[::1]，**远端（非 loopback）路径不存在** | `rust-local-service.cjs:270-277` |
| 失败清理 | READY 失败 → SIGTERM → 等退 → SIGKILL → 等退；未确认退出抛「owned process exit was not confirmed」 | `main.cjs:1463-1479` |
| 优雅退出 | `before-quit` → `shutdownServer()`：owned 子进程 SIGTERM（宽限）→ SIGKILL，未确认退出保留线索并报告 PID；复用非 owned → 「detached from external server」零接触 | `main.cjs:6906-6952`、`6820-6904` |
| 分支消费 | `serverNodeKind==='lingxi-service'` 约 20 处 IPC 分流（office 预览走 rust、browser 控制抛 `browser_connection_required` 等） | `main.cjs` 全文 grep |
| 渲染侧消费 | `get-server-connection-info` IPC → preload `getServerConnectionInfo` → `activeServerConnection.serverNodeKind`（Electron host 专属成员，web host 无此成员——预览标记天然桌面限定） | `main.cjs:5613-5618`；`app-init.ts:111-228` |
| 渲染产物 | `desktop/dist-renderer/` 已构建，`loadFile` 回退存在——无 vite 也可起真实窗口 | `main.cjs:543,621-627` |
| debug 二进制 | `rust/target/debug/lingxi-service` 就绪（19:31 构建，277MB） | — |
| 模型面配置 | 服务从 `{home}/lingxi-service/models.json` 读独立模型面（A04 同款 providers/models 结构），`--config` 亦可内嵌 | `rust/crates/lingxi-service/src/config.rs:1159-1189`、`paths.rs:36-49` |
| 替身 Provider 模式 | A04：`R08_SYNTHETIC_ONLY` apiKey + 本机回环 SSE 三幕剧本 + golden | `docs/rust-tauri/R08/PROGRESS-a04.md` |
| Electron 驱动先例 | R06 office-live：合成 HOME spawn Electron + Rust 服务 + 真实 preload | `desktop/src/react/__tests__/office-live/main.cjs:1-40` |

**缺口确认**（与预研一致）：①预览语义不可见；②远端模式不存在；③冻结策略未成矩阵；
④`_lastServerSpawn` 只记自身 spawn（`main.cjs:996,1418,2157`），无进程树证据面；⑤⑥ A05/A06 夹具缺。

## 3. 目标架构

### 3.1 预览可见性（任务书第 1 条后半 + 预研缺口 1）

env 门保持唯一选择机制（显式、默认 node）。补两个可见面，全部只在 rust 模式出现：

1. **主窗口标题**：`createMainWindow` 标题在 `rustDesktopEnabled()` 为真时后缀 `· Rust 预览`
   （走 `mt()` 壳 i18n；splash/quick-chat/settings 窗口不动）。实现点：`main.cjs:1784`（`title: "Lingxi"`）。
2. **崩溃诊断条目**：`buildServerCrashDiagnostics()`（`main.cjs:2396`）追加
   `Server runtime: rust (preview)` / `Rust connection mode: local-owned | local-reused | remote`。
   `_lastServerSpawn` 已有 command/pid/alive/exit 条目，复用不重建。
3. **生产默认不变证明**：`rustDesktopEnabled` 只读 env，无打包侧默认值改写；证据 = 单元测试断言
   无 env 时 `node` + packaged 路径仍走 node（既有测试 `tests/desktop-rust-local-service.test.mjs` 已覆盖
   门行为，补一条显式默认断言如缺）。

### 3.2 远端模式（任务书第 3 条 + 预研缺口 2，**本次主要新增代码**）

新增 `readRemoteRustConnection(env)`（`desktop/src/shared/rust-local-service.cjs`），与
`readOwnedRustConnection` **并列独立**——不复用其 loopback 强制（`localAddress`），但保留同等刚性：

- `LINGXI_DESKTOP_RUST_REMOTE_URL`：绝对 http(s) URL，显式端口，禁 userinfo/path/query/hash；
  host 为空/端口越界 → 抛。
- `LINGXI_DESKTOP_RUST_REMOTE_TOKEN_FILE`：绝对路径 JSON `{"token":"^[0-9a-f]{32}$"}`，
  经既有 `readPrivateJson(..., requirePrivate=true)` 读取（owner-only 校验原样复用，token 不进 env/日志/命令行）。
- 非 loopback + 明文 http：必须显式 `LINGXI_DESKTOP_RUST_REMOTE_ALLOW_CLEARTEXT=1`（演练专用，
  命中即 loud log）；https 不需要。缺失标记 → 抛错拒绝。
- 门一致性：设了 REMOTE_URL 而 `LINGXI_DESKTOP_SERVER_RUNTIME≠rust` → 抛「contradiction」；
  `app.isPackaged` + remote → 抛（预览演练限 dev；正式化属 R07 后续）。
- 返回 `{ port, transport, baseUrl, token, remote: true }`。

`startRustServer()` 分支：REMOTE_URL 存在 → **跳过** `resolveRustBinary` / `readExistingRustConnection` /
`spawn` 全部本地路径 → `verifyRustDesktopIdentity(remote)`（Bearer + server/identity 形状校验原样保留）
→ `publishRustDesktopConnection` + `rustConnectionMode='remote'`（新状态变量，进诊断）。
`serverProcess=null; reusedServerPid=null` ⇒ `before-quit` 的 `hasActiveOwnedServer=false` ⇒ 直接放行，
**所有退出路径对远端零接触**（结构性保证，不靠分支纪律）。

远端模式窗口标题后缀 `· Rust 预览（远端）`，与本地 `· Rust 预览` 区分。

#### 3.2.1 host 传播链（Lead 评审必修，2026-10-22）

**缺口**（Lead 复核确认）：`server-connection.ts:228-229` `createLocalServerConnection` 硬编码
`127.0.0.1`；`main.cjs:5614` `get-server-connection-info` 只回 port/token/kind/transport。按原设计
发布 remote 连接后渲染层仍连回环 → A06 客户端连错地址。补链如下：

1. **main.cjs**：新全局 `serverHost = null`（null 语义 = 127.0.0.1，node 路径行为逐字节不变）；
   `publishRustDesktopConnection` 增设 `serverHost = connection.host ?? null`（remote 连接带真实
   hostname；本地 owned/复用不带 → null）。`get-server-connection-info` 增 `host: serverHost` 字段。
2. **preload.cjs**：透传 invoke 结果，无改动。
3. **types.ts**：`PlatformApi.getServerConnectionInfo` 返回类型增 `host: string | null`。
4. **server-connection.ts**：`createLocalServerConnection` 增可选 `serverHost`。默认/null/loopback →
   现 local 形态不变；`serverNodeKind==='lingxi-service'` 且非 loopback host → **改走手动连接形态**
   （对齐 L484-495 既有助手，不走 local 形态）：
   `baseUrl = ${transport}://${serverHost}:${port}`、`wsUrl` 同理；
   `kind = normalizeBrowserConnectionKind(undefined, host)`（lan/custom_remote 分类）；
   `trustState/credentialKind = normalizeBrowserTrustState/normalizeBrowserCredentialKind(undefined, kind)`
   （lan → 'lan'/'device_credential'，非 loopback 绝不再产出 'local'/'loopback_token'）；
   `connectionId` 保持 `LOCAL_CONNECTION_ID` 语义由 kind 字段区分，label 标 `Rust 预览（远端）@host`。
   host 含端口/userinfo/路径 → 抛（与 main 侧 URL 校验同源同则）。
5. **调用点全链**：`app-init.ts:137,177`、`SettingsContent.tsx:262,548`、`QuickChatApp.tsx:332`、
   `screenshot.ts:383` 均把 connection info 的 `host` 传入 `createLocalServerConnection`。
6. **main 侧审计**：grep `main.cjs` 全部 `127.0.0.1:${serverPort}` / `localhost:${serverPort}` 拼串点，
   逐点确认 rust-remote 下不可达（owned/reused 为空时不触发）或改走 serverHost；审计结论记入
   PROGRESS-t03.md（缺格才改码，同矩阵纪律）。
7. **批次 B 验收断言**（Lead 指定）：单测断言 remote host 出现在最终 `baseUrl`/`wsUrl`，
   且 kind≠'local'、credentialKind≠'loopback_token'、token 原样保留。

**实施注记（2026-10-22 批次 B 落地时补充）**：
- 调用点实为 **7 处**：上列 6 处之外，`onboarding/OnboardingApp.tsx:107` 也经
  `createLocalServerConnection`（rust 模式构造后即抛「Rust onboarding is not migrated」，无实际远端
  消费；仍补传 host 保持纵深）。另 `server-restarted` 事件负载与 SettingsContent/app-init 重启回调
  一并补 `host`；`screenshot.ts` 指 `utils/screenshot.ts:380`（fetchAvatarAsDataUrl 改用
  getServerConnectionInfo 取 host）。
- main 侧审计结论：4 处回环拼串全部 rust-remote 不可达（1244/1257 server-info.json 复用仅 node 可达；
  2265 rust kind 前置抛；4172 rust 模式无 sessionPath；4801 rustHost 前置 return），无须改码。
- 窗口标题双层：BrowserWindow opts.title（加载前窗口期）+ app-init.ts document.title（加载后覆写，
  远端加（远端）后缀）。
- `websocket.ts:198` 显式 port/token 分支全仓无空调用以外的调用点，未传 host，记录在案。

### 3.3 冻结策略矩阵（任务书第 4 条）

成文矩阵（`docs/rust-tauri/R08/T03_FREEZE_MATRIX.md`），列 = 宿主模式，行 = 触发面：

| 触发 | node-owned（对照） | rust 本地 owned | rust 本地复用 | rust 远端 |
|---|---|---|---|---|
| 关窗口（有托盘） | 隐藏常驻 | 同左（不动服务） | 同左 | 同左 |
| 关窗口（无托盘） | forceQuitApp→quit | 同左 → before-quit | 同左 | 同左 |
| 托盘退出 | isQuitting→before-quit→shutdownServer | SIGTERM→SIGKILL，超时报告残留 PID | detach 零接触 | 零接触（结构性） |
| 彻底退出 before-quit | 同上 | 同上；未确认退出保留线索+loud | detached 日志 | 直接 allow |

落地动作：逐格核对现有代码填矩阵；**缺格才改码**（预期缺口集中在：rust owned 退出超时的
残留报告文案已带 PID（`main.cjs:6859`），remote 格由 §3.2 结构性满足）。矩阵每格由 A05/A06
或专项集成证据支撑，不许纸面声明。

### 3.4 进程树记录器（任务书第 5 条）

新增 `scripts/rust-tauri/r08_t03_process_tree.mjs`：
- 输入：根 PID（Electron 主）、输出路径；`ps -Ao pid,ppid,comm,args` 全表自研建树（mac/linux 通用；
  win32 本阶段标记未覆盖）。
- 分类标注每节点：`electron-ui` / `lingxi-service` / `node-kernel`（args 含 `server/bundle` 或
  `hana-server`）/ `other`；快照 JSON + 人读文本。
- A05/A06 在关键相位（启动后 / 任务中 / 退出后）各拍一张；断言面直接消费 JSON。

### 3.5 状态变量小结（main.cjs 新增面）

`rustConnectionMode: 'local-owned' | 'local-reused' | 'remote' | null`（publish 处赋值；诊断+标题消费）。
仅此一个新变量；其余复用既有 `serverProcess/reusedServerPid/reusedServerOwned` 语义。

## 4. DesktopHost 复用与 Electron 对照标记（任务书第 2 条）

- 渲染侧消费 serverNodeKind 已走 `usePlatform().getServerConnectionInfo()`（Electron host 成员；
  T02 web host 不提供该成员——web 宿主流不到预览语义，符合预期）。本任务不改渲染层依赖边界。
- **对照标记**：rust 模式下仍走 Electron 的路径——①browser 远程控制（rust 宿主下已抛
  `browser_connection_required`，main.cjs:5731+，属显式拒绝非静默降级）；②PDF 预览
  `office-pdf-helper.cjs` 主进程子进程（T02 白名单既有）；两者在 T03_DESIGN 与冻结矩阵文档中
  标注「Electron 对照，正式目标须可切 R07 新宿主」。docx/xlsx 预览 rust 模式已分流
  `readRustOfficePreview`（main.cjs:6455/6469），不属于对照面。

## 5. A05 设计：旧界面真正使用 Rust 内核

夹具 `scripts/rust-tauri/r08_t03_a05_preview_file_task.mjs`（node 驱动，全相位快照）：

1. 合成 HOME `mktemp lingxi-r08-a05-*`；写入 `agents/lingxi/config.yaml`（A04 fixture 同形）、
   `product/yuan/lingxi.md`、`{home}/lingxi-service/models.json`（synthetic plane：endpoint 指替身、
   apiKey `R08_SYNTHETIC_ONLY`、chat+title 两模型）。
2. 起确定性替身 Provider（复用/对齐 r00-t06-stub-provider.mjs 的 openai-completions SSE 形态；
   剧本三幕：tool_call 读 `a05-seed.txt` → tool_call 写 `a05-out.txt` → final answer 收束）。
3. spawn `electron .`：`LINGXI_HOME=<合成>`、`LINGXI_DESKTOP_SERVER_RUNTIME=rust`、
   `LINGXI_SERVICE_BIN=<abs debug 二进制>`；无 vite，走 `dist-renderer` 真实页面。
4. 等 `{home}/lingxi-service/instance.json` → 读 addr+token（同机 owner，harness 可读）→
   `verifyRustDesktopIdentity` 等价校验（server/identity Bearer）。
5. **进程树快照①**：断言含 electron 进程组 + 恰好 1 个 `lingxi-service`；**零** `node-kernel`；
   `{home}/server-info.json` 不存在（旧 Node 服务未启动的结构性证明）。
6. **真实文件任务**：经服务 WS/HTTP 面（A01 线协议）发任务「读 a05-seed.txt 内容写入 a05-out.txt」→
   等 run terminal；断言：`a05-out.txt` 字节等于剧本预期；执行轨迹 = key_events
   `tool_call_started/completed` + WS 帧序列 + provider 请求计数；进程树快照②（任务中）：仍无
   node-kernel、无 pi/python 旧循环进程。
7. **预览可见性断言**：主窗口标题经 CDP（`--remote-debugging-port` + `/json` targets）读取含
   「Rust 预览」；CDP 受限 → 退路为诊断文本断言 + 显式偏差记录（不静默跳过）。
8. 退出：SIGTERM Electron → 等退 → 快照③：owned rust 子进程已确认退出、无残留；
   崩溃诊断文件含 runtime/mode 条目。
9. **FAIL/BLOCKED 升级**：文件任务若因 fs 路由未映射失败（工具路由 404/未实现）→ 抓取确切
   路由+响应体，标 BLOCKED，立即报 Lead；不绕道、不替身补洞。

### 5A. A05 演练结论（2026-10-22，drill-06/07/08；Lead 裁决接受双 BLOCKED 呈现）

实装夹具为 `r08_t03_a05_drill.mjs`（本设计第 6 步拆为读腿/写腿双会话，替身改 marker 驱动
三幕剧本）。演练暴露两个预览平价缺口，**同总根因**（预览启动无 `--config` → 无 workspace），
**不同失效机制**：

- **PPG-1（写腿：工具不注册）**：main.cjs:1437 仅 `--home` → rust main.rs:455 workspace_root
  唯一赋值点缺位 → lib.rs:1459-1476 workspace_access=None → lib.rs:1666 write/edit 仅
  workspace 分支注册 → 模型工具面恒为 14 项（无 write/edit）→ 写任务 `failed.provider_error`。
  drill-08 帧级硬断言：唯一 `assistant_run_end reason=failed.provider_error` + error 帧
  `run_failed` + 零 `assistant_segment` 伪回答。
- **PPG-2（读腿：注册但授权零根、无注入孔道）**：read 已注册并声明进模型面，但
  lib.rs:1498-1508 `session_read_access = ResourceAccess::new(&[])`（零根策略）；每轮授权
  刷新 runs.rs:2949 只经 lib.rs:2042-2047 接 workspace_access（预览=None）→ 会话
  `authorizedFolders`（CreateSessionBody 合法字段，201 接受）永不生效；`sessions/new` cwd
  孔道 503（session_focus.rs:169-178）、`restore_session_roots` 要 workspace plane
  （resourceio.rs:934-936）、`grant_root` 生产零调用方。read 结构性不可用。
- **范式差异**：旧 Node = `/api/fs/*` 任意路径 HTTP 路由（T01 残账 DIRECT 6）；Rust =
  workspace 约束工具注册 + 会话授权目录双闸。

**去向**：PPG-1/PPG-2 登记 T04/T05 MUST-ADJUDICATE（桌面预览 workspace 策略——是否桌面生成
`--config` 指向默认书桌——属产品语义决策）+ T08 终检查清单（R08 收口前二者须被修复或作为
成文预览限定正式接受）。A05 正向实绩（freeze 行 1/行 3 实窗验证、node-kernel=0 ×3、写腿帧级
红证据、14 工具面清单）见 PROGRESS-t03.md 批次 D 与证据 t03-a05-drill-08。

## 6. A06 设计：远端模式不误关服务

夹具 `scripts/rust-tauri/r08_t03_a06_remote_multi_client.mjs`：

1. 测试服务：独立合成 HOME；`--bind <本机 LAN IP>:0 --network-mode lan --home <svcHome>`（非 loopback
   事实绑定；READY 行取端口）；models.json synthetic plane；替身 Provider 同 A05。
2. 从 `{svcHome}/lingxi-service/local-token.json` 取 token → 写两份 client token 文件（0600）。
3. 两个 Electron 实例 A/B：各自独立合成 LINGXI_HOME（互不相同、不同于 svcHome），env：
   `LINGXI_DESKTOP_SERVER_RUNTIME=rust` + `LINGXI_DESKTOP_RUST_REMOTE_URL=http://<lan-ip>:<port>`
   + `LINGXI_DESKTOP_RUST_REMOTE_TOKEN_FILE=<各自>` + `LINGXI_DESKTOP_RUST_REMOTE_ALLOW_CLEARTEXT=1`。
4. 断言（连接后）：A/B 各自 HOME 无 `lingxi-service/instance.json`（未本地 spawn）；进程树全表
   恰好 1 个 lingxi-service（fixture 的 pid）；A/B 日志均经 identity 校验；诊断 mode=remote。
5. A 发真实文件任务并完成 → **quit B**（SIGTERM）→ 断言：服务 pid 存活且 uptime 连续
   （instance.json 不变）；A 再发第二个任务仍完成；进程树无第二个 lingxi-service 出现（无偷启动）。
6. quit A → 服务仍存活；harness 显式收尾 fixture 服务（fixture owned，允许）。
7. 偏离预案：本机无可用 LAN 接口（离线/沙箱）→ 退路 127.0.0.1 + remote 路径（仍验证零接触语义），
   记显式偏差并报 Lead 裁决；不静默降级为本地复用演练。

## 7. 分批计划

- **批次 A（本批）**：T03_DESIGN.md 评审 → Lead 裁决项：About 页 badge 是否做、A06 LAN 绑定 vs
  loopback 退路、远端模式 dev-only 限制。
- **批次 B**：远端模式（rust-local-service.cjs + main.cjs 分支 + 标题/诊断 + §3.2.1 host 传播链
  全链 6 处调用点）+ 单元测试
  （`tests/desktop-rust-local-service.test.mjs` 扩 remote 用例：URL 形状/token 文件/cleartext 门/
  矛盾门全抛错面；server-connection 侧 remote host→baseUrl/wsUrl 断言）。
- **批次 C**：进程树记录器 + 冻结矩阵文档（逐格代码核对，缺格才补码）。
- **批次 D**：A05 夹具 + 演练 + 证据。
- **批次 E**：A06 夹具 + 演练 + 证据。
- **批次 F**：收口门禁（typecheck、既有 vitest 无回归、证据索引、PROGRESS-t03.md 回写）→ 报 Lead。

## 8. 可度量收口标准

1. `tests/desktop-rust-local-service.test.mjs` 全绿（含 remote 新增用例），`npm run typecheck` 绿，
   既有 react vitest 无新红（HEAD-red 基线沿用 T02 记录的两个文件）。
2. A05 证据目录：进程树快照×3 + 执行轨迹 + out 文件哈希 + 诊断文本，`node-kernel=0` 硬断言。
3. A06 证据目录：双客户端日志 + 进程树快照 + 服务存活断言（quit B 后 pid 不变、任务继续）。
4. 冻结矩阵文档逐格有证据锚或代码行锚。
5. 生产默认 node 的单元断言绿；packaged+remote 抛错断言绿。

## 9. 与任务书逐条核对（自审）

| 任务书要求 | 设计落点 |
|---|---|
| 1. 显式预览启动选择，不改生产默认；就绪/版本/认证握手 | §3.1（env 门唯一、默认证明）+ 复用既有握手（§2） |
| 2. 复用 DesktopHost；Electron 对照标记；可切 R07 新宿主 | §4 |
| 3. 本地/远端分开；远端不起本地第二服务、不关远端任务 | §3.2（结构性零接触）+ A06 实证 |
| 4. 关闭/退托盘/彻底退出按冻结策略；退出超时报告残留进程 | §3.3 矩阵 + 既有 PID 报告 |
| 5. 完整进程树，区分 Electron 仅 UI 与旧 Node 内核在执行 | §3.4 记录器分类 |
| A05 真实文件任务、Rust 独立运行、不回退旧 Pi 循环 | §5（树断言 + 轨迹 + 替身剧本） |
| A06 两客户端同一服务、关一个另一个仍工作、无偷启动 | §6 |
| 禁止占位/跳断言/仅更新状态 | 全部断言硬编码进夹具；FAIL/BLOCKED 显式升级 |

## 10. 风险与缓解

| 风险 | 缓解 |
|---|---|
| fs 路由在 rust 服务未映射 → A05 卡住 | 显式 BLOCKED 报 Lead（Lead 预案已立），不绕道 |
| macOS GUI 会话限制 Electron 起窗 | 本机为桌面 macOS，先例 office-live 可起；失败则红证据+根因 |
| LAN 接口不可用（A06） | §6 退路 + 显式偏差 + Lead 裁决 |
| 明文 token 红线 | 仅演练 env 显式 opt-in + loud log；https 路径保留；token 只经 owner-only 文件 |
| dist-renderer 陈旧致起窗失败 | 失败即重建并留证据；不把构建问题伪装成功能失败 |
| 替身 Provider 形态与 r00 stub 不兼容 | 对齐 A04 三幕剧本语义自写 r08_t03 stub，保留红证据 |
| win32 进程树/ guardian 差异 | 本阶段 macOS 演练；矩阵 win32 格标注平台限定，不虚报覆盖 |
