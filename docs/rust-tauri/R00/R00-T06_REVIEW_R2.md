# R00-T06 独立验收报告 R2｜测量旧版本并冻结性能协议（R1 修复后完整验收）

- 审查者：REVIEWER-R00-T06-R2（一次性独立验收代理；非 R1 审查者、非执行者、非修复者；不做修复，不进入下一 Task，不派生子智能体）
- 审查日期：2026-09-24（本地 +0800）
- 对象任务：R00-T06（验收 R00-A11、R00-A12，均 REQUIRED）——对**修复后完整 Task** 做全新对抗性验收，不只查 R1 F01–F07；不依赖 R1 的 PASS 与此前任何代理的主观结论
- 审查方式：任务书（MANIFEST 校验后）+ 候选全量阅读（协议/阈值/基线/12 脚本/原始记录）→ 从真实生产源码入口追踪 → 全部关键统计独立重算 → 一条关键链路真实隔离复测（含敏感扫描、产物指纹独立 shasum、进程清理核验）

## 0. 结论

**VERDICT: PASS**

两个 REQUIRED 验收（R00-A11 进程树测量不漏算、R00-A12 阈值先于实现结果）经全新对抗性核验与真实复测均成立。三项必须交付齐备且与原始数据、82 文件候选三方自洽。R1 的 F01–F07 全部真实修复且经本人独立重算/复现验证；重点审查的 fixed_prefix 门槛 10,620→10,626B（+6B）**裁定为数值溯源更正、非为通过测试的事后放宽**（论证见 §6.3：原值无任何落盘证据、新值双通道实测且本人第三次独立复现、G2/G3 测量结果至今不存在、change_log rev2 治理完整、更正后门槛语义精确等于任务书「不高于旧同任务固定前缀」的定义）。本轮新发现 3 项（F08–F10，均 LOW，无阻断项）与 4 项非阻断观察。

## 1. 坐标核对（全部独立重算，不沿用任何先前结论）

| 项 | 核对结果 |
|---|---|
| HEAD | `git rev-parse HEAD` = `64c302ec482888222833136d0d20c64d4cbb350b` = 任务指定 Task base ✓ |
| 分支 | `codex/rust-tauri-migration` ✓；未 commit/push/PR/tag/release（git log 顶端仍为 64c302ec4） |
| 工作区 | 仅总控账本既有未提交修改（`M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`，总控维护）+ 本任务 82 候选文件 + 本报告；无生产代码/测试改动 ✓ |
| 任务书完整性 | `Lingxi_Rust_Tauri_Taskbooks_2026-09-23` 32 文件 `shasum -a 256 -c MANIFEST.sha256` 全 OK ✓（00/01/02/03/04/05/06/90/91、R00 阶段书 T06 全文、stage-index/task-catalog R00-T06、acceptance-catalog R00-A11/A12、05 §6 全文均已读） |
| 候选 82 文件聚合 SHA-256 | 独立重建清单（12 脚本 + 3 docs + 67 artifacts，不含报告/R1 报告/总控账本/.mimosa）→ 按「SHA256+双空格+路径+换行」路径排序拼接整体哈希 = **`8f6de53d66c4a2d7d63c5c67de3376ae3e0e02ab981e4c2d8f41e40066143785`** ✓ 与任务书、报告 §8、总控账本三方一致 |
| 报告 §8 逐文件哈希 | 82/82 与工作区实际文件逐一比对一致；复测清理后候选哈希重回 8f6de53d…（候选未被本审查改变） |
| 执行报告 | `docs/rust-tauri/R00/R00-T06_REPORT.md` SHA-256 = `8efc7fcab601e41ea25f21b44a8406841970c96cf8baed10b2a79e93dd50ca6b` ✓ 与任务书一致 |
| 总控账本 | R00-T06 = INDEPENDENT_REVIEW / READY_FOR_REVIEW_AFTER_R1_REPAIR，candidate_sha256=8f6de53d…（82 文件）、report_sha256=8efc7fca…，A11/A12=PENDING_R2_REVIEW ✓（本报告为该待裁项的输入，未修改账本） |

