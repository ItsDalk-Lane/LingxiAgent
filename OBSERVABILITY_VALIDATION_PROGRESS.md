# Observability Validation Progress — Phase 10（第九轮）

> 跨会话断点文件。新会话首先阅读本文件 + MODEL_OBSERVABILITY_E2E_TRUTH_AUDIT.md，
> 然后从第一个未完成 Scenario 继续，不重头重复。
> Phase 10 全部矩阵与结论的最终事实源是 MODEL_OBSERVABILITY_E2E_TRUTH_AUDIT.md
> 与 MODEL_OBSERVABILITY_RELEASE_ACCEPTANCE.md（完成后新增）。

## Baseline

| 项 | 值 |
| --- | --- |
| PHASE10_START_SHA | `d0b65509b1dd436b399f6b5b40b7330471ee50b8`（round-8 seal commit） |
| Verified tree at start | `61779cbdda5b46082f32a554b99279149980c0b4`（Phase 9 UI 树） |
| Branch | `feature/model-call-observability`（worktree 干净） |
| main | `e62bb53545c3439cf798a9785f97ada3e1d6a3e1` |

## Checklist（Step 顺序，按任务书 §一百六十七）

- [x] Step 0 — 记录 HEAD/main/worktree/verified SHA；读完 Phase 1~9 全部 12 份文档
- [x] Step 1 — 全仓 Model Egress 重扫（结论：仍为 MC-01～MC-10；新增 LATENT：
      `core/media/local-cli-wrapper.ts` runLocalCliMedia，全仓零 importer，未接线，
      详见 E2E_TRUTH_AUDIT §1；不产生 MC-11）
- [x] Step 2 — MODEL_OBSERVABILITY_E2E_TRUTH_AUDIT.md（40 Scenario 矩阵 + 14 节设计）
- [x] Step 3 — Severity 标准（TRUTH_AUDIT §14）
- [x] Step 4 — Fake Provider Witness（tests/helpers/model-observability-scenario-harness.ts；
      node:http 真实 server + SSE/JSON/401/429/500/hang/reset fixture + 请求 journal）
- [x] Step 5 — Scenario Harness（temp HOME + installModelObservabilityPersistence +
      createModelObservabilityQueryService + Hono route + ledger accounting 接线
      （镜像 engine wiring，consumer 收 {type:"llm_usage",entry} 事件形状））
- [x] Step 6 — MC-01 simple + multi-turn（真实 Pi AgentSession.prompt：ModelRuntime
      registerProvider("witness-provider") + inMemory SessionManager + observer ext
      经 DefaultResourceLoader extensionFactories 注入（生产同机制）；S1 四层 payload、
      hook body≡witness body（redaction 允许差异=本地路径内联替换+sanitizationStatus）；
      S2 tool loop C2.parent=C1.callId（非 toolCallId）、tool result 回流 witness 第 2 轮）
- [ ] Step 7 — parallel tool + subagent（S3/S4 未做——S2 已证 loop 链；S3/S4 依赖
      更重 coordinator ingress，计划以 ALS 真实机制测试补）
- [ ] Step 8 — MC-02 / MC-03（部分覆盖：MC-10 测试证 unavailable 语义同源）
- [x] Step 9 — MC-04 四协议 fixture（S7 全绿：anthropic/openai-chat/responses/codex；
      witness body≡capture≡mapping、callId 全链、usage 全链、毒丸 DB/WAL/SHM 零命中）
- [ ] Step 10 — utility representative matrix（S8 部分：diary 终稿/health 类经 S7/S18
      覆盖 callText 机制；approval repair/memory 模板场景未做）
- [x] Step 11 — MC-05（S11：POST probe 进 Observatory、GET /models 0 record）
- [x] Step 12 — MC-06 normal + codex 401（S12+S13 硬场景全绿：1 call/2 attempts/
      2 provider_request ordinal 1,2/2 provider_response/1+1 semantic/查询=1 Call；
      refresh 走凭证 bus 非 HTTP 生成）
- [x] Step 13 — MC-07 CLI（S15：OPAQUE 不升级、argv/stdout 毒丸不进 payload、0 HTTP）
- [x] Step 14 — MC-08 video（S16：submit=call（witness 真实 HTTP）、poll 0 新事件）
- [x] Step 15 — MC-09 speech（S17 双协议：openai+volcengine；audio=local_file_reference、
      language hint、body.user.uid 协议脱敏（witness 见毒丸/库内 <redacted:credential>）、
      transcription 语义响应）
