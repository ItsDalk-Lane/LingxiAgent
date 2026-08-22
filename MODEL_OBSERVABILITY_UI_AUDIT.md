# MODEL_OBSERVABILITY_UI_AUDIT.md — Phase 9 UI 架构审计

> 编码前审计（Phase 9 任务书 §八，15 问）。基线：`feature/model-call-observability`
> @ `156892e5`（Phase 8 功能树 `cfab8556`）。当前代码是唯一事实源；本审计只回答
> 问题并固化 UI 架构决策，不改任何代码。

## 基线验证（Step 0）

- branch：`feature/model-call-observability`；worktree 干净。
- HEAD `156892e5`（seal commit）；功能 HEAD `cfab8556` = Phase 8 Unified Query &
  Control Plane。`git log main..HEAD` 含 Phase 1～8 全部提交；
  `git diff --stat main...HEAD` = 147 files / +28242 / −440。
- 后端事实源（已完整阅读）：`lib/llm/model-observability-query-types.ts`（filter/
  groupBy/cursor/DTO）、`model-observability-query.ts`（Query Service）、
  `model-observability-export.ts`、`model-observability-preferences.ts`、
  `model-observability-read-database.ts`、`model-observability-blob-store.ts`、
  `model-call-payload-types.ts`、`semantic-input-provenance.ts`、
  `provider-request-provenance.ts`、`server/routes/model-observability.ts`、
  `server/http/route-security.ts`、`core/engine.ts`（settings/health 方法）。

## Q1. Settings 主区域真实尺寸与滚动模型

- 入口：`desktop/src/settings-main.tsx` → `SettingsApp` → `SettingsContent
  variant="window"`；modal 变体由 `components/SettingsModalShell.tsx` 内嵌
  `variant="modal"`。
- 独立窗口：`desktop/main.cjs:2993-3000` 创建 720×700（min 720×500），随主窗
  resize 同步到 90%。
- 布局：左 nav 180px（`--settings-nav-width`）+ 主列；主列横向 padding
  `--settings-main-x-padding` = `--space-40`（40px）×2。默认窗口下内容列
  ≈ 720 − 180 − 80 = **460px 宽**；`max-width: none`，宽窗不加限。
- 滚动：`.settings-main`（`Settings.module.css:321-332`）是**唯一**内容滚动容器
  （`overflow-y: auto; overscroll-behavior: contain`）；nav 独立滚动；切 tab 时
  `SettingsContent.tsx:221-225` 直接赋 `scrollTop = 0`（jsdom 无 scrollTo，
  是故意的）。
- 页面外壳：`SettingsPage`（components/SettingsPrimitives.tsx）layout 只有
  providers 用 `fill`，其余 `flow`。
- **决策**：Observability 页内部不再自造整页滚动；Filter Bar / Metrics /
  Groups / Ledger 都是 `settings-main` 内的流式区块；Inspector 用右侧
  drawer（宽屏）/ 全宽 overlay（窄屏），drawer 内部自己滚动。

## Q2. window / modal 两 variant 对 UsageTab 的尺寸差异

- modal：`SettingsModalShell.module.css:47-50` — `width: min(90vw, 100vw - 48px)`、
  `height: min(90vh, …)`、`min-height: min(500px, …)`；`@media (max-width: 760px)`
  近全屏。modal 头为 grid（返回键 + tab 标题），`settings-main::before` 有
  sticky 顶部渐变遮罩（`Settings.module.css:347-360`）。
- window：header 是 drag region；tab 标题在滚动区内（`SettingsContent.tsx:280-287`）。
- **含义**：新页面最小可用宽度 ≈ 720×0.9 − 180 − 80 ≈ **388px**（modal）与
  460px（window）。设计按 380px+ 可用：Filter Bar wrap、Metrics grid 降列、
  Inspector 窄屏全宽 overlay。§一百四十九/一百五十验收两 variant。

## Q3. 可复用 CSS design tokens

- 结构 token（主题无关，`desktop/src/styles.css:7-118`）：spacing
  `--space-2..40`、radii `--radius-xs/sm/md/lg` + `--radius-input/card`、字号
  `--fs-title/body/ui/caption/hint/micro`、`--font-ui/serif/mono`、动效
  `--duration-instant/fast/slow` + 4 easing、scrim `--scrim-15..45`、tooltip
  token。