## 2. 审查环境与实际执行（命令/退出码）

- 环境：Apple M3 Ultra（Mac15,14）/ 28 核 / 96GiB / macOS 27.0 / darwin arm64（与冻结协议 §1 同机，复测可比）；Node v24.16.0；Python 3.14。AC 电源。
- 主要命令与退出码（全部本人实际执行）：
  - 统计独立重算（Python nearest-rank p95，从 raw JSONL 直接重算 n/median/p95/min/max/CV）：server 腿最终 run 全部 8 个计时项、desktop 腿冷/热 app-ready 与 quit teardown、PDF 直测、office 产品链、PTY 1 MiB 新 run 与旧 0.4MB run——**与报告 §4 表、BASELINE_BENCHMARK.json、阈值 baseline_g1 三方逐位一致**（例：cold→info 1170.755/1195.92、health 1190.74/1206.84、SIGTERM 93.59/95.63/CV 0.0125、长会话首页 1.79/3.66/1.55/25.99/CV 1.575、ETag 1.52/4.53/0.43、短会话 1.22/1.56/0.20、翻页 33.74/21 页、流式 19.655/22.56/0.522 与 27.485/31.63/0.403、取消 2.985/6.46 与 2.74/6.22、PTY total 756.63/787.65/736.14/897.02/0.0359 与首块 403.39/424.79/0.0598、desktop 9227.17/9375.29/0.0142 与 9166.27/9707.36、teardown 258.71/307.56/0.0464、pdf 812.06/830.67/0.0103、office tool_end 851.75/879.37/844.39/906.88/0.0144、旧 PTY 639.57/652.94）——退出码 0。
  - **真实复测（随机抽取关键链路）**：`node scripts/rust-tauri/r00-t06-probe-fixed-prefix.mjs` → **退出码 0**（详见 §4-F04：10,626B 双通道第三次独立复现）；复测自产 raw 文件已删除、临时目录已确认清空、`ps` 核查 0 残留进程、候选哈希复核不变。
  - 产物指纹独立核验：`shasum -a 256` 盘上 `dist-server/mac-arm64/bundle/index.js` = `5095cc7f…`（== build-release-summary == 最终 server run summary）；`dist/mac-arm64/Lingxi.app/Contents/Resources/app.asar` = `7b8478ab…`（== summary）；mtime 2026-09-23T21:37:44Z / 21:38:54Z 与 summary 的 artifactMtimes 一致（**原始被测构建仍在盘、未被重建**）。
  - `node --check scripts/rust-tauri/r00-t06-*.mjs`（12 文件）全过；JSON 可解析（阈值/基线/build summary/全部 summary/探针证据，19 文件）全过。
  - A12 前提独立复核：无 `rust/` 目录、无 Cargo.toml、无 .rs 源文件（全仓 find）。
  - 敏感信息扫描（私钥/sk-/api_key/ghp_ 模式）：候选 82 文件 0 命中；token 仅合成 `not-a-secret` 类。
  - raw 文件计数：server 42 + desktop 23 + build 1 = 66 ✓（报告 §2 声称一致；`.mimosa/` 2 处为 R1 审查插件残留、.gitignore 项、不入候选，报告已声明）。

## 3. 生产链真实性核验（从真实源码入口追踪，非读日志）

