# R08-T03 预研笔记（Lead，2026-10-22）

> 用途：T03「Electron启动Rust服务的预览模式」接手者的起点事实。非设计文档；
> 设计由接手者按任务书补齐（T02_DESIGN.md 同规格）。事实均以行号锚定源码。

## 已存在的基础设施（不要重建）

`desktop/main.cjs`（6967 行）已具备 R07 期 Rust 桌面模式：

- **选择门**：`startServer()` L1481 — `rustDesktopEnabled()` 真则走
  `startRustServer()` 并 return，否则旧 Node server 路径。门定义在
  `desktop/rust-desktop.cjs`（bundle 内 d5 模块）：读
  `process.env.LINGXI_DESKTOP_SERVER_RUNTIME`，`'node'`（默认）/`'rust'`，
  其他值抛错。**显式选择的 env 通道已存在**，T03 的「显式预览启动选择」应在其上
  加可见 affordance/标记，而非新开选择机制。
- **二进制解析**：`resolveRustBinary` — dev 模式用 `LINGXI_SERVICE_BIN` 或
  `rust/target/debug/lingxi-service`（须绝对路径、普通文件、可执行）；packaged
  走 `resources/rust-service/` + build.json 清单 + 平台/架构/魔数/sha256 验签
  （macOS 允许签名突变时比 contentSha256 + codesign --verify）。
- **启动与握手**：`startRustServer()` L1338 —
  `spawn(binary, ['--home', lingxiHome])`，解析 stdout
  `LINGXI_SERVICE_READY addr=… home=…`（超时 SERVER_INFO_FIRST_WAIT_MS），
  随后 `readOwnedRustConnection`（校验 instance.json/local-token.json：
  pid/homePath/instanceId/token/bindAddr 一致，token 文件 owner-only）+
  `verifyRustDesktopIdentity` + `publishRustDesktopConnection`。
  **就绪/版本/认证握手已存在**。
- **实例复用**：`readExistingRustConnection` — 已有实例（pid 存活 + 实例记录
  有效）则复用，`reusedServerOwned=false`；packaged 下未验证身份拒绝复用
  （`RUST_DESKTOP_EXISTING_INSTANCE_UNVERIFIED`）。
- **失败清理**：READY 失败 → SIGTERM → 等退 → SIGKILL → 等退；未确认退出则抛
  「owned process exit was not confirmed」。**退出超时报告残留的要求部分已有**。
- **分支消费**：renderer 侧 `serverNodeKind==='lingxi-service'` 已在约 20 处
  IPC handler 分流（office 预览、browser 控制拒绝等）。

## T03 相对现状的缺口（任务书逐条对照）

1. **显式预览选择不改变生产默认**：env 门已是显式；缺口在「预览」语义的可见性
   ——窗口标题/关于页/诊断里标明 preview，且生产打包默认仍为 node。
2. **本地托管 vs 远端两种模式分开**：现状只有本地 spawn/本地复用；
   `readOwnedRustConnection` 的地址校验强制 127.0.0.1/[::1]（bundle m()），
   **远端（非 loopback）模式不存在**。A06 要求远端模式不启动本地第二服务、
   不关闭远端任务 —— 需要显式 remote 连接配置路径 + 退出策略对它完全不杀进程。
   安全红线：远端模式不得放宽 token/身份校验，禁止静默降级。
3. **关闭窗口/退托盘/彻底退出按冻结策略**：需盘点 main.cjs 现有
   window-all-closed/before-quit/tray 路径在 `serverNodeKind==='lingxi-service'`
   下的行为（L2228/L4758/L4772/L4804 已有分支），补齐冻结策略矩阵。
4. **完整进程树记录**：`_lastServerSpawn` 只记自身 spawn；A05 证据要求
   进程树（Electron 仅 UI vs 旧 Node 内核仍在执行的区分）+ 执行轨迹。
5. **A05 真实文件任务**：禁用旧 Node 服务 + 预览开启下完成真实文件任务，
   Rust 独立运行、不回退旧 Pi 循环；外部协议用确定性替身（复用 a04 的
   替身剧本模式）。**前置依赖：lingxi-service debug 二进制**（Lead 已启动
   后台构建 job bash-b282ea52ff3c4d64）。
6. **A06 多客户端**：两个客户端连同一测试服务，关一个另一个仍工作。
   需要一个可被两个 Electron 实例共享的测试服务启动夹具 + 进程存活断言。

## 依赖与排序

- T02 DesktopHost（t02-core 在途）：T03 第 2 条「复用 DesktopHost」直接消费。
- 原生浏览器/PDF 若仍用 Electron 对照需标记，正式目标须可切 R07 新宿主。
- T03 不动 rust/ 产品源码（缺能力先登记，不提心吊胆改内核）。

## 红线提醒

- 真 `~/.lingxi` 永不触碰；预览演练一律合成 HOME（LINGXI_HOME 指向隔离目录）。
- 旧 Node 服务「禁用」指演练环境内不启动，不改生产启动代码的默认路径。