- 色彩 token 按主题（`desktop/src/themes/*.css`）：`--bg/--bg-card/--bg-hover/
  --text/--text-muted/--accent(+rgb)/--danger(+rgb)/--border/--overlay-*`。
- 样式立法（硬约束）：
  - `scripts/style-discipline.mjs`：bare px spacing / hardcoded color / literal
    duration 按文件基线只减不增；**新代码全部用 var() token**。
  - `settings-primitives-contract.test.ts`：`desktop/src/react/settings/tabs/**`
    的 `style={{` 计数封顶 48（只减不增）——**新组件禁止 JSX inline style**；
    动态值走 SVG 属性（chart）或 ref `el.style.setProperty`（UsageCursorTip
    先例）。删除 legacy Usage 文件会释放一部分既有额度，但新 UI 仍以 0 新增
    为目标。
  - 同测试要求 tabs/ 下不直接 import `widgets/(Toggle|SelectWidget)`——共享
    控件一律走 `desktop/src/react/ui` barrel。
- **决策**：新样式集中在 `Settings.module.css` 追加 `.observability-*` 区块
  （legacy `.usage-*` 区块在 retirement 后删除）；纯 token，零新增违例。

## Q4. 旧 Usage chart 哪些可改纯 presentational

- `UsageLedgerCharts.tsx`：`ModelOrbit / SplitRing / DailyBars / RequestLedger /
  UsageLegend` props 已是 `UsageAggregate`/`UsageLedgerEntry`，看似接近
  presentational，但内部 import `t`、`usage-ledger-model`（含旧聚合语义）与
  `useUsageCursorTip`，且视觉（orbit 环、双环、cache/uncached 双色柱）绑定
  UsageAggregate 的 token/cache 语义。
- **决策（§三十八）**：不把新 `ModelObservabilityGroupMetrics` 强转回旧类型。
  图表新写轻量 SVG 组件（date → 竖柱；model/category/provider/status → 横向
  ranked bars；session/task/agent → ranked list），统一消费
  `ModelObservabilityGroupBucket[]`。旧 chart 组件整体退休。

## Q5. 强依赖 UsageLedgerEntry、应直接退休的旧组件

- `UsageLedgerSection.tsx`（页面状态 + 4 视图互斥）——退休。
- `UsageLedgerCharts.tsx`（ModelOrbit/SplitRing/DailyBars/RequestLedger）——退休。
- `UsageCursorTip.tsx`（cursor-following tooltip，只服务旧 chart）——退休。
- `usage-ledger-actions.ts`（`/api/usage/llm` client + Entry 类型）——退休。
- `usage-ledger-model.ts`：`aggregateEntries/groupEntries/groupDailyEntries/
  groupDateWindowEntries` 退休（§三十七：新页面不再浏览器端 aggregate）；
  **纯 formatter 迁移**：`formatCompactNumber/formatNumber/formatPercent/
  formatCost/formatTime/num` 提取进
  `tabs/observability/model-observability-format.ts`（§一百四十四，formatCost
  修掉 `$0.0007→$0.00` 问题，见 §一百四十七）。
- 引用面实证：除上述文件外，仅 `SettingsContent.tsx`（TAB_TITLE_KEYS）与两组
  旧测试引用 `settings.usage.*` / 这些模块（grep 全仓确认）。

## Q6. renderer 能否安全 import Phase 8 DTO

- **不能**直接 import `lib/llm/model-observability-query-types.ts`：第 20 行
  `import { createHash } from "node:crypto"`（cursor fingerprint）。renderer
  现有惯例只 import `shared/*`（theme-registry / error-user-messages /
  browser-preferences / compaction-mode 等，relative path 逃逸 vite root，
  tsconfig include 覆盖）；**无 renderer → lib/ 先例**。
- `lint:boundary`（lint-open-boundary.mjs）是 open-export manifest 棘轮，不查
  renderer node builtin；renderer 侧只有 dev-only 的 `.cjs require()` 防护——
  即没有现成机器防线，必须靠结构拆分 + 新 contract 测试。
