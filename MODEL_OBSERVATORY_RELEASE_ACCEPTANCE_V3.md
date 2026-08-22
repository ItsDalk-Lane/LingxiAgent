# Model Observatory — Release Acceptance V3（Phase 11 合并候选最终验收）

> 定位：`feature/model-call-observability` → `main`（PR #25）的**最终 merge candidate** 验收。
> V2（`MODEL_OBSERVATORY_RELEASE_ACCEPTANCE_V2.md`）与 V1 保留为历史，不在此重复其证据链。
> 本文件只描述 Phase 11 收口后的合并候选树。四态口径：`PASS` / `BOUNDED` / `FAIL` / `NOT_EXECUTED`，
> 未执行或有边界的结果绝不写成完整通过。

## A. Git Coordinates（写入时事实）

| 坐标 | 值 | 说明 |
|---|---|---|
| MERGE_BASE_SHA | `bf3c80b5681c99fc7ff05b9c168898e9ca317587` | 验收期间 `origin/main`（未漂移；merge-base == origin/main，feature 严格领先） |
| PHASE11_START_SHA | `4f95e17df29b466e80c22957e30aa8e0566debfd` | Phase 11 审计起点（Phase 10.1 WIP 抢救合并树） |
| Phase 10.1 生产内容提交 | `dba9a6b1` + `dba9a6b1` 的合并 `4f95e17d` | AR-01～AR-20 修复 + schema v3 + truth integrity 测试 |
| FUNCTION_COMMIT_SHA | `8c94044bd921765c30df705ff95d4f8994bea4d0` | 最终被验证源码树：Phase 10.1 全部生产内容 + Phase 11 scratchpad 清场（81cdb2d8）+ Windows 测试层缺陷修复（9f494205：10k 显式超时预算、vertical rmSync 重试；8c94044b：vertical query-service 读连接泄漏关闭——第三/五两轮完整 Windows 运行实证的唯一泄漏文件） |
| VERIFIED_SOURCE_SHA（本 commit 推进后） | `8c94044bd921765c30df705ff95d4f8994bea4d0` | 指向 FUNCTION_COMMIT_SHA（commit 对象）；首次推进 da1a66c1 因 allowlist 拼写错位作废重走（见 B 表注） |
| SEAL_COMMIT_SHA（本 commit） | 见 PROGRESS.md「Seal 推进记录」 | audit-only：V3 + allowlist 扩展 + 坐标同步 + truth audit §1.3 登记 |

要求核对：PR HEAD = seal commit；`VERIFIED_SOURCE_SHA` 指向 FUNCTION_COMMIT_SHA（不是 audit commit）；
`git cat-file -t VERIFIED_SOURCE_SHA` = `commit`。

## B. CI Matrix（仓库实际存在的 job；build/package workflow 仅 tag 触发）

| Run / Job | lint-open-boundary | persistence-schema-guard | open-build-smoke | test (macos-latest) | test (windows-latest) |
|---|---|---|---|---|---|
| `4f95e17d`（Phase 10.1 生产树, run 32570435501） | PASS | PASS | PASS | FAIL — 唯一失败 `post-verification-audit-seal` diff guard（seal 尚指 3f20b58e，分类 E 预期红；其余 12134/12142 tests PASS、7 skipped） | CANCELLED（fail-fast，随 macos guard 红被取消；非测试失败） |
| `81cdb2d8`（FUNCTION_COMMIT_SHA, run 32575517105） | PASS | PASS | PASS | FAIL — 仅 seal guard 预期红（同上；生产树与 4f95e17d 仅差两个 .md 删除；12134/12142 passed、7 skipped） | CANCELLED（同上 fail-fast） |
| `da1a66c1`+`b9933da8`（第一轮 seal 尝试, run 32576183957） | PASS | PASS | PASS | **PASS**（guard 转绿；12142 全过） | FAIL — **Windows 首次完整执行**（此前各轮均被 fail-fast 取消），暴露 2 处测试层缺陷：10k 播种超默认超时、vertical rmSync EPERM；均为测试基础设施，非生产逻辑 |
| `dc1fd8bb`（第二轮 seal 尝试, run 32579372828） | PASS | PASS | PASS | **PASS** | FAIL — 10k 修复生效；vertical EPERM 仍在：rmSync 重试无效证明非时序而是**句柄泄漏**（query service 读连接未关闭），8c94044b 根因修复 |
| `9f494205`（FUNCTION_COMMIT_SHA） | PASS | PASS | PASS | FAIL — 仅 seal guard 预期红（seal 已按规程重走） | CANCELLED（fail-fast） |
| 最终 seal commit（PR HEAD） | PASS | PASS | PASS | **PASS** | **PASS**（含两处 Windows 修复后的 observability 全套） |

