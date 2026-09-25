# R01-T05 独立对抗性验收报告（REVIEW R1）

- 任务：R01-T05（HTML/PDF 与办公处理原型）；验收 R01-A09 / R01-A10（REQUIRED）
- 验收代理：ZCode:R01-T05-review-r1（全新独立，未参与执行；只读审查 + /tmp 隔离复跑）
- 基线 HEAD：`5a8a8e24ab9e8e105da5132d16b175221a91d646`；分支 `codex/rust-tauri-migration`
- 被审执行者：ZCode:R01-T05-exec-r1，报告 `docs/rust-tauri/R01/R01-T05_REPORT.md`
- 平台：macOS 27.0 arm64；Node v24.16.0；Python 3.14.3；Rust 1.98.1；Chrome 153.0.8010.52（复跑实测一致）
- 日期：2026-09-25
- **最终判定：PASS**（A09/A10 全部 REQUIRED 声明独立复现；发现 1 项 MEDIUM + 3 项 LOW/INFO，均为披露/加固项，未推翻任何验收声明）

## 0. 基线与工作区核实

`git rev-parse HEAD` = 基线一致。`git status --porcelain` 与 `git diff --stat HEAD` 结果与任务书候选清单完全一致：
修改 5 文件（ORCHESTRATOR_PROGRESS.json 仅 task_base_sha 一行、DEPENDENCY_DECISIONS.md 仅追加 D-10、
DEPENDENCY_RULES.json 仅 browser-spike 职责补注、spike crate 的 Cargo.toml description 与 lib.rs `pub mod pdf;`），
新增 6 项（spike_pdf.rs / pdf.rs / r01-t05-replay.sh / tests/migration/r01-t05/ / artifacts/…/T05/ / 三份文档）。
`git diff HEAD -- desktop/ server/ core/ lib/ shared/ cli/ hub/ plugins/ skills2set/ package*.json rust/Cargo.lock`
**全部为空**——生产零改动、Cargo.lock 零 diff 声明属实。本验收全程未修改任何已提交文件与执行者交付物；
所有复跑产物在 /tmp/r01t05-review 与 /tmp/r01t05-replay（CARGO_TARGET_DIR=/tmp）。验收后 `git status` 与验收前逐行一致。

## 1. 候选清单 + 复算哈希

| 交付物 | 执行者声明哈希前缀 | 本验收复算 SHA-256 | 结论 |
|---|---|---|---|
| PDF_SPIKE_REPORT.md | 52d2af89…4fffb | 52d2af89155efb33f4ec1a2c79ee40bdfa6f76a93b492c8ca86144a7d824fffb | 一致 |
| ADR-003-document-renderer.md | 71351dea…6e13 | 71351deae35cf4187aa05ba8f73c25e785a57030edf8117b40d1b586cc116e13 | 一致 |
| rust/.../src/pdf.rs | 1f449862…326c | 1f44986290f35a1059eece17dba9a5faec11eb381f5657049186f91ac301326c | 一致 |
| rust/.../src/bin/spike_pdf.rs | 13aad66b…7788 | 13aad66b280ac61052766a76b0ba5de422456f2cec0b38232a0c86bca1e17788 | 一致 |
| scripts/rust-tauri/r01-t05-replay.sh | 2e5d8752…7364 | 2e5d8752cde311bf5dbed4cd53f399958f815987df15fd7f898f604db64a7364 | 一致 |

证据包 `artifacts/rust-tauri/R01/T05/SHA256SUMS.txt` 实测 **632 项**（执行者报告写 632 正确；
spike 报告 § header 写 "623 项" 为笔误，见发现 F3）。随机抽查 30 项 + 关键产物 8 项
（a09 双链 PDF、a10 双链 PDF、matrix-verdicts.json、compare.json、perf-summary.json、canary.log），
**38/38 复算一致，0 mismatch，0 missing**（seed=20260925）。
样本确定性：`generate_samples.py --check` exit=0；本验收复跑的 samples-sha256.txt 与执行者证据**逐行一致**
（diff 为空），证明样本生成器确定性可重放。