| 声明 | 生产源码/原始数据证据 | 裁定 |
|---|---|---|
| cache 契约诊断开关 | `core/session-coordinator.ts:381` `LINGXI_CACHE_CONTRACT_DEBUG === "1"`（报告写 :380，±1 行） | ✓ |
| systemPromptBytes 字节定义 | `lib/llm/cache-prefix-contract.ts:81` `Buffer.byteLength(systemPromptText, "utf8")` | ✓ |
| run_code 主聊天不可用（PTY 直驱理由） | `core/session-coordinator.ts:2201` 附近 buildTools 无 sessionRef → engine 降级 null → run-code "no active session"（R1 已源码级定位，本轮抽样复核未推翻） | ✓ |
| 隔离 | fixture.mjs：provider-catalog→127.0.0.1 stub、config.yaml 副本+memory enabled:false、每样本全新 LINGXI_HOME/HOME 重定向（raw 行内 home 均为 `/var/folders/.../lingxi-r00t06-*` 临时目录） | ✓ |
| 进程树口径 | `r00-t06-lib.mjs` collectTreePids 每帧 `pgrep -P` 递归重走 + `ps` 同窗读数；帧含 pid/ppid/rssKb/vszKb/pcpu/etime/comm + vanished 字段 | ✓ |
| darwin→mac 归一化（F02 修复） | bench-server:43、bench-desktop:508、build-release:42 OS_DIR_NAME + 产物缺失抛错 | ✓ |
| PTY 夹具（F06 修复） | bench-pty:48-49 `LINE_TEXT` 127 字符 + `\n` = 128B/行，**启动断言长度≠127 即抛错**；新 run outputBytes 全 32 样本恒为 1,056,849B（≥1 MiB 夹具实际交付） | ✓ |
| stub journal 构成 | 最终 run journal 348 行分类：fast 主聊天 32 行（28,598×10/28,599×22 → median 28,599）、hang 64 行（28,590×20/28,591×44 → 去重 median 28,591）、666/665B 带 marker 32 行（标题侧线）、1,653/3,966/2,925B（persona editor/标题重试侧线）——与基线 modelWaitSeparation 与探针 roles 逐类吻合 | ✓ |
| 真实供应商 | `realProviderMeasurements.calls=0`；stub/probe 全部 127.0.0.1；更新检查死端口 | ✓ |

## 4. R1 F01–F07 修复逐项复核（全部独立验证，不采信修复记录自述）

- **F01/F01b/F01c（被弃 run 数字误植）**：报告 §4 server 五行（→health 1190.7/1206.8/1171.8/1212.2/0.009、SIGTERM 93.59/95.63/91.06/95.98/0.012、ETag 1.52/4.53/1.33/4.75/0.43、短会话 1.22/1.56/0.86/2.29/0.20、取消流停 2.74/6.22/1.37/6.57/0.42）经我从最终 run raw 逐位重算**全部一致**；三处 CV 笔误（桌面退出 0.046、office 0.014、SIGTERM 0.012）与重算值一致；阈值 desktop_teardown 258.71/307.56/CV0.046 与 server_rss 451,504 均与最终 run raw 一致；内存表 451,504 与 idle-tree rssSeriesKb 中位数一致。**修复属实**。
- **F02（构建指纹空值）**：盘上产物指纹/双口径大小/mtime 三者互证（§2）；repair-build-summary.mjs 守卫逻辑真实（bundle SHA≠最终 run summary 记录值即 fail、asar SHA≠原 summary 即 fail，绝不写猜测值）；原失真值在 repairNote 留档；基线 build 块回填完整。**修复属实，且产物未重建的被测一致性由我独立 shasum 证实**。
- **F03（双口径）**：build summary/基线/阈值/报告四处全部带 logicalBytes/duBytes 双字段+口径注释+calibersNote；阈值 25% 收益锁定 du 字段并指明两处同值互证（812,728,320）。**修复属实**。
- **F04（固定前缀无证据）**：见 §6.3 专项。**修复属实且经本人第三次独立复现**。
- **F05（frozen_at）**：现值 23:20:53Z + frozen_at_basis 完整交代 23:15 名义时刻来历与不可能性论证；change_log rev2 留 from/to/reason。rev1 的 mtime 取证在案于 R1 报告（本审查无法回溯 rev1 文件，见 O2），但 A12 实质前提不依赖时间戳（§6.2）。**修复属实**。
- **F06（PTY 0.4MB 冒称 1MB）**：不采纳缩协议选项、夹具提到协议值（128B/行断言）并以真实组件栈重测 32 样本（run 01-45-52-350Z，outputBytes=1,056,849B、transcript 完成标记 32/32、leftoverHelperProcs=0）；旧 0.4MB run 原样保留作过程对照；协议 §8/§12 变更记录留档；基线 ptyLeg 指向新 run。**协议未缩小、真实 1 MiB 已入基线。修复属实**。
- **F07（杂项）**：bench-server 旧注释已改、协议补 memory 开关条件与 §11 三条命令、报告指针改实值。**修复属实**。

