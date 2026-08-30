# PROGRESS — Seal 推进台账（前身：openhanako v0.444.1 → v0.447.4 上游同步收口）

上游同步已于 2026-08-20 经 PR #20 合入 main（merge 0f941e5b）并随 v0.1.29 发布；
本文件自那以后作为 seal 推进台账延续，「Seal 推进记录」一节是现行工作流。

## 审计坐标（固定，执行期间从未移动）

```
UPSTREAM_BASE_SHA     = cc19cb49b0786d61ed723764e0a83baf87887270  (openhanako v0.444.1)
UPSTREAM_TARGET_SHA   = c6d0405294be67cb134c2758f6472748ee73e2be  (openhanako v0.447.4)
LINGXI_BASE_SHA       = 97595264ead8735a04559507ddaade25db8a4e15  (v0.444.1 同步完成点, PR #2)
LINGXI_START_SHA      = ca0b417e36a6a1f80947458aaed328a25718e41b  (main HEAD @ 2026-08-20)
VERIFIED_SOURCE_SHA   = 6bb5878f3a6f5b43be565218e117d4e9b0a4066e  (最终验证所针对的 feature commit（其 tree 即被验证源码树）；2026-08-30 同上 + 拆解系统优化 P0+P1+P2 全量（职责收缩/扩展并行/Query Family/候选总预算/Adaptive Specialist/Gap Analyzer/否定 exclusion）)
工作分支              = feature/upstream-sync-0.447.4
```

`VERIFIED_SOURCE_SHA` 是最终 typecheck、lint、tests、build、package 所验证的源码树；
它不是 commit 内容的一部分，因而不存在 Git 自引用（`SHA = hash(contents)`）。

### Post-verification audit seal

`VERIFIED_SOURCE_SHA` 之后只允许审计材料变更；生产代码、测试逻辑、runtime
generated artifacts 不允许改变。当前 branch HEAD 由 Git ref 自身标识
（`git rev-parse HEAD`），不写入自身 commit 内容。完整性由
`.sync-audit/verify-post-verification-diff.mjs`（`git diff --name-only
VERIFIED_SOURCE_SHA..HEAD` 仅允许审计 allowlist）与
`tests/post-verification-audit-seal.test.ts` 机器门禁保证。

ΔU = 18 commits / 133 paths（7850+/738-）；ΔL = 346 paths；overlap = 29 paths。
原始数据：`.sync-audit/delta-U-final.txt`（重算并与旧 delta-U.txt 逐字节一致）、
`delta-L.txt`、`overlap-paths.txt`、`per-commit-paths.txt`。

## 收口轮执行顺序（2026-08-20 第二轮）

按任务书要求的顺序执行，全部完成：

```
code fixes（f3a1525a persona degraded fallback / 382907c4 dream config 契约 / 0271c4e9 dream diff）
  ↓
matrix rebuilt（40525fd4：机器真相源 JSON + 生成投影 + 审计测试，29+96+4+4=133）
  ↓
generated artifacts regenerated（12d87d44：closure/fingerprint/inventory；二次生成零漂移）
  ↓
upstreamVersion already 0.447.4（345d6b54 已写入，本轮未回退）
  ↓
final typecheck → final lint → final boundary lint（def7ec74 修齐 lint/tsc 后全绿）
  ↓
final targeted tests（26 文件 355 用例）→ final full npm test（1137 文件 11551 用例 0 failed）
  ↓
final builds（build:server / build:server:open / build:client 全绿）
  ↓
final package（npm run pack exit 0 → dist/mac-arm64/Lingxi.app + 产物抽检）
  ↓
VERIFIED_SOURCE_SHA（本文件与 UPSTREAM_SYNC_AUDIT.md / UPSTREAM_SYNC_MATRIX.md 记录同一 40 位 SHA）
```

## 收口轮修复清单（第一轮遗留的 4 个核心问题）

| 项 | 内容 | 状态 | 证据 |
|---|---|---|---|
| P1-A | 133-path 审计矩阵重建 | ✅ | 40525fd4；`.sync-audit/upstream-sync-matrix.json` 机器真相源 + `build-sync-matrix.mjs` 生成投影 + tests/upstream-sync-matrix.test.ts 7 用例机器校验；旧矩阵 18+102+5+4=129 不闭合问题消除 |
| P1-B | AGENTS.md migration 失败时 Persona 运行时丢失 | ✅ | f3a1525a；失败记录结构化（`failedDetails` + `buildFailedPersonaRenameIndex`）→ engine 运行时状态 → Agent `_personaMigrationFallback` → `resolvePersonaSource(migrationFallback)`；新文件永远优先、无失败记录不读旧文件、下次启动成功后 fallback 自动消失；public 变体同规则；11 迁移用例 + smoke 23/23 |
| P2-A | Dream revision current-vs-revision diff | ✅ | 0271c4e9；detail 响应带后端现读 current 快照（`snapshotDreamSections`，不落文件路径）；UI 逐段统一 diff（复用 line-diff 工具）；确认前现取 current；相同则禁用恢复；恢复后刷新列表与对比；组件测试 A–F 全绿 |
| P2-B | upstreamVersion=0.447.4 最终源码树重跑构建/打包 | ✅ | 见下「已执行测试」6–9；renderer 产物 grep 到 0.447.4 |

## 第一轮已完成的 14 个同步提交

保留未动：4f5b2d00（基线）→ f781f10f / cb4647d1 / f32c3f64（persona）→
e715b8e4 / 9e2fa339 / 8f249913（dream）→ 27d14477 / ba9cb461（automation）→
fb032eea（markdown URL）→ 63bc92b7（context ring）→ 18727d24（windows seed）→
abbfb593（派生物）→ 345d6b54（upstreamVersion 0.447.4）。

## 已执行测试（最终源码树，全部指向 VERIFIED_SOURCE_SHA 对应树）

1. `npm run typecheck` — tsc×3（root + node + test）全绿。
2. `npm run lint` — **0 errors** / 8118 warnings（main 基线 8037；新增均为既有风格类 warning）。
   `.sync-audit/*.mjs` 纳入 node-globals lint 域（def7ec74）。
3. `npm run lint:boundary` — ok（1 条 known open→closed edge，ratchet 基线内既有债务）。
4. 定向测试 26 文件 **355 用例全绿**：persona migration / persona-source / workspace-exclude /
   agents-route / agent-config（dream 契约）/ dream 全链路 7 件套 / DreamRevisionBrowser /
   AgentMemory / cron-store / desk-route-cron / AutomationPanel / AssistantMessage suggestion /
   md-decorations / context-ring / windows-installer-contract / upstream-version-consistency /
   persistence-schema-tripwire / export-open-tree / server-composition-boundary /
   upstream-sync-matrix。
5. `npm test` 全量 — **1137 文件通过 | 1 skipped；11551 用例通过 | 7 skipped；0 failed**（66.8s）。
   （较第一轮 1136/11532：+1 文件 = upstream-sync-matrix 审计测试；+19 用例 = 矩阵 7 +
   persona degraded 5 + dream config 3 + dream diff 组件 4。）
6. `npm run build:server` + `npm run build:server:open` + `build:client` 全绿
   （签名用 /tmp 一次性 throwaway keypair，未入库；第一次后台运行因缺 LINGXI_SIGN_KEY 硬报错，
   补 keypair 后重跑通过——硬报错是设计行为，非静默降级）。
7. renderer 产物验证：`desktop/dist-renderer/assets/SettingsContent-*.js` grep 到 `0.447.4`
   （vite define 注入 lingxi.upstreamVersion 生效；构建发生在 0.447.4 写入之后）。
8. `npm run pack`（SKIP_NOTARIZE=true，electron-builder --dir）**exit 0** →
   `dist/mac-arm64/Lingxi.app` 产出；seed resources verified（renderer+server archive+manifest+sig）；
   ad-hoc resign verified。
9. 打包产物抽检：seed tarball 含 `lib/agents-templates/`、`lib/agents-public-templates/`
   （含 Lingxi 品牌 lingxi.md）；**无** ishiki-templates/public-ishiki-templates/ishiki.example.md
   残留；packed server bundle 含 Dream 后端（"Dream may not rewrite Today or Week"、
   pending-apply.json、dream/revisions）与全部 dream_* 稳定错误码、agents-md-rename 迁移步骤、
   getFailedPersonaRename 接线；`/ishiki`、`public-ishiki` 字符串仅存于合法 legacy API alias
   与迁移 rename 字面量（API/数据兼容，非旧协议复活）。
10. migration smoke（`.sync-audit/migration-smoke.mjs`）**23/23**——含本轮升级的语义断言：
    failed rename → effective persona 仍是用户自定义内容（fromTemplate=false）；
    无失败记录 → 旧文件不读；下次启动改名成功 → fallback 消失、内容不丢。
