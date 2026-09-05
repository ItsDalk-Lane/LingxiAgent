# PROGRESS — Seal 推进台账（前身：openhanako v0.444.1 → v0.447.4 上游同步收口）

上游同步已于 2026-08-20 经 PR #20 合入 main（merge 0f941e5b）并随 v0.1.29 发布；
本文件自那以后作为 seal 推进台账延续，「Seal 推进记录」一节是现行工作流。

## 审计坐标（上游坐标固定，源码验证坐标按阶段推进）

```
UPSTREAM_BASE_SHA     = cc19cb49b0786d61ed723764e0a83baf87887270  (openhanako v0.444.1)
UPSTREAM_TARGET_SHA   = c6d0405294be67cb134c2758f6472748ee73e2be  (openhanako v0.447.4)
LINGXI_BASE_SHA       = 97595264ead8735a04559507ddaade25db8a4e15  (v0.444.1 同步完成点, PR #2)
LINGXI_START_SHA      = ca0b417e36a6a1f80947458aaed328a25718e41b  (main HEAD @ 2026-08-20)
VERIFIED_SOURCE_SHA   = 8ade0726a10648a15cf08f22e0cbbeb16ca512b6  (2026-09-06 双线合并树封印：环境信息卡+PR#43 契约路径不变量)
历史上游同步工作分支  = feature/upstream-sync-0.447.4
当前知识重构执行分支  = feat/knowledge-retrieval-research-p0-p3
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

### 2026-09-04：P3 最终验证进行中

- 固定基线与执行分支不变。P3-01 至 P3-06 已按序提交；P3-07 清理生产旧编排、重复查询/范围解析和旧预算，历史回归实现移出发布目录。
- 知识测试 1087 PASS，本机平台检查 151 PASS，三组类型检查、全量 lint、边界检查通过。首次全量 13734 PASS / 2 FAIL / 7 既有 SKIP：一项为旧审计坐标，一项为完整性内部工具登记；登记修复及隔离边界回归 52 PASS。最终完整全量复验仍待完成。
- 本机服务端、开放服务端、客户端、种子验签与实际归档重启检索均 exit 0；原生 HNSW 和移除原生扩展后的精确回退及本地检索均已实际执行。完整应用打包、当前源码四平台和最终生成物一致性仍进行中。
- 详细交付与剩余项见 KNOWLEDGE_REFACTOR_* 报告。本条不推进 VERIFIED_SOURCE_SHA、不声明最终封印完成；最终源提交与复验后按已获授权同步审计，保留独立封印，不合并 main。


### 2026-09-04：P2 阶段全部门禁与审计收口

- 被验证源码：`d4292b2d0ba5029e7c4b1d1e2969b031f5c7b903`，固定基线仍为 `3eab85891a1747c64064252804f70c0a3773f021`，执行分支 `feat/knowledge-retrieval-research-p0-p3`。P2-01 至 P2-08 按序提交完成；每阶段同步审计已获用户授权，最终封印保留。
- 首轮全量 13415 PASS / 19 FAIL / 7 既有 SKIP，84.79s：17 项真实源码启动不支持参数属性、1 项工具目录只采普通入口、1 项旧 P1 审计坐标。先修实现与真实快照采样，保留测试和全部豁免表不变；真实启动回归 48 PASS、原目录 3 PASS。
- 修复后指定测试与目录覆盖 86 PASS、本机平台 smoke 94 PASS；三套类型、全量 ESLint（0 error / 9188 warning）、开放边界通过。完整服务端/开放服务端/客户端五入口构建通过，种子验签与真实归档解包/安装/解析/重启/不可变原文读取通过；当前仅 macOS arm64 证据，原生 HNSW 与移除扩展 portable 均通过。日志 `/tmp/lingxi-knowledge-p2-fixed-{build-server,verify-seed,packaged-smoke,build-open,build-client}.log`。
- 修复后五生成器两轮全部通过，完整 git diff 两轮均为 0；测试清单 937 文件，第二轮逐字一致；开放树各 882 文件且逐文件一致；清单 66 stores / 779 sites、知识库 v18、指纹 3beb2e79…、运行闭包 10687 文件。详见 `/tmp/lingxi-knowledge-p2-fixed-generator-results.json`。
- 全量审计复验 `npm test` exit 0：1333 文件 PASS / 1 既有 SKIP，13434 测试 PASS / 0 FAIL / 7 既有 SKIP，86.98s；日志 `/tmp/lingxi-knowledge-p2-audit-full.log`。阶段收口 `56dc1086aa06d0592c12e8a53eef9b8ea8546812` 相对已验证源码 d4292b2d 仅更新 KNOWLEDGE_REFACTOR_PROGRESS.md；生产、测试、构建逻辑与生成物无差异，已用 git diff 验证。
- 当前源码坐标推进到上述阶段收口，审计矩阵与差异守卫另行复验；阶段审计提交只包含原有白名单六文件，P2 全部门禁满足后按序进入 P3-01。没有修改白名单、没有合并 main，P3 最终封印仍保留。


### 2026-09-04：P1 完整阶段门禁收口

- 被验证提交：`2387398589ec5494e1adb28b014dc84ebcf15a64`，任务书固定基线保持 `3eab85891a1747c64064252804f70c0a3773f021`。P1-01 至 P1-08 按序完成，用户授权的阶段审计与最终封印继续分开执行。
- 第三轮远程 Build `33829055797` 在 `8295e5ff937cf9d3e49c082231188a01bd56122b` 上整体 SUCCESS：质量门禁 13077 PASS / 17 既有 SKIP / 0 FAIL（410.55s），四平台完整构建均 PASS，各平台知识测试 94 PASS、真实包内 native=hnsw 与 removed-native=portable 均通过；Windows standalone 再次独立验证通过。最终产物启动/历史升级回归 304 PASS（1.88s）。
- 稳定 Linux runner：100k HNSW P95 6.041646ms，exact P95 416.031149ms，68.86056 倍，top-10 overlap 99.75%，墙钟门禁已启用并通过；原始生成报告已提交，不改原始测量数据。
- 当前收口提交相对上述远程验证提交只增加任务进度和两份生成性能报告；生产代码、测试和构建逻辑相同。六项 P1 期末行为均核实真实入口与已通过测试，详见 KNOWLEDGE_REFACTOR_PROGRESS.md。
- 阶段末五生成器两轮全部通过，第二轮完整 git diff --exit-code 为 0，测试清单逐字节一致；三种本地构建、包内与平台烟测、类型/lint/boundary 已通过。当前源码全量审计复验 exit 0：1302 文件 PASS / 1 既有 SKIP，13087 测试 PASS / 7 既有 SKIP / 0 FAIL，80.46s，日志 `/tmp/lingxi-knowledge-p1-close-audit-full.log`。P1 阶段门禁全部通过；审计提交后按序进入 P2-01，未合并 main。


### 2026-09-04：P1 第二轮跨平台修复复验

- 当前源码：`f86da54313e35a5868c6f045c9495717d61ba1bb`；仍属 P1-08，保留最终封印，不改审计白名单。
- 第二轮远程 `33826852985`：两种 Mac 与 Linux 全构建 PASS，Windows 原导入清理 PASS、包内原生检索 FAIL；质量门禁原请求间隔用例 FAIL（13074 PASS / 1 FAIL / 17 既有 SKIP，466.48s）。记录失败不删改测试。
- 计时修复新增回归证明旧代码实际间隔 0ms；保持并发与间隔配置，用同步调用后的单调时钟计时。Windows 索引刷盘改用可写句柄，仍执行 fsync，原生和回退断言不变。
- 本机修复专项 6 文件 / 53 PASS，P1 指定 8 文件 / 44 PASS，平台烟测 8 文件 / 94 PASS；类型检查三套、lint（0 error / 9176 warning）、boundary、三种构建、种子验签、包内原生与缺扩展回退烟测均通过。五生成器两轮通过且零漂移；数据库版本和 paid vectors 不变。
- 全量复验 `npm test` exit 0：1302 文件 PASS / 1 既有 SKIP，13087 测试 PASS / 7 既有 SKIP / 0 FAIL，78.60s；日志 `/tmp/lingxi-knowledge-p108-third-audit-full.log`。第三轮四平台待提交后运行；仍属 P1-08，不进入 P2。


### 2026-09-04：P1 跨平台修复复验

- 当前源码：`c452a705f55286df3cd18373390819cf34d60fc5`；首轮远程 `33825262170` 已确认质量门禁、macOS arm64、Linux x64 通过，Windows 清理目录与 Intel 原生链接失败。
- 保留原测试与平台门禁，修复关闭资源占用和锁定 2.26.0 原生包缺失链接；算法、配置、数据表与原文不变。新增关闭回归与构建契约测试。
- 修复本地定向 8 文件 / 57 PASS，三套类型检查、lint（0 error / 9176 warning）、boundary、三种构建、包内正常与回退烟测、两轮生成器通过。Intel 修复同一二进制分别以 arm64 与 x64 真执行建图/查询通过，x64 本地为 Rosetta，不冒充 Intel 物理 runner。
- 修复后全量复验：`npm test` exit 0，1301 文件 PASS / 1 既有 SKIP，13085 测试 PASS / 7 既有 SKIP / 0 FAIL，80.13s；日志 `/tmp/lingxi-knowledge-p1-repair-audit-full.log`。第二轮远程门禁待回填；当前仍 P1-08，最终封印保留。


### 2026-09-04：知识重构 P1 阶段审计（本机已通过，远程验证中）

- 用户授权沿用 P0：每阶段验证后同步审计记录，保留最终封印，不改审计白名单。
- 被验证源码：`9bee41dcfade7baad689d6979bbb3f8ede0b48ee`，P1-01 至 P1-08 按序提交；固定基线仍为 `3eab85891a1747c64064252804f70c0a3773f021`。
- 本机构建、指定测试、三套类型检查、lint/boundary、五生成器两轮、完整包内正常/缺原生扩展两种启动检索均已通过，详见源提交中的 KNOWLEDGE_REFACTOR_PROGRESS.md。
- 全量复验：`npm test` exit 0，1300 文件 PASS / 1 既有 SKIP，13081 测试 PASS / 7 既有 SKIP / 0 FAIL，78.99s；日志 `/tmp/lingxi-knowledge-p1-audit-full.log`。原审计测试与质量依赖测试均通过，未删除、跳过或放宽测试。
- 四平台 Build 与固定 Linux runner 性能结果待回填；尚未进入 P2，不声明整个任务完成。


### 2026-09-04：知识重构 P0 阶段审计

- 用户授权：每阶段完成验证后同步审计记录，并保留任务书最终封印提交；任务范围与顺序不变。
- 被验证源码：`5c016df183ad207cf1ca33de274abb7a4eb10057`（固定基线 `3eab85891a1747c64064252804f70c0a3773f021`，P0-00 至 P0-08 按序完成）。此审计提交只含既有白名单材料，没有修改测试或扩大白名单。
- 2026-09-04 全量复验：`npm test` exit 0；1281 文件通过、1 既有跳过；13002 测试通过、7 既有跳过、0 失败；76.42s。日志 `/tmp/lingxi-knowledge-p0-seal-full-20260904.log`。原封印唯一失败已经消除。
- 同一源码内容的其余 P0 门禁：三套类型检查、全量 lint、开放边界、指定知识测试、三种本机构建、五个生成器两轮无漂移均通过，精确命令、结果及历史失败见 `KNOWLEDGE_REFACTOR_PROGRESS.md` P0-08。提交后未修改运行代码、测试或生成物。
- 本机真实性能：10k 热 P95 1.624334ms；100k 热 P95 17.814625ms、冷 P95 220.717334ms；远程调用 0、证据最多 8 条、892 tokens。仅 macOS arm64；Linux 手工工作流、后续阶段跨平台验证及最终打包尚未执行，不作全项目交付或正式签名声明。
- P0 阶段门禁全部通过，允许进入 P1-01。任务进度文件按原白名单限制随下一项源码提交回填；最终封印保留。


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

- **2026-08-30 注入预算实践上限 96k（实测回归修复）**（功能树 0cf94f45/seal
  本提交；2 files / +40-1）：P2 实测发现锚点/融合池随预算伸缩把 512k 动态
  预算装到满格（usedTokens 493,757/495,616、136 块），主模型预填充 49.4 万
  token 使 session/reply 77.3s——检索提速被预填充吃回。注入预算 =
  min(动态预算, KNOWLEDGE_INJECTION_BUDGET_MAX_TOKENS=96k)；「能装≠该装」，
  注入同时受模型上下文与预填充时间预算约束（§十三 思想）。验证：新增 1
  用例 + full npm test 12835 passed / 0 failed 后推进。

- **2026-08-30 revert 注入预算实践上限 96k**（功能树 e7b14874/seal 本提交）：
  用户指出该修复未经批准且与其亲自指定的「锚点 50%\/融合池 70% 按上下文
  倒推」设计相抵——诊断请求（「查看一下」）不应自行变成修复提交。revert
  恢复原设计；「512k 证据装满预算 → 主模型预填充 77s」的权衡数据已呈报
  用户，是否加上限及数值由用户后续拍板。验证：typecheck×3 绿 + injector/
  execution 套件 98 用例绿后推进。
- **2026-08-31 知识问答重构：覆盖两档化 + 主模型滚动多轮注入**（功能树
  7de81782/seal 本提交）：用户批准的方案（检索侧机器完全不动，改动集中在
  证据交给主模型与作答段）。①覆盖判定三档→两档（exhaustive 执行链路/蒸馏
  压缩/broad→exhaustive 升级整体移除；executor/reduction/distiller 三模块删
  除；manifest 裁成 fidelity 面；store coverage run 写 API 删除；表与 DDL 保留
  存量兼容零迁移）；②超预算改 knowledge-rollup 滚动注入（会话主模型经
  session streamFn 侧线缓冲调用逐部分消化，中间笔记逐部分标注传递，循环内
  模型可 need-more-evidence 自主补充检索，轮/查询数硬上限，孤立超限单条也送
  消化轮）；③knowledgeDistill 槽位移除、ws 事件换
  knowledge_rollup_progress/knowledge_supplement_search、stats 新增 rollup
  契约、前端胶囊/折叠卡/五语言。已知代价（已呈报用户确认）：超预算 turn 从
  廉价蒸馏模型多批并行变主模型 N 轮串行，延迟与 token 花费上升。验证：
  typecheck×3 绿 + 全量 npm test 12766 用例通过（0 失败；persistence 指纹
  compatible repin sha256:4172d591…、开放边界清单重生成后 tripwire/lint 绿）
  后推进。
- **2026-08-31 过程可见二轮：knowledge_trace 逐行广播**（功能树 a4048480/seal
  本提交）：用户反馈检索期界面只有三点干等，要求对齐编程 Agent 的工具调用
  过程卡全程可见。engine 拆解/扩展/补证闭包与 retrieve 门面统一插桩
  knowledge_trace 事件（只发元数据禁发模型输出），前端过程行堆按 id 原位
  更新（检索行 start=查询词 → done=「N 个搜索结果」），rollup/supplement
  事件同步映射为 read/note 行；首个非知识事件保守清除整堆。验证：typecheck×3
  绿 + 全量 npm test 12771 用例通过（0 失败）后推进。
- **2026-08-31 过程可见三轮：过程行堆=等待态本体**（功能树 c0310788/seal 本
  提交）：用户实测反馈「检索提示先消失、退回三个点干等很久才有输出」——根因
  是过程行堆被 session_user_message 等普通事件保守清除，把主模型预填充+生成
  的漫长等待裸露成三个点。改为：过程行只在答案正文首个 text_delta 或
  assistant_run_end（中止/空回包兜底）时收起；submit 两路径检索完成即发
  note+detail=answer 的「正在生成回答」行盖住预填充期。验证：typecheck×3 绿 +
  全量 npm test 12772 用例通过（0 失败）后推进。
- **2026-08-31 过程可见四轮：合成工具卡**（功能树 287d9333/seal 本提交）：用户
  三轮反馈后定稿形态——对齐编程 Agent，一个动作一张卡依次长在助手消息流里。
  ws 层把 knowledge 事件翻译成合成 tool_start/tool_end 喂 streamBufferManager
  （复用既有工具卡管线：无消息时自动建助手消息，正文同条复用）；ToolCall 增
  可选 resultNote（检索卡 done 显「N 个结果」）；回答卡以在途集合守卫防空
  tool_end 凭空造消息（chat-turn-lifecycle 抓到的回归）。旧过程行堆整体移除。
  验证：typecheck×3 绿 + 全量 npm test 12772 用例通过（0 失败）后推进。
- **2026-08-31 实测回归双修复**（功能树 22492163/seal 本提交）：用户实测一轮
  6.5 分钟无输出，observability 取证（reply 调用 240s aborted + 163s 重试 +
  cache_contract_violation ×2）实锤：滚动单份只按剩余预算装填 → 49 万 token 巨
  份；侧线调用无用途标记污染缓存契约致真实轮全量重填。修复：单份封顶 64k +
  runWithProviderCompatPurpose(knowledge_rollup) + 守卫跳过非 chat 用途。
  隔离实例端到端因克隆环境模型解析怪癖未能跑通（与修复无关），以单元/集成
  测试 + 全量回归覆盖。验证：typecheck×3 绿 + 全量 npm test 12773 用例通过
  （0 失败；closure/boundary/指纹三生成物重钉）后推进。
- **2026-08-31 用户截图验收 + 文案修补**（功能树 70b210a7/seal 本提交）：用户
  实机截图确认合成工具卡形态达标（一动作一卡长在消息流）；修补充检索卡标签
  与 resultNote 文案重复（换 count-only 新键 chat.knowledgeSupplementQueryCount，
  五语言）。验证：typecheck×3 绿 + 定向套件 30 用例绿（纯文案与前端单点改动，
  未触发指纹/闭包面）后推进。
- **2026-08-31 知识问答两档化**（功能树 55834549/seal 本提交）：answerMode
  qa/assist → fast/detailed（存量值读取侧按详细、显示层保留旧标签、默认快速）；
  快速档零辅助 LLM 轮（engine 跳 planner + injector 跳拆解/扩展/gap/探测）+
  rerank 动态门控（RRF 融合分领先 ≥0.008 跳过、扎堆 5s 期限、rerankSkippedReason
  留痕）+ 证据封顶（锚点 ≤12/渲染预算 ≤8192）+ 禁滚动；详细档原行为（回归锚）；
  rerankPolicy 三层穿线；stats.stageTimings 九段计时；golden set 质量门禁
  （tests/knowledge-retrieval-golden.test.ts 双档 recall）；五语言 8 新键。
  验证：typecheck×3 绿 + eslint 0 error + lint:boundary 绿 + 全量 npm test
  12782 用例通过（lifecycle delete-wins 一例为预存 flaky：干净树 6 次重跑亦
  1 次红，与本改动无关）；fingerprint 未动（sha256:5f525a1d 不变）后推进。

- **2026-08-31 v0.1.32 release metadata**（功能树 5d7a81a1/seal 本提交）：version
  0.1.31→0.1.32 + releaseGeneration 9→10（双 bump 防同代静默跳过激活）+ digest
  v0.1.32（手写 v1：两档化/过程工具卡/延迟治理/供应商协议四条目；经
  --append-history 进 v2 滚动史，共 13 条）+ release-preflight 测试坐标推进。
  验证：validate-release-digest v1/v2 均过 + release-preflight --tag v0.1.32
  PASS（gen 10 > 9）+ test:artifact-release-smoke 8 文件 304 用例绿后推进。

- **2026-08-31 fix(ci) renderer 上传 glob 修复**（功能树 ec1850c/seal 本提交）：
  v0.1.32 首次构建的 release 作业在 renderer-<ver>.tar.gz 上传处硬失败——矩阵
  os 自 macos-latest 改 macos-15 后 installer 工件名漂移，旧 glob
  '*installer-macos-latest-arm64*' 永不匹配；归档四平台字节同一，find 改为
  匹配任意 installer 副本（runner 标签漂移免疫）。workflow 单行修复；验证：
  yaml 解析合法 + workflow 契约套件（quality-gates-contract/package-build-
  order/publish-train）67 用例绿 + seal diff guard 复验后推进。v0.1.32 tag
  按 v0.1.30 先例删旧 draft 后重打（旧 run 33410110948 的 13 个已上传产物
  随 draft 一并废弃重出）。
- **2026-09-01 迁移 #54：清理 utility_model/utility_large_model 死键**
  （fix/knowledge-latency-hardening，已随 20a9e944 批量提交）：辅助槽排障时实锤——283d9581
  语义 Slot 重构删了两个旧键的全部读写路径，但存量 preferences.json 里的值
  一直没清，用户配置里躺着的 gemma4 死键造成「已配置取标题模型」的错觉
  （title 槽实际读 title_model，未配置回退 chat）。core/migrations.ts 加
  #54 纯删除迁移（不做值迁移：Slot 回退 chat 已是实际行为，静默映射反而
  凭空换模型）；migrations.test.ts 以 #54 为 runner 夹具重写（空注册表
  断言全部更新 + 删除/幂等/全新安装三组行为测试，7 用例）。验证：typecheck
  ×3 绿、tripwire/cli-closure-census 原样绿（指纹 sourceDigest 不含
  migrations.ts、closure 只记模块图，均无需重钉）；dev 实例重启实跑
  _dataVersion 53→54、两键删除、memory_model 等活跃键原样保留。打包版
  （~/.lingxi）同款死键待携带 #54 的版本升级时自动清理。

- **2026-09-01 fix(test) spawn 等待预算放宽**（功能树 37580730/seal 本提交）：
  v0.1.32 合入后 main push run 在 macos-intel 连续两轮假红（同树 PR run 绿、
  本地三文件 30/30 绿、git diff 证实 PR 头与 merge 提交树一致）——三个真实
  spawn server 测试文件的等待预算（10s/15s/25s）被劣化 runner 上 Node 24
  冷启动 TS 转换击穿（同窗口套件时长 24m→45m）。测试断言的是顺序契约非
  墙钟 SLA：spawn 等待统一 60s、用例超时 90s。验证：typecheck×3 绿 +
  三文件 30/30 绿后推进。

- **2026-09-01 记忆系统升级 + 观测轨迹详情页 + 迁移#54 批量提交**（功能树
  20a9e944/seal 本提交，feat/pending-sep01）：三线合集 80 files / +14435-420。
  ①借鉴 nuphus 记忆五项落地：tenets 用户原则层（tenet_propose 工具 + 聊天
  审批卡 + 设置页 + agents 路由 CRUD，active 注入新会话 system prompt）、
  记忆导航节（navigation.md 经 assemble 第 5 段注入 cache 分界线后）、facts
  语义检索（float32 BLOB + JS 余弦，FTS×向量 RRF 融合 + memory.embedding_model
  配置 + engine 侧回填）、检索零结果诊断、经验反馈路由（👍/👎 记经验库）。
  ②迁移 #54 utility_model/utility_large_model 死键清理（见上条）。
  ③模型观测轨迹：列表 minCallCount≥2 过滤（cursor 指纹绑定）+ dsh
  ui-trajectory 详情页移植（trace-detail/ 11 文件，会话 join 双通道 +
  prompt-snapshot sidecar 路由 + 载荷 TXT 直出 + @tanstack/react-virtual
  3.14.10）+ 主链分类修复（subsystem/purpose 不只 parentCallId）。
  指纹（compatible）/closure/inventory/export-manifest 已更新，五语言齐。
  验证：typecheck×3 绿 + eslint 0 error + 全量 npm test 12862 用例通过
  （唯一失败为 post-verification-audit-seal 预期红，旧坐标 37580730 下）后推进。

- **2026-09-02 模型调用统一 + 视频上传闭环 + 工作台继承**（功能树
  ea03c627/seal 本提交，feat/pending-sep01）：79 files / +2738-1181。
  ①模型调用统一：来源身份公共契约与解析服务
  （lib/llm/model-observability-source-identity.ts）、观测 schema v3→v4
  additive（来源快照不含正文、当前名称增强、轨迹根来源投影）、助手消息与
  模型调用持久关联（隐藏 custom entry + 历史加载归并 + 编号优先）、调用
  详情全屏化（载荷自动并行纯文本、技术信息默认收起）、观测持久化全开
  （旧客户端 false 拒绝、设置页只保留保留天数）。②新建聊天继承当前主
  工作台。③视频上传闭环：浏览器上传（魔数/大小/数量三段式门禁）+
  shared/video-mime 重构 + 四类供应商格式交集 + known-models 目录声明。
  ④样式纪律收口：新增 UI 全部走 token（space/overlay/text/border +
  类内局部定义行），style-discipline 棘轮零新增（首跑红 4 项 → token 化
  归零）。指纹/closure/export-manifest 已更新，五语言齐。验证：typecheck×3
  绿 + 全量 npm test 12895 用例通过（CSS 收口后复跑全绿；首跑仅
  style-discipline 4 项违例红）后推进。

- **2026-09-02 embedding gate Windows 粒度派发修复**（功能树 cc6315e0/seal
  本提交，feat/pending-sep01，1 file / +8-2）：windows-2025 CI 连续三轮假红
  实锤根因——scheduleDispatch 的 setTimeout(wait) 在 Windows 计时器粒度
  （~15.6ms）下提前一个粒度醒，两次派发实测间隔 66ms < 配置 80ms，破坏
  「至少间隔 minRequestIntervalMs」语义（tests/knowledge-lifecycle.test.ts
  间隔断言假红）。修复：dispatch 回调先 intervalElapsed 复验，未过节流窗口
  则 scheduleDispatch 续等剩余时间；语义在所有平台成立，断言不放宽。同日
  另两轮失败（auth-storage legacy key 恢复、vitest worker 意外退出）与代码
  无关（本地 31/31 绿、同 job attempt 1 全绿），属 runner 抖动。验证：
  typecheck×3 绿 + knowledge-lifecycle 16/16 + persistence tripwire 15/15 +
  closure/boundary 39/39 后推进。

- **2026-09-02 v0.1.33 release metadata**（功能树 850bf49e/seal 本提交，
  release/v0.1.33，5 files / +271-76，数据-only，零生产代码变化）：package.json
  版本 0.1.33 + releaseGeneration 11（双 bump，防同代静默跳过激活）；release
  digest v1（手写 digest 工作流：6 items——模型调用统一/视频上传/记忆系统
  升级/观测轨迹详情页 high·feature、工作台继承 medium·improvement、迁移#54
  low·migration）+ v2 追加至 14 条（generate-release-digest --append-history）；
  release-preflight 活体测试随版本推进。验证：validate-release-digest v1/v2
  过 + release-preflight --tag v0.1.33 PASS（gen 11 > 10）+ release 目标
  套件 6 文件 55 用例全绿后推进。

- **2026-09-02 AtomGit 镜像旧 release 清理降级 best-effort**（功能树
  61099bcd/seal 本提交，fix/atomgit-mirror-delete-tolerant，2 files /
  +96-2）：v0.1.33 发布 mirror-atomgit 实锤——18 产物全部上传成功后，
  收尾删除被取代旧 release 被 GitCode v2 web 端点拒（425
  TOKEN_INVALID_ERROR）。排查证据链：token 在 v5 完全有效且 self-permission
  Owner 满权限；v5 无 release-only DELETE（路由 405）；v2 五种鉴权传法
  （Bearer/access_token 查询参数/PRIVATE-TOKEN/Cookie/X-GitCode-Token）
  全拒；镜像堆积 15 release 证明清理从未成功过。结论：v2 只认浏览器会话，
  PAT 结构性无解。修复：retainOnlyTargetRelease/makePrereleaseQuotaRoom
  删除改经 deleteOldReleaseTolerant，失败 WARN 继续不判红；真实环境端到端
  exit 0（14 删除 → 14 WARN）。验证：typecheck×3 绿 + 全量 npm test
  12896 通过（含新增 425 降级用例）后推进。

- **2026-09-05 v0.1.33 新建会话/工作台五轮修复**（功能链
  7d2672d2→332e8960→d0dd2492→01cdd80b/seal 本提交；
  三提交 23+3+2 文件 / +2489-55，另 01cdd80b 测试语义补钉 1 文件，含 engine.ts
  持久化指纹 compatible 重钉 sha256:d7239a0f… 不变）：修复轮一（新建会话继承源重排/去空缓存种子/
  loadSessions reconcile 自愈/五语言文案）、轮二（engine 暴露 getSessionWorkspaceMount，
  switch 回包补齐挂载身份，治左栏空白三症状）、轮三（默认工作台与 Agent 工作台目录
  同目录两本账合流）、轮四（默认工作台显示名=配置目录名派生+启动合流键）、轮五
  （Windows 工作台显示双缺陷：split('/') 取名改 workspaceDisplayName、挂载/历史跨源
  去重、大小写变体挂载创建复用、继承链归一化）。验证：typecheck×3 绿 + eslint 改动
  文件 0 error + 定向 29/475/146/27 用例绿 + 全量 npm test 12956 过/2 败（1=推进前
  seal guard 旧坐标预期红，推进后归零；1=既有 DeskSection Jian drawer 用例，前轮
  stash 对照证实先在）后推进。环境备注：本轮全量首跑 41 文件级失败均为 workspace 包
  @lingxi/plugin-* 未构建的解析失败（2026-09-04 npm install 后 dist 缺失），
  `npm run build:packages` 后归零，非代码回归。
  二次推进（01cdd80b）：包构建后暴露 mobile 全局新建聊天用例——轮一规则 B 改语义时
  其桌面孪生已更新、该用例因包未构建从未执行而被漏掉，断言仍锁定旧「重置到 Primary
  工作台」语义；按同一裁决翻转为「目录保持当前显示工作台，Agent 身份仍重置 Primary」
  （MobileApp.test.tsx 24/24 绿；DeskSection Jian drawer 隔离 27/27 绿，判负载相关
  既有 flaky）。UPSTREAM_SYNC_AUDIT/MATRIX 的 SHA 副本随二次推进同步。

- **2026-09-02 安全双件套 + 沙盒拒绝分因文案**（功能树 275d82c7/seal 本提交，
  feat/pending-sep02，628d2f90+275d82c7 两提交 32 files / +1053-58）：
  ①安全双件套——lib/security/injection-scan.ts 固定规则注入扫描（去零宽/HTML
  注释防绕过，high→block、medium→warn，只加警告不改原文），knowledge 普通/
  滚动注入链路打 UNTRUSTED 边界；lib/extensions/agent-loop-guard-ext.ts 同
  签名第 7 次/同工具连败 5 次阻断（第 3/5 次前置警告），注册先于 compaction
  guard；持久化指纹 compatible 重钉（server/index.ts 属守卫源）。
  ②沙盒拒绝分因——Windows 实测会话全程 operate（完整权限）档下 stage_files
  投递工作区外文件被拒，旧文案「权限级别: read_only」致模型误判为会话只读
  模式，反复请求用户切权限并原样重试；PathGuard.check 拒绝结构化（level +
  cause: outside_write_scope|blocked|unresolvable），分因文案明示「与会话权限
  模式无关」+ 两条出路（session_folders 授权目录/复制进工作区），safety-policy
  兜底改拼接不覆盖，系统提示词补反误诊指引（中英+golden 快照同步），五语言
  sandbox.denied 拆三键（deniedOutsideWriteScope/deniedBlocked/
  deniedUnresolvable）。③tenets locale 键迁移 settings.tenets* →
  settings.memory.tenets*（跟随设置页结构）。验证：typecheck×3 绿 + 定向
  176 用例绿 + 全量 npm test 12929 passed / 7 skipped（exit 0，275d82c7 树
  复跑）+ 持久化 tripwire 15/15 + locale parity 绿后推进。

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

## 2026-09-04 v0.1.33 新建会话空白/串台诊断开工回执
- 目标：store 层坐实「新建会话首屏空白」与「串台成主工作台」两症状机制,给修复清单;不动实现源码。
- 顺序：任务0基线 → 任务1空白复现 → 任务2串台复现或排除 → 任务3归因+修复清单。
- 最大风险:串台症状部分依赖 GUI/WS 时序,store 层复现不出——兜底=候选路径清单+排除理由(任务书已许可)。
- 基线(2026-09-04 本工作树):`npm install` exit 0;`./node_modules/.bin/vitest run desktop/src/react/__tests__/stores/session-actions.test.ts` → 89 passed / 0 failed / 0 skipped / 396ms,与任务书预期一致。
- 白名单:仅 desktop/src/react/__tests__/stores/ 下新测试文件 + PROGRESS.md + BLOCKED.md;不 commit 不 push。

## 2026-09-04 v0.1.33 新建会话空白/串台诊断开工回执
- 目标：store 层坐实「新建会话首屏空白」与「串台成主工作台」两症状机制,给修复清单;不动实现源码。
- 顺序：任务0基线 → 任务1空白复现 → 任务2串台复现或排除 → 任务3归因+修复清单。
- 最大风险:串台症状部分依赖 GUI/WS 时序,store 层复现不出——兜底=候选路径清单+排除理由(任务书已许可)。
- 基线(2026-09-04 本工作树):`npm install` exit 0;`./node_modules/.bin/vitest run desktop/src/react/__tests__/stores/session-actions.test.ts` → 89 passed / 0 failed / 0 skipped / 396ms,与任务书预期一致。
- 白名单:仅 desktop/src/react/__tests__/stores/ 下新测试文件 + PROGRESS.md + BLOCKED.md;不 commit 不 push。

## 2026-09-04 任务1完成:空白机制坐实

- 新文件 `desktop/src/react/__tests__/stores/session-new-session-blank.test.ts`(1 用例,名含 characterization: KNOWN DEFECT)。
- 复现流程:createNewSession → ensureSession(mock new-detached 与 switch 返回固定会话)→ switchSession 完成,全程不注入 WS 事件。
- 三事实断言全绿:welcomeVisible===false;currentSessionPath===新会话;/session/new-v033.jsonl 缓存存在且 items===[]。
- 机制证据:mock 的 /api/sessions/messages 备好了首条消息,但断言其从未被调用——stageDetachedSessionForActivation 先 initSession(path,[],false)(session-actions.ts:1155)种空缓存,switchSession hasData 判据(:864-868)为真跳过 loadMessages,首屏内容从此全靠 WS 事件;WS 事件不达(入口闸门 ws-message-handler.ts:97-140 丢身份不匹配事件/断连)即空白。
- 反向验证:翻转 welcomeVisible 断言 → 红(`AssertionError: expected false to be true`)→ 还原 → 绿。命令:
  `./node_modules/.bin/vitest run desktop/src/react/__tests__/stores/session-new-session-blank.test.ts`

## 2026-09-04 任务2完成:串台=机制b拉力坐实+流程内不可达排除清单;文件树半边机制c坐实

- 新文件 `session-new-session-crosstalk.test.ts`(2 用例)与 `desk-new-session-capture.test.ts`(1 用例,真实 store+真实 activateWorkspaceDesk),均名含 characterization: KNOWN DEFECT,各自反向验证红→绿已贴对话。
- 机制b拉力:无会话态(currentSessionPath=null ∧ pendingNewSession=false ∧ pendingSessionSwitchPath=null)下 loadSessions 把视图强切 sessions[0](主工作台会话)并加载其记录(session-actions.ts:626-635)——测试直造该状态坐实。
- 「无会话态」全部到达点静态枚举(grep pendingNewSession/currentSessionPath 全部赋值点+逐一核实):
  1. 冷启动初始形状(session-slice.ts:200-206)——loadSessions 强切即设计内 bootstrap。可达,非新建流程。
  2. archiveSession 归档当前会话(session-actions.ts:1380 清空)——:1386-1390 立即兜底切 sessions[0],窗口内 WS 触发的强切目标与之一致。可达,设计内。
  3. QuickChatApp.tsx:125 设 pendingNewSession:false——quick-chat 为独立 HTML 入口(quick-chat-main.tsx/quick-chat.html)=独立渲染进程,store 实例隔离。排除。
  4. mobile-init.ts——mobile 独立入口。排除。
  5. desk-actions.ts:295/:1331 设 currentSessionPath:null 但同 patch 带 pendingNewSessionIdentityPatch()(草稿态),受 loadSessions guard 保护。排除。
  6. 新建会话流程内部:invalidateSessionSwitches(:1175)→大 setState(:1216-1238)之间全同步无 await;switchSession :775 设 pendingSessionSwitchPath 先于首个 await(:793);:871-901 一次性翻三标志;错误路径(:805-811/:987-993)保持草稿态。⇒ 流程内「无会话态」不可达。排除。
  7. ws-message-handler 从不写三标志(只读+:114-127 locator 修复,写 currentSessionId 需 currentSessionPath===sessionPath)。排除。
  8. setCurrentSessionPath setter(session-slice.ts:224)主窗口零调用;setCurrentSessionRef(:225)仅 QuickChat。排除。
  9. app-event-actions 'agent-switched'(:121)为显式切换;loadSessions :591-607 locator 回写要求 currentSessionId 非空+currentSessionPath=null 组合,主窗口流程不出现。排除。
  10. 视图层无串台路径:grep sessions[0] 仅上述两处 store 站点;ChatArea 在 currentSessionPath=null 时渲染 null(chat/ChatArea.tsx:41)。排除。
- 结论:对话记录串台的 store 层路径(机制b)在新建会话流程内**不可达**;机制真实存在但仅冷启动/归档两设计内入口触发。「记录串台」生产候选(未排除,store 层外):服务端身份/WS 时序(new-detached 会话身份与 switch 回包不一致→闸门丢事件→空白;或 replay 重放错会话)——需服务端配合复现。
- 文件树串台=机制c坐实:createNewSession 只清三件套(:1235-1237),deskBasePath/deskWorkspaceMountId/deskTreeFilesByPath 全程不重置(crosstalk 测试);activateWorkspaceDesk 快照-恢复把清了一半的 desk 写回主工作台存档 workspaceDeskStateByRoot[root].deskFiles=[](desk-new-session-capture 测试,desk-actions.ts:394-455)。继承同工作台时旧树显示属功能意图;存档污染(回工作台丢子目录/文件列表)是缺陷。

## 2026-09-05 环境信息卡:运行信息胶囊新增 Git 四行(变更/本地/分支/提交或推送)(实现完成,未提交待 GUI 复测)

- 需求(用户截图+四点裁决):右上角「运行信息」胶囊内新增「环境信息」卡,四行=变更(行级增删合计,点击弹变更文件列表,文件点击看 diff)/本地(就地展开:主工作树 vs 分支工作树)/分支(弹层列全部分支,点击切换)/提交或推送(弹窗:提交·提交并推送·推送,提交信息留空 AI 生成,「包含未暂存的更改」默认勾选+统计,无可推送时推送置灰);明确排除创建 PR;胶囊名不改。
- 现状基线:仓库此前**零结构化 git 集成**(无 git 库依赖/无 IPC),全部新建;git 经 execFile 数组参数直跑(shell:false 防注入,GIT_TERMINAL_PROMPT=0 防挂起)。
- 后端:
  - `server/git/git-command.ts`(新):runGit/tryGit 封装+纯函数解析器(numstat -z/for-each-ref %(HEAD)/worktree porcelain)+collectGitStatus(已暂存+未暂存+未跟踪行计,未跟踪按文件行数对齐 numstat 语义,512KB 截断)/worktreeInfo/checkoutBranch/commitChanges/pushChanges/fileDiff(未跟踪合成 new-file patch)/路径与分支名防穿越防 option 注入校验。
  - `server/routes/git-environment.ts`(新):/api/git/{status,worktree-info,branches,checkout,file-diff,commit,push,ai-commit-message};dir 准入沿用 desk 惯例(agent 根/isApprovedDeskDir/isApprovedWorkspaceDir/cwd_history,symlink 解析比较);非 git 目录只读端点 200+isRepo:false 降级;ai-commit-message 收集 numstat+截断 diff+未跟踪开头,走 hub `utility:call-text`(auxiliary summarize 槽,复用 usage/trace 记账),45s 超时,输出剥围栏取首行限 100 字。
  - 注册:`full-root.ts` 挂 /api(与 desk 同层,签名 (engine,hub));`route-security.ts` 显式登记 GET→files.read/POST→files.write(不吃 STUDIO_OWNER 兜底);`server-composition-boundary.test.ts` 金名单补 `"/api" :: createGitEnvironmentRoute`(排序位 createFsRoute 与 createInputDraftsRoute 之间)。
- 前端:
  - `utils/git-env-api.ts`(新):8 端点客户端(lingxiFetch,commit 60s/push 150s 超时,操作端点结构化结果不抛错);`utils/unified-diff.ts`(新):patch→行(kind=add/del/ctx/hunk,首个 hunk 前的文件头跳过,hunk 后正文按前缀字面解析)。
  - `runtime/GitEnvironmentCard.tsx`(新):四行卡(压平皮肤),dir=deskWorkspaceNativeRoot||deskBasePath,非本地目录不渲染,非 git 仓库四行降级禁用,加载失败可点击重试;变更行千分位 +绿/-红;分支弹层(AnchoredPortal)当前分支✓/他树检出禁用/点击即切换;工作台切换自动重载。挂载点 `RuntimeInfoCapsule.tsx` SessionStatusCard 之后。
  - `runtime/GitChangesModal.tsx`(新):文件行=路径(截断+title)+该文件±统计,点击行内展开行级 diff(懒加载+会话缓存,1500 行截断,二进制/失败诚实提示);`runtime/GitCommitModal.tsx`(新):顶部分支条可下拉切换,提交信息 textarea(留空→AI 生成回填再提交),勾选行含未暂存±统计,三按钮禁用语义=提交(无可提交)/提交并推送(两者皆无)/推送(无可推送);提交并推送=先提交(若可)→刷新→再推送(nothing_to_push 静默跳过)。
  - 胶囊交互修复:portal 弹层(分支列表)打 `runtime-capsule-anchored` 标记类,胶囊「点外收起」捕获监听放行该标记,否则点分支会先塌容器卸载弹层;模态走 Overlay scope=inline(原位渲染,天然在胶囊 DOM 内不触发收起)。
  - locales 五语言 gitEnv.* 37 键(zh/zh-TW/en/ja/ko)。
  - 样式纪律:两个 module.css 首版裸间距/硬编码色违例,重写为 token+局部立法(--git-hover-wash 等收进定义行),扫描器复核我的文件 0 违例。
- 测试(全部新增或更新,61 例):
  - `tests/git-command.test.ts`(新,20):解析器纯函数+真实临时仓库(init/commit/worktree add)集成,锁定 status 汇总数字、worktree 主/从判别、未跟踪合成 patch、checkout/commit/push 契约、穿越拒绝。
  - `tests/git-environment-route.test.ts`(新,11):真实临时仓库+mock engine/hub,status/branches/worktree/checkout/commit/push 端到端,dir 校验(400/403),AI 生成净化(围栏/多行)与无 hub 503、clean tree 400。
  - `desktop/.../__tests__/utils/unified-diff.test.ts`(新,4);`__tests__/components/GitEnvironmentCard.test.tsx`(新,8)/`GitChangesModal.test.tsx`(新,6)/`GitCommitModal.test.tsx`(新,8);`RuntimeInfoCapsule.test.tsx` 更新(补 GitEnvironmentCard mock+存在性断言)。
- 验证(2026-09-05):
  - 定向:git-command 20/20,route 11/11,前端 git 四件 26/26+胶囊 4/4+unified-diff 4/4。
  - `npm run typecheck` ×3 配置 exit 0(两轮,CSS 立法重写后复跑仍绿);eslint 新文件 0 error(any 告警与既有路由文件同风格)。
  - 全量 `npx vitest run`:13090 passed / 7 failed / 7 skipped。7 失败逐一 stash 对照+记忆清单核实**全部先在**(上轮遗留):i18n parity zh-TW/ja/ko 缺 skills.panel.externalRemove+rightWorkspace.tabs.projectSkills 2 key(×3)、style-discipline 上轮 CSS(ArchivedSessionsModal/SkillsPanel/TurnUsagePills/RightWorkspacePanel,我的文件 0 违例)、release-preflight 版本号、upstream-sync-matrix VERIFIED_SOURCE_SHA 门(未提交态)、AssistantMessage.interlude 重渲染断言。
- 待 GUI 复测:①胶囊展开见「环境信息」卡四行与增删/分支/工作树类型;②变更弹窗点文件展开 diff(含未跟踪新文件);③分支弹层切换;④提交弹窗:留空提交走 AI 生成回填、勾选含未暂存、推送置灰语义、提交并推送链;⑤非 git 工作台降级文案。重启 dev server 即可(TS 源直跑,无需重构建)。
- 复测修两处(GUI 反馈驱动,2026-09-05 深夜):
  1. **目录准入漏挂载注册表**:切换器注册的工作台目录(如 pending-sep04 工作台)被 /api/git/* 403 拒绝(cherry-studio 碰巧在 cwd_history 才放行)。修复=准入根纳入 studio-mounts.json 的 active local_fs 挂载根(rootLocator.path);回归测试含 disabled 挂载不享受准入反例。live 验证:pending-sep04 工作台 → isRepo:true, feat/pending-sep04, +3,924 -3。
  2. **AI 提交信息过短(用户裁决改「标题+正文」)**:原提示词限一行≤50 字+净化只取首行。改为 Conventional Commits 提示词(首行 type(scope):≤50 字,空行,「- 」要点≤8 条每条≤30 字),maxTokens 200→500,净化函数改保留多行(剥围栏/前缀、压连续空行、整体限 600 字)。前端 textarea/commit -m 均已支持多行,零改动。路由测试更新为标题+正文断言,12/12 绿。
  - 另:Default 工作台(~/Desktop/OH-WorkSpace)非 git 仓库显示「非 Git 仓库」属预期(卡片跟随当前会话工作台)。
- 追加两点(用户第二轮 GUI 反馈,同日):**①卡可折叠**——标题行改按钮+Chevron,四/五行内容包 Collapse(对齐「本次对话」卡),弹层与模态留在 Collapse 外避免卸载;**②提交历史(VS Code 源代码管理图表风格)**——服务端新增 `GET /api/git/log`(git log --date-order NUL 分字段+\x1e 分记录,parseLogRecords 解析 %D 装饰:HEAD->/分支/远端/tag,route-security 登记 files.read);前端 utils/git-graph.ts 纯函数泳道布局(槽池制,x 位置稳定,合并/会合曲线)+ GitHistoryModal(每行=泳道 SVG[竖线/节点/HEAD 红点圆环/合并贝塞尔]+提交信息+refs 徽标[HEAD·分支/远端/tag 三色]+作者·相对时间[五语言]+右端短哈希徽标),卡片加第五行「提交记录」入口。locales 增 history/noCommits/相对时间 7 键×5 语言。测试:parseLogRecords 单测×2、泳道算法×4(线性/合并/复用泳道/空)、/api/git/log 路由(自造提交不依赖用例顺序)、GitHistoryModal 组件×5、卡片折叠+历史入口×2;git 域 8 文件 75/75 绿,typecheck×3 绿,新 CSS 零棘轮违例,eslint 0 error。live 验证:/api/git/log 对本 worktree 返回真实提交(head+remote refs、父子衔接)。
- 悬浮提示整体换自研(用户第三轮反馈:原生 title 出现无规律、内容不完整):**根因两层**——原生 title 由 OS 渲染不可控;且此前 log 只取 %s 首行,多行正文根本没传。修复:①log 格式加 %B,parseLogRecords 输出 `message` 字段(完整标题+正文,尾换行修剪);②git 域四处组件的原生 title 全部替换为共享 `ui/Tooltip`(函数子元素模式):历史弹窗标题→panel 大面板(多行 pre-wrap,悬停 500ms 必现,显示完整提交信息)、短哈希→完整 40 位哈希、变更弹窗文件名→完整路径、提交弹窗分支名/卡片分支行/主工作树路径→对应全文、分支弹层不可点项→「他树检出」提示(anchor span 包裹,disabled 按钮上 hover 仍生效);移除变更行错误 title(文案已可见)。测试:parseLogRecords 8 字段+message 多行断言、路由 message 断言、Tooltip 悬停用例×2(真实计时器等渲染→假计时器推进 500ms→tooltip 必现,mouseLeave 即隐);git 域 8 文件 77/77 绿,typecheck×3 绿。
- 哈希点击复制(用户第四轮反馈):历史弹窗短哈希徽标改 button,点击→navigator.clipboard.writeText 完整 40 位哈希+toast「已复制提交 ID」+徽标短暂显示 ✓(1.2s 回落,绿色描边反馈);Tooltip 悬停看全哈希与点击复制并存。locales 增 copyHash/copied ×5。git 域 78/78 绿。

## 2026-09-05 聊天页用量/用时胶囊任务(开工回执+任务0基线)

- 任务:主聊天 assistant 轮次操作行加「用量」「用时」胶囊+明细弹窗,1:1 复刻 `design-review/harness-usage-pills-reference/`(只读),数据=observability 账本按轮真实聚合。让步顺序:数据真实>样式一致>覆盖面>速度。
- 任务0基线(2026-09-05 实测,feat/pending-sep04,HEAD=d6fbd0d3):
  - `npm run typecheck` → exit 0 ✓(与任务书一致)
  - `npx vitest run desktop/src/react/__tests__/chat --exclude '**/dist/**'` → 11 files / 60 tests 全绿 ✓(与任务书一致)
  - `git status --short` → **68 条(63 M + 5 ??)**,任务书说 73 条(68 M + 5 ??)。差值=5 个 M 文件,已被 13:20 的提交 d6fbd0d3(归档分组)吞并,5 个 ?? 原样在列;任务书快照写于该提交之前。证据与处置见 BLOCKED.md 顶部;以当前实测 68 条为冻结基线(只多不少)。
- 基线 68 条完整快照（2026-09-05 17:42 `git status --short` 原样固化，替代 /tmp 临时文件；其中 `lib/llm/model-observability-query.ts`、`shared/model-observability-api-contract.ts` 既是基线 M 又在白名单——按任务 1 只做纯增量编辑，其余 66 条一个字节不碰）：
  <details><summary>63 M + 5 ?? 全清单</summary>

  ```
  M build/cli-runtime-closure.json
  M build/persistence-schema-fingerprint.json
  M core/engine.ts
  M core/mount-aware-file-service.ts
  M core/session-coordinator.ts
  M desktop/src/locales/en.json
  M desktop/src/locales/ja.json
  M desktop/src/locales/ko.json
  M desktop/src/locales/zh-TW.json
  M desktop/src/locales/zh.json
  M desktop/src/react/__tests__/components/ChatSidebar.test.tsx
  M desktop/src/react/__tests__/components/DeskCwdSkills.test.tsx
  M desktop/src/react/__tests__/components/RightWorkspacePanel.test.tsx
  M desktop/src/react/__tests__/components/SkillsPanel.test.tsx
  M desktop/src/react/__tests__/mobile/MobileApp.test.tsx
  M desktop/src/react/__tests__/settings/observability/ObservabilityTraceForest.test.ts
  M desktop/src/react/__tests__/settings/observability/TraceConversationModel.test.ts
  M desktop/src/react/__tests__/settings/observability/TraceDetailOverlayRendering.test.tsx
  M desktop/src/react/__tests__/stores/desk-actions.test.ts
  M desktop/src/react/__tests__/stores/desk-new-session-capture.test.ts
  M desktop/src/react/__tests__/stores/session-actions.test.ts
  M desktop/src/react/__tests__/stores/session-new-session-blank.test.ts
  M desktop/src/react/__tests__/stores/session-new-session-crosstalk.test.ts
  M desktop/src/react/__tests__/stores/session-new-session-workspace.test.ts
  M desktop/src/react/components/DeskSection.tsx
  M desktop/src/react/components/SkillsPanel.module.css
  M desktop/src/react/components/SkillsPanel.tsx
  M desktop/src/react/components/WelcomeScreen.tsx
  M desktop/src/react/components/app/ChatSidebar.tsx
  M desktop/src/react/components/desk/Desk.module.css
  M desktop/src/react/components/desk/DeskCwdSkills.tsx
  M desktop/src/react/components/right-workspace/RightWorkspacePanel.module.css
  M desktop/src/react/components/right-workspace/WorkspaceStableBody.tsx
  M desktop/src/react/settings/tabs/observability/model-observability-actions.ts
  M desktop/src/react/settings/tabs/observability/trace-detail/TrajectoryTable.tsx
  M desktop/src/react/settings/tabs/observability/trace-detail/TrajectoryTimeline.tsx
  M desktop/src/react/settings/tabs/observability/trace-detail/trace-conversation-model.ts
  M desktop/src/react/settings/tabs/observability/trace-detail/trajectory-record.ts
  M desktop/src/react/settings/tabs/skills/SkillRow.tsx
  M desktop/src/react/stores/desk-actions.ts
  M desktop/src/react/stores/desk-slice.ts
  M desktop/src/react/types.ts
  M lib/llm/model-observability-persistence.ts
  M lib/llm/model-observability-query-types.ts
  M lib/llm/model-observability-query.ts
  M lib/llm/model-observability-schema.ts
  M lib/llm/model-observability-trace-store.ts
  M lib/llm/model-trace-scope.ts
  M lib/resource-io/providers/local-fs-provider.ts
  M server/index.ts
  M server/routes/desk.ts
  M shared/model-observability-api-contract.ts
  M tests/cors-policy.test.ts
  M tests/desk-route.test.ts
  M tests/mobile-workbench-route.test.ts
  M tests/model-observability-detail-vertical.test.tsx
  M tests/model-observability-export.test.ts
  M tests/model-observability-query-truth-integrity.test.ts
  M tests/model-observability-schema-v2.test.ts
  M tests/model-observability-settings.test.ts
  M tests/model-observability-store-schema.test.ts
  M tests/model-trace-scope.test.ts
  M tests/mount-aware-file-service.test.ts
  ?? design-review/
  ?? desktop/src/react/__tests__/components/WorkspaceSwitcher.test.tsx
  ?? desktop/src/react/components/right-workspace/WorkspaceSwitcher.tsx
  ?? desktop/src/react/utils/workspace-switch.ts
  ?? tests/model-observability-session-trace-reuse.test.ts
  ```
  </details>
- 理解的目标/顺序/最大风险(≤10 行):①任务1 query 层透出 inputUncachedTokens(读 input_uncached_tokens,null 语义同现有字段)+合约类型+query 测试;②任务2 新建胶囊组件+聚合(Σ口径,缓存命中=cacheRead÷(total−output),TPS=Σoutput÷ΣdurationMs,总用时=turnProjection completedAt−startedAt),仅 completed 轮渲染,无数据不渲染,挂 MessageFooterActions,新增 5 组测试;③任务3 反向验证红→绿。最大风险:①参考包样式 1:1 复刻与项目 CSS 体系映射的保真度;②无 usage 数据的判定口径(旧会话必须不渲染);③chat 目录测试跑法含 chat-semantics/chat-performance 邻接套件,新增文件须放 `__tests__/chat/` 新文件不动现有测试。

## 2026-09-05 聊天页用量/用时胶囊任务(任务1+2+3 完成记录,未提交)

- **任务1(透出未缓存输入)**:`shared/model-observability-api-contract.ts` 的 `ModelObservabilityUsageSummary` 增加 `inputUncachedTokens?: number | null`;`lib/llm/model-observability-query.ts` 两处:call 投影 `usageOf` 产出 `inputUncachedTokens: finiteIntegerOrNull(usage.input_uncached_tokens)`(该列本就在 USAGE_INTEGER_FIELDS 腐败检测闭集内,负值整行 corrupt 语义自动覆盖)、Trace 聚合 summary 补 `sumKnown("inputUncachedTokens")`。**合约字段设为可选的原因**:`ObservabilityTraceForest.test.ts`/`TraceConversationModel.test.ts` 等现有测试以显式类型注解构造 `ModelObservabilityCallListItem` 字面量,必填字段会炸冻结测试的 typecheck(现有测试文件不许改);服务端投影始终产出该键,null=无事实语义同其余字段。query 层测试新文件 `__tests__/chat/usage-uncached-input-query.test.ts` 4 用例(透出与总输入分离/NULL→null 不冒充 0/负值 corrupt/无 usage 行 unknown)。
- **任务2(胶囊+聚合+挂载)**:新文件 `components/chat/turn-usage.ts`(聚合+格式化:Σ 口径、缓存命中 1:1 移植 formatCacheHitPercent 防 99.95→100 失真算法+正边界进位、紧凑 K/M、千分位、整秒时长、TPS;`turnUsageWindow` 资格=AssistantTurnStatus==='completed'+起止时间戳齐备)、`TurnUsagePills.tsx`(双胶囊+portal 弹窗:锚定上方 12px 钳位、Esc/外点关闭、行按数据有无条件渲染、中文文案组件内常量照抄 locale-keys.zh.txt、内联 SVG 图标、TTFT 行不移植)、`TurnUsagePills.module.css`(参考包视觉规格 1:1,颜色/圆角/动效换项目变量 --text/--text-muted/--bg/--border)、`use-turn-usage-stats.ts`(POST /api/model-observability/query/calls,filter=sessionPath+since/until(绑定 started_at,since 含/until 不含),失败静默=null)。挂载:`MessageFooterActions.tsx` 加 `statsNode` 插槽(时间文本之后);`AssistantMessage.tsx` 完成轮才启用 hook。**适配说明**:①输入/输出行在无事实时也隐藏(参考包类型保证非空故无条件渲染;本项目全可空,数据真实>样式一致);②formatCacheHitPercent 加 `cacheRead>=prompt → '100'` 前置钳位(参考包对 read>prompt 会落入防失真分支产出无意义 99.x);③胶囊标签保留 ' tok' 后缀(与参考包 consumed='用量 {total} tok' 一致)。
- **任务2 验收 5 组**:a 有数据渲染双胶囊(紧凑总量+整秒用时)/b totalTokens 无事实整体 null 且不出现 0(+资格面:streaming/failed/aborted/缺时间戳无胶囊)/c 弹窗行条件渲染(缓存写入>0、缓存命中、其中推理、TTFT 永不、TPS 按有无)/d 聚合求和与命中百分比(含 9990/10000→99.9 不上 100、5.95% 正边界进位 6)/e 三 call 混合 null 求和+模型标签去重保序+TPS 分子分母同源。新测试文件 3 个共 23 用例:query 4+aggregate 11+pills 8。
- **验证(2026-09-05 18:14 本工作树)**:`npm run typecheck` exit 0(注:此前两轮用管道 tail 取 $? 的写法会吃到 tail 的退出码,本轮起改 `npm run typecheck; echo $?` 直取);`npx vitest run desktop/src/react/__tests__/chat --exclude '**/dist/**'` → 14 files/83 tests 全绿 skip=0(基线 60+新增 23);邻接套件 AssistantMessage 渲染方(AssistantMessageCompletionActions/automation-suggestion/skill-block/media-generation/block-renderers/chat-semantics/process-fold/computer-app-approval/SessionCollabDraftCard/session-file-expired/MobileApp)52+86 用例全绿;eslint 改动文件 0 error(AssistantMessage 2 warning 为 HEAD 既有行号平移,新文件 0 警告)。
- **任务3 反向验证**:把 aggregate 测试 e 组 `uncachedInputTokens` 断言 200→故意 201 → `Tests 1 failed | 10 passed`(`expected 200 to be 201`)→ 还原 → chat 全套 83/83 绿。
- **并发改动提示(非本任务所为)**:17:42(本任务抓基线同分钟)另有会话/用户在本工作树改了 4 个白名单外文件:ArchivedSessionsModal.tsx/.module.css/其 test(+批量恢复 switchTo 选项)、stores/session-actions.ts。本任务未触碰、未回滚;基线 68 条完整性经 comm 比对全部原样在列。

- **GUI 复测反馈修复(2026-09-05 19:13,胶囊不显示)**:用户实测看不到胶囊。根因坐实——`AssistantTurnProjection` 类型虽有 startedAt/completedAt,但三个 `projectAssistantTurn` 调用点(流式 `use-stream-buffer.ts:410`、收尾 `:512`、历史 `history-builder.ts:665`)**全都不传**这两个字段,`turnUsageWindow` 在真实应用永远 null,胶囊成死代码;单测 fixture 自带时间戳故未暴露。修复(全白名单内):①`use-stream-buffer.ts` commitLiveRun 收尾时传入 startedAt=本条 assistant 之前最近 user 消息 timestamp、completedAt=收尾时刻 Date.now()(本轮全部模型调用 START 于两者之间,与账本 since 含/until 不含、绑定 started_at 口径对齐);②历史重建路径(history-builder 在 utils/,非白名单不可改)改在消费端兜底——`turn-usage.ts` 新增 `turnUsageWindowFromNeighbors`(上一条 user 时刻~本条 entry 时刻,含状态门槛与越序防护),`AssistantMessage.tsx` 投影窗口缺失时回退;③补 4 用例(回溯跳过 interlude/三种状态门槛/缺时间戳或时序倒挂拒绝/找不到消息)。验证:`npm run typecheck` exit 0;chat+semantics+performance 14 文件 87 用例全绿(83+4);eslint 新增 0 warning(use-stream-buffer 的 turnKeyFrom 1 条为 HEAD 既有)。注:实时占位消息的 timestamp=流开始时刻,不能当轮结束用,故历史/实时两条腿缺一不可。
- **GUI 复测反馈(2026-09-05,归档后工作台自动切换)——非本任务改动导致,已归因待裁决**:机制在 `session-actions.ts:1429-1431`(archiveSession):归档当前会话清空 currentSessionPath 后,`sessions.length===0` 才建新会话,否则**无条件 `switchSession(updated.sessions[0].path)`**——sessions 是跨工作台全局列表,于是默认工作台归档→跳到全局最新一条(常在别的工作台);别的工作台归档→跳回(常是)默认工作台。该行代码出自 08-05 基线提交 d5275e56,HEAD 原样存在,本任务与 17:42 并发改动均未触碰;近期侧栏归档入口 UX(d6fbd0d3)让它更易触发。且 `tests/session-actions.test.ts:2390/2446` 有用例锁定该行为,修复需改冻结测试+非白名单文件,方向(归档后留空白草稿/按当前工作台过滤候选)待用户裁决后另行开工。

- **GUI 复测反馈(2026-09-05,归档后工作台自动切换)——用户拍板「把问题一也修复了」,已修复**:机制=`session-actions.ts` archiveSession 归档当前会话清空 current* 后无条件 `switchSession(sessions[0].path)`(08-05 基线 d5275e56 既有);且 `loadSessions` 内部「首次加载」兜底(currentSessionPath 空时拉 sessions[0],626-635)会让「只删显式跳转」失效。**修复**(用户授权覆盖冻结边界):归档当前会话或草稿态归档旧会话 → 先 `createNewSession()` 回「新建聊天」草稿态——在置空 current* 前读取被归档会话的工作台归属做继承(规则 B 新建跟随当前),写入的 pendingNewSession 挡住 loadSessions 的 sessions[0] 兜底;归档后台会话(正开着别的会话)行为不变。锁旧行为用例(session-actions.test.ts「归档当前 session」)改写为新语义断言(currentSessionPath=null + pendingNewSession=true + 列表里的 '/other' 不得成为当前会话)。**红→绿**:旧实现+旧 mock 队列 → `AssertionError: expected '/other' to be null`(1 failed | 88);新实现+新队列(补 createNewSession 的 permission 默认值请求 mock)→ 89/89 绿。注意供数陷阱:permission mock 插在 /api/sessions 之前会被旧实现的 loadSessions 当列表消费掉,sessions=[] 恰好走旧「空列表→createNewSession」分支,红证必须用旧队列(已踩坑并记录)。**验证**:`npm run typecheck` exit 0;stores+WelcomeScreen+ChatSidebar+ArchivedSessionsModal+MobileApp+app-init 36 文件 511 用例全绿;eslint 0 error(10 warning 全为该文件基线既有);基线 68 条 git status 原样在列。

- **GUI 复测反馈第二轮(2026-09-05 20:40,「还是看不到胶囊」→ 三重真因全部修复,真机验证通过)**:用户以 `sess_0mtob4efv_a9f834bb2c789bd5afce` 实测仍无胶囊。逐层排查:①账本数据✓(5 调用全 present,session_id/session_path 双写,Σtotal=108,649);②服务层以胶囊精确 filter 探查✓(tsx 直连 sqlite,5 calls 全返回,inputUncachedTokens 在);③真实 UI 悬停后 AX 树无胶囊节点→渲染端问题。**真因三层**:⑴ `desktop/main.cjs loadPageFromDir` 只有设 `VITE_DEV_URL` 才走 vite,`--dev` 实际加载 `desktop/dist-renderer/` 构建产物(时为 16:12 构建,早于全部改动)——5173 上的 vite 无人消费,重启应用无效,**必须 `npm run build:renderer`**;⑵ `chatSessions` 以 sessionScopedKey(sessionId 优先)为键(chat-slice:72),我最初的裸 path 选择器在真实会话(有 sessionId)必查空——历史邻居回退全断,改用 `sessionScopedValue` 作用域查找;⑶ assistant entry timestamp=回复**开始**落盘时刻(实测 19:36:28,轮实际至 19:37:34),历史窗口上界不能用它——改为轮次边界「上一条 user 时刻 ~ 下一轮 user 时刻−1,最后一轮=now」;随之发现展示用时也不能用该边界(会把闲置时间算进「用时」),历史轮 runMs 改由账本事实推导=本轮最后调用 ended_at−startedAt,null 时用时胶囊整体隐藏。实时路径(commitLiveRun 写投影时间戳)不变。**新增 turn-usage-mount.test.tsx 4 用例**(mock lingxiFetch 模块边界,不 mock 被测组件):完成轮投影带时间戳→发查询+渲染双胶囊/生产同款 sessionId 键+locator 邻居回退→查询窗口 since/until 断言/账本空→无胶囊不渲染 0/streaming 轮不发请求;另补邻居窗口边界语义 5 断言(含 fake timers 锁「最后一轮=now」)。**验证**:`npm run typecheck` exit 0;chat+semantics+performance 15 文件 95 用例全绿;eslint 0 error(4 warning 均 HEAD 既有);`npm run build:renderer` 后重启应用,真机(同一会话)AX 树+截图证实「用量 109K tok」「用时 1分27秒」与账本全轮数据吻合,弹窗行(模型/缓存命中/未缓存输入/缓存读取/输出+其中推理)齐全。

- **GUI 复测反馈第三轮(2026-09-05 21:00,用户两点追加)**:①「用时和速度」弹窗补**首 token 用时（TTFT）**行——推翻早前「TTFT 不显示」的猜测性裁决;口径=本轮最早一次 provider 响应到达(first_response_at)−轮开始(用户消息时刻),聚合取 min,事实缺失/时钟倒挂→null 整行不渲染;新增 formatLatencySeconds 移植(<10s 一位小数,≥10s 取整)。②胶囊与时间文本的显隐行为统一——最新消息 persistent 场景下,原先胶囊/时间恒显而复制/截图等按钮悬停才显,视觉不一致;`MessageFooterActions` 给 statsNode 加 `.messageFooterStats` 包装,Chat.module.css 新增 persistent 场景下 time+stats 默认 opacity:0/pointer-events:none、消息组 hover/:focus-within/行 hover 显示,与操作按钮完全同规则。**验证**:`npm run typecheck` exit 0;chat 三套件 15 文件 98 用例全绿(新增 TTFT 推导/TTFT 缺失行隐藏/最后一轮 now 边界 fake-timers/persistent 包装类等断言);eslint 0 error;`npm run build:renderer` 后重启应用,真机 AX+截图证实:用时弹窗出现「首 token 用时（TTFT）1.2秒」(真实账本值),非悬停态消息页脚(含时间/胶囊/按钮)整体隐藏。

## 2026-09-05 修复轮七:新建聊天助手身份跟随当前(规则B补全,用户拍板「B 方向」;接续修复轮与 01cdd80b mobile 补钉线索,非 disposal 工作流)

- 背景:mobile 用例补钉(01cdd80b)后用户追问「助手和工作台不是没有绑定吗,为什么要区分」——确认架构上 agentId 与工作目录本是独立维度,「全局新建重置回 Primary」是历史入口语义,轮一实现规则 B 时只改了工作台维度、助手维度经代码注释有意保留,产生「助手回 Primary + 工作台留当前」的不对称组合。用户裁决 B 方向:**助手身份也跟随当前助手**。
- 改动(session-actions.ts createNewSession,单点):
  - `selectedPrimaryAgentId`(无条件钉 Primary)改 `selectedAgentIdForDraft`——有 currentAgentId 时为 **null**(null=「跟随当前」:欢迎页 displayAgent 取 selectedAgentId||currentAgentId,建会话体 buildPendingSessionCreateBody 仅在 selectedAgentId≠currentAgentId 时显式带 agentId,与 handleSelectHistory 的 null 约定一致);仅无当前助手时才显式落 Primary 兜底。
  - setState 注释同步(「全局新建仍回到 Primary Agent」→「助手与工作台都不重置回 Primary」)。
  - 服务端语义核实:new-detached 省略 agentId 时 coordinator createSession 回落 `this._d.getAgent()`(当前活跃助手)——省略即跟随当前,桌面主线路径(单助手=current=Primary)本就走 null 形态,本次只是把多助手场景并入同一形态。
- 测试(4 处翻转,红→绿完整对证):
  - session-actions.test.ts ×3:两条 `selectedAgentId==='hana'` 断言翻 toBeNull(其一用例名从「resets the agent to primary」改为「follows the current agent」);「carries an explicit project id」用例请求体期望删去 `agentId:'hana'`(wire 形态锁定:省略=跟随当前,响应 echo 同步 'mio')。
  - MobileApp.test.tsx ×1:01cdd80b 翻过的用例再翻助手半边(toBeNull),用例名改「…and current agent」。
  - 红:还原实现后 4 failed(3× `expected 'hana' to be null` + 1× 请求体不匹配);恢复后绿。
- 验证:stores 全目录+mobile+WelcomeScreen+session-sections+app-init **37 文件 510 用例全绿**。
- **未提交**:工作树同期有并行会话在途改动(disposal 工作流:session-coordinator/sessions 路由/ArchivedSessionsModal/ChatSidebar 等,session-actions.ts 亦被其改过但区域不冲突),本轮改动与之共存待统一提交;用户本轮未下达提交指令。

## 2026-09-05 修复轮五:Windows 初始对话界面工作台显示双缺陷(用户 bug 报告 2026-09-04)

- 报告两症状+一次生机制,定位全部核实后修复(纯显示层+服务端创建护栏,不动工作台切换/文件功能):
  1. **症状一(指示行/列表显示整条路径)**:WelcomeScreen 三处手写 `split('/').pop()` 取名(指示行 folderName、本次工作台列表项、额外文件夹列表项)对 Windows 反斜杠路径失效。改用 shared/workspace-history.ts 既有 `workspaceDisplayName()`(先归一分隔符再取末段);本次工作台列表项本就经 buildWorkspacePickerItems 归一(红证在指示行与额外文件夹两处),额外文件夹列表(workspaceFolders)是原生路径直渲染,一并治。
  2. **症状二(同一工作台出现两次)**:下拉两来源(挂载 studioWorkspaces + 历史/主目录 buildWorkspacePickerItems)直接拼接、无跨源去重——同目录既是挂载又在 cwd_history 时渲染两行。FolderHistory 渲染前对历史/主目录条目按 `isSameWorkspacePath()`(反斜杠+Windows 大小写归一)与可见挂载的 nativeRootPath 跨源比对,命中即只留挂载行(带 label 与挂载移除钮)。Agent 主目录条目只与(隐藏的)默认挂载同根,不受影响。远端 principal 拿不到 nativeRootPath 时不比对(无从比对,维持两来源并列)。
  3. **次生机制(Windows 大小写变体挂载)**:`localFsMountId` 派生不做大小写归一,同目录大小写变体各造一条 active 挂载。server/routes/studio-workspaces.ts createLocalPathWorkspace 创建前按「字符串不同但折叠后同根」(win32 下 resolve+lowercase,POSIX 恒 false)找既有 active local_fs 挂载,命中复用;完全相同字符串仍走 upsert(保留重加改 label 既有语义)。不改 mountId 派生本身——存量挂载的 mountId 被会话 meta 引用,改派生会孤立既有身份。
  4. **继承链归一**(报告 触发链路①):session-actions.ts createNewSession 的 inheritedLocalFolder/keptSelectedFolder/keptDeskFolder 由 `.trim()` 改 `normalizeWorkspacePath()`,与 applyFolder 落 selectedFolder 的规范形态一致,反斜杠原生 cwd 不再进入前端草稿态。
- 测试(红→绿:临时还原四处实现改动→5 用例红,恢复→全绿):
  - WelcomeScreen +3:指示行反斜杠取名 / 挂载-历史跨源去重(红证:两行 nest-drama `expected length 1 but got 2`)/ 额外文件夹反斜杠取名。
  - session-new-session-workspace +1:继承反斜杠 cwd 落 selectedFolder 前归一(红证:`expected 'C:\Users\...' to be 'C:/Users/...'`)。
  - studio-workspaces-route +2:win32 桩下大小写变体复用既有挂载(红证:`expected 'local_fs_be8c76a96653d5e1' to be 'mount_case'`,registry 仅 1 条 local_fs)/ 精确重加仍走 upsert 更新 label(锁定护栏不破既有语义)。
- 验证(2026-09-05 本工作树):定向 3 文件 29 用例绿;桌面 stores+WelcomeScreen+session-sections+app-init 475 用例绿;服务端 tripwire+workspace 相关 6 文件 146 用例绿;http-route-security 27 绿;`npm run typecheck` exit 0;eslint 改动文件 0 error(12 warning 全为既有 no-explicit-any,行号平移)。desktop 全量 `__tests__/`:2461 过/1 败(败者=既有 DeskSection Jian drawer 用例)+31 文件级环境性失败(workspace 包 @lingxi/plugin-protocol 未构建,均前轮 stash 对照证实的先在项),与基线一致零新增。
- 本轮触碰文件均不在 163 个持久化受护源内;check 脚本对 engine.ts 的未重钉报错为修复轮二遗留状态(tripwire 15/15 仍绿),本轮未新增受护源改动。

## 2026-09-05 修复轮七:归档分组支持折叠(用户反馈:分组下记录要能整组收起)

- ArchivedSessionsModal.tsx:分组头改为可点击折叠/展开——`collapsedGroups: Set<key>` 状态(key=mount:/path:/ungrouped,列表刷新后保留);组头 role=button+aria-expanded+Enter/Space 键盘切换;行内勾选与删除整组按钮 stopPropagation 不误触折叠;折叠时组头(含勾选/删除/徽标/统计)保留、仅收起记录行。CSS:组头手型光标+chevron 箭头 90° 旋转动画(token 风格)。
- 测试:+2 用例(点击组头整组收起/再展开+aria-expanded 翻转;删除按钮不触发折叠)。红→绿:stash 组件后折叠用例红(`expected null to be truthy`),恢复绿;全套 18/18,邻接套件 461 绿,typecheck 0,eslint 0 error。

## 2026-09-05 修复轮六:移除工作台简化为直接归档(用户裁决撤销二选一)

- 用户追加裁决:移除工作台**不再弹二选一,直接归档**。改动:
  - `WelcomeScreen.tsx`:删除 WorkspaceDisposalDialog 挂载/处置状态/handler;handleRemoveWorkspace 简化为——0 条直接移除;>0 先静默调 disposeWorkspaceSessions('archive'),成功后 removeStudioWorkspace + 成功 toast(「工作台已移除,N 条对话已归档」),失败则 error toast 且不移除;removingMountRef 防重入。
  - 删除组件文件 WorkspaceDisposalDialog.tsx/.module.css。
  - 5 语言清理 workspace.disposal 下无用键(title/count/hint/archive/archiveDesc/delete/deleteDesc/deletedToast),保留 archivedToast/failed;服务端 disposal 路由的 delete 档保留为 API 能力(归档界面的整组永久删除仍走既有 archived/delete 路由,不受影响)。
  - 测试:WelcomeScreen 二选一用例改写为「直接归档、无对话框」(断言 disposal 立即以 action:archive 调用+DELETE 顺序+无对话框文本);零会话用例改名。旧对话框行为下 disposal 不会先于选择被调用,该断言天然构成回归锁。
- 验证:WelcomeScreen 17/17;stores+归档界面+app-init+SecurityTab+服务端 disposal/sweep+i18n 共 475 用例绿;typecheck exit 0;eslint 0 error 0 warning;无悬挂引用(WorkspaceDisposalDialog/disposed keys 全仓无残留)。

## 2026-09-05 修复轮五:移除工作台的对话处置 + 归档界面工作台分组 + 入口迁移(用户四点裁决全落地)

- 用户裁决:移除工作台时对话二选一(归档/永久删除),无保留档、无孤儿桶;绕过移除的孤儿(目录被直接删/mount 失效)静默自动归档不弹框;归档界面按工作台分组+已移除徽标+整组删除;入口迁到侧栏功能行垃圾桶按钮,设置安全页旧入口移除;重新添加同路径工作台时提示可恢复的归档数。
- 服务端(server/routes/sessions.ts):
  - 抽取 `archiveActiveSessionCore`(单条归档路由的锁内序列,单条路由改走它,32 用例无回归);
  - 新路由 `POST /api/sessions/workspace-disposal`:{workspaceMountId|cwd, action:archive|delete};身份口径与左栏一致(mount 严格+native 根路径 cwd 双形态);delete=归档后复用永久删除内部序列;流式会话跳过计数返回;
  - 新路由 `POST /api/sessions/sweep-orphaned-workspaces`:mount 失效或 cwd 目录已从磁盘删除的会话静默自动归档(目录仍在但未引用的不清——换配置目录的残留可经重新打开找回,不算暗数据);
  - `core/session-coordinator.ts` listArchivedSessions 行投影补 cwd/workspaceMountId/workspaceLabel(从 manifest workspaceScope/meta/列表投影缓存,存量归档可正确分组)。
- 客户端:
  - `session-actions.ts`:+disposeWorkspaceSessions/sweepOrphanedWorkspaceSessions/countArchivedSessionsForWorkspace;ArchivedSession 类型补三字段;
  - `WelcomeScreen.tsx`:移除前双形态计数,0 条直接移除,>0 弹 `WorkspaceDisposalDialog`(新组件+CSS,二选一);处置成功后才 removeStudioWorkspace;handleBrowse 重新添加后查归档数给提示;
  - `ArchivedSessionsModal.tsx` 重写:按 mount/cwd/未归属三型分组;default 组显示名=配置目录名(与主界面同规则);身份解析不到现存工作台 → 「该工作目录已移除」徽标(无身份组不标);组级勾选+删除整组;组内保持时间排序与单条操作;
  - `app/ChatSidebar.tsx`:功能行新增垃圾桶按钮(设置按钮旁)打开归档界面;`SecurityTab.tsx` 移除归档区块/挂载/state;`app-init.ts` 工作台列表加载后接清扫(归档了东西发非阻塞 info toast);
  - 5 语言:删 settings.security.archivedChats* 三键,新增 sidebar.archivedChats、session.archived.group.*/deleteGroup*、workspace.disposal.*、workspace.sweep.archivedToast、workspace.archivedHint。
- 测试:新 tests/workspace-disposal-route.test.ts 7 用例(身份拒绝/双形态归档/删除/流式跳过/default mount 解析/清扫两态);WelcomeScreen +2(二选一对话框全链路/零会话直接移除);ArchivedSessionsModal +3(分组+徽标+default 显示名/整组删除/组级勾选)+1 条既有用例更新(checkbox 顺序因组级勾选变化);SecurityTab 1 条改为锁定入口移除;app-init 断言清扫调用。
- 红→绿:stash 全部实现后 13+ 用例红,恢复后 56/56 绿。
- 验证:服务端+路由+i18n 152 用例绿;客户端相关套件 496 用例绿;typecheck exit 0;eslint 改动文件 0 error(sessions.ts +1 条与既有 6 条同款的防御性空 catch warning)。

## 2026-09-05 修复轮四:默认工作台显示名规则 + 启动合流键可靠性(用户拍板:始终显示配置目录名,未配置才显示 Default)

- 用户复测图证:启动时工作台名=配置目录名但列表缺 Default 期记录;发首条消息后工作台名翻成 "Default"、记录才齐。用户细化规则:**启动与运行期显示名一律=设置里配置的工作台目录名;"Default" 仅在未配置任何目录时显示**。
- 改动:
  1. **显示名派生**(desk-actions.ts):新增 `defaultWorkspaceDisplayName()`——取 store.homeFolder(显式配置信号,未配置为 null)的目录名,未配置回落 "Default";**不用服务端解析根路径推导**(未配置时服务端根回落内置目录,目录名非用户所愿)。两个咽喉点覆写:`applyStudioWorkspace`(mountId='default' 时 label 一律派生,selectedWorkspaceLabel/deskWorkspaceLabel 同源)与 `activateWorkspaceDesk`(同规则覆写 options.label——switchSession 恢复 desk、发送后翻转等一切路径经此)。全部显示面(DeskSection 面板标题:164/WorkspaceStableBody:47/SessionStatusCard:62/选择器按钮)读 deskWorkspaceLabel 或 selectedWorkspaceLabel,均被覆盖。WelcomeScreen 两处调用去掉硬编码 'Default'。非 default mount 严格保留调用方标签。
  2. **启动合流键可靠性**(app-init.ts):引导期(agent config 就绪后)`void loadStudioWorkspaces()`——defaultWorkspaceRootPath 不再依赖 FolderPicker 挂载时的一次性请求,启动首屏左栏即可做 mount≡cwd 双形态合并(修图一「启动列表缺一整本账」)。
- 效果:启动态名称=配置目录名(本地形态 basename)+列表两本账合并;发送后 desk 翻成 default mount 但显示名仍=配置目录名——**两态名称与列表一致,不再出现「先空白、发一条才齐/名称跳变」**。
- 测试:desk-actions +4(派生覆盖服务端标签/未配置回落 Default+反斜杠与空白边界/非 default 保留标签/switch 恢复咽喉点);app-init +1(引导期调用 loadStudioWorkspaces)+mock 面补齐。红→绿:stash 实现后 5 红(含轮三的根捕获),恢复全绿。
- 验证:stores+WelcomeScreen+session-sections+app-init 471 用例全绿;typecheck exit 0;eslint 改动文件 0 error 0 warning。

## 2026-09-05 修复轮三:默认工作台与 Agent 工作台目录「同目录两本账」合流(用户拍板 1+2+3 全做)

- 背景:用户确认 Default 工作台(内置 mount "default")与设置页 Agent 工作台目录是同一目录的两个入口,但对话分裂成两本账——会话身份键有两种形态(mount 形态=经切换器/挂载创建,带 workspaceMountId;cwd 形态=经目录历史/旧版本创建,只带 cwd),左栏作用域对两种形态严格互斥(session-sections.ts 原 :170-175)。
- 三项改动:
  1. **入口归一**(WelcomeScreen.tsx):目录历史选 Agent 主目录(handleSelectHistory)与欢迎页 Agent 芯片(AgentChips)两个本地形态入口,统一改走 `applyStudioWorkspace({mountId:'default'})`;AgentChips 保留「解析到同一目录不重载 desk」优化(比较改为按解析后的工作台身份,selectedWorkspaceMountId==='default' 时取 defaultWorkspaceRootPath 对比)。跨 Agent 选择时 desk 的 mount 根在会话落到目标 Agent 前按当前 Agent 解析,属草稿期瞬态,首条消息后随 switch 回包归位(注释已记)。
  2. **作用域归一**(session-sections.ts):WorkspaceScope 增可选 `defaultRootPath` 合流键;resolveWorkspaceScope 在「desk/pending 落在 default mount」或「本地路径 === 默认根」时携带;sessionBelongsToWorkspaceScope 双向放行——default mount 作用域收 cwd 指向该根的旧形态会话,该根的本地作用域收 mount "default" 会话;其余 mount 保持严格互斥。store 新增 `defaultWorkspaceRootPath`(desk-slice),loadStudioWorkspaces 从 isDefault 条目的 nativeRootPath 捕获(列表本体保留 Default 条目——预览/文件刷新按 mountId 查找依赖它);SessionList 接线传入。
  3. **切换器隐藏 Default**(WelcomeScreen FolderHistory):渲染过滤 isDefault 条目(仅 UI 层;同一目录经历史/主目录条目进入且已是 mount 形态)。
- 测试:session-sections +4 用例(双向合流/非 default 仍严格/合流键仅按需附着);desk-actions +1(默认根捕获+列表保留);WelcomeScreen +2(历史选主目录→mount 形态、Default 行隐藏且 store 保留)并更新 2 条锁定旧本地形态的用例(agent 芯片选择→断言 mount 形态);「同目录不重载」用例原样通过。
- 红→绿:stash 全部实现改动后 8 用例红(4 作用域+1 捕获+2 更新+1 历史入口),恢复后全绿。
- 验证:stores 426 / session-sections 16 / WelcomeScreen 12 全绿;components 目录 798 过/1 败(先在的 DeskSection Jian drawer 用例,与本改动无关,前轮已 stash 对照证实);typecheck exit 0;eslint 改动文件 0 error(顺手清了 WelcomeScreen 失时效的 any-disable 与 unused import,warning 从 HEAD 3 → 0)。
- 效果预期:同一目录(=默认工作台=Agent 工作台目录)的 mount 形态与 cwd 形态会话在左栏合并显示;不再产生新的 cwd 形态会话(入口全部走 mount);切换器不再出现与目录历史重复的 Default 行。旧 cwd 会话无需迁移即并入。

## 2026-09-05 修复轮二:左栏列表空白三症状的真根因(engine 未暴露 getSessionWorkspaceMount)

- 用户复测反馈:修复轮一只治好「新建聊天拽回默认目录」,症状1/2/3(新对话后记录区空白、点新建聊天列表才出现、点进记录列表又清空)原样未动。
- 重新定位(静态链+真服务器探针辅助):
  - **switch 路由的工作台身份回传只信 `engine.getSessionWorkspaceMount`**(server/routes/sessions.ts:2616 经 sessionWorkspaceMountFields,与 create 路由不同、无 workspaceSelection.mount fallback);
  - **真实 engine 只暴露了 getSessionWorkspaceFolders/getSessionAuthorizedFolders 两个委托**(core/engine.ts:2117-2122),漏了 getSessionWorkspaceMount——路由 optional-call `engine.getSessionWorkspaceMount?.()` 静默拿到 undefined;
  - ⇒ **switch 回包永远不带 workspaceMountId/workspaceLabel** ⇒ 客户端 resetDeskForSessionWorkspace(cwd,mountId=null) → desk 落本地目录键;
  - 而会话列表投影带 mountId(engine 侧直接读 meta,core/session-coordinator.ts:6664-6666;new-detached 落库即写 meta :2661-2663);
  - 客户端作用域谓词:本地作用域**严格排除**带 mountId 的会话(session-sections.ts:173)→ 活跃会话态左栏必然空白(症状1/3);草稿态取 selected*(mount)→ 列表正常(症状2「点新建聊天才显示」)。三个症状一个根因。
  - 路由契约本有测试锁(tests/sessions-route.test.ts:127/:169-170,mock 的 engine 带该方法)——证明设计意图就是 engine 暴露它,真实 engine 漏配。
- 修复:core/engine.ts 补一行委托 `getSessionWorkspaceMount(p){ return this._sessionCoord.getSessionWorkspaceMount(p); }`(紧随两个兄弟委托)。
- 回归测试:tests/engine-session-workspace-mount.test.ts 2 用例(存在性+委托传参/默认参)。红→绿:`git stash push -- core/engine.ts` 后 2 红(`expected 'undefined' to be 'function'`),恢复后 2 绿。
- 验证(2026-09-05):tripwire 15/15;路由+引擎+协调器定向 133+47 用例绿;stores 425 绿;typecheck exit 0;eslint engine.ts 0 error 且 warning 数与 HEAD 一致(129 行含汇总)。探针脚本(/tmp,未入库)确认了 409/清单/模型门等服务端行为,静态链闭合后不再依赖。
- 链条覆盖:coordinator meta 读(既有)→ engine 委托(新测试)→ 路由发射(既有测试)→ 客户端 desk 恢复(desktop stores 测试)。

## 2026-09-05 修复轮:新建会话工作台语义(用户拍板=规则B「跟随当前工作台」,批准动手)

- 用户经三轮描述确认病灶全貌(见 BLOCKED.md 2026-09-05 条目)并批准修复。改动三处+文案:
  1. **createNewSession 继承源重排**(session-actions.ts:1199-1240):无当前会话时继承「当前显示的工作台」——草稿选择(applyFolder/applyStudioWorkspace 写入的 selectedFolder/selectedWorkspaceMountId)优先,其次 desk 已激活身份(deskWorkspaceMountId/deskBasePath,仅本地目录态取路径);两者皆空才落 Primary Agent 工作台(设置页默认语义保留)。Primary 兜底加「未选 mount」门,防 selectedFolder 被 Primary 路径污染。修:症状4(其他工作台点新建聊天被拽回默认)、Default 下列表显示设置目录记录(显示作用域与数据归属分家)。
  2. **去掉空缓存种子**(原 :1155):stageDetachedSessionForActivation 不再 initSession(path,[],false),switchSession 走 !hasData→loadMessages 真实拉历史并 stamp revision。修:症状1(新会话消息区空白——不再依赖 WS 事件)。
  3. **loadSessions 尾部补 reconcile 触发**(:636-641):列表刷新后校验当前会话缓存 revision,落后即补拉——闭合「列表说磁盘前进了、缓存永远不追」的自愈缺口(桌面端此前仅 chat-find-locate/移动端触发)。修:「退出重进才恢复」类残留空白。
  4. **文案对齐**(5 locales homeFolderDesc,如 zh.json:2317):「新建对话跟随当前打开的工作台;未打开其他工作台时默认使用此目录,巡检和定时任务也在这里执行」。
- 测试:
  - 更新锁定旧行为的现有用例 1 条(session-actions.test.ts「without a current session…」:期望从落 Primary 改为保持 desk 显示的目录);为「posts one new-session request」补 messages 端点 mock(断言未动)。
  - 新增 session-new-session-workspace.test.ts 5 用例:mount 草稿保持/folder 草稿保持/仅 desk 身份保持/Primary 兜底保留/reconcile 自愈链。
  - session-new-session-blank.test.ts 由 KNOWN DEFECT 表征翻为 regression: FIXED(锁定历史真实加载)。
- 红绿证据:`git stash push -- session-actions.ts` 后新回归 5 用例中 4 例红(第 5 例 Primary 兜底新旧行为一致故两态皆绿),pop 恢复后全绿。
- 验证(2026-09-05 本工作树):
  - `./node_modules/.bin/vitest run desktop/src/react/__tests__/stores/` → **31 文件 / 425 用例全绿**(原 89 基线含于其中)。
  - `./node_modules/.bin/vitest run desktop/src/react/__tests__/` → **2445 passed / 1 failed**;失败集合与「无我的改动」基线逐文件一致(1 例先在的 DeskSection Jian drawer 用例+31 个文件级环境性失败=workspace 包 @lingxi/plugin-protocol 未构建,均先在,stash 对照证实)。
  - `npm run typecheck` → exit 0(root/node/test 三段);i18n parity 5 用例绿;eslint 改动文件 0 error(实现文件 warning 数与 HEAD 一致=1)。
- 未动(维持已诊断待办):机制c desk 存档污染(desk-new-session-capture.test.ts 仍锁定该缺陷,P1-3 清单在案)、机制b 强切无会话态(P2-5 纵深防御,枚举已证不可达)。

## 2026-09-04 任务3完成:归因结论与修复清单(不动手,仅清单)

### 归因(两症状各一句)

1. **空白=架构层为主,ea03c627 是触发器而非根因**:「会话身份(switch 完成)/消息缓存(种子空缓存+hasData 短路)/内容来源(WS 事件流)」三源无事务绑定,首屏正确性隐式依赖 WS 事件全达且不被入口闸门丢弃;ea03c627 让「新建会话」首次稳定走完 stage空缓存→skip历史加载 这条链(:1155→:864-868),WS 一断供即空白。
2. **串台(文件树/存档)=架构层与表层混合**:会话身份与 desk 缓存两套事实无事务绑定是架构层旧疾;createNewSession 半清空 desk(:1235-1237)再被 activateWorkspaceDesk 快照回写(desk-actions.ts:394-455)是 ea03c627 重写引入的具体缺陷点。**串台(对话记录)=机制b(session-actions.ts:626-635)真实存在但新建流程内不可达**(任务2枚举排除);生产残留候选=服务端身份/WS 时序,未排除(见 BLOCKED.md)。

### 修复清单(保留 ea03c627 继承语义,全部只修时序/边界;优先级从高到低)

| # | 位置 | 改动 | 预期效果 | 回归风险 | 验收测试名(建议) |
|---|---|---|---|---|---|
| P0-1 | session-actions.ts:1155 | stageDetachedSessionForActivation 移除 `initSession(ref.sessionPath,[],false)` 预种空缓存,让 switchSession :864-868 走 !hasData→loadMessages 拉历史并 stamp revision | 新会话首屏由历史加载兜底,WS 事件丢/迟不再空白;revision 落 stamp 后自愈链有基点 | 低:空会话多一次 messages 请求;WS 先到时 session_user_message 自会 initSession(ws-message-handler.ts:965-967),缓存含真实内容不受影响 | 「ensureSession 完成后新会话缓存从 /api/sessions/messages 加载而非空种子」(现 session-new-session-blank.test.ts 翻红即修复生效) |
| P0-2 | session-actions.ts:638(loadSessions 尾部) | loadSessions setState 完成后补 `void reconcileCurrentSessionMessages('sessions_refresh')` | 闭合自愈缺口:InputArea.tsx:836 发送后刷列表、列表投影带 revision(server/routes/sessions.ts:958)而缓存 revision=null 时自动补拉;桌面端当前仅 chat-find-locate/移动端前台触发(ChatMessageSurface.tsx:371/MobileApp.tsx:204) | 中:所有缓存落后会话都会补拉(网络量↑);流式会话已被 reconcile :537 streamingSessions guard 排除 | 「loadSessions 后缓存 revision 落后触发补拉」 |
| P1-3 | session-actions.ts:1235-1237 | createNewSession 不再清 desk 三件套(deskCurrentPath/deskFiles/deskJianContent),desk 状态整体交 activateWorkspaceDesk 的 capture-restore(同 key=原样继承;异 key=恢复目标存档) | 主工作台存档不再被清空快照污染;新会话草稿期 desk 稳定显示继承工作台(继承语义更完整) | 低:草稿期旧 deskFiles 短暂可见(本就是要显示的工作台);ea03c627 现有锁定测试不涉三件套清空断言 | 「createNewSession 不污染 workspaceDeskStateByRoot」(现 desk-new-session-capture.test.ts 翻红即修复生效)+「继承同工作台 desk 状态原样保留」 |
| P1-4(备选) | desk-actions.ts:397-399 | 若不动 createNewSession:captureCurrentWorkspaceDeskState 在 store.pendingNewSession===true 时跳过 capture | 同 P1-3 止血 | 中:草稿期用户在 desk 的合法操作不被存档;不如 P1-3 干净 | 同 P1-3 |
| P2-5 | session-actions.ts:626-635 | 强切 sessions[0] 加一次性 bootstrap 标志(冷启动/归档两设计内入口显式置位),防未来新入口在无会话态被静默拉走 | 纵深防御(当前枚举已排除可达路径,无行为变化) | 低:需同步 archiveSession :1386-1390 兜底与 app-init 冷启动置标志,否则破坏现有行为 | 「冷启动/归档仍落 sessions[0]」「未来无会话态不被 loadSessions 拉走」 |

取舍说明:5 项全部不回退「新建聊天继承当前主工作台」;P1-3 反而让 desk 侧继承语义更完整(原样继承,而非清空)。

## 2026-09-02 安全双件套开工回执
- 目标：为外部证据增加机械注入扫描，为 Agent 工具循环增加阶梯式跑飞守卫。
- 顺序：共享扫描引擎 → 知识普通/滚动链路 → 工具扩展 → 开放边界 → 全量验收。
- 最大风险：警告或边界误改证据原文、扫描晚于截断、计数跨会话串扰。
- 安全边界：只加警告与阻断原因，不静默丢弃证据，不记录正文/参数/本机路径。
- 基线：`npm test` exit 0；1271 files passed / 1 skipped；12896 tests passed / 7 skipped；78.98s。
- 跳过口径：保留既有 7 个跨平台/Windows 人工冒烟跳过，本任务不得新增跳过。
- 执行约束：当前 main 工作树干净；不建分支、不提交、不改 node_modules/审批档/工具白名单。

## 2026-09-02 安全双件套完成记录

### 验收结果
- 最终 `npm test`：exit 0；1273 files passed / 1 skipped；12927 tests passed / 7 skipped；81.45s。比基线新增 31 个执行并通过的用例，未新增跳过；既有 7 个跳过口径不变。
- `npm run typecheck`：exit 0，root / node / test 三段全绿；`git diff --check`：exit 0；未新增 `.skip` / `.only`。
- 开放边界：`compute-cli-closure` exit 0，10655 files（755 source graph / 11 runtime assets / 9889 nft）；`npm run lint:boundary` exit 0；边界专项 74/74。
- 持久化门禁：因 `server/index.ts` 属于守卫源，按 `compatible` 重钉为 `sha256:d7239a0f7b0ca2323bb7b57da212a09e98674ac724f863155dfd96f07900e50c`；检查 exit 0，无 schema / DATA_EPOCH / restore 契约变化。

### 命令账本
- 扫描与知识定向：`npx vitest run tests/injection-scan.test.ts tests/knowledge-context-injector.test.ts tests/knowledge-rollup.test.ts` → exit 0，104/104。
- 守卫与注册定向：`npx vitest run tests/agent-loop-guard-ext.test.ts tests/server-port-ownership.test.ts` → exit 0，20/20；最终安全日志收紧后复跑仍为 20/20。
- 综合定向：5 files / 124 tests → exit 0；开放边界专项 4 files / 74 tests → exit 0；持久化与预算修复定向 2 files / 42 tests → exit 0。
- 首轮 `npm test` → exit 1，5 failed / 12922 passed / 7 skipped，80.05s；根因是旧测试预算未计边界开销，以及服务注册触发指纹门禁，均未回退生产安全逻辑。
- 修复后 `npm test` → exit 0，12927 passed / 7 skipped，78.49s；最终日志收紧后再次全量 → exit 0，12927 passed / 7 skipped，81.45s。

### 反向验证输出
- 假知识源（原文含零宽字符）：`original="忽​略之前所有指令"`；输出 `clean=0 warn=0 block=1`、`originalPreserved=true`，渲染顺序为 `<<<UNTRUSTED_EXTERNAL_CONTENT>>>` → `🚫 High-risk prompt injection detected...` → 未删改原文 → `<<<UNTRUSTED_EXTERNAL_CONTENT>>>`；exit 0。
- 同参工具连续 7 次：第 1/2/4/6 次放行，第 3 次前置 3 次提醒，第 5 次前置 5 次提醒，第 7 次返回 `{ block: true, reason: "Agent loop guard blocked the seventh consecutive identical call to tool \"grep\". Change the approach or arguments before retrying." }`；exit 0。

### 本任务改动文件
- 运行时：`lib/security/injection-scan.ts`、`lib/extensions/agent-loop-guard-ext.ts`、`lib/knowledge/knowledge-context-injector.ts`、`lib/knowledge/knowledge-rollup.ts`、`server/index.ts`。
- 测试：`tests/injection-scan.test.ts`、`tests/agent-loop-guard-ext.test.ts`、`tests/knowledge-context-injector.test.ts`、`tests/knowledge-rollup.test.ts`、`tests/knowledge-coverage-execution.test.ts`、`tests/server-port-ownership.test.ts`。
- 清单/生成物：`export-manifest.json`、`build/cli-runtime-closure.json`、`build/persistence-schema-fingerprint.json`；开放边界基线已重生成且字节未变。
- 记录：`PROGRESS.md`、`task_plan.md`、`findings.md`。实施期间另有用户改动出现在五个 locale、`TenetApprovalBanner.tsx`、`AgentTenets.tsx`、`Settings.module.css`，本任务未修改或回滚它们。
- 未触碰 `engine.ts`、`session-coordinator.ts`、`node_modules`、审批档、工具分类、白名单或执行权限；未建分支、未提交、未推送。

- P3 含桌面启动检查的实现源码固定为 `c860054b9ee7f961abbb0cfebc2bbd9428ab6bd3`；两轮 14 条生成/校验命令全部 exit 0，每轮完整工作区 diff 为 0；960 个测试文件清单逐字节一致（SHA-256 78836634483cb524c7f52167b44a275bc7623a4f34d27d81b12c9e78bd3ecfd0），两个开放树 885 文件逐个一致。新桌面检查实际验证首启引导页内容与服务端；本条按用户授权同步审计坐标用于全量复验，四平台与最终封印仍待完成。

- 当前源码 `c860054b9ee7f961abbb0cfebc2bbd9428ab6bd3` 的全量审计复验已 exit 0：全量 13738 PASS / 0 FAIL / 7 既有 SKIP，命令总耗时 87.507s；日志 `/tmp/lingxi-knowledge-p307-audit-full.log`。仅同步六个既有审计文件用于当前源码四平台 CI，最终封印仍保留；没有扩大审计白名单。

- P3 四平台首轮知识专项、Linux 性能、质量全量均通过；Linux 桌面因解包沙箱权限失败，macOS arm64/Windows 桌面失败阶段待诊断。本次源码 `9c1373917c66c630b30e24d294e58ff5458df237` 补 Linux 正确沙箱权限和脱敏失败诊断，原启动断言不变；本机 CI 界面归档重打包启动通过。按用户授权推进阶段审计用于复验，不建立最终封印。

- 全量发现下载写入尚未关闭即删除的竞态，确定性红测后修为等待关闭再清理，106 项下载回归通过。当前待验证源码 `75dd47d126490b85157f336c7969303bcddc3b95` 包含该修复和平台启动诊断；最终封印仍待四平台实际通过。

- 当前源码 `75dd47d1` 的阶段全量复验已通过：13739 PASS / 0 FAIL / 7 既有 SKIP，84.87s；lint exit 0，两轮完整生成器差异不变，测试清单 960 项（SHA-256 4c0b1fa2097818b9c7d5e44cd22a05c611b2b0ff4981a2991d9a76b55c763065），开放树 885 文件一致。第二轮四平台待执行，最终封印未建立。

- 第二轮真实平台确认 Linux 沙箱权限修复有效；macOS arm64/Windows 都是 splash 初始加载被 preparing 重复导航中止（ERR_ABORTED）。当前源码 `4ec98d0e185e3785f18cd40a7274c32ec9bff7e4` 等待初始文档完成后只切换一次，三个确定性红测修后通过，原启动失败断言不变；本机完整打包与真实桌面启动通过，阶段全量和第三轮四平台待执行。

- 当前源码 `aba59a5c`（运行时修复 `4ec98d0e` 加兼容指纹）全量 13742 PASS / 0 FAIL / 7 既有 SKIP，81.09s，lint exit 0。两轮 14 条生成器全部通过，960 项测试清单 SHA-256 `8c16cc006d9055e0e53e1b87179e938af7c3ee200fa41f81903057a227cde5b9`，885 开放树文件一致。第二轮 macOS 双架构和 Windows 均确认 splash -3 重复导航，第三轮验证当前修复；最终封印未建立。

- 当前源码 `93764185d30f75048a5611c7b6854858448bd915`：Windows 只结束本次测试进程树并如实记录清理结果，确定性回归及本机实际启动/清理通过；阶段全量 13746 PASS / 0 FAIL / 7 既有 SKIP，83.65s，lint exit 0。两轮 14 条生成器均通过，961 项测试清单 SHA-256 `7386b9ccb52a904b88d9f66ef12de0d413266bdd65b71bfe506af603269ea7b3`，885 开放文件逐字节相同；数据契约不变。第三轮其余平台与质量已通过，第四轮验证清理修复；最终封印未建立。

- 当前源码 `61ae60d1afe4f878ae638ff4de96d7e4f30bfefe` 修复启动早期调试列表的单次探测超时，仍保留原 90s 总期限、真实页面/后台与崩溃检查。全量 13751 PASS / 0 FAIL / 7 既有 SKIP，78.81s；类型、lint、本机实际启动及清理均通过。两轮 14 条生成器通过，961 项测试清单 SHA-256 `43df0f0f503484a22fc2d1e0c532db9d01fe820ea3e8b1ae1839bac40d6a46fd`，885 开放文件相同。第四轮 Intel 临时磁盘阻塞已解除，新的探测失败另有红测与修复，原记录均保留；第五轮待验证，最终封印未建立。

## 知识 P0-P3 最终交付与封印

- 固定基线 `3eab85891a1747c64064252804f70c0a3773f021`；最后实现/修复源码 `61ae60d1afe4f878ae638ff4de96d7e4f30bfefe`；CI 阶段审计 `790b496d88d8af6ad4a621085b93d802cc2553f9`；交付文档提交 `eb5e13d8f567dc05ae2888c71122c838e4a4c9e6`。CI 提交到交付提交仅 56 个任务文档与证据文件变化，产品和测试代码不变。
- 第五轮 Build `33864141539` 全部成功：质量 13741 PASS / 0 FAIL / 17 既有 SKIP（643.87s）；四平台知识专项各 151 PASS / 0 FAIL / 0 SKIP；完整构建、包内检索、真实桌面与清理全部通过；统一产物门禁 305 PASS / 0 FAIL / 0 SKIP（1.63s）。没有发版或合并 main。
- 本机类型、lint、边界、两轮生成物、构建和打包证据完整归档在 KNOWLEDGE_REFACTOR_* 报告及 artifacts/knowledge-*。原环境失败及修复记录保留，任务范围内无剩余实现项或环境阻塞。
- 此最终封印只修改既有六份审计文件；封印前全量已针对交付提交通过，实际结果见下一条。

- 最终封印前 `npm test` 已实际 exit 0：13751 PASS / 0 FAIL / 7 既有 SKIP；1357 测试文件通过 / 1 既有跳过，79.33s（命令总耗时 79.776s）。UTC 2026-09-04T11:22:01.945189+00:00 → 2026-09-04T11:23:21.722008+00:00。原始日志 `/tmp/lingxi-knowledge-p307-final-seal-full.log`；矩阵一致性、独立差异门禁及 Git 差异检查通过。所有任务已完成；最终封印采用任务书固定标题 `chore(audit): advance verified source for knowledge P0-P3 refactor`，仅六份审计文件，不合并 main。

## 2026-09-04 详细研究会话组装修复与独立封印

- 用户反馈截图后独立核实：研究空扩展结果缺少 SDK 必需运行载体，会话还未调用模型就失败。修复源码提交 `c9353fc6b2e2f6b90ed125fc28cf47ad36e4a7d0`；原任务最终封印 `b4fbcb07f4207f5afe57baf5a8779ba6b7ff341b` 保留在历史中。
- 真实组装红测三个研究入口全部重现相同错误，普通隔离会话通过；修复后五项真实 SDK 回归通过。原参数单测的整段会话替身是此前漏测原因，现已补齐，不修改用户知识库、失败记录或预算。
- 修复后工作树全量 13756 PASS / 0 FAIL / 7 既有 SKIP，1358 文件通过 / 1 既有跳过，82.74s，exit 0；三套类型、全仓 lint（0 错误、9190 既有警告）、持久化与开放边界门禁通过。提交后核心代码、测试与生成物和受测工作树一致；原始日志与各步结果见 `artifacts/knowledge-research-runtime-fix/progress.md`。
- 本机客户端/服务端构建、原生依赖检查、一次性签名种子验签通过；macOS arm64 目录包使用已安装的同版本 Electron 42.8.1，最终打包 exit 0 且签名校验通过。未配置正式 Apple 公证，因此本次不声明公证或新的四平台发布通过；原下载/凭证错误及退出码保留。验证临时私钥已清理，正式公钥与依赖版本未变。
- 兼容指纹 `sha256:01923b378ab07195c438fed0cb0fc356da0c4061c7d2e270a8946c97e4875cc0`；只更新会话协调器源码摘要，无数据格式或迁移变化。运行闭包重生成无差异。
- 本次独立封印只同步既有六份审计文件，保持原差异白名单与门禁；不合并 main。
- 封印前矩阵与审计门禁 10/10 通过，exit 0，148ms；独立差异检查通过。封印后的六文件差异由同一独立检查再次确认，日志 `/tmp/lingxi-research-runtime-audit.log`。

## 2026-09-05 0.1.34 合并前独立审计封印

- 固定源码：`004cdafd2a3ff69ca38ea7993e33c4150e75e0a7`；连续查阅、分块、引用与后台停止变更以该树为准，历史研究入口保留专门回归。原任务书后续变更已由用户明确授权，当前授权包括合并 main 及正式发布。
- 本机全量（暂不含等待源码提交的唯一封印文件）13756 PASS / 0 FAIL / 7 既有 SKIP，195.847s；之后新增连续查阅 3 项、引用悬浮窗 2 项各自通过，含相关旧用例复验共 18 PASS。所有测试由同一源码树提供；运行日志、初轮 157 个失败及修复经过保留，详见 `artifacts/release-v0.1.34/premerge-validation.json` 与 KNOWLEDGE_REFACTOR_PROGRESS.md。
- 三套类型检查 exit 0；最终测试类型检查 exit 0；全仓 lint exit 0（既有警告未升级为错误）、开放边界 exit 0；存储扫描 66 stores / 779 sites，运行闭包重新生成无漂移，兼容指纹已同步。
- 客户端五入口、完整服务端、种子验签、macOS arm64 目录包、开放服务端构建及正反启动检查均 exit 0。本机使用一次性产物密钥和 ad-hoc 签名、明确不公证；正式发布依赖远程密钥与四平台构建，不将本机结果冒充正式发行证据。
- 此提交仅推进审计坐标，保留独立封印。四平台合并门禁和 0.1.34 标签发布尚未执行，后续以 GitHub 实际运行记录为准。

## 2026-09-05 四平台类型检查容量修复封印

- 固定源码：`3e625e6bc41fd6c7274403380dd24cb3ba352adc`。
- PR #41 首轮正确提交的 CI `33939166103` 在 macOS arm64 测试工程类型检查耗尽默认约 2GB 堆，exit 134；原始日志 `/tmp/lingxi-pr41-mac-arm-failure.log` 保留。该平台后续测试未执行，不记通过。开放边界、存储兼容前哨和开放构建/启动已经成功。
- 只为 CI 与发布流程的 Typecheck 步骤显式配置 4096MB 堆，类型规则、检查目录、测试命令和平台矩阵不变，未更改应用代码。相同容量下三套类型检查 exit 0；两份流程契约 23 PASS，exit 0。完整运行源码与上一封印 004cdafd 一致，沿用该源码已通过的本机全量、构建和打包证据；远程内存修复待新运行确认。


## 2026-09-05 Windows 知识回归清理同步封印

- 固定源码：`6607e9af03d64186ebce6a1109ccb94936323506`。
- 首轮 CI `33939166103`：Windows 知识专项 150 PASS / 1 FAIL，失败发生在导入回归清理临时目录，EPERM；原文导入与重启断言此前已执行。原始日志 `/tmp/lingxi-pr41-windows1.log` 保留。
- 异步关闭契约同步遗漏了部分测试内的显式关闭及重排共享夹具；现均等待真实关闭后再删目录或重开管理器，未增加删除重试、忽略异常或变更断言。静态核对直接创建的管理器仅剩两处刻意检验“关闭发起时立即失效”的调用不就地等待，仍由各自清理钩子等待同一关闭承诺。
- 相关 11 文件 / 53 项本机通过，测试工程类型检查 exit 0。应用源码与 004cdafd 一致；第二轮 macOS arm64 已通过 4GB 配置的 Typecheck，确认原 2GB 内存阻塞解除。最终 Windows 清理和四平台全量待新运行，不复用旧失败为成功。


## 2026-09-05 v0.1.34 发布资料最终封印

- 固定源码：`0adf3727e065e394b71c75cc8006707f1477ab7e`。
- 功能 PR #41 已合并主分支（b02aaab7）；远程 CI 33939710990 四平台及七项门禁全部 PASS，原始坐标与作业链接见 artifacts/release-v0.1.34/feature-ci-validation.json。
- 版本 0.1.34 / releaseGeneration 12；上一正式版 0.1.33 / generation 11。摘要通过生成器追加到 v2 历史，双份摘要验证与 release:preflight PASS。
- 发布资料定向回归 6 文件 / 54 项 PASS；运行时代码保持已通过四平台的功能合并版本。
- 当前是发布资料封印；正式四平台安装包、签名与上传尚未执行，等待发布分支门禁及标签流水线，不能视为已经发布。

## 2026-09-05 契约执行路径不变量修复

- 固定源码：`ce701ee20727e7cdaaf3d6f838ae8ca5727c2b63`；固定基线为 v0.1.34 发布提交 `60d910b84572c525a7c9c49216fb9206623bf7a4`，执行分支为 `fix/tool-contract-path-invariance-v0134`。
- 用户明确授权调整 P12 封印顺序：先完成构建，再推进封印，封印后完整重跑全量测试；没有跳过封印用例、扩大白名单或把封印前失败记为通过。
- P12-01：底层执行边界扫描 2129 个生产源码文件、0 违规；任务书指定 25 文件 / 389 测试全部通过。
- P12-02 静态门禁：三段 typecheck exit 0；lint exit 0（0 errors / 9231 warnings）；开放边界 exit 0（仅 1 条既有债务）；`git diff --check` exit 0，验证前工作树干净。
- 封印前全量基准：1373 文件通过 / 1 失败 / 1 跳过，13903 测试通过 / 1 失败 / 7 跳过；唯一失败为旧 `VERIFIED_SOURCE_SHA` 正确拒绝本任务源码，保留原始日志且不记通过。封印后必须完整复跑并达到 0 fail 才能完成 P12-02。
- P12-03：`build:server`、`build:server:open`、`build:client`、`verify:seed-kit` 均 exit 0；当前平台为 darwin-arm64。正式服务端完成 9075 文件运行闭包裁剪、四项原生运行冒烟、12 个 Mach-O ad-hoc 签名与双归档；开放服务端完成白名单校验和 8985 文件运行闭包裁剪；客户端五入口全部构建；seed kit 清单与签名验真通过。
- 构建使用 `/tmp` 一次性匹配密钥对与构建期公开 keyset，仅作为本地门禁；私钥、公开清单和临时目录已精确删除，未入库，未将本机 ad-hoc 结果冒充正式发行签名或公证。
- 封印后 P12-02 最终全量复核 exit 0：1374 文件通过 / 1 跳过，13904 测试通过 / 7 跳过，0 fail；skip 数与固定基线一致，没有新增。原始日志：`/tmp/lingxi-tool-contract-p1202-full-tests.log`。
- P12-05：矩阵校验 133 条通过；post-verification diff 仅含 6 个审计 allowlist 文件；`upstream-sync-matrix` 与 `post-verification-audit-seal` 共 2 文件 / 10 测试全部通过；工作树干净。日志：`/tmp/lingxi-tool-contract-p1205-matrix.log`、`/tmp/lingxi-tool-contract-p1205-post-diff.log`、`/tmp/lingxi-tool-contract-p1205-seal-tests.log`。
- 原始日志：`/tmp/lingxi-tool-contract-p1201-boundary.log`、`/tmp/lingxi-tool-contract-p1201-targeted.log`、`/tmp/lingxi-tool-contract-p1202-typecheck.log`、`/tmp/lingxi-tool-contract-p1202-lint.log`、`/tmp/lingxi-tool-contract-p1202-open-boundary.log`、`/tmp/lingxi-tool-contract-p1202-full-tests-preseal.log`、`/tmp/lingxi-tool-contract-p1203-build-server.log`、`/tmp/lingxi-tool-contract-p1203-build-server-open.log`、`/tmp/lingxi-tool-contract-p1203-build-client.log`、`/tmp/lingxi-tool-contract-p1203-seed-kit.log`。
- `TOOL_INVOCATION_REPAIR_FACTS.json` 中 `sourceCandidateSha` 与 `sealSha` 按任务书保持 `null`，避免提交自引用；真实源码候选记录在本节，真实封印提交只在最终执行报告中给出。
- PR #43 首轮 Windows CI 在 `tests/tool-invocation-path-parity.test.ts` 出现 10 项 `PREPARED_INVOCATION_MISMATCH`：测试夹具固定使用 Unix 会话路径，而 Windows 包装层按平台规范化为绝对路径。生产 fail-closed 正确拒绝不一致事实，没有放宽。
- 最小修复提交 `ce701ee20727e7cdaaf3d6f838ae8ca5727c2b63` 只把测试会话路径改为当前平台的规范绝对路径。修复后定向测试 1 文件 / 12 测试通过；三段 typecheck exit 0；本地全量测试 1374 文件通过 / 1 跳过、13904 测试通过 / 7 跳过、0 fail。日志：`/tmp/lingxi-pr43-windows-path-fixture-green.log`、`/tmp/lingxi-pr43-windows-path-fixture-typecheck.log`、`/tmp/lingxi-pr43-windows-path-fixture-full-test.log`。
