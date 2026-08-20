# UPSTREAM_SYNC_MATRIX — v0.444.1 → v0.447.4

坐标：U0=cc19cb49 U1=c6d04052 L0=97595264 L1起始=ca0b417e。
133 个 upstream 变更路径全量分类。冲突等级：A=Lingxi未改动 / B=双方改同文件职责不冲突 / C=Lingxi已重构该职责 / D=产品差异或生成物。

最终 disposition 只允许：ADOPTED / ADAPTED / REGENERATED / INTENTIONAL_DIVERGENCE。

## 集群 1：Memory Dream（9dd70cd5, 476b2d7c, 408b7a18, f29d15f2, ca6dbf95, 483c5fe0, 06e17341）

| path | upstream change | Lingxi overlap | risk | decision | implementation | status |
|---|---|---|---|---|---|---|
| lib/memory/dream/memory-units.ts | 新增：原子记忆单元 atomize/dedupe/optimize/compose/verify | 无 | A | ADAPTED | 同名移植，接 Lingxi memory 数据布局 | ✅ |
| lib/memory/dream/model-runner.ts | 新增：Dream 模型调用流水线（软目标+5000 硬上限） | 无 | A | **ADAPTED（关键）** | 模型解析改走 `engine.resolveAuxiliaryExecution("memory")`，禁止 utility | ✅ |
| lib/memory/dream/revision-store.ts | 新增：revision 持久化 | 无 | A | ADAPTED | 同名移植，入 Lingxi store registry | ✅ |
| lib/memory/dream/runner.ts | 新增：编排+并发锁+memory_changed 竞态 | 无 | A | ADAPTED | 同上 | ✅ |
| lib/memory/dream/state-store.ts | 新增：Dream 状态持久化+错误码 | 无 | A | ADAPTED | 同上 | ✅ |
| lib/memory/prompts/dream.ts | 新增：5 阶段 prompt | 无 | A | ADOPTED | 同名移植（校对 Lingxi 术语） | ✅ |
| lib/memory/memory-ticker.ts | M：维护完成后按 auto_enabled 触发 Dream | 无（L0 后未改） | A | ADAPTED | 合并 Dream 钩子进当前 ticker | ✅ |
| server/routes/memory-dream.ts | 新增：Dream REST 路由（稳定错误码） | 无 | A | ADAPTED | 同名移植，对齐 Lingxi 路由约定 | ✅ |
| server/composition/open-root.ts | M：挂载 dream 路由 | 无 | A | ADOPTED | 同位置挂载 | ✅ |
| server/http/route-security.ts | M：dream 路由安全分级 | 无 | A | ADOPTED | 同步规则 | ✅ |
| server/routes/agents.ts | M：persona+dream 两端点 | 无 | A | ADAPTED | 随两集群合并 | ✅ |
| shared/persistence/store-registry.ts | M：登记 dream state/revision store | L0 后有改 | B | ADAPTED | 结构化合并 | ✅ |
| shared/error-user-messages.ts | M：dream_* 错误码→用户文案 | 无 | A | ADOPTED | 合并错误码段 | ✅ |
| desktop/.../agent/AgentMemoryDream.tsx | 新增：Dream 设置区 UI | 无 | A | ADAPTED | 移植并对齐 Lingxi settings API client | ✅ |
| desktop/.../agent/DreamRevisionBrowser.tsx(+module.css) | 新增：revision 浏览/diff/restore | 无 | A | ADAPTED | 同上 | ✅ |
| desktop/.../agent/agent-memory-dream-actions.ts | 新增：UI action 层 | 无 | A | ADAPTED | 同上 | ✅ |
| desktop/.../agent/dream-error-presenter.ts | 新增：错误码→i18n | 无 | A | ADOPTED | 同名移植 | ✅ |
| desktop/.../agent/AgentMemory.tsx | M：嵌入 Dream 区 | 无 | A | ADAPTED | 合并进当前组件 | ✅ |
| desktop/.../tabs/AgentTab.tsx | M：persona 文案+dream 入口 | L0 后有改 | C | ADAPTED | 按当前结构合并 | ✅ |
| desktop/.../Settings.module.css | M：dream 样式 | L0 后有改 | B | ADAPTED | 追加 dream 样式段 | ✅ |
| desktop/.../settings-search-index.ts | M：dream 搜索项 | L0 后有改 | B | ADAPTED | 追加 | ✅ |
| tests/memory-dream-{model-runner,revision,route,runner,units}.test.ts | 新增 5 测试 | 无 | A | ADAPTED | 移植+新增 memory-slot 专项断言 | ✅ |
| tests/memory-ticker-dream.test.ts | 新增 | 无 | A | ADAPTED | 移植 | ✅ |
| desktop/.../agent/__tests__/DreamRevisionBrowser.test.tsx | 新增 | 无 | A | ADAPTED | 移植 | ✅ |
| desktop/.../__tests__/settings/AgentMemory.test.tsx | M | 无 | A | ADAPTED | 合并断言 | ✅ |
| desktop/.../__tests__/shared/error-user-messages.test.ts | M | 无 | A | ADAPTED | 合并断言 | ✅ |
| tests/agent-config-tools-disabled.test.ts | M：dream 工具门控 | 无 | A | ADAPTED | 合并 | ✅ |
| tests/http-route-security.test.ts | M：dream 路由断言 | 无 | A | ADAPTED | 合并 | ✅ |