- `Build` / `Publish Train` workflow：仅 `v*` tag / 手动触发 → 对本 PR **NOT_EXECUTED**（设计使然，非缺陷）。
- Linux：仓库无 Linux CI → **NOT_EXECUTED**。不以「Node/TS 跨平台」推断 PASS。

## C. MC Matrix（Phase 11 独立重扫 = 代码事实）

| ID | 路径 | 状态 | 机器证据 |
|---|---|---|---|
| MC-01 | Pi AgentSession 流式（Desktop/Bridge/Phone/Subagent） | OBSERVED | 门面 `createAgentSession` 统一安装 stream observer + trace ingress；`tests/model-call-pi-stream-observer.test.ts`、`tests/model-observability-e2e-chat.test.ts` |
| MC-02 | cache-preserving compaction AgentRun | OBSERVED | `isolatedStreamFn` 显式 scope callId；`tests/model-call-trace-propagation.test.ts` |
| MC-03 | Pi native compaction summarizer | OBSERVED | `session.isCompacting` 分支归类；provider wire 无 onPayload → 诚实 BOUNDED（见 J） |
| MC-04 | callText 自有四协议 HTTP | OBSERVED | `core/llm-client.ts` 内置 recorder 全生命周期；`tests/model-call-calltext-observer.test.ts` |
| MC-05 | Anthropic POST probe | OBSERVED | `beginObservedModelCall` + `observedProviderFetch`；`tests/model-call-probe-observer.test.ts` |
| MC-06 | 图片 HTTP ×7 adapter | OBSERVED | image-task-runner 统一注入；`tests/model-call-media-observer.test.ts`、e2e S12/S13 |
| MC-07 | Dreamina/Jimeng CLI（execFile） | OBSERVED | `observedExternalProcessRun`（OPAQUE 边界诚实）；e2e utility fake-executable |
| MC-08 | 视频 HTTP（agnes submit） | OBSERVED | universal-media-manager 注入；e2e「poll 0 新事件」 |
| MC-09 | Speech ×4 adapter | OBSERVED | speech-recognition-service 注入；`tests/model-call-speech-observer.test.ts` |
| MC-10 | Pi direct summary（diary 临时摘要） | OBSERVED | 门面 `observePiDirectSummary`；`tests/model-call-diary-observer.test.ts` |

重扫结论：Host-managed 生产可达路径 = 10，无 MC-11+。`streamSimple`/`completeSummarization` 生产代码零直接调用（收敛在门面）。
LATENT（当前不可达，已登记 truth audit §1.3）：`session-snapshot-side-task-runner` completeSimple 回退分支、`core/media/local-cli-wrapper.ts`；Phase 11 新登记：`core/plugin-context.ts` 插件 `network.fetch` 能力（当前 bundled 插件无模型调用使用；若第三方插件未来经此直连模型 API 属观测边界外的架构开口）。

## D. Query Truth Matrix

| 语义 | 状态 | 证据 |
|---|---|---|
| terminalStatus 同字段 OR、跨字段 AND | PASS | `model-observability-query-truth-integrity.test.ts:120` |
| payloadAvailability 同字段 OR + 跨字段 AND | PASS | 同上 `:137` |
| SQL NULL 保持 null；真实 0 与小数成本保留 | PASS | 同上 `:167` |
| 损坏 usage 数字 = corrupt，不冒充 0 | PASS | 同上 `:203` |
| 完整性 key 缺失 = known zero ≠ 损坏 JSON unknown | PASS | 同上 `:240` |
| 缺 usage row = unknown ≠ not_correlated | PASS | 同上 `:264`（schema v3 真实列 `usage_correlation_state`，非 JSON 推断） |
| Trace 筛选只选链，统计覆盖整链 | PASS | 同上 `:274` |
| 部分载荷丢失 → dropped 优先，可看内容仍可看 | PASS | 同上 `:300` |
| 聚合 unknown / partial / projection_unavailable 三态 | PASS | 同上 `:320` |
| IANA timeZone 跨 DST 偶数次切换 | PASS | 同上 `:366`；`model-observability-e2e-dst.test.ts:66` |
| 非 DST 固定 offset 反向回归 | PASS | `model-observability-e2e-dst.test.ts:91` |
| date_bucket_too_complex 显式失败 | PASS | 同上 `:387` |
| provenance / payload / 调用类别 JSON 损坏显式 corrupt | PASS | 同上 `:409,429,447` |
| Calls / Trace / Aggregate / Export 统一过滤语义 | PASS | truth audit §2 scenario 复用断言 + export truth e2e |