- [x] Step 16 — MC-10 diary（S18：2 临时摘要+终稿 same trace、parent=null、
      MC-10 wire=unavailable 诚实 / MC-04 终稿=FULL）
- [x] Step 17 — error/abort/timeout matrix（S19：429/500/invalid JSON/hang/reset/abort
      全绿；aborted≠error；毒丸错误正文不入 durable）
- [ ] Step 18 — concurrency / ALS leak（S20/S21/S22 未做）
- [x] Step 19 — recording mode equivalence（S23：ON/OFF witness body 逐字节相同 +
      业务返回一致）
- [ ] Step 20 — crash/restart（S24）
- [ ] Step 21 — retention / GC（S25/S26）
- [ ] Step 22 — queue overflow / write failure（S27/S28）
- [ ] Step 23 — Query/filter/group/pagination truth（S29）
- [ ] Step 24 — timezone/DST（S30 — 进行中）
- [ ] Step 25 — HTTP/security/SSRF/path/XSS（S31）
- [ ] Step 26 — credential poison cross-layer scan（S32 — 部分经各 scenario 内嵌覆盖）
- [ ] Step 27 — UI vertical slice（S34）
- [ ] Step 28 — Trace/Inspector/Payload vertical slice（并入 S34）
- [ ] Step 29 — Export truth（S35）
- [ ] Step 30 — Blob performance/security（S36）
- [ ] Step 31 — general performance/backpressure（S37/S38）
- [ ] Step 32 — fix all proven P0
- [ ] Step 33 — fix all proven P1
- [ ] Step 34 — rerun all failing scenarios
- [ ] Step 35 — schema/persistence artifacts if affected
- [ ] Step 36 — typecheck ×3
- [ ] Step 37 — eslint
- [ ] Step 38 — lint:boundary
- [ ] Step 39 — persistence guards
- [ ] Step 40 — i18n guards
- [ ] Step 41 — targeted observability tests
- [ ] Step 42 — full npm test
- [ ] Step 43 — build:server / build:server:open / build:client
- [ ] Step 44 — package/smoke where environment allows
- [ ] Step 45 — MODEL_OBSERVABILITY_RELEASE_ACCEPTANCE.md
- [ ] Step 46 — update observability docs
- [ ] Step 47 — VERIFIED_SOURCE_SHA seal

## 已交付测试文件（Phase 10 新增）

- tests/helpers/model-observability-scenario-harness.ts（witness + harness + fixtures）
- tests/model-observability-e2e-calltext.test.ts（S7×4 + S23；5 用例）
- tests/model-observability-e2e-chat.test.ts（S1/S2 真实 Pi session；2 用例）
- tests/model-observability-e2e-media-speech.test.ts（S11/S12/S13/S17×2；5 用例）
- tests/model-observability-e2e-utility.test.ts（S15/S16/S18/S19×5；9 用例）

## 教训（本轮新增）

- pi SSE fixture：事件块之间必须空行（单块多 data: 行按 spec 拼接 → JSON.parse 失败）。
- initializeAccounting consumer 期望 engine 事件形状 {type:"llm_usage", entry}，
  不是裸 ledger entry。
- Pi 真实 session 三要素：ModelRuntime.registerProvider(name,{baseUrl,apiKey,
  authHeader})（auth 可解析 + baseUrl 不被覆盖）；SessionManager.inMemory()；
  observer 扩展经 DefaultResourceLoader({extensionFactories}) 注入（engine 同机制）。
- Pi customTools execute 契约：execute(toolCallId, args) → {content:[{type:"text",text}]}。
- runSubmitInBackground 的 bus/log/config 从 submitCtx 读（不是 ctx）。
- query/aggregate 输入是 normalized 形状：normalizeModelObservabilityQuery({filter:{...}})；
  aggregate 显式 {filter, groupBy:[], dateBucket:null}。

## Findings Ledger（P0/P1/P2/P3；修复记录 before/root cause/fix/after）

### F-1（P1，已修）DST 历史 date bucket 分错日期 + dateBucket 未知字段静默忽略