11. 派生物确定性：closure / inventory / fingerprint 生成器连跑两次，第二次 `git diff` 为零。
12. Windows installer contract（tests/windows-installer-contract.test.ts，19 用例）通过。
    CI 触发记录：见下「Windows CI」。

## Windows CI（任务书 §30 补强）

仓库已有 Windows runner 配置，本轮未新增 CI 定义（已存在即满足"合理增加"）：

- `.github/workflows/ci.yml`：matrix 含 `windows-latest`，跑全量 vitest（含
  windows-installer-contract 19 用例）；触发条件 push/PR → main。
- `.github/workflows/build.yml`：matrix 含 `windows-latest + nsis`，
  `npx electron-builder --win nsis --publish never` 构建真实安装器；
  触发条件 tag v* 或 workflow_dispatch。

收口触发记录：

- 第一次 dispatch（run 32329281129，headSha c3e9fbc5）在 release-preflight 硬失败：
  `release-preflight.mjs` 无 `--tag` 时回退到 `GITHUB_REF_NAME`（分支名），与 package
  version 必然不匹配——workflow 预存缺陷，分支 dispatch 从未可用。最小修复：非 tag ref
  显式跳过 release 门禁（tag 路径行为不变，3707a450，含注释说明；3 个 workflow 契约
  测试 22 用例本地验证通过）。
- 第二次 dispatch（run 32329515438，headSha 3707a45053517f20c36888e092dc6a4579471608）
  **全绿**：release-preflight / quality-gate（ubuntu 全量 npm test，含
  windows-installer-contract 19 用例与 upstream-sync-matrix 审计）/ renderer-box /
  build×4 / artifact-release-smoke 全部 success；release、publish-train、
  mirror-atomgit 按设计 tag-gated 跳过。
- Windows leg（windows-latest, nsis, x64）关键步骤全部 success：Download/Smoke-test
  MinGit、Materialize seed signing key、Build server bundle、Build Windows sandbox
  helper、Build Windows standalone server archive、Verify Windows standalone server
  archive（--smoke，package smoke）、Verify seed kit (Windows)、Build Windows
  installer（`electron-builder --win nsis --publish never`）。
- 证据链接：https://github.com/ItsDalk-Lane/LingxiAgent/actions/runs/32329515438
- CI headSha 3707a450 与 VERIFIED_SOURCE_SHA 之间仅有本文件记录与 VERIFIED_SOURCE_SHA
  标注（docs-only），代码/测试/构建输入零差异
  （`git diff 3707a450..VERIFIED_SOURCE_SHA --stat` 可核）。

真实 Windows 安装器执行（在本机运行 NSIS）受宿主平台限制未进行——不伪造"真机通过"。

## 关于 upstreamVersion 的更正（任务书 §36）

~~旧说法："upstreamVersion 是纯元数据，所以 bump 后无需重跑 build"~~ —— 更正为：

`lingxi.upstreamVersion` 不改变业务逻辑，但属于 renderer build-time metadata
（package.json → vite.config.ts define → import.meta.env.LINGXI_UPSTREAM_VERSION →
AboutTab），因此最终源码树重新执行了 renderer build / package smoke（本文件第 6–9 项），
产物中已验证 0.447.4 注入。

## 第一轮发现与处置（保留）

1. 上游 package.json 在 U0..U1 区间仅有 version 字段变化，Dream 无新增运行时依赖 →
   package.json/lock 整体 INTENTIONAL_DIVERGENCE；Pi SDK 保持 0.84.1 未降级。
2. Lingxi 人格体系原 ishiki 命名整体迁往 AGENTS.md 协议；模板 lingxi.md 与上游 hanako.md
   内容同步，但产品品牌路径不同（hanako.md → lingxi.md），故矩阵中 4 个 hanako.md
   路径分类为 ADAPTED（品牌级路径映射），非 ADOPTED。
3. Dream 绑定 Lingxi 辅助模型槽（memory slot），tests/memory-dream-memory-slot.test.ts 锁定，
   未复活 utility 架构；本轮 diff 功能未触碰该链路。
4. overlap 29 路径中 C 类最高风险 AssistantMessage.tsx 以干净三方合并落地，automation
   suggestion 失败走 ContentBlock 架构。
5. cron-store / memory-ticker 上游 patch 基线一致性好；Lingxi 扩展（configRevision/
   storeRevision）保全。
6. 派生物两轮重新生成（abbfb593、12d87d44）；persistence fingerprint 两轮 compatible
   review（persona 改名 + dream additive stores；agent-manager 回调接线），DATA_EPOCH=1 不变。

## Seal 推进记录

seal 不是一次性终点，而是"当前被验证树"的游标；每次审计期后的结构性收尾都需复跑验证并推进：

- **2026-08-20 收口**（d4cf92a8）：全部最终验证（typecheck/lint/测试/构建/打包/CI）针对的树。
- **2026-08-20 文档清场**（6e28d74e4717ee36631bd9e3384c57cc1ced4487）：删除三份已完成的历史
  流水文档（findings.md / task_plan.md / chat_rendering_progress.md，事件结论已沉淀在
  INCIDENT_REPORT.md 并随之归档至 gitignored `archived/`），移除临时 worktree
  /tmp/lingxi-main-lint。复跑 typecheck（绿）+ 全量 npm test（11553 passed，唯一失败为
  seal allowlist 预期红）后推进。生产代码、测试逻辑、runtime artifacts 零变化
  （`git diff d4cf92a8..6e28d74e --stat` 仅 6 个 .md 删除）。
- **2026-08-20 归档修复**（051f6117c10846dbb244e0dd0fb86004ba0e7e66）：合并后首个正常开发
  提交（归档标题回退 + 手动批量删除，14 files / +327-7，含 persistence fingerprint
  compatible repin）。复跑 typecheck x3（绿）+ eslint（0 error）+ 全量 npm test
  （11560 passed，唯一失败为 seal allowlist 预期红）后推进。
- **2026-08-20 v0.1.29 release**（fabd6dbf86fb5234fff319918ca164eea68548b1）：release 元数据
  提交（package.json/package-lock 版本 0.1.29 + releaseGeneration 7，release-digest.v1/v2
  v0.1.29 条目，release-preflight 活体测试随版本推进；5 files / +385-119，数据-only，
  零生产代码变化）。验证：typecheck x3（绿）+ validate-release-digest（v1/v2 均过）+
  release-preflight --tag v0.1.29（PASS）+ 目标套件 8 文件 64 tests（release-digest-schema /
  validate-release-digest / update-digest-history / generate-release-digest /
  release-preflight / release-workflow-gates / post-verification-audit-seal /
  upstream-sync-matrix 全绿）后推进。
- **2026-08-20 mac self-install 功能**（dcf3546adb3c41bbc32d4c2fd2899e3f28f47566）：mac
  ad-hoc 签名构建的自动更新自安装路径（新增 desktop/mac-self-install.cjs + 对应测试，
  auto-updater 集成与测试；5 files / +796-7）。验证：PR CI typecheck/lint/build 全绿 +
  全量测试 macos 1138 passed（旧坐标 fabd6dbf 下仅 post-verification-audit-seal 预期红）；
  windows 首轮暴露 defaultPendingDir 用例硬编码 POSIX 路径断言失败，改为 path.join 构造
  期望值（本地 52 tests 绿 + win32 路径语义一致）后推进。
- **2026-08-21 凭证边界修复**（b868889569fb4dbeef5deb7dcfee6c78fa9ac32e）：全仓模型调用与
  凭证边界断点修复（P0/P1/P2 12 风险项；53 files / +1692-829，含删除退役
  core/execution-router.ts、新增 temporary-provider-credential-boundary 与
  model-request-accounting、6 个新边界测试；审计/过程文档未入库）。验证：本地 typecheck×3
  （绿）+ 新增 6 个边界测试 13 用例全绿 + 改动涉及 16 个核心测试文件 414 用例全绿 +
  PR CI macos/windows 全量测试仅 post-verification-audit-seal 预期红（旧坐标 dcf3546a 下，
  11599+ 用例绿）后推进。
- **2026-08-21 保留标签管道修复**（c83d238ad6bede9c169a5be66ab74cd4cdde0eb4）：聊天历史
  保留标签管道 raw 单次消费与段净化边界（15 files / +1073-68，含净化后空 text 段删除、
  text_end 不制造假 final_answer 段、reservedProcessedTextKeys 三入口同关、providers
  summary 媒体绑定一次性计算）。验证：本地 typecheck×3（绿）+ 新增/改动测试 125 用例全绿 +
  全量测试 11625 用例绿（旧坐标 b8688895 下仅 post-verification-audit-seal 预期红；
  release-preflight 本地偶发超时、单独重跑 6/6 绿）后推进。
