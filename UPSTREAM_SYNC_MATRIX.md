# UPSTREAM_SYNC_MATRIX — v0.444.1 → v0.447.4

> 本文件由 `node .sync-audit/build-sync-matrix.mjs` 从 `.sync-audit/upstream-sync-matrix.json` 生成，禁止手改。
> Source-JSON-SHA256: dd8740eac1f73f08b90a89838a45bdebe4ec063f64ac7dd400f50494646c4951

## 审计坐标

```
U0 = cc19cb49b0786d61ed723764e0a83baf87887270  (openhanako v0.444.1)
U1 = c6d0405294be67cb134c2758f6472748ee73e2be  (openhanako v0.447.4)
L0 = 97595264ead8735a04559507ddaade25db8a4e15  (Lingxi v0.444.1 同步完成点, PR #2)
L1 = ca0b417e36a6a1f80947458aaed328a25718e41b  (本轮同步开始时 main)
VERIFIED_SOURCE_SHA = 22492163a8f107204d219b0da5ea1bbeaf243b1e
```

## 统计（脚本计算，禁止人工填写）

```
Total upstream paths: 133
ADOPTED: 25
ADAPTED: 100
REGENERATED: 4
INTENTIONAL_DIVERGENCE: 4
UNKNOWN: 0
MISSING: 0
DUPLICATE: 0
25 + 100 + 4 + 4 = 133
```

冲突等级：A=Lingxi未改动 / B=双方改同文件职责不冲突 / C=Lingxi已重构该职责 / D=产品差异或生成物。

## 逐路径矩阵（133 行，每个 upstream changed path 一行）

