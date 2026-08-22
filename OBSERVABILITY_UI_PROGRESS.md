# Model Observatory UI — Phase 9 进度（2026-08-22 第八轮）

基线：`feature/model-call-observability` @ 156892e5（round-7 seal 树）。
任务书：MODEL_OBSERVABILITY_UI_AUDIT.md（15 问预审计）。状态：**全部完成**。

## 交付总览

把 Phase 1–8 的事实层（trace/payload/provenance/query/export/settings/storage）
变成生产级用户工作台，替换旧 Usage 页。内部 tab id 保持 `usage`（§五），
可见名称升级为 模型观测/Model Observatory（五语言同步，§六）。

### 1. Browser-safe wire 单一事实源

- `shared/model-observability-api-contract.ts`：A 查询 wire / B 响应 DTO /
  C payload+provenance / D settings+health+export+blob 四节；闭集数组
  （terminal status、payload kind/visibility/fidelity/sanitization、
  transformation、mapping precision、semantic category/role/source/root/shape、
  groupBy 14 维、error kind 12 种、blob id 正则与安全 media major）。
- renderer 只 import 该文件（§九：绝不 import lib/llm/node:crypto）；
  `lib/llm` 五个既有模块改为 re-export + 本地 `import type`（教训：
  `export {} from` 不绑定本地名）。
- `export-manifest.json` 收录（boundary 门禁）。

### 2. API client + error contract

`model-observability-actions.ts`：`observabilityRequest` 内核（30s 超时只覆盖
响应头到达；export 流式 `timeout:null`）；`ModelObservabilityRequestError`
保留 status/kind/code/field/matchedCalls/maxCalls/reason（§十一）；abort 不吞
（§十二）；403 无 code → kind 兼任 code。loader 覆盖 health/settings(PUT)/
query calls/traces/aggregate(POST)/call/trace/payload detail/export stream/
blob probe(HEAD)+fetch(GET)。

### 3. Filter / Metrics / Groups / Ledger

- `model-observability-filter.ts` 纯函数：默认 7d；since inclusive/until
  exclusive；`localUtcOffsetMinutes()` 是 API 东半球为正约定的唯一换算点
  （§十六）；multi 字段→wire 单数键（categories→subsystem alias §十九）；
  空=不发字段；exact ID 单值数组；tri-state 显式才发（§二十四）。
- FilterBar：5 preset + custom datetime-local、14 维 groupBy（≤3，§三十八）、
  provider/model/category facet（aggregate 懒加载、cache≤24）、status 闭集、
  More Filters（operation/callPurpose/attributionKind facet + 4 exact ID +
  inputShape/precision/payloadAvailability + 2 tri-state）、chips 单删+清空
  （§二十五）；filter 变更主动重置 cursor。
- `ObservabilityMetrics`：8 卡只用 overall（calls/traces hint、total/in/out
  tokens、cache read+命中率（仅 cacheObservedCount>0）、errors+incomplete、
  avg duration、cost null→"—"）；usage coverage 行。
- `ObservabilityGroups`：date 纯 SVG 柱图（无内联 style 字面量，动态宽度经
  ref 设 CSS var）、单维 ranked rows + BarTrack、多维 ranked list；provider/
  model/category/status 单维点击即筛选。
- `ObservabilityCallLedger`：12 列表格、cursor Load More（callId 去重）、
  generation+AbortController stale 防护、invalid_cursor 显式提示重载、
  context 列 sessionId/taskId 按值筛选按钮、payload 5 态 tooltip、
  usage_missing 正交警示。

### 4. Inspector / Provenance / Trace

- `ObservabilityCallInspector`（Overlay inline 抽屉）：header summary +
  CopyButtons、Overview dl（attribution/inputShape/persistence/usage 等）、
  AttemptsTable（attempt ≠ provider request 显式 note）、payload pipeline
  （semantic_request → per-attempt request/response → semantic_response）、
  trace 节 + openTrace 导航。