- **2026-08-21 保留标签指纹补钉**（be95b34412ea2636a983a1cb681239ddcdfb59ee）：CI
  persistence-schema-guard 要求受护源文件被 touch 时同次重钉指纹；providers.ts（受护源）
  在 c83d238a 被改，本次以 compatible 分类补钉 build/persistence-schema-fingerprint.json
  （review 记录更新，schema 形状不变）。验证：tripwire 15 用例绿 + guard 前哨通过后推进。
- **2026-08-21 模型调用可观测性**（三轮：a9a5f3f4 → b9238533 → 53fa4575，
  第三轮 = e25079a2 功能树 + 53fa4575 strict-typecheck 收尾树）：
  Model Call Observer 全量实现（第一轮契约+文本运行时 a9a5f3f4；第二轮 MC-05～09 接入+
  安全收口+控制面分离 b9238533；第三轮 Phase 3.5 残余旁路闭合 + Phase 4 全局 Trace 传播
  e25079a2：MC-10 diary direct summary 接入、ModelTraceScope/统一身份解析/ingress 接线/
  工具因果边界/MC-01 WeakMap ledger 关联，闭包差量 MODEL_CALL_CLOSURE_DELTA.md，export
  manifest 收录 3 新模块，persistence 指纹 compatible repin）。第三轮验证：本地 typecheck×3
  （e25079a2 树上的新测试文件存在 6 处 tsc 类型缺陷，53fa4575 收尾修复——与第二轮
  a9a5f3f4 同类收尾）+ eslint 0 error + lint:boundary/闭包清单 + 新增 3 测试文件 35 用例 +
  前两轮 96 用例回归全绿 + 全量测试 11741/11740 用例绿（两轮旧坐标下唯一失败均为
  post-verification-audit-seal 预期红）后推进。

- **2026-08-22 模型调用可观测性第四、五轮（provenance + payload capture）**（第四轮
  功能树 3cf0e6ed/seal ea909c6e；第五轮 bfde47bcc6617751e19b94b138ee23a3fcd0d946）：第四轮 Phase 5 Semantic Input
  Provenance（统一契约 + MC-01～10 全路径 provenance sidecar，全量 11776 通过）；
  第五轮 Phase 6 Sensitive Payload Capture——四层级正文通道（Semantic/Provider
  Request/Response × MC-01～10）+ Redaction Contract（credential 键 / Volcengine
  body.user.uid 协议专项 / inline secret 正反例 / URL / 本地路径 / 二进制
  externalization / span offset remap）+ Provider-Wire Provenance（callText 四协议
  构造时 mapping + post-compat 校验降级）。38 files / +5302-121（契约五模块 +
  全路径集成 + 7 测试文件 103 用例 + 审计/进度文档；export-manifest 收录 5 新
  模块；cli-runtime-closure 重 pin）。第五轮验证：typecheck ×3（绿）+ eslint
  0 error + lint:boundary 绿 + 既有观测 131 用例回归 + full npm test 11881 通过；
  seal/matrix/tripwire/boundary 推进后复验。

- **2026-08-22 模型调用可观测性第六轮（durable storage）**（功能树
  bfde47bc/seal 本提交）：Phase 7 Durable Model Observatory Storage——单
  SQLite（user_version=1）Trace/Payload Store + 外置 Blob Store + privileged
  Blob Externalizer contract + bounded 异步 coordinator + retention/GC（payload
  可先过期）+ crash reconciliation（不伪造终态）+ engine/server 生产 wiring
  （默认 disabled）。Store Registry ×2 登记 + fingerprint introspector +
  compatible repin；毒丸 DB+wal+shm 字节级扫描零命中；新增 6 测试文件 44 用例；
  第六轮验证：typecheck ×3 / eslint 0 error / lint:boundary / scanner 61 stores /
  data-epoch 80 / 既有观测 302 回归 / full npm test 11925 全绿；seal/matrix/
  tripwire 推进后复验。

- **2026-08-22 模型调用可观测性第七轮（unified query & control plane）**（功能树
  cfab8556/seal 本提交）：Phase 8 Unified Observatory Query & Control Plane——
  统一 Query Contract（Filters + Group By + Drill Down，category≡subsystem）+
  keyset pagination（cursor 与 query fingerprint 绑定）+ SQLite 内聚合（date
  bucket 显式 utcOffsetMinutes）+ schema v2（model_call_usage durable accounting
  projection：llm_usage live ingestion + bounded ledger 幂等 backfill，
  error.message 不入库）+ read-only query side（v1 历史库不迁移可读）+ trace
  explorer 后端（cycle-safe）+ payload exact retrieval（OPAQUE/UNAVAILABLE 不
  升级）+ observability settings 持久化 preference（默认 disabled，payload/blob
  额外 opt-in，runtime reconfigure 不删历史）+ HTTP surface（route-security
  显式登记：metadata=STUDIO_OWNER，正文/settings/export=LOCAL_ONLY）+ JSONL
  streaming export。第七轮验证：typecheck ×3 / eslint 0 error / lint:boundary /
  scanner 站点登记 + fingerprint compatible repin（sha256:b0712be2…）/
  新增 7 测试文件 53 用例 / full npm test 11975 全绿；seal/matrix/tripwire
  推进后复验。
- **2026-08-22 模型调用可观测性第八轮（Model Observatory UI）**（功能树
  61779cbd/seal 本提交；进度 OBSERVABILITY_UI_PROGRESS.md）：Phase 9 把 Phase 1–8 事实层
  变成用户工作台，替换旧 Usage 页——browser-safe wire 单一事实源
  shared/model-observability-api-contract.ts（renderer 不 import lib/llm）+
  独立 API client（error contract 全字段保留）+ FilterBar/Metrics/Groups/
  Ledger（cursor+stale 防护）+ Call Inspector（overview/attempts/payload
  管线）+ provenance locator-only 解析器 + provider mapping + TraceExplorer
  （buildTraceForest orphan/环/未覆盖防御）+ payload 卡四态正文 + 纯文本
  JsonViewer + blob 预览状态机 + Recording Settings（desired≠effective 诚实、
  blob⊆payload、opt-in 确认、无加密事实文案）+ onboarding 安全默认 + 导出
  双通道流式保存（IPC 桥 abort 删部分文件；FSA partialLeft 如实标注）。
  Backend 白名单增量：getStoredBlob（路径从 blobId 重算，不信任 DB
  relative_path）+ blobs exact route（GET/HEAD LOCAL_ONLY + 安全
  content-type）。Legacy 退休：8 文件 + 650 行 CSS + settings.usage.* 五语言
  删除；内部 tab id `usage` 不变、可见名五语言升级为模型观测/Model
  Observatory。i18n settings.observability.* 完整子树（含 values 23 组闭集
  矩阵）+ parity 绿。第八轮验证：typecheck ×3 / eslint 0 新增 error /
  lint:boundary（closure 重生成后）/ persistence 豁免
  desktop-observability-export-output + scanner receipt 重生成 + fingerprint
  compatible repin（sha256:15591e09…）/ 新增 10 测试文件 83 用例 /
  full npm test 12052 全绿；seal/matrix/tripwire 推进后复验。


- **2026-08-22 模型调用可观测性第十~十一轮（Phase 10.1 修复 + Phase 11 合并收口）**
  （功能树 8c94044b/seal 本提交；中间链 81cdb2d8 清场 → da1a66c1 首次推进 →
  b9933da8 allowlist 拼写修复 → 9f494205 Windows 测试修复① → dc1fd8bb 二次
  推进 → 8c94044b Windows 测试修复②（vertical 读连接泄漏））：Phase 10.1 从 codex worktree 抢救并入
  （dba9a6b1+merge 4f95e17d）：AR-01～AR-20（P1×18 + P2×2）全 FIXED——IANA
  timezone/DST 步进扫描、同字段 OR 跨字段 AND、NULL/unknown/corrupt/not_correlated
  语义、聚合三态、Trace 全链统计、partial payload dropped、Blob 两阶段写+retry
  不重写文件+无悬空引用+流式 GET、动态重配代际（四类在途切换+有界排空）、
  schema v3（usage_correlation_state 真实列，v1/v2→v3 单事务迁移+rollback+future）。
  Phase 11（81cdb2d8）：仅删除 codex scratchpad（task_plan.md/findings.md，
  对齐 2026-08-20 文档清场先例），零生产代码变化；egress 独立重扫 MC-01～10
  全 OBSERVED、无 MC-11+，plugin network.fetch 开口登记（LATENT/ARCHITECTURAL）。
  验证：typecheck ×3 / eslint 0 error / boundary 基线不变 / 三 generator 重生成
  零漂移（fingerprint sha256:f4cfa1e8…）/ full npm test 12134 passed（唯一失败
  = seal guard 预期红）/ build:server（临时密钥，构建后删除）/ build:server:open /
  build:client / pack 冒烟（ad-hoc 签名验证）。远端：81cdb2d8 轮 4/5 job 绿、mac test 仅 seal guard 红（分类 E）、
  win fail-fast 取消；b9933da8 轮 macOS 全绿（guard 转绿）+ Windows 首次
  完整执行暴露 2 处测试层跨平台缺陷（10k 播种超时 / vertical rmSync EPERM，
  均非生产逻辑，此前各轮被 fail-fast 掩盖）；9f494205 修复①（超时预算+rmSync 重试）后 dc1fd8bb 二次推进，Windows 证明
  重试无效 → 真根因是 vertical 测试 query service 只读连接泄漏（macOS 允许
  删打开文件故本地不可见），8c94044b 显式关闭后第三次重走。首次推进（da1a66c1）因 seal allowlist 三处
  OBSERVATORY/OBSERVABILITY 拼写错位失效（含一处 cda8dbe5 起的死条目），
  b9933da8 逐字符修复。最终 seal commit 轮（PR HEAD）目标全绿。Release Acceptance V3 见
  MODEL_OBSERVATORY_RELEASE_ACCEPTANCE_V3.md；V2 保留历史。

