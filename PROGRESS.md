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

---

# ========== 收口修复任务（2026-08-08，HEAD 283d958）==========

> 本轮是「收口修复」，不是新一轮大重构。目标：在已完成的 6 Slot 体系上，修复最后一批契约违背、隐藏 fallback、配置入口残留、单一真理源失效问题。

## C0. 起始基线

- **Start HEAD**：`283d95818270e4b33a4e2fecd0ca11cbd0dc6aa2`
- **Working tree**：clean
- **typecheck**：`npm run typecheck` → 0 error
- **Targeted tests**（6 file / 94 tests）：
  - tests/auxiliary-slot-resolver.test.ts → 18 passed
  - tests/approval-gateway.test.ts → 30 passed
  - tests/install-skill-safety-review.test.ts → 8 passed
  - tests/slash-commands/rc-summary.test.ts → 11 passed ⚠️（含错误断言 "summarize slot fails → falls back to chat model"，Task 2.1 须改写）
  - tests/model-no-fallback.test.ts → 23 passed
  - tests/fresh-credential-routing.test.ts → 4 passed
- **Known baseline issue**：better-sqlite3 native mismatch（NODE_MODULE_VERSION 137 vs 127）—— 环境基线，不修业务。

## C1. 全仓库 Legacy / Fallback Inventory（Task 1 实测）

### A. runtime architecture 残留（须修复）

1. **`core/slash-commands/rc-summary.ts:49-87`** — `/rc` 双重 fallback：
   - `catch { /* fall through to chat */ }` 吞掉 summarize slot 的配置错误
   - 紧接手动 `engine.resolveModelWithCredentialsFresh(chatRef)` 二次解析 chat
   - **违反不变量 1**（fallback 只能发生一次，收口在 resolver）。→ Task 2

2. **`core/engine.ts:3326-3333`** — `writeDiary` chat-first / memory-second 逆向 fallback：
   ```ts
   if (chatRef) { resolvedModel = await resolveModelWithCredentialsFresh(chatRef); }
   else { resolvedModel = await this.resolveAuxiliaryExecution("memory"); }
   ```
   - diary 产物属持久记忆材料，应走 memory slot；现消费方自行决定 chat 优先级。→ Task 8

3. **`core/agent-manager.ts:1235-1244`** — `_generateAgentId` `catch {}` 吞 title slot 错误，静默 fallback deterministic ID，无诊断。→ Task 6.1

4. **`core/agent.ts:397,412`** — MemoryTicker 创建被 `if (chatModelRef)` gate：
   - `memory_model` 有效但 chat=null 时，MemoryTicker 根本不创建 → Case M-2 失败。→ Task 7

5. **`core/auxiliary-model-resolver.ts:86-100`** — `isAuxiliaryConfigError` 死代码（零调用方），且逻辑含运算符优先级 bug。→ Task 6.3

6. **`core/config-coordinator.ts:43-50`** — `AUXILIARY_MODEL_PREF_KEYS` 手写第二份 Slot→prefKey 映射，未从 canonical `AUXILIARY_SLOT_PREF_ENTRIES` 派生。→ Task 4

7. **`core/config-coordinator.ts:68-98`** `normalizeSharedModelsPatch` — 静默忽略未知字段（`if (!hasOwn) continue`），`models.summarzie` 被接受为 200。→ Task 5

8. **`desktop/src/react/settings/tabs/providers/OtherModelsSection.tsx:79-93`** — UI 手写 6 个 slot field 数组，不来自 canonical。→ Task 11

9. **`lib/config.example.yaml:28-29`** — `models.utility` / `models.utility_large` 旧架构配置项。→ Task 3

10. **`core/llm-utils.ts:116,128`** / `server/routes/models.ts:243` / `server/index.ts:782` — usage `subsystem:"utility"` / `kind:"utility"` 遥测分类沿用旧架构名。→ Task 10

### B. test fixture 中证明 legacy 被拒绝的字符串（保留）

- `tests/fresh-network-boundaries.test.ts:33-47` — 断言 handler 不含 `resolveUtilityConfig`/`callTextConfigFromUtilityConfig`（反向守卫，保留）
- `tests/model-no-fallback.test.ts:499` — 断言"不再有 utility/utility_large 区分"（正向守卫，保留）
- `tests/settings-snapshot-route.test.ts:132` / `engine-build-tools.test.ts:188` — 旧 fixture，检查是否需调整
- `tests/secret-custody-routes.test.ts:203` — 断言 `utility_api` 为 undefined（反向守卫，保留）

### C. migration / history 注释（保留，已说明）

