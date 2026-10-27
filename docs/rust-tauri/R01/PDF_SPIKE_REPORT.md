# R01-T05 HTML→PDF 渲染候选 Spike 报告（PDF_SPIKE_REPORT）

- 任务：R01-T05；验收场景 R01-A09 / R01-A10（REQUIRED）
- 基线 HEAD：`5a8a8e24ab9e8e105da5132d16b175221a91d646`；分支 `codex/rust-tauri-migration`
- 平台：macOS 27.0 arm64（Apple M3 Ultra）实测；Windows/Linux UNVERIFIED（R09/R10 门禁）
- 日期：2026-09-25
- 证据根：`artifacts/rust-tauri/R01/T05/`（SHA256SUMS.txt 632 项）；重放：`scripts/rust-tauri/r01-t05-replay.sh`
- 候选链：`rust/crates/lingxi-browser-spike` 的 `spike_pdf` bin（CDP `--remote-debugging-pipe` 驱动受控 Chrome 153.0.8010.52，`Page.printToPDF`）；旧链：生产 `desktop/src/office-pdf-helper.cjs`（Electron 42.8.1 / Chromium 142 系）经隔离 harness 只读复用

## 1. 现役契约矩阵（office-pdf-helper.cjs / office-pdf-fonts.cjs / plugins/office 行级取证）

| 契约项 | 现役实现 | 来源 |
|---|---|---|
| 入口 | 独立进程 `--hana-office-html-to-pdf <job.json>`；server 经 `LINGXI_OFFICE_PDF_HELPER_EXEC` spawn | office-pdf-helper.cjs:7-22；plugins/office/lib/html-to-pdf.ts:34-53 |
| job 字段 | htmlPath(必须存在)/outputPath(必须)/viewport(1280×900, clamp 320..4096)/printBackground(true)/preferCSSPageSize(true)/pageSize("A4")/landscape(false)/margins(对象透传)/allowJavaScript(false)/embedLingxiFonts(true)/settleMs(250, 0..30000)/timeoutMs(60000, 1000..300000) | office-pdf-helper.cjs:45-68 |
| 字体注入 | 白名单三族 EB Garamond / Noto Serif SC / JetBrains Mono 的 @font-face，url 重写为绝对 file://；任一族缺失或字体文件缺失即**显式失败**（不静默回退）；`LINGXI_RENDERER_DIST` 注入优先且错误指针显式失败 | office-pdf-fonts.cjs:19,61-79,137-175 |
| 资产等待 | settleMs + `document.fonts.ready` + 全部 img load/error；JS 关闭时 evaluate 失败被 catch，仅靠 settleMs | office-pdf-helper.cjs:70-89 |
| 渲染沙箱 | 隐藏 BrowserWindow，`sandbox:true, nodeIntegration:false, contextIsolation:true`，`javascript` 随 allowJavaScript | office-pdf-helper.cjs:95-105 |
| 打印参数 | `webContents.printToPDF`（Chromium 打印管线） | office-pdf-helper.cjs:117-127 |
| 页眉页脚 | **现役契约不含** headerTemplate/footerTemplate/displayHeaderFooter 参数；两链输出均无页眉页脚（实测一致，符合默认 `displayHeaderFooter:false`） | normalizeJob 无此字段；实测 §3 |
| 资源边界 | **无**：加载 file:// 页面后无任何请求拦截（A10 实测后果见 §5） | office-pdf-helper.cjs 全文无 webRequest 拦截 |
| 临时文件 | helper 本身只写 outputPath；job.json 由插件层写 `<dataDir>/jobs/<id>/`（生产语义）；本任务旧链对照全部重定向 /tmp | html-to-pdf.ts:138-160 |
| 输出校验 | 插件层 `fsp.stat` 存在性+大小；helper 无结构校验 | html-to-pdf.ts:163 |

## 2. 固定样本集（tests/migration/r01-t05/samples/，生成器可重放 `--check`）