- **决策（§九）**：新建 `shared/model-observability-api-contract.ts`——纯
  `type`/`interface`/const enum arrays（group-by dimensions、payload
  availability、usage availability、terminal status、export options 形状、
  health/settings 响应 DTO），**禁止** node:*/fs/path/better-sqlite3。
  `lib/llm/model-observability-query-types.ts` 改为从 shared import 并
  re-export 全部 DTO（normalize/cursor/fingerprint 留 server 侧，签名与语义
  不变）。新增 contract 测试：(a) shared 模块源码扫描无 forbidden import；
  (b) shared const 数组与 normalizer 闭集逐项对齐（防 Server/UI 漂移）。

## Q7. 现有文件保存 abstraction

- **不存在 save dialog / streaming writer**。main.cjs 只有 showOpenDialog ×4
  （folder/files/skill/plugin）与整内容写 IPC（`write-file` /
  `write-file-if-unchanged`（5MB snapshot cap）/ `write-file-binary`（base64）/
  `copy-file`）——全部是 renderer 指定任意路径的 workbench 语义，不满足
  §一百一十六（用户先选路径 + streaming + renderer 无任意 FS 权限）。
- 既有 export 两范式：server 写文件 + `showInFinder`（skills bundle）；
  renderer `res.blob()` + `a.download`（会话导出，整段进内存——§一百一十五
  明确禁止用于本轮 export）。
- **决策**：新增最小专用 streaming save bridge（详见 §「Export Save Bridge
  决策」）。

## Q8. connection state 能否判断 local owner / remote server

- 能。`desktop/src/react/services/server-connection.ts`：
  `isLocalOwnerConnection(connection)`（:523，`kind==='local' &&
  credentialKind==='loopback_token'`）；settings store 持有
  `activeServerConnection`（SettingsContent.tsx:70-84 组装）。
- 先例：`AccessTab.tsx:79,398` 用同一判定 disable 本地限定控件。
- **决策（§一百三十三/一百三十四）**：UI 用 `isLocalOwnerConnection` 提前
  disable + tooltip（payload body / settings PUT / export / blob preview），
  但不复制权限判断、不信 hostname；**route security 仍是最终真相**——403
  `local_only_route` 时 UI 优雅降级显示 LOCAL_ONLY 说明（§六十/一百六十九）。

## Q9. 可复用 modal/drawer/popover primitive

- `ui/Overlay.tsx`：open/onClose/scope('inline'|'window')/backdrop/closeOnEsc/
  closeOnBackdrop/trapFocus（自带 focus trap + focus restore + portal）——
  Inspector drawer、Settings dialog、Export dialog、确认框全部用它。
- `ui/ConfirmDialog.tsx`：payload/blob opt-in 二次确认（§一百零二/一百零四）。
- `ui/Toggle.tsx`、`ui/SelectWidget.tsx`、`ui/Tooltip.tsx`、`ui/Button.tsx`。
- 无现成 drawer——用 Overlay `scope="inline"` + 自有 CSS 类实现右侧
  drawer（宽屏 45%～55%）/ 全宽 overlay（窄屏），不引入新依赖。

## Q10. JSON/code viewer

- 不存在通用 JSON viewer。先例只有 `<pre><code>`（SkillViewerOverlay:183 等）。
- **决策**：新写 `JsonValueViewer`（纯文本渲染、monospace、wrap toggle、copy、
  大对象折叠、超长文本首段 + 展开）。严禁 dangerouslySetInnerHTML / eval /
  Markdown HTML execution（§六十六/一百五十四）。

## Q11. resize split pane

- 不存在（SidebarLayout 只管理固定 CSS var 宽度 + resize 自动折叠）。
- **决策**：不造 draggable splitter。Inspector 宽度用 CSS（宽屏
  `min(52%, 720px)`，窄屏全宽 overlay，媒体查询对齐 SettingsModalShell 的
  760px 断点），满足 §五十二且零新 primitive。

## Q12. virtualization / list primitive

- 无库、无手写 windowing、无 IntersectionObserver。长列表惯例 = server 分页。
- **决策（§四十七/四十八）**：Call Ledger 用 keyset cursor「加载更多」
  （每页 50，保留已加载），不无限滚动、不虚拟化；Inspector 内长 payload
  文本用「首段 + 展开全文」+ `<pre>` 纯文本（§六十七），不建几十万 spans。

## Q13. i18n locale 完整性 guard