## 2. 逐场景独立复跑（命令 / 退出码 / 结论）

统一环境：`env -u 全部代理变量`；loopback-only 代理 127.0.0.1:18482（自验 200 loopback / 403 example.com）
+ canary 127.0.0.1:18291；数据目录 /tmp。除注明"源码确证"外均为**实际运行**。

### 2.1 全流程重放（独立 ART 目录 /tmp/r01t05-review/T05）

命令：`bash scripts/rust-tauri/r01-t05-replay.sh /tmp/r01t05-review/T05`
**退出码 0**。replay-summary.jsonl 19 场景退出码全部符合预期（a10-infinite-new=2、a10-infinite-old=1，
其余 0）。随后 `python3 tests/migration/r01-t05/analyze_matrix.py /tmp/r01t05-review/T05` exit=0，
**28 VERIFIED / 0 FAILED**——与执行者声明完全一致，且矩阵内容（MediaBox 值、inkBBox 值、DENY 计数、
cleanup 字段）与执行者证据逐项吻合。我的 a09/compare.json 的 35 项 checks 与执行者证据**逐字节同义**
（差异仅路径/时间戳）。proxy.log REFUSED 条数：执行者 286 / 本验收 291（同量级，Chrome 后台外联全部
被代理层拒绝，声明属实）。

### 2.2 R01-A09 中文长文档（独立提取与复算，不信执行者中间产物之外任何声明）

对本验收自己产出的双链 PDF（PDFKit 探针 + 自写 Python 复算）：

| 判据 | 旧链 | 候选链 | 独立复算结果 |
|---|---|---|---|
| 页数 | 74 | 74 | 一致（声明 74=74 属实） |
| 表格行 ROW-NNN-END 唯一计数 | 140 | 140 | 140/140=140/140，无丢行 |
| 异常空白页（ink<0.0005） | 无 | 无 | 两链空集 |
| 归一化全文相似度（difflib, autojunk=False） | — | — | **0.99910**（87,161 vs 87,148 字符），与声明 0.99910 逐位一致；方法无自我放宽（归一化仅去空白，阈值 0.99 低于实测两个数量级的差距） |
| 生僻字哨兵 | 龘靐麤爨驫鬱齉爩龗灪𠮷 全在 | 同左 + 𪚥 | 逐字核验一致；**𪚥(U+2A6A5) 旧链文本层缺失、候选链存在**——旧链 Ext-B 缺陷声明属实 |
| 位图视觉差异（16×16 亮度网格平均绝对差，0–255） | — | — | **max=7.16 / mean=6.43**，与声明逐位一致 |
| 图片 XObject | 2 | 2 | 一致 |
| 字体嵌入 | EBGaramond/NotoSerifSC/JetBrainsMono 三族齐 | 三族齐 | /BaseFont+/FontName 双扫一致；负对照 embedLingxiFonts=false 时三族均消失（仅 STSongti/PingFang 系统回退） |

相似度 0.0009 缺口的构成（本验收独立 diff opcode 分析，119 处）：绝大多数为旧链文本层**数字串转置**
（如源 HTML `庚42`+`40805` 两格 → 旧链提取 "4240085"、候选链提取正确的 "4240805"），
属 Chromium 142 文本运行顺序的提取层伪差，非渲染错误（位图网格差同步极小）；另有 SVG 数学内容
文本化差异与 𪚥 缺字。compare_outputs.py 的判据（页数/空白页/哨兵/行数/相似度阈值 0.99/网格阈值 20）
无自我放宽：哨兵 token 列表来自任务书级内容，阈值远低于实测余量。
逐页 PNG 抽查：page-01 两链位图尺寸 1192×1686 vs 1192×1684（2px = 1pt MediaBox 舍入差），目视级一致。

**A09 判定：VERIFIED（独立复现）。**

### 2.3 R01-A10 危险资源与失败可控

