# PROGRESS.md — 辅助模型语义 Slot 重构 + 审批 / 安全审查彻底隔离

> 执行者日志。断点续跑先读本文件。每完成一项立刻更新。

## 1. 起始 HEAD

```
0250f5fc41ad50c30dd08b5f4259524f2696115b
```

Working tree status: `M build/cli-runtime-closure.json`（无关本任务，不触碰）

## 2. 基线测试结果（Task 0 实测）

### typecheck

```
tsc --noEmit && tsc --noEmit -p tsconfig.node.json && tsc --noEmit -p tsconfig.test.json
→ 0 error
```

### Targeted tests

```
tests/approval-gateway.test.ts          → 30 passed
tests/engine-build-tools.test.ts        → 22 passed
tests/session-permission-wrapper.test.ts → 51 passed
```

### Full suite baseline（`/tmp/aux-slot-baseline.log`）

```
Test Files  31 failed | 1047 passed | 1 skipped (1079)
Tests       162 failed | 10711 passed | 7 skipped (10880)
```

### 失败测试清单（预先存在的基线失败）

主要根因：`better-sqlite3` 原生模块版本不匹配（`NODE_MODULE_VERSION 137 vs 127`）。

| 文件 | 失败数 | 根因分类 |
|------|--------|----------|
| tests/agent-master-session-decoupling.test.ts | 17 | sqlite 原生模块 |
| tests/session-manifest-coordinator.test.ts | 16 | sqlite 原生模块 |
| tests/session-manifest-engine.test.ts | 14 | sqlite 原生模块 |
| tests/session-manifest-store.test.ts | 14 | sqlite 原生模块 |
| tests/persistence-schema-tripwire.test.ts | 12 | sqlite 原生模块 |
| tests/file-history-store.test.ts | 9 | sqlite 原生模块 |
| tests/session-ownership-resolution.test.ts | 9 | sqlite 原生模块 |
| tests/file-history-service.test.ts | 7 | sqlite 原生模块 |
| tests/agent-tools-conditional-injection.test.ts | 7 | sqlite 原生模块 |
| tests/sessions-archived-route.test.ts | 7 | sqlite 原生模块 |
| tests/memory-search-channel-scope.test.ts | 5 | sqlite 原生模块 |
| tests/fact-store-cjk-search.test.ts | 5 | sqlite 原生模块 |
| tests/session-manifest-resolver.test.ts | 6 | sqlite 原生模块 |
| tests/session-manifest-branch-head.test.ts | 3 | sqlite 原生模块 |
| tests/fact-store-branch-replacement.test.ts | 3 | sqlite 原生模块 |
| tests/builtin-tool-permission-coverage.test.ts | 3 | sqlite 原生模块 |
| tests/session-coordinator-isolated-abort.test.ts | 3 | sqlite 原生模块 |
| tests/i18n-locale-parity.test.ts | 3 | i18n locale 缺 key |
| tests/data-epoch-checkpoint-provider.test.ts | 2 | sqlite 原生模块 |
| tests/session-coordinator-archived.test.ts | 2 | sqlite 原生模块 |
| tests/session-coordinator-tool-snapshot.test.ts | 2 | sqlite 原生模块 |
| tests/better-sqlite3-guardrail.test.ts | 1 | sqlite 原生模块 |
| tests/session-coordinator.test.ts | 1 | sqlite 原生模块 |
| tests/session-list-resilience.test.ts | 1 | sqlite 原生模块 |
| tests/session-tool-gating.test.ts | 1 | sqlite 原生模块 |
| tests/agent-interactive-card-tools.test.ts | 1 | sqlite 原生模块 |
| tests/open-boundary-lint.test.ts | 2 | 边界 lint |
| tests/api-health-session-store-integration.test.ts | 2 | sqlite / 超时 |
| tests/server-composition-boundary.test.ts | 1 | 边界 lint / 超时 |
| desktop/.../screenshot.test.ts | 1 | 截图头像 |
| desktop/.../settings-search-layout.test.ts | 2 | 设置布局 CSS |

**验收基准**：最终 full suite fail ≤ 162，skip ≤ 7，typecheck = 0 error。

## 3. 调用点 Inventory（Task 0.5）

### 现有架构总结

dual-utility 体系有**两条并行路径**：
- **路径 A（Agent 字段缓存）**：`agent._utilityModel`(死字段) / `agent._memoryModel`(=utility_large) → MemoryTicker / 日记
- **路径 B（Engine 现场解析）**：`engine.resolveUtilityConfigFresh()` → title/summarize/approval/install-skill

