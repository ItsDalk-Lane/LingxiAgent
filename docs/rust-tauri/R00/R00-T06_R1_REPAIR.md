# R00-T06｜R1 验收后修复轮记录（REPAIR-R00-T06-R1）

- 修复代理：REPAIR-R00-T06-R1（一次性修复代理；只处理 R1 验收列出的 F01–F07，不做独立验收、不自封 PASS）
- 日期：2026-09-24（本地 +0800）
- 输入：[R00-T06_REVIEW_R1.md](R00-T06_REVIEW_R1.md)（VERDICT: PASS + F01–F07 须修复轮处理；原样保留作历史证据）
- 基线：HEAD `64c302ec482888222833136d0d20c64d4cbb350b`（分支 `codex/rust-tauri-migration`，未提交，与 Task base 一致）；总控账本 `ORCHESTRATOR_PROGRESS.json` 未触碰
- 边界：未建分支/worktree、未 commit/push/PR/tag/release、未改生产代码；全部实测用隔离 LINGXI_HOME/OS 临时目录 + 本地确定性模型端点（127.0.0.1），零真实供应商/付费 API/真实外发/真实用户数据；未派生子智能体
- 结论：**F01–F07 全部处理完毕**；脚本/数据/协议有实际变更，R1 的 PASS 不再覆盖当前候选，交由新验收轮复核。执行者状态 READY_FOR_REVIEW（见报告抬头）

## 0. 修复轮总表

| 项 | 严重度（验收定级） | 处理 | 涉及文件 |
|---|---|---|---|
| F01/F01b | MEDIUM | 报告 §4 五行+CV 笔误、阈值 desktop_teardown/noise_tolerance 按最终 run 重算更正 | REPORT、THRESHOLDS |
| F01c（同族新增） | —（本轮发现） | 报告内存表 server 空闲 RSS、阈值 server_rss_median_kb 同源更正（455,200→451,504） | REPORT、THRESHOLDS |
| F02 | MEDIUM | darwin→mac 归一化修复 + 盘上原始产物不重建重算指纹（守卫通过）+ 基线 build 块回填 + 报告指针更正 | build-release.mjs、bench-desktop.mjs、repair-build-summary.mjs（新）、build-release-summary.json、BASELINE、REPORT |
| F03 | LOW | 双口径（statSync 逻辑字节 vs du 分配字节）在 build summary/基线/阈值/报告全部标注；25% 收益对照锁定 du 口径 | 同上 + THRESHOLDS |
| F04 | MEDIUM | 真实会话路径双通道补采：systemPromptBytes=**10,626**（诊断=wire 一致）；666B/28,590B 构成查明；阈值/基线/报告按实测更正（10,620→10,626，证据驱动） | probe-fixed-prefix.mjs（新）、探针 raw×2、THRESHOLDS、summarize.mjs、BASELINE、REPORT |
| F05 | MEDIUM | frozen_at 更正为 rev1 首次写入的可核证 mtime 23:20:53Z；23:15 来历在 frozen_at_basis + change_log rev2 交代 | THRESHOLDS、REPORT |
| F06 | LOW | **不采纳**「协议缩成 0.4MB」选项；夹具提到协议值 128B/行=1 MiB，真实组件栈重测 32 样本；旧 0.4MB 结果保留；PDF 输入如实注明实际 239,073B | bench-pty.mjs、新 pty raw×2、PROTOCOL §8/§12、BASELINE、REPORT |
| F07 | LOW | bench-server 旧 run_code 注释更正（含 env 注释）、bench-pty 旧「~76B≈0.6MB」注释更正、协议补 memory 开关测量条件、报告 §2/§3 指针改实值、协议 §11 补命令、新增协议 §12 变更记录 | bench-server.mjs、bench-pty.mjs、PROTOCOL、REPORT |

缩写：REPORT=`docs/rust-tauri/R00/R00-T06_REPORT.md`、THRESHOLDS=`docs/rust-tauri/PERFORMANCE_THRESHOLDS.json`、PROTOCOL=`docs/rust-tauri/R00/R00-T06_PROTOCOL.md`、BASELINE=`artifacts/rust-tauri/R00/T06/BASELINE_BENCHMARK.json`。