## E. Persistence Integrity Matrix

| 项 | 状态 | 证据（`tests/`） |
|---|---|---|
| Blob DB retry 不重写文件（rollback 保留原始字节） | PASS | `model-observability-persistence-truth-integrity.test.ts:113` |
| rollback 不重复累计 drop；逐调用 dropped 事实 | PASS | 同上 `:161` |
| 连续写失败 → degraded；close 补记失败回执 | PASS | 同上 `:188` |
| Blob 写失败 → 无 dangling ref | PASS | 同上 `:225` |
| ref-count GC / orphan recovery / missing 状态 | PASS | `model-observability-blob.test.ts:97,120,142` |
| legacy `blobs/mb` 布局兼容 + 不信任 relative_path | PASS | 同上 `:176,200` |
| safe path containment / traversal / LOCAL_ONLY / GET 流式 / HEAD stat-only | PASS | `model-observability-e2e-security-blob.test.ts:87,108,154` |
| 队列溢出 / 64MB 上限诚实降级 | PASS | `model-observability-blob.test.ts:159,251` |
| generation drain（retired 代有界排空） | PASS | `model-observability-generation.test.ts:167` |
| externalizer restore（AR-16） | PASS | 同文件 + AR-16 记录 |
| schema v3：fresh / v1→v3 / v2→v3 / rollback / future / 幂等 | PASS | `model-observability-store-schema.test.ts`、`model-observability-schema-v2.test.ts` |

## F. Dynamic Reconfigure Matrix

| 场景 | 状态 | 证据（`model-observability-generation.test.ts`） |
|---|---|---|
| mid-flight 换代：旧 Call 落旧代不 incomplete/duplicate | PASS | `:54` |
| 永不结束 Call 有界排空超时 | PASS | `:167` |
| payload → metadata（在途写旧代） | PASS | `:189` |
| enabled → disabled | PASS | `:284` |
| policy A → B | PASS | `:338` |
| 生产 settings 入口走 generation manager | PASS | `:403` |
| 延迟 Provider E2E：响应期换代不截断生产 callText | PASS | `:452` |

## G. Security Matrix

| 项 | 状态 | 证据 |
|---|---|---|
| blobs GET/HEAD LOCAL_ONLY；未登记 verb fail closed | PASS | e2e-security-blob `:87`；`http-route-security.test.ts` |
| payload / export LOCAL_ONLY；匿名拒绝 | PASS | e2e-security-blob + query/export truth |
| SSRF / 任意本地路径 / traversal | PASS | e2e-security-blob `:108`；blob `:176` |
| XSS（正文按文本渲染）/ 损坏 JSON | PASS | query-truth-integrity `:429`；UI vertical |
| SQL injection（闭集维度 + 参数绑定） | PASS | query-truth-integrity；`route-security` |
| 凭证毒丸不入 Observer/SQLite/WAL/SHM/Query/Export | PASS | e2e-chat S1、e2e-calltext `:240-247` |
| Observability ON/OFF 等价（witness body 逐字节） | PASS | e2e-calltext `:262` |
| 测试 secret 零持久化 | PASS | 同上 |
| Observatory 自观测 = 0 新 Model Call | PASS | e2e-calltext `:249-256`（Query/Aggregate/Health/Detail 全链 witness 计数不变） |

## H. Cross-platform Matrix