## 5. R00-A11 裁定：PASS（进程树测量不漏算）

- 前置「基线启动浏览器及 PDF helper」：成立——浏览器 5 会话经真实链路（WS prompt→模型替身 tool_use→mcp_call→browser 工具→BrowserManager→/internal/browser→Electron WebContentsView），navigate 5/5 `toolNames=['mcp_call']`、0 errorEvents、往返 median 21.43ms（本人重算）；PDF 双证据：打包二进制直测 32/32（%PDF、确定性 174,571B）+ 完整产品链 32/32（toolStatus 全 succeeded、确定性 175,218B）。
- 「主程序与全部辅助进程均计入，采样窗口一致」：桌面空闲 5 进程（主+2Helper+server+Renderer）、开 5 视图后 6 进程（+1 同源共享 Renderer，真实 Chromium 行为）逐帧稳定；**office 链关键证据经本人核验**：转换窗 8×300ms 采样树 1→5→5→1 进程（procsSeen 含打包 Lingxi helper 主进程 + 2 Lingxi Helper + Lingxi Helper (Renderer)，RSS 峰 1,033,888KB，转换后回落 1 进程 493MB）——helper 以 server 子进程同窗入树，无漏算。
- 退出不漏算：桌面 64 样本（冷+热）quit 全部 `forced=false`、leftoverMainPids/leftoverServerPids 全空（本人遍历核验）；server finalShutdown procsAfter=0；office 退出 102.29ms；PTY 残留 0；探针/复测清理均 0 残留。
- 边界如实登记（WebContentsView 进程内形态、PTY 直驱理由、office standalone server env 合同），不构成对 A11 规格的偏离。

## 6. R00-A12 裁定：PASS（阈值先于实现结果）

### 6.1 前提「尚未产生 Rust 性能数据」
本人独立复核：全仓无 `rust/` 目录、无 Cargo.toml、无 .rs 文件；G2/G3 未实现。**「冻结先于 Rust 结果」成立**（该前提与任何时间戳字段无关，可随时独立复核——本审查即第三次独立复核）。

### 6.2 门槛完整性与治理
- 8 项门槛（取消 p95≤250ms、清理 3s/5s、历史读取 p95≤24ms=max(2×p95,p95+20ms)、固定前缀 ≤10,626B、长时增长 2h/1000 任务斜率、迁移收益预选分发占用 ≥25%、桌面启动 p95≤19.4s、空闲树内存 ≤1,573,000KB）均具备固定值+单位+比较方向、applies_to/scope、baseline_g1（含 run ID）、noise_tolerance、rationale+source；judgment_rules 五条（禁弃离群、禁混构建、模型等待分离、平台诚实、失败诚实）齐备；与任务书 05 §6.2 六类一一对应。
- 平台诚实：darwin-arm64=MEASURED，其余三平台 PENDING_MEASUREMENT 且明令禁止外推 ✓。
- change_log 治理：rev2 逐项 from/to/reason，且每项 from 值均可对回被弃 run raw 或「无落盘证据」的事实（本人重算/检索复核）；除 fixed_prefix 外全部门槛判定值与 R1 验收报告 §5 记录的 R1 时值一致（250/3000/5000/24/长时/25%/19400/1573000）——**无借修复之机改动其他门槛**。
- 长时资源增长 G1 基线 NOT_RUN_THIS_ROUND 登记 + R10 补测承诺（R1 已裁定按规格可接受，本轮维持：T06 步骤负载清单不含 2h 长跑，05 §6.2 允许冻结规则而非当轮执行）。

