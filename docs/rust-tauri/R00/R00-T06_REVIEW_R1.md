# R00-T06 独立验收报告 R1｜测量旧版本并冻结性能协议

- 审查者：REVIEWER-R00-T06-R1（一次性独立验收代理；非执行者，不做修复，不进入下一 Task）
- 审查日期：2026-09-24（本地 +0800）
- 对象任务：R00-T06（验收 R00-A11、R00-A12，均 REQUIRED）
- 审查方式：源码/脚本/协议/阈值/基线/原始样本全量阅读 + 从真实生产入口追踪 + 统计独立重算 + 一条关键生产链真实隔离复测（不只读日志）

## 0. 结论

**VERDICT: PASS**

两个 REQUIRED 验收（R00-A11 进程树测量不漏算、R00-A12 阈值先于实现结果）经对抗性核验与真实复测均成立；三项必须交付齐备且与原始数据自洽。发现 7 项缺陷（F01–F07，其中 4 项 MEDIUM），全部为报告/文档/构建指纹层的溯源与传播问题，不动摇测量真实性、不改变任何门槛判定结果，不构成 BLOCKING；但均须在修复轮处理（PERFORMANCE_THRESHOLDS.json 的修正须走其自身 change_log 治理，不得借机放宽门槛值）。无需重测性能腿；F04 需一次轻量补采。

## 1. 坐标核对（全部通过）

| 项 | 核对结果 |
|---|---|
| HEAD | `64c302ec482888222833136d0d20c64d4cbb350b` = 任务指定 Task base ✓（`git rev-parse HEAD`） |
| 分支 | `codex/rust-tauri-migration` ✓ |
| 前置 R00-T05 | 总控账本 status=DONE、task_commit_sha=`64c302ec…`、A09/A10=PASS ✓ |
| 候选 | 75 个新增文件（不含报告与总控账本），与 `git status` 展开后集合一致 ✓ |
| 候选聚合 SHA-256 | 按「路径排序的 `shasum -a 256` 行（哈希+双空格+路径+换行）拼接后整体哈希」重算 = `e0a49021f37cc50e1b24c61f05ffadca3977842810a64132b8a59149941495ee` ✓（与账本/报告声明一致） |
| 执行报告 | `docs/rust-tauri/R00/R00-T06_REPORT.md` SHA-256 = `d2ae32823482a07c9c661ca20d3576213e96643e8c3a67eddb1be3937e0fd0f5` ✓ |
| 报告 §8 逐文件哈希 | 75/75 与工作区实际文件逐一比对，0 不符；集合与候选集相等 ✓ |
| 工作区 | 仅账本既有未提交修改（总控维护，未触碰）+ 本任务 75 文件 + 本报告；无生产代码改动 ✓ |

## 2. 审查环境与实际执行