## 1. F01/F01b/F01c｜被弃 run 数字误植（溯源更正，门槛不变）

**核对方法**：Python 从最终 run raw JSONL 与两个被弃 run summary 逐位对照报告 §4 每行与阈值 baseline_g1（验收已独立重算最终 run 统计，本轮直接对表）。

**报告 §4 更正（5 行 + 2 处笔误）**：

| 指标 | 原报告（=被弃 run 22-05-11-181Z） | 更正（最终 run 23-12-26-989Z） |
|---|---|---|
| Server 冷启动 →health 200 | 1188.4/1211.3/1151.8/1211.8/0.013 | 1190.7/1206.8/1171.8/1212.2/0.009 |
| Server SIGTERM→整树消失 | 93.3/95.5/92.1/96.5/0.010 | 93.59/95.63/91.06/95.98/0.013 |
| 长会话 ETag 304 | 1.44/1.94/1.14/2.01/0.13 | 1.52/4.53/1.33/4.75/0.43 |
| 短会话首页 | 1.31/4.40/1.04/5.35/0.58 | 1.22/1.56/0.86/2.29/0.20 |
| 取消 abort→流停 | 4.16/5.81/1.97/5.95/0.31 | 2.74/6.22/1.37/6.57/0.42 |
| 桌面退出行 CV（数值本身已是最终 run） | 0.005 | 0.046（cold 序列 stdev≈12.1ms） |
| office 产品链行 CV（修复轮程序化对表新发现，验收未点名） | 0.010 | 0.014（raw cv=0.01437） |
| SIGTERM 行 CV 舍入 | 0.013 | 0.012（raw cv=0.01248） |
| 内存表 Server 空闲 RSS（验收未点名，同族） | 455,200（=被弃 22-05-11 run） | 451,504（最终 run idle-tree 实测） |

**阈值文件更正（change_log rev2 留档 from→to 与理由）**：
- `process_tree_cleanup.baseline_g1.desktop_teardown_all_gone_ms`：258.3/260.7（被弃桌面 run 22-23-53-802Z）→ **258.71/307.56**（最终 run 22-47-51-720Z）；noise_tolerance「CV≈0.004（≈1ms）」→「CV≈0.046（stdev≈12.1ms）」。门槛 3000/5000ms 不变。
- `idle_memory_regression.baseline_g1.server_rss_median_kb`：455,200（被弃 22-05-11 run）→ **451,504**。门槛 1,573,000KB 不变。

其余报告行、BASELINE 数值经逐位核对本就是最终 run 值，未动。无需重测（数据在 raw）。

## 2. F02｜darwin→mac 路径 bug → 构建指纹空值（修复 + 不重建重算）

**脚本修复**：
- `r00-t06-build-release.mjs`：`PLATFORM_ARCH` 改用归一化 `OS_DIR_NAME`（darwin→mac）——`dist-server/<mac-arch>` 与 `dist-server-artifact/<mac-arch>` 盘点路径落空的根因；产物目录缺失时**抛错**（不再静默记 0）。
- `r00-t06-bench-desktop.mjs` size 阶段：`dist-server`/`dist-server-artifact` 同样按 mac- 归一化；du 失败/目录缺失显式记 error 并计入 FAIL（不再静默省略）。

**指纹重算（不重建，`r00-t06-repair-build-summary.mjs`）**：被测产物自原始构建（2026-09-23T21:37–21:38Z，文件 mtime 在案）一直在盘、未被重建。重算前置守卫（任一不过即非零退出、不写文件）：
1. 盘上 `dist-server/mac-arm64/bundle/index.js` SHA-256 `5095cc7ff74f60bfe0ca02332ad9455aabdb9abb065e3b82135d36ec76e6081a` **==** 最终 server run（23-12-26-989Z）summary 逐样本记录的 `serverBundleSha256`（被测字节一致性的原始记录互证）；
2. 盘上 `app.asar` SHA-256 `7b8478ab…` **==** 原 build-release-summary 记录（该块首轮路径正确）。