- **2026-08-23 图标/logo 全量 rebrand 链**（seal 8c94044b → 440e9f57，五次推进：
  86a029a7 → cc697956 → 3b347a4f → aac8c605 → 440e9f57）：13 文件素材替换
  （4.png 图标源 / 5.png logo 源 + tray + windows icon 重生成），各轮 chore(audit)
  提交推进坐标；期间 3113d337 清理过时进度文档并同步审计清单。台账条目本轮补记。

- **2026-08-24 观测台时间线折线图迭代 + v0.1.30 发布前收口**（功能树
  30632983/seal 本提交；一并覆盖 440e9f57 之后的 0561c1ce 时间线图表与详情视图）：
  date 分组柱状图升级为按真实日期比例定位的平滑折线（Catmull-Rom 贝塞尔 + 面积渐变
  + non-scaling stroke + 贪心抽稀 X 轴标签 + hover tooltip，新增
  ObservabilityDateLine 几何回归测试）；相对日期预设锚点 presetAnchorMs（since 冻结，
  分页游标 filter 指纹不再随请求时刻漂移）；调用抽屉 scrim 自层接管外点击关闭 +
  轨迹↔调用交叉跳转对称收起；DateLine hooks 顺序修复；style ratchet 收口
  （trace-dot 裸时长 → var(--duration-instant)、tooltip 阴影 → 新 token
  --chart-tooltip-shadow，含 0561c1ce 遗留 +1 bare-duration 一并修复）；i18n 五语言
  datePointTooltip。验证：typecheck ×3（绿）+ eslint 改动文件 0 error + observability
  定向 14 文件 100 用例 + style-discipline 8 用例 + full npm test 12142 passed
  （唯一失败 = 推进前 seal guard 预期红）后推进。

- **2026-08-24 v0.1.30 release**（7010fb06，各坐标以文首为准）：release 元数据提交
  （package.json/package-lock 版本 0.1.30 + releaseGeneration 8，release-digest.v1
  v0.1.30 手写条目 + v2 --append-history prepend（11 entries），release-preflight
  活体断言随版本推进；5 files / +323-127，数据-only，零生产代码变化）。验证：
  typecheck ×3（绿）+ validate-release-digest v1/v2 + release-preflight --tag v0.1.30
  （PASS，historicalMaximum 0.1.29/7）+ test:artifact-release-smoke 8 文件 304 用例 +
  digest/workflow/seal/matrix 目标套件 7 文件 59 用例全绿后推进。

- **2026-08-24 Linux /tmp redaction 修复**（e76918ef，v0.1.30 tag 重打前插入）：
  v0.1.30 build run 32729830490 的 Linux quality-gate 首跑暴露 redaction 本地路径
  正则盲区——FULL/INLINE_LOCAL_PATH 不认 /tmp、/var/tmp（macOS /private/var/folders
  与 Windows 盘符恰好被覆盖），Linux TMPDIR 下 blob 外置路径泄漏进 payload 正文、
  audio 整串不转 descriptor（payload-media/payload-speech/e2e-media-speech 三文件
  七用例）。补 POSIX 临时目录分支 + 回归锁定用例。验证：定向 4 文件 77 用例绿 +
  persistence guard OK（redaction.ts 非受护源）+ typecheck ×3 + eslint 0 error 后推进。

- **2026-08-25 聊天界面布局重构 + 上下文用量分类详情**（69d69ca8，v0.1.31
  release 分支首提交）：工作台主体左移 + 聊天记录限定当前工作台 + 运行期卡片
  整合为右上悬浮信息胶囊（RuntimeInfoCapsule）+ 聊天搜索改顶部入口与居中
  搜索界面（ChatSearchOverlay / use-session-search）+ 输入区控制按钮重排为
  ComposerToolbar（退役 InputControlBar）+ 侧栏入口与标题栏随新布局重排；
  context-usage-breakdown 分类估值契约（lib/llm + shared 单一事实源）+
  Context Ring 真实分类明细 + contextUsageEstimate 随请求边界去抖落盘
  session-meta、entry 重建恢复、压缩置空；设置/关于页 Toggle 卡 loading
  修复（initSettings 并行化 agentId 竞态，snapshot/avatars 串行于 agents 后）。
  76 files / +5807-3318；export-manifest 收录 2 新模块、cli-runtime-closure
  重 pin、persistence fingerprint compatible repin（sha256:ab819c64…）。
  验证（F 树，提交前工作区即该 tree）：typecheck×3（绿）+ eslint 0 error +
  full npm test 12180 passed / 0 failed（推进前 seal guard 旧坐标下工作区
  未提交态不触发 diff）后推进。

- **2026-08-25 v0.1.31 release**（87249ede，各坐标以文首为准）：release 元数据
  提交（package.json/package-lock 版本 0.1.31 + releaseGeneration 9，
  release-digest.v1 v0.1.31 手写条目（布局重构/上下文分类明细/Toggle 卡
  loading 修复）+ v2 --append-history prepend（12 entries），release-preflight
  活体断言随版本推进；5 files / +150-164，数据-only，零生产代码变化）。
  验证：typecheck×3（绿）+ validate-release-digest v1/v2 + release-preflight
  --tag v0.1.31（PASS，historicalMaximum 0.1.30/8）+ 目标套件 8 文件 65
  用例（release-preflight / release-digest-schema / validate-release-digest /
  update-digest-history / generate-release-digest / release-workflow-gates /
  post-verification-audit-seal / upstream-sync-matrix 全绿）后推进。