- `observability-provenance-resolve.ts`：locator-only 解析（§七十六红线），
  UTF-16 [start,end) slice；三态 resolved/structural/unavailable
  （root_missing/path_missing/span_out_of_range/not_text）；provider locator
  走 transport.body 同纪律。
- `ObservabilityProvenance`：分区列表 + 详情 dl + ResolutionPane + 跨链
  highlightOrdinal；`ObservabilityProviderProvenance`：映射行 + ordinal 跳转 +
  transformation/mappingPrecision 徽章 + Pi null 映射诚实 absent。
- `ObservabilityTraceExplorer`：`buildTraceForest` 纯函数（roots/edges/
  orphanEdges 构建、visited-set 环截断、orphan「Missing parent」合成节点、
  未覆盖 call 防御追加根）；degraded 警告不 crash。

### 5. Payload 呈现（§一百五十四 renderer 安全）

- `JsonValueViewer`：纯文本 pre + data-wrap 切换 + 复制 + 长文首段展开；
  无 innerHTML/eval/HTML 预览。
- `ObservabilitySemanticResponse`：text 块（reasoning 默认折叠）、toolCalls
  （展示不执行）、structuredOutput/media 块、raw JSON 切换、completeness
  常显。
- `ObservabilityPayloadCard`：kind/visibility/fidelity/sanitization 徽章 +
  诚实 hint tooltip（visibilityHint/fidelityHint/sanitizationHint）；
  contentState 4 态；懒加载 idle→loading→loaded；semantic_request →
  provenance inspector；复制按钮文案 Copy captured payload（§一百五十六）。
- `ObservabilityBlobPreview`：HEAD probe → >8MB 需确认 → preview（仅
  image/audio/video 安全 major）/opaque octet-stream → Download；missing/
  invalid/local_only 诚实态；objectURL revoke。

### 6. Settings / Onboarding / Export

- `ObservabilitySettingsDialog`：desired/effective 拆分展示（mismatch →
  configuredButInactive + reason code，绝不伪装 Active §一百）；at-rest
  加密诚实「无（依赖本机文件权限保护）」（§一百零三）；persistBlobs ⊆
  persistPayloads（payload 关 → blob 连带关，§一百零五）；payload/blob
  opt-in ConfirmDialog 列 5 类内容 + 无加密事实；retention 1..3650 校验
  （role=alert + save 禁用）；PUT 失败明说（§一百零七）。
- Onboarding（disabled + store absent）：单按钮只设 enabled=true/
  persistTraceMetadata=true/persistPayloads=false/persistBlobs=false（§九十九）；
  无 delete-all/clear-all 功能（§一百零八）。
- `observability-export-save.ts`：双通道（Electron IPC 桥 / File System
  Access）；流式 reader.read() 分块（§一百一十五禁全量缓冲）；文件名
  `lingxi-model-observability-YYYYMMDD-HHmm.jsonl`（§一百一十七）；abort →
  IPC 桥删部分文件 / FSA 如实 partialLeft；无能力 → 禁用。
- ExportDialog：当前筛选 echo、includePayloads（默认 false + sanitized
  副本 hint）、maxCalls 校验（413 → 缩小范围文案，绝不自动抬上限 §一百一十八）、
  字节进度、取消。
- IPC 桥（main.cjs + preload + PlatformApi）：`observability-export:
  begin/write/end/abort`；文件名白名单正则；≤4MB 分块；exportId 绑定
  sender webContents；sender destroyed 清扫；abort unlinkSync 部分文件。

### 7. Backend 最小增量（§一百九十三/一百九十四 白名单内）

- `getStoredBlob(blobId)`：blob id 正则校验 → state=missing → BlobMissing；
  路径从 blobId 重算（shard=前 2 字符 sanitize + containment check），
  **绝不信任 DB relative_path**；readFileSync 返回 server-only 类型
  （不进 wire contract）。