重算结果（回填 build-release-summary.json，保留原 buildId/构建时间/git 快照，新增 `rebuilt:false`/`recomputedAt`/`recomputeReason`/原失真值留档；错误口径的旧值在 `repairNote` 保存对照）：

| 产物 | logicalBytes（statSync） | duBytes（du -sk） | 指纹 |
|---|---|---|---|
| dist-server/mac-arm64 | 672,507,925 | 713,519,104 | bundle/index.js = `5095cc7f…` |
| dist/mac-arm64/Lingxi.app | 971,247,121（==原 summary 构建时值，产物未变的二次互证） | 812,728,320（==桌面腿 size 阶段 du，口径互证） | app.asar = `7b8478ab…` |
| dist-server-artifact/mac-arm64 | 353,708,203 | 353,718,272 | 4 文件逐个 SHA-256（renderer tar.gz / seed json / sig / server tar.gz） |

BASELINE build 块经 summarize 重读新 summary 回填；报告 §2/§3 指针改为真实指纹值。**若必须重建**的情形（产物不在盘）脚本显式失败提示另获授权，本轮未发生。

## 3. F03｜分发占用双口径标注

- build-release-summary 与 BASELINE：每个产物同时记 `logicalBytes`（statSync 逐文件求和，注明口径）与 `duBytes`（du -sk，协议 §9 冻结口径），并加 `calibersNote`（禁止跨口径相减或混算）。
- THRESHOLDS `migration_benefit_distribution.scope`：明确 G3 25% 收益对照只取 du 字段——`desktopLeg.distributionSize.lingxiApp.bytes`（812,728,320，与 `build.desktopApp.duBytes` 同值互证）；`logicalBytes=971,247,121` 仅参考（APFS 分配/逻辑差 ≈158MB），取错基数会算错收益。
- REPORT §4 分发占用行同步标注。

## 4. F04｜固定前缀 10,620B 无证据 → 双通道实测 **10,626B**

**补采工具**：`scripts/rust-tauri/r00-t06-probe-fixed-prefix.mjs`（新增，可复现命令落报告 §9）。真实打包 server（dist-server/mac-arm64/hana-server）+ 冻结夹具（bench-fast、memory off）+ 隔离临时 LINGXI_HOME，复现基准的两类会话（与 bench-server 同入口同参数）：
- A stream 型：`POST /api/sessions/new {memoryEnabled:false, permissionMode:"operate"}` → WS prompt `bench-stream-0 direct answer, no tools.`（fast）
- B cancel 型：新会话 → WS prompt `bench-cancel-0 hold the stream.`（hang）→ 首 5 chunk 后 WS abort → abort_result

**双通道结果（raw：`fixed-prefix-probe-2026-09-24T01-50-15-634Z.json`；首版脚本诊断行截断缺陷修正后重跑，两份均保留）**：
1. 产品诊断通道：`LINGXI_CACHE_CONTRACT_DEBUG=1`（core/session-coordinator.ts:380 产品自带开关）→ `cache_contract`（drift_auto_renew 后）`systemPromptBytes=10626`，toolCount=7。字节定义：会话最终 system prompt 的 UTF-8 字节（lib/llm/cache-prefix-contract.ts:81）。
2. wire 通道（探针最小 openai-completions 应答器逐请求分解）：主聊天请求（stream=true、toolCount=7）首条 system 消息 = **10,626B**；tools 数组 JSON = 10,287B；user 消息为数组形态含上下文 ≈7.2KB。

**实测不支持 10,620**（差 6B，原值无任何落盘证据）→ 按真实口径更正：THRESHOLDS `fixed_prefix_overhead` threshold.value 10620→**10626**、baseline_g1 重写（system_prompt_bytes=10626、main_chat_request_body_bytes=28,599、title_sidecall_request_body_bytes=666、tools_schema_json_bytes=10,287、measurement 含会话类型/双通道字节定义/复现命令/evidence 路径），change_log rev2 完整记录（数值溯源更正，非为通过测试放宽——G2/G3 结果不存在）。方向说明：+6B 是把无证据的数改成实测数，非放宽行为，且 666B 字段原语义错误（见下）。