核心执行链：
```
Engine.resolveUtilityConfigFresh
  → ConfigCoordinator._utilityResolverArgs  // [agentConfig, getSharedModels(), getUtilityApi()]
  → ModelManager.resolveUtilityConfigFresh
  → ExecutionRouter.resolveUtilityConfigFresh  // 返回 dual {utility, utility_large, api_key, large_api_key, ...}
  → callTextConfigFromUtilityConfig(config, role)  // 选 utility 或 utility_large
  → callTextConfigFromResolvedModel → callText
```

可复用通用基础设施：`composeResolvedModelExecution` + `callTextConfigFromResolvedModel`（model-execution-config.ts）

### Inventory 表（旧符号 → 目标 Slot）

| 调用点 | file:line | 旧角色 | 真实业务语义 | 目标 Slot | fallback | 持久化 |
|--------|-----------|--------|-------------|-----------|----------|--------|
| summarizeTitle | llm-utils.ts:218→221 | utility | 会话标题生成 | **title** | 是(→chat) | 否 |
| generateAgentId | llm-utils.ts:501→506 | utility | agent ID slug 生成 | **title** | 是(→chat) | 否 |
| translateSkillNames | llm-utils.ts:277→282 | utility | 技能名短词翻译 | **title** | 是(→chat) | 否 |
| summarizeActivity | llm-utils.ts:316→338 | utility_large | 执行摘要(~50字) | **summarize** | 是(→chat) | 否 |
| summarizeActivityQuick | llm-utils.ts:405→414 | utility | 快速摘要(~30字) | **summarize** | 是(→chat) | 否 |
| generateDescription | llm-utils.ts:574→576 | utility | agent 公开简介 | **summarize** | 是(→chat) | 否 |
| rc-summary Tier1 | rc-summary.ts:58 | utility | /rc 会话摘要 | **summarize** | 是(跨级→large→chat) | 否 |
| rc-summary Tier2 | rc-summary.ts:73 | utility_large | /rc 会话摘要 fallback | **summarize** | 是 | 否 |
| rc-summary Tier3 | rc-summary.ts:88 | chat | /rc 会话摘要 fallback | **summarize** | 是 | 否 |
| MemoryTicker | agent.ts:422-434 | _memoryModel(=utility_large) | 会话记忆提炼→FactStore | **memory** | 是(→chat) | 是(FactStore) |
| channel memory summary | channel-router.ts:1047-1048 | utility | 频道记忆压缩→FactStore | **memory** | 是 | 是(FactStore) |
| 日记 fallback | engine.ts:3300 | agent.memoryModel | 日记生成模型 | **memory** | 是 | 是 |
| VisionBridge | (vision 相关) | vision | 辅助视觉理解 | **vision** | image-capable chat | 否 |
| Approval reviewer | engine.ts:385-391→approval-gateway.ts:637 | utility(role:"utility") | 意图授权审批 | **approval** | **禁止** | 否 |
| install_skill safetyReview | install-skill.ts:83→101 | utility | Skill 安全审查 | **guard** | **禁止** | 否 |
| hub utility:call-text | server/index.ts:761-789 | utility | 插件/工具调用 | **summarize** | 是 | 否 |
| hub model:sample-text | server/index.ts:790-827 | utility | 插件/工具采样 | **summarize** | 是 | 否 |

### 生产代码 legacy 符号分布（396 命中，dist-* 构建产物忽略）