## 集群 2：AGENTS.md 人格迁移（bed24b93, b3927f07）

| path | upstream change | Lingxi overlap | risk | decision | implementation | status |
|---|---|---|---|---|---|---|
| core/agents-md-migration.ts | 新增：启动迁移器（ishiki→AGENTS，保留旧文件，失败可重试） | 无 | A | ADOPTED | 同名移植+品牌检查 | ✅ |
| core/persona-source.ts | M：PersonaKind identity\|agents | 无 | A | ADAPTED | 改名+兼容读取 | ✅ |
| core/agent.ts | M：人格路径+workspace excludeFiles | 无 | A | ADAPTED | 合并 | ✅ |
| core/agent-manager.ts | M：默认人格文件创建 | 无 | A | ADAPTED | 合并 | ✅ |
| core/bridge-session-manager.ts | M | L0 后有改 | B | ADAPTED | 结构化合并 | ✅ |
| core/engine.ts | M | L0 后有改 | B | ADAPTED | 结构化合并 | ✅ |
| core/first-run.ts | M：种子人格文件名 | 无 | A | ADAPTED | 合并 | ✅ |
| core/llm-utils.ts | M | 无 | A | ADAPTED | 合并 | ✅ |
| core/session-coordinator.ts | M | L0 后有改 | B | ADAPTED | 结构化合并 | ✅ |
| core/workspace-instruction-files.ts | M：新增 excludeFiles（精确绝对路径） | 无 | A | ADOPTED | 移植 | ✅ |
| hub/agent-executor.ts | M | L0 后有改 | B | ADAPTED | 结构化合并 | ✅ |
| lib/character-cards/service.ts | M：导出写新 key、导入兼容 legacy | 无 | A | ADAPTED | 合并 | ✅ |
| lib/diary/diary-writer.ts | M | 无 | A | ADAPTED | 合并 | ✅ |
| lib/sandbox/policy.ts | M | 无 | A | ADAPTED | 合并 | ✅ |
| server/routes/settings-snapshot.ts | M | 无 | A | ADAPTED | 合并 | ✅ |
| server/routes/agents.ts | M：新 persona 路由 canonical+旧 alias | 无 | A | ADAPTED | 同 handler 双路由 | ✅ |
| lib/ishiki-templates→agents-templates (6) | R100 重命名 | Lingxi 有 lingxi.md | A | ADAPTED | 目录改名，保留 lingxi.md 品牌文件 | ✅ |
| lib/public-ishiki-templates→agents-public-templates (6) | R100 | 同上 | A | ADAPTED | 同上 | ✅ |
| lib/ishiki.example.md→agents.example.md | R100 | 有 Lingxi 品牌内容 | A | ADAPTED | 改名保留内容 | ✅ |
| desktop settings UI（store.ts, actions.ts, helpers.ts, AgentTab, BridgeTab, useBridgeState, CharacterCardPreviewOverlay, Settings.module.css, settings-search-index） | M：ishiki→agents 文案/API | 部分 overlap | B/C | ADAPTED | 逐文件合并 | ✅ |
| scripts/build-server.mjs | M：模板目录名 | L0 后有改 | B | ADAPTED | 仅套用目录改名 | ✅ |
| scripts/build-server-open.mjs | M：同上 | 无 | A | ADAPTED | 同上 | ✅ |
| scripts/export-open-tree.mjs | M：注释同步 | 无 | A | ADOPTED | 注释更新 | ✅ |
| scripts/i18n-backfill-{ja,ko}.json | M：noIshiki→noAgentsMd | 无 | A | ADOPTED | key 改名 | ✅ |
| tests/agents-md-startup-migration.test.ts | 新增：迁移全场景 | 无 | A | ADOPTED | 移植 | ✅ |
| tests/workspace-instruction-files-exclude.test.ts | 新增 | 无 | A | ADOPTED | 移植 | ✅ |
| tests/{persona-source,agents-route,character-card-import,agent-description,agent-experience-toggle,agent-interactive-card-tools,agent-locale-resolution,agent-manager-create-defaults,agent-master-session-decoupling,agent-system-prompt-section-order,agent-tools-conditional-injection,bridge-session-orphan-repair,bridge-session-teardown,build-server-open,builtin-tool-permission-coverage,channel-router-memory-master,first-run-default-workspace,server-composition-boundary,session-tool-gating,settings-snapshot-route}.test.ts | M：命名/行为断言更新 | 无 | A | ADAPTED | 合并断言 | ✅ |
| desktop 测试（AgentTab, BridgeTab×2, actions, helpers, useBridgeState.snapshot, SettingsContent） | M | 3 个 overlap | B | ADAPTED | 合并 | ✅ |
| skills2set/character-creator/SKILL.md + references/{anti-slop,card-format}.md + user-guide/SKILL.md | M：AGENTS.md 命名对齐 | Lingxi 品牌 | A | ADAPTED | 保留品牌文案改命名 | ✅ |

