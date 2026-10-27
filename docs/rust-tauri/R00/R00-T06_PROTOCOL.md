# R00-T06｜性能测量协议（冻结版）

任务：R00-T06 测量旧版本并冻结性能协议。本文件是**事前冻结**的测量条件与口径，
先于任何测量执行确定；后续修改必须留独立差异记录（PERFORMANCE_THRESHOLDS.json
的 change_log 同理），不得在看到结果后回头放宽。

比较组别（05_验收与性能协议 §6.1）：

| 组 | 内容 | 首测时点 |
|---|---|---|
| G1 旧 Electron+Node | 本任务实测对象 | R00-T06（本文件） |
| G2 旧 Electron+Rust | 同一桌面壳 + Rust 内核 | R08 |
| G3 Tauri+Rust | Tauri 宿主 + Rust 内核 | R09/R10 |

三组必须按本协议同条件测量后才可比较；Rust 侧结果在本任务时点**尚不存在**
（工作区无 `rust/` 目录，已核实）。

## 1. 硬件 / OS / 电源 / 工具链（本机实测平台）

| 项 | 冻结值（采集自测量机） |
|---|---|
| 机型 | Apple M3 Ultra（Mac15,14） |
| CPU | 28 核 |
| 内存 | 96 GiB（103079215104 B） |
| OS | macOS 27.0（Build 26A428），darwin arm64 |
| 电源 | AC 电源；lowpowermode=0（每次运行重新采集进原始数据） |
| Node（测量 harness） | v24.16.0 |
| Server 运行时 | dist-server bundle 自带 Node（见 §2） |
| Electron | 打包产物自带（Electron 42.x，electron-builder --dir） |

- 每次基准运行把 `collectHostInfo()` 快照（含当时 loadavg/uptime）写入原始数据；
  稳态样本在 load1 < 8 时采集，超出则在报告标注、不静默混入。
- **平台范围**：本协议在 macOS arm64 上**已实测**；macOS x64 / Windows x64 /
  Linux x64 是同协议**待测平台**（参数预设见 PERFORMANCE_THRESHOLDS.json），
  在各自平台按本文件同一流程实测后填充，**不得用本机数据外推或虚报**。

## 2. 构建模式（禁止 debug/release 混算）

全部性能样本取自**发布优化构建**：

| 腿 | 被测对象 | 构建 |
|---|---|---|
| Server（内核腿） | `dist-server/mac-arm64/`（`hana-server` wrapper + 自带 Node + `bundle/index.js`） | `npm run build:server`（Vite 生产 bundle） |
| Desktop（桌面腿） | `dist/mac-arm64/Lingxi.app`（asar + 生产 bundle） | `npm run build:client` + electron-builder `--dir` |

- 构建编排：`scripts/rust-tauri/r00-t06-build-release.mjs`（HEAD、构建 ID、
  bundle/seed SHA-256 写入 `raw/build-release-summary.json`）。
- **签名边界**：安装包 seed 签名使用一次性 ed25519 测试密钥
  （`scripts/artifact-keygen.mjs` 生成于 OS 临时目录，`LINGXI_SIGN_KEYSET`
  构建期替换 main bundle 内联 keyset——这是仓库文档载明的本地验证路径）。
  测试密钥**绝不**进入仓库/交付物/报告正文，也绝不作为发布密钥。此打包产物
  只用于本机性能测量，不是可发布安装包；ad-hoc 代码签名 + `SKIP_NOTARIZE=true`
  不触任何外部服务。
- 开发模式（Vite HMR、`--dev`、tsx 源码加载）一律**不采样**。
- G2/G3 比较时用各自发布产物（R08/R09 定义），禁止拿 Rust debug 构建对比本基线。

## 3. 冷 / 热启动定义与 OS 缓存边界

- **应用冷启动（cold）**：进程完全退出后间隔 **≥5s** 再启动；LINGXI_HOME 为
  稳态模板的全新克隆（磁盘状态逐样本一致）；**不声称清除了操作系统缓存**
  （macOS 无 root 不可 purge；OS page cache 状态不控制，这是显式声明的边界）。
- **热启动（warm）**：上一实例完全退出后 **≤0.5s** 内再启动（二进制/页缓存确定热）。
- 首次安装后的第一次启动（解包 seed、首启迁移）**单列**为 first-boot，不进入
  稳态启动系列。
- 比较三组时必须使用相同的间隔与模板克隆方式。

## 4. 就绪信号（冻结）

| 对象 | 就绪 = | 外部观察方式 |
|---|---|---|
| Server | `server-info.json` 出现（pid/port 匹配）**且** `GET /api/health` 返回 200 `status:"ok"` | 25ms 轮询；两项时刻分别记录 |
| Desktop | 主进程 spawn → 诊断日志出现 `app-ready` 事件行 | `$LINGXI_HOME/diagnostics/desktop-launch/renderer.log`（每次启动重置，25ms 轮询） |

