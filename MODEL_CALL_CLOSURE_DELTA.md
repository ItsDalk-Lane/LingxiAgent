# Model Call Closure Delta — Phase 3.5 重新闭合（2026-08-21）

> 基线：`feature/model-call-observability` @ e795b9ef（round-2 树 b9238533 + audit seal）。
> 本文只记录本轮重新验证发现的路径变化，不重写原始审计报告
> （`MODEL_CALL_OBSERVABILITY_AUDIT.md` 保持 bf3c80b5 历史快照，§九十六）。

## 1. 重扫口径

对当前 HEAD 重新执行 §九十指定的全量出口反扫（`generateSummary` / `completeSimple`
/ `streamSimple` / `streamFn` / `callText` / `fetch` / `fetchImpl` / `adapter.submit`
/ `adapter.transcribe` / `execFile` / `runAgentLoop`），覆盖 lib / core / hub /
plugins / server / packages 的生产 caller。

## 2. 结论：Host-managed 生产可达路径 = 10（修正旧报告的 9）

### 2.1 新发现：MC-10 diary temporary summary（Pi direct generateSummary）

上一轮实施报告（Remaining Gaps）已把它记为"范围外已知旁路"；本轮按任务书
Phase 3.5 正式复核并确认其**生产可达**，且是**独立架构路径**：

```text
POST /api/diary/write（server/routes/diary.ts:28）
  → engine.writeDiary()（core/engine.ts:3419）
  → lib/diary/diary-writer.ts collectDiaryMaterialResult()
  → generateTemporarySummary() × N 个 session（diary-writer.ts:294）
  → defaultGenerateTemporarySummary()（diary-writer.ts:327）
  → generateDiaryCompactionSummary()（diary-writer.ts:350）
  → Pi generateSummary()（pi-coding-agent compaction.js:456，未传 streamFn）
  → completeSummarization() → completeSimple() → streamSimple() → Provider
```

逐项事实（任务书 §三）：

1. **生产可达**：是。desktop 设置页"生成日记"按钮 → REST 路由 → 全链同步可达。
2. **真的触发 Provider**：是。`generateSummary` 第 10 参 `streamFn` 为 undefined
   时回落 `completeSimple(model, context, requestOptions)` 直发（compaction.js:440-449）。
3. **经过当前 ModelCallObserver**：否。不经 Pi AgentSession streamFunction
   （无 session 参与）、不经 callText、不经 MC-05～09 任一接入点。
4. **进入 Usage Ledger**：否。该链路无 withModelRequestAccounting / ledger.start。
5. **Provider retry 可见性**：diary 未传 retry policy（`policy?.enabled` → maxRetries=0），
   无语义重试；pi-ai transport retry（retryProviderRequest）在 SDK 内部，不可见。
   → `attemptVisibility = logical_boundary`（不伪造 exact）。
6. **可不 fork Pi 接入**：是。`lib/pi-sdk/index.ts` facade 包装 re-export 即可
   （Observed direct summary boundary，复用 ModelCallRecorder/Observer/Identity）。

边界划分：**diary 最终生成**（diary-writer.ts:722 callText，operation
`diary_write`）仍是 MC-04，已观测；**临时摘要**（generateDiaryCompactionSummary）
是 MC-10。一次 `/diary` = N 次 MC-10 临时摘要 + 1 次 MC-04 终稿。

### 2.2 completeSimple 回落（session-snapshot-side-task-runner）— LATENT

`lib/llm/session-snapshot-side-task-runner.ts:94` 仍是唯一 caller of facade
`completeSimple`；其唯一上层 `lib/memory/memory-reflection-runner.ts` 的
`runMemoryReflection` 全仓无生产 caller（仅 rolling-summary-format.ts 注释提及）。

→ **LATENT / NOT_CURRENTLY_REACHABLE**。不为其制造生产行为（§六）；
facade `completeSimple` 保持裸 re-export，若未来激活须先补 Observer。

### 2.3 其余出口复核（无变化）

- `runAgentLoop` 生产 caller 仍仅 `lib/llm/cache-preserving-compaction-agent-run.ts:463`（MC-02）。
- `execFile` 生产 caller 仍两处：`plugins/jimeng-cli/adapters/dreamina.ts`（MC-07）、
  `server/routes/media.ts:364 openWithSystem`（本地文件打开，非 AI）。
- `callText` 17 个生产调用点全部经 `core/llm-client.ts` 统一 observer（MC-04）。
- `fetch/fetchImpl` 生产站点与原审计 §4 清单一一对应（MCP OAuth/web-search/
  wechat 桥/xai-oauth/目录探测/插件市场/search-tools GitHub 下载/桥接媒体/
  端口探测 等均为非 AI 控制面或普通外部服务；AI 出口仅 llm-client、
  provider-client、7+1 个媒体 adapter、4 个 speech adapter）。
- `generateSummary` 生产 caller 仅 diary-writer（经 facade）。
- `streamSimple` 无 Lingxi 直接生产 caller（仅 pi-ai 内部与注释）。

### 2.4 修正声明（§九十五）

```text
旧 Phase 3 结论：9 Host-managed production-reachable paths
Phase 3.5 re-audit：发现 production-reachable residual path（MC-10）
修正为：10 paths
```

这不是失败——是 Observability 审计体系开始发现自身漏项的正常结果。
原始审计报告不改写（§九十六），本文件即 Audit Addendum。

## 3. MC-10 矩阵行（接入后）

| Path | Logical Call | Attempt visibility | Provider wire | Ledger | 观测边界 |
| ---- | ------------ | ------------------ | ------------- | ------ | -------- |
| MC-10 Pi direct summary（diary 临时摘要） | FULL | logical_boundary | 无事件（诚实缺失，同 MC-03：summarizer options 不含 onPayload，无 session 扩展链） | 本轮补：ledger.start 包 generateDiaryCompactionSummary，metadata 带 modelCallId/traceId | `lib/pi-sdk/index.ts` observed generateSummary |