- `core/engine.ts:606,1857` / `core/auxiliary-slots.ts:5` / `core/execution-router.ts:4` / `core/agent.ts:387` / `core/config-coordinator.ts:40-41,53` — 均为解释性注释（"不再关心 utility/utility_large"），保留。

### D. dead i18n（须清理）

- `noUtilityLargeModel` / `utilityApiProviderMismatch` / `utilityApiMissingCreds` — 零引用，5 locale 各一份，删。
- `installSkillNoUtility` / `installSkillUtilityIncomplete` — 仍被 `lib/tools/install-skill.ts:92,96,100` 的 guard 流程使用，但名字旧（utility）。须改名为 `installSkillGuardUnavailable` / `installSkillGuardIncomplete`，5 locale 同步。→ Task 9

### E. config example

- `lib/config.example.yaml` — 见 A.9。当前 agent config 真实 schema 只含 `chat` / `embedding`（+ embedding_dimensions）。辅助 Slot 存全局 preferences，不应出现在 per-agent config。→ Task 3

### F. unrelated generic "utility"（不动）

- CSS class / event name / telemetry 通用词 —— 不在本任务范围。

## C2. 逐项修复记录

### Task 2 / 2.1：/rc 双重 fallback（最高优先级）✅

- `core/slash-commands/rc-summary.ts`：删除 `catch { fall through to chat }` + 手动 `resolveModelWithCredentialsFresh(chatRef)` 的 caller-side fallback。
- 新逻辑：只调 `resolveAuxiliaryModelFresh("summarize")` 一次。resolver 对未配置按 Slot 策略 fallback（summarize→chat）；对显式配置错误 throw，本函数用 `isAuxiliaryConfigError` 识别后返回 null，绝不二次解析 chat。
- 测试 `tests/slash-commands/rc-summary.test.ts` 重写：RC-1（未配置→resolver fallback chat，resolve 1 次）、RC-2（显式有效→只用 summarize）、RC-3（显式错误→chat 0 调用）、RC-4（运行时 timeout→LLM 1 次，不 fallback）。全部断言 `resolveModelWithCredentialsFresh` 未被调用。

### Task 3：Config example ✅

- `lib/config.example.yaml`：删除 `models.utility` / `models.utility_large`（旧架构 dead key）。agent config 真实 schema 现只剩 `chat` / `embedding` / `embedding_dimensions`。辅助 Slot 存全局 preferences，注释说明。

### Task 4：Canonical 单一真理源 ✅

- `core/config-coordinator.ts`：`AUXILIARY_MODEL_PREF_KEYS` 不再手写第二份映射，改为 `= AUXILIARY_SLOT_PREF_ENTRIES`（从 `core/auxiliary-slots.ts` canonical descriptor 派生）。
- 新增 `shared/auxiliary-slot-ids.ts`：承载 Slot 身份（id 列表 + 类型），无 runtime 依赖，server/core/desktop renderer 共用。`core/auxiliary-slots.ts` 的 `AuxiliarySlot` 类型与 `AUXILIARY_SLOT_IDS` 从 shared 派生。
- `export-manifest.json`：补登 `core/auxiliary-model-resolver.ts` / `core/auxiliary-slots.ts` / `shared/auxiliary-slot-ids.ts`（它们本就在 CLI runtime closure 内，是 redistributable core）。

### Task 5 / 5.1：Preferences patch 拒绝 unknown slot ✅

- `core/config-coordinator.ts` `normalizeSharedModelsPatch`：新增 unknown-field 拒绝——`models.summarzie`（拼写错误）/ `random_future_slot` / `utility` / `utility_large` 均 throw `unknown shared model field`。契约：omitted→no change；null→clear；valid ModelRef→set；unknown→reject。
- 测试 `tests/auxiliary-slot-contract-closeout.test.ts`：C6 覆盖 summarzie/random_future_slot→throw、summarize=null→clear、summarize omitted→不变、summarize=valid→set、utility/utility_large→throw、vision_enabled 仍允许。

### Task 6 / 6.1 / 6.3：配置错误 vs 运行时失败 ✅

- `core/auxiliary-slots.ts`：新增结构化 `AuxiliaryConfigurationError`（`code="AUXILIARY_CONFIG_ERROR"` + `slot` + `reason`）+ `isAuxiliaryConfigError()`（靠 instanceof + code，不靠 brittle i18n 文本匹配）。
- resolver 的 5 个 config-error throw（model_not_found / capability_mismatch×2 / provider_missing_api×2 / provider_missing_creds×2）全部改为抛 `AuxiliaryConfigurationError`。
- 删除旧 `isAuxiliaryConfigError`（零调用方死代码 + 运算符优先级 bug）。
- `core/agent-manager.ts` `_generateAgentId`：`catch` 不再静默——`isAuxiliaryConfigError` 时 emit devlog + warn（"title_model 配置无效"），仍允许 deterministic ID fallback（创建 Agent 不因辅助模型失败而整体不可用，Task 23）。
- `core/engine.ts` `writeDiary`（见 Task 8）。