- 运行时：`desktop/src/lib/i18n.js`（classic script → `window.i18n`/`window.t`；
  miss 返回 key 本身）；React 侧 `settings/helpers.ts t()`。
- 机器防线：`tests/i18n-locale-parity.test.ts`——en.json 每个 leaf key 必须在
  zh/zh-TW/ja/ko 解析到（hard fail）。`scripts/sync-locale-parity.mjs` 是辅助
  backfill 工具。
- **决策（§六/七）**：新增 `settings.observability.*` 独立 namespace，五套
  locale 同步手写翻译（不用 backfill 占位）；`settings.tabs.usage` 值升级为
  模型观测/Model Observatory/…；`TAB_TITLE_KEYS.usage` 改指同一 key
  （`settings.tabs.usage`）消除「左栏用量统计 / 标题模型观测」漂移（§六）；
  legacy `settings.usage.*` 子树在确认零引用后整套删除（grep 锁定）。

## Q14. desktop test environment 如何 mock lingxiFetch

- 单 root `vitest.config.js`（alias `@`→desktop/src/react）；renderer 测试首行
  `// @vitest-environment jsdom`，位于 `desktop/src/react/__tests__/**`。
- 范式（SettingsContent.test.tsx / UsageTab.test.tsx / UsageLedgerSection.test.tsx）：
  `vi.hoisted` + `vi.mock('../../settings/api', …)` 注入假 `lingxiFetch`/
  `lingxiFetchJson`；`beforeEach` 铺 `window.t`/`window.i18n`/`window.platform`；
  zustand `useSettingsStore.setState` 注 connection；testing-library +
  jest-dom。
- `lingxiFetch(path, opts)` 支持 caller `signal`（转发进内部 AbortController，
  默认 30s 超时），非 2xx 抛 `errorWithCode`（code 经
  `shared/error-user-messages.ts` 归一）。
- jsdom 无 `Element.scrollTo`（用 scrollTop 赋值）；无
  `URL.createObjectURL`（blob preview 测试需 stub）。
- **决策**：API client 层接受 `fetchImpl` 注入（默认 lingxiFetch），组件测试
  mock actions 模块；client 测试直接注入假 fetch 验证 error code 归一、
  AbortController、export streaming saver 协议。

## Q15. Stored Blob preview：exact-id、LOCAL_ONLY、no path exposure 方案

- Phase 7 blob store 事实：`blobId = mb_<random>`（`mintModelObservabilityBlobId`）；
  磁盘 `blobs/<shard2>/<blobId>.bin`；`blob_objects` 行持有 media_type/
  byte_length/state(ready|missing)/relative_path；既有 `readBlob` 写侧语义
  （missing 时 UPDATE state）不能进 read-only query 连接。
- **方案（本轮唯一后端扩展，§一百一十九～一百二十七）**：
  - `GET /api/model-observability/blobs/:blobId`，route-security 显式登记
    **LOCAL_ONLY**（isModelObservatoryRoute 增加 blobs 正则 + policy 分支）。
  - id 校验：`^mb_[A-Za-z0-9]{8,64}$`（bounded length）；非法 → 400
    invalid_blob_id。
  - 读取路径：query service 新增 `getStoredBlob(blobId)`——readonly DB 查
    blob_objects（row 不存在 → 404 not_found；state='missing' → 404
    blob_missing）；文件路径**由 blobId 重算**（与 write 侧同一 shard 函数），
    resolve 后强制落在 blobs root 内（containment check），**绝不消费 DB 的
    relative_path**（防手工改库 traversal）；文件不存在 → 404 blob_missing
    （不 500）。DB absent → 404 not_initialized（绝不 mkdir/建库，§一百二十三）。
  - 响应头：`Cache-Control: no-store`、`X-Content-Type-Options: nosniff`、
    `Content-Length`；`Content-Type` 只取 DB media_type 且经安全 allowlist
    （`image/*`/`audio/*`/`video/*` 主类型 + 字符合法校验），其余 →
    `application/octet-stream`。响应体只有 bytes；**绝不返回**
    relative_path/absolute_path/LINGXI_HOME（§一百二十六）。
  - 外部引用（local_file_reference / signed URL / externalized 无 blobId）：
    根本不到达本 route；UI 只在 record.blobIds 有值时给 Preview 按钮
    （§一百二十四/一百二十八）。
  - UI：默认只显示 media type/byteLength/blobId + Preview 按钮（点击才取
    bytes）；`URL.createObjectURL` 展示 image/audio/video 原生控件，关闭
    revoke；>阈值（默认 8MB）先大小提示再确认；octet-stream 不解析只给
    metadata + Download（仍是 exact stored blob）。

