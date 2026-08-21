# Model Call Observer — 进度（已完成）

第一轮（Phase 1 契约 + Phase 2 文本运行时）：基线 main @ e62bb535，Step 0–9 完成。
第二轮（Phase 2.5 安全收口 + Phase 3A MC-05～09 接入 + Phase 3B 控制面分离）：
基线 feature/model-call-observability @ 9dfde99a，全部完成。
第三轮（Phase 3.5 残余闭合 + Phase 4 Trace）：基线 @ e795b9ef，全部完成。

## 第三轮完成项

- Phase 3.5 残余闭合（修正 9 → 10 paths，MODEL_CALL_CLOSURE_DELTA.md）：
  - diary temporary summary（Pi generateSummary 直发 completeSimple）确认为
    生产可达独立旁路 → **MC-10**；facade re-export 包装为 Observed direct
    summary（lib/llm/observed-pi-direct-summary.ts），复用既有
    Recorder/Observer/Identity；diary 传 observerContext 获得归属 + ledger
    entry（metadata 三元组关联）。attemptVisibility=logical_boundary、零伪造
    wire 事件。真实 Pi 链测试（stub fetch 伪 SSE Provider）锁定。
  - session-snapshot-side-task-runner completeSimple：仍无生产 caller →
    LATENT / NOT_CURRENTLY_REACHABLE（不制造生产行为）。
- Phase 4 Trace Contract（lib/llm/model-trace-scope.ts，AsyncLocalStorage）：
  - ModelTraceScope（traceId/origin/refs/causalParentCallId/lastCallId）+
    统一身份解析 resolveModelTraceContext（explicit → scope → singleton；
    traceId 恒非空、parent 无事实即 null）。
  - Ingress 接线：desktop user_turn / bridge / phone / slash（inherit-or-mint）；
    automation cron、diary、memory daily+compile、dream、speech、probe、
    health check（force-new detach）；plugin/media（inherit-or-mint）；
    facade session.prompt 兜底 ingress（subagent 继承不 mint）。
  - 因果传播：stream observer 推进 lastCallId（loop 内 C2.parent=C1）；
    session-options 工具边界（全工具唯一收口）快照 causalParentCallId=产生
    toolCall 的那次调用 → 工具内 Vision/Approval/Media/Subagent/callText 自动
    继承（并行工具双双 parent=C1，子 scope 冻结快照互不覆盖）。
  - Ledger trace metadata：MC-04/05/06/07/08/09/10 + MC-02 spread 三元组；
    MC-01 message_end 补账经 WeakMap（model-call-correlation.ts）补齐
    correlation（无侵入，对象 GC 自动回收）。
  - TestModelCallObserver 扩展 Trace Explorer（eventsForTrace/callsForTrace/
    childrenOf/rootsForTrace/callIdentity/assertTraceGraphValid）。
- 测试：新增 3 文件 35 用例（trace-scope 17 / propagation 14 场景 / diary 4）
  + 更新 calltext 契约测试；第一/二轮 96 回归全绿。
- 验证：typecheck ×3 绿；eslint 0 error；lint:boundary 绿（manifest 收录
  model-trace-scope/model-call-correlation/observed-pi-direct-summary）；
  cli-runtime-closure 复核；persistence fingerprint compatible repin；
  full npm test 通过。
- §九十终验反扫答案：**NO**（无生产可达 Model Call 缺 Observer lifecycle；
  10/10 路径覆盖）。

## Seal

本轮功能提交后，VERIFIED_SOURCE_SHA 按仓库既有机制推进到新验证树
（单独 audit commit：verified-source-sha.txt + matrix 文件）。