执行者声明场景全部独立复现（命令见 2.1 重放）：
- 候选链 S5：fetch-log 7×DENY（/etc/passwd iframe Document、img Image、XHR、master.passwd、/etc/hosts
  fetch、canary.invalid、loopback canary），输出 PDF 无泄漏（文本层无 root:/nobody: 真实条目），exit=0。
- canary.log 命中归属核验：候选 fetch-log 显式 DENY 该 URL（请求未发出），canary 命中的时间戳落在
  旧链运行窗口——归属论证成立。
- 旧链 decoy 对照：DECOY-MARKER-7f3a 完整嵌入旧链 PDF（旧链无拦截层，真实泄漏），声明属实。
- S6 无限脚本：候选 exit=2「loadURL timed out after 5000ms」、无 s6.pdf、profile_removed=true、
  leftover_processes 前后均空；旧链 exit=1（5331ms）无产物；JS 关闭变体正常产出 exit=0。

**本验收自造对抗样本（执行者样本之外新增，全部实测）**：

| 样本 | 攻击向量 | 结果 |
|---|---|---|
| ATK1 | 符号链接逃逸（allowlist 目录内 symlink → /etc/passwd，iframe+img+fetch 三通道） | 3×DENY（canonicalize 后越界识别正确），exit=0，清理干净 |
| ATK2 | URL 变体：file:///etc/ 目录列表 iframe、file://localhost/etc/passwd、%70 百分号编码、file://// 四斜杠、/private/etc 物理路径、fetch 后 btoa 外带至 canary.invalid | 7×DENY（含外带通道），exit=0 |
| ATK3 | meta refresh 0s 跳 file:///etc/passwd（主框架导航） | Document DENY，页面保持原文档，exit=0 |
| ATK4 | **WebSocket ws://127.0.0.1:18291 + wss://canary.invalid** | **WS 握手真实到达 loopback canary（日志命中）——见发现 F1**；wss 经代理 CONNECT 被 403 |
| ATK5 | load 后 setTimeout 死循环（挂起变体，非阻塞 load） | printToPDF 阶段超时，exit=1，无产物，进程/profile 清理核验为空——但见发现 F2（exit 码非 2） |
| ATK6 | fetch/sendBeacon/EventSource/WebSocket 四通道打 loopback canary | 前三者全部 Fetch 层 DENY（XHR/Ping/XHR）；**仅 WebSocket 逃逸（canary 唯一命中 /t05-ws-confirm）** |

**A10 判定：VERIFIED（任务书面样本与执行者声明的 7 项 DENY、超时清理、零伪产物全部独立复现；
JS 关闭惰性化复现）。** WebSocket 逃逸为本次验收新发现的候选链边界缺口，见 F1——它不推翻 A10
（规格样本为 file:// 越权 + 无限脚本；执行者从未声明 WS 覆盖），但 spike 报告 §3「其余全拒」的表述
范围过大，必须披露后方可作为 R09 需求基线。

### 2.4 旧链对照真实性

- `git diff HEAD -- desktop/` 为空；old_chain_harness.cjs 仅 `require("../../../desktop/src/office-pdf-helper.cjs")`
  只读调用 + setPath 全重定向 /tmp + loopback 代理 + 禁后台外联开关——旧链输出确由未修改的生产 helper 产生（源码确证 + 运行复核）。
- 真实 /etc/passwd 残留核查：对 artifacts/rust-tauri/R01/T05/ 与 /tmp 复跑区全量 grep 真实系统文件特征
  （`/var/empty`、`_windowmanager`、`_usbmuxd`、`_nsurlsessiond` 等 macOS passwd 特有条目）——**零命中**；
  仅有的 `root:...:0:0:` 命中是合成 decoy（`root:DECOY:0:0:Decoy...`）。执行者「开发期真实泄漏产物已删除、
  未入证据链」声明与可见证据一致（无法证伪已删除产物曾经的内容，但仓库与证据链当前无任何真实系统文件残留）。