### 6.3 专项裁定：fixed_prefix 10,620→10,626B（+6B）是纠正错误来源，不是违反任务书的事后放宽

- **原值无证据**：R1-F04 确认 10,620 在全部 artifacts 中无落盘记录；本审查独立 grep 复核——10620 仅出现于总控账本（历史 finding 文字）、阈值 change_log 的 from 值、探针脚本注释、T02 审计文件的无关哈希子串，**无任何测量数据支持**。
- **新值三重实证**：探针两次（01-48-37/01-50-15）wire 通道主聊天 system 消息 10,626B + 产品诊断通道 drift_auto_renew systemPromptBytes=10,626（cache-prefix-contract.ts:81 定义=UTF-8 字节）双通道一致；**本人第三次重跑探针（02-16-03，退出码 0）复现同值**（wire 10,626/tools 10,287/请求体 28,599–28,607B；诊断 10,626×2 会话）。脚本无硬编码（systemMessageBytes 由 Buffer.byteLength 实测）。
- **666B 侧线与主聊天请求的区分属实**：探针 roles 字段实证 666B 请求 =「对话标题生成器」侧线（system 403B、无 tools、marker 在其 user 对话摘要内——这正是 R1 表面矛盾的成因）；主聊天请求 28,599/28,591B = system 10,626 + tools schema 10,287 + user 数组 ≈7.2KB，与 stub journal 逐类吻合。
- **不可能「为通过测试」**：修改发生于 R1 修复轮（2026-09-24），此时及本审查时点 G2/G3 结果均不存在（无 rust/ 数据）——不存在任何可被该 +6B「放通过」的测试。
- **语义回归**：门槛定义「不高于旧同任务固定前缀」——旧系统真值即 10,626；保留无证据的 10,620 将对新系统施加比旧系统真实值再小 6B 的超任务书要求。更正使门槛精确等于任务书语义。
- **张力如实登记**：R1 §5 附带条件曾要求「门槛数值本身不得变动」，其 §9.4 同时允许「指标证据补采或口径重定义」。修复轮选择了补采证据并按证据更正数值、走完整 change_log。本审查裁定：该处理符合 R1 条件的**治理实质**（不得借机放宽以通过测试——不存在待通过测试；修正走 change_log——已走）与任务书 05 §6.2 字面（「之后修改需要独立变更记录，不能为了通过测试放宽」——有独立变更记录、无测试可过）。若未来 G2/G3 出现后门槽数值再向宽松方向变动而无测量错误证据，即属违规——此为本裁定的明确边界。

## 7. 发现（本轮新发现，全部 LOW，无阻断项；编号延续 R1 的 F01–F07）

### F08｜LOW｜BASELINE_BENCHMARK.json 未汇总 office 产品链指标
- 证据：基线顶层 serverLeg/desktopLeg/ptyLeg/modelWaitSeparation 均无 office 产品链块；851.8/879.4ms 等 office 链数据仅存在于 `raw/server/office-pdf-*.jsonl`、`summary-office-*.json` 与报告 §4 表。summarize.mjs 无 `--office-run` 参数（grep 证实）。
- 根因：office 补测是 R1 前的追加相，汇总器未同步扩展。
- 同类范围：仅基线汇总入口；raw/summary/报告表数据完整且自洽（本人重算一致）。
- 后果：G2/G3 以基线为单一消费入口时会漏掉该行，需回读 raw summary。
- 修复要求：下轮 summarize 增加 office 块（或在基线 modelWaitSeparation/desktopLeg 加显式指针）。
- 必须重跑：无（数据已在 raw）。