## 5. 样本与统计

- 启动 / 请求 / 历史加载 / 取消 / PTY / PDF 每项 **≥30 个独立样本**（脚本默认 32）。
- 内存状态（空闲/负载中）：**10 帧 × 500ms** 采样窗（内存非启动/请求类，按窗报中位数/最大值）。
- 每项报告 n、mean、median、p95（nearest-rank）、min、max、stdev、CV，以及
  **原始逐样本数据**（JSONL，含时间戳）。
- 离散程度超过 CV>0.5 的项在报告中单独标注解释。

## 6. 内存口径（冻结）

- **主口径：进程树 RSS 求和（KB，不去重共享页）**。macOS 无 PSS；`ps -o rss=`
  逐进程读取。Chromium/Electron 多进程共享页会被重复计入——这是**保守上界**，
  对三组同样保守；每帧保留逐进程明细（pid/ppid/rss/vsz/command），
  未来如需 PSS 类口径可复算。
- **进程树归属（R00-A11）**：每帧从根 PID 集合（桌面腿 = Electron 主进程 PID +
  server PID；server 腿 = server PID）用 `pgrep -P` **重新递归遍历**后代，
  与 `ps` 快照同窗口读取；主进程与全部辅助进程（GPU/网络/utility/renderer/
  server 子进程）同窗计入。帧内消失的 PID 记入 `vanished`。
- 采样窗口固定（10×500ms=5s），窗口前后各记一帧树全量快照作归属证据。

## 7. 模型等待与本地开销分离（冻结）

- 模型协议由**确定性本地 stub**（`r00-t06-stub-provider.mjs`，127.0.0.1）提供：
  - `fast`：零附加延迟（24 chunk × 64 字符，内容只依赖 chunk 序号）——测得的
    端到端时延 ≈ 旧系统本地开销 + 网络 loopback + stub 服务时间；
  - `delayed`：固定 250ms 首字节 + 10ms/chunk（模拟模型等待，仅在需要区分
    “模型等待占比”时使用，结果单列）；
  - `hang`：输出 5 chunk 后停住（供取消测试制造“请求停留在流中”）。
- stub 逐请求记录服务端时间线（收到/首字节/末字节/输出字节/请求体规模），
  与客户端样本按 `bench-<kind>-<i>` marker 关联；客户端时延 − stub 服务时间
  = 本地开销。
- **真实供应商测量单列且本轮为 0 次调用**（无真实账号、无付费 API、无外发）；
  其百分比绝不与本地改善混算。

## 8. 数据集（全部合成，冻结）

| 数据 | 规模 | 生成 |
|---|---|---|
| 长会话 | 1000 条消息 JSONL（`bench-long-1000.jsonl`） | `scripts/lib/history-read-fixture.mjs` `buildLongRunFixtureBytes(1000)`（与既有基准同语料，sha256 入审计） |
| 短会话 | 20 条消息 | 同上（n=20） |
| 流式输出 | 24 chunk × 64 字符（≈1.5KB 文本） | stub 固定表 |
| PTY 大输出 | node 内核打印 8192 行 × 128B（127 字符 + \n）= 1 MiB | 确定性脚本（`yes <line> \| head -n 8192`；R1 修复 F06：首轮夹具误为 48B/行≈0.4MB，已按本协议值提到 128B/行重测，见 §12） |
| 浏览器页面 | 每页 ≈512KB 确定性 HTML（本地 stub 提供） | stub `/page/<n>` |
| PDF 输入 | 目标 200KB 确定性 HTML → Chromium printToPDF；实际 **239,073 B**（`unit.repeat` 按字符数取整 + 中文多字节，实测即此值，如实注明不缩小） | 测量脚本生成 |
| LINGXI_HOME | 每样本全新克隆（pristine 模板含 provider-catalog→stub、agents/lingxi/config.yaml→bench-fast、preferences setupComplete、合成会话）。**测量条件：config.yaml 的记忆总开关被 `r00-t06-fixture.mjs` 重写为 `enabled: false`**——关闭 memory-ticker 后台对合成会话的滚动摘要/修复（后台写会引入与被测负载无关的噪声）；其余配置逐字保留 example | `r00-t06-fixture.mjs` |

会话并发：流式/取消样本逐个新会话（30 个并发历史在同一稳态 server 上顺序驱动）；
浏览器多实例 = 5 个会话各持一个真实 WebContentsView（产品上限 MAX_INSTANCES=5）。

## 9. 工作负载矩阵（05 §6 最低负载的落地）