**666B / 28,590B 构成查明**（stub journal + 探针一致）：
- **666B = 「对话标题生成器」侧线调用**（system 403B「你是一个对话标题生成器…」+ user 142B 含首轮对话摘要——bench marker 文本因此出现在其 user 消息里，被 summarize 的 marker 关联误当成了主聊天请求体；这也是 R1 验收「666B 不可能含 10.6KB system 消息」矛盾的解释）。
- **28,590/28,599B = 主聊天请求体**（system 10,626B + 7 工具 schema 10,287B + user 上下文数组）；cancel 相（hang）journal 同构 28,591B；两相 marker 均为 None 的原因：主聊天 user 消息是数组形态且 marker 提取仅对字符串 content 生效——探针 roles 字段实证。
- 基线 `modelWaitSeparation` 相应重构：`fixedPrefix`（666B 块）改注明为标题侧线调用，新增 `mainChatRequest`（fast 32 行 median 28,599B / hang 按 seq 去重 32 行 median 28,591B / 探针分解 systemMessageBytes=10,626、toolsJsonBytes=10,287）。

## 5. F05｜frozen_at 不可信（按可核证历史更正）

- 证据链：THRESHOLDS rev1 引用的最新数据 = 最终 server run summary（写入于 2026-09-23T23:17:18Z，文件 mtime）；rev1 文件首次写入 mtime = 2026-09-24T07:20:53+0800 = **2026-09-23T23:20:53Z**（R1 验收 F05 已取证；修复轮改动前再次 `stat` 复核一致）。
- 更正：`frozen_at` 23:15:00Z → **2026-09-23T23:20:53.000Z**；新增 `frozen_at_basis` 说明 23:15 来历（执行者草拟的名义时刻，早于其引用数据实际产出时刻，不可能成立）。
- 「冻结先于 Rust 结果」不依赖该字段：修复轮再次独立复核工作区无 `rust/` 目录、无 Cargo.toml、无任何 Rust 性能数据，并把该复核写入 `frozen_before_rust_results.evidence`。未虚构任何历史。

## 6. F06｜PTY 夹具 0.4MB < 协议 1MB（提夹具重测，不缩协议）

- **不采纳**验收报告列出的「协议改成 0.4MB」选项（任务书 05 §6.1：冻结后不得因结果缩小负载；总控指令明确）。
- 夹具修复：`bench-pty.mjs` LINE_TEXT 提为 127 字符 + `\n` = **128B/行**，8192 行 = **1,048,576B = 1 MiB**（启动时断言行宽，注释记录 R1-F06 变更与旧值）；顺带更正原「~76B ≈ 0.6MB」错误注释。
- 重测（真实 node-pty/TerminalSessionManager/terminal-ws-bridge/TerminalOutputStream 栈，run `pty-2026-09-24T01-45-52-350Z`，32 样本，退出码 0）：

| 指标 | 旧 0.4MB run（22-12-53-212Z，保留） | 新 1 MiB run（01-45-52-350Z，入基线） |
|---|---|---|
| outputBytes/样本 | 401,441B | 1,056,849B（含 \r\n 转换与完成标记） |
| 全程 median/p95 | 639.6 / 652.9ms | **756.6 / 787.6ms**（min 736.1 / max 897.0 / CV 0.036） |
| 首块交付 median/p95 | 416.2 / 431.4ms | **403.4 / 424.8ms**（min 387.8 / max 531.0 / CV 0.060） |
| transcript 完成标记 / 残留 | 32/32 / 0 | 32/32 / 0 |

- BASELINE ptyLeg 更新为新 run（output 规格行 lineBytes 48→128）；REPORT §4 PTY 两行更新并在脚注保留旧值对照；PROTOCOL §8 PTY 行注明实际夹具与首轮偏差、§12 变更记录留档。
- PDF 输入：协议 §8 如实注明实际 **239,073B**（目标 200KB 按字符取整 + 中文多字节的确定结果，非事后缩小）；bench-desktop 注释同步。