- **2026-08-28 knowledge-notebook + provider-compat + 四平台 CI 修复**（功能链
  1f3e1ea1（knowledge-notebook：知识存储/导入/检索/研究/引用、KnowledgePage UI、
  server 路由与测试，97 files +21k，Store Registry knowledge-database 等登记）
  → da7fa01a（provider-compat：输出预算控制与模型解析，output-budget 扩展 +
  anthropic 兼容层适配 + CI 矩阵钉住 macos-15/macos-15-intel/windows-2025/
  ubuntu-24.04 精确 runner + knowledge/build 平台 smoke）
  → 2e7798ae（CI 修复，seal 本轮坐标））：PR #29 首轮 CI 四平台 test job 全红
  （各 5–7 失败），三因收口——① knowledge-query-service 两处
  /tmp/lingxi-embed-diag.log appendFileSync 调试残留为未登记持久化点
  （persistence 三测试全平台红），删除后 store-inventory 重生成（skill 删除点
  已随 da7fa01a 迁至 lib/skills/skill-removal.ts，committed inventory 仍指
  server/routes/skills.ts 旧路径，此前被未登记点先抛遮蔽）+ fingerprint
  compatible repin（sha256:6a120a12…）；② macos-15-intel 新 runner 上三个
  real esbuild+nft 慢测试超时（单轮 60–110s），预算 120s/180s→420s/600s；
  ③ session-manifest corrupt-manifest 清理仅关 manifest store，而本 PR 起
  LingxiEngine 构造即打开 knowledge 三库句柄（knowledge/knowledge-fts/
  knowledge-vector），Windows rmSync EPERM（macOS 可删已打开文件故本地不可见，
  与 Phase 10.1 vertical 泄漏同型），改 engine.dispose() 统一收口 + rmSync
  maxRetries 兜底。验证（2e7798ae 树）：typecheck×3（绿）+ 目标回归
  （persistence-store-registry / persistence-startup-receipt /
  persistence-schema-tripwire / session-manifest-engine / cli-closure-census /
  knowledge-query 全绿）+ full npm test 12324 passed / 0 failed（推进前 seal guard
  旧坐标预期红；另 artifact-core-ustar afterEach 本地并行清理 ENOTEMPTY 一次，
  单跑 10/10 绿且四平台 CI 从未红，判本地环境 flake）后推进。
  第二轮（52d29b3c，seal 终坐标）：上轮 CI arm64/ubuntu/windows 全绿，唯
  macos-15-intel 满载下 persistence-store-registry 双扫描测试（generates
  deterministic / anchors by ordinal）打穿 vitest 默认 10s（单扫描测试擦线过：
  单次全仓扫描在该 runner 逼近 10s，两次必超，算术非抖动，重试不可解）。
  修法与 cli-closure 同款：全仓扫描测试显式超时预算（单扫描 60s ×4 处、
  双扫描 120s ×2 处，断言零变化）。验证：两文件 17 用例绿 + typecheck×3
  绿后二次推进。
  第三轮（236109df，seal 终坐标）：上轮 CI arm64/ubuntu/windows 全绿，
  macos-15-intel 换 model-observability-blob size cap（64MB+1 Uint8Array
  分配）打穿 10s——边缘型重负载单测逐个冒头是打地鼠，系统性收口：
  vitest.config 全局 testTimeout/hookTimeout 10s→60s（6 倍余量；更慢
  场景仍由显式预算覆盖）。验证：blob 套件绿 + typecheck×3 绿后三次推进。
  第四轮（7de8ed8e，seal 终坐标）：上轮 CI 唯一失败 macos-15-intel
  markdown-blocks「collects complete direct syntax-tree children」断言只收
  到首块——根因是 CodeMirror 增量解析按时间预算推进，syntaxTree() 可能
  返回只含首块的部分树（快机器预算内解析完故不可见，生产环境真实潜在
  bug 非纯测试问题）。修复：collectMarkdownBlocks 与 block-selection
  缓存键改 ensureSyntaxTree(doc.length, 1s) 同步补齐完整树；装饰/hover
  增量路径保持部分树语义不变。验证：editor 套件 80 用例绿 + typecheck×3
  绿后四次推进。

- **2026-08-29 知识问答链路四连修 + 蒸馏并行化与进度可视化**
  （功能树 26906c83/seal 本提交；32 files / +1340-172）：分块配置同源
  （resolveEffectiveChunkTargetChars 贯穿查询 ensure 链/卡片视图/
  knowledge_read，消除查询侧 1200 重建与摄入指纹互打架的全量重嵌事故；
  ensureVectorArtifacts 在途去重）；rerank 按笔记本引用路由（v8 全局槽退役
  断链补齐，配置不可解析显式降级 RRF 留痕，报错包装带 cause，重排上限提为
  shared MODEL_OPERATION_RERANK_MAX_DOCS=100 单一真理源）；蒸馏并行化
  （批预算按实测吞吐 EMA 动态推算 10s 目标、惰性建批，32 路自适应并发池
  限流/超时逐层减半，超时线性化，sections 按批序整合）；蒸馏逐批进度事件
  knowledge_distill_progress → 聊天「蒸馏中 · N 批」胶囊（独立于流式态）+
  发送即本地「知识库检索中」；用户消息知识库元信息行精简为来源与模式。
  生成产物：cli-runtime-closure 重生成、持久化指纹 compatible repin。
  验证：typecheck×3（绿）+ eslint 0 error + full npm test 12523 passed /
  0 failed 后推进。

- **2026-08-29 knowledge 引用链路 Phase 7/8 + 管线重构 + 多项会话修复**
  （功能树 93f05a3c/seal 本提交；113 files / +12996-9097）：消息级笔记本
  引用主链路——shared/knowledge-refs 契约（notebookIds + qa/assist，会话内
  持续生效、服务端无状态透传）+ knowledge-context-injector（拆解+检索+注入
  块生成，失败显式留痕禁静默降级）+ knowledge_read 工具（超预算时子 Agent
  并行读分片，只读 + studio 隔离）+ knowledge-distiller（证据超注入预算分批
  提炼，两次失败整体判失败退回分片清单并留痕）；前端 KnowledgeReferenceBar/
  Button、KnowledgeRetrievalFold、knowledge-reference-slice。管线重构：research
  管线退役（4 模块+2 测试删除），导入改走 ingestion-service，新增
  source-file-watcher / knowledge-history-compressor / chunker 自动策略。
  会话修复：知识问答空白时延（turnPending 本地即亮+拆解/直检并行）、技能
  胶囊重载退化（[Use skill:] 三处对称解析）、正文段结构性降级（textSignature
  全默认 final_answer，turn-projector 三路径统一）、辅助槽 fresh 凭证形状
  （snake/camel 错配）。生成产物重 pin：closure/fingerprint（compatible）/
  inventory/export-manifest。验证：typecheck×3（绿）+ eslint 0 error +
  full npm test 12504 passed / 0 failed（推进前 seal guard 旧坐标预期红）后推进。

- **2026-08-30 knowledge coverage 证据链 Phase 9/10 + Agent 三工具 Phase 11
  + ProcessingArtifact 管线 Phase 12 + EvidenceManifest 持久化**
  （功能树 7b240ce1/seal 本提交；80 files / +23534-844）：Phase 9 coverage
  真执行两波——落库层 knowledge-coverage-{unit,manifest,planner,executor}.ts
  （契约/Planner/Executor/Ledger/Gate/Evidence 聚合，schema v13→v14
  coverage_runs/coverage_shards + loadResumableCoverageRun 恢复语义）+ 会话
  接线（injector exhaustive Planner→manifest→execute→aggregate 证据注入 +
  降格留痕、engine coverage worker 闭包、submit 检索期 abort、WS
  knowledge_coverage_progress、broad→exhaustive 自动升级、run 总时长上限）；
  Phase 10 层级归约 knowledge-coverage-reduction.ts（ev_ 稳定 id→Source→
  Notebook→Cross-Notebook，级预算内零 LLM 调用，结构化 JSON I/O + DP 分解
  校验，纠错一次后仍失败降级保序截断；injector 三岔口替换删伪 chunk distill
  过渡路径，stats coverageReduction）；Phase 11 三工具 knowledge_outline/
  knowledge_grep/knowledge_manage（scope 校验链抽 lib/tools/knowledge-scope.ts
  共享，STANDARD 分类 + 审批 review 档 + 五语言文案）；§六十七
  EvidenceManifest 轻量持久化（schema v14→v15 身份链表、injector evidence
  第三字段、engine 按 TurnScope 冻结集合复核、GC/deleteSource 引用保护）；
  Phase 12 ProcessingArtifact 管线 + 目录导入（source-processors.ts DOCX/
  XLSX/CSV 块级行级 + 防护上限、schema v15→v16、importDirectory sha 去重
  三组明细、import-directory 路由 local-owner 限定、桌面端入口 + 5 语言
  locale、不支持格式显式拒绝）；查询链路配套（TurnScope 冻结语义、rerank
  按引用路由 + 跨笔记本 RRF 融合只消名次、chunk profile/index variant 分离、
  生命周期与 GC 保护）。生成产物：export-manifest 收录新模块、
  cli-runtime-closure 重生成、persistence 指纹/inventory/receipt compatible
  repin、store-registry 登记。验证：typecheck×3（绿）+ eslint 0 error +
  full npm test 12746 passed / 0 failed 后推进。

- **2026-08-30 knowledge-watch 条件驱动假时钟 + win32 retry=2（四连修之四）**
  （功能树 33473a8f/seal 本提交；2 files / +20-17）：上轮 5 轮循环仍有
  洞——防抖计时器在轮询 stat 完成那一刻才建出（假时钟停在当时值），stat
  晚于全部固定 settle 轮数完成时（Linux 满载轮次即此）计时器停在「未来
  的假时钟」永不触发；改 waitForWithClock「settle → 推进防抖窗 → 查条件」
  循环直至条件满足（上限 180s 留在 300s 轮询窗内），两处轮询检出点改用。
  auth-storage recovers 在 retry=1 下仍复发（第 2 个红周期），win32 retry
  提至 2；若再挂达连续 3 周期约束即停改上报。验证：typecheck×3 绿 +
  full npm test 12556 passed / 0 failed（封印推进后复跑）后推进。

