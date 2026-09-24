# R00-T06 报告｜测量旧版本并冻结性能协议

任务：R00-T06（测量旧版本并冻结性能协议）｜验收：R00-A11、R00-A12（均 REQUIRED）
执行者结论：**READY_FOR_REVIEW**（实施与自测完成；独立验收由总控另行指派，本报告不自称 PASS）

> **修复轮标注（R1）**：本报告已在 R1 独立验收（VERDICT: PASS，附 F01–F07 须修复项）后由修复代理
> REPAIR-R00-T06-R1 更新。F01–F07 全部处理完毕；逐项内容、命令与证据见
> [R00-T06_R1_REPAIR.md](R00-T06_R1_REPAIR.md)，R1 验收报告原样保留作历史证据。
> 脚本/数据/协议在本轮有实际变更，R1 的 PASS 不再覆盖当前候选，需新验收轮复核。
> 本轮修正均为数值溯源/证据补采，全部门槛判定值除 fixed_prefix 10,620→10,626B
> （证据驱动更正，见 §6 与 R1_REPAIR F04）外一字未动。

## 1. 范围与源码基线

- 只执行 R00-T06（含 R1 修复轮）。未创建分支/worktree，未 commit/push/PR/tag/release，未改生产代码。
- 基线 HEAD = `64c302ec482888222833136d0d20c64d4cbb350b`（分支 `codex/rust-tauri-migration`），与任务指定 Task base 一致；本任务无提交，HEAD 不变。
- 前置核验：总控账本 R00-T05 = DONE / PASS / task_commit `6f58b9351` 已推送（本地与 `origin/codex/rust-tauri-migration` 一致）。
- 工作区：`docs/rust-tauri/ORCHESTRATOR_PROGRESS.json` 的预先存在未提交修改原样保留（总控维护，未触碰）；本任务新增文件见 §8。
- Rust 结果核验：冻结阈值时（rev1 首次写入时刻 2026-09-23T23:20:53Z，依据文件系统 mtime，见阈值文件 `frozen_at_basis` 与 change_log rev2——R1-F05 更正：原 23:15:00Z 为草拟名义时刻，早于其引用数据的实际产出时刻 23:17:18Z，不可能成立）工作区**不存在 `rust/` 目录、不存在任何 Rust 性能数据**；R1 修复轮（2026-09-24T01:5xZ）再次独立复核仍不存在——R00-A12 的"先于实现结果"前提持续成立。

## 2. 交付物

| 交付 | 位置 |
|---|---|
| BASELINE_BENCHMARK.json | `artifacts/rust-tauri/R00/T06/BASELINE_BENCHMARK.json`（G1 全部指标+统计+环境+构建指纹（R1 修复后为真实指纹与双口径）+原始数据指针） |
| PERFORMANCE_THRESHOLDS.json | `docs/rust-tauri/PERFORMANCE_THRESHOLDS.json`（8 项门槛：固定值/单位/适用平台与构建/理由/噪声容限/判定规则/变更记录；change_log 至 rev2） |
| 原始性能记录 | `artifacts/rust-tauri/R00/T06/raw/`（R1 修复轮后 66 个 .json/.jsonl = server 42 + desktop 23 + build-release-summary 1：逐样本、进程树帧、stub 请求日志、构建摘要、各腿 summary、固定前缀探针证据；含首轮过程 run 与 0.4MB PTY 旧夹具数据。原报告此处写 51 为初稿时点计数，office 补测 run 其后加入） |
| 冻结测量协议 | `docs/rust-tauri/R00/R00-T06_PROTOCOL.md`（硬件/OS/电源、构建模式、冷热启动定义与 OS 缓存边界、内存口径、样本统计、模型等待分离、负载矩阵、隔离与清理；§12 变更记录） |
| 可重复测量脚本 | `scripts/rust-tauri/r00-t06-{build-release,lib,stub-provider,fixture,bench-server,bench-pty,bench-desktop,bench-office-pdf,summarize,smoke,probe-fixed-prefix,repair-build-summary}.mjs` |
| R1 修复轮记录 | `docs/rust-tauri/R00/R00-T06_R1_REPAIR.md`（F01–F07 逐项处理、命令、证据、候选差异） |
| 本报告 | `docs/rust-tauri/R00/R00-T06_REPORT.md` |

放置说明：阈值文件放 `docs/rust-tauri/` 顶层（R08/R10 测量时的稳定消费路径，与总控账本同层）；协议文档按 R00 惯例放 `docs/rust-tauri/R00/`。`--help-arm64` 空目录（dist-server 下，2026-09-22 既有）与本任务无关，未动。

## 3. 测量条件（冻结要点，全文见协议文档）

