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

