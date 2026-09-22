# CREDENTIAL_ROUTING — 模型选择与新鲜凭证获取（P04-T02）

日期：2026-09-22｜基线 HEAD `3286c96e5`。本文记录当前真实接线与验证证据；除标注外均为 UNCHANGED_VERIFIED（本阶段零生产代码改动，仅补测试）。

## 1. 统一解析链（单一权威）

```text
模型身份（provider+id 联合键，三处一致）
  shared/model-ref.ts findModel            —— chat/auxiliary（model-manager._availableModels）
  core/provider-registry.ts getOperationModel —— embedding/rerank 操作目录
  core/media/media-execution-target-resolver —— 媒体/语音 lane（credentialProviderId 显式道）

凭证新鲜获取（请求边界，唯一入口）
  core/model-manager.ts:589 resolveProviderCredentialsFresh(provider, {forceRefresh, staleApiKey})
    ├─ credentialSource=auth-storage   → AuthStorage/ModelRuntime.getApiKey（OAuth 自动换新）
    │    └─ forceRefresh → core/oauth-force-refresh.ts（auth.json 文件锁内旋转）
    ├─ credentialSource=provider-catalog → ProviderRegistry 原始条目
    └─ credentialSource=none           → 本地/无鉴权端点（api_key=""）
  chat 流式链例外：Pi ModelRuntime.getAuth 自取（AuthStorage 同一 auth.json 锁），
    lib/pi-sdk/index.ts:454 withLingxiCredentialBoundary 拒绝宿主环境变量/ambient 凭证。
```

- `resolveSync`（缓存凭证）仅用于摄入管线可解析性探测（engine._canResolveKnowledgeEmbeddingRef）；真实请求一律 `resolveFresh`（resolver）或 `resolveAuxiliaryModelFresh`/`resolveModelWithCredentialsFresh`（callText 族）。
- 媒体 Codex 401 → `forceRefreshOAuthApiKey(staleApiKey)` 单次旋转后重试（openai-codex.ts:283-295）；刷新本身是控制面，不构成新 logical call。

## 2. 验收语义与证据

| 规则 | 实现位置 | 测试（文件 · 用例） | 状态 |
|---|---|---|---|
| provider+model 联合身份，同名不互串（A01） | provider-registry.ts:1119 getOperationModel；model-ref.ts findModel | tests/model-operation-resolver.test.ts · “P04-A01：同名 modelId 的两个 provider 不互串”（真实 ProviderRegistry 双 provider 同 id） | PASS（新增） |
| 缺凭证 fail-closed，不偷回退到可用 provider（A02） | model-operation-resolver.ts compose（provider_missing_creds）；auxiliary-model-resolver.ts（provider_missing_creds，配置错误不 fallback）；model-no-fallback.test.ts 全族 | tests/model-operation-resolver.test.ts · “P04-A02”；tests/auxiliary-slot-resolver.test.ts 既有配置错误族 | PASS |
| 并发刷新单飞、异账户隔离（A03） | oauth-force-refresh.ts backend.withLockAsync + staleApiKey 复用 | tests/oauth-force-refresh.test.ts · “refreshes exactly once when two callers race on the same stale credential” | PASS（既有） |
| 配置轮换立即生效（A04） | resolveFresh 每次请求边界重取；getApiKey 后 `_authStorage.reload()+clearAuthCache()`（model-manager.ts:614-615） | tests/model-operation-resolver.test.ts · “P04-A04”；tests/model-manager-auth-storage.test.ts 既有轮换族 | PASS（新增+既有） |
| 合法无 apiKey：本地端点 / 裸 header 凭证 / OAuth（A05） | allowsMissingApiKey ?? isLocalBaseUrl（本地判定未扩大：仅 localhost/127.0.0.1/0.0.0.0）；hasCredentialHeaders | tests/model-operation-resolver.test.ts · “P04-A05”×2；tests/model-operation-resolver.test.ts · “ollama 自添加嵌入条目…”（authType none）；tests/model-manager-auth-storage.test.ts（OAuth） | PASS（新增+既有） |
| 取消中的任务刷新后不复活（A08） | model-operation-client.execute：resolveFresh 完成后 fetch 收到的 combinedSignal 已 aborted → 立即拒绝、零发送 | tests/model-operation-client.test.ts · “P04-A08：任务在凭证刷新期间被取消” | PASS（新增） |
| 凭证对象不进 payload/错误/trace refs | callText 仅 apiKey 进请求头；错误 context 只含 model/provider/status/request_id；model-trace-scope refs ≤8 键 string ≤128 字符（sanitize） | tests/model-call-safety-gate.test.ts、tests/model-call-payload-redaction.test.ts | PASS（既有） |
| 不同 slot provider 独立刷新 | auxiliary-model-resolver 按 model.provider 单道 | tests/fresh-credential-routing.test.ts · “refreshes different slot providers independently” | PASS（既有） |
| 媒体 lane 新鲜凭证 | image-task-runner.ts:544 提交前 resolveMediaExecutionTarget 现取 | tests/media-credential-routing-parity.test.ts | PASS（既有） |

## 3. 负向验证（T07-4 注入）

- **注入 #1（错误 provider 选择）**：临时删除 provider-registry.getOperationModel 的 provider 联合键匹配 → P04-A01 红（1 failed，exit 1），还原后全绿。日志：`artifacts/refactor-2026/P04/logs/P04-FAULT-provider-mismatch.*`。
- **注入 #2（漏装 observer）**：临时注释 lib/pi-sdk/index.ts installModelCallStreamObserver → e2e chat 3/3 红，还原后绿。日志：`P04-FAULT-observer-missing.*`。

## 4. 边界与不做的事

- 未新增“统一客户端”包住一切；直接 fetch（callText/model-operation-client）的契约归属见 MODEL_CALLSITE_MATRIX。
- 本地可信地址判定维持三主机名，未扩大（任务书 T02-4）。
- AuthStorage/FileAuthStorageBackend 保留导出（SDK 锁定合法使用，P04-T08-1 禁删项）。