### 2.5 margins 单位（inch）发现——独立二分证实

本验收自跑旧链二分（A4, preferCSSPageSize=false）：
- margins=10 → exit=1，错误 "margins must be less than or equal to pageSize"（10in > A4 宽 8.27in，校验按 inch 比较）；
- margins=4 → exit=0，墨迹左缘 inkBBox=576px@2x = 288pt = **恰好 4 inch**；
- margins=0 → exit=0。
三点二分独立证实「Electron 42.8.1 printToPDF margins 数值按 inch 解释」（electron.d.ts "in pixels" 注释与实测不符）。
候选链对齐方式审查（pdf.rs）：margins 数值直传 CDP margin*（inch），无 px→in 换算——与实测生产语义一致；
S3 双链 inkBBox 逐值相同 [217,151,880,398]（本验收复跑同值）佐证。Chrome 侧 pageSize 具名→inch 映射表
与 Electron 文档一致（源码确证）。

### 2.6 性能口径审查

计时方法诚实性（源码确证）：候选 wall = bash `time.time()` 夹住 spawn→exit（含启动/清理）；print_ms 由
spike 内部 Instant 单独记录；旧链 wall = run_old_chain.mjs Date.now() 夹住 spawn→exit。两链同口径，
候选未被写得更有利（候选含一次性 Chrome 启动与 500ms 关闭等待，全部计入）。
本验收复测（S1 271,712B）：候选 median 1736.7/p95 1746.7ms（n=16，printToPDF 330ms）、旧链
median 1005.5/p95 1046ms（n=8）——与执行者 1644.3/953.5/312.5 同量级（机时差异内），比例关系
（候选 ≈1.7× 旧链、打印核心 330ms 与旧链总时长同量级）一致，冷启动归因成立（spike-result.json 逐项
timings 可查）。
R00 阈值对照声明核实：PERFORMANCE_THRESHOLDS.json 全文无 office/PDF 专项冻结指标（grep 零命中，属实）；
R00-T06 基线 812.1/835.6ms（helper 直测，239,073B 样本）与 851.8/879.4ms（产品链）在
docs/rust-tauri/R00/R00-T06_REPORT.md §表 逐值核对一致。「非阈值判定、口径对照」的定性诚实。

### 2.7 门禁回归（全部亲自复跑）