| 文件 | 命中数 | 主要符号 |
|------|--------|---------|
| core/execution-router.ts | 34 | resolveUtilityConfig/Fresh, utility_large, ROLE_TO_PREF_KEY |
| core/config-coordinator.ts | 26 | SHARED_MODEL_KEYS, getUtilityApi/setUtilityApi, utility_api_* |
| core/agent.ts | 23 | _utilityModel, _memoryModel, setUtilityModel/setMemoryModel |
| core/engine.ts | 13 | resolveUtilityConfig/Fresh 委托, approval wiring |
| core/llm-utils.ts | 12 | callTextConfigFromUtilityConfig, utility role |
| core/model-execution-config.ts | 7 | callTextConfigFromUtilityConfig |
| core/model-manager.ts | 4 | resolveUtilityConfig 委托 |
| core/agent-manager.ts | 6 | resolveUtilityConfigFresh (generateAgentId/desc) |
| lib/approval-gateway.ts | 9 | createModelApprovalReviewer, role:"utility", LEGACY_REVIEWER_IDS |
| lib/tools/install-skill.ts | 11 | resolveSafetyReviewUtilityConfig, callTextConfigFromUtilityConfig |
| lib/memory/config-loader.ts | 3 | utility_api block |
| hub/channel-router.ts | 3 | callTextConfigFromUtilityConfig |
| server/index.ts | 6 | utility:call-text handler, model:sample-text handler |
| server/routes/*.ts | 8 | utility_api 暴露/写入 |
| desktop/.../OtherModelsSection.tsx | 3 | utility/utility_large UI |
| desktop/.../onboarding-actions.ts | 1 | selectedUtility/selectedUtilityLarge |
| desktop/locales/*.json (×5) | 3 each | utilityModel/utilityLargeModel i18n keys |

## 4. 最终 Slot → 调用点映射

| Slot | Preference Key | Fallback | 调用点 |
|------|---------------|----------|--------|
| title | title_model | chat | summarizeTitle, generateAgentId, translateSkillNames |
| summarize | summarize_model | chat | summarizeActivity, summarizeActivityQuick, generateDescription, rc-summary, hub utility:call-text/model:sample-text |
| memory | memory_model | chat | MemoryTicker, channel memory summary, 日记 fallback |
| vision | vision_model | image-capable chat | VisionBridge |
| approval | approval_model | **none** | Approval Gateway reviewer |
| guard | guard_model | **none** | install_skill safetyReview |

## 5. 修改文件列表

### 新增文件
- `core/auxiliary-slots.ts` — 6 Slot canonical descriptor（single source of truth）
- `core/auxiliary-model-resolver.ts` — 统一 AuxiliaryModelResolver
- `tests/auxiliary-slot-resolver.test.ts` — Slot contract tests (Case A-I, 18 tests)

### 核心架构修改
- `core/execution-router.ts` — 删除 resolveUtilityConfig/Fresh、_resolveUtilityModels、utility_api override 逻辑；保留通用 resolve() (chat/embed)
- `core/model-execution-config.ts` — 删除 callTextConfigFromUtilityConfig；保留 composeResolvedModelExecution + callTextConfigFromResolvedModel
- `core/config-coordinator.ts` — 删除 getUtilityApi/setUtilityApi/normalizeUtilityApiPreferences/resolveUtilityConfig*/_utilityResolverArgs；SHARED_MODEL_KEYS→AUXILIARY_MODEL_PREF_KEYS（6 Slot）
- `core/model-manager.ts` — 删除 resolveUtilityConfig/Fresh 委托
- `core/engine.ts` — 新增 _auxResolver + resolveAuxiliaryModel/Fresh/Execution + _auxResolveContext + _withAuxiliaryUsageAttribution；删除 resolveUtilityConfig*/getUtilityApi/setUtilityApi/_resolveUtilityOptions/_withUtilityUsageAttribution；approval wiring 改用 approval slot
- `core/agent.ts` — 删除 _utilityModel/_memoryModel 字段、setUtilityModel/setMemoryModel、utilityModel/memoryModel getter；MemoryTicker 改用 resolveAuxiliaryExecution("memory")；resolvedMemoryModel/memoryModelUnavailableReason 改用 memory slot
- `core/agent-manager.ts` — resolveUtilityConfigFresh dep → resolveAuxiliaryModelFresh；_generateAgentId/_refreshDescription 改用 title/summarize slot
- `core/llm-utils.ts` — 6 个函数（summarizeTitle/translateSkillNames/summarizeActivity/summarizeActivityQuick/generateAgentId/generateDescription）从 utilConfig 改为 resolved config
- `core/slash-commands/rc-summary.ts` — 三级 fallback 改为 summarize slot + chat fallback
- `hub/channel-router.ts` — _memorySummarize 改用 resolveAuxiliaryModelFresh("memory")
- `lib/approval-gateway.ts` — createModelApprovalReviewer 改收 resolveApprovalModel；删除 callTextConfigFromUtilityConfig import、LEGACY_REVIEWER_IDS、smallToolModelReviewer 参数
- `lib/tools/install-skill.ts` — safetyReview 改收 resolveGuardModel；删除 resolveSafetyReviewUtilityConfig
- `lib/memory/config-loader.ts` — 删除 utility_api block
- `server/index.ts` — hub utility:call-text/model:sample-text 改用 resolveAuxiliaryModelFresh("summarize")
- `server/routes/preferences.ts` — 删除 utility_api 读写
- `server/routes/agents.ts` — 删除 utility_api 引用
- `server/routes/config.ts` — 删除 utility_api 引用
- `server/routes/settings-snapshot.ts` — 删除 utility_api 暴露

### Desktop UI
- `desktop/src/react/settings/tabs/providers/OtherModelsSection.tsx` — 2+1 行（utility/utility_large/vision）→ 6 Slot
- `desktop/src/react/settings/tabs/AgentTab.tsx` — hasUtilityModel → hasMemoryModel
- `desktop/src/react/onboarding/steps/ModelStep.tsx` — 删除 selectedUtility/selectedUtilityLarge 强制要求
- `desktop/src/react/onboarding/onboarding-actions.ts` — 删除 selectedUtility/selectedUtilityLarge