- **硬件/OS/电源**：Apple M3 Ultra（Mac15,14）28 核 / 96 GiB / macOS 27.0（26A428）/ darwin arm64 / AC 电源 / lowpowermode=0；每 run 采集 loadavg（实测期 load1 约 2–4，远低于 28 核）。
- **发布优化构建**（禁止 debug/release 混算）：server 腿 = `npm run build:server` → `dist-server/mac-arm64` 生产 bundle（bundle/index.js SHA-256 = `5095cc7ff74f60bfe0ca02332ad9455aabdb9abb065e3b82135d36ec76e6081a`，与最终 server run 逐样本记录的 serverBundleSha256 字节一致互证）；桌面腿 = `npm run build:client` + electron-builder `--dir` → `dist/mac-arm64/Lingxi.app`（app.asar SHA-256 = `7b8478abcb7e3b4e239b3726236e616f0a6979ae83157399e94282be477654a1`）。构建指纹与双口径大小在 `raw/build-release-summary.json` 与 BASELINE build 块（R1-F02 修复：原 build 脚本 darwin→mac 归一化缺失致指纹空值，产物未重建、自盘上原始被测产物重算，守卫见 R1_REPAIR）。seed 签名用**一次性 ed25519 测试密钥**（`scripts/artifact-keygen.mjs` 生成于 OS 临时目录 + `LINGXI_SIGN_KEYSET` 构建期替换内联 keyset——仓库文档载明的本地验证路径；测试密钥不进仓库/报告/交付物，本产物只用于测量、非可发布安装包）。electron-builder 因本机无外网改用 `--config.electronDist=node_modules/electron/dist`（同一 42.8.1 解包目录，app-builder-lib 原生支持）；公证 `SKIP_NOTARIZE=true`、ad-hoc 签名，全链零外呼。
- **冷/热启动**：cold = 整树退出后 ≥5s、稳态模板全新克隆；warm = 退出后 ≤0.5s。**不声称清除 OS 缓存**（无 root 不可 purge，协议显式声明边界）；首启解包（793MB seed→artifacts）单列 first-boot 不入稳态系列。
- **内存口径**：macOS `ps -o rss=` 逐进程、**进程树求和不去重**（保守上界，Chromium 系共享页重复计入），每帧保留逐进程明细供未来换口径复算；无 PSS（macOS 不可得），不跨平台混算。
- **进程树归属（A11）**：每帧从根 PID 集合 `pgrep -P` 递归重走 + ps 同窗读数；桌面腿双根（Electron 主进程 + server PID）；退出清理以"SIGTERM 前捕获整树 → 全部 PID 消失"核对（防 reparent 漏算）。
- **模型等待分离**：确定性本地 stub（127.0.0.1，openai-completions SSE，fast/delayed/hang 三模式，逐请求记录服务时间线）；真实供应商**本轮 0 次调用**。
- **数据**：全合成（1000/20 条消息会话与仓库既有基准同语料、PTY 8192 行 × 128B = 1 MiB/样本（R1-F06 修复后夹具，旧 0.4MB 夹具数据保留为过程记录）、512KB 本地页面、目标 200KB（实际 239,073B，中文多字节，协议 §8 注明）HTML→PDF）；每样本全新 LINGXI_HOME 克隆 + HOME 重定向（绝不读真实用户数据；模板含「记忆总开关 enabled:false」测量条件，协议 §8）。
- **样本**：各计时项 32 个独立样本（≥30）；内存 10×500ms 窗；全部报 n/median/p95/min/max/stdev/CV + 原始逐样本数据。

## 4. G1（旧 Electron+Node）基线结果

最终采用 run：server=`run-2026-09-23T23-12-26-989Z`、desktop=`run-2026-09-23T22-47-51-720Z`、pty=`pty-2026-09-24T01-45-52-350Z`（R1-F06 修复：PTY 夹具按协议值提到 128B/行≈1 MiB 后重测的 32 样本；全部 0 失败。中间失败 run 与 0.4MB 旧夹具 PTY run 的原始数据保留在 raw/ 作过程记录）。

> R1-F01 修复说明：下表 server 五行（→health、SIGTERM、ETag304、短会话首页、取消流停）原误植被弃中间 run `run-2026-09-23T22-05-11-181Z` 的数字，修复轮按最终 run raw 逐行重算更正（好坏方向混杂、无选择性挑数，根因与处理见 R1_REPAIR）；三处 CV 笔误一并更正：桌面退出 0.005→0.046、office 产品链 0.010→0.014、SIGTERM 行舍入 0.013→0.012。

| 指标（单位 ms 除注明） | n | median | p95 | min | max | CV |
|---|---|---|---|---|---|---|
| Server 冷启动 →server-info | 32 | 1170.8 | 1195.9 | 1141.8 | 1196.4 | 0.013 |
| Server 冷启动 →health 200 | 32 | 1190.7 | 1206.8 | 1171.8 | 1212.2 | 0.009 |
| Server 热启动 →server-info | 32 | 1175.4 | 1201.5 | 1147.9 | 1251.9 | 0.016 |
| Server SIGTERM→整树消失 | 32 | 93.59 | 95.63 | 91.06 | 95.98 | 0.012 |
| 长会话(1000条)首页 HTTP | 32 | 1.79 | 3.66 | 1.55 | 25.99 | 1.57* |
| 长会话 ETag 条件 GET(304) | 32 | 1.52 | 4.53 | 1.33 | 4.75 | 0.43 |
| 短会话(20条)首页 HTTP | 32 | 1.22 | 1.56 | 0.86 | 2.29 | 0.20 |
| 长会话全量翻页（21 页整程） | 3 | 33.7 | 34.6 | — | — | — |
| 流式请求→首增量（stub fast） | 32 | 19.7 | 22.6 | 17.0 | 83.7 | 0.52 |
| 流式请求→run 结束 | 32 | 27.5 | 31.6 | 24.0 | 94.8 | 0.40 |
| 取消：abort→abort_result（hang 流中） | 32 | 2.99 | 6.46 | 1.56 | 6.85 | 0.40 |
| 取消：abort→流停(status) | 32 | 2.74 | 6.22 | 1.37 | 6.57 | 0.42 |
| PTY 1 MiB 输出全程（spawn→交付完） | 32 | 756.6 | 787.6 | 736.1 | 897.0 | 0.036 |
| PTY 首块交付 | 32 | 403.4 | 424.8 | 387.8 | 531.0 | 0.060 |
| PDF 200KB HTML→PDF（spawn→exit） | 32 | 812.1 | 830.7 | 799.4 | 835.6 | 0.010 |
| PDF 同负载·完整产品链（office 工具） | 32 | 851.8 | 879.4 | 844.4 | 906.9 | 0.014 |
| 桌面冷启动 →app-ready | 32 | 9227.2 | 9375.3 | 9087.6 | 9851.3 | 0.014 |
| 桌面热启动 →app-ready | 32 | 9166.3 | 9707.4 | 8966.1 | 9796.5 | 0.019 |
| 桌面退出：SIGTERM→捕获整树全消失 | 32 | 258.7 | 307.6 | 256.3 | 307.8 | 0.046 |
| 桌面首启（含 793MB seed 解包） | 1 | 9217.5 | — | — | — | 单列 |

*长会话首页 CV 1.57 来自单样本 25.99ms 离群（其余 ≤4ms）——已按协议保留在原始数据中不剔除；p95 3.66ms 不受影响。

*PTY 行为 1 MiB 夹具（8192 行 × 128B，实测交付 outputBytes=1,056,849B 含 PTY 行尾 \r\n 转换与完成标记）结果；旧 0.4MB 夹具（48B/行）的过程结果 median 639.6ms / p95 652.9ms（首块 416.2/431.4ms）保留在 raw/ `pty-2026-09-23T22-12-53-212Z` 作对照。

