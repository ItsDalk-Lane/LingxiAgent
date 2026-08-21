# Model Call Observer — 进度（已完成）

第一轮（Phase 1 契约 + Phase 2 文本运行时）：基线 main @ e62bb535，Step 0–9 完成。
第二轮（Phase 2.5 安全收口 + Phase 3A MC-05～09 接入 + Phase 3B 控制面分离）：
基线 feature/model-call-observability @ 9dfde99a，全部完成。

## 第二轮完成项

- Phase 2.5 对抗性收口：
  - 错误安全契约：`normalizeModelCallError` → {name, code, message(仅
    `markModelCallSafeMessage` 显式标记的内部固定文案)}；Provider raw error
    body → AppError.message → observer 的泄漏链在唯一出口切断。
  - Metadata Safety Gate：`sanitizeModelCallDetails`（键 denylist 归一整键
    匹配 + 值形状 gate），在 Recorder emit 唯一出口执行，集成点无法绕过。
  - providerRequestId 边界：string only / trim / ≤128（超长丢弃）。
  - Recorder 状态机：logical_call_end 后一切方法 silent no-op；attemptErrored
    防重复投递。
  - Attempt visibility 枚举（exact/logical_boundary/external_process_boundary）
    + providerWireVisibility 枚举（request_response/response_only/opaque）。
- Phase 3A：`lib/llm/model-call-integration.ts` 统一接入层（HTTP attempt
  helper 可重复调用 = 同 call 多 attempt；external process helper）。
  - MC-05 probeProvider：Anthropic POST 分支进 observer（callId 进 ledger
    metadata）；GET /models 分支拆出 accounting（控制面）。
  - MC-06 runSubmitInBackground 逻辑调用边界 + 7 个 HTTP image adapter 的
    fetch 全部经 observedProviderFetch（Codex 401 refresh = 1 callId +
    2 attemptId，硬验收测试锁定）。
  - MC-07 dreamina submit 经 observedExternalProcessRun（opaque，不伪造 wire）。
  - MC-08 submitVideo 边界 + agnesVideoAdapter fetch；providerTaskId 响应后
    关联；poll 0 事件。
  - MC-09 _transcribeWithAccounting 边界 + 4 个 speech adapter fetch；
    fileId/inputSizeBucket 安全 attribution；Volcengine body 内 credential
    不进事件（测试锁定）。
- Phase 3B：媒体 poll、GET /models probe、外部凭证授权全部出 Usage Ledger
  且 0 observer 事件（4 个测试文件锁定新契约）。
- 测试：新增 5 文件 37 用例（safety-gate/probe/media/speech/control-plane）
  + 更新 4 个既有文件；第一轮 MC-01～04 回归全绿。
- 验证：typecheck ×3 绿；eslint 0 error；lint:boundary 绿（manifest 收录
  model-call-integration.ts）；persistence fingerprint compatible repin；
  cli-runtime-closure/open-boundary-baseline 复核；full `npm test`
  11706 通过 / 0 失败 / 7 skipped。
- §74 Egress 复扫：仍为 9 条 Host-managed 路径，无 MC-10（media.ts 的
  execFile 为本地文件打开，非 AI；diary-writer/side-task 两条范围外旁路
  记入 Remaining Gaps）。

## Seal

本轮功能提交后，VERIFIED_SOURCE_SHA 按仓库既有机制推进到新验证树
（单独 audit commit：verified-source-sha.txt + matrix 文件）。