| 校验 | 命令 | 退出码 |
|---|---|---|
| T01 正向 | `python3 docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0 |
| T01 N1–N15 | `python3 docs/rust-tauri/R01/r01_t01_check_ownership.py --self-test` | 0（PASS-NEG N14/N15 等逐条复核在日志中） |
| T01 生成器 | `python3 docs/rust-tauri/R01/r01_t01_build_ownership.py --check` | 0（736 features / 69 stores up-to-date） |
| T02 roundtrip | `bash scripts/rust-tauri/r01-t02-roundtrip.sh` | 0 |
| T02 handshake | `bash scripts/rust-tauri/r01-t02-handshake.sh` | 0 |
| T02 check-generated | `bash scripts/rust-tauri/r01-t02-check-generated.sh` | 0 |
| cargo test | `cargo test --workspace --offline`（CARGO_TARGET_DIR=/tmp） | 0（**44 passed**：browser-spike 11 + kernel 7 + spike 19 + protocol 7；执行者报 "37 passed 11+7+19" 少计 protocol 7 个，见 F4） |
| cargo fmt | `cargo fmt --all --check` | 0 |
| cargo clippy | `cargo clippy --locked --workspace --all-targets --offline -- -D warnings` | 0 |

DEPENDENCY_RULES.json / DEPENDENCY_DECISIONS.md diff 审查：均为**追加式补注**（D-10 零新增依赖登记 +
模块职责补注 spike_pdf），无删改既有条目，内容与代码事实一致（Cargo.lock 零 diff 已独立证实）；
属诚实登记而非放松。pdf.rs 单元测试含 fetch 策略/百分号解码/字体 CSS 提取用例（源码确证，随 cargo test 通过）。

## 3. 发现问题

### F1（MEDIUM）候选链资源边界不覆盖 WebSocket——loopback WS 真实连通，远端 WS 仅靠测试代理兜底

- **最小重现**：`/tmp/r01t05-review/atk/atk4-websocket.html`（或 atk6-channels.html），页面内
  `new WebSocket('ws://127.0.0.1:18291/t05-ws-probe')`，经候选链渲染（allowJavaScript=true）：
  canary 日志出现真实 GET 握手命中（两次独立复现：/t05-ws-probe 与 /t05-ws-confirm），而 fetch-log.jsonl
  对该请求零记录、零 DENY。同页面对照：fetch/XHR/sendBeacon(Ping)/EventSource 打同一 canary **全部
  Fetch 层 DENY**——即 HTTP 家族全覆盖，仅 WS 逃逸。wss://canary.invalid 未连通，但其阻断来自
  测试吊具的 loopback-only 代理（proxy.log `REFUSED CONNECT canary.invalid:443`），**不是候选代码的边界**。
- **根因**：spike_pdf.rs 的拦截仅 `Fetch.enable({patterns:[{urlPattern:"*"}]})`；Chrome 153 的 Fetch 域
  不对 WebSocket 握手触发 `requestPaused`（WS 不走 Fetch 拦截通道），`decide_fetch` 策略因此从未被咨询。
  而 `--proxy-server` 形态对 loopback 目标本就放行（loopback-only 代理允许 loopback），于是 ws://127.0.0.1:*
  端到端连通。生产形态（ADR-003 规划为"Fetch 拦截 + loopback 代理形态"）下该缺口会原样继承：
  loopback WS 可达本机任意服务；无代理部署时远端 WS 亦不受候选代码约束。
- **影响面**：不能让页面读到 file://（Fetch 层仍拒），但构成①对 loopback 服务的探测/交互通道、
  ②把 allowlist 目录内可读内容经 WS 外带至 loopback 监听者的通道。对"渲染不可信 HTML"场景是真实缺口。
- **严重度**：MEDIUM。不推翻 A10（规格样本 = file:// 越权 + 无限脚本；执行者全部 DENY 声明逐条属实，
  从未声明 WS 覆盖）；但 PDF_SPIKE_REPORT §3「Fetch 域 allowlist…其余全拒」的表述范围过大，
  ADR-003 后续强制门禁将该边界形态列为正式 renderer 需求下限，缺口若不在此刻登记将静默传播。
- **处置要求（PASS 附带，不阻塞本任务判定）**：在 PDF_SPIKE_REPORT §8 与 ADR-003 门禁节补记
  「WebSocket 不经 Fetch 域拦截」已知缺口；R09 正式实现前须选定修复（如 Fetch.enable 显式 WS 图案 +
  `Network.setWebSocket...` 类机制、注入 CSP `connect-src` 收敛、或 launcher 层禁 WS），并在 A10 家族
  增补 WS 用例。

### F2（LOW）spike_pdf 超时退出码契约不一致：printToPDF 阶段超时 exit=1 而非文档承诺的 exit=2

- **最小重现**：ATK5（load 后 setTimeout 死循环，timeoutMs=6000）→ stderr/result 为
  `printToPDF: timeout: Page.printToPDF id=15`，**exit=1**（spike_pdf.rs 以 `e.contains("timed out")`
  判定超时，而 CDP 超时错误文案为 "timeout: …"）。
- **影响**：功能无损（非零退出、零产物、清理核验为空，A10 语义满足）；仅 bin 文档注释
  「超时 exit 2」与实际不符，loadURL 阶段超时（文案含 "timed out"）才走 exit=2。
- **处置**：统一超时判定（按 CdpError::Timeout 类型而非字符串匹配——pdf.rs 已有 `err_is_timeout`
  未被 main 使用），或修正文档。原型阶段任一即可。

### F3（LOW）报告计数笔误两处

- R01-T05_REPORT.md §2 称 compare_outputs.py「all_pass=true（41 项）」；实际 compare.json（执行者证据与
  本验收复跑一致）为 **35 项 checks**（all_pass=true 本身属实）。
- PDF_SPIKE_REPORT 页首称 SHA256SUMS.txt「623 项」；实际 **632 项**（R01-T05_REPORT §1 写 632 正确）。
- 严重度 LOW（实质结论不受影响，计数失准）。

### F4（INFO）执行者 cargo test 计数少报

报告称 "37 passed: 11+7+19"；本验收复跑实际 **44 passed**（另有 lingxi-protocol 7 个未计入）。
少报而非虚增，不构成可信性问题；记录以校准账本。

### F5（INFO）A09 文本层差异构成（不构成缺陷）

0.0009 相似度缺口主要由旧链（Chromium 142）文本运行顺序导致的数字串提取转置构成（见 §2.2），
位图层无对应差异；属提取层伪差。另页位图高度 2px 舍入差（1686 vs 1684 @2x）。均不影响 A09 判据。

## 4. 执行者声明核验总表

| 声明 | 核验方式 | 结论 |
|---|---|---|
| matrix 28 VERIFIED / 0 FAILED | 独立全流程重放 + analyze_matrix 复跑 | 属实（实际运行） |
| A09 74=74 页、140/140 行、0.99910、零空白、三字体双链嵌入、图片 2=2、视觉 max 7.16 | 自有探针复算（§2.2） | 逐项属实（实际运行） |
| compare 41 项 all_pass | 复算 | all_pass 属实；**41 实为 35**（F3） |
| A10 7×DENY、零泄漏 | 复跑 + 自造 6 组对抗样本 | 属实；另发现 WS 边界缺口（F1） |
| 无限脚本 exit=2 超时清理无产物 | 复跑 + 挂起变体 ATK5 | 属实（load 阶段）；print 阶段超时 exit=1（F2） |
| JS 关闭可产出 | 复跑 | 属实 |
| margins 实为 inch | 独立二分 m=10/4/0 | 证实（实际运行） |
| 旧链无拦截 file:// 可嵌入、Ext-B 缺字 | decoy 对照复跑 + 𪚥 逐字核验 | 属实 |
| 真实 /etc/passwd 产物已删除、证据零残留 | 全量特征 grep | 当前证据链干净（实际运行） |
| 性能归因冷启动、非阈值判定 | 计时源码审查 + 复测 + R00 基线逐值核对 + THRESHOLDS 无 office 指标核实 | 属实 |
| 门禁回归全绿、Cargo.lock 零 diff | 9 项全部亲自复跑 + git diff | 属实（44 passed，F4 少报） |
| 生产零改动 | git diff HEAD 全生产目录 | 属实 |

## 5. 范围与限制

- 仅 macOS arm64 实测；Windows/Linux 不在本验收环境覆盖（执行者已如实登记 UNVERIFIED → R09/R10 门禁）。
- 旧链 Electron 42.8.1 为开发态 harness；与打包发布构建的绝对性能差未在本任务校准（执行者已声明口径差异）。
- 已删除的开发期真实 /etc/passwd 产物无法回溯取证；当前仓库与证据链经特征扫描无任何真实系统文件内容。
- 预存失败（审计封印家族等）与本任务无关，未追修，符合任务书口径。

## 6. 最终判定

**PASS**。R01-A09 与 R01-A10 的通过条件在本验收的独立复跑与自造对抗样本下全部成立；执行者关键声明
逐项属实（两处计数笔误与一处超时 exit 码文档偏差为 LOW，不改实质结论）。F1（WebSocket 边界缺口）
为必须随本验收一并登记的披露项：建议在 PDF_SPIKE_REPORT §8 与 ADR-003 补记并纳入 R09 门禁需求；
该项属原型边界的完备性增强，不构成对 A10 规格判定的推翻。