## 7. F07｜杂项失准 + 同类搜索

- (a) `bench-server.mjs` 头注释 pty.bigoutput 阶段块删除，改为注明该阶段移至 bench-pty 及原因；spawnServer 的 PATH 注释同步（env 保留以与已冻结 run 条件一致）。
- (b) PROTOCOL §8 LINGXI_HOME 行补「记忆总开关 enabled:false」测量条件与理由（fixture.mjs 既有行为）。
- (c) REPORT §2/§3「指纹见 build-release-summary」落空指针改为实际指纹值（F02 一并修）。
- 同类搜索（grep 全候选 + 数值对表）命中并处理：F01c（server 空闲 RSS 两处）、bench-pty 旧注释、bench-desktop PDF 注释、PROTOCOL §11 缺 bench-pty/探针/重算命令（补全）。另发现 `stats()` 助手对字节数组沿用默认 `unit:"ms"` 的标签笔误（存在于新旧 raw summary，含本轮 PTY run 的 outputBytes/transcriptBytes 字段）——**纯标签问题**（数值与字段名正确，报告/基线表已按 bytes 呈现），为保持 raw 测量记录原样未回改历史文件，登记于此供下轮脚本演进时统一。

## 8. 隔离、清理与安全

- 全部实测（build summary 重算、PTY 重测、固定前缀探针×2、基线重汇总）使用：隔离临时 LINGXI_HOME（mkdtemp，结束删除）、HOME 重定向、本地 127.0.0.1 端点、合成 token（`not-a-secret` 类）；零真实用户数据/真实供应商/付费 API/外呼。
- 进程清理：PTY 重测 leftoverHelperProcs=0；探针 server SIGTERM 整树 110ms 退出、`ps` 复核 0 残留；修复轮未再启动桌面/office 负载。临时目录（lingxi-r00t06-fpprobe-* 等）已删除。
- 敏感信息：新增文件无真实密钥/令牌（探针 excerpt 仅含产品提示词开头与合成对话）；测试签名密钥仍只在 OS 临时目录，未入仓库。
- 本轮未修改：`ORCHESTRATOR_PROGRESS.json`（总控）、`R00-T06_REVIEW_R1.md`（历史证据原样）、生产代码、任务书。

## 9. 候选集变化（相对 R1 验收时 75 文件）

新增 7 个：`scripts/rust-tauri/r00-t06-probe-fixed-prefix.mjs`、`scripts/rust-tauri/r00-t06-repair-build-summary.mjs`、`docs/rust-tauri/R00/R00-T06_R1_REPAIR.md`（本文件）、`artifacts/.../raw/server/fixed-prefix-probe-2026-09-24T01-48-37-477Z.json`（过程记录）、`artifacts/.../raw/server/fixed-prefix-probe-2026-09-24T01-50-15-634Z.json`（canonical）、`artifacts/.../raw/server/pty-driver-pty-2026-09-24T01-45-52-350Z.jsonl`、`artifacts/.../raw/server/summary-pty-2026-09-24T01-45-52-350Z.json`；修改 9 个既有候选：5 个脚本（build-release/bench-desktop/bench-server/bench-pty/summarize）+ PROTOCOL + THRESHOLDS + build-release-summary.json + BASELINE_BENCHMARK.json；另 REPORT 自身同步更新（REPORT 不在候选集内，其哈希另列，见 §10）。完整 82 文件清单与哈希见 REPORT §8。`.mimosa/`（R1 审查方扫描插件残留）为 .gitignore 项，不入候选。

## 10. 修复轮未做 / 留给下轮验收

- 不执行 R00-T07；不提交/推送；不重跑未受影响的性能腿（server/desktop/office：产物未重建、字节一致性已由守卫互证，原始数据仍为被测证据）。
- 需新验收轮重点：见 REPORT §11 第 7 条。