- **2026-08-30 knowledge-watch 轮询测试慢 runner 修复（Windows CI 三连修之三）**
  （功能树 7cddd563/seal 本提交；1 file / +12-4）：兜底轮询检出为假时钟
  推进 + 真实 fs stat，原固定 settleIo(20) 轮后单次推进 DEBOUNCE_MS——
  满载 Windows runner（全程 774s，本机 10 倍）stat 晚完成则防抖计时器
  在推进后才建出，waitFor 的 setImmediate 循环不推假时钟，永不触发。
  改「settle → 推进防抖窗」循环 5 轮（5×1500ms 远不到下一个 300s 轮询
  点，无二次检出），恢复后检出同款脆弱点一并修。验证：typecheck×3 绿 +
  full npm test 12556 passed / 0 failed（封印推进后复跑）后推进。

- **2026-08-30 Windows CI 稳定性两连修（PR #30 监控修复）**
  （功能树 91d66f93/seal 本提交；2 files / +10-1）：①knowledge-store 测试
  migrateLegacyGlobalModelRefs 泄漏 SQLite 句柄未 close 就删临时目录，
  Windows rmSync 报 EPERM（macOS/Linux 删打开文件合法故本机全绿）——补
  store.close() + afterEach rmSync 退避重试；②model-manager-auth-storage
  同代码结果漂移（main 8/28 挂 2 例/PR 一轮全过/三轮挂 1 例，SDK getAuth
  无解析走兼容分支 ok:true+apiKey undefined），抢救/投影/凭证链路逐层排查
  均为同步确定性逻辑，结合 main 近 8 轮 Windows 矩阵挂 4 轮且失败文件各异，
  定性 runner 环境抖动——vitest test.retry 仅 win32=1（与 ci.yml npm ci
  的 nick-fields/retry 同一处置逻辑），macOS/Linux 保持 0。两修各验：
  typecheck×3 绿 + eslint 改动文件 0 error + full npm test 12556 passed /
  0 failed（封印推进后复跑）后推进。


- **2026-08-30 build 修复 underscore 钉入 server 外置传递依赖**
  （功能树 0a45563a/seal 本提交；1 file / +6-1）：PR #31 CI open-build-smoke
  红——外置包 mammoth 的传递依赖 underscore 被 nft 按 ["node","import"]
  条件追踪，运行时 CJS require 走其 exports require.node 分支
  （underscore-node.cjs），分支文件集不同致剪枝后产物缺文件，服务器入口
  import 即崩。沿用 lru-cache 先例把 underscore 钉进 pinnedTransitiveDeps
  默认清单（完整安装 + 豁免剪枝），open/closed 两套构建共用一处生效。
  验证：本地 build:server:open + smoke:server:open 正反冒烟全过（protecting
  20→21）；build:server 钉依赖阶段确认入保护名单（签名步骤因本地无
  LINGXI_SIGN_KEY 停止为预期）；eslint 0 error + full npm test 12746
  passed / 0 failed 后推进。

- **2026-08-30 Windows CI 测试修复：knowledge-store 新测试补关库**
  （功能树 563478a0/seal 本提交；1 file / +2）：migrateLegacyGlobalModelRefs
  测试漏 store.close()，better-sqlite3 句柄悬空，Windows 上 afterEach
  fs.rmSync 删临时目录 EPERM（posix 允许删打开中文件故本地全绿）。对齐
  同文件关库纪律。验证：该文件 13/13 绿 + full npm test 12746 passed /
  0 failed 后推进。

- **2026-08-30 knowledge-watch 测试抖动修复：等待补 fake 时间泵**
  （功能树 2ffef293/seal 本提交；1 file / +23-2）：该文件 fake 计时器 +
  refresh 链真实 fs I/O，轮询 stat 晚于固定 settleIo 完成时防抖计时器迟建，
  原 waitFor 只泵真实事件循环致迟到计时器永不触发，CI ubuntu 实测超时
  （run 1 同代码通过、本地 5/5 绿，负载抖动非回归）。waitFor 增加
  fakeTimers 泵参数，防抖相关 7 处等待统一传 DEBOUNCE_MS。验证：该文件
  5/5 绿 + typecheck×3 + full npm test 12746 passed / 0 failed 后推进。

- **2026-08-30 分支合并 main（PR #30）+ 合并树 seal 推进**
  （合并树 dd508491/seal 本提交）：PR #30（模型操作原生协议 + 用户打标签 +
  knowledge v9 向量保留 + CI 稳定性修复）在 PR #31 CI 期间合入 main，产生
  13 文件冲突。合并语义：knowledge schema 两条线合一（本分支 v9-v16 保留，
  向量保留重编号 v17 additive + 跨线护栏，schema v17）；向量索引库以 variant
  架构为基座吸收 last_used_at（v3，v2 老库按表形分流迁移）；manager 孤儿
  处理并集（状态重算 + 即时索引回收 + TurnScope/manifest 保护闸）；
  index-store removeArtifact 去掉对已 DROP 的 artifact_indexes 的 DELETE
  （原样保留会事务回滚吞掉 FTS 清理）。指纹按合并树 compatible repin。
  验证：typecheck×3（绿）+ full npm test 12778 passed / 0 failed +
  build:server:open / smoke:server:open 正反冒烟全过后推进。

- **2026-08-30 Windows 轮询测试超时修复：waitForWithClock 补真实时间维度**
  （功能树 57d716e5/seal 本提交；1 file / +29-8）：轮询检出的 stat/refresh 链
  进度由 libuv 线程池队列决定（默认 4 线程全 worker 共享），Windows CI 实测
  一轮 refresh 需真实秒级；原 waitForWithClock 按迭代计数预算在几毫秒真实
  时间即耗尽。改双维度预算：Atomics.wait 真实阻塞 15ms/轮 + settle + 推进
  假时钟，600 轮 = 真实 ≥9s + 假时钟 150s 仍留轮询窗内；两处调用点挂调试
  转储。验证：该文件 6/6 绿 + typecheck×3 + full npm test 12778 passed /
  0 failed 后推进。

- **2026-08-30 知识提问延迟三连修复 + engine 构造顺序修复**
  （功能树 2d447aa4+01dfb168/seal 本提交；12 files / +452-22，含 persistence
  指纹 compatible repin）：桌面 dev 实测一次知识提问 1 分钟以上，observability
  逐调用记录定位三处结构性瓶颈——①分片装填只数正文不数渲染开销：行级小单元源
  （XLSX/CSV 一行一 block）provenance 头（unitId sha256/snapshot/parseArtifact/
  blockId/offsets）是正文 2–3 倍，300 单元挤一片渲染后 54k token（vs 16k 预算）
  → MiniMax-M3 分片 19 调用 0 成功全灭于线性化超时；②必败场景无熔断：按
  bounded retry + 30min run 上限最坏白烧半小时；③rerank 无期限：siliconflow
  VL-Reranker 单次 11–56s 排队方差，检索尾巴固定 ~66s，且传输类失败直接
  KNOWLEDGE_RETRIEVAL_UNAVAILABLE 炸掉整个检索。修复：planCoverageShards 成本
  = 正文 + coverageUnitPromptOverheadTokens；executor 熔断（零成功 + 终态
  failed ≥ COVERAGE_CIRCUIT_BREAK_FAILURES=4 提前取消剩余，新 reason code
  KNOWLEDGE_COVERAGE_CIRCUIT_BREAK，任一成功即豁免）；rerank 15s 期限竞速
  （KNOWLEDGE_RERANK_DEADLINE_MS）超时/传输失败降级保 RRF 名次 +
  rerankDegradeReason 留痕（注入块行 + stats）。附带 engine _models 先于
  _knowledge 构造（存量库迁移期闭包读 providerRegistry 崩溃循环，CI 新库
  测不到）。验证：typecheck×3 绿 + eslint 0 error + 新增 8 用例 + knowledge
  簇 282 用例 + full npm test 12785 passed / 0 failed（首轮 census 2 用例
  并行负载抖动、复跑与隔离复跑均全绿）后推进。

- **2026-08-30 exhaustive 交互式规模闸（延迟加固第二轮）**
  （功能树 11474e82/seal 本提交；2 files / +83-0）：第一轮修复实测后暴露
  更深一层问题——680 万 token 语料按开销计费正确分出 1073 片 exhaustive，
  交互式窗口本质装不下（2–3 小时）；MiniMax-M3 压 46s 超时线成败各半 →
  熔断被「任一成功即豁免」正确豁免 → 继续磨（用户按停时 4 分钟完成 5/1073
  片），coverage 期前端无进度渲染体感即卡死。修复：KNOWLEDGE_EXHAUSTIVE_
  MAX_SHARDS=24（≈40 万 token ≈ 一本书），runExhaustiveCoverage 计划期计数
  超阈即显式降格 broad + 留痕（两条入口统一收口：计划 exhaustive / §四十一
  自动升级），指引 knowledge_grep/outline + subagent 分治；阈值内语义不变。
  验证：typecheck×3 绿 + eslint 0 error + 新增 2 用例 + coverage 簇 110
  用例 + full npm test 12787 passed / 0 failed 后推进。