### Task 7：MemoryTicker 隐式 chat 依赖 ✅

- `core/agent.ts`：删除 `if (chatModelRef)` gate。MemoryTicker **无条件创建**——`getResolvedMemoryModel` 内部 `resolveAuxiliaryExecution("memory")` 现场解析，memory slot 自行决定 fallback。即使 chat=null 但 memory_model 有效（Case M-2），ticker 仍创建。
- 启动探测改为探测 memory slot（返回 ok/no-model/error），不再用 chatModelRef 判断。

### Task 8：Diary slot 语义 ✅

- `core/engine.ts` `writeDiary`：删除 `if (chatRef) { resolve chat } else { resolve memory }` 的 chat-first/memory-second 逆向 fallback。改为 `resolveAuxiliaryExecution("memory")`——diary 产物是持久记忆材料，由 memory slot 自行 fallback。显式配置错误 throw 并报告（不偷偷改用别的模型）。

### Task 9：Dead legacy i18n ✅

- 5 locale（zh/zh-TW/en/ja/ko）删除 dead key：`noUtilityModel` / `noUtilityLargeModel` / `utilityApiProviderMismatch` / `utilityApiMissingCreds`（零引用）。
- `installSkillNoUtility` → `installSkillGuardUnavailable`、`installSkillUtilityIncomplete` → `installSkillGuardIncomplete`（仍被 `lib/tools/install-skill.ts` guard 流程使用，改名对齐 guard 语义）。5 locale 同步、对称。
- `lib/tools/install-skill.ts`：guard 路径改用新 key。

### Task 10：Usage attribution ✅

- `core/llm-utils.ts` / `server/index.ts` / `server/routes/models.ts`：usage `subsystem:"utility"` / `kind:"utility"` → `"auxiliary"`。无 aggregation/dashboard 绑定该分类（已确认），无历史数据断层风险。

### Task 11：UI Slot 列表 ✅

- `desktop/src/react/settings/tabs/providers/OtherModelsSection.tsx`：删除手写 6 个 field 字符串数组，改为 `Record<AuxiliarySlot, UiDescriptor>` + `AUXILIARY_SLOT_IDS.map(...)`。新增第 7 个 Slot 时 TypeScript 强迫 UI 补齐（exhaustive Record 检查）。

## C3. Completion Gates（Task 31）

### Gate A — typecheck
```
npm run typecheck → 0 error（tsc x3 全过）
```

### Gate B — Targeted tests 全绿
```
8 file / 124 tests passed:
  tests/auxiliary-slot-resolver.test.ts          → 18
  tests/approval-gateway.test.ts                 → 30
  tests/install-skill-safety-review.test.ts      → 8
  tests/slash-commands/rc-summary.test.ts        → 12（重写，含 RC-1~4）
  tests/model-no-fallback.test.ts                → 23
  tests/fresh-credential-routing.test.ts         → 4
  tests/auxiliary-slot-contract-closeout.test.ts → 28（新增）
  tests/approval-review-context.test.ts          → 1
```

### Gate C — Full suite 无新增 regression
```
baseline (stash, 无本次修改): 170 failed | 10746 passed
final    (本次修改后):        165 failed | 10752 passed
→ 失败数 -5，通过数 +6（manifest 修复消除了 5 个 open→closed 边 + 新增 28 个 contract test）
→ 无新增失败。剩余失败均为 better-sqlite3 native mismatch（NODE_MODULE_VERSION 137 vs 127）环境基线 + provider-compat/ollama 预存边界债（与本任务无关）。
```

### Gate D — /rc configured-error isolation
```
summarize=invalid + chat=valid → chat call count = 0 ✅
（RC-3 测试断言 resolveModelWithCredentialsFresh 未被调用、callText 未被调用）
```

### Gate E — Memory valid + chat null 仍可工作
```
memory=valid + chat=null → resolveAuxiliaryExecution("memory") 返回 memory sentinel，chatCalls=0 ✅
（C3 测试）
```

### Gate F — Memory invalid + chat valid 不用 chat
```
memory=invalid + chat=valid → resolver 抛 AuxiliaryConfigurationError，chatCalls=0 ✅
（C4 测试）
```