| 样本 | 内容 | 大小 |
|---|---|---|
| S1 s1-long-zh.html | 中文长文档：生僻字三组（子集内 龘靐麤爨驫鬱 / 子集外 BMP 齉爩龗灪 / 扩展B 𠮷𪚥）、140 行跨页表格（thead 重复 + break-inside:avoid + nowrap 行标记 ROW-NNN-END）、程序生成 PNG（data: + 相对文件双引用）、JetBrains Mono 代码块、SVG+CSS 数学内容、650 段长正文 | 271,712 B |
| S2 s2-page-semantics.html | `@page { size: A5 landscape; margin: 15mm }` + `break-before:page` + 12 个 `break-inside:avoid` 块 | 2,780 B |
| S3 s3-margins-paper.html | 纸张/方向/页边距探针（CORNER-ORIGIN-MARKER 绝对定位原点） | 1,127 B |
| S4 s4-js-gated.html | JS 开关哨兵（JS-EXECUTED-MARKER-7f3a 仅 JS 开启时出现） | 1,152 B |
| S5 s5-dangerous.html | file:///etc/passwd iframe+img+XHR、file:///etc/master.passwd、https://canary.invalid 远端、http://127.0.0.1:18291 loopback canary | 1,952 B |
| S5d s5-dangerous-decoy.html | S5 的合成诱饵变体（`../decoy/passwd`，allowlist 之外），供旧链对照，避免把真实系统文件内容写入证据 | 1,697 B |
| S6 s6-infinite-script.html | 内联 `while(true){}` 阻断 load 事件 | 1,165 B |

字体：全部使用仓库随产品分发的同一批 woff2（Google Fonts 上游，SIL OFL 1.1；来源/许可/子集覆盖实测表见 `tests/migration/r01-t05/FONTS.md`）。图片为 Python 逐像素生成的确定性 PNG（无外部素材）。样本零远端引用（S5 的远端引用是故意放入的拒绝探针）。

## 3. 逐项验证（步骤3 全项）