| 内存/规模（KB 除注明） | 值 |
|---|---|
| Server 空闲进程树 RSS（1 进程） | 451,504（10 帧中位数；R1-F01c 更正：原 455,200 系被弃中间 run 数字） |
| 桌面空闲进程树 RSS（5 进程：主+GPU+网络+utility+renderer） | 1,048,784 |
| 桌面+5 浏览器视图（6 进程，同源视图共享渲染进程） | 1,015,568 |
| 流式往返本地开销（端到端−stub 服务时间 0ms） | median 27.5ms |
| 固定前缀（主聊天会话，R1-F04 双通道实测） | system prompt **10,626 B**（诊断=wire 一致）；主聊天请求体 median 28,599 B（= system 10,626 + 7 工具 schema JSON 10,287 + user 上下文数组 ≈7.2KB）；标题侧线调用请求体 666 B |
| 分发占用：Lingxi.app（--dir 应用目录，du 口径） | 812,728,320 B（≈775 MiB）；同目录 statSync 逻辑字节 971,247,121 B 仅参考（R1-F03：两口径不得混算，25% 收益对照只用 du）；seed 归档 du 353,718,272 B；seed 解包后 793,124,864 B |

浏览器多实例：5 个会话经真实链路（WS prompt→模型替身 tool_use→mcp_call→browser 工具→BrowserManager→/internal/browser→Electron WebContentsView）加载本地 512KB 确定性页面，navigate 工具往返 median 21.4ms（n=5），零错误。

## 5. R00-A11 自测证据（进程树测量不漏算）

- 前置达成：桌面腿真实启动了浏览器视图（上节链路，5 实例=产品上限 MAX_INSTANCES=5）与 PDF helper；PDF 有两套证据：(a) 打包二进制 `--hana-office-html-to-pdf` 直测（32/32 `%PDF-` 校验，median 812.1ms）；(b) **完整产品链补测**（run `office-2026-09-23T23-29-35-263Z`）：WS prompt → 模型替身 tool_use → `mcp_call` → `office_html-to-pdf` 插件工具 → **server spawn 打包二进制 helper** → SessionFile 落盘，32/32 成功（`%PDF-` 头 + 确定性 175,218 B），prompt→tool_end median 851.8ms（p95 879.4ms，CV 0.01）；产品链相对直测的编排开销 ≈40ms。
- **helper 以 server 子进程入树**（A11 关键证据）：首样本转换期间对 server PID 树的 8×300ms 采样窗显示树从 1 进程（server 467MB）膨胀到 5 进程（server + 打包 Lingxi helper 主进程 + Lingxi Helper GPU/网络 + Lingxi Helper (Renderer)，RSS 峰值 1,033,888KB），转换结束后回落 1 进程（493MB）——主程序与全部辅助进程同窗计入，逐进程明细（pid/comm/rss）在 `summary-office-*.json` 的 `treeDuringFirstConvert`。
- 采样窗一致：每次采样从根 PID 集合重新 `pgrep -P` 递归遍历 + `ps` 同窗读数；桌面空闲 5 进程、开 5 视图后 6 进程（进程数逐帧稳定，见 `idle-tree-*.json`/`browser-tree-*.json` 的逐帧逐进程明细）。
- 退出不漏算：SIGTERM 前捕获整树（防 reparent），核对全部 PID 消失——32/32 干净（桌面 teardown median 258.7ms；server 93.59ms；office 链 server 102.3ms；PTY helper 进程 close 后 0 残留；R1 修复轮 PTY 1 MiB 重测同样 32/32 干净、leftoverHelperProcs=0）。
- 原始证据：进程树帧 JSON（每帧含 pid/ppid/rss/vsz/pcpu/etime/comm）+ 逐样本 JSONL + 时间戳，均在 raw/ 目录。
- 边界如实登记：浏览器视图为 Electron 进程内 WebContentsView（产品形态，无独立 helper 进程）；同源 5 视图共享渲染进程是真实 Chromium 行为，非漏算。office 链的 helper exec 经 `LINGXI_OFFICE_PDF_HELPER_EXEC` 指向打包二进制（与产品内 server→helper 的 env 合同一致；产品桌面内该值来自 `LINGXI_DESKTOP_EXEC_PATH`）。

## 6. R00-A12 自测证据（阈值先于实现结果）

- 前提：保存阈值时（rev1 首次写入 2026-09-23T23:20:53Z；R1-F05 更正：原 23:15Z 为草拟名义时刻，见 §1）不存在任何 Rust 性能数据（无 `rust/` 目录；G2/G3 未实现）——文件内 `frozen_before_rust_results` 记录该核验；R1 修复轮再次复核仍成立。
- 全部 8 项门槛有：固定数值+单位+比较方向、适用组别/平台/构建/负载、G1 基线引用（含 run ID）、事前量化的噪声容限、理由与来源（taskbook 建议/噪声调整/R00 自定）、判定规则（采样、构建模式、模型等待分离、平台诚实、失败诚实）。
- 覆盖任务书 §6.2 全部六类：取消受理（p95≤250ms）、进程树清理（正常≤3s/强杀≤5s）、本地业务/历史读取（p95≤max(2×基线, 基线+20ms)=24ms）、固定前缀（≤10,626B 除已批准增量；R1-F04 证据驱动更正 10,620→10,626，双通道实测见 §4）、长时资源增长（2h/1000 任务无单调增长；旧系统长跑本轮未执行，登记 NOT_RUN_THIS_ROUND，R10 补测 G1 后对照）、迁移收益（**预选：分发占用减 ≥25%**，G1=775MiB，du 口径，R1-F03 标注禁与逻辑字节混算）+ 两项 R00 自定回归门槛（桌面启动 p95≤19.4s、空闲树内存≤1.5×基线）。
- 平台区分：darwin-arm64=MEASURED（本机数据）；darwin-x64/win32-x64/linux-x64=PENDING_MEASUREMENT（不得外推/虚报）。跨平台内存口径差异届时按协议登记。
- 变更治理：`change_log` 至 rev2（R1 修复轮数值溯源更正，逐项 from/to/reason，全部门槛判定值除 fixed_prefix 10,620→10,626B 外未动）；后续修改仍须独立变更记录，不得为通过测试放宽。
- 带哈希：`docs/rust-tauri/PERFORMANCE_THRESHOLDS.json` SHA-256 见 §8。