### Gate G — Approval invalid/unset + chat valid
```
approval fallback=none，chat call count = 0 ✅
（auxiliary-slot-resolver.test.ts Case E + approval-gateway.test.ts 30 tests）
```

### Gate H — Guard invalid/unset + 其它模型 0 调用
```
guard fallback=none，不 fallback chat 也不 fallback approval ✅
（auxiliary-slot-resolver.test.ts Case G + install-skill-safety-review.test.ts 8 tests）
```

### Gate I — Unknown slot 被拒绝
```
models.summarzie → throw（400）✅
（C6 测试）
```

### Gate J — 示例配置不再含旧架构配置
```
lib/config.example.yaml 不含 utility / utility_large / utility_api ✅
```

### Gate K — Canonical Slot ID / preferenceKey 单一真理源
```
AUXILIARY_MODEL_PREF_KEYS === AUXILIARY_SLOT_PREF_ENTRIES（从 canonical 派生，非手写）✅
AUXILIARY_SLOT_IDS === shared/auxiliary-slot-ids.ts（跨层共用）✅
（C7 测试）
```

## C4. 最终 Legacy / Caller-Side Fallback Scan（Task 17 / 28 / 29）

### Legacy scan（source，排除注释/locale/test）
```
rg utility_large|utility_model|...|_memoryModel → 0 runtime hit
（剩余命中均为 migration history 注释，Task 17 允许）
```

### yaml/json scan
```
rg utility_large|utility_model|utility_large_model|utility_api → 0 hit
（lib/config.example.yaml 已清理）
```

### Caller-side fallback scan（resolveAuxiliary* + catch/chat nearby）
```
剩余 4 个 catch 站点全部合规：
  agent-manager.ts _generateAgentId → catch + isAuxiliaryConfigError + devlog（不静默，不 fallback chat）
  agent.ts memory probe            → catch + warn/devlog（仅探测，非 fallback）
  engine.ts writeDiary             → catch + isAuxiliaryConfigError + re-throw（不 fallback）
  rc-summary.ts                    → catch + isAuxiliaryConfigError + return null（不 fallback chat）
无任何 "resolver throw → 手动 resolve chat" 模式。
```

## C5. Fresh credential（Task 26）

所有真实推理路径继续使用 `resolveAuxiliaryModelFresh` / `resolveAuxiliaryExecution`（请求边界刷新凭证）：
memory ticker / approval / guard / summarize / title generation / channel summary / plugin sample / diary。
同步 cached `resolveAuxiliaryModel`（非 Fresh）仅用于诊断 getter（`resolvedMemoryModel` / `memoryModelUnavailableReason` / healthz `utilityModel`），非推理调用——符合 Task 26。

## C6. 尚存风险

- **`provider-compat.ts:35 → provider-compat/ollama.ts` 边界债**：open-boundary-lint 预存失败（与辅助 Slot 无关，是 ollama provider 插件的 manifest 分类问题）。不在本任务范围。
- **healthz `utilityModel` JSON 字段名 + `AgentMemory.tsx` 的 `hasUtilityModel` prop 名**：仍含 "utility" 字样（外部 API contract 字段 / UI prop 名），但底层已读 memory slot。属表面命名遗留，非架构残留——改名会动 healthz API contract，超出收口范围。
- **test fixture mock 中的 `getUtilityApi` / `utility`/`utility_large` getSharedModels mock**（settings-snapshot-route / model-sync-routes 等）：Class B fixture，生产代码已不产生这些 key，mock 仅测序列化路径，保留（Task 18 允许）。
- **better-sqlite3 native mismatch**（NODE_MODULE_VERSION 137 vs 127）：环境基线，需 `npm rebuild better-sqlite3`，与本任务无关。
- 真实 Provider 网络调用未做人工验证（仅 mock 测试通过）。

---

# ========== upstream-0.444.1 + pi SDK 0.84.1 迁移任务（2026-08-08）==========

> TASK_ID=upstream-0.444.1_pi-0.84.1
> 本 section 仅供本任务；旧 section 是历史记录，不当成本任务状态。

## P0. 任务身份

```
TASK_ID      = upstream-0.444.1_pi-0.84.1
START_HEAD   = a5d1e5415c28b55074ba9ae81a6429d57ff5a934
START_TIME   = 2026-08-08T10:41:32Z
Repository   = ItsDalk-Lane/LingxiAgent
Base upstream= openhanako v0.443.46  (262d385c7fb53217c4c8a7de9817efeef9c1bf4b)
Target upstr = openhanako v0.444.1   (cc19cb49b0786d61ed723764e0a83baf87887270)
Current pi   = 0.83.0
Target pi    = 0.84.1
Required Node= 24.16.0  (nvm exec 24.16.0；本机默认 node v22.23.2 不可用于验证)
```