## Export Save Bridge 决策（§一百一十五/一百一十六）

- 现状无 save dialog / streaming writer（Q7）。新增最小专用 IPC（desktop only，
  不污染 web）：
  - `main.cjs`（wrapIpcHandler）：
    `observability-export:begin`（{suggestedFileName}）→ showSaveDialog
    （默认 `lingxi-model-observability-YYYYMMDD-HHmm.jsonl`，§一百一十七——
    不含 session name/prompt/agent 私有名）→ canceled → `{canceled:true}`；
    否则 `fs.openSync(path,'w')`，生成随机 exportId 存 Map（绑定 sender
    webContents id）→ `{canceled:false, exportId, fileName}`。
  - `observability-export:write`（{exportId, chunkBase64}）→ 单 chunk ≤ 4MB
    校验后 `fs.writeSync`；未知/越权 exportId → reject。
  - `observability-export:end`（{exportId}）→ close + 出 Map →
    `{bytesWritten}`。
  - `observability-export:abort`（{exportId}）→ close + **删除 partial 文件**
    （显式策略：取消/出错不留半截文件）+ 出 Map。窗口 destroy 时清扫该
    webContents 的未完结 exportId。
  - renderer 不获得任意路径写权限（路径只来自 save dialog，exportId 是
    capability token）。
- preload/types 按三触点惯例补齐（`observabilityExportBegin/Write/End/Abort`
  optional 方法）。
- 非 Electron（dev:web 浏览器）：`window.showSaveFilePicker`（File System
  Access API）→ `createWritable()` 真流式；两者皆无 → Export 按钮 disable +
  tooltip 说明。**任何路径都不允许 `res.text()`/`res.blob()` 整缓存**
  （§一百一十五）。
- 消费侧：`res.body.getReader()` 分块解码直接写 saver；manifest 首行解析
  totalCalls 用于进度显示；server cancel 语义 = reader.cancel()（route 已
  实现 generator return）。

## 页面架构决策（§一百九十五/一百九十六裁剪）

```
desktop/src/react/settings/tabs/observability/
  model-observability-types.ts        # wire DTO re-export + UI 本地类型
  model-observability-actions.ts      # API client（health/settings/query/detail/
                                      # payload/export/blob + error 归一 + abort）
  model-observability-format.ts       # compact/exact number、duration、cost、
                                      # time、shortId（自 legacy 迁移并修正）
  model-observability-filter-model.ts # draft/applied filter ↔ wire filter、chips、
                                      # date preset（since inclusive/until exclusive）、
                                      # utcOffsetMinutes = -new Date().getTimezoneOffset()
  use-observability-queries.ts        # health/settings/aggregate/calls hooks
                                      # （AbortController + request generation 双保险）
  ModelObservabilitySection.tsx       # 页面编排：单一 unified query state
  ObservabilityFilterBar.tsx          # Date/Provider/Model/Category/Status/More/
                                      # GroupBy/Refresh/Export/Settings
  ObservabilityFilterChips.tsx        # 已应用 filter chips + clear all
  ObservabilityMetrics.tsx            # overall metric cards（只渲染 server aggregate）
  ObservabilityGroups.tsx             # group buckets → date bars / ranked bars / list
  ModelCallLedger.tsx                 # call rows + load more（cursor）
  ModelCallInspector.tsx              # drawer：Overview/Attempts/Pipeline/Trace 区
  ModelPayloadCard.tsx                # metadata 先行 + 展开才取正文 + badges
  JsonValueViewer.tsx                 # 通用 JSON 纯文本 viewer
  SemanticResponseView.tsx            # semantic response 友好拆分 + Raw 切换
  ProvenanceInspector.tsx             # section list + locator 精确取段（无内容搜索）
  ProviderMappingView.tsx             # semantic→provider mapping（null 诚实表达）
  ModelTraceExplorer.tsx              # roots/edges/orphanEdges 树 + cycle visited set
  ObservabilitySettingsDialog.tsx     # desired/effective + opt-in 确认 + retention
  ObservabilityExportDialog.tsx       # 当前 filter 摘要 + includePayloads + 流式保存
  BlobPreview.tsx                     # lazy preview + Object URL 生命周期
```

