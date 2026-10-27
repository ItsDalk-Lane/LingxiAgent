# ADR-003 文档渲染器选型（R01-T05）

- 状态：提议（prototype 证据完备，待评审）
- 日期：2026-09-25
- 基线：`5a8a8e24ab9e8e105da5132d16b175221a91d646`
- 证据：`docs/rust-tauri/R01/PDF_SPIKE_REPORT.md`、`artifacts/rust-tauri/R01/T05/`

## 背景

Lingxi 现役 HTML→PDF 由 Electron 隐藏 BrowserWindow + `webContents.printToPDF` 承载
（desktop/src/office-pdf-helper.cjs，契约矩阵见 spike 报告 §1：job 字段/字体白名单注入/
资产等待/JS 开关/超时/沙箱）。迁移必须保留实际输出质量与安全处理边界；任务书明确禁止
以「支持 PDF 的 Rust 库」充当等价证据。

## 候选与实际验证

### 候选 A：纯 Rust PDF 库（printpdf / typst 系 / wkhtmltopdf 封装等）
- 未经实测即排除，理由按任务书口径：它们不是 Chromium 打印管线，无法逐项对齐现役 HTML
  打印语义（@page/preferCSSPageSize/分页控制/Chromium 文本整形与字体子集嵌入）；任务书
  要求"必须处理现有 HTML 打印语义"且不接受库能力声明作为等价证据。
- 判定：**拒绝**（不满足等价性前提，不进入实测）。

### 候选 B：系统 WebView 打印（WKWebView `createPDF` 等）
- 未实测：WKWebView 在 T04 已证明浏览器能力契约存在缺口（上传/代理/接管），其 PDF 打印
  走 AppKit 打印管线而非 Chromium 语义，与现役输出不构成同引擎对照；Chromium 候选已全绿，
  无动机再开一条异引擎对照线。如实登记为**未实测**而非否决。
- 判定：**不采用**（语义族不同；保留为后备研究项）。

### 候选 C：受控 Chromium（CDP `--remote-debugging-pipe` + `Page.printToPDF`）
- Rust 原型 `spike_pdf`（lingxi-browser-spike crate 内 bin target，零新增依赖，D-10）实测：
  - 现役 job 契约逐项复刻并通过同一样本矩阵（S1–S6，详见 spike 报告 §3 表）；
  - A09：74 页中文长文档与旧链逐页对照全过（文本相似度 0.99910、140/140 表格行、
    字体三族嵌入、无空白页、位图网格差 max 7.16/255）；
  - A10：资源 allowlist（Fetch 域）拒绝全部 7 个越权请求、无限脚本 5s 超时杀树无伪产物、
    profile 清理核验为空；repair-r1 增补 WebSocket 边界（Fetch 域不覆盖 WS，改由注入
    CSP `connect-src` 排除 ws:/wss: 封堵，负向用例 7/7 PASS，见 spike 报告 §8 第 7 条）；
  - 超出旧链两项实测改进：修复旧链 file:// 越权内容进入 PDF 的安全缺口；修复旧链
    扩展B 字形（𪚥）静默缺失。
- 判定：**采纳**。

## 决策

文档渲染器方向 = **受控 Chromium + CDP `Page.printToPDF`**，与 ADR-002 浏览器宿主共用同一
受控 Chromium 运行时（不引入第二份浏览器）。纯 Rust 库与异引擎 WebView 打印明确不作为主路径。

## 理由

1. 唯一与现役同引擎族（Chromium 打印管线）的候选，HTML 打印语义逐项可对齐（且已对齐实测）。
2. CDP pipe 无 TCP 调试口；Fetch 域拦截提供了生产缺失的资源边界（A10 证据）。
3. 零新增 Rust 依赖；渲染宿主是进程外二进制，崩溃域与 Rust 服务隔离。
4. 实测发现已固化为迁移契约（margins 实为 inch、preferCSSPageSize=false 角例差异、
   字体三族白名单 fail-closed 注入）。

## 分发依赖与许可

- Chromium 宿主：BSD-3-Clause 为主的多许可树（与 Electron 内嵌 Chromium 同构，不新增许可类别）；
  体积与分发方案（随包 / 复用系统浏览器）留 R09 决策，成本计入 ADR-002 的同一宿主预算。
- 字体三族 SIL OFL 1.1（已随产品分发，见 tests/migration/r01-t05/FONTS.md）。
- 驱动层零新增第三方 Rust 依赖（D-10）。

## 能否退出 Electron（本子链结论）

**可以**（HTML→PDF/办公渲染子链）。条件：
1. R09 落实 Chromium 宿主分发方案（与浏览器宿主共用一份）；
2. Windows/Linux 按本任务同矩阵重验（本 ADR 仅 macOS arm64 VERIFIED）；
3. preferCSSPageSize=false 角例差异做出处置决定（接受 KNOWN-DIFF 或注入 `@page{size:auto}`
   覆盖样式对齐旧链）。

## 退出条件（出现以下任一情况须重开本 ADR）

- R09 决定不随包/不依赖 Chromium 宿主（则本子链失去渲染运行时）；
- Chromium 上游移除 `Page.printToPDF` 或 Fetch 域拦截能力；
- Windows/Linux 门禁实测打印语义不成立；
- 契约新增 Chromium 打印管线不支持的能力（如 Chromium 不实现的 @page margin boxes 页眉页脚——
  现役契约同样不含此能力，见 spike 报告 §1）。

## 后续强制门禁

- Windows / Linux 全矩阵重验 → R09/R10。
- 候选链资源 allowlist 边界（Fetch 拦截 + loopback 代理形态）进入正式 renderer 模块需求清单，
  迁移实现不得低于本原型（不得回退到旧链的无拦截形态）。
- **WebSocket 边界（R1 验收 F1 补记）**：Fetch 域不覆盖 WS 握手（Chrome 153 实测，
  且 `Network.setBlockedURLs` 对 WS 无效）；本原型 repair-r1 起以注入 CSP `connect-src`
  （排除 ws:/wss:）封堵并有负向用例（`run_repair_r1_negative.sh`）。R09 正式 renderer
  必须保留等价或更强的 WS 边界，并将 WS 用例纳入 A10 家族回归。
  负责人：R09 任务执行者；截止：R09 验收前（缺失即阻塞 R09 通过）。
- **超时退出码契约（R1 验收 F2 补记）**：各阶段超时（load/printToPDF 等）一律 exit=2，
  其他错误 exit=1；repair-r1 已统一判定并加回归，R09 实现须沿用该契约。