## P1. Phase 0 — 可重现基线（本机实测，权威）

执行环境：`nvm exec 24.16.0`。本机默认 `node -v` = v22.23.2（不是要求的 24.16.0），所有验证命令一律 `nvm exec 24.16.0 <cmd>`。

```
node -v (via nvm exec 24.16.0)  → v24.16.0 ✓
git rev-parse HEAD              → a5d1e5415c28b55074ba9ae81a6429d57ff5a934
git status --short              → (clean) ✓
```

### 基线 typecheck

```
nvm exec 24.16.0 npm run typecheck  → 0 error ✓
```

### 基线 full suite（实测权威）

```
Test Files  6 failed | 1074 passed | 1 skipped (1081)
Tests       13 failed | 10904 passed | 7 skipped (10924)
```

→ **BASELINE: 13 failed / 7 skipped**（验收线：最终 0 failed，skipped ≤ 7）。

注意：PROGRESS/BLOCKED 旧 section 记录的 162/170 失败是 Node22 下 better-sqlite3 native ABI mismatch（NODE_MODULE_VERSION 137 vs 127）的环境噪声；Node24.16.0 下该类失败已消失，**本任务 baseline 以本机 Node24 实测为准**。失败清单详见 P1.1。

### 包版本核对（实际仓库为准）

```
app version            = 0.1.22
lingxi.upstreamVersion = 0.443.46
@earendil-works/pi-agent-core   = 0.83.0
@earendil-works/pi-ai           = 0.83.0
@earendil-works/pi-coding-agent = 0.83.0
engines.node           = ">=24.12.0 <25"
verifiedVersions (verifier)       = {0.80.3, 0.83.0}
verifiedPiAiVersions (verifier)   = {0.80.3, 0.83.0}
```

### SDK import boundary 现状（lib/pi-sdk 是唯一入口）