## 7. 观察到的旧系统行为事实（登记，不裁决）

1. **主聊天会话的 run_code 不可用**：经 mcp_call 调 run_code 返回 `no active session — run_code kernels are session-scoped`。定位：主聊天会话创建路径的 buildTools 不传 sessionRef（`core/session-coordinator.ts:2201` 调用块无 runtimeSessionRef/getSessionPath；`core/engine.ts:4033` 降级为 `() => null`），run_code 的 deps.getSessionPath 恒 null。PTY 负载因此改走真实组件栈直驱（真 node-pty/manager/bridge/stream，协议已登记边界）。是否属缺陷由后续阶段裁决，非本任务范围。
2. 模型可见工具面 = 7 个（read/write/edit/exec_command + mcp_call/describe/search），全部其余第一方/插件工具经 mcp_call 延迟目录暴露——这是 R06 上下文/提示词冻结的重要输入（主聊天 systemPrompt 10,626B + tools schema 10,287B；另：每样本首轮后有一次「对话标题生成器」侧线调用，666B/无 tools——R1-F04 双通道实测并落盘证据 `raw/server/fixed-prefix-probe-*.json`，可复现命令 `node scripts/rust-tauri/r00-t06-probe-fixed-prefix.mjs`）。
3. `nextBefore` 以字符串返回（"951"）——新实现分页协议须兼容（首版 walk 脚本因此误断，已修，属测量工具问题非产品问题）。
4. **`POST /api/sessions/new` 的 `permissionMode` 不落到会话**：请求 `operate` 后会话仍为 `auto`（实测 GET /api/sessions 复核）；产品路径是渲染端调 `POST /api/plan-mode {mode}`（office 补测采用该真实入口切 operate）。
5. **auto 模式下静态 review 权限工具的自动审批评审器 fail-closed**：无辅助（小/大）模型时返回 `TOOL_APPROVAL_UNAVAILABLE`（"Automatic approval review did not complete… check the small/large utility model settings"），不静默放行——office 链首测因此失败，属旧系统正确行为，登记供 R03/R05 权限语义冻结参考。
6. 同源多浏览器视图共享渲染进程（Chromium site isolation 行为），5 视图仅 +1 进程。

## 8. 候选文件与摘要（R1 修复轮后）

- 候选 = 本任务（含 R1 修复轮）新增/更新的 **82 个文件**（路径排序）：12 个测量/修复脚本、3 个 docs（协议+阈值+R1_REPAIR）、67 个 artifacts（基线+raw 66，含 office 产品链补测 run、PTY 1 MiB 新 run、固定前缀探针证据与全部过程记录）；**不含本报告自身**（避免自引用，报告哈希另行单列）、总控账本 `docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`（预先存在修改）与 R1 验收报告 `R00-T06_REVIEW_R1.md`（历史证据，原样保留）。dist/dist-server/d-* 为 gitignore 构建产物，不入候选；raw 下 `.mimosa/`（R1 审查方扫描插件残留）为 .gitignore 项，不入候选。
- 中间失败 run（22:05/22:14/22:23 等时间戳文件）、0.4MB 旧 PTY 夹具 run（22-12-53-212Z）与探针首跑（01-48-37-477Z）的原始数据均保留作过程记录；BASELINE_BENCHMARK.json 仅引用最终 run（PTY 腿为 1 MiB 新 run）。
- HEAD：`64c302ec482888222833136d0d20c64d4cbb350b`（未提交，候选以工作区文件交付）。

**逐文件 SHA-256（双空格分隔，路径排序）：**