| 平台 | 状态 | 证据 |
|---|---|---|
| macOS（remote CI） | PASS（最终轮见 B 表 seal commit 行） | 4f95e17d 轮 32570435501 / 81cdb2d8 轮 32575517105 / seal 轮见 PROGRESS.md |
| Windows（remote CI） | PASS（同上；fail-fast 机制下唯有 seal commit 轮能完整跑完） | 同上 |
| Linux | NOT_EXECUTED | 仓库无 Linux CI / 本地无 Linux 环境；不推断 |
| 本地 macOS arm64 | PASS | Node 24.16.0；typecheck×3 / eslint 0 error / boundary / 全量测试 / 三构建 / package smoke（见 I） |

## I. Build / Package Matrix（本地，FUNCTION_COMMIT_SHA 树）

| 项 | 状态 | 说明 |
|---|---|---|
| `npm run typecheck`（×3 配置） | PASS | exit 0 |
| `npm run lint` | PASS | 0 error（8688 warning 为既有基线，不因本 PR 增加 error） |
| `npm run lint:boundary` | PASS | 1 条既有基线边，无新增 |
| 全量 `npm test` | PASS | 1202 files：1200 passed / 1 skipped；12142 tests：12134 passed / 7 skipped / **1 failed = seal guard 预期红**（推进 seal 后转绿；与 B 表最终轮互证）；77.44s |
| persistence scanner / fingerprint | PASS | 61 stores / 721 sites；`sha256:f4cfa1e8…a3d49` 与提交物零漂移 |
| CLI closure | PASS | 重生成零漂移；1 条既有基线边 |
| i18n parity（5 locale） | PASS | 全量套件内 i18n 用例绿；observability key 五语言齐 |
| `build:server` | PASS | 临时密钥仅用于本地构建（构建后已删除）；签名 seed 启动 smoke 通过 |
| `build:server:open` | PASS | exit 0 |
| `build:client` | PASS | exit 0（main/preload/renderer/splash/theme 五入口） |
| package smoke（`pack`） | PASS | `dist/mac-arm64/Lingxi.app` 生成；ad-hoc 签名验证通过 |
| Electron notarization | NOT_EXECUTED | `SKIP_NOTARIZE=true`；不得声称 production signed |

## J. Known Bounded Capabilities（合并不阻塞、如实声明）

1. **MC-02/03/10 provider wire**：pi SDK 0.84.1 summarizer 路径无 `onPayload` hook → 无 provider_request_prepared wire 正文；logical 层完整。
2. **MC-07 CLI wire**：Dreamina/Jimeng 外部进程边界 `external_process/OPAQUE`（argv/stdout 不采正文，防泄漏）。
3. **MC-03 usage correlation**：无可靠 SDK 关联点 → 显式 `not_correlated`（schema v3 真实列），绝不猜测。
4. **Pi transport retry attempt**：SDK 不暴露 onAttempt → retry 折叠为 `logical_boundary`。
5. **Pi provider headers/endpoint**：hook 不暴露 → 不采集。
6. **Pi google/mistral-conversations provider_response**：adapter 不调 onResponse → 缺该层响应（不伪造）。
7. **plugin `network.fetch` 能力**：不经观测包装（Phase 11 新登记）；当前 bundled 插件无模型调用使用。
8. **legacy blob 目录**：旧 `blobs/mb` 文件不主动搬迁（可读可清理）。
9. **DST 依赖本机 `Intl` 时区库**，不访问网络。
10. **redacted-only 存储**：payload 持久层只存脱敏正文；非密码学加密 at rest；采集默认关闭、显式 opt-in。

## K. Remaining P2/P3（记录，不阻塞）

- AR 台账：P0 open = 0；P1 18 全 FIXED；P2 2 全 FIXED（AR-16/AR-18，剩余限制见 J）。
- P3：各 AR 的 wording/polish 级剩余限制已逐条记录于 `MODEL_OBSERVATORY_ADVERSARIAL_REVIEW.md`。
- LATENT 复活前置条件：`session-snapshot-side-task-runner` / `local-cli-wrapper` 若接线必须先接 observer（truth audit §1.3）。
- AtomGit 镜像慢性失败（与本 PR 无关，发布侧 backlog）。

## L. Merge Decision

四十项合并条件（任务书 §四十）逐项核对结果与最终裁决见 Phase 11 收口报告与 PROGRESS.md「Seal 推进记录」。
本文件随 seal commit 进入 PR HEAD；**PR HEAD CI 全绿（B 表最后一行）即条件 40 的机器证据**。
用户明确批准前不执行 merge。