- `GET/HEAD /api/model-observability/blobs/:blobId`：LOCAL_ONLY；no-store/
  nosniff/content-length；安全 content-type（image/audio/video major 校验，
  否则 octet-stream）；invalid_blob_id(400)/not_found/blob_missing(404)。
  不自动访问外部引用、不读本地任意路径、不 fetch URL（§一百二十四）。
- API response 无 relative_path/absolute_path/LINGXI_HOME（§一百二十六）。
- 旧 usage 后端（usage-ledger.ts + GET /api/usage/llm）零改动（§一百四十二）。

### 8. Legacy retirement + i18n + CSS

- `UsageTab.tsx` 重写为渲染 ModelObservabilitySection（`data-tab="usage"`
  不变）；SettingsContent 标题键 → `settings.tabs.usage`。
- 删除 8 个 legacy 文件（UsageLedgerSection/Charts/CursorTip/
  usage-ledger-actions/model + 3 测试）；`settings.usage.*` 子树五语言删除
  （零引用）；nav/search 走 tabs.usage。
- i18n：`settings.observability.*` 完整子树五语言（含 values 23 组闭集矩阵、
  错误 13 键、visibility/fidelity/sanitizationHint、payload/provenance/
  providerMapping/inspector/trace/recording/export/blob/onboarding/jsonViewer/
  semanticResponse；retentionTrace/Payload/Blob 三个变量传入键补齐）；
  parity 测试绿。
- CSS：Settings.module.css 删 legacy usage 块 650 行；新增 observability
  块 157 类（纯 var() token：间距/色彩/时长；宽高/边框 px 合法域）；style-
  discipline 基线下调（bare-spacing 174→170、hardcoded-color 150→147）；
  `style={{` 棘轮零新增（含注释里的字面量也清了）。
- 修正 chip 字段键映射 bug（`replace(/s$/)` 会砍坏 terminalStatuses/
  payloadAvailabilities → 显式 Record 映射）。

### 9. 测试矩阵（10 文件 83 用例）

`desktop/src/react/__tests__/settings/observability/`：contract（闭集按值
锁定 + blob 正则反注入）/ filter（preset 数学、wire 映射、chips）/ format
（null≠0、$0.0007、时长分层）/ actions（error contract 全字段、413/400/403/
abort、HEAD probe）/ labels（chip 显式映射）/ provenance-resolve（三态 ×
全部 unavailable 原因 + emoji 代理对）/ TraceForest（嵌套/多根/orphan 合成/
环截断/未覆盖防御）/ export-save（文件名纪律、流式分块、abort 删文件 vs
partialLeft、能力探测）/ Metrics（8 卡、null—、命中率条件出现）/ Settings
Dialog（mismatch、加密事实、blob⊆payload、opt-in 确认、retention 校验、PUT
载荷）。新测试单独过 tsc（root tsconfig 覆盖 desktop/src/**）。

## 验证

- typecheck ×3 绿；eslint 本轮目录 0 error/0 新增（仓库既有 2 个
  require() error 在 mcp ConnectorToolList，rename 基线遗留，非本轮引入）；
  lint:boundary 绿；persistence：新站点登记（desktop-observability-export-
  output 豁免：用户选路径 + abort 删部分文件）→ scanner receipt 重生成 →
  fingerprint compatible repin（sha256:15591e09…）；i18n parity + locale
  coverage 绿；full npm test（数字见 PROGRESS.md 第八轮条目）。

## Seal

功能提交后 VERIFIED_SOURCE_SHA 推进到本轮功能树（6 处坐标：
verified-source-sha.txt / build-sync-matrix.mjs / upstream-sync-matrix.json /
PROGRESS.md / UPSTREAM_SYNC_AUDIT.md / UPSTREAM_SYNC_MATRIX.md），
单独 chore(audit) commit。
