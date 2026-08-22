# PAYLOAD_CAPTURE_PROGRESS.md — Phase 6 进度（Sensitive Payload Capture）

> 本文件是 Phase 6 的跨会话断点文件：新会话先读本文件 +
> MODEL_CALL_PAYLOAD_CAPTURE_AUDIT.md，再动代码。
> 基线：`feature/model-call-observability`；Phase 5 完成树 `3cf0e6ed`（seal `ea909c6e`）。

## Current phase

**Phase 6 完成（待 seal 推进）。** 开发顺序按任务书 §一百七十一执行完毕：
Step 0（基线）→ 1（Capture Boundary Audit）→ 2-5（契约五模块 + Noop/Test sink +
bounded redactor）→ 6-7（semantic request capture + MC-04 端到端）→ 8-9
（provider provenance + callText 四协议 mapping）→ 10-11（Pi capture + hook
fidelity 实证）→ 12-15（MC-05/06/07/08/09/10）→ 16-20（安全/性能/矩阵测试）→
21-25（typecheck ×3 / eslint 0 error / lint:boundary / targeted / full npm test）。

## Audited paths（MODEL_CALL_PAYLOAD_CAPTURE_AUDIT.md 摘要）

- Pi hook 实证（0.84.1 原厂 dist）：before_provider_request payload = compat 后、
  序列化前最终 body（runtime_exact）；after_provider_response = status+headers
  （metadata_only）；google/mistral-conversations 无 onResponse；summarizer
  options 无 onPayload（MC-03/MC-10 wire 不可见）；onPayload 每 logical attempt
  恰一次（transport retry 不重复触发）。
- Volcengine ASR body credential：adapters.ts `body.user.uid`（协议专项规则）。
- 控制面（0 record 结构性保证）：GET /models、媒体 poll、资产下载、credential
  resolve/refresh/authorize、媒体能力刷新、Dreamina query/checkAuth。

## Capture coverage（MC-01～MC-10 四层，全 FULL/显式降级）

见 OBSERVABILITY_IMPLEMENTATION_NOTES.md「MC-01～MC-10 Capture Matrix」。
非 FULL 项全部显式：MC-01 response=METADATA_ONLY（hook 能力）；MC-02/03/10
provider wire=UNAVAILABLE；MC-07=OPAQUE；probe 成功 response=METADATA_ONLY。

## Redaction coverage（正反例测试锁定）

API Key（header 键 + body 键 + inline sk-）｜Bearer/Basic｜JWT｜GitHub/Google/
AWS token｜PEM 整块｜kv secret｜Cookie/Set-Cookie｜OAuth refresh｜Volcengine
body credential（协议专项）｜Signed URL（X-Amz/X-Goog query → descriptor）｜
本地绝对路径（整串 descriptor + inline 替换）｜Binary（Buffer/Blob/base64/
data URL/FormData → external_blob）。反例：普通 prompt/memory/response/UUID/
file id/研究文本存活。

## Provider mapping

callText 四协议（anthropic-messages/openai-completions/openai-responses/
openai-codex-responses）：构造时产生 + normalizeProviderPayload 后 locator 校验
（失配降级 structural）；codex 空系统注入 adapter_injected → instructions span。
Pi 路径 null（vendor 构造，无法 sidecar 化——诚实，§六十一）。

## Tests

新增 7 文件 103 用例：redaction 49 / capture 9 / calltext 12 / pi 8 / media 13 /
speech 7 / summary 5。既有观测 14 文件 131 用例回归全绿。typecheck ×3、
eslint 0 error、lint:boundary 绿（export-manifest 收录 5 新模块）。

## Known gaps

1. Pi provider request 的 headers/endpoint 不可见（hook 不暴露）——矩阵已注明。
2. Pi provider mapping sidecar 需上游支持，当前 null。
3. redacted_thinking 只保留 `[redacted_thinking]` 结构标记。
4. 未来 Payload Store / Blob Store / Query API / Export / Usage UI 均未实现
   （本轮明示不做，§一六一～一六五）。

## Seal

功能树推进后按仓库既有机制更新（见 OBSERVABILITY_PROGRESS.md 对应条目）。
