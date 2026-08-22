# PROMPT_PROVENANCE_PROGRESS.md — Phase 5 Semantic Input Provenance 进度

> 状态：**本轮已完成**（2026-08-21，分支 feature/model-call-observability）。
> 换会话先读本文件 + SEMANTIC_INPUT_PROVENANCE_AUDIT.md +
> OBSERVABILITY_IMPLEMENTATION_NOTES.md 的 Phase 5 节。

## Current phase

Phase 5 全部 21 步完成；等待 seal 推进与合并。

## 实现坐标

| 模块 | 文件 |
| --- | --- |
| Contract + renderer + builders | lib/llm/semantic-input-provenance.ts |
| 快照 payload 装配 | lib/llm/semantic-input-provenance-payload.ts |
| Sidecar（recorder 持有 + summary + symbol 引用） | lib/llm/model-call-recorder.ts |
| scope 携带（MC-02 runner → observer） | lib/llm/model-call-scope.ts |
| MC-01/03 边界 + turn 标记 + 尾段扩展 | lib/pi-sdk/model-call-stream-observer.ts |
| MC-02 per-call 构造 | lib/llm/cache-preserving-compaction-agent-run.ts |
| MC-04 归一化 + fallback + codex adapter_injected | core/llm-client.ts |
| length-contract repair provenance | core/output-length-contract.ts |
| layout template identity 集成 | lib/llm/prompt-layout.ts |
| Agent canonical 装配 | core/agent.ts buildSystemPromptArtifact |
| 快照冻结/恢复 | core/session-prompt-snapshot.ts + core/session-coordinator.ts（主路径 :2011-2090/:2672、isolated :7975-:8150） |
| MC-05 | lib/llm/provider-client.ts |
| MC-06/08（+MC-07 形状） | core/media/image-task-runner.ts / core/media/universal-media-manager.ts |
| MC-09 | core/speech-recognition-service.ts |
| MC-10 | lib/pi-sdk/index.ts + lib/llm/observed-pi-direct-summary.ts |
| utility callers | deep-memory / compile / session-summary+rolling-summary-format / dream/model-runner / approval-gateway / vision-bridge / diary-writer / agent-appearance-summary / llm-utils / rc-summary / install-skill / server routes models |

## Precision status

- exact：MC-05/06/08/09/10、MC-01 system 前缀+messages+tools、MC-04 显式 caller。
- partial：MC-01（SDK 尾段）、MC-02（system）、MC-03（全部 structural）、MC-04
  fallback caller（plugin passthrough）、MC-07（CLI wire）。
- 每条 gap 见 OBSERVABILITY_IMPLEMENTATION_NOTES.md「Known Opaque Sources」。

## Tests

- tests/semantic-input-provenance.test.ts（契约 19）
- tests/semantic-provenance-integration.test.ts（MC-01/02/03 + 快照 9）
- tests/semantic-provenance-calltext.test.ts（MC-04 e2e 4）
- tests/agent-system-prompt-equivalence.test.ts（golden 3）+ tests/fixtures/
  system-prompt-golden-{zh,en}.txt（改造前生成）
- 既有 memory/dream/approval/vision/diary/appearance/observer/trace/control-plane
  回归随 full suite（11776）全绿。

## Remaining gaps / 下一轮入口

- Phase 6 Request/Response Capture：semanticRequest + provenance 在 callId 下
  并取；Redaction Contract；Provider wire 层 provenance。
- rolling summary system 内嵌值 span 拆分（模板字面量重构，收益低，暂列 gap）。
- SDK 若未来 export SUMMARIZATION_SYSTEM_PROMPT / skills 注入边界，MC-03 与
  MC-01 尾段可升 exact（届时删 mirror 比较逻辑）。

## Seal

- 持久化指纹：compatible repin（build/persistence-schema-fingerprint.json，
  sha256:b3a5d2b7…）。
- cli-runtime-closure / open-boundary baseline 重算；export-manifest 收录两个
  新 lib 文件。
- VERIFIED_SOURCE_SHA：见 .sync-audit/verified-source-sha.txt（本轮功能 commit
  后推进）。