## 集群 3：Automation store 恢复（a14a13bc, 61a2a6bf）

| path | upstream change | Lingxi overlap | risk | decision | implementation | status |
|---|---|---|---|---|---|---|
| lib/desk/cron-store.ts | M：tmp 校验恢复+损坏备份+稳定错误 | Lingxi 扩展在 L0 前并入 | B | ADAPTED | 行为不变量提取后合并，保留 configRevision/storeRevision/重入保护 | ✅ |
| server/routes/desk.ts | M：错误传播不泄露 fs 路径 | 无 | A | ADAPTED | 合并 | ✅ |
| desktop/.../components/AutomationPanel.tsx | M：reload 失败保数据+Add 防重复 | 无 | A | ADAPTED | 合并 | ✅ |
| desktop/.../chat/AssistantMessage.tsx | M：suggestion 失败 UI | L0 后已重构 | **C** | ADAPTED | 经 Lingxi ContentBlock/语义 UI 表达，禁止回退架构 | ✅ |
| desktop/.../__tests__/components/AutomationPanel.test.tsx | 新增 | 无 | A | ADAPTED | 移植 | ✅ |
| desktop/.../AssistantMessage.automation-suggestion.test.tsx | M | 无 | A | ADAPTED | 对齐 ContentBlock 后重写 | ✅ |
| tests/cron-store.test.ts | M | 无 | A | ADAPTED | 合并 | ✅ |
| tests/desk-route-cron.test.ts | M | 无 | A | ADAPTED | 合并 | ✅ |