- **before**（failing test `tests/model-observability-e2e-dst.test.ts`）：
  America/Los_Angeles 跨 DST（2026-03-08）两条当地 23:30 的 call——A（PST）当地
  日期 2026-03-07、B（PDT）当地 2026-03-08。旧契约只支持固定
  `utcOffsetMinutes`：renderer 发送「当前 offset」会把 A 分到 03-08、B 分到
  03-07（互换）；且 `{timeZone:...}` 被正常器**静默忽略**回落 offset=0（UTC
  分桶 03-08/03-09），比错误 offset 更糟（silent wrong）。
- **root cause**：SQL `strftime('%Y-%m-%d', started_at, printf('%+d minutes', ?))`
  的 offset 是 per-query 常量；历史行的本地 offset 与查询时不同。normalizer
  不拒绝 dateBucket 未知字段。
- **fix**（§八十一最小扩展，不改 schema）：
  1. `shared/model-observability-api-contract.ts`：DateBucket 增加可选
     `timeZone`（IANA；与 utcOffsetMinutes 二选一，同时给出 → invalid_filter）。
  2. `model-observability-query-types.ts`：normalizer 校验 timeZone
     （Intl 构造试探，无效 → invalid_filter）；dateBucket 未知字段显式
     unknown_field 拒绝（消灭静默忽略）。
  3. `model-observability-query.ts`：timeZone 分桶 = 过滤集 [min,max]
     started_at 一次轻量 SQL 探测 → JS Intl 二分 DST transition（≤16 段，
     retention 180d 正常 ≤2 段）→ SQL `CASE WHEN started_at < b_i THEN
     strftime(off_{i-1}) ... ELSE strftime(off_last)` 有界展开；空集回落
     offset(Date.now())。
- **after**：failing test 转绿（A→03-07、B→03-08）；反向回归（固定 offset
  UTC+8 非 DST 语义保持）绿；既有 query/export/durable-matrix 33 用例绿。
- 反向回归（§一百五十八）：`tests/model-observability-e2e-dst.test.ts` 第二用例。

### F-4（P1，已修）round-8 seal 把 tree sha 写入 VERIFIED_SOURCE_SHA（坐标类型错误）

- **before**：`.sync-audit/verified-source-sha.txt` = `61779cbd…`（git cat-file
  = **tree**，为 Phase 9 功能 commit `ec2a7e1b` 的 tree）；seal guard 测试
  `rev-parse --verify <sha>^{commit}` 自 round-8 seal 起结构性失败
  （full suite 1 failed——本轮全量首跑捕获）。
- **root cause**：round-8 seal commit（d0b65509）写入的是 `ec2a7e1b^{tree}`
  的 40-hex 而非功能 commit 本身；历轮（cfab8556/bfde47bc/3cf0e6ed）均为
  commit。
- **fix**：坐标修正为 `ec2a7e1bc0c2a3b7e3fe8cf65d20659caee2b12d`（round-8
  功能 commit——被 round-8 验证覆盖的确切树）；同轮把 Phase 10 三份新审计
  文档加入 AUDIT_ALLOWLIST（guard 自身文件在 allowlist 内，属审计性维护）。
- **follow-up（全量复跑捕获）**：仅改 txt 后 upstream-sync-matrix Gate A 红——
  坐标五处事实源（txt / build-sync-matrix.mjs 常量 / matrix JSON /
  UPSTREAM_SYNC_MATRIX.md / UPSTREAM_SYNC_AUDIT.md / PROGRESS.md）必须同步。
  终态：常量改为 commit sha（注释同步更正"tree 坐标"错误论据）→ 重跑
  build-sync-matrix.mjs 再生 JSON+MD → 两份文档坐标行修正。
- **after**：seal guard 3 用例 + Gate A 7 用例双绿；全量复跑 0 failed。
  本轮 seal 推进时写入**功能 commit sha**（非 tree），并在 seal commit
  message 显式注明。

### F-2（P3，文档修正）Phase 7 durable matrix「MC-09 audio→blob stored」过实

- 实测（S17 openai speech）：audio 语义输入=local_file_reference；FormData
  的 web Blob → external_blob descriptor（captureStatus=externalized，无
  blobId）；media b64_json（字符串）同样只 externalized。blob store 的真实
  生产者=runtime 真实 materialized 的 Node Buffer/TypedArray（当前 MC 链罕见）。
- 处置：不是代码缺陷（descriptor 诚实、绝不写字节）；Release Acceptance 的
  Payload/Blob 矩阵按实际语义标注 externalized（BOUNDED）。