- 环境：同一测量机 Apple M3 Ultra（Mac15,14）/ 28 核 / 96GiB / macOS 27.0 / darwin arm64（与冻结协议 §1 一致，复测可比）；Node v24.16.0；Python 3.14。
- 任务书完整性：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23` 内 32 文件 `shasum -a 256 -c MANIFEST.sha256` 全部 OK（任务书未被篡改）。已读 00/01/02/03/04/05/06/90/91、R00 阶段书 T06 全文、task-catalog R00-T06、acceptance-catalog R00-A11/A12、stage-index R00。
- 主要命令与退出码：
  - 统计独立重算（Python，从 raw JSONL 直接重算 n/median/p95/min/max/stdev/CV）：server/desktop/stream/cancel/pdf 各 32 样本与 summary、BASELINE_BENCHMARK.json **完全一致**（例：cold→info 1170.75/1195.92、health 1190.74/1206.84、desktop app-ready 9227.17/9375.29、pdf 812.06/830.67）。
  - `node --check scripts/rust-tauri/r00-t06-*.mjs`（10 文件）：全部通过。
  - **真实复测**：`node scripts/rust-tauri/r00-t06-bench-office-pdf.mjs --samples 2` → 退出码 0（详见 §4）。
  - 产物指纹：`shasum -a 256 dist-server/mac-arm64/bundle/index.js` = `5095cc7f…`（与 server 腿 summary 记录一致）；`app.asar` = `7b8478ab…`（与 build-release-summary 一致）；dist/dist-server mtime = 2026-09-23T21:37–21:38Z，即**原始被测构建仍在盘、未被重建**。
  - 敏感信息扫描：75 文件无真实密钥/令牌/私钥模式命中；仅合成 `not-a-secret` 类 token（68 处，已声明）。
  - 复测后清理：删除复测自产 3 个 raw 文件（目录回到 38 文件基线），`ps` 核查 0 个 Lingxi/hana-server/office 残留，`/tmp` 无 lingxi-r00t06-office-* 残留。

## 3. 生产链真实性核验（从真实入口追踪，非读日志）

| 声明 | 生产源码证据 | 裁定 |
|---|---|---|
| `--hana-office-html-to-pdf` 为真实产品入口 | `desktop/bootstrap.cjs:218` 分发 → `desktop/src/office-pdf-helper.cjs` | ✓ |
| office 插件 helper env 合同 | `plugins/office/lib/html-to-pdf.ts:36-37`：`LINGXI_OFFICE_PDF_HELPER_EXEC` → 兜底 `LINGXI_DESKTOP_EXEC_PATH` | ✓ |
| 产品桌面内 server→helper 合同 | `desktop/main.cjs:1826` 注入 `LINGXI_DESKTOP_EXEC_PATH=process.execPath`、`main.cjs:1854` 注入 `LINGXI_RENDERER_DIST`（打包态指向已激活 renderer） | ✓（bench 用 env 变量指向同一打包二进制，合同一致） |
| 浏览器链 server→Electron | `server/index.ts:1166-1175` `/internal/browser` 独立 WS；`desktop/main.cjs:3282-3283` browser-cmd → `WebContentsView` | ✓ |
| MAX_INSTANCES=5 | `lib/browser/browser-manager.ts:48` | ✓ |
| 主聊天 run_code 不可用（PTY 直驱理由） | `core/session-coordinator.ts:2201` buildTools 调用块无 runtimeSessionRef/getSessionPath → `core/engine.ts:4033` 附近降级 `() => null` → `lib/tools/run-code-tool.ts:226` "no active session — run_code kernels are session-scoped" | ✓（报告 §7 事实 1 成立） |
| browser 负载真实走链 | `raw/desktop/browser-run-…22-47-51…jsonl` 5 行，navigate 5/5 记录 `toolNames=['mcp_call']`、0 errorEvents；树帧 6 进程含新 renderer（同源共享） | ✓ |
| office 链 helper 入树 | `summary-office-…23-29-35…json` `treeDuringFirstConvert`：树 1→5→1 进程，含打包 Lingxi helper 主进程 + 3 个 Lingxi Helper（GPU/网络/Renderer），RSS 峰 1,033,888KB，转换后回落 1 进程 | ✓ |
| 退出不漏算 | 桌面 quit 前 `snapshotTree` 捕获整树（防 reparent）后核对全部 PID 消失：32/32 `forced=false`、leftover 空（冷/热/首启/final 全查）；server finalShutdown procsAfter=0；office 退出 102.3ms；PTY close 后 0 残留 helper | ✓ |
| 隔离与零外呼 | 每样本全新 LINGXI_HOME/HOME 重定向（raw 行内 home 路径均为 `/var/folders/.../lingxi-r00t06-*` 临时目录）；`LINGXI_UPDATE_FEED_URL` 指向本地死端口；stub 全部 127.0.0.1；真实供应商 0 次调用 | ✓ |

## 4. R00-A11 裁定：PASS（进程树测量不漏算）

- 前置「基线启动浏览器及 PDF helper」：成立。浏览器 = 真实 WebContentsView×5（经 WS prompt → 模型替身 tool_use → mcp_call → browser 工具 → BrowserManager → /internal/browser → Electron，navigate n=5 全成功）；PDF 有两套证据：打包二进制直测 32/32（%PDF 头、确定性 174,571B）+ 完整产品链 32/32（prompt→tool_end median 851.8ms，SessionFile 落盘，输出确定性 175,218B）。
- 「主程序与全部辅助进程均计入，采样窗口一致」：`r00-t06-lib.mjs` 每帧 `pgrep -P` 递归重走 + `ps` 同窗读数；桌面腿双根（Electron 主 + desktop-owned server）；每帧逐进程 pid/ppid/rss/vsz/pcpu/etime/comm + 帧时间戳落盘（browser-tree 10 帧跨 5.7s、office 8×300ms 转换窗均实证）。桌面空闲 5 进程、开 5 视图 6 进程逐帧稳定；server 空闲 451,504KB 与桌面腿树内 server 455,152KB 交叉吻合。
- 32 样本原始数据、统计重算、build/run 关联：startup JSONL 64 行（冷/热各 32）逐行重算与 summary/baseline 零偏差；`serverBundleSha256=5095cc7f…` 逐 run 记录且与当前盘上构建字节一致；office/pty/pdf/cancel/history 各 32/32/32/32/32(+3 walk) 行齐全。
- **独立复测（不只读日志）**：本人实际重跑 office PDF 产品链（2 样本）：退出码 0，`/api/plan-mode` 切 operate 成功，2/2 `toolStatus=succeeded`、输出 175,218B 与候选确定性一致，tool_end 875–923ms（与候选 median 851.8ms 同量级），**转换期间 server 树 1→4→5→1 进程、helper 及 GPU/网络/Renderer 子进程入树后退出**——A11 核心机制独立重现。全部使用隔离临时 HOME + 本地确定性 stub，复测产物与进程已清理。
- 边界（按本 Task 规格可接受，均已如实登记）：浏览器视图为 Electron 进程内 WebContentsView（产品形态，无独立 helper）；同源 5 视图共享 1 渲染进程（真实 Chromium 行为）；PTY 为真实组件栈直驱（node-pty/TerminalSessionManager/terminal-ws-bridge/TerminalOutputStream 同源组件，未经 WS chat 路由——理由已获源码级证实，见 §3）；office 链 server 为 standalone spawn（env 合同与产品一致并披露）。

## 5. R00-A12 裁定：PASS（阈值先于实现结果）

- 前提「尚未产生 Rust 性能数据」：**本轮独立核实**——工作区无 `rust/` 目录、无 Cargo.toml、无任何 Rust 侧测量数据；G2/G3 未实现。冻结先于 Rust 结果成立（时序瑕疵见 F05，不推翻该前提）。
- 「所有性能门槛有固定值、适用环境和理由」：`docs/rust-tauri/PERFORMANCE_THRESHOLDS.json` 8 项（取消受理 p95≤250ms、进程树清理 3s/5s、本地业务/历史读取 p95≤24ms（=max(2×基线p95, 基线p95+20ms) 事前噪声容限公式）、固定前缀 ≤10,620B、长时资源增长 2h/1000 任务斜率判据、迁移收益预选分发占用 ≥25%、桌面启动 p95≤19.4s、空闲树内存 ≤1.5×基线）均具备：数值+单位+比较方向、applies_to（G2/G3）、scope（平台/构建/负载/口径）、baseline_g1、noise_tolerance、rationale+source、judgment_rules（禁丢弃离群、禁 debug/release 混算、模型等待分离、平台诚实、失败诚实）、change_log rev1。与任务书 05 §6.2 六类建议值一一对应且未放宽。
- 平台诚实：darwin-arm64=MEASURED；darwin-x64/win32-x64/linux-x64=PENDING_MEASUREMENT，明确禁止外推——未把未测平台标为通过 ✓。
- 指标与协议 §6/§5 原义一致（RSS 树求和不去重口径、nearest-rank p95、10×500ms 内存窗、≥30 样本）；G2/G3 可在同条件使用——协议+10 个脚本可重复，本人实际重跑其一验证了可重复性。
- 附带条件：F01（baseline_g1 桌面 teardown 引用了被弃 run 的数字）、F04（10,620B 无证据）、F05（frozen_at 不实）须按 change_log 治理修正，门槛数值本身不得变动。
- 「长时资源增长 G1 基线 NOT_RUN_THIS_ROUND（R10 补测）」：T06 步骤列举的负载不含 2h 长跑，05 §6.1 将其定为门槛行、§6.2 允许 R00 冻结规则而非执行；文件已如实登记并给出 R10 补测承诺——按本 Task 规格可接受（观察 O3），不构成把 NOT_RUN 当 PASS。

## 6. 交付物核验

| 任务书要求 | 状态 |
|---|---|
| BASELINE_BENCHMARK.json | ✓ 存在于 `artifacts/rust-tauri/R00/T06/`，全部指标与 raw 自洽（独立重算验证）；build 块指纹缺陷见 F02/F03 |
| PERFORMANCE_THRESHOLDS.json | ✓ 存在于 `docs/rust-tauri/`，结构完整（见 §5） |
| 原始性能记录 | ✓ raw/{server,desktop} 51 文件：逐样本 JSONL、进程树帧、stub 请求日志、构建摘要、各腿 summary；中间失败 run 一并保留作过程记录，最终 run 与过程 run 区分明确 |

## 7. 发现（F=缺陷，O=观察）

### F01｜MEDIUM｜执行报告 §4 混用被弃中间 run 的统计数据
- 证据：报告 §4 表称最终 server run=`run-2026-09-23T23-12-26-989Z`，但 5 行（冷启动→health 1188.4/1211.3/1151.8/1211.8、SIGTERM 93.3/95.5/92.1/96.5、ETag304 1.44/1.94/1.14/2.01、短会话 1.31/4.40/1.04/5.35、取消流停 4.16/5.81/1.97/5.95）**逐位匹配中间 run `run-2026-09-23T22-05-11-181Z` 的 summary**，与最终 run（1190.7/1206.8…、93.59/95.63…、1.52/4.53…、1.22/1.56…、2.74/6.22…）不符；BASELINE_BENCHMARK.json 为最终 run 值（正确）。另桌面退出行 CV 写 0.005（实际 0.046）。
- 根因：报告初稿基于 22-05 run 起草，改用最终 run 后仅更新了部分行（好坏方向混杂，无选择性挑数迹象）。
- 同类范围：仅报告 §4 表格；基线 JSON、阈值门槛值不受影响。
- 后果：读者以报告为据会引用错 run 的数字；阈值文件亦有一处同源错误（见 F01b）。
- 修复要求：报告 §4 全表改为最终 run 数值（或每行标注来源 run）；CV 笔误更正。
- 必须重跑：无（数据已在 raw）。
- **F01b（同根因，登记于同一修复）**：`PERFORMANCE_THRESHOLDS.json` `process_tree_cleanup.baseline_g1.desktop_teardown_all_gone_ms` = 258.3/260.7 实为被弃桌面 run `run-2026-09-23T22-23-53-802Z` 的数值（note 却引用最终 run 22-47-51），其 noise_tolerance「CV≈0.004」同源失实（最终 run CV=0.046、stdev≈12ms）。门槛值（3000/5000ms）不受影响。修正须按该文件规则记 change_log（数值溯源更正，非放宽）。

### F02｜MEDIUM｜build 脚本 macOS 路径 bug → 冻结基线 build 块指纹空值
- 证据：`r00-t06-build-release.mjs:148,150` 拼 `dist-server/darwin-arm64`、`dist-server-artifact/darwin-arm64`（未做 darwin→mac 归一化；bench-server.mjs:41、bench-office-pdf.mjs:31 均正确归一化），目录不存在 → `dirSizeBytes=0`、sha=null → `build-release-summary.json` serverBundle.bytes=0/bundleSha256=null、seedArchiveDir 空 → 传播进 `BASELINE_BENCHMARK.json` build 块（serverBundleSha256=null/serverBundleBytes=0/seedArchive.bytes=0）。bench-desktop size 阶段（:509-510）同样拼错路径，du 失败即静默省略（最终 summary 仅剩 lingxiApp 项）。
- 根因：产物目录命名约定（darwin→mac）只在部分脚本落实。
- 后果：报告 §2/§3 「bundle SHA-256 见 build-release-summary」指向落空；基线交付物的构建指纹块为空。**不阻断**：真实指纹在 server 腿 summary（`serverBundleSha256=5095cc7f…`），且本人核实当前盘上 `dist-server/mac-arm64/bundle/index.js` 与之字节一致、dist 产物 mtime 即原始构建时刻（2026-09-23T21:38Z）——「HEAD 上正式生产 bundle」的实质主张可验证成立。
- 修复要求：build-release.mjs 与 bench-desktop size 阶段补 darwin→mac 归一化；重生成 build summary 并把真实指纹写回基线 build 块（或基线直接引用 server 腿 summary 指纹）；报告 §2/§3 指针更正。
- 必须重跑：构建盘点步骤（可基于现有产物重算，无需重新测量性能腿）。

### F03｜LOW｜分发占用双口径并存未标注
- 证据：`BASELINE_BENCHMARK.json` `build.desktopAppBytes=971,247,121`（构建时 statSync 逻辑字节求和）vs `desktopLeg.distributionSize.lingxiApp=812,728,320`（桌面 run `du -sk` 分配字节，协议 §9 冻结口径）。两者相差 ~158MB 为 APFS 分配/逻辑差异，同一文件内无口径标注。
- 后果：G3 对照若取错字段会算错 25% 收益的基数。
- 修复要求：在基线 build 块标注口径（逻辑字节 vs du 分配字节），或统一为 du 口径；阈值文件 lingxi_app_bytes（du 口径）保持不变。

### F04｜MEDIUM｜固定前缀基线 10,620B 无 raw 证据，且与请求日志表面矛盾
- 证据：全部 artifacts 中 grep 不到 `10620`/`systemPrompt` 任何记录；阈值 `fixed_prefix_overhead.baseline_g1.system_prompt_bytes=10620` 注明取自「会话缓存契约诊断」，该诊断输出未落盘。同时 stub journal 显示：server 腿 stream 请求体仅 **666B**（不可能含 10.6KB system 消息；toolCount=None）、cancel 请求体 **28,590B**、桌面腿浏览器请求 toolCount=7（含工具数组）——「system prompt 字节数」这一指标在不同会话类型下测量基础不明确。
- 后果：G2/G3 执行该门槛（≤10,620 bytes）时缺少可复现的测量命令；当前证据无法复核基线数。
- 修复要求：补采并落盘「会话缓存契约诊断」原始输出（明确会话类型与命令），或在阈值中把该指标改定义为可由 stub journal 复算的口径（如 operate 会话请求体字节数），随 change_log 记录指标口径澄清（门槛数值方向不变）。
- 必须重跑：一次轻量补采（不涉性能腿重测）。

### F05｜MEDIUM｜阈值文件 frozen_at 时间戳不可信
- 证据：`frozen_at=2026-09-23T23:15:00.000Z`；实际文件写入时刻为 23:20:53Z（本地 mtime 2026-09-24T07:20:53+0800），且内容引用的 run-23-12-26 cancel/history/idle 数值在 23:15 时尚未产出（该 run summary 23:17:18Z 才写入）——23:15 对最终内容不可能成立。
- 后果：作为「时序证明」的字段本身不准确，削弱 A12 证据链的可信度（尽管实质前提独立成立：见 §5）。
- 修复要求：change_log 修正为实际冻结时刻（或补记「内容定稿时刻」），并说明 23:15 的来历；门槛值不变。
- 必须重跑：无。
- 证据强度边界声明：本审查无法证明 23:15–23:20 之间不存在更早草稿；但「冻结先于 Rust 结果」不依赖该时间戳（Rust 数据至今不存在，可随时独立复核）。

### F06｜LOW｜冻结协议数据集描述与实际夹具不符
- 证据：协议 §8 写 PTY 负载「8192 行 × ~128B ≈ 1MB」，实际 lineBytes=48 → 8192×49≈401,441B（raw outputBytes 证实）；bench-pty.mjs:43 自身注释「~76B ≈ 0.6MB」亦错。PDF 输入协议写「≈200KB」，实际 htmlBytes=239,073B（中文多字节字符）。
- 后果：冻结负载与执行负载不一致，违反「冻结后不缩小负载」的字面要求（本例为文档数字失准而非结果导向缩水，PTY 计时数据本身真实）。
- 修复要求：协议按实际值更正（留差异记录），或按协议 1MB 重建夹具并重测 PTY 腿（二选一，前者影响小）。

### F07｜LOW｜杂项文档失准
- (a) bench-server.mjs 头注释仍描述「pty.bigoutput 经真实 agent 工具链 run_code」阶段——该阶段已移至 bench-pty 并改直驱（协议 §9 已登记，注释未同步）；(b) 协议 §8 LINGXI_HOME 模板清单未记载 fixture.mjs:77 关闭 memory 后台开关的测量条件（代码内有注释与理由）；(c) 报告 §3 「SHA-256 见 build-release-summary」错位（归 F02 一并修）。

### 非阻断观察（不需修复裁决，供后续阶段输入）
- **O1**：cancel 相每样本产生 2 次 hang 请求（journal 64/32）且请求体 28,590B vs stream 666B——旧系统取消/重试与提示词组装的真实行为，供 R03/R05 权限与取消语义冻结参考。
- **O2**：每次 server 启动伴随 2 次无 marker 模型调用（msgCount 2/4，boot 即发）——属真实启动路径组成，G1 启动指标已一致计入。
- **O3**：长时资源增长 G1 基线 NOT_RUN_THIS_ROUND（R10 补测）、其余三平台 PENDING_MEASUREMENT、安装包（dmg/nsis）尺寸未测（--dir 目录口径替代并披露）、真实供应商 0 次调用——均按规格如实登记为边界/待测，非 REQUIRED 缺失，未伪装为通过。
- **O4**：浏览器 start 工具仅首会话记录到 tool_start（navigate 5/5 全量真实）；`startToolEndMs` 以 n=1 如实入库，未夸大。
- **O5**：A11/A12 相关测试未 mock 待测对象：stub 仅替代外部模型协议（任务书 §1 允许层），被测的 server/桌面/工具链/进程树/存储全为真实路径；未发现静默降级或断言永真。

## 8. 授权与安全合规

- 无真实用户数据读取/迁移（全部合成夹具 + 临时 HOME 重定向）；无真实供应商/付费 API/真实外发（stub 127.0.0.1、更新检查指向死端口）；无越权进程/权限操作；测试自启进程全部清理（候选侧 32/32 退出无残留 + 本审查复测后清理核验）。
- 测试签名密钥为一次性 ed25519、仅存 OS 临时目录，未入仓库/报告/交付物（构建产物中仅公钥 keyset，符合 AGENTS.md 红线「测试签名 key 不得当正式发布 key」）。
- 本审查未修改任何产品源码、脚本、阈值、基线、协议、测试实现或总控账本；仅新增本报告；未 commit/push/PR/tag/release；未派生子智能体。

## 9. 修复轮要求汇总（不改变 PASS 结论的前提条件）

1. F01/F01b：报告 §4 与阈值 `process_tree_cleanup.baseline_g1`/`noise_tolerance` 改回最终 run 数值（change_log 记录，门槛值不变）。
2. F02：build 脚本路径归一化 + 基线 build 块真实指纹回填 + 报告指针更正。
3. F03：分发占用口径标注。
4. F04：system prompt 指标证据补采或口径重定义（change_log）。
5. F05：frozen_at 修正为实际时刻（change_log）。
6. F06/F07：协议 PTY/PDF 数据集数字与杂项注释更正。

验收到此停止；不执行修复，不进入 R00-T07。