### i18n (5 locales)
- `desktop/src/locales/{zh,en,ja,ko,zh-TW}.json` — 删除 utilityModel/utilityLargeModel/utilityApi；新增 6 Slot × (label/hint/fallback)

## 6. 新增测试列表

- `tests/auxiliary-slot-resolver.test.ts` (18 tests)
  - Case A: 六 Slot 使用六个不同 sentinel
  - Case B: 修改一个 Slot 不影响其它 Slot
  - Case C: 未配置普通 Slot → fallback chat
  - Case D: 配置错误不得 fallback
  - Case E: Approval 未配置 → null（不 fallback chat）
  - Case F: Approval 配错 → throws（不 fallback chat）
  - Case G: Guard 未配置 → null（不 fallback）
  - Case H: Vision capability 强制
  - Case I: Fresh credential 独立解析
  - resolveAuxiliaryExecution 返回完整对象

## 7. typecheck 结果

```
tsc --noEmit && tsc --noEmit -p tsconfig.node.json && tsc --noEmit -p tsconfig.test.json
→ 0 error
```

## 8. targeted tests 结果

```
tests/auxiliary-slot-resolver.test.ts          → 18 passed
tests/approval-gateway.test.ts                 → 30 passed
tests/engine-build-tools.test.ts               → 22 passed
tests/session-permission-wrapper.test.ts       → 51 passed
tests/model-execution-config.test.ts           → passed
tests/install-skill-safety-review.test.ts      → passed
tests/model-no-fallback.test.ts                → 23 passed
tests/fresh-credential-routing.test.ts         → passed
tests/fresh-network-boundaries.test.ts         → passed
tests/agent-description.test.ts                → passed
tests/llm-utils-error-cause.test.ts            → passed
tests/model-sync-routes.test.ts                → 54 passed
tests/secret-custody-routes.test.ts            → passed
tests/update-config-non-focus.test.ts          → passed
tests/rc-summary.test.ts                       → passed
tests/channel-router-memory-master.test.ts     → passed
tests/channel-router-personality.test.ts       → passed
```

## 9. full suite baseline vs final

```
baseline: 162 failed | 10711 passed | 7 skipped (10880)
final:    162 failed | 10724 passed | 7 skipped (10893)
```

**fail = 162（= baseline，0 新增失败）；skip = 7（= baseline，0 新增 skip）；pass = 10724（+13，新增 contract tests）。**

## 10. legacy scan 结果

```bash
rg -n 'utility_large|resolveUtilityConfig|callTextConfigFromUtilityConfig|large_api_key|large_base_url|large_headers|small_tool_model|large_tool_model|LEGACY_REVIEWER_IDS|setUtilityModel|setMemoryModel|_utilityModel|_memoryModel|requireUtilityLarge' core lib hub server desktop packages shared -g '*.{ts,tsx,cjs,mjs,js,jsx}'
```

**生产代码 0 命中**（仅注释中引用旧术语作为迁移说明）。
`role: "utility"` / `role: "utility_large"` 0 命中。

## 11. Approval 反向验证

- **Case E (approval 未配置)**：`approval=null, chat=valid` → resolver 返回 null → gateway fail-closed → ask_user。freshCallLog 为空（chat model 0 次调用）。✓
- **Case F (approval 配错)**：`approval=invalid/model` → resolver throws → gateway fail-closed → ask_user。freshCallLog 为空（chat model 0 次调用）。✓
- Engine approval wiring 使用 `resolveApprovalModel`（approval slot），不再接收 `role: "utility"`。✓

## 12. Guard 反向验证

- **Case G (guard 未配置)**：`guard=null, chat=valid` → resolver 返回 null → safetyReview 返回 `{safe: false}` → 进入风险确认路径。freshCallLog 为空（chat/approval 0 次调用）。✓
- install-skill safetyReview 使用 `resolveGuardModel`（guard slot），guard 不可用时 fail-closed（不宣称 safe）。✓

## 13. Slot isolation 反向验证

- **Case B**：修改 approval A→B 后，title/summarize/memory/vision/guard 解析结果全部保持不变。✓
- **Case A**：6 个 Slot 配置 6 个不同 sentinel，每个入口只收到自己的 sentinel。✓

## 14. 尚存风险

- 真实 Provider 网络调用未做人工验证（仅 mock 测试通过）——作遗留风险。
- better-sqlite3 原生模块版本不匹配（NODE_MODULE_VERSION 137 vs 127）导致 162 个基线测试失败，与本重构无关，需要 `npm rebuild better-sqlite3` 修复环境。
- i18n locale parity (zh-TW/ja/ko 缺部分 key) 为基线遗留问题，非本次引入。