状态纪律（§十三/十四）：`ModelObservabilitySection` 持单一
`appliedFilter + datePreset + groupBy + callCursor + selectedCallId/TraceId`；
文本类 filter（session/task/conversation id）走 draft → Enter/debounce apply；
select 类即时 apply；applied filter 变化 → cursor 清空、ledger 重置、
inspector 保留上下文按规则处理。

## Legacy Retirement 预登记（最终矩阵在 OBSERVABILITY_IMPLEMENTATION_NOTES.md）

| Legacy | 处置 | 理由 |
| --- | --- | --- |
| UsageLedgerSection.tsx | Replaced→Deleted | 四互斥视图被统一工作台取代 |
| UsageLedgerCharts.tsx | Replaced→Deleted | 绑定 UsageAggregate；图表按新 metrics 新写 |
| UsageCursorTip.tsx | Deleted | 只服务旧 chart |
| usage-ledger-actions.ts | Deleted | 旧 /api/usage/llm client（后端 API 保留） |
| usage-ledger-model.ts | Deleted（formatter 迁出） | aggregateEntries/group* 不再是数据真相 |
| formatCompactNumber/… | Reused（迁移至 observability-format） | 纯函数，formatCost 精度修正 |
| `settings.usage.*` locale | Deleted（确认零引用后） | §七 |
| `settings.tabs.usage` locale | 保留 + 文案升级 | id 不变（§五/六） |
| UsageTab.test.tsx / UsageLedgerSection.test.tsx | 重写/删除 | 注册与新页面行为测试取代 |

## 风险与陷阱登记（编码时对照）

1. **utcOffsetMinutes 符号**：JS `Date.getTimezoneOffset()` 东半球为负，API
   「东半球为正」——前端传 `-getTimezoneOffset()`，专项测试锁定（§十六）。
2. **inline style 棘轮**：tabs/** 下 `style={{` 0 新增；chart 用 SVG 属性，
   动态 CSS var 用 ref。
3. **style-discipline**：新 CSS 全 token（spacing/color/duration）。
4. **i18n parity**：新 key 五文件同结构同步落地，否则 hard fail。
5. **cursor 绑定 filter**：applied filter 任何变化必须清 cursor（§四十九）。
6. **stale response**：aggregate/calls 双通道 AbortController + generation id
   （§十二/一百六十四）。
7. **costTotal null ≠ 0**；cacheObservedCount=0 → hit rate 显示 —；
   usageCovered/usageMissing 可见（§三十～三十三）。
8. **payload availability 五态**与 usage availability 三态各自独立 badge；
   `unknown ≠ 未保存`（§四十五）。
9. **OPAQUE/UNAVAILABLE/METADATA_ONLY/corrupt** 各自 explanatory state，
   绝不渲染 `{}`（§六十三/六十四/一百六十六～一百七十）。
10. **provenance 只消费 locator**（root→path→UTF-16 slice），禁止内容搜索反推；
    span=null/非 exact → structural 说明（§七十五～七十八）。
11. **Trace 渲染 visited set**（即使后端已检测 cycle，§九十三）。
12. **不要 flush writer**：Refresh = health + aggregate + 首页 calls（§五十）。
13. **export 默认 metadata-only**；includePayloads 显式勾选 + 安全提示；永远
    没有 includeRaw / blob bytes（§一百一十一～一百一十四）。
14. **recording disabled ≠ 无数据**：queryStatus=ready 时历史照常浏览
    （§九十七/一百八十）。
15. **blob preview**：默认 lazy、超阈值确认、Object URL revoke、octet-stream
    不解析（§一百二十八～一百三十一）。
16. **vitest 不查类型**：新测试文件必须单独过 tsc（tsconfig.test），前两轮
    翻车点（见 memory model-call-observability-round3）。
17. **usage_status 与 terminalStatus 正交**：usage_missing 只能是小 warning，
    不把 call 染成 error（§四十四）。
18. **section.test 的 `data-tab="usage"`**：UsageTab 保留 `data-tab` 属性 +
    nav 顺序（providers→models→usage）既有断言继续成立。