```text
12b740a38b43325bb6dbdae5394ca84aedc361f318f11b33389cba0cca80644d  artifacts/rust-tauri/R00/T06/BASELINE_BENCHMARK.json
fdd5e26fc0bf7a6dbc252503cb05cb0e7c68face18bbe1f81ee11d65a3da9440  artifacts/rust-tauri/R00/T06/raw/build-release-summary.json
d2b2c64a082e09310b163b1ac9b1bbf5178c9431ec5516f51ba2b9433941f353  artifacts/rust-tauri/R00/T06/raw/desktop/browser-run-2026-09-23T22-14-52-330Z.jsonl
f0ce00b98b8cfe15408c5503cab1e99977f27d0ea5500f3eb7e6051b648f64eb  artifacts/rust-tauri/R00/T06/raw/desktop/browser-run-2026-09-23T22-23-53-802Z.jsonl
4ea5ca85d8041343a2bc69d13afa5576dec412dcc539c02894d43483f5ccdabd  artifacts/rust-tauri/R00/T06/raw/desktop/browser-run-2026-09-23T22-47-51-720Z.jsonl
db3fa30c364ccc40f92041a61560589672d82395d37db5917ec8a5fd97fcf4d9  artifacts/rust-tauri/R00/T06/raw/desktop/browser-tree-run-2026-09-23T22-14-52-330Z.json
afe97795baff7f6eb02b71591289e6b5a8611f7260b7e8443246a6544e5a0030  artifacts/rust-tauri/R00/T06/raw/desktop/browser-tree-run-2026-09-23T22-23-53-802Z.json
21bc0682d594eb769076a5b3ecb43416b1f281fba0dddffee80079319e8b2b3c  artifacts/rust-tauri/R00/T06/raw/desktop/browser-tree-run-2026-09-23T22-47-51-720Z.json
0919fb8b4ca810ea30a7eadaf351d8d291514b98d1ded67eb64fc09489fb5515  artifacts/rust-tauri/R00/T06/raw/desktop/idle-tree-run-2026-09-23T22-14-52-330Z.json
f7f7a5ed9108993351db499af0f391a9686f01f4cba3c4dcb9a2de795ece5faa  artifacts/rust-tauri/R00/T06/raw/desktop/idle-tree-run-2026-09-23T22-23-53-802Z.json
eb7eb41092b68cc064fb0f893224c3dedc6a0d84e4d99651ab43693f5964612d  artifacts/rust-tauri/R00/T06/raw/desktop/idle-tree-run-2026-09-23T22-47-51-720Z.json
f06fa0818c4f6181eae7aec9fa7566f0acdc0d7e6f153f5fb250b576fdb05bde  artifacts/rust-tauri/R00/T06/raw/desktop/pdf-run-2026-09-23T22-14-52-330Z.jsonl
98dbf886b4be2cfab487795a44a8f69bacfdcf331a7b2f7a6bfbbbbe0074c565  artifacts/rust-tauri/R00/T06/raw/desktop/pdf-run-2026-09-23T22-23-53-802Z.jsonl
b76046ff59ffec4138f7890f4921d4c67aebcf11bd535dad3111cb55c53a4888  artifacts/rust-tauri/R00/T06/raw/desktop/pdf-run-2026-09-23T22-47-51-720Z.jsonl
3477a9601471daec3247390cc19f1918981aa6522430095ac1cbda81f11bad4f  artifacts/rust-tauri/R00/T06/raw/desktop/startup-run-2026-09-23T22-14-52-330Z.jsonl
e3db4acf9933a7fc43e939ffce8e8bc7475ee1932b2e95ff2608130e53dc3270  artifacts/rust-tauri/R00/T06/raw/desktop/startup-run-2026-09-23T22-23-53-802Z.jsonl
b37710ceffe01e32326b4b2c5a74f58b2bde857a44bb7ba85a1c4ebba3d4cdb7  artifacts/rust-tauri/R00/T06/raw/desktop/startup-run-2026-09-23T22-47-51-720Z.jsonl
1b06ec8205fd30e58e1a87993257f30ba9aeeba3cd4d9aaa3fec54e055519b8b  artifacts/rust-tauri/R00/T06/raw/desktop/stub-journal-run-2026-09-23T22-14-13-277Z.jsonl
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  artifacts/rust-tauri/R00/T06/raw/desktop/stub-journal-run-2026-09-23T22-14-52-330Z.jsonl
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  artifacts/rust-tauri/R00/T06/raw/desktop/stub-journal-run-2026-09-23T22-23-53-802Z.jsonl
7ae9e046fdad64bf73b1c4a501cfe4ba5a727c9fb5cbc9322585fe2f173beffa  artifacts/rust-tauri/R00/T06/raw/desktop/stub-journal-run-2026-09-23T22-47-51-720Z.jsonl
9fde3d4dc83ba2d1cf697c0d01cd7ab582361f27799a0e5467095db4386a4567  artifacts/rust-tauri/R00/T06/raw/desktop/summary-run-2026-09-23T22-14-13-277Z.json
784ac8a040912c4739368dd011e1027ec2c077790a817c06731c7b8980dcfde2  artifacts/rust-tauri/R00/T06/raw/desktop/summary-run-2026-09-23T22-14-52-330Z.json
fa9a3bdcc8de9f4728af74d68477eebd139310db4ddda771d817cfa4753e86f2  artifacts/rust-tauri/R00/T06/raw/desktop/summary-run-2026-09-23T22-23-53-802Z.json
119d22a4b2ba0e0a788cce3d5fd02f367ca367a4805c152c04a070caec6b1f93  artifacts/rust-tauri/R00/T06/raw/desktop/summary-run-2026-09-23T22-47-51-720Z.json
6cf3dfe0afe8814ccb56d514cd6c722c03060e8aed1739dc3a0b48ad8ea1066b  artifacts/rust-tauri/R00/T06/raw/server/cancel-run-2026-09-23T21-59-17-410Z.jsonl
7d60bcb58f1a5962acdd42b0b3e6d21baac172ba74451d8908f1ec5072e1b241  artifacts/rust-tauri/R00/T06/raw/server/cancel-run-2026-09-23T22-05-11-181Z.jsonl
fe0e1b861a3000cfef07b963d6d24d737548633ee97985fa6d558519a96c74e4  artifacts/rust-tauri/R00/T06/raw/server/cancel-run-2026-09-23T23-12-26-989Z.jsonl
723f0c05db297a875b6d9198b5a15f246487ed71f2789270ce8cd9f99c61fac3  artifacts/rust-tauri/R00/T06/raw/server/fixed-prefix-probe-2026-09-24T01-48-37-477Z.json
504a0a14515babf2b900848c9061bda54744603a59256896d3fd7a589494fad5  artifacts/rust-tauri/R00/T06/raw/server/fixed-prefix-probe-2026-09-24T01-50-15-634Z.json
158284ce90f717a0b73a2b7e9d5072e931731228143774a4699382fb7e04bed5  artifacts/rust-tauri/R00/T06/raw/server/history-run-2026-09-23T21-59-17-410Z.jsonl
3f3780c33a648f7a9338aa673b4ef7cec574881e8f95bada9e9b623f4e8ee6ff  artifacts/rust-tauri/R00/T06/raw/server/history-run-2026-09-23T22-05-11-181Z.jsonl
c0d6227b113192d4a07ead18af4782c80bd70089882d4da216640b88966b22a3  artifacts/rust-tauri/R00/T06/raw/server/history-run-2026-09-23T23-12-26-989Z.jsonl
c11d445c2b5b532f5f4ec31fd1a475f29e87503342e29f966627e181c67c2e70  artifacts/rust-tauri/R00/T06/raw/server/idle-tree-run-2026-09-23T21-59-17-410Z.json
57c1b2b97c6d3d7a5dac2d5be5d4bab021f8726027306020e0f23e5af59320cd  artifacts/rust-tauri/R00/T06/raw/server/idle-tree-run-2026-09-23T22-05-11-181Z.json
80eb9c6309bf6f60e070f236177abd4f0ca85eb6e27d4ca746059bfe0e493f85  artifacts/rust-tauri/R00/T06/raw/server/idle-tree-run-2026-09-23T23-12-26-989Z.json
3ac52c15d82589358e86dc552bf73dd447fb81908a7a13cd4d80b8c38daf712c  artifacts/rust-tauri/R00/T06/raw/server/office-pdf-office-2026-09-23T23-26-03-574Z.jsonl
bcc3247875451846039ebd4df90b89402cd8f06d722c5af5d1ac10b587311abe  artifacts/rust-tauri/R00/T06/raw/server/office-pdf-office-2026-09-23T23-28-09-564Z.jsonl
f307bfa751656ebb00d370389d8c9aaa0b6bb9c2b5e31a0211b31e1405ab4e3d  artifacts/rust-tauri/R00/T06/raw/server/office-pdf-office-2026-09-23T23-29-15-055Z.jsonl
3dfa4621f4095288f900f9d018596d1d8bb51d5989fe44e9f6c2f7fbb8d6ef5e  artifacts/rust-tauri/R00/T06/raw/server/office-pdf-office-2026-09-23T23-29-35-263Z.jsonl
55ff0c01659f8cae21714eec98e6751e77cac4ad44797623843fb37e33c40c94  artifacts/rust-tauri/R00/T06/raw/server/pty-driver-pty-2026-09-23T22-12-45-250Z.jsonl
81a3b0bc95835cdf16ef40210e99bcaa150c046590fadf0039e7b16bba595f6e  artifacts/rust-tauri/R00/T06/raw/server/pty-driver-pty-2026-09-23T22-12-53-212Z.jsonl
ab1c885eed8e577b898929ed92532a3d0474f849ba395d5040a627315b95901b  artifacts/rust-tauri/R00/T06/raw/server/pty-driver-pty-2026-09-24T01-45-52-350Z.jsonl
dd72c7d59062a9ca115bc07eea76802058a991d53b2278952730a17e275c1248  artifacts/rust-tauri/R00/T06/raw/server/startup-run-2026-09-23T21-59-17-410Z.jsonl
b9116dfffab23292964dcaf4010b7bd33f13d590f0af98295b78d0cd309b058b  artifacts/rust-tauri/R00/T06/raw/server/startup-run-2026-09-23T22-05-11-181Z.jsonl
03bc36511b5fb9ab6eaa0d0562cda787d9c18ba9ea7fd7435e7f5f0402daf829  artifacts/rust-tauri/R00/T06/raw/server/startup-run-2026-09-23T23-12-26-989Z.jsonl
1de7aa4d74ca579eb7e336e885d9352b34a97a31fd439ff4c4b7053efc711bc2  artifacts/rust-tauri/R00/T06/raw/server/stream-run-2026-09-23T21-59-17-410Z.jsonl
a581158204bf41d3af1a9089b16ee6b7ca33c36130c270681eed37998d2286d9  artifacts/rust-tauri/R00/T06/raw/server/stream-run-2026-09-23T22-05-11-181Z.jsonl
4adbe4f276641516a57e2e8c21fe6e1ec64a761f909218ac2fb507ce33c3f104  artifacts/rust-tauri/R00/T06/raw/server/stream-run-2026-09-23T23-12-26-989Z.jsonl
c2d7e106a2842425006f862843a666e0d7f6e2da866d952fff64411b4ffb241a  artifacts/rust-tauri/R00/T06/raw/server/stub-journal-office-2026-09-23T23-26-03-574Z.jsonl
8562ca3f1df141a61110f5e19cf72a6313584f34f132336ae19fd0634d6d8c84  artifacts/rust-tauri/R00/T06/raw/server/stub-journal-office-2026-09-23T23-28-09-564Z.jsonl
64c19e084106517fc843611741beea118ed31a3e21426a7a8f186b1052467c37  artifacts/rust-tauri/R00/T06/raw/server/stub-journal-office-2026-09-23T23-29-15-055Z.jsonl
be499c3a103e7b03bafcf5738a1916dcde5c24165b32776e91e568ad8d0e9a34  artifacts/rust-tauri/R00/T06/raw/server/stub-journal-office-2026-09-23T23-29-35-263Z.jsonl
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  artifacts/rust-tauri/R00/T06/raw/server/stub-journal-run-2026-09-23T21-56-45-950Z.jsonl
284ddbc1789c5e3a41ccf531dc01914f3745eedb2f13edd9d4b64aa481f03bd4  artifacts/rust-tauri/R00/T06/raw/server/stub-journal-run-2026-09-23T21-59-17-410Z.jsonl
6646ea94ef52af443db5b29aaaf65d6a9c4088d29c8d283337957da33596b1fc  artifacts/rust-tauri/R00/T06/raw/server/stub-journal-run-2026-09-23T22-05-11-181Z.jsonl
b42e1674259682fbb08eac639f0cac11233fd853832a8fda82ddd9a73f83f275  artifacts/rust-tauri/R00/T06/raw/server/stub-journal-run-2026-09-23T23-12-26-989Z.jsonl
ce19af3df334a5c8551cd50bd0816e42067ef824cb07305e83ee6af1cc4ad4e9  artifacts/rust-tauri/R00/T06/raw/server/summary-office-2026-09-23T23-26-03-574Z.json
611ea21312b6fef58664483652d73b753c9a2932a03dcf4b16b1215d0895f268  artifacts/rust-tauri/R00/T06/raw/server/summary-office-2026-09-23T23-28-09-564Z.json
dfbd8e4975f51835240d3bd78478523ef0e2465b3d628b81f45e7d18ed1ce003  artifacts/rust-tauri/R00/T06/raw/server/summary-office-2026-09-23T23-29-15-055Z.json
ca25a2d823c1e62ad1d9a763af2bb1a238e1ae263b00a6d3fc468e62a9eb4cde  artifacts/rust-tauri/R00/T06/raw/server/summary-office-2026-09-23T23-29-35-263Z.json
3c77fa05f74b0ebd5aabbe33f4a0a7d6cb7c4ae71bf2444e3f076eb9401add62  artifacts/rust-tauri/R00/T06/raw/server/summary-pty-2026-09-23T22-12-45-250Z.json
7887f7d80cefbcdebbb6422a22bade62736754e0c6eb896a21fb4e4c4b828193  artifacts/rust-tauri/R00/T06/raw/server/summary-pty-2026-09-23T22-12-53-212Z.json
dfb8ca7e34324db1102ee99f380aca21d42a81869d79905c8dd85beaf383cd17  artifacts/rust-tauri/R00/T06/raw/server/summary-pty-2026-09-24T01-45-52-350Z.json
6a211ed0b390aa79fb677b8fe822d7ff92c14f09fe1600957501941d86fbf477  artifacts/rust-tauri/R00/T06/raw/server/summary-run-2026-09-23T21-59-17-410Z.json
8b77a014457aeaf82827b3ec0892c2fb8432bcdbdff87b25ea2cd3d91521391e  artifacts/rust-tauri/R00/T06/raw/server/summary-run-2026-09-23T22-05-11-181Z.json
0da25bc3c7ddb61cf96d1eca6a954fc223a9517c5b206d4c8262e679bb077d4f  artifacts/rust-tauri/R00/T06/raw/server/summary-run-2026-09-23T23-12-26-989Z.json
bf40009e347df014dba1d2a2ecb7505bf4bdccfdef1992fc99b144749a992331  docs/rust-tauri/PERFORMANCE_THRESHOLDS.json
e941118ca2b85111ad148d92e2247a2f31824bdb54044a590e9825d60ddaebe2  docs/rust-tauri/R00/R00-T06_PROTOCOL.md
e30a0f116997b628462e46cc8e83bfcc41dec0f6dc66d656cb558a5da41c3326  docs/rust-tauri/R00/R00-T06_R1_REPAIR.md
063f79b96d5a4200b4c6197d3063af99db55202c18712acb459fc0642ad4c3d3  scripts/rust-tauri/r00-t06-bench-desktop.mjs
fc33c1f6a6b45e681362c741c9617650977dd06b034eb01384ed237c8b656ea9  scripts/rust-tauri/r00-t06-bench-office-pdf.mjs
025fad801dc080e7cd63cc254ff69e890ead843fce4eb8ca659938705aa92156  scripts/rust-tauri/r00-t06-bench-pty.mjs
ae4ebc5280bfffb17217bdf7284b90bd4ff53e96acb2fb0c67429c3a5f5b473c  scripts/rust-tauri/r00-t06-bench-server.mjs
01bb6f9d886ac25558db38a3ed137070d98df07cbd057c96983759bce1354cb8  scripts/rust-tauri/r00-t06-build-release.mjs
96d1824a02374519db1b2b44fe1cbe64917dfb32922eaf1982cab63e7caf46d0  scripts/rust-tauri/r00-t06-fixture.mjs
4c6e0a639b026f916b655a4e9816b18818d8aab1a3f6828a2bceb7caf337b7b9  scripts/rust-tauri/r00-t06-lib.mjs
e0c4473864d482e0b14dd85c768e9fd28f36572adec26c4164a2a3c20be72914  scripts/rust-tauri/r00-t06-probe-fixed-prefix.mjs
611b6e577ffdd6ecdbe5bbac5d395ace5ceb548dea593d423df8a20c3b65f4cc  scripts/rust-tauri/r00-t06-repair-build-summary.mjs
ad14bed90501d1f25e284eee7993d64da26a93f535dfa2a4b9bba8f4f78f7012  scripts/rust-tauri/r00-t06-smoke.mjs
19e5bb0c08312709b45c4ca026346475794adedc65b4f3cf3747b78a7d090f9b  scripts/rust-tauri/r00-t06-stub-provider.mjs
eac0c344293757b29291984de03afd3a0d0dfbbdc2939076a1e1d6ace262bb83  scripts/rust-tauri/r00-t06-summarize.mjs
```