- **2026-08-30 证据锚点随注入预算伸缩 + 移除「检索数量」设置**
  （功能树 47fed9ef/seal 本提交；11 files / +134-106，含 cli-runtime-closure
  随 manifest→estimate-text-tokens 依赖边重生成）：Phase 8 固定 40 锚点在
  大上下文模型下只占预算一成（512k 窗口 → ~50 万 token 预算 vs 40 块 ≈ 5 万
  token）。resolveEvidenceAnchorBudget 按融合候选平均 token 伸缩（≤50% 预算、
  下限 40 兜底、上限 240 防碎屑）；同轮移除笔记本设置「检索数量」控件——
  候选预算链每层独立封顶后 retrievalTopK ≥60 无实际影响，保存原样回传存量
  值，五语言 locale 键同步移除。验证：typecheck×3 绿 + eslint 0 error +
  新增 7 用例 + knowledge 簇 195 用例 + full npm test 12791 passed / 0
  failed 后推进。

- **2026-08-30 嵌入/重排供应商协议兼容修复**（功能树 08ead330/seal 本提交；
  17 files / +428-40，含 persistence fingerprint compatible repin）：全部供应商
  插件的嵌入/重排调用链对各家官方文档逐家核验后的实锤三连修——①千问 rerank
  新增 dashscope-rerank 双端点方言（gte-rerank 系/qwen3-vl-rerank 系走原生
  嵌套端点 + output.results 归一化，qwen3-rerank 系走 compatible-api/v1/
  reranks 官方复数端点；旧实现改写后拼单数 /rerank 必 404），cohere-rerank
  协议在 compatible-mode base 上的改写后缀同步修正为复数；②MiniMax 嵌入新增
  minimax-embeddings 方言（/v1/embeddings?GroupId= + texts/type(db|query)
  请求体 + vectors 归一化 + HTTP 200 内嵌 base_resp 错误码显式抛错），groupId
  进 registry ALLOWED 白名单 + 模型编辑面板输入位 + 五语言文案，inputType
  穿透查询侧固定 query（官方 db/query 算法分离）、缺 GroupId 显式报错；
  ③rerank 文档上限 100→50（防御火山方舟 doubao-rerank 单次 50 上限，精度远
  小于该值已饱和），查询侧裁剪与客户端断言共用单一真理源。验证：typecheck×3
  绿 + full npm test 12799 passed / 0 failed + 指纹 compatible repin（engine.ts
  闭包透传不触及持久化形状）+ tripwire/census 门禁单跑绿后推进。

- **2026-08-30 融合池上限随预算倒推（阀 A）+ 检索列表二次展开**
  （功能树 4e6bfe87/seal 本提交；10 files / +177-17）：接续锚点伸缩（阀 B
  50%）补齐候选侧——resolveFusionPoolBudget = 预算 × 70% ÷ 候选平均 token
  （池是候选水位，略高于锚点配额留选择余量），下限 60（小预算既有召回水位）、
  上限 480（防碎片块碎屑化）；fuseSubQueryResults 可选 cap 参数（缺省 60
  向后兼容），编排/降格重算/render 统计三处同源接线；多子查询大预算端到端
  实测融合池 180 块全保留（旧行为 60 截断）。聊天检索列表二次展开：首屏
  10 条 +「显示更多（还有 N 条）」二级一次性放出（≤10 条无按钮），五语言
  knowledgeRetrievalShowMore。验证：typecheck×3 绿 + eslint 0 error + 新增
  5 用例（倒推公式三态含 1M 口径示例/端到端池 180/UI 双路）+ knowledge 簇
  182 用例 + full npm test 12805 passed / 0 failed 后推进。

- **2026-08-30 查询嵌入失败/期限降级（退 FTS 不炸检索不空等）**
  （功能树 d6be53ae/seal 本提交；3 files / +248-9）：与 rerank 期限降级对称
  补齐嵌入侧——旧口径查询嵌入网络失败抛 KNOWLEDGE_RETRIEVAL_UNAVAILABLE 丢
  掉已算好的 FTS 候选（注入块变检索不可用）、且无期限（闭包 HTTP 超时 300s
  全额放行，挂着的供应商卡五分钟）。新 reason 码 KNOWLEDGE_EMBEDDING_FAILED +
  invokeEmbeddingWithDeadline 15s 竞速（与 rerank 同构）+ vectorIndex.search
  意外错误同纪律降级；无配置纯 FTS 路径不动（调查确认三层本就支持无嵌入/
  无重排运行）。验证：typecheck×3 绿 + eslint 0 error + 新增 5 用例 +
  knowledge 簇 200 用例 + full npm test 12810 passed / 0 failed 后推进。

- **2026-08-30 拆解系统优化（P0+P1+stats，评审文档分档落地）**
  （功能树 656cbaf0/seal 本提交；6 files / +503-58，含指纹 compatible repin）：
  P0——拆解提示词删同义改写规则（与扩展器职责重复且致 RRF 多倍投票）、规则 2
  改 Evidence Need 定义（需要相同证据的查询必须合并）；解析器宽容输入+严格
  消费（未知字段忽略，必需字段/内容非法仍拒）；围栏/空白程序剥离不走 LLM
  纠错。链路重排——扩展 LLM 与子查询检索批并行（消除拆解→扩展→检索的串行
  LLM 跳，最坏 15s）。P1——Query Family 两级融合（family 0=直检+扩展变体、
  子查询各自成族、探测各自领号；族内归一→族间等权 RRF，变体数量不再等于
  投票权）+ 候选总预算 240（每查询 topK 分摊夹 [24,60]，engine 透传，同时
  约束 rerank 输入）。Stats——decompositionLatencyMs/RetryCount/
  originalQueryHits/expansionUniqueHits/queryOverlapRatio/evidenceNeedGains。
  P2（多专家拆解/Gap Analyzer/结构化解码/否定 constraint）明确不做。验证：
  typecheck×3 绿 + eslint 0 error + 新增 10 用例 + knowledge 簇 254 用例 +
  full npm test 12820 passed / 0 failed 后推进。

- **2026-08-30 拆解系统优化 P2 收官（Adaptive Specialist/扩展门控/Gap
  Analyzer/否定 exclusion）**（功能树 6bb5878f/seal 本提交；5 files /
  +868-23，含指纹 compatible repin）：§三~§五——廉价复杂度闸（纯规则词标，
  无 LLM Router）：simple→0 方向（完全跳过拆解 LLM）/focused→1/compound→2/
  complex→3-4；四个专业拆解器（fact/cause/relation/validation）认知职责
  分离、方向间并行（墙钟≈单次调用）、合并去重封顶 4；部分方向失败不降级
  留痕。词标 pattern 禁 g 标志（/g 的 .test() lastIndex 状态致评估非纯函数，
  实测踩坑）。§十一——扩展条件门控：simple 与 broad+focused 跳过改写扩展
  （省一次 LLM，expansionSkipReason 留痕）。§二十二——Gap Analyzer 二轮
  补证：高覆盖模式/零命中条件触发，≤3 条补证查询各自领家族，最多一轮，
  secondPass*/gapQueries 留痕。§九——exclusions 词法约束（embedding 对否定
  不可靠）：融合后词面剔除 + 过度匹配保护（>半数放弃过滤留痕）。§十六/§十二/
  §十八明确不做（解码收益被宽容解析边际化/需别名基建/覆盖由 coverage run
  承担）。验证：typecheck×3 绿 + eslint 0 error + 新增 12 用例 + knowledge
  簇 283 用例 + full npm test 12834 passed / 0 failed 后推进。


## 最终状态：已合并（上游同步部分）

- 上游同步 PR #20 已于 2026-08-20 合入 main（merge 0f941e5b）并随 v0.1.29 发布；
  此后各功能沿「Seal 推进记录」逐轮推进，当前 VERIFIED_SOURCE_SHA 以文首坐标为准
  （历史验证链见各轮 seal 记录；本节旧快照曾停留在 Phase 8 树 61779cbd，已过期删除）。
- Upstream ΔU：133 / 133 paths。
- Disposition：ADOPTED 25 + ADAPTED 100 + REGENERATED 4 + INTENTIONAL_DIVERGENCE 4 = 133
  （脚本计算，`build-sync-matrix.mjs --check`：missing=0 / extra=0 / duplicate=0 / unknown=0）。
- 4 个 `hanako.md → lingxi.md` 品牌映射统一分类为 ADAPTED。

### Post-verification diff 记录（`git diff --name-only VERIFIED_SOURCE_SHA..HEAD`，2026-08-23 main @ 2e8077de）

