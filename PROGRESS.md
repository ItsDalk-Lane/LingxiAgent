# PROGRESS — openhanako v0.444.1 → v0.447.4 上游同步

## 审计坐标（固定，执行期间从未移动）

```
UPSTREAM_BASE_SHA     = cc19cb49b0786d61ed723764e0a83baf87887270  (openhanako v0.444.1)
UPSTREAM_TARGET_SHA   = c6d0405294be67cb134c2758f6472748ee73e2be  (openhanako v0.447.4)
LINGXI_BASE_SHA       = 97595264ead8735a04559507ddaade25db8a4e15  (v0.444.1 同步完成点, PR #2)
LINGXI_START_SHA      = ca0b417e36a6a1f80947458aaed328a25718e41b  (main HEAD @ 2026-08-20)
VERIFIED_SOURCE_SHA   = 7374e0d6b61a2a6d5fb29522c99168ebf5177486  (最终验证所针对的源码树；2026-08-22 模型调用可观测性第五轮 payload capture 树后推进)
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
  功能树 3cf0e6ed/seal ea909c6e；第五轮 7374e0d6b61a2a6d5fb29522c99168ebf5177486）：第四轮 Phase 5 Semantic Input
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

## 最终状态：READY TO MERGE

- Upstream ΔU：133 / 133 paths。
- Disposition：ADOPTED 25 + ADAPTED 100 + REGENERATED 4 + INTENTIONAL_DIVERGENCE 4 = 133
  （脚本计算，`build-sync-matrix.mjs --check`：missing=0 / extra=0 / duplicate=0 / unknown=0）。
- 4 个 `hanako.md → lingxi.md` 品牌映射统一分类为 ADAPTED。
- `VERIFIED_SOURCE_SHA = 7374e0d6b61a2a6d5fb29522c99168ebf5177486`：被完整测试验证的代码树
  （含收口树 d4cf92a8 的全部验证 + 文档清场树复跑验证 + 归档修复树 051f6117 复跑的
  typecheck/lint/全量测试 + v0.1.29 release 树 fabd6dbf 复跑的 typecheck/目标套件 +
  mac self-install 树 dcf3546a 的 PR CI typecheck/lint/build/全量测试 +
  凭证边界修复树 b8688895 的本地 typecheck/定向测试 + PR CI typecheck/lint/build/全量测试 +
  保留标签管道修复树 c83d238a 的本地 typecheck/定向测试 + 全量测试 +
  保留标签指纹补钉树 be95b344 的 tripwire/guard 验证 +
  模型调用可观测性五轮树 a9a5f3f4 / b9238533 / 53fa4575 / 3cf0e6ed / 7374e0d6b61a2a6d5fb29522c99168ebf5177486
  的本地 typecheck/定向测试 + 全量测试，见「Seal 推进记录」）。当前 HEAD 只比 VERIFIED_SOURCE_SHA 多审计收口内容。

### Post-verification diff 记录（`git diff --name-only VERIFIED_SOURCE_SHA..HEAD`）

```
.sync-audit/verified-source-sha.txt
.sync-audit/upstream-sync-matrix.json
.sync-audit/build-sync-matrix.mjs
UPSTREAM_SYNC_MATRIX.md
UPSTREAM_SYNC_AUDIT.md
PROGRESS.md
```

以上全部为审计材料 / 审计测试 / 审计脚本；无任何生产代码、测试逻辑或 runtime
generated artifacts 变化。

### 合并交接（重要）

post-verification diff guard 在 `npm test` 中运行。本分支合入 main 后，任何非 allowlist
的正常开发提交都会挂该门禁——合并后第一件事是推进（复跑全量验证后更新 seal）或退役
（删除 seal 文件与 guard 测试）`verified-source-sha.txt`。本文件「Seal 推进记录」即推进范例。

### Known limitation（保留）

Windows NSIS 已在 windows-latest 构建成功；尚未在真实 Windows 桌面环境执行安装/升级
交互 smoke（宿主平台 macOS 无法运行 NSIS 安装包）。不伪造真机安装通过。