**候选聚合 SHA-256**（上列 82 行按「SHA256 + 双空格 + 路径 + 换行」拼接后整体哈希）：

```text
8f6de53d66c4a2d7d63c5c67de3376ae3e0e02ab981e4c2d8f41e40066143785
```

（R1 验收时的 75 文件候选聚合 `e0a49021…` 已被本修复轮取代：+7 新文件、9 个既有文件内容更新。）

**报告自身 SHA-256**：本报告不在候选集内，亦不自嵌哈希（嵌入即改变自身）；修复代理在交付说明中另列最终值，验收方对当前文件执行 `shasum -a 256 docs/rust-tauri/R00/R00-T06_REPORT.md` 可随时核对。

## 9. 实际命令与结果

| 命令 | 退出码 | 结果 |
|---|---|---|
| `node scripts/rust-tauri/r00-t06-build-release.mjs` | 0 | 发布优化全链：keygen→build:client→build:server（prefer-offline）→helpers→bundled-bins→verify:seed-kit→electron-builder --dir；产物指纹入 build-release-summary.json |
| `node scripts/rust-tauri/r00-t06-smoke.mjs` | 0 | 冒烟：stub/fixture/就绪/health/sessions-new/流式（firstDelta 55–59ms）/mcp_call 工具链 |
| `node scripts/rust-tauri/r00-t06-bench-server.mjs` | 0 | 最终 run 6 阶段全过、0 错误（此前两轮因脚本 bug 失败后修复重跑，过程数据保留） |
| `node scripts/rust-tauri/r00-t06-bench-pty.mjs` | 0 | 32/32，transcript 完成标记 32/32，helper 残留 0 |
| `node scripts/rust-tauri/r00-t06-bench-office-pdf.mjs` | 0 | 完整产品链 32/32（%PDF 校验 + 确定性 175,218B；转换期间 helper 以 server 子进程入树，见 summary `office-2026-09-23T23-29-35-263Z`；前置两次小样本冒烟失败后修复权限模式切换与输出校验，过程数据保留） |
| `node scripts/rust-tauri/r00-t06-bench-desktop.mjs --phases template,startup,idle,browser,pdf,size` | 0 | 最终 run 全过、0 错误（此前两轮因模板端口/字体路径/日志锚定失败后修复重跑，过程数据保留） |
| `node scripts/rust-tauri/r00-t06-summarize.mjs --server-run …-23-12-26-989Z --desktop-run …-22-47-51-720Z` | 0 | 首轮汇总，stub 日志关联 n=32 |
| `node --check scripts/rust-tauri/r00-t06-*.mjs`（9 文件） | 0 | 语法全过 |
| `npm run typecheck` | 0 | 三配置全过（本任务未改 TS/生产代码，确认零干扰） |
| `python3 -c "json.load(...)"`（阈值/基线/全部 summary） | 0 | JSON 全部可解析 |
| 进程清理核查 `ps aux \| grep …` | — | 测量结束后 0 个 Lingxi/服务器/PTY 残留进程；47613 端口释放；自建 $TMPDIR 目录清空 |

