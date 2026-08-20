# Upstream v0.447.4 同步审计（已完成）

> 上一轮（v0.444.1）审计见 git 历史（L0=97595264 时的 UPSTREAM_SYNC_AUDIT.md）。
> 逐路径处置矩阵见 UPSTREAM_SYNC_MATRIX.md；逐阶段测试记录见 PROGRESS.md。

## 审计坐标

| 角色 | 提交 | 说明 |
| --- | --- | --- |
| U0：上游基线 | `cc19cb49b0786d61ed723764e0a83baf87887270` | openhanako `v0.444.1` |
| U1：上游目标 | `c6d0405294be67cb134c2758f6472748ee73e2be` | openhanako `v0.447.4` |
| L0：Lingxi 同步基线 | `97595264` | chore: sync upstream 0.444.1 and pi SDK 0.84.1 (PR #2) |
| L1：同步开始 Lingxi | `ca0b417e36a6a1f80947458aaed328a25718e41b` | 2026-08-20 main HEAD |
| 最终 Lingxi | 本提交（`git log` 中标题 `chore(sync): mark upstream compatibility as v0.447.4`；最后代码提交 `abbfb593e9a969b1cad394475ac22782e2d4953f`） | feature/upstream-sync-0.447.4 收尾提交链末端 |

坐标在同步执行期间从未移动。

## 上游变更统计

- commit count：18（U0..U1）
- changed path count：133（ΔU = U0..U1，`git diff --name-status` 条目数）
- 分布：**added 23 / modified 97 / renamed 13（全部 R100 纯改名）/ deleted 0**
- 处置分布：ADOPTED 18 / ADAPTED 102 / REGENERATED 5 / INTENTIONAL_DIVERGENCE 4 / UNKNOWN 0 / IGNORED 0
- 13 个 R100 均为 `*-templates/ishiki*` → `*-templates/agents*` 模板目录改名（计入 ADOPTED）。

## 功能审计

### 1. AGENTS.md 人格协议迁移（上游核心变更）
- **ADOPTED**：`core/agents-md-migration.ts` 原样落地。启动迁移规则：ishiki.md→AGENTS.md、public-ishiki.md→AGENTS.public.md；新旧并存时旧文件保留为 `.pre-agents-rename.bak`；**从不删除用户文件**，失败下次启动重试、不阻塞启动。
- **ADAPTED**：`core/persona-source.ts`（PersonaKind `identity|agents`、`agentPersonaFilePaths()`）、`core/engine.ts` 启动步 `agents-md-rename`、`shared/persistence/store-registry.ts` 路径模式与 rename 规则；workspace 注入增加 `excludeFiles` 精确绝对路径排除，杜绝人格文件重复注入（session-coordinator + bridge-session-manager 三处接线）。
- API 兼容：canonical `/agents-md` + `/public-agents-md` 路由 + 旧 `/ishiki` + `/public-ishiki` 别名绑定同一 handler；角色卡导出写新 key（`prompts.agents/publicAgents`），导入永久接受旧 key。
- 验证：persona migration 定向测试 + workspace 注入测试 + 旧数据 migration smoke 16/16。

### 2. Memory Dream（上游新增子系统）
- **ADOPTED**：`lib/memory/dream/` 五阶段管线（atomize/dedupe/optimize/compose/verify）、软目标（facts~400 / longterm~800）+ 5000 字符硬上限、per-agent 运行锁、input-hash 复核（`dream_memory_changed`）、revision-before-write、`auto_enabled` 默认 false、稳定错误码 `dream_*`（9 条 user-message 映射）。
- **ADAPTED（Lingxi 架构接入）**：`core/agent.ts` `getResolvedMemoryModel` 闭包走 `engine.resolveAuxiliaryExecution("memory", { agentId })`——**复用 Lingxi 辅助模型槽，未复活 utility 架构**；`memory-ticker.ts` 合并上游 Dream 触发逻辑并 gated on `getDreamAutoEnabled()===true`；server 路由 `createMemoryDreamRoute` 挂载 + route-security 放行；设置页 Dream 控制与 DreamRevisionBrowser UI。
- 契约锁定：tests/memory-dream-memory-slot.test.ts 断言 runner 仅解析 `("memory", { agentId })` 且正确映射 execution shape。

### 3. Automation store 损坏恢复
- `lib/desk/cron-store.ts` 上游 patch 直接落地（基线与 U0 字节一致）：tmp 校验后提升、逐字节损坏备份（时间戳+pid+冲突后缀）、稳定错误码（`cron_store_corrupt/unavailable/recovery_failed`）、**禁止静默返回 []**。
- UI（AutomationPanel）：`addingManualJob` 锁、`throwOnHttpError:false` 显式降级、reload 失败保留现有数据；AssistantMessage 走 ContentBlock 架构呈现 AutomationSubmissionError。
- Lingxi 扩展（configRevision/storeRevision）保全。

### 4. Markdown 裸 URL（fb032eea）
- `md-decorations.ts` 合并：裸 URL 不再被装饰/破坏；编辑器与聊天渲染链路定向测试通过。

### 5. Context Ring 顺序（63bc92b7）
- 压缩动作优先级与上游对齐；ContextRing 组件定向测试通过。

### 6. Windows seed 清理（18727d24）
- `build/installer.nsh`：`RMDir /r "$INSTDIR\resources\seed"`，注释已品牌化为 LINGXI_HOME；**清理范围严格限于安装目录应用自有 seed resource，绝不触及 LINGXI_HOME 或任何用户数据**。
- tests/windows-installer-contract.test.ts 宏名修正为 `lingxiRemoveOwnedInstallTrees`，19 用例通过。

### 7. i18n（5 locale，key-level merge）
- zh / zh-TW / en / ja / ko 五语言按 key 级合并（脚本化）：删除 `settings.agent.ishiki*`/`publicIshiki*`/`noIshiki`，新增 `agentsMd*`/`publicAgentsMd*`/`noAgentsMd`/`settings.memory.dream` 树/`error.code.dream*`，更新 `input.refreshAndCompact*` 措辞。**未整文件覆盖任何 locale**。

### 8. bundled skills（f32c3f64）
- 人格相关 skill 文档对齐 AGENTS.md 协议措辞。

### 9. 依赖与派生物
- **INTENTIONAL_DIVERGENCE**：package.json / package-lock.json（上游 U0..U1 仅 version bump，无新依赖）；release-digest.v1/v2.json（上游 digest 是上游发布历史，拷贝即伪造）；Pi SDK 保持 0.84.1 未降级。
- **REGENERATED**：CLI runtime closure、persistence schema fingerprint（compatible 增补 + compatibility-reason，DATA_EPOCH 不变）、persistent store inventory、export-manifest（704→712，手工策展权威源）。

## 下游保全审计

以下 Lingxi 下游能力全部以「全量测试套件 + 定向契约测试 + 构建/打包/migration smoke」确认零回归：

| 能力 | 保全证据 |
| --- | --- |
| Provider / Ollama | 全量套件内 provider/ollama 测试全绿；providers 数据在 migration smoke 中逐字节完好 |
| 辅助模型槽（chat/title/summarize/memory/vision/approval） | auxiliary slot 契约测试全绿；Dream 新增消费方走同一 contract 并被专项测试锁定 |
| 审批槽（approval slot） | 全量套件审批链路测试全绿 |
| 聊天语义管线（canonical 单通道 / 稳定 ID / Outcome / fold） | AssistantMessage 合并走 ContentBlock 架构；聊天语义测试全绿 |
| thinking / mood | 全量套件相关用例全绿；mood 剥离与 stepEnd 段/轮区分未受影响 |
| ReservedTagScanner | 全量套件扫描器用例全绿 |
| Memory（memory slot / facts / daily） | migration smoke 断言 memory/facts/daily 全量完好；memory 相关测试全绿 |
| Automation Lingxi 扩展（configRevision/storeRevision） | cron-store 定向测试断言扩展字段保全 |
| 用户数据 | migration smoke 16/16：sessions/cron/providers/user/persona 全部完好；persona 迁移只改名/备份、从不删除 |
| 发布签名公钥集 | `shared/artifact-core/pinned-keyset.json` 未触碰（轮换只追加规则未被触发） |

## 红线复核

- 未 merge upstream main、未整体 cherry-pick、未 `checkout --theirs`——全部经三方合并或行为级重写。
- 未拷贝上游 receipt/digest/fingerprint——派生物全部由 Lingxi 脚本在本仓库重新生成。
- 无静默降级：cron-store 错误显式抛出带稳定码；UI 显式降级均标注（`throwOnHttpError:false` + toast）。
- 无私钥/密钥写入代码或报告；构建签名使用一次性 throwaway keypair（/tmp，未入库）。
- `upstreamVersion` 在全部验证通过后才 bump（本审计收口时执行）。
- 全程未删除或跳过任何失败测试（vitest 误捕获的 3way 草稿非测试文件，删除草稿后全绿）。
