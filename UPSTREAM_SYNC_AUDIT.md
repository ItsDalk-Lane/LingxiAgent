# Upstream v0.444.1 → v0.447.4 同步审计

> **状态：READY TO MERGE。**
>
> 逐路径处置矩阵（机器真相源 + 生成投影）见 UPSTREAM_SYNC_MATRIX.md 与
> `.sync-audit/upstream-sync-matrix.json`；逐阶段测试记录见 PROGRESS.md；
> 矩阵不变量由 tests/upstream-sync-matrix.test.ts 机器校验。

## 审计坐标

| 角色 | 提交 | 说明 |
| --- | --- | --- |
| U0：上游基线 | `cc19cb49b0786d61ed723764e0a83baf87887270` | openhanako `v0.444.1` |
| U1：上游目标 | `c6d0405294be67cb134c2758f6472748ee73e2be` | openhanako `v0.447.4` |
| L0：Lingxi 同步基线 | `97595264ead8735a04559507ddaade25db8a4e15` | chore: sync upstream 0.444.1 and pi SDK 0.84.1 (PR #2) |
| L1：同步开始 Lingxi | `ca0b417e36a6a1f80947458aaed328a25718e41b` | 2026-08-20 main HEAD |
| VERIFIED_SOURCE_SHA | `6607e9af03d64186ebce6a1109ccb94936323506` | 2026-09-05 Windows 知识回归清理同步封印；验证明细见 PROGRESS.md 最新条目 |

## Audit seal model

`VERIFIED_SOURCE_SHA` 是被最终测试、构建、打包验证的源码树。

其后的 commit 只允许修改审计元数据。当前 branch HEAD 不在自身 commit 内容中记录
（以避免 Git SHA 自引用：`SHA = hash(contents)`，自引用会无限漂移）。完整性由
post-verification diff guard（`.sync-audit/verify-post-verification-diff.mjs` +
`tests/post-verification-audit-seal.test.ts`）保证：`git diff --name-only
VERIFIED_SOURCE_SHA..HEAD` 仅允许审计文件变化，任何生产代码、测试逻辑、
runtime generated artifacts 变更都会使 guard 失败（exit 1）。

## 上游变更统计（ΔU）

- commit count：18（U0..U1）
- changed path count：133（`git diff --name-status U0 U1`，原始输入 `.sync-audit/delta-U-final.txt`）
- 分布：added 23 / modified 97 / renamed 13（全部 R100 纯改名）/ deleted 0
- ΔL（L0..L1 Lingxi 侧变更）：346 paths；overlap（ΔU∩ΔL）：29 paths
  （原始数据：`.sync-audit/delta-L.txt`、`.sync-audit/overlap-paths.txt`）

## Disposition summary（脚本计算）

```
Total upstream paths: 133
ADOPTED: 25
ADAPTED: 100
REGENERATED: 4
INTENTIONAL_DIVERGENCE: 4
UNKNOWN: 0 / MISSING: 0 / DUPLICATE: 0
25 + 100 + 4 + 4 = 133
```

13 个 R100 模板重命名逐 path 独立成行（含 renamed_from），不再聚合；
5 个 locale、21+7 个测试文件同样逐 path 独立成行。
其中 4 个 `hanako.md`（lib/agents-templates/{,en/}hanako.md 与
lib/agents-public-templates/{,en/}hanako.md）因品牌路径映射
（upstream `hanako.md` → Lingxi `lingxi.md`）分类为 ADAPTED，而非 ADOPTED。

## Key adaptations

1. **AGENTS.md 人格协议迁移 + migration-degraded fallback（本轮修复）**
   `ishiki.md → AGENTS.md`、`public-ishiki.md → AGENTS.public.md` 启动改名（不删用户文件、
   新旧并存置 `.pre-agents-rename.bak`、每次启动重试）。**本轮补齐缺口**：改名失败的结果
   从"仅打 log"升级为结构化运行时状态（`engine._failedPersonaRenames`，
   `buildFailedPersonaRenameIndex`），`resolvePersonaSource` 增加显式 `migrationFallback`
   参数——仅当本次启动明确记录改名失败、且新文件不存在时，才临时读取旧文件
   （`fromTemplate=false`）；新文件永远优先；无失败记录的 out-of-band 旧文件一律不读
   （不重建永久双读协议）；public 变体同规则。下次启动改名成功后 fallback 自动消失。
   证据：tests/agents-md-startup-migration.test.ts 11 用例（含 5 个 degraded 场景）、
   migration smoke 23/23（含语义断言：failed rename 后 effective persona 仍是用户自定义内容）。
2. **Memory Dream + Lingxi memory 语义槽**：`Agent.getResolvedMemoryModel` 闭包 →
   `engine.resolveAuxiliaryExecution("memory", { agentId })` → MemoryTicker → DreamRunner。
   不复活 utility/utility_large，不回落 chat/title/summarize/approval/vision/guard。
   契约锁定：tests/memory-dream-memory-slot.test.ts。
3. **Dream revision current-vs-revision diff（本轮新增）**：revision detail 响应携带后端
   现读的当前记忆快照（`snapshotDreamSections`，与 `revision.before` 同构
   facts/today/weekDays/longterm，不暴露文件路径）；DreamRevisionBrowser 渲染逐段统一 diff
   （复用 `desktop/src/react/utils/line-diff.ts`，+ = 恢复后出现 / − = 恢复后移除 / 相同段
   折叠标注），进入确认前重新现取 current，current == revision 时显示"相同"并禁用恢复按钮，
   恢复成功后刷新版本列表与对比。证据：DreamRevisionBrowser.test.tsx 6 用例（任务书 A–F）、
   tests/memory-dream-route.test.ts 7 用例（含 current 快照断言）。
4. **Dream config 契约补齐（本轮）**：`PUT /api/agents/:id/config` 的
   `memory.dream.auto_enabled` 布尔校验——生产实现本就与 upstream 逐字节一致，本轮补齐
   缺失的 3 个契约测试（200 透传 / "yes"、1、null → 400 且 `updateConfig` 未被调用）。
5. **Automation store 恢复**：tmp 校验 → 逐字节损坏备份 → 原子提升；稳定错误码
   `cron_store_corrupt/unavailable/recovery_failed`；禁止静默返回 []；未知错误不透传
   本地绝对路径；UI 保留上一次 jobs/badge、Add 防重复；suggestion 失败走 ContentBlock
   架构（不回退 AssistantMessage 聊天语义管线）。证据：tests/cron-store.test.ts、
   tests/desk-route-cron.test.ts、AutomationPanel/AssistantMessage 组件测试。
6. **Markdown 裸 URL**：`md-decorations.ts` 保持裸 URL 可见（fb032eea）。
7. **Context Ring**：压缩动作优先级与上游对齐（63bc92b7）。
8. **Windows seed 清理**：`RMDir /r "$INSTDIR\resources\seed"` 严格限于安装目录应用自有
   seed，不触 LINGXI_HOME / agents / providers / memory / sessions（18727d24）。
   证据：tests/windows-installer-contract.test.ts 19 用例。
9. **i18n（5 locale key-level merge）**：zh/zh-TW/en/ja/ko 逐 key 合并（删 ishiki*、
   增 agentsMd*/dream 树/error.code.dream*）；本轮追加 dream revisions diff 5 个 key。
   证据：tests/i18n-locale-parity.test.ts、tests/react-locale-coverage.test.ts。
10. **bundled skills**：人格相关 skill 文档对齐 AGENTS.md 命名，保留 Lingxi 品牌文案。
11. **依赖（INTENTIONAL_DIVERGENCE）**：package.json/package-lock.json（上游 U0..U1 仅
    version bump、无新依赖）；release-digest.v1/v2.json（Lingxi 发布历史独立，拷贝即伪造）。
    Pi SDK 保持 0.84.1 未降级；`lingxi.upstreamVersion = 0.447.4`。
12. **生成物（REGENERATED，全部由 Lingxi 生成器在最终源码树重新生成）**：
    cli-runtime-closure（dream 模块入闭包；本轮新增 memory-dream → revision-store 边）、
    persistence-schema-fingerprint（两轮 compatible review；DATA_EPOCH=1 不变）、
    persistence-store-inventory（59 stores / 702 sites）、export-manifest（704→712，
    手工策展权威源；本轮无新增生产文件故不变）。确定性验证：全部生成器连跑两次，
    第二次 git diff 为零。

## Lingxi invariants（下游保全确认）

| 能力 | 状态与证据 |
| --- | --- |
| Provider / Ollama 统一架构 | 全量套件 provider/ollama 用例全绿；migration smoke 断言 providers 数据逐字节完好 |
| 辅助模型语义槽（title/summarize/memory/vision/approval/guard） | 全量套件 auxiliary slot 契约全绿；Dream 仅消费 memory 槽（专项测试锁定） |
| 审批槽独立 | 未重新绑定 memory/utility；全量套件审批链路全绿 |
| 聊天语义管线（canonical 单通道 / ContentBlock registry / live-turn store / deferred history / 稳定 ID / thinking/mood/tool/skill fold） | AssistantMessage automation suggestion 走 ContentBlock 架构；聊天语义测试全绿 |
| ReservedTagScanner | 全量套件扫描器用例全绿 |
| Memory（facts/today/week/longterm） | Dream 只改 Facts/Long-term（Today/Week 受保护：applyDreamSections 拒绝改写）；revision-before-write；memory_changed 竞态保护；5000 字符硬上限 |
| Automation 下游扩展（configRevision/storeRevision/重入保护） | cron-store 定向测试断言扩展字段保全 |
| 用户数据 | migration smoke 23/23：persona 只改名/备份从不删除；memory/cron/sessions/providers/user 全量完好 |
| 发布签名公钥集 | `shared/artifact-core/pinned-keyset.json` 未触碰；本地构建签名用一次性 throwaway keypair（/tmp，未入库） |

## 红线复核

- 未 merge upstream main、未整体 cherry-pick、未 `checkout --theirs`——全部三方合并或行为级重写。
- 未复制上游 package.json/package-lock.json/release digest/persistence receipts/CLI closure。
- 无静默降级：cron-store 错误显式抛出带稳定码；persona degraded fallback 由启动失败记录
  显式驱动并打 log；Dream diff 超上限时显式降级为"无法计算差异"标注（不做假 diff）。
- 未删除/skip 任何失败测试；本轮新增测试 19 个全部通过。
- 无私钥/密钥入库；构建签名密钥对为 /tmp 一次性 throwaway。

## Known limitations

- **Windows 真实安装器执行未在本机进行**（宿主平台 macOS 无法执行 NSIS 安装包）。
  Windows contract 由 tests/windows-installer-contract.test.ts（19 用例）门禁；
  仓库 CI 已有 windows-latest runner（ci.yml 全量套件含该 contract；build.yml
  windows-latest + nsis 腿构建真实 NSIS 安装器）。收口后在 CI 触发记录见 PROGRESS.md。