**R1 修复轮新增命令与结果（2026-09-24，详见 R00-T06_R1_REPAIR.md）：**

| 命令 | 退出码 | 结果 |
|---|---|---|
| `node scripts/rust-tauri/r00-t06-repair-build-summary.mjs` | 0 | F02：盘上原始被测产物（未重建，rebuilt=false）指纹重算；守卫通过（server bundle SHA == 最终 run 记录 `5095cc7f…`、app.asar SHA == 原 summary `7b8478ab…`）；serverBundle logical 672,507,925 / du 713,519,104；desktopApp logical 971,247,121 / du 812,728,320；seed 归档 logical 353,708,203 / du 353,718,272（4 文件逐个 SHA-256） |
| `node scripts/rust-tauri/r00-t06-bench-pty.mjs`（夹具修复后） | 0 | F06：1 MiB 夹具（8192×128B）重测 32/32（run `pty-2026-09-24T01-45-52-350Z`；outputBytes=1,056,849B；transcript 完成标记 32/32；helper 残留 0）；旧 0.4MB run 数据保留 |
| `node scripts/rust-tauri/r00-t06-probe-fixed-prefix.mjs` | 0 | F04：真实会话路径固定前缀补采——cache contract 诊断 systemPromptBytes=10,626（drift_auto_renew 后）与 wire 实测 system 消息 10,626B 双通道一致；666B=标题侧线调用、28,599/28,591B=主聊天请求（tools schema 10,287B）；探针运行 2 次（首版行截断缺陷修正后重跑，两份证据均保留）；进程清理 SIGTERM 整树 110ms、0 残留 |
| `node scripts/rust-tauri/r00-t06-summarize.mjs --server-run …-23-12-26-989Z --desktop-run …-22-47-51-720Z --pty-run …-01-45-52-350Z` | 0 | 基线重建：build 块真实指纹+双口径、ptyLeg=1 MiB 新 run、modelWaitSeparation 增 mainChatRequest（fast 32 行 median 28,599B / hang 按 seq 去重 32 行 median 28,591B）与 fixedPrefix 口径澄清；server/desktop 腿统计与上版逐位一致 |
| `node --check scripts/rust-tauri/r00-t06-*.mjs`（12 文件） | 0 | 语法全过 |
| `python3 -c "json.load(...)"`（阈值 rev2/基线/build summary/探针证据） | 0 | JSON 全部可解析 |
| 进程清理核查 `ps` | — | 修复轮全部自启进程（probe server、打包 server、PTY 子进程）清理完毕，0 残留；临时 LINGXI_HOME 目录已删 |