## 集群 4：Markdown 裸 URL（2870af8e）

| path | upstream change | overlap | risk | decision | implementation | status |
|---|---|---|---|---|---|---|
| desktop/src/react/editor/md-decorations.ts | M：裸 URL 保持可见 | 无 | A | ADOPTED | 移植 patch | ✅ |
| desktop/.../__tests__/editor/md-decorations.test.ts | M | 无 | A | ADOPTED | 移植 | ✅ |

## 集群 5：Context Ring（d356e6ce）

| path | upstream change | overlap | risk | decision | implementation | status |
|---|---|---|---|---|---|---|
| desktop/.../input/ContextRing.tsx | M：压缩动作优先级排序 | 无 | A | ADOPTED | 移植 | ✅ |
| desktop/.../__tests__/components/context-ring.test.tsx | M | 无 | A | ADOPTED | 移植 | ✅ |

## 集群 6：Windows seed 清理（ecc2c055）

| path | upstream change | overlap | risk | decision | implementation | status |
|---|---|---|---|---|---|---|
| build/installer.nsh | M：overlay 更新清理旧 resources/seed | L0 后有改 | B | ADAPTED | 合并，禁触 LINGXI_HOME | ✅ |
| tests/windows-installer-contract.test.ts | M | 无 | A | ADAPTED | 合并断言 | ✅ |

## 集群 7：i18n（贯穿多 commit）

| path | upstream change | overlap | risk | decision | implementation | status |
|---|---|---|---|---|---|---|
| desktop/src/locales/{zh,zh-TW,en,ja,ko}.json | M：dream/persona/automation/context-ring 新 key | 全部 L0 后有改 | B | ADAPTED | **仅 key-level merge**，禁止整文件覆盖 | ✅ |

## 集群 8：构建脚本与生成物

| path | upstream change | overlap | risk | decision | implementation | status |
|---|---|---|---|---|---|---|
| scripts/compute-cli-closure.mjs | M：justification 不再引用易变行号 | 无 | A | ADOPTED | 移植 | ✅ |
| build/cli-runtime-closure.json | M | 生成物 | D | REGENERATED | Lingxi 生成器重新生成 | ✅ |
| build/persistence-schema-fingerprint.json | M | 生成物 | D | REGENERATED | 同上 | ✅ |
| build/persistence-store-inventory.json | M | 生成物 | D | REGENERATED | 同上 | ✅ |
| export-manifest.json | M | 生成物 | D | REGENERATED | 同上 | ✅ |

## 集群 9：产品差异（INTENTIONAL_DIVERGENCE）

| path | upstream change | decision | 理由 | status |
|---|---|---|---|---|
| package.json | 仅 version 0.444.1→0.447.4 | INTENTIONAL_DIVERGENCE | Lingxi 自有 name/version/pi 0.84.1/发布体系；上游无新依赖 | ✅ |
| package-lock.json | 随 version | INTENTIONAL_DIVERGENCE | 同上 | ✅ |
| release-digest.v1.json | v0.446.6/v0.447.4 摘要 | INTENTIONAL_DIVERGENCE | Lingxi 发布历史独立 | ✅ |
| release-digest.v2.json | 同上 | INTENTIONAL_DIVERGENCE | 同上 | ✅ |

## 统计（终态 2026-08-20）

- 总路径 133：ADOPTED 18 / ADAPTED 102 / REGENERATED 5 / INTENTIONAL_DIVERGENCE 4（13 个 R100 模板重命名按 ADOPTED 计入；Dream i18n key 已含于 ADAPTED locale 行）
- UNKNOWN / IGNORED：0
- 全部实现落点与测试见 UPSTREAM_SYNC_AUDIT.md；逐阶段测试记录见 PROGRESS.md