| 负载 | 实现 | 计时口径 |
|---|---|---|
| 启动可交互 | desktop 腿 cold/warm | spawn→app-ready |
| 服务就绪 | server 腿 cold/warm | spawn→server-info+health |
| 空闲内存 | 两腿各 10 帧窗 | 树 RSS 中位数 |
| 1000 条长会话加载 | 真实 `GET /api/sessions/messages?limit=50`（首屏/全量翻页/ETag 304） | 请求总时延 + TTFB |
| 持续流式输出 | 真实 WS `/ws` prompt→stub fast 流 | 首增量、run 结束 |
| 取消清理 | stub hang 流中 WS `abort` | abort→abort_result；abort→流停 |
| PTY 大输出 | 真实 node-pty backend + TerminalSessionManager + terminal-ws-bridge + TerminalOutputStream（组件与产品同源；边界：未经 WS chat 路由——旧系统主聊天会话经 mcp_call 调 run_code 返回 "no active session"，见报告实测事实） | spawn→首次交付→全部交付；transcript 落盘字节与完成标记核对 |
| 浏览器多实例 | agent `browser` 工具→真实 WebContentsView×5 | 工具往返 + 整树内存窗 |
| PDF 转换 | 打包二进制 `--hana-office-html-to-pdf`（Chromium printToPDF） | spawn→exit + %PDF 校验 |
| 分发占用 | du：Lingxi.app / dist-server / seed 归档 | 字节 |

## 10. 隔离与安全边界

- 每样本 LINGXI_HOME = OS 临时目录全新克隆；`HOME` 一并指向临时目录
  （杜绝 `~/.lingxi` 真实指针）；server 以 `LINGXI_PORT=0`（随机端口）、
  合成 token（`not-a-secret`）运行。**不读真实用户数据。**
- 桌面腿更新检查经产品自带 env 开关 `LINGXI_UPDATE_FEED_URL` 指向本地死端口，
  全程零真实外呼；模型全部走本地 stub。
- 测量结束清理本脚本启动的全部进程树（SIGTERM→等待→必要时 SIGKILL 兜底），
  并记录残留 PID；不清理任何用户数据。

## 11. 可重复性

```bash
node scripts/rust-tauri/r00-t06-build-release.mjs            # 重建发布优化产物
node scripts/rust-tauri/r00-t06-bench-server.mjs             # server 腿全阶段
node scripts/rust-tauri/r00-t06-bench-pty.mjs                # PTY 腿（真实组件栈直驱）
node scripts/rust-tauri/r00-t06-bench-desktop.mjs            # 桌面腿全阶段
node scripts/rust-tauri/r00-t06-summarize.mjs                # 汇总 BASELINE_BENCHMARK.json
node scripts/rust-tauri/r00-t06-probe-fixed-prefix.mjs       # 固定前缀证据探针（R1-F04）
node scripts/rust-tauri/r00-t06-repair-build-summary.mjs     # 盘上产物指纹重算（R1-F02，不重建）
```

原始数据：`artifacts/rust-tauri/R00/T06/raw/{server,desktop}/`（JSONL 逐样本 +
进程树帧 + stub 请求日志 + 构建摘要）。重跑会生成新 RUN_ID 目录，不覆盖旧样本。

## 12. 变更记录

本协议为事前冻结文件；以下修改均留独立记录（R1 验收报告
`R00-T06_REVIEW_R1.md` F02/F06/F07 → 修复轮 `R00-T06_R1_REPAIR.md`）：

| 轮 | 变更 | 理由 |
|---|---|---|
| R1 修复（2026-09-24） | §8 PTY 行注明实际夹具 128B/行（首轮误为 48B/行≈0.4MB，低于冻结值）；按任务书 05 §6.1「冻结后不得因结果缩小负载」，将夹具提到协议值并以真实组件栈重测 32 样本（新 run `pty-2026-09-24T01-45-52-350Z`，outputBytes=1,056,849B ≥1MiB），旧 0.4MB run 数据保留作过程记录 | F06 |
| R1 修复（2026-09-24） | §8 PDF 输入注明实际 239,073B（目标 200KB 按字符取整 + 中文多字节的确定结果，非事后缩小） | F06 |
| R1 修复（2026-09-24） | §8 LINGXI_HOME 行补记「记忆总开关 enabled:false」测量条件（fixture.mjs 既有行为，此前未入协议文档） | F07b |
| R1 修复（2026-09-24） | §11 补 bench-pty / 固定前缀探针 / 产物指纹重算三条命令 | 可重复性补全 |
| R1 修复（2026-09-24） | 构建产物目录约定 darwin→mac 在 build-release 与 bench-desktop size 阶段全部落实（脚本修复，见 R1_REPAIR）；基线 build 块指纹自此真实 | F02 |