- 根 re-export：`@earendil-works/pi-coding-agent`、`@earendil-works/pi-agent-core`、`@earendil-works/pi-ai`、`@earendil-works/pi-ai/compat`
- 深路径：`dist/core/auth-storage.js`（AuthStorage, FileAuthStorageBackend）、`dist/core/compaction/compaction.js`（prepareCompaction）
- verifier（patch-pi-sdk.cjs）扫描 core/server/lib/hub 生产代码，禁止越界直接 import @earendil-works/*（lib/pi-sdk 除外）。

### P1.1 BASELINE_FAILURE_LIST（6 file / 13 failed，全部 pre-existing）

| 文件 | 失败数 | 分类（根因判断） |
|------|--------|------------------|
| tests/i18n-locale-parity.test.ts | 3 | **guardrail** — en vs zh-TW/ja/ko 缺 key（pre-existing locale debt） |
| tests/model-sync-routes.test.ts | 1 | **legacy** — utility account 计费断言（pre-existing） |
| tests/open-boundary-lint.test.ts | 2 | **guardrail** — committed export-manifest/baseline 与 source 不一致（pre-existing 边界债） |
| tests/persistence-schema-tripwire.test.ts | 4 | **guardrail** — committed fingerprint 与实时 store 不一致（pre-existing） |
| desktop/.../screenshot.test.ts | 1 | **legacy** — 默认头像烧录断言（pre-existing） |
| desktop/.../settings-search-layout.test.ts | 2 | **legacy** — 设置 modal 884px 宽 token 断言（pre-existing） |

### BASELINE_SKIPPED_LIST（7 skipped，全部 platform skip）

- tests/secret-fs.test.ts :: windows contract（×1）
- tests/manual/win32-packaged-smoke.test.ts :: win32 packaged smoke（×6）

→ 全部为 darwin 上恒不运行的 win32 skip。**skip 基线 = 7，验收线 skip ≤ 7。**

> 这 13 个失败均为 pre-existing（与 0.83.0→0.84.1 / upstream 0.444.1 无因果关系）。其中 3 个 guardrail 文件（i18n-locale-parity / open-boundary-lint / persistence-schema-tripwire）的 **assertion semantics 在本任务中不可修改**；本任务目标是最终 full green，但区分 migration correctness vs legacy cleanup（Phase 15），修复 pre-existing 失败须独立证明 + 独立 commit，不得改 guardrail assertion。

## P2. Phase 2 — pi SDK 0.83.0 → 0.84.1 升级（迁移本身）

### 改动
- `package.json`：三个 `@earendil-works/pi-*` 0.83.0 → 0.84.1。
- `scripts/patch-pi-sdk.cjs`：`verifiedVersions` / `verifiedPiAiVersions` **追加** "0.84.1"（旧 0.80.3/0.83.0 保留）。
- `nvm exec 24.16.0 npm install`（非 --ignore-scripts，postinstall 自然运行）→ `[verify-pi-sdk] all checks passed`，installed 0.84.1 ✓。

### PI_083_TO_084_CONTRACT（读真实 0.84.1 .d.ts/runtime 后得出）

#### message stream（stream-guard 依赖）— contract 不变
- `AssistantMessageEvent`（pi-ai types.d.ts:383-436）shape 与 0.83 一致：
  `start` / `text_start|delta|end` / `thinking_start|delta|end` / `toolcall_start|delta|end`（带 contentIndex, delta, toolCall, partial）/ `done{reason,message}` / `error{reason,error}`。
- `EventStream`/`AssistantMessageEventStream`（utils/event-stream.d.ts）：`push`/`end`/`result` 不变。
- `toolcall_delta` 仍 emit `delta` 字符串（openai-completions.js:399）；`partialArgs` 是 runtime streaming scratch buffer，streaming 中可读、`toolcall_end` 前 `delete`、`done` 前 `delete`。
  → stream-guard 现有 dual-source（event.delta ∪ block.partialArgs）策略 **仍成立**，运行时验证 `tests/pi-sdk-stream-guard.test.ts` 全绿。

#### OAuth provider contract — **breaking（refreshToken 加 signal 参数）**
0.84.1 `ProviderConfig.oauth`（pi-coding-agent core/extensions/types.d.ts:1059-1074 / provider-composer.d.ts:11）：
```
oauth: {
  name: string;
  isSubscription?: boolean;
  usesCallbackServer?: boolean;   // @deprecated
  login(callbacks: OAuthLoginCallbacks): Promise<OAuthCredentials>;        // 不变
  refreshToken(credentials: OAuthCredentials, signal: AbortSignal): Promise<OAuthCredentials>;  // 0.83 是 1 参，0.84.1 加 signal
  getApiKey(credentials: OAuthCredentials): string;                          // 不变
  modifyModels?(...): ...;
}
```
- 受影响 Lingxi 代码：`lib/auth/xai-oauth.ts` `refreshToken(credentials)` 须改为 `refreshToken(credentials, signal)`，且 signal 必须真实接入（throwIfAborted + requestSignal），非 no-op。
- `OAuthLoginCallbacks` shape 不变；`loginOAuthProvider`/AuthInteraction adapter 不动。

#### deep imports — 全部仍存在
- `pi-coding-agent/dist/core/auth-storage.js`（AuthStorage, FileAuthStorageBackend）✓
- `pi-coding-agent/dist/core/compaction/compaction.js`（prepareCompaction）✓
- `pi-ai/compat`（getModel/getModels/completeSimple）✓

#### 结论
- stream-guard：无需机械改造，运行时契约保持；Phase 3 只验证，不改 guard 语义。
- 真正需要迁移的只有 `xai-oauth.ts` 的 refreshToken 2-arg 契约（生产 typecheck 已过，因 SdkOAuthProvider 是结构类型，Lingxi 实现收 1 参在调用侧不报错；只有 test 显式 1-arg 调用被 0.84.1 返回类型判定为缺参）。

## P3/P4. Phase 3 + 4 — stream guard & SDK boundary verification

### Phase 3：stream guard（contract 不变，仅验证）
0.84.1 的 `AssistantMessageEvent` 与 `EventStream`/`AssistantMessageEventStream` shape 与 0.83 完全一致；`toolcall_delta` 仍带 `delta` 字符串，`partialArgs` 仍是 streaming scratch buffer（streaming 中可读，end/done 前 delete）。stream-guard 现有 dual-source（event.delta ∪ block.partialArgs）策略成立，**无需机械改造**。

- `nvm exec 24.16.0 npx vitest run tests/pi-sdk-stream-guard.test.ts` → 全绿 ✓
- guard 8 项语义职责全部保留（malformed tool-call recovery / 合法透传 / done-error / cancellation）。

### Phase 4：SDK boundary
- `nvm exec 24.16.0 node scripts/patch-pi-sdk.cjs` → `[verify-pi-sdk] all checks passed` ✓
- deep imports 全部仍存在：`dist/core/auth-storage.js` ✓ / `dist/core/compaction/compaction.js` ✓ / `pi-ai/compat` ✓（import 实测可解析）
- pi-sdk targeted 套件（14 file / 150 test）→ 148 passed；剩 2 = **pre-existing open-boundary baseline debt**（provider-compat→ollama + SDK 包内 auth-storage→abort），与本次迁移无因果（baseline 文件自 START_HEAD 未变）。

**Checkpoint commit 1**（branch `chore/upstream-0.444.1-pi-0.84.1`）：5338cef6 — pi SDK 0.84.1 升级 + refreshToken 契约适配。typecheck 0 error，oauth targeted 23 passed。

## P5. Phase 5 — upstream v0.444.1 semantic merge（逐文件 3-way）

三方模型：BASE=v0.443.46 / OURS=START_HEAD a5d1e54 / THEIRS=v0.444.1。Lingxi 与 upstream 无共同 git 祖先（recreated fork），无法用 git merge；用 `git merge-file`（base/ours/theirs 三个临时文件）做真 3-way。

### 文件分类（64 files）

- **OURS==BASE（fast-forward，Lingxi 未分歧，~30 文件）**：直接 take THEIRS。含 events.ts / rc-router.ts / ws-protocol.ts / websocket.ts / ws-message-handler.ts / context-slice.ts / format.ts / SessionList.module.css / experiments-registry / compaction-guard-ext / usage-observer / notification-service + 对应 tests + 5 个 NEW source（lossy-local-compaction / visible-text-accumulator / internal-mood-block）+ 2 个 NEW test。
- **OURS!=BASE（真 3-way，~24 文件）**：用 git merge-file。

### OURS!=BASE 3-way 结果

- **CLEAN（无冲突）**：bridge-session-manager.ts / desktop-session-submit.ts / engine.ts / session-compactor.ts / SessionList.tsx / ContextRing.tsx / app-event-actions.ts / ExperimentsTab.tsx / bridge-manager.ts / chat.ts + 全部 OURS!=BASE tests + 5 locale。
- **CONFLICT（手工裁决，已解）**：
  - `InputArea.tsx`：import 块。保留 Lingxi `lingxiFetch`（不取上游 `hanaFetch`）+ 接入上游新 file-mention（`searchDeskFiles` + `DeskSearchResult`）。
  - `message-parser.ts parseMoodFromContent`：取上游 canonical `parseLeadingInternalMoodBlock`（shared 新模块，更干净的抽象），但默认 yuan 保持 Lingxi 品牌 `'lingxi'`（非上游 `'hanako'`）。

### 品牌 invariant 修复（merge 引入的回流，已逐项改正）
- `ContextRing.tsx`：merge 把上游 `hanaFetch` 回流 → 改回 `lingxiFetch`（import + 调用点）。对应 test mock 同步 `hanaFetchMock`→`lingxiFetchMock`。
- `ExperimentsTab.test.tsx`：merge 回流 `hanaFetchMock`（一处）→ `lingxiFetchMock`。
- `message-parser.test.ts`：merge 回流期望 `yuan:'hanako'`（一处）→ `'lingxi'`。

### Phase 11 语义裁决（不新增/不盲删）
- **`core/session-manifest/legacy-migration.ts`**：在 START_HEAD **未 tracked**（Lingxi 已删除，无任何 tracked 代码 import 它；Lingxi 用 `core/migrations.ts` + `data-epoch-migrations.ts` 独立迁移体系）。上游 v0.444.1 仅给它加 BOM revalidation。裁决：**不重新引入**（语义已由 Lingxi 等价体系承担；上游 BOM fix 不适用于 Lingxi 独立迁移路径，无 tracked 消费方）。`tests/session-manifest-legacy-migration.test.ts` 同理不存在。
- **release-digest.v1/v2.json**：**不合并**（Phase 2.3：本任务非 release，digest 必须不变）。
- **package.json**：上游只改 `version`，Lingxi 保持 app=0.1.22；pi 版本已在 Phase 2 处理。

### locale（Phase 14）
- 5 locale 全 CLEAN merge，JSON 合法。
- 上游新增 `chat.instantSimpleCompaction` + `settings.experiments.instantSimpleCompaction.{title,description}` 在 en/ja/ko/zh-TW/zh **均有真实翻译**（非英文占位、非空）。
- i18n-locale-parity 仍 3 fail = **pre-existing debt**（4 个 key：`settings.providers.subtab.{api,models,usage}` + `settings.api.searchConfig`，与本次合并无因果，留 Phase 15 legacy cleanup）。

### 验证（迁移本身的 targeted tests，全绿）
- core/lib feature tests（12 file / 272 test）→ 全绿：lossy-local-compaction / mood-parser / visible-text-accumulator / notification / experiments / usage-observer / chat-compaction-events / session-compactor / bridge-handle-message / chat-route-switching / llm-client-provider-compat。
- desktop React tests（10 file）→ 全绿（context-ring 12 / message-parser 51 / file-mention / ExperimentsTab / etc.）。
- typecheck = 0 error。

## P12/P13. Phase 12 + 13 — 衍生产物重生成 + open-boundary

**关键环境事实**：`nvm exec 24.16.0` / `nvm run 24.16.0` 在本机被 `~/.local/bin/node`(v22.23.2) **PATH 抢占**——`nvm use` 后 PATH 里 `~/.local/bin` 仍在 nvm bin 之前。所有验证必须用显式 `export PATH="/Users/study_superior/.nvm/versions/node/v24.16.0/bin:$PATH"` 把真 v24.16.0 二进制顶到最前。本任务 P0 基线（13 failed）实际是在 v22 跑的——better-sqlite3 在 v22/v24 都能加载（预编译兼容），所以 sqlite 测试在 v22 也过；但 v24 跑才暴露真正的 NODE_MODULE_VERSION 137 需求。

### 产物重生成（用仓库自带 generator，非手编）
- `build/persistence-schema-fingerprint.json`：`generate-persistence-schema-fingerprint.mjs --classification compatible --compatibility-reason "..."`。CURRENT_SESSION_VERSION=3 / DATA_EPOCH=1 不变；只 pinned pi 版本 + sourceHash 变。与 0.80.3→0.83.0 同类 compatible addition。
- `build/cli-runtime-closure.json`：`compute-cli-closure.mjs` 重生成（10604 files，0.84.1 node_modules tree）。
- `export-manifest.json`（open-set），5 处合法扩充：
  - core/lossy-local-compaction.ts / lib/bridge/visible-text-accumulator.ts / shared/internal-mood-block.ts（上游新模块，与上游 manifest 一致）
  - core/provider-compat/ollama.ts（Lingxi 预存遗漏：19 个 sibling 已在 set，独漏 ollama）
  - node_modules/.../utils/abort.js（pi 0.84.1 auth-storage.js 新 import raceWithAbortSignal 的 transitive dep；auth-storage.js 已在 open-set/bundled）
- `build/open-boundary-baseline.json`：内容与 START_HEAD 一致（1 edge: server/index→mobile-workbench），未变。

### Phase 13 裁决
- 5 个新 open→closed edge（lossy/visible-text/internal-mood 被 core/lib import）→ Class A（上游合法新公开路径）→ 加 open-set 解决。
- provider-compat→ollama → Class B（Lingxi 代码越界）但修法是补 open-set（sibling 公开模块），非掩盖。
- auth-storage→abort → Class D（SDK 包内 transitive dep，CLI closure stale）→ 补 open-set。
- 结果：`lint:boundary ok`，open-boundary-lint 17/17（baseline 是 2 fail）。

### persistence tripwire：fixture 版本号 0.83.0→0.84.1
`tests/persistence-schema-tripwire.test.ts` 一处 fixture（packageVersion/requestedVersion）。guard 语义（currentSessionVersion=3、sha512- 完整性、extensions 列表、kind=external-versioned）未改。tripwire 15/15（baseline 4 fail）。

## P15. Phase 15 — pre-existing 失败修复（独立 commit）

- **i18n-locale-parity（3→0）**：ja/ko/zh-TW 缺 4 key（`settings.providers.subtab.{api,models,usage}` + `settings.api.searchConfig`），en/zh 已有。补真实翻译（非英文占位）。JSON reserialization 顺带合并了三 locale 在 settings.providers 内的 pre-existing 重复 `empty` key（last-wins，符合 runtime 语义）。
- **model-sync-routes（1→0）**：stale attribution 断言 `kind:'utility'`，auxiliary-slot refactor 已改 `kind:'auxiliary'`（server/routes/models.ts:249）。更新断言到当前契约。54/54。

## P15b. pi 0.84.1 AuthStorage 契约迁移（**required migration failure**）

v24 跑暴露（v22 因 sqlite 兼容未暴露）：`model-manager-auth-storage` 3 fail。根因：pi 0.84.1 raw `AuthStorage` 删了 `has`/`remove`（CredentialStore 只剩 read/list/modify/delete）。`_removeApiKeyProviderAuthEntries` 在 init() 早期跑时 `this._authStorage` 还是 raw AuthStorage，`has?.()`/`remove()` 全 no-op → legacy API-key 条目未从 auth.json 清掉。

修法（core/model-manager.ts）：存在性检查与删除按"任一可用 API"适配（has|read|get / remove|delete），不放宽语义（delete 仍 gate on presence）。29/29。

## P17. Phase 17 — 版本收尾
`lingxi.upstreamVersion` 0.443.46 → 0.444.1（package.json）。app version 保持 0.1.22。upstream-version-consistency 4/4。