## 10. 未验证范围 / BLOCKED / 边界

- **真实与替身边界**：模型协议=确定性本地 stub（fast/delayed/hang）；真实供应商测量**0 次调用**（未授权，单列未执行）。浏览器视图加载的是本地确定性页面；PDF/PTY/历史/流式输入全合成。
- **长时资源增长（2h/1000 任务）**：阈值已冻结、本轮**未执行**（登记 NOT_RUN_THIS_ROUND；属 R10 性能证明的执行项，G1 对照届时补测）。
- **其他平台**（macOS x64 / Windows x64 / Linux x64）：PENDING_MEASUREMENT，无任何外推数据。
- **安装包（dmg/nsis）尺寸**：未构建（公证/签名链不在授权内）；分发占用以 --dir 应用目录字节为口径（775 MiB），安装包压缩差异未测。
- **桌面腿构建边界**：--dir 应用为 ad-hoc 签名 + 测试 seed 密钥，非可发布安装包（内容同构，签名形态不同；尺寸口径不受影响，已在协议 §2 登记）。electronDist 使用 node_modules 解包目录（无外网）。
- **PTY 负载边界**：真实组件栈直驱（协议 §9 已登记理由=旧系统主聊天 run_code 不可用）；office 工具编排层未经模型驱动（browser 工具链已证明目录可达）。
- **首启样本**：n=1（真实首启解包计时单列，非 ≥30 系列）。
- OS page cache 未清（协议显式边界）；本机 load1 2–4（96GB/28 核机器的常规负载，已记录每 run 快照）。

## 11. 建议独立验收重点

1. 抽一条原始 JSONL（如 `raw/server/startup-run-…23-12-26-989Z.jsonl`）核对逐样本数据与 §4 表一致（注意 §4 表 R1-F01 修复说明：server 五行已按最终 run 重算）；重跑 `node scripts/rust-tauri/r00-t06-bench-server.mjs --phases startup --samples 5` 比对量级（启动 ~1.17s、CV <5%）。
2. 打开 `raw/desktop/browser-tree-run-2026-09-23T22-47-51-720Z.json` 核对 6 进程帧明细（A11：浏览器视图渲染进程计入）与 `browser-run-…jsonl` 的 mcp_call 工具链零错误记录。
3. 核验 A12 时序性：阈值文件 `frozen_at`（2026-09-23T23:20:53Z，basis 见文件内 `frozen_at_basis`）/`frozen_before_rust_results` 早于任何 Rust 产出（当前仓库无 rust/ 目录即证据，与时间戳字段无关）；逐项检查门槛固定值/理由/噪声容限完整性；change_log rev2 的每项 from→to 是否与 raw 可对回。
4. 复核 §7 观察事实 1（主聊天 run_code "no active session"）的源码定位：`core/session-coordinator.ts:2201`（无 sessionRef）与 `core/engine.ts:4033`（降级 null）。
5. 复核 office 产品链证据：`raw/server/summary-office-2026-09-23T23-29-35-263Z.json` 的 `treeDuringFirstConvert`（server 树内出现打包 Lingxi helper 及 GPU/网络/Renderer 子进程）与对应 `office-pdf-*.jsonl` 逐样本 %PDF 校验。
6. 检查隐私：stub 日志与 raw 数据无真实凭证/用户数据（token 为合成 `not-a-secret` 类；会话全合成）。
7. **R1 修复轮复核**：读 [R00-T06_R1_REPAIR.md](R00-T06_R1_REPAIR.md) 逐项核对 F01–F07 处理与证据——重点：build-release-summary 守卫链（盘上产物 SHA 与最终 run 记录互证）、探针双通道 10,626B、PTY 1 MiB 新 run 统计与 §4 表一致、阈值 change_log rev2 各项 from 值确与被弃 run 数字对应、协议 §12 变更记录与实际 diff 一致。