### F-3（P2，已补测试）getStoredBlob（Phase 9 blob exact route 后端）零测试覆盖

- before：`getStoredBlob`/blobs 路由不在任何测试（route-security 测试也没覆盖
  blobs 条目）。
- fix：`tests/model-observability-e2e-security-blob.test.ts`——LOCAL_ONLY
  （GET/HEAD 远程 owner 403 local_only_route、本地允许、anonymous 拒）、
  POST fail closed、真实 blob GET roundtrip + no-store/nosniff、HEAD 不读
  字节（64MB <1s stat-only）、invalid/traversal/超长 id 全 400、不存在 404、
  磁盘删除 404 blob_missing（不 500）。

### 非 findings（复核为正确行为，记录防误判）

- MC-01 durable 缺 provider_request/provider_response：为测试未挂 engine 级
  observer 扩展（server/index.ts:551 registerExtensionFactory）——生产恒注册；
  E2E 已按生产机制（DefaultResourceLoader extensionFactories）注入后四层齐全。
- callText 本地绝对路径（pi docs 路径）在 capture 中被内联替换：Redactor §三十
  文档化行为；sanitizationStatus=redacted 如实记录（S1 断言锁定）。
- crash 后 DTO terminalStatus=null + interruptedByRestart=true：Phase 7 契约
  （不伪造终态）；filter 的 `incomplete` 伪值与 UI Incomplete 呈现分层正确。
- MC-03 usage correlation=NONE：pi 0.84.1 completeSummarization options 无
  onPayload/usage hook（Phase 6 audit §1.4 实证），无准确 callId association
  point → 保持 not_correlated，Release Acceptance 标 KNOWN CAPABILITY GAP
  （不按时间/模型/顺序猜，§三十/三十一）。

## Checklist 进度见上方（Step 36-40 门禁结果）

- typecheck ×3：全绿（main/node/test）。
- eslint：0 error（114 no-explicit-any warning，与既有测试风格一致）。
- lint:boundary：绿（1 既有基线 debt，无新增）。
- persistence scanner：61 stores/720 sites 绿；fingerprint 未变
  （sha256:15591e09…，与 Phase 9 repin 一致——本轮无 store 形状变化）。
- compute-cli-closure：重生成绿。
- i18n parity/coverage：绿。
- targeted observability 回归：12 文件 134 + 15 文件 161 + 17 文件 157 全绿。
- **full npm test（F-4 全部修正后复跑，最终记录）**：12084 passed /
  0 failed / 7 skipped（1196 files passed，exit=0，2026-08-22）。
- **builds**：build:server / build:server:open / build:client 三者全绿
  （throwaway 签名 key 经 scripts/artifact-keygen.mjs 生成于 /tmp，0600，
  不入库不提交；LINGXI_SIGN_KEY + LINGXI_SIGN_KEYSET 指向它）。
- **package smoke**：renderer bundle 含 model-observability client；
  server bundle（bundle/index.js）含 route（model-observability/calls、
  /export）、query（model_call_usage）、storage（observability.sqlite、
  interrupted_by_restart、blob_missing）标记；verify-seed-kit 对
  throwaway keyset 验证 seed manifest+签名 OK。
- Windows/Linux：**NOT EXECUTED**（仅本机 macOS arm64；不伪造 PASS）。

## 教训（本轮新增）

- pi SSE fixture：事件块之间必须空行（单块多 data: 行按 spec 拼接 → JSON.parse 失败）。
- initializeAccounting consumer 期望 engine 事件形状 {type:"llm_usage", entry}，
  不是裸 ledger entry。
- Pi 真实 session 三要素：ModelRuntime.registerProvider(name,{baseUrl,apiKey,
  authHeader})（auth 可解析 + baseUrl 不被覆盖）；SessionManager.inMemory()；
  observer 扩展经 DefaultResourceLoader({extensionFactories}) 注入（engine 同机制）。
- Pi customTools execute 契约：execute(toolCallId, args) → {content:[{type:"text",text}]}。
- runSubmitInBackground 的 bus/log/config 从 submitCtx 读（不是 ctx）。
- query/aggregate 输入是 normalized 形状：normalizeModelObservabilityQuery({filter:{...}})；
  aggregate 显式 {filter, groupBy:[], dateBucket:null}。
- tsconfig.test 非 strict：`if (!x.ok)` 不收窄 union——用 `x.ok === false`。