```
.sync-audit/build-sync-matrix.mjs
.sync-audit/upstream-sync-matrix.json
.sync-audit/verified-source-sha.txt
MODEL_OBSERVATORY_RELEASE_ACCEPTANCE_V3.md
PROGRESS.md
UPSTREAM_SYNC_AUDIT.md
UPSTREAM_SYNC_MATRIX.md
```

以上全部为审计材料；无任何生产代码、测试逻辑或 runtime generated artifacts 变化。

### Seal 工作流（合并后现行）

post-verification diff guard 在 `npm test` 中运行（独立可执行形态为
`.sync-audit/verify-post-verification-diff.mjs`，两处 allowlist 副本须同步维护）。
上游同步分支已合并；此后任何非 allowlist 的正常开发提交都会挂该门禁——提交前
要么推进（复跑全量验证后更新 seal），要么退役（删除 seal 文件与 guard 测试）
`verified-source-sha.txt`。本文件「Seal 推进记录」即推进范例。

### Known limitation（保留）

Windows NSIS 已在 windows-latest 构建成功；尚未在真实 Windows 桌面环境执行安装/升级
交互 smoke（宿主平台 macOS 无法运行 NSIS 安装包）。不伪造真机安装通过。

## 2026-08-25 Notebook-first Knowledge 目标

### 当前阶段：本机闭环完成，外部证据待执行

- 已完整读取用户附件 3911 行，确认目标是 Notebook-first、可信引用和可审计全文研究的完整产品能力。
- 已发现主工作区存在 11 个无关文档删除，因此从 `e1bac7be` 创建隔离分支 `codex/knowledge-notebook` 与独立工作区。
- 已建立 `task_plan.md` 和 `findings.md` 作为本目标的断点续跑记录。
- Phase 0–10 的本机实现、逐条规格审计、缺口修复和可执行验证均已完成；四平台正式流水线已接好但尚未远端执行，真实付费模型 live smoke 与提交后 audit seal 保持 `NOT_EXECUTED`。

### Phase 1–3：Knowledge 持久化、可信引用与原生页面（工作区实现）

- 已建立独立 Knowledge 数据库和托管快照，完成 Notebook、来源成员关系、解析产物、引用与重启恢复；未使用偏好文件承载业务数据。
- 已完成 TXT、Markdown、HTML、PDF 的首批引用级解析；扫描 PDF 明确标记需要 OCR，外部原件删除后历史快照和引用仍可读取。
- 已挂载仅限 Studio Owner 的 `/api/knowledge/*`，本机绝对路径导入额外要求 Local Owner，返回值不泄露内部或用户路径。
- 已接入原生一级“知识”页面：Notebook 管理、来源导入/状态/查看器、Notebook-only 查询范围均已完成。
- 当前定向验证：后端 7 文件/55 项、页面与页签 5 文件/16 项、类型检查、持久化扫描均通过；完整回归与 seal 留待所有阶段结束后执行。

### Phase 4：FTS Quick Answer（工作区实现）

- 已完成发送时不可变范围冻结、中文全文索引、稳定分块、标准回答、后端引用校验和失败状态持久化。
- 标准回答只从选中 Notebook 的冻结范围检索；范围为空或任一来源未就绪会明确失败，不会偷偷排除来源。
- 原生页面已能发送标准回答，明确显示“基于相关内容检索”；历史引用可打开冻结快照并高亮原句。
- 当前 Phase 1–4 与模型角色定向验证 10 文件/50 项全部通过，三套类型检查通过；下一阶段进入可选混合检索。

### Phase 5：Provider Operation + Hybrid Retrieval（工作区实现）

- 已把 Google 原生文本调用补进共享非流式边界；Embedding/Rerank 作为独立 Provider 操作能力，不进入辅助模型角色或聊天目录。
- 已建立统一操作解析、供应商凭证刷新、用量/观测调用客户端，以及独立可重建向量缓存；未新增 Knowledge 专属密钥。
- FTS-only、FTS+Vector、FTS+Vector+Rerank 三种状态均有端到端契约测试；设置页相邻呈现知识分析、可选嵌入、可选重排。
- 当前相关验证累计覆盖 43、78、12、20、70 项分组测试，三套类型检查通过；真实供应商付费网络 smoke 因本机无密钥保持 `NOT_EXECUTED`。

### Phase 6–8：全文研究、证据链与研究界面（工作区实现）

- 已完成冻结范围、覆盖清单、分析单位、批次/尝试账本与重启恢复；检索只改变执行顺序，不能减少全文覆盖。
- 已完成服务端引文复验、证据/结论/矛盾事实与受约束综合；覆盖或完整矛盾检查不足 100% 时只能形成部分完成报告。
- 已完成活动研究重连、取消/恢复、覆盖进度、冲突、限制和历史引用跳转界面；研究事实保存在独立数据库，宿主任务中心只负责展示和控制。
- 已完成本地文件刷新、粘贴来源和带每跳 SSRF 防护的冻结网页快照；历史内容版本不受外部来源变化或删除影响。

### Phase 9：本地验证收口（工作区实现）

- 持久化治理为 65 个 Store / 756 个访问点；Knowledge 真实 schema v5 进入指纹与 tripwire，兼容性指纹为 `sha256:34763e876cb11167ccc0388a9bbbfb4c1f177081d7c1fd415d1b245a1080997b`。
- 正式运行闭包为 10634 个文件（734 source graph / 11 runtime assets / 9889 nft）；开放边界保持 1 条既有基线边。
- 生产服务签名构建和原生依赖启动冒烟通过，临时签名材料已销毁；前端生产构建通过。
- 真实浏览器使用隔离数据完成 Notebook 创建、粘贴来源、READY 状态、逐字正文和行号锚点查看；控制台与页面错误均为 0。
- 最终完整回归（仅排除需要提交坐标的 seal 测试）为 1221 文件通过、1 跳过；12270 项通过、7 跳过；类型检查、零错误代码检查、边界和补丁格式均通过。
- 生产依赖审计保留既有 7 个中等级别问题、退出码 1；未执行破坏性自动修复。
- `WORKFLOW_READY / NOT_EXECUTED`：macOS arm64、macOS Intel x64、Windows x64、Linux x64 已使用明确宿主并接入源级恢复、种子验签和真实归档两次启动烟测，但未提交、未远端执行。
- `NOT_EXECUTED`：真实供应商 Embedding/Rerank live smoke、获准提交后的 audit seal。当前未提交、未推送。

### Phase 10：逐条规格审计与缺口修复（本机完成）

- 已把附件 75 个主题章节、A–M 核心场景和全部明确禁项映射到源码、测试与运行时证据；未定义协议的“高影响歧义确认”未用主观启发式伪造实现。
- 已补齐最终综合发现证据不足时的 Verification Step：只复查冻结范围内全部分析单元，新证据经逐字校验进入账本，再执行第二次受约束综合；步骤、单元、尝试和关系均可恢复、可取消、可审计。
- 已补齐 Notebook 软删除后的历史报告/引用可读、Knowledge 连续请求热读取凭证与模型、多格式多 Notebook 范围计数、失败解析整单拒绝、无范围禁发和检索免责声明等验收证据。
- 已补齐四平台交付链：矩阵先硬校验真实宿主，再运行新建/重启/Full Research/Verification 崩溃恢复；服务器种子验签后从真实归档解包、启动两次并读回冻结正文。当前 macOS arm64 真包链已通过，另三宿主待远端运行。
- 定向集中回归 27 文件 284 项、生成物/边界回归 11 文件 138 项、持久化门禁 5 文件 54 项均通过；四平台补强后全仓为 1221 文件、12270 项通过。Renderer 与带一次性签名的完整服务器构建通过，临时密钥已精确销毁。
## 2026-08-25 Knowledge 最终本机复验补充

- 四平台工作流契约：4 files / 28 tests 全部通过。
- Knowledge 平台稳定性集合：9 files / 85 tests 全部通过。
- 最终低并发全量测试：1222 files passed、1 skipped；12274 tests passed、7 skipped；130.80s。
- 三套 TypeScript 检查、代码规范检查（0 error，仓库既有 warnings）、开放边界 ratchet、补丁空白检查均通过。
- 四种真实托管主机的工作流执行仍为 `NOT_EXECUTED`：当前改动未提交，且本任务没有 commit、push 或远程工作流触发授权。
## 2026-08-26 Knowledge 外部阻塞收口

- 重新按 3911 行任务书审计当前状态；本机可闭合项没有发现新缺口。
- 当前复跑：四平台统一稳定性 85/85、工作流契约 28/28、本机真实服务器归档新装/重启/快照读回，退出码均为 0。
- 远端不存在 `codex/knowledge-notebook` 分支或该分支工作流运行。Phase 9 四种真实宿主仍为 `NOT_EXECUTED`；需要明确 commit、push 和远程触发授权后才能继续。