| # | upstream path | change | upstream commits | cluster | overlap | class | disposition | Lingxi implementation | test evidence | notes | status |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | build/cli-runtime-closure.json | M | 9dd70cd5,408b7a18,f29d15f2,def3e661,bed24b93,d96b5d67 | dream,build-receipts,agents-md-persona | 生成物 | D | REGENERATED | Lingxi 生成器（scripts/compute-cli-closure.mjs）在最终源码树重新生成，禁止从 upstream 复制 | 二次生成 git diff 为零（deterministic 验证）+ npm run build 链路 |  | ✅ |
| 2 | build/installer.nsh | M | ecc2c055 | windows-seed-cleanup | L0 后有改 | B | ADAPTED | overlay 更新时 RMDir /r "$INSTDIR\resources\seed"；严格限于安装目录应用自有 seed，禁触 LINGXI_HOME/用户数据 | tests/windows-installer-contract.test.ts（19 用例） | 真实 Windows 安装器执行未进行（宿主平台限制），contract 测试为门禁 | ✅ |
| 3 | build/persistence-schema-fingerprint.json | M | 9dd70cd5,61a2a6bf,bed24b93 | dream,automation-recovery,agents-md-persona | 生成物 | D | REGENERATED | Lingxi 生成器重新生成；compatible 增补 + compatibility-reason，DATA_EPOCH 不变 | persistence fingerprint 定向校验 + 二次生成零漂移 |  | ✅ |
| 4 | build/persistence-store-inventory.json | M | 9dd70cd5,408b7a18,61a2a6bf,bed24b93 | dream,automation-recovery,agents-md-persona | 生成物 | D | REGENERATED | Lingxi 生成器重新生成 | persistence store inventory 定向校验 + 二次生成零漂移 |  | ✅ |
| 5 | core/agent-manager.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 默认人格文件创建改 AGENTS 命名；本轮 setCallbacks 接线 getFailedPersonaRename | tests/agent-manager-create-defaults.test.ts（26 用例）+ migration smoke |  | ✅ |
| 6 | core/agent.ts | M | 9dd70cd5,bed24b93 | dream,agents-md-persona | 双方均改（dream 模型槽 + persona） | C | ADAPTED | 人格路径 + workspace excludeFiles + getResolvedMemoryModel 闭包；本轮 readAgentsMdSource/_readPublicAgentsMd 消费 migration-degraded fallback | tests/agents-md-startup-migration.test.ts（Agent 真实链路断言）+ tests/memory-dream-memory-slot.test.ts + agent 系列定向测试 | 上游 dream 与 persona 两集群均触及 | ✅ |
| 7 | core/agents-md-migration.ts | A | bed24b93 | agents-md-persona | 无 | A | ADOPTED | 同名移植 + 品牌检查；本轮扩展 failedDetails 结构化失败记录 + buildFailedPersonaRenameIndex（migration-degraded 运行时状态） | tests/agents-md-startup-migration.test.ts（11 用例，含 5 个 degraded fallback 用例）+ migration smoke 23/23 |  | ✅ |
| 8 | core/bridge-session-manager.ts | M | bed24b93 | agents-md-persona | L0 后有改 | B | ADAPTED | 结构化合并（workspace excludeFiles 接线之一） | tests/bridge-session-orphan-repair.test.ts + tests/bridge-session-teardown.test.ts + tests/workspace-instruction-files-exclude.test.ts（bridge 路径用例） |  | ✅ |
| 9 | core/engine.ts | M | bed24b93 | agents-md-persona | L0 后有改 | B | ADAPTED | 结构化合并 + 启动步 agents-md-rename；本轮把失败结果保留为 _failedPersonaRenames 运行时状态并暴露 getFailedPersonaRename | tests/agents-md-startup-migration.test.ts + migration smoke 23/23 + 全量套件 engine 启动链路 |  | ✅ |
| 10 | core/first-run.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 种子人格文件名改 AGENTS 命名 | tests/first-run-default-workspace.test.ts + migration smoke（首跑清洁断言） |  | ✅ |
| 11 | core/llm-utils.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 合并命名更新 | 全量套件内 llm-utils 消费链路（agent/system prompt 组装测试） |  | ✅ |
| 12 | core/persona-source.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | PersonaKind identity|agents + agentPersonaFilePaths()；本轮新增显式 migrationFallback 参数（仅启动失败记录可构造） | tests/persona-source.test.ts（15 用例）+ tests/agents-md-startup-migration.test.ts |  | ✅ |
| 13 | core/session-coordinator.ts | M | bed24b93 | agents-md-persona | L0 后有改 | B | ADAPTED | 结构化合并（workspace excludeFiles 接线之一） | tests/workspace-instruction-files-exclude.test.ts（desktop session 路径用例）+ tests/session-tool-gating.test.ts |  | ✅ |
| 14 | core/workspace-instruction-files.ts | M | bed24b93 | agents-md-persona | 无 | A | ADOPTED | 新增 excludeFiles（精确绝对路径排除，杜绝人格文件重复注入） | tests/workspace-instruction-files-exclude.test.ts（4 用例） |  | ✅ |
| 15 | desktop/src/locales/en.json | M | 9dd70cd5,d356e6ce,408b7a18,06e17341,bed24b93 | dream,context-ring,agents-md-persona | L0 后有改 | B | ADAPTED | key-level merge（同 zh）；本轮追加 dream revisions diff 5 key | tests/i18n-locale-parity.test.ts + tests/react-locale-coverage.test.ts | 禁止整文件覆盖 | ✅ |
| 16 | desktop/src/locales/ja.json | M | 9dd70cd5,d356e6ce,408b7a18,06e17341,bed24b93 | dream,context-ring,agents-md-persona | L0 后有改 | B | ADAPTED | key-level merge（同 zh）；本轮追加 dream revisions diff 5 key | tests/i18n-locale-parity.test.ts + tests/react-locale-coverage.test.ts + scripts/i18n-backfill-ja.json | 禁止整文件覆盖 | ✅ |
| 17 | desktop/src/locales/ko.json | M | 9dd70cd5,d356e6ce,408b7a18,06e17341,bed24b93 | dream,context-ring,agents-md-persona | L0 后有改 | B | ADAPTED | key-level merge（同 zh）；本轮追加 dream revisions diff 5 key | tests/i18n-locale-parity.test.ts + tests/react-locale-coverage.test.ts + scripts/i18n-backfill-ko.json | 禁止整文件覆盖 | ✅ |
| 18 | desktop/src/locales/zh-TW.json | M | 9dd70cd5,d356e6ce,408b7a18,06e17341,bed24b93 | dream,context-ring,agents-md-persona | L0 后有改 | B | ADAPTED | key-level merge（同 zh）；本轮追加 dream revisions diff 5 key | tests/i18n-locale-parity.test.ts + tests/react-locale-coverage.test.ts | 禁止整文件覆盖 | ✅ |
| 19 | desktop/src/locales/zh.json | M | 9dd70cd5,d356e6ce,408b7a18,06e17341,bed24b93 | dream,context-ring,agents-md-persona | L0 后有改 | B | ADAPTED | key-level merge：删 ishiki* 增 agentsMd*/dream 树/error.code.dream*；本轮追加 dream revisions diff 5 key | tests/i18n-locale-parity.test.ts + tests/react-locale-coverage.test.ts | 禁止整文件覆盖 | ✅ |
| 20 | desktop/src/react/__tests__/components/AssistantMessage.automation-suggestion.test.tsx | M | a14a13bc | automation-recovery | 无 | A | ADAPTED | 对齐 ContentBlock 架构后重写 | 本文件自身全绿 |  | ✅ |
| 21 | desktop/src/react/__tests__/components/AutomationPanel.test.tsx | A | a14a13bc | automation-recovery | 无 | A | ADAPTED | 移植 | 本文件自身全绿 |  | ✅ |
| 22 | desktop/src/react/__tests__/components/context-ring.test.tsx | M | d356e6ce | context-ring | 无 | A | ADOPTED | 移植 | 本文件自身全绿 |  | ✅ |
| 23 | desktop/src/react/__tests__/editor/md-decorations.test.ts | M | 2870af8e | markdown-bare-url | 无 | A | ADOPTED | 移植 | 本文件自身全绿 |  | ✅ |
| 24 | desktop/src/react/__tests__/settings/AgentMemory.test.tsx | M | 9dd70cd5,06e17341 | dream | 无 | A | ADAPTED | 合并断言 | 本文件自身全绿（5 用例） |  | ✅ |
| 25 | desktop/src/react/__tests__/settings/AgentTab.test.tsx | M | bed24b93 | agents-md-persona | 3 个 desktop 测试 overlap 之一 | B | ADAPTED | 合并 | 本文件自身全绿 |  | ✅ |
| 26 | desktop/src/react/__tests__/settings/BridgeTab.credentials.test.tsx | M | bed24b93 | agents-md-persona | 3 个 desktop 测试 overlap 之一 | B | ADAPTED | 合并 | 本文件自身全绿 |  | ✅ |
| 27 | desktop/src/react/__tests__/settings/BridgeTab.permission-mode.test.tsx | M | bed24b93 | agents-md-persona | 3 个 desktop 测试 overlap 之一 | B | ADAPTED | 合并 | 本文件自身全绿 |  | ✅ |
| 28 | desktop/src/react/__tests__/settings/actions.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 合并 | 本文件自身全绿 |  | ✅ |
| 29 | desktop/src/react/__tests__/settings/helpers.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 合并 | 本文件自身全绿 |  | ✅ |
| 30 | desktop/src/react/__tests__/settings/useBridgeState.snapshot.test.tsx | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 合并 | 本文件自身全绿 |  | ✅ |
| 31 | desktop/src/react/__tests__/shared/error-user-messages.test.ts | M | 06e17341 | dream | 无 | A | ADAPTED | 合并断言 | 本文件自身全绿（26 用例） |  | ✅ |
| 32 | desktop/src/react/components/AutomationPanel.tsx | M | a14a13bc | automation-recovery | 无 | A | ADAPTED | reload 失败保留上一次 jobs + badge；Add 防重复（addingManualJob 锁） | desktop/src/react/__tests__/components/AutomationPanel.test.tsx |  | ✅ |
| 33 | desktop/src/react/components/chat/AssistantMessage.tsx | M | a14a13bc | automation-recovery | L0 后已重构（聊天语义管线） | C | ADAPTED | automation suggestion 失败 UI 经 Lingxi ContentBlock/renderer registry 架构表达，未套 upstream 旧 JSX patch | desktop/src/react/__tests__/components/AssistantMessage.automation-suggestion.test.tsx | 禁止回退聊天架构 | ✅ |
| 34 | desktop/src/react/components/input/ContextRing.tsx | M | d356e6ce | context-ring | 无 | A | ADOPTED | 移植：压缩动作优先级排序 | desktop/src/react/__tests__/components/context-ring.test.tsx |  | ✅ |
| 35 | desktop/src/react/editor/md-decorations.ts | M | 2870af8e | markdown-bare-url | 无 | A | ADOPTED | 移植 patch：裸 URL 不再被装饰/破坏 | desktop/src/react/__tests__/editor/md-decorations.test.ts |  | ✅ |
| 36 | desktop/src/react/settings/Settings.module.css | M | 9dd70cd5,bed24b93 | dream,agents-md-persona | L0 后有改 | B | ADAPTED | 追加 dream 样式段（不整文件覆盖） | desktop/src/react/settings/__tests__/SettingsContent.test.tsx + DreamRevisionBrowser/AgentMemory 组件测试 |  | ✅ |
| 37 | desktop/src/react/settings/__tests__/SettingsContent.test.tsx | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 合并 | 本文件自身全绿 |  | ✅ |
| 38 | desktop/src/react/settings/actions.ts | M | bed24b93 | agents-md-persona | 部分 overlap | B | ADAPTED | ishiki→agents 文案/API 逐文件合并 | desktop/src/react/__tests__/settings/actions.test.ts |  | ✅ |
| 39 | desktop/src/react/settings/helpers.ts | M | bed24b93 | agents-md-persona | 部分 overlap | B | ADAPTED | ishiki→agents 文案/API 逐文件合并 | desktop/src/react/__tests__/settings/helpers.test.ts |  | ✅ |
| 40 | desktop/src/react/settings/overlays/CharacterCardPreviewOverlay.tsx | M | bed24b93 | agents-md-persona | 部分 overlap | B | ADAPTED | ishiki→agents 文案/API 逐文件合并 | desktop/src/react/settings/__tests__/SettingsContent.test.tsx |  | ✅ |
| 41 | desktop/src/react/settings/settings-search-index.ts | M | bed24b93 | agents-md-persona | L0 后有改 | B | ADAPTED | 追加 dream 搜索项 | desktop/src/react/settings/__tests__/SettingsContent.test.tsx（搜索索引消费侧） |  | ✅ |
| 42 | desktop/src/react/settings/store.ts | M | bed24b93 | agents-md-persona | 部分 overlap | B | ADAPTED | ishiki→agents 文案/API 逐文件合并 | desktop/src/react/__tests__/settings/actions.test.ts + helpers.test.ts + useBridgeState.snapshot.test.tsx |  | ✅ |
| 43 | desktop/src/react/settings/tabs/AgentTab.tsx | M | 9dd70cd5,bed24b93 | dream,agents-md-persona | L0 后有改 | C | ADAPTED | 按当前结构合并 persona 文案 + dream 入口 | desktop/src/react/__tests__/settings/AgentTab.test.tsx | dream 与 agents-md-persona 两集群均触及 | ✅ |
| 44 | desktop/src/react/settings/tabs/BridgeTab.tsx | M | bed24b93 | agents-md-persona | 部分 overlap | B | ADAPTED | ishiki→agents 文案/API 逐文件合并 | desktop/src/react/__tests__/settings/BridgeTab.credentials.test.tsx + BridgeTab.permission-mode.test.tsx |  | ✅ |
| 45 | desktop/src/react/settings/tabs/agent/AgentMemory.tsx | M | 9dd70cd5 | dream | 无 | A | ADAPTED | Dream 区嵌入合并进当前组件 | desktop/src/react/__tests__/settings/AgentMemory.test.tsx（5 用例） |  | ✅ |
| 46 | desktop/src/react/settings/tabs/agent/AgentMemoryDream.tsx | A | 9dd70cd5,408b7a18,06e17341 | dream | 无 | A | ADAPTED | 移植并对齐 Lingxi settings API client（saveDreamAutoEnabled 走 /api/agents/:id/config） | desktop/src/react/__tests__/settings/AgentMemory.test.tsx（5 用例） |  | ✅ |
| 47 | desktop/src/react/settings/tabs/agent/DreamRevisionBrowser.module.css | A | 408b7a18 | dream | 无 | A | ADAPTED | 移植 + 本轮追加 diff 行样式（lineAdded/lineRemoved/lineSame，沿用 FileHistoryModal 色板约定） | DreamRevisionBrowser.test.tsx（diff 标记渲染断言） |  | ✅ |
| 48 | desktop/src/react/settings/tabs/agent/DreamRevisionBrowser.tsx | A | 408b7a18,06e17341 | dream | 无 | A | ADAPTED | 移植；本轮升级为 current-vs-revision 统一 diff（复用 desktop/src/react/utils/line-diff.ts），确认前现取 current，相同版本禁用恢复 | desktop/src/react/settings/tabs/agent/__tests__/DreamRevisionBrowser.test.tsx（6 用例，含 A–F diff/确认/刷新契约） |  | ✅ |
| 49 | desktop/src/react/settings/tabs/agent/__tests__/DreamRevisionBrowser.test.tsx | A | 408b7a18,06e17341 | dream | 无 | A | ADAPTED | 移植；本轮重写为 current-vs-revision diff 契约（任务书 A–F 用例） | 本文件自身全绿（6 用例） |  | ✅ |
| 50 | desktop/src/react/settings/tabs/agent/agent-memory-dream-actions.ts | A | 9dd70cd5,408b7a18,06e17341 | dream | 无 | A | ADAPTED | 移植；本轮 loadDreamRevision 改返回 { revision, current } 并新增 dreamSectionsEqual | DreamRevisionBrowser.test.tsx（经 importOriginal 保留纯函数真实实现） |  | ✅ |
| 51 | desktop/src/react/settings/tabs/agent/dream-error-presenter.ts | A | 06e17341 | dream | 无 | A | ADOPTED | 同名移植：dream_* 错误码 → i18n | DreamRevisionBrowser.test.tsx（本地化恢复错误用例） |  | ✅ |
| 52 | desktop/src/react/settings/tabs/bridge/useBridgeState.ts | M | bed24b93 | agents-md-persona | 部分 overlap | B | ADAPTED | ishiki→agents 文案/API 逐文件合并 | desktop/src/react/__tests__/settings/useBridgeState.snapshot.test.tsx |  | ✅ |
| 53 | export-manifest.json | M | 9dd70cd5,408b7a18,f29d15f2,bed24b93 | dream,agents-md-persona | 生成物 | D | REGENERATED | Lingxi 手工策展权威源重新生成（704→712） | export manifest 定向校验（tests 内 manifest 一致性用例） |  | ✅ |
| 54 | hub/agent-executor.ts | M | bed24b93 | agents-md-persona | L0 后有改 | B | ADAPTED | 结构化合并 | 全量套件内 agent-executor 链路（hub 执行测试） |  | ✅ |
| 55 | lib/agents-public-templates/butter.md<br>← lib/public-ishiki-templates/butter.md | R100 | bed24b93 | agents-md-persona | 无（纯改名） | A | ADOPTED | R100 目录改名落地 | tests/agents-md-startup-migration.test.ts（public 变体迁移用例）+ 打包 smoke | R100 纯改名 | ✅ |
| 56 | lib/agents-public-templates/en/butter.md<br>← lib/public-ishiki-templates/en/butter.md | R100 | bed24b93 | agents-md-persona | 无（纯改名） | A | ADOPTED | R100 目录改名落地 | tests/agents-md-startup-migration.test.ts + 打包 smoke | R100 纯改名 | ✅ |
| 57 | lib/agents-public-templates/en/hanako.md<br>← lib/public-ishiki-templates/en/hanako.md | R100 | bed24b93 | agents-md-persona | 品牌路径映射 | A | ADAPTED | 上游 AGENTS 模板行为与内容同步；Lingxi 保留产品品牌路径 hanako.md → lingxi.md | tests/agents-md-startup-migration.test.ts + 打包 smoke | 品牌级路径映射，分类为 ADAPTED | ✅ |
| 58 | lib/agents-public-templates/en/ming.md<br>← lib/public-ishiki-templates/en/ming.md | R100 | bed24b93 | agents-md-persona | 无（纯改名） | A | ADOPTED | R100 目录改名落地 | tests/agents-md-startup-migration.test.ts + 打包 smoke | R100 纯改名 | ✅ |
| 59 | lib/agents-public-templates/hanako.md<br>← lib/public-ishiki-templates/hanako.md | R100 | bed24b93 | agents-md-persona | 品牌路径映射 | A | ADAPTED | 上游 AGENTS 模板行为与内容同步；Lingxi 保留产品品牌路径 hanako.md → lingxi.md | tests/agents-md-startup-migration.test.ts + 打包 smoke | 品牌级路径映射，分类为 ADAPTED | ✅ |
| 60 | lib/agents-public-templates/ming.md<br>← lib/public-ishiki-templates/ming.md | R100 | bed24b93 | agents-md-persona | 无（纯改名） | A | ADOPTED | R100 目录改名落地 | tests/agents-md-startup-migration.test.ts + 打包 smoke | R100 纯改名 | ✅ |
| 61 | lib/agents-templates/butter.md<br>← lib/ishiki-templates/butter.md | R100 | bed24b93 | agents-md-persona | 无（纯改名） | A | ADOPTED | R100 目录改名落地 | tests/persona-source.test.ts（模板回落链）+ 打包 smoke（agents-templates 存在、ishiki-templates 不残留） | R100 纯改名 | ✅ |
| 62 | lib/agents-templates/en/butter.md<br>← lib/ishiki-templates/en/butter.md | R100 | bed24b93 | agents-md-persona | 无（纯改名） | A | ADOPTED | R100 目录改名落地 | tests/persona-source.test.ts（en 语言模板回落用例）+ 打包 smoke | R100 纯改名 | ✅ |
| 63 | lib/agents-templates/en/hanako.md<br>← lib/ishiki-templates/en/hanako.md | R100 | bed24b93 | agents-md-persona | 品牌路径映射 | A | ADAPTED | 上游 AGENTS 模板行为与内容同步；Lingxi 保留产品品牌路径 hanako.md → lingxi.md | tests/persona-source.test.ts + 打包 smoke | 品牌级路径映射，分类为 ADAPTED | ✅ |
| 64 | lib/agents-templates/en/ming.md<br>← lib/ishiki-templates/en/ming.md | R100 | bed24b93 | agents-md-persona | 无（纯改名） | A | ADOPTED | R100 目录改名落地 | tests/persona-source.test.ts + 打包 smoke | R100 纯改名 | ✅ |
| 65 | lib/agents-templates/hanako.md<br>← lib/ishiki-templates/hanako.md | R100 | bed24b93 | agents-md-persona | 品牌路径映射 | A | ADAPTED | 上游 AGENTS 模板行为与内容同步；Lingxi 保留产品品牌路径 hanako.md → lingxi.md | tests/persona-source.test.ts + 打包 smoke | 品牌级路径映射，分类为 ADAPTED | ✅ |
| 66 | lib/agents-templates/ming.md<br>← lib/ishiki-templates/ming.md | R100 | bed24b93 | agents-md-persona | 无（纯改名） | A | ADOPTED | R100 目录改名落地 | tests/persona-source.test.ts + 打包 smoke | R100 纯改名 | ✅ |
| 67 | lib/agents.example.md<br>← lib/ishiki.example.md | R100 | bed24b93 | agents-md-persona | 有 Lingxi 品牌内容 | A | ADAPTED | 改名并保留 Lingxi 品牌内容（example 兜底人格） | tests/persona-source.test.ts（example 兜底层用例） | R100 但内容含品牌差异，故 ADAPTED | ✅ |
| 68 | lib/character-cards/service.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 导出写新 key（prompts.agents/publicAgents），导入永久兼容 legacy key | tests/character-card-import.test.ts |  | ✅ |
| 69 | lib/desk/cron-store.ts | M | a14a13bc | automation-recovery | Lingxi 扩展在 L0 前并入 | B | ADAPTED | 行为不变量提取后合并：tmp 校验→逐字节损坏备份→原子提升；保留 configRevision/storeRevision/重入保护 | tests/cron-store.test.ts | 稳定错误码 cron_store_corrupt/unavailable/recovery_failed；禁止静默返回 [] | ✅ |
| 70 | lib/diary/diary-writer.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 合并命名更新 | 全量套件内 diary 链路测试 |  | ✅ |
| 71 | lib/memory/dream/memory-units.ts | A | 408b7a18,f29d15f2,ca6dbf95 | dream | 无 | A | ADAPTED | 同名移植，接 Lingxi memory 数据布局（facts/today/week/longterm） | tests/memory-dream-units.test.ts（11 用例） |  | ✅ |
| 72 | lib/memory/dream/model-runner.ts | A | 9dd70cd5,476b2d7c,408b7a18,f29d15f2,ca6dbf95,483c5fe0 | dream | 无 | A | ADAPTED | 模型解析改走 engine.resolveAuxiliaryExecution("memory", { agentId })，禁止 utility/chat 回落 | tests/memory-dream-model-runner.test.ts（10 用例）+ tests/memory-dream-memory-slot.test.ts（3 用例，锁槽位契约） | 软目标 + 5000 字符硬上限 | ✅ |
| 73 | lib/memory/dream/revision-store.ts | A | 9dd70cd5,408b7a18 | dream | 无 | A | ADAPTED | 同名移植，入 Lingxi store registry；snapshotDreamSections 复用为 current-vs-revision diff 的当前快照源 | tests/memory-dream-revision.test.ts（8 用例） |  | ✅ |
| 74 | lib/memory/dream/runner.ts | A | 9dd70cd5,476b2d7c,408b7a18,f29d15f2,ca6dbf95,483c5fe0,06e17341 | dream | 无 | A | ADAPTED | 同名移植：编排 + per-agent 并发锁 + memory_changed 竞态 + revision-before-write | tests/memory-dream-runner.test.ts（11 用例）+ tests/memory-dream-memory-slot.test.ts |  | ✅ |
| 75 | lib/memory/dream/state-store.ts | A | 9dd70cd5,408b7a18,06e17341 | dream | 无 | A | ADAPTED | 同名移植：Dream 状态持久化 + dream_* 稳定错误码 | tests/memory-dream-runner.test.ts + tests/memory-dream-route.test.ts（错误码映射断言） |  | ✅ |
| 76 | lib/memory/memory-ticker.ts | M | 9dd70cd5,408b7a18,f29d15f2,06e17341 | dream | 无（L0 后未改） | A | ADAPTED | 合并 Dream 触发钩子进当前 ticker，gated on getDreamAutoEnabled()===true | tests/memory-ticker-dream.test.ts（4 用例） |  | ✅ |
| 77 | lib/memory/prompts/dream.ts | A | 9dd70cd5,476b2d7c,408b7a18,f29d15f2,ca6dbf95,483c5fe0 | dream | 无 | A | ADOPTED | 同名移植（5 阶段 prompt，校对 Lingxi 术语） | tests/memory-dream-model-runner.test.ts / tests/memory-dream-runner.test.ts（prompt 经 runner 链路断言） |  | ✅ |
| 78 | lib/sandbox/policy.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 合并命名更新 | 全量套件内 sandbox policy 测试 |  | ✅ |
| 79 | package-lock.json | M | 9dd70cd5,d356e6ce,476b2d7c,408b7a18,f29d15f2,ca6dbf95,483c5fe0,06e17341,2870af8e,def3e661,a14a13bc,61a2a6bf,bed24b93,b3927f07,d96b5d67,ecc2c055,c6d04052 | dream,context-ring,markdown-bare-url,build-receipts,automation-recovery,agents-md-persona,windows-seed-cleanup,release-digest | 产品差异 | D | INTENTIONAL_DIVERGENCE | 随 package.json；上游无新依赖可移植 | npm install 链路 + 全量套件 |  | ✅ |
| 80 | package.json | M | 9dd70cd5,d356e6ce,476b2d7c,408b7a18,f29d15f2,ca6dbf95,483c5fe0,06e17341,2870af8e,def3e661,a14a13bc,61a2a6bf,bed24b93,b3927f07,d96b5d67,ecc2c055,c6d04052 | dream,context-ring,markdown-bare-url,build-receipts,automation-recovery,agents-md-persona,windows-seed-cleanup,release-digest | 产品差异 | D | INTENTIONAL_DIVERGENCE | Lingxi 自有 name/version/pi 0.84.1/发布体系；上游 U0..U1 仅 version bump 无新依赖 | npm run typecheck + 全量套件（依赖未变更前提下的回归证据） | lingxi.upstreamVersion=0.447.4 保留 | ✅ |
| 81 | release-digest.v1.json | M | 5f08a4f3,c6d04052 | release-digest | 产品差异 | D | INTENTIONAL_DIVERGENCE | Lingxi 发布历史独立；上游 digest 拷贝即伪造 | release-digest 由 scripts/generate-release-digest 系列维护（包内回退数据） |  | ✅ |
| 82 | release-digest.v2.json | M | 5f08a4f3,c6d04052 | release-digest | 产品差异 | D | INTENTIONAL_DIVERGENCE | 同上 | release-digest 由 scripts/generate-release-digest 系列维护（包内回退数据） |  | ✅ |
| 83 | scripts/build-server-open.mjs | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 同上（模板目录改名） | tests/build-server-open.test.ts |  | ✅ |
| 84 | scripts/build-server.mjs | M | bed24b93 | agents-md-persona | L0 后有改 | B | ADAPTED | 仅套用模板目录改名（ishiki-templates→agents-templates） | tests/build-server-open.test.ts + npm run build:server 产物抽检（seed 含 agents-templates） |  | ✅ |
| 85 | scripts/compute-cli-closure.mjs | M | bed24b93,d96b5d67 | agents-md-persona,build-receipts | 无 | A | ADOPTED | 移植：justification 不再引用易变行号 | build/cli-runtime-closure.json 由该生成器重新生成且二次生成零漂移（确定性验证） |  | ✅ |
| 86 | scripts/export-open-tree.mjs | M | bed24b93 | agents-md-persona | 无 | A | ADOPTED | 注释同步 | export-manifest.json 重新生成 + export manifest 定向校验 |  | ✅ |
| 87 | scripts/i18n-backfill-ja.json | M | bed24b93 | agents-md-persona | 无 | A | ADOPTED | noIshiki→noAgentsMd key 改名 | tests/i18n-locale-parity.test.ts |  | ✅ |
| 88 | scripts/i18n-backfill-ko.json | M | bed24b93 | agents-md-persona | 无 | A | ADOPTED | noIshiki→noAgentsMd key 改名 | tests/i18n-locale-parity.test.ts |  | ✅ |
| 89 | server/composition/open-root.ts | M | 9dd70cd5 | dream | 无 | A | ADOPTED | 同位置挂载 createMemoryDreamRoute | tests/server-composition-boundary.test.ts（8 用例）+ npm run lint:boundary |  | ✅ |
| 90 | server/http/route-security.ts | M | 9dd70cd5,bed24b93 | dream,agents-md-persona | 无 | A | ADOPTED | dream 路由安全分级同步 | tests/http-route-security.test.ts |  | ✅ |
| 91 | server/routes/agents.ts | M | 9dd70cd5,bed24b93 | dream,agents-md-persona | 无 | A | ADAPTED | persona 双路由（canonical /agents-md + legacy /ishiki alias 同 handler）+ dream auto_enabled 布尔校验（400 先于 updateConfig） | tests/agents-route.test.ts（30 用例）+ tests/agent-config-tools-disabled.test.ts（12 用例，含本轮补齐的 3 个 dream 契约） | upstream 两个集群（dream + agents-md-persona）都改了此文件，此处只记一行 | ✅ |
| 92 | server/routes/desk.ts | M | a14a13bc | automation-recovery | 无 | A | ADAPTED | 错误传播不泄露 fs 绝对路径 | tests/desk-route-cron.test.ts |  | ✅ |
| 93 | server/routes/memory-dream.ts | A | 9dd70cd5,408b7a18,06e17341 | dream | 无 | A | ADAPTED | 同名移植对齐 Lingxi 路由约定；本轮扩展 revision detail 响应附带后端现读的 current 快照（current-vs-revision diff 数据源） | tests/memory-dream-route.test.ts（7 用例，含 current 快照断言） |  | ✅ |
| 94 | server/routes/settings-snapshot.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 合并命名更新 | tests/settings-snapshot-route.test.ts |  | ✅ |
| 95 | shared/error-user-messages.ts | M | 06e17341 | dream | 无 | A | ADOPTED | dream_* 错误码 → 用户文案映射合并 | desktop/src/react/__tests__/shared/error-user-messages.test.ts（26 用例） |  | ✅ |
| 96 | shared/persistence/store-registry.ts | M | 9dd70cd5,bed24b93 | dream,agents-md-persona | L0 后有改 | B | ADAPTED | 结构化合并：dream state/revision store 登记 + AGENTS.md 路径模式/rename 规则 | persistence fingerprint 校验（build/persistence-schema-fingerprint.json 重新生成 + compatible 判定）+ migration smoke 23/23 | DATA_EPOCH 不变 | ✅ |
| 97 | skills2set/character-creator/SKILL.md | M | b3927f07 | agents-md-persona | Lingxi 品牌 | A | ADAPTED | 保留品牌文案，命名对齐 AGENTS.md 协议 | 打包 smoke 抽检（bundled skills 随产物带出）+ 全量套件 skills 链路 |  | ✅ |
| 98 | skills2set/character-creator/references/anti-slop.md | M | b3927f07 | agents-md-persona | Lingxi 品牌 | A | ADAPTED | 保留品牌文案，命名对齐 AGENTS.md 协议 | 打包 smoke 抽检（bundled skills 随产物带出） |  | ✅ |
| 99 | skills2set/character-creator/references/card-format.md | M | b3927f07 | agents-md-persona | Lingxi 品牌 | A | ADAPTED | 保留品牌文案，命名对齐 AGENTS.md 协议 | 打包 smoke 抽检（bundled skills 随产物带出） |  | ✅ |
| 100 | skills2set/user-guide/SKILL.md | M | b3927f07 | agents-md-persona | Lingxi 品牌 | A | ADAPTED | 保留品牌文案，命名对齐 AGENTS.md 协议 | 打包 smoke 抽检（bundled skills 随产物带出） |  | ✅ |
| 101 | tests/agent-config-tools-disabled.test.ts | M | 9dd70cd5 | dream | 无 | A | ADAPTED | 合并；本轮补齐 upstream 的 dream auto_enabled 正反契约（200 透传 / 400 拒非布尔且 updateConfig 未被调用） | 本文件自身全绿（12 用例，含 3 个 dream 契约） | 上一轮矩阵误标 ✅，本轮实补后转真绿 | ✅ |
| 102 | tests/agent-description.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新 | 本文件自身全绿 |  | ✅ |
| 103 | tests/agent-experience-toggle.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新 | 本文件自身全绿 |  | ✅ |
| 104 | tests/agent-interactive-card-tools.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新 | 本文件自身全绿 |  | ✅ |
| 105 | tests/agent-locale-resolution.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新 | 本文件自身全绿（4 用例） |  | ✅ |
| 106 | tests/agent-manager-create-defaults.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新 | 本文件自身全绿（26 用例） |  | ✅ |
| 107 | tests/agent-master-session-decoupling.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新 | 本文件自身全绿 |  | ✅ |
| 108 | tests/agent-system-prompt-section-order.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新 | 本文件自身全绿（2 用例） |  | ✅ |
| 109 | tests/agent-tools-conditional-injection.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新 | 本文件自身全绿 |  | ✅ |
| 110 | tests/agents-md-startup-migration.test.ts | A | bed24b93 | agents-md-persona | 无 | A | ADOPTED | 移植；本轮新增 5 个 migration-degraded 用例（失败保人格/无永久双读/新文件优先/下次成功/public 变体） | 本文件自身全绿（11 用例） |  | ✅ |
| 111 | tests/agents-route.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新（canonical + legacy alias 双路由） | 本文件自身全绿（30 用例） |  | ✅ |
| 112 | tests/bridge-session-orphan-repair.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新 | 本文件自身全绿 |  | ✅ |
| 113 | tests/bridge-session-teardown.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新 | 本文件自身全绿 |  | ✅ |
| 114 | tests/build-server-open.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新（模板目录改名） | 本文件自身全绿 |  | ✅ |
| 115 | tests/builtin-tool-permission-coverage.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新 | 本文件自身全绿 |  | ✅ |
| 116 | tests/channel-router-memory-master.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新 | 本文件自身全绿 |  | ✅ |
| 117 | tests/character-card-import.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新（legacy key 导入兼容） | 本文件自身全绿 |  | ✅ |
| 118 | tests/cron-store.test.ts | M | a14a13bc | automation-recovery | 无 | A | ADAPTED | 合并恢复断言（含 Lingxi 扩展字段保全） | 本文件自身全绿 |  | ✅ |
| 119 | tests/desk-route-cron.test.ts | M | a14a13bc | automation-recovery | 无 | A | ADAPTED | 合并错误传播断言 | 本文件自身全绿 |  | ✅ |
| 120 | tests/first-run-default-workspace.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新 | 本文件自身全绿 |  | ✅ |
| 121 | tests/http-route-security.test.ts | M | 9dd70cd5 | dream | 无 | A | ADAPTED | 合并 dream 路由断言 | 本文件自身全绿 |  | ✅ |
| 122 | tests/memory-dream-model-runner.test.ts | A | 9dd70cd5,476b2d7c,408b7a18,f29d15f2,ca6dbf95,483c5fe0 | dream | 无 | A | ADAPTED | 移植 + 新增 memory-slot 专项断言 | 本文件自身全绿（10 用例） |  | ✅ |
| 123 | tests/memory-dream-revision.test.ts | A | 9dd70cd5,408b7a18 | dream | 无 | A | ADAPTED | 移植 | 本文件自身全绿（8 用例） |  | ✅ |
| 124 | tests/memory-dream-route.test.ts | A | 9dd70cd5,408b7a18,06e17341 | dream | 无 | A | ADAPTED | 移植；本轮新增 detail 响应 current 快照断言 | 本文件自身全绿（7 用例） |  | ✅ |
| 125 | tests/memory-dream-runner.test.ts | A | 9dd70cd5,476b2d7c,408b7a18,f29d15f2,ca6dbf95,483c5fe0,06e17341 | dream | 无 | A | ADAPTED | 移植 | 本文件自身全绿（11 用例） |  | ✅ |
| 126 | tests/memory-dream-units.test.ts | A | 408b7a18,f29d15f2,ca6dbf95 | dream | 无 | A | ADAPTED | 移植 | 本文件自身全绿（11 用例） |  | ✅ |
| 127 | tests/memory-ticker-dream.test.ts | A | 9dd70cd5,f29d15f2 | dream | 无 | A | ADAPTED | 移植 | 本文件自身全绿（4 用例） |  | ✅ |
| 128 | tests/persona-source.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新 | 本文件自身全绿（15 用例） |  | ✅ |
| 129 | tests/server-composition-boundary.test.ts | M | 9dd70cd5 | dream | 无 | A | ADAPTED | 命名/行为断言更新（含 dream 路由挂载清单） | 本文件自身全绿（8 用例） |  | ✅ |
| 130 | tests/session-tool-gating.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新 | 本文件自身全绿 |  | ✅ |
| 131 | tests/settings-snapshot-route.test.ts | M | bed24b93 | agents-md-persona | 无 | A | ADAPTED | 命名/行为断言更新 | 本文件自身全绿 |  | ✅ |
| 132 | tests/windows-installer-contract.test.ts | M | ecc2c055 | windows-seed-cleanup | 无 | A | ADAPTED | 合并断言 + lingxiRemoveOwnedInstallTrees 宏名修正 | 本文件自身全绿（19 用例） |  | ✅ |
| 133 | tests/workspace-instruction-files-exclude.test.ts | A | bed24b93 | agents-md-persona | 无 | A | ADOPTED | 移植 | 本文件自身全绿（4 用例） |  | ✅ |