| 项 | 方法 | 结果 | 证据 |
|---|---|---|---|
| 字体就绪 | settleMs + fonts.ready/images 等待（JS 开）；JS 关时与 helper 相同的 catch 语义；字体嵌入用 /BaseFont+/FontName 扫描验证（Skia PDF 部分字体字典在压缩对象流内，两者都扫） | VERIFIED：A09 两链均嵌入 EBGaramond/NotoSerifSC/JetBrainsMono 子集；负对照 embedLingxiFonts=false 时三族均不出现（只剩系统回退 STSongti/PingFang） | a09/compare.json font_* 六项 PASS；font-negative/；matrix-verdicts.json |
| 分页 | S2 `break-before:page` 哨兵页归属；S1 74 页无异常空白页 | VERIFIED：PAGE2-START 两链均在第 2 页且不在第 1 页；S1 逐页墨迹覆盖率全部 > 0.0005 | matrix-verdicts s2-break-before-*；compare no_blank_pages_* |
| 纸张/页边距 | S2 @page A5 landscape（preferCSSPageSize=true）；S3 Letter+landscape+custom margins | VERIFIED：S2 两链 MediaBox=[0,0,594.96,420]（A5 横）；S3 两链 MediaBox=[0,0,792,612] 且墨迹 bbox **逐值相同** [217,151,880,398] | matrix-verdicts s2-page-css-true-*、s3-* |
| **margins 单位考据（重要实测发现）** | 旧链 bisect：margins={10} 在 A4 被拒（"margins must be less than or equal to pageSize"），{4} 通过且墨迹左缘恰为 4in，{0} 通过 | Electron 42.8.1 printToPDF 的 margins 数值**实际按 inch 解释并直传 Chromium**（electron.d.ts 注释 "in pixels" 与实测行为不符）；候选链按实测生产语义直传 inch，不做 px→in 换算 | §见 s3 两链 bbox 一致；bisect 记录于 R01-T05_REPORT §3 |
| **preferCSSPageSize=false 角例（已知差异 KNOWN-DIFF）** | S2 × preferCSSPageSize=false × 双链 + 候选 Letter/A4 矩阵 + headful 复测 | 旧链（Chromium 142）：取 job pageSize(A4) + CSS landscape 方向 → [0,0,842.88,595.92]；候选链（Chrome 153，headless 与 headful 同）：CSS @page 全优先、参数无效 → [0,0,594.96,420]。默认路径（preferCSSPageSize=true）两链一致；仅当显式 false 且 CSS 声明 size 时发散。缓解路径已记录（渲染前注入 `@page{size:auto}` 覆盖样式即可对齐，R09 实现期决定） | matrix-verdicts s2-page-css-false-*；bisect 于 /tmp 留证 |
| 超时 | S6 无限脚本 + allowJavaScript=true + timeoutMs=5000 | VERIFIED：候选 exit=2，`loadURL timed out after 5000ms`，无输出文件，浏览器树杀净、profile 目录删除、ps 残留扫描为空；旧链参考 exit=1（5342ms）同样无产物 | a10-infinite/*/run-result.json、artifact-check.txt |
| JS 开关 | S4 × allowJavaScript=false/true × 双链 | VERIFIED：JS 关时两链均无 JS-EXECUTED-MARKER、静态哨兵在；JS 开时两链均出现；S6 在 JS 关时正常产出（死脚本惰性化，exit=0） | matrix-verdicts s4-*、a10-jsoff-* |
| 远端资源读取限制 | S5 候选链：Fetch 域 allowlist（html 目录 + 字体目录 + data:/blob:），其余全 DENY；WebSocket 不经 Fetch 域（R1 验收 F1），由注入 CSP `connect-src`（白名单排除 ws:/wss:）在页面策略层阻断（repair-r1 起）；叠加 loopback-only 代理 + `proxy-bypass-list=<-loopback>` | VERIFIED：7 条 DENY（/etc/passwd iframe Document、img Image、XHR、master.passwd、/etc/hosts fetch、canary.invalid、loopback canary），PDF 无泄漏内容；canary 服务器零候选命中（候选命中数 0；日志中的 canary GET 经时间戳 06:07:12Z 归属旧链窗口，旧链无拦截层会真实抓取 loopback 资源）；repair-r1 复跑 7 DENY 不变、ws://loopback canary 零命中（§8 第 7 条） | a10-dangerous/new/fetch-log.jsonl、canary.log、run-result.json；repair-r1/ws-negative/ |
| 临时文件清理 | 候选 profile 唯一目录（/tmp 下 job-\<pid\>），结束后删除 + ps 残留扫描 | VERIFIED：成功与超时路径均 profile_removed=true、leftover_processes=[]（含无限脚本 SIGTERM→SIGKILL 路径） | 各 run-result.json cleanup 节 |
| 输出校验 | 候选写出前校验 %PDF- 头 + %%EOF 尾；先 tmp 再原子 rename；PDFKit 探针二次校验（可打开、页数、逐页文本/位图） | VERIFIED：失败路径零产物（超时/错误均不留 PDF 或 tmp）；成功产物 PDFKit 全部可解析 | spike-result.json、artifact-check.txt、probe.json |

## 4. R01-A09 中文长文档输出完整（VERIFIED，判据归独立验收）

同一 S1 样本分别走两链（均 A4、printBackground、preferCSSPageSize=true、JS 关、注入字体、settle 250ms）：

| 判据 | 旧链 | 候选链 | 结论 |
|---|---|---|---|
| 页数 | 74 | 74 | 一致 |
| 表格行 | 140/140 | 140/140 | 无丢行 |
| 异常空白页 | 无 | 无 | — |
| 归一化全文相似度 | — | — | 0.99910（87,161 vs 87,148 字符） |
| 语义哨兵 | 全部在 | 全部在 | 含三组生僻字、ROW-001/070/140-END、DOC-END-OMEGA、求根公式、欧拉恒等式、代码块标记 |
| 位图视觉差异 | — | — | 逐页 16×16 亮度网格平均绝对差 max=7.16 / mean=6.43（0–255 刻度，阈值 20） |
| 图片 | XObject=2 | XObject=2 | 无缺失 |
| 字体嵌入 | 三族齐 | 三族齐 | 另有 STSongti/PingFang 系统回退子集（两链一致） |

**实测发现（候选优于旧链）**：扩展B 字 𪚥(U+2A6A5) 在**旧链输出中整字缺失**（文本层与位图层均无，无豆腐块提示），候选链正常渲染并进入文本层。两链 page-01 位图对照（`a09/{old,new}/probe/page-01.png`）可直接目视确认。属旧链（Electron 42/Chromium 142）预存缺陷，登记不追修。

## 5. R01-A10 危险资源与失败可控（候选链 VERIFIED，判据归独立验收）

- **越权拒绝**：S5 全部 7 个非 allowlist 请求被 Fetch 层 DENY（AccessDenied）并逐条落日志；输出 PDF 文本不含 /etc/passwd 任何内容；exit=0（页面其余部分正常渲染）。
- **旧链对照（RECORD-ONLY）**：旧链对同构 decoy 样本**真实泄漏**——`../decoy/passwd` 内容（DECOY-MARKER-7f3a）完整嵌入旧链 PDF（`a10-dangerous/old/probe/text.txt`）。另有一次开发期对真实 `file:///etc/passwd` 的旧链试运行同样泄漏（真实系统文件内容进入 PDF，该产物已删除，未入证据链）。结论：生产 helper 无资源拦截层，候选链补齐此边界。
- **http 外发限制**：canary.invalid 远端引用被候选 DENY；全任务期间所有浏览器进程均套 loopback-only 代理，代理日志 286 条 REFUSED（Chrome 后台 GCM/时间校检/组件更新等全部被拒，含旧链 Electron 对 canary.invalid 的 1 次 CONNECT 尝试——被代理层拒绝）；loopback canary 仅旧链命中 1 次（时间戳归属其运行窗口），候选 0 命中。
- **无限脚本**：候选 exit=2 超时、无伪成功产物、进程树与临时目录清理均核验为空；同一文件 JS 关闭时正常产出（对照证明拒绝来自 JS 执行而非文档本身）。
- **WebSocket 边界（R1 验收 F1 补记）**：修复前 WS 不经 Fetch 域拦截（ws://127.0.0.1 握手两次实测到达 loopback canary，fetch-log 零记录）；repair-r1 起由注入 CSP `connect-src`（排除 ws:/wss:）阻断，负向用例实测 canary 零命中、页面 onerror、exit=0 不误伤渲染（§8 第 7 条，证据 `repair-r1/ws-negative/`）。

## 6. 性能对照（R00-T06 冻结基线）

PERFORMANCE_THRESHOLDS.json 中**无 office/PDF 专项冻结指标**（逐 metrics 核对该文件确认）；以下为对 R00-T06 office 基线的口径对照，不是阈值判定：

| 口径 | 值（ms） | 样本 |
|---|---|---|
| R00-T06 基线：打包 helper 直测 spawn→exit | median 812.1 / p95 835.6 | 239,073 B（32 样本） |
| R00-T06 基线：完整产品链 prompt→tool_end | median 851.8 / p95 879.4 | 同上 |
| 本任务旧链（开发 Electron harness）spawn→exit | median 953.5 / p95 1021（n=8） | 271,712 B |
| 本任务候选链 spike_pdf spawn→exit | median 1644.3 / p95 1663.5（n=16） | 271,712 B |
| 候选链其中 printToPDF 本身 | median 312.5 / p95 324 | 同上 |

口径差异声明：① 样本大 13.6%；② 基线为打包发布构建 helper，本任务两链均为开发态二进制；③ 候选为一次性冷启动 Chrome（无驻留），每次含浏览器启动（launch+attach ≈ 800ms 级）；④ 候选默认挂 loopback 代理与 Fetch 拦截。候选慢约 1.7×（同口径旧链）主要来自 Chrome 冷启动，正式实现可用驻留浏览器/页签复用摊销（R09 设计项）；打印核心 312ms 与旧链同量级。无冻结阈值被触及。

## 7. 渲染器分发依赖与许可

- **候选渲染宿主**：Chromium 系浏览器二进制（实测 Google Chrome 153.0.8010.52，本机 /Applications）。CDP pipe 驱动层（Rust）2.3MB 级，无新增第三方依赖（D-10）。
- **正式分发选项**（R09 决策）：随包 Chromium（BSD-3-Clause 主许可，含大量第三方组件许可树，打包体积 ~400MB 级压缩前/与 Electron 同量级，因为 Electron 本体即 Chromium）或复用 T04 结论中的同一受控 Chromium 宿主（浏览器能力与 PDF 渲染共用一个宿主，避免双份 Chromium）。许可文本随包义务与 Electron 现状同构（Electron 分发同样包含 Chromium 许可树），无新增许可类别。
- **字体**：EB Garamond / Noto Serif SC / JetBrains Mono，SIL OFL 1.1，已随产品分发，迁移不新增字体许可义务。
- **退出 Electron 判定（办公链局部）**：HTML→PDF 子链可以退出 Electron——候选链在全部实测项达到或超过旧链（另修复 file:// 越权泄漏与 Ext-B 字形缺失两个旧链缺陷）。前置条件：① R09 决定 Chromium 宿主分发方案（与浏览器宿主共用）；② Windows/Linux 按同矩阵重验（本次仅 macOS arm64）；③ preferCSSPageSize=false 角例差异的处置决策（接受 KNOWN-DIFF 或注入覆盖样式对齐）。

## 8. 已知差异与限制登记

1. preferCSSPageSize=false + CSS @page size 角例：两链语义不同（§3 KNOWN-DIFF）。
2. 旧链 Ext-B 字形缺失（𪚥）：预存缺陷，登记。
3. 旧链无资源拦截（file:// 越权内容进入 PDF、loopback 资源真实抓取）：预存安全缺口，候选链已补齐，迁移实现不得低于候选链。
4. margins 数值单位实为 inch（与 Electron 文档注释不符）：迁移契约按实测行为冻结。
5. 仅 macOS arm64 实测；Windows/Linux UNVERIFIED → R09/R10 强制门禁。
6. 候选链为一次性冷启动原型；性能优化（驻留、并发 job）属实现期事项。
7. **WebSocket 不经 Fetch 域拦截（R1 验收 F1，repair-r1 已修）**：Chrome 153 的 Fetch 域
   不对 WS 握手触发 `requestPaused`，修复前 `new WebSocket('ws://127.0.0.1:…')` 端到端连通
   （验收两次复现 canary 命中），wss 远端当时仅靠测试吊具的 loopback-only 代理兜底；
   且 `Network.setBlockedURLs(["ws://*","wss://*"])` 对 WS **实测无效**（repair-r1 复现留证
   `repair-r1/setblockedurls-ineffective/`）。影响面：不能读 file://（Fetch 层仍拒），但构成
   对 loopback 服务的探测/交互通道与 allowlist 内容经 WS 外带通道。修复（repair-r1）：
   `Page.addScriptToEvaluateOnNewDocument` 注入 CSP `connect-src file: data: blob: http: https:`
   （白名单刻意只排除 ws:/wss:，http(s)/file 连接照常穿透 Fetch 审计层，S5 的 7 条 DENY
   证据语义不变），fail-closed（注入失败即 job 失败）。负向用例
   `tests/migration/r01-t05/run_repair_r1_negative.sh`：ws://loopback canary 零命中 +
   页面 onerror 标记 + exit=0 不误伤渲染（7/7 PASS，证据 `repair-r1/negative/`）。
   **R09 门禁**：正式 renderer 实现必须保留等价或更强的 WS 边界（注入 CSP / launcher 层
   禁 WS / 其他机制），并在 A10 家族回归 WS 用例；负责人 = R09 任务执行者，
   截止 = R09 验收前。交叉影响：T04 浏览器 spike（spike_browser）无 Fetch 拦截层、
   仅靠代理层约束，loopback WS 在其原型路径同样不受候选代码约束——属同一机制族缺口，
   登记留 T08 风险登记（本修复不改 T04 交付）。
8. **printToPDF 阶段超时 exit 码（R1 验收 F2，repair-r1 已修）**：CDP 超时文案为
   "timeout: …"（如 "printToPDF: timeout: Page.printToPDF id=N"），修复前 spike_pdf 只匹配
   "timed out"，print 阶段超时实际 exit=1 而非文档承诺的 2（功能无损：非零退出、零产物、
   清理核验为空）。repair-r1 统一超时判定（`error_is_timeout` 覆盖两类文案 + bin 级单元
   回归），实测 printToPDF 阶段超时 exit=2（证据 `repair-r1/negative/print-timeout/`）。