### F09｜LOW｜字节数组统计块 unit 标签沿用 "ms"（修复轮新产物仍复现）
- 证据：新 PTY run（修复轮产物）`outputBytes`/`transcriptBytes` 统计块 `unit:"ms"`；desktop 旧 run `pdfBytes` 同。R1_REPAIR §7 已登记该 stats() 默认标签笔误但未修脚本，故修复轮新产物继续复现。
- 根因：stats() 助手对字节数组未接受显式 unit。
- 后果：纯标签问题（数值/字段名正确，报告/基线表已按 bytes 呈现）；对 G2/G3 消费有轻微误导可能。
- 修复要求：stats() 支持显式 unit 并在下次产物的脚本演进时统一；历史 raw 保持原样（测量记录不回改的决定可接受）。
- 必须重跑：无。

### F10｜LOW｜「逐样本记录的 serverBundleSha256」措辞与实际记录粒度不符
- 证据：报告 §3/§9 与 R1_REPAIR §2 称指纹「与最终 server run 逐样本记录的 serverBundleSha256 字节一致互证」；实际逐样本 startup JSONL 行内无该字段（本人遍历为空集），该值记录于 run summary（单处）。
- 根因：措辞失准（守卫实现本身读的正是 run summary，行为与措辞不一致的是文字而非代码）。
- 后果：互证实质成立（盘上字节 == summary 记录值，本人独立 shasum 双向核实）；仅证据描述粒度失真。
- 修复要求：下轮文档措辞更正为「run summary 记录」。
- 必须重跑：无。

### 非阻断观察（不需修复裁决，供后续阶段输入）
- **O1**：流式首增量 CV=0.522 > 0.5，未按协议 §5「CV>0.5 单独标注解释」加注（长会话首页 CV 1.57 有脚注；0.52 仅略超阈值且 max 83.65ms 离群已入原始数据）。R08 前报告补一句解释即可。
- **O2**：frozen_at_basis 引用的 rev1 mtime（2026-09-23T23:20:53Z）取证在案于 R1 报告，rev2 覆盖后本审查无法直接复核该历史 mtime（当前文件 mtime 为 rev2 写入时刻 2026-09-24T09:54:07+0800，合理）。A12 实质前提不依赖该时间戳（§6.1），不构成风险。
- **O3**：浏览器 start 工具仅首会话记录 tool_end（n=1 如实入库，R1-O4 维持）。
- **O4**：raw/ 下 2 处 `.mimosa/hook-state/`（R1 审查方插件残留）不入候选、属 .gitignore 项，报告已声明；建议总控在提交前清理目录以避免误入库。

## 8. 授权与安全合规

- 本审查未修改任何产品源码、脚本、阈值、基线、协议、执行/修复报告、R1 报告或总控账本；仅新增本报告；未 commit/push/PR/tag/release；未派生子智能体；未进入 R00-T07。
- 复测全程隔离：mkdtemp 临时 LINGXI_HOME/HOME、127.0.0.1 本地确定性模型端点、合成 token、零真实供应商/付费 API/真实外发/真实用户数据；复测自产 raw 文件与临时目录已删除，自启进程（打包 server、探针）SIGTERM 整树退出 109ms、`ps` 复核 0 残留；候选聚合哈希复测前后一致（8f6de53d…）。

## 9. 验收停止点

R00-A11=PASS、R00-A12=PASS、三项交付齐备且与 82 文件候选（聚合 SHA-256 `8f6de53d66c4a2d7d63c5c67de3376ae3e0e02ab981e4c2d8f41e40066143785`）一致、无 BLOCKING 发现、F08–F10 均 LOW 不阻断、相关回归（脚本语法/JSON 解析/统计重算/链路复测）实际执行。验收到此停止；不执行修复，不进入下一 Task。后续轮次（若总控要求处理 F08–F10）注意 §6.3 的门槛治理边界。
