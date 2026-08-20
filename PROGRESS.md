# PROGRESS — openhanako v0.444.1 → v0.447.4 上游同步

## 审计坐标（固定，执行期间从未移动）

```
UPSTREAM_BASE_SHA   = cc19cb49b0786d61ed723764e0a83baf87887270  (openhanako v0.444.1)
UPSTREAM_TARGET_SHA = c6d0405294be67cb134c2758f6472748ee73e2be  (openhanako v0.447.4)
LINGXI_BASE_SHA     = 97595264ead8735a04559507ddaade25db8a4e15  (v0.444.1 同步完成点, PR #2)
LINGXI_START_SHA    = ca0b417e36a6a1f80947458aaed328a25718e41b  (main HEAD @ 2026-08-20)
LINGXI_FINAL_SHA    = __FINAL_SHA__  (收口提交；其后仅有一个写入该 SHA 的 seal 提交)
工作分支            = feature/upstream-sync-0.447.4
```

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
FINAL_SHA（本文件与 UPSTREAM_SYNC_AUDIT.md / UPSTREAM_SYNC_MATRIX.md 记录同一 40 位 SHA）
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

## 已执行测试（最终源码树，全部指向 FINAL_SHA 对应树）

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
- CI headSha 3707a450 与 FINAL_SHA 之间仅有本文件记录与 FINAL_SHA 标注（docs-only），
  代码/测试/构建输入零差异（`git diff 3707a450..FINAL_SHA --stat` 可核）。

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
   字节一致（纯品牌改名）→ 模板迁移=目录改名+保留品牌文件。
3. Dream 绑定 Lingxi 辅助模型槽（memory slot），tests/memory-dream-memory-slot.test.ts 锁定，
   未复活 utility 架构；本轮 diff 功能未触碰该链路。
4. overlap 29 路径中 C 类最高风险 AssistantMessage.tsx 以干净三方合并落地，automation
   suggestion 失败走 ContentBlock 架构。
5. cron-store / memory-ticker 上游 patch 基线一致性好；Lingxi 扩展（configRevision/
   storeRevision）保全。
6. 派生物两轮重新生成（abbfb593、12d87d44）；persistence fingerprint 两轮 compatible
   review（persona 改名 + dream additive stores；agent-manager 回调接线），DATA_EPOCH=1 不变。
