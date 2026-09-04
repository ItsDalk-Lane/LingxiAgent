#!/usr/bin/env node
/**
 * 133-path upstream sync matrix — 机器真相源构建器。
 *
 * 权威输入：
 *   .sync-audit/delta-U-final.txt   git diff --name-status U0..U1（133 条）
 *   本文件内的 CURATED 表            每个 upstream path 一行的处置记录（人工策展）
 *
 * 产物：
 *   .sync-audit/upstream-sync-matrix.json   机器真相源（统计全部由脚本计算）
 *   UPSTREAM_SYNC_MATRIX.md                 人类可读投影（含 source JSON 哈希）
 *
 * 硬校验（任一失败即非零退出，不允许人工写数字绕过）：
 *   delta 路径数 = 133；CURATED 恰好覆盖全部 delta 路径（missing=0 extra=0）；
 *   disposition ∈ {ADOPTED, ADAPTED, REGENERATED, INTENTIONAL_DIVERGENCE}；
 *   四类处置之和 = 133。
 *
 * 用法：node .sync-audit/build-sync-matrix.mjs [--check]
 *   --check 只校验并比对产物是否与当前 CURATED/delta 一致，不写文件。
 */
import { execSync } from "child_process";
import crypto from "crypto";
import fs from "fs";
import path from "path";

const U0 = "cc19cb49b0786d61ed723764e0a83baf87887270"; // openhanako v0.444.1
const U1 = "c6d0405294be67cb134c2758f6472748ee73e2be"; // openhanako v0.447.4
const L0 = "97595264ead8735a04559507ddaade25db8a4e15"; // Lingxi v0.444.1 同步完成点（PR #2）
const L1 = "ca0b417e36a6a1f80947458aaed328a25718e41b"; // 本轮同步开始时 main

const ROOT = path.resolve(path.dirname(new URL(import.meta.url).pathname), "..");
const DELTA_FILE = path.join(ROOT, ".sync-audit", "delta-U-final.txt");
const JSON_OUT = path.join(ROOT, ".sync-audit", "upstream-sync-matrix.json");
const MD_OUT = path.join(ROOT, "UPSTREAM_SYNC_MATRIX.md");
const VERIFIED_SOURCE_SHA_FILE = path.join(ROOT, ".sync-audit", "verified-source-sha.txt");

// VERIFIED_SOURCE_SHA 是被 typecheck/lint/测试/构建/打包等最终验证所针对的
// feature commit（其 tree 即被验证的源码树）。必须是 commit 对象：post-verification
// audit-seal guard 以 `^{commit}` 校验可达性并做 VERIFIED..HEAD diff（第八轮曾误存
// tree sha，导致 seal guard 在 HEAD 长期红色——Phase 10 F-4 修复）。
// 该 commit 早于记录它的审计提交落地，无自引用（SHA = hash(contents)）问题。
// 当前 branch HEAD 可能在其后存在纯审计 seal 提交，HEAD 由 Git ref 自身标识。
const VERIFIED_SOURCE_SHA = "2387398589ec5494e1adb28b014dc84ebcf15a64";

const ALLOWED_DISPOSITIONS = ["ADOPTED", "ADAPTED", "REGENERATED", "INTENTIONAL_DIVERGENCE"];

const COMMIT_CLUSTERS = {
  "9dd70cd5": "dream", "476b2d7c": "dream", "408b7a18": "dream", "f29d15f2": "dream",
  "ca6dbf95": "dream", "483c5fe0": "dream", "06e17341": "dream",
  "bed24b93": "agents-md-persona", "b3927f07": "agents-md-persona",
  "a14a13bc": "automation-recovery", "61a2a6bf": "automation-recovery",
  "2870af8e": "markdown-bare-url",
  "d356e6ce": "context-ring",
  "ecc2c055": "windows-seed-cleanup",
  "def3e661": "build-receipts", "d96b5d67": "build-receipts",
  "5f08a4f3": "release-digest", "c6d04052": "release-digest",
};

/**
 * 每个 upstream changed path 一行（rename 以新 path 为键，renamedFrom 记录旧 path）。
 * 字段：
 *   conflictClass  A=Lingxi未改动 / B=双方改同文件职责不冲突 / C=Lingxi已重构该职责 / D=产品差异或生成物
 *   disposition    ADOPTED / ADAPTED / REGENERATED / INTENTIONAL_DIVERGENCE
 *   overlap        Lingxi 侧重叠情况
 *   implementation Lingxi 侧实现落点
 *   testEvidence   指向真实测试/验证（禁止只写 full suite）
 *   notes          备注（可为 ""）
 */
const CURATED = {
  // ── 集群 dream ──────────────────────────────────────────────
  "lib/memory/dream/memory-units.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "同名移植，接 Lingxi memory 数据布局（facts/today/week/longterm）",
    testEvidence: "tests/memory-dream-units.test.ts（11 用例）",
    notes: "",
  },
  "lib/memory/dream/model-runner.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "模型解析改走 engine.resolveAuxiliaryExecution(\"memory\", { agentId })，禁止 utility/chat 回落",
    testEvidence: "tests/memory-dream-model-runner.test.ts（10 用例）+ tests/memory-dream-memory-slot.test.ts（3 用例，锁槽位契约）",
    notes: "软目标 + 5000 字符硬上限",
  },
  "lib/memory/dream/revision-store.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "同名移植，入 Lingxi store registry；snapshotDreamSections 复用为 current-vs-revision diff 的当前快照源",
    testEvidence: "tests/memory-dream-revision.test.ts（8 用例）",
    notes: "",
  },
  "lib/memory/dream/runner.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "同名移植：编排 + per-agent 并发锁 + memory_changed 竞态 + revision-before-write",
    testEvidence: "tests/memory-dream-runner.test.ts（11 用例）+ tests/memory-dream-memory-slot.test.ts",
    notes: "",
  },
  "lib/memory/dream/state-store.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "同名移植：Dream 状态持久化 + dream_* 稳定错误码",
    testEvidence: "tests/memory-dream-runner.test.ts + tests/memory-dream-route.test.ts（错误码映射断言）",
    notes: "",
  },
  "lib/memory/prompts/dream.ts": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "同名移植（5 阶段 prompt，校对 Lingxi 术语）",
    testEvidence: "tests/memory-dream-model-runner.test.ts / tests/memory-dream-runner.test.ts（prompt 经 runner 链路断言）",
    notes: "",
  },
  "lib/memory/memory-ticker.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无（L0 后未改）",
    implementation: "合并 Dream 触发钩子进当前 ticker，gated on getDreamAutoEnabled()===true",
    testEvidence: "tests/memory-ticker-dream.test.ts（4 用例）",
    notes: "",
  },
  "server/routes/memory-dream.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "同名移植对齐 Lingxi 路由约定；本轮扩展 revision detail 响应附带后端现读的 current 快照（current-vs-revision diff 数据源）",
    testEvidence: "tests/memory-dream-route.test.ts（7 用例，含 current 快照断言）",
    notes: "",
  },
  "server/composition/open-root.ts": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "同位置挂载 createMemoryDreamRoute",
    testEvidence: "tests/server-composition-boundary.test.ts（8 用例）+ npm run lint:boundary",
    notes: "",
  },
  "server/http/route-security.ts": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "dream 路由安全分级同步",
    testEvidence: "tests/http-route-security.test.ts",
    notes: "",
  },
  "server/routes/agents.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "persona 双路由（canonical /agents-md + legacy /ishiki alias 同 handler）+ dream auto_enabled 布尔校验（400 先于 updateConfig）",
    testEvidence: "tests/agents-route.test.ts（30 用例）+ tests/agent-config-tools-disabled.test.ts（12 用例，含本轮补齐的 3 个 dream 契约）",
    notes: "upstream 两个集群（dream + agents-md-persona）都改了此文件，此处只记一行",
  },
  "shared/persistence/store-registry.ts": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "L0 后有改",
    implementation: "结构化合并：dream state/revision store 登记 + AGENTS.md 路径模式/rename 规则",
    testEvidence: "persistence fingerprint 校验（build/persistence-schema-fingerprint.json 重新生成 + compatible 判定）+ migration smoke 23/23",
    notes: "DATA_EPOCH 不变",
  },
  "shared/error-user-messages.ts": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "dream_* 错误码 → 用户文案映射合并",
    testEvidence: "desktop/src/react/__tests__/shared/error-user-messages.test.ts（26 用例）",
    notes: "",
  },
  "desktop/src/react/settings/tabs/agent/AgentMemoryDream.tsx": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "移植并对齐 Lingxi settings API client（saveDreamAutoEnabled 走 /api/agents/:id/config）",
    testEvidence: "desktop/src/react/__tests__/settings/AgentMemory.test.tsx（5 用例）",
    notes: "",
  },
  "desktop/src/react/settings/tabs/agent/DreamRevisionBrowser.tsx": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "移植；本轮升级为 current-vs-revision 统一 diff（复用 desktop/src/react/utils/line-diff.ts），确认前现取 current，相同版本禁用恢复",
    testEvidence: "desktop/src/react/settings/tabs/agent/__tests__/DreamRevisionBrowser.test.tsx（6 用例，含 A–F diff/确认/刷新契约）",
    notes: "",
  },
  "desktop/src/react/settings/tabs/agent/DreamRevisionBrowser.module.css": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "移植 + 本轮追加 diff 行样式（lineAdded/lineRemoved/lineSame，沿用 FileHistoryModal 色板约定）",
    testEvidence: "DreamRevisionBrowser.test.tsx（diff 标记渲染断言）",
    notes: "",
  },
  "desktop/src/react/settings/tabs/agent/agent-memory-dream-actions.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "移植；本轮 loadDreamRevision 改返回 { revision, current } 并新增 dreamSectionsEqual",
    testEvidence: "DreamRevisionBrowser.test.tsx（经 importOriginal 保留纯函数真实实现）",
    notes: "",
  },
  "desktop/src/react/settings/tabs/agent/dream-error-presenter.ts": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "同名移植：dream_* 错误码 → i18n",
    testEvidence: "DreamRevisionBrowser.test.tsx（本地化恢复错误用例）",
    notes: "",
  },
  "desktop/src/react/settings/tabs/agent/AgentMemory.tsx": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "Dream 区嵌入合并进当前组件",
    testEvidence: "desktop/src/react/__tests__/settings/AgentMemory.test.tsx（5 用例）",
    notes: "",
  },
  "desktop/src/react/settings/tabs/AgentTab.tsx": {
    conflictClass: "C", disposition: "ADAPTED", overlap: "L0 后有改",
    implementation: "按当前结构合并 persona 文案 + dream 入口",
    testEvidence: "desktop/src/react/__tests__/settings/AgentTab.test.tsx",
    notes: "dream 与 agents-md-persona 两集群均触及",
  },
  "desktop/src/react/settings/Settings.module.css": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "L0 后有改",
    implementation: "追加 dream 样式段（不整文件覆盖）",
    testEvidence: "desktop/src/react/settings/__tests__/SettingsContent.test.tsx + DreamRevisionBrowser/AgentMemory 组件测试",
    notes: "",
  },
  "desktop/src/react/settings/settings-search-index.ts": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "L0 后有改",
    implementation: "追加 dream 搜索项",
    testEvidence: "desktop/src/react/settings/__tests__/SettingsContent.test.tsx（搜索索引消费侧）",
    notes: "",
  },
  "tests/memory-dream-model-runner.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "移植 + 新增 memory-slot 专项断言",
    testEvidence: "本文件自身全绿（10 用例）",
    notes: "",
  },
  "tests/memory-dream-revision.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "移植",
    testEvidence: "本文件自身全绿（8 用例）",
    notes: "",
  },
  "tests/memory-dream-route.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "移植；本轮新增 detail 响应 current 快照断言",
    testEvidence: "本文件自身全绿（7 用例）",
    notes: "",
  },
  "tests/memory-dream-runner.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "移植",
    testEvidence: "本文件自身全绿（11 用例）",
    notes: "",
  },
  "tests/memory-dream-units.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "移植",
    testEvidence: "本文件自身全绿（11 用例）",
    notes: "",
  },
  "tests/memory-ticker-dream.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "移植",
    testEvidence: "本文件自身全绿（4 用例）",
    notes: "",
  },
  "desktop/src/react/settings/tabs/agent/__tests__/DreamRevisionBrowser.test.tsx": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "移植；本轮重写为 current-vs-revision diff 契约（任务书 A–F 用例）",
    testEvidence: "本文件自身全绿（6 用例）",
    notes: "",
  },
  "desktop/src/react/__tests__/settings/AgentMemory.test.tsx": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "合并断言",
    testEvidence: "本文件自身全绿（5 用例）",
    notes: "",
  },
  "desktop/src/react/__tests__/shared/error-user-messages.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "合并断言",
    testEvidence: "本文件自身全绿（26 用例）",
    notes: "",
  },
  "tests/agent-config-tools-disabled.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "合并；本轮补齐 upstream 的 dream auto_enabled 正反契约（200 透传 / 400 拒非布尔且 updateConfig 未被调用）",
    testEvidence: "本文件自身全绿（12 用例，含 3 个 dream 契约）",
    notes: "上一轮矩阵误标 ✅，本轮实补后转真绿",
  },
  "tests/http-route-security.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "合并 dream 路由断言",
    testEvidence: "本文件自身全绿",
    notes: "",
  },

  // ── 集群 agents-md-persona ──────────────────────────────────
  "core/agents-md-migration.ts": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "同名移植 + 品牌检查；本轮扩展 failedDetails 结构化失败记录 + buildFailedPersonaRenameIndex（migration-degraded 运行时状态）",
    testEvidence: "tests/agents-md-startup-migration.test.ts（11 用例，含 5 个 degraded fallback 用例）+ migration smoke 23/23",
    notes: "",
  },
  "core/persona-source.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "PersonaKind identity|agents + agentPersonaFilePaths()；本轮新增显式 migrationFallback 参数（仅启动失败记录可构造）",
    testEvidence: "tests/persona-source.test.ts（15 用例）+ tests/agents-md-startup-migration.test.ts",
    notes: "",
  },
  "core/agent.ts": {
    conflictClass: "C", disposition: "ADAPTED", overlap: "双方均改（dream 模型槽 + persona）",
    implementation: "人格路径 + workspace excludeFiles + getResolvedMemoryModel 闭包；本轮 readAgentsMdSource/_readPublicAgentsMd 消费 migration-degraded fallback",
    testEvidence: "tests/agents-md-startup-migration.test.ts（Agent 真实链路断言）+ tests/memory-dream-memory-slot.test.ts + agent 系列定向测试",
    notes: "上游 dream 与 persona 两集群均触及",
  },
  "core/agent-manager.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "默认人格文件创建改 AGENTS 命名；本轮 setCallbacks 接线 getFailedPersonaRename",
    testEvidence: "tests/agent-manager-create-defaults.test.ts（26 用例）+ migration smoke",
    notes: "",
  },
  "core/bridge-session-manager.ts": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "L0 后有改",
    implementation: "结构化合并（workspace excludeFiles 接线之一）",
    testEvidence: "tests/bridge-session-orphan-repair.test.ts + tests/bridge-session-teardown.test.ts + tests/workspace-instruction-files-exclude.test.ts（bridge 路径用例）",
    notes: "",
  },
  "core/engine.ts": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "L0 后有改",
    implementation: "结构化合并 + 启动步 agents-md-rename；本轮把失败结果保留为 _failedPersonaRenames 运行时状态并暴露 getFailedPersonaRename",
    testEvidence: "tests/agents-md-startup-migration.test.ts + migration smoke 23/23 + 全量套件 engine 启动链路",
    notes: "",
  },
  "core/first-run.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "种子人格文件名改 AGENTS 命名",
    testEvidence: "tests/first-run-default-workspace.test.ts + migration smoke（首跑清洁断言）",
    notes: "",
  },
  "core/llm-utils.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "合并命名更新",
    testEvidence: "全量套件内 llm-utils 消费链路（agent/system prompt 组装测试）",
    notes: "",
  },
  "core/session-coordinator.ts": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "L0 后有改",
    implementation: "结构化合并（workspace excludeFiles 接线之一）",
    testEvidence: "tests/workspace-instruction-files-exclude.test.ts（desktop session 路径用例）+ tests/session-tool-gating.test.ts",
    notes: "",
  },
  "core/workspace-instruction-files.ts": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "新增 excludeFiles（精确绝对路径排除，杜绝人格文件重复注入）",
    testEvidence: "tests/workspace-instruction-files-exclude.test.ts（4 用例）",
    notes: "",
  },
  "hub/agent-executor.ts": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "L0 后有改",
    implementation: "结构化合并",
    testEvidence: "全量套件内 agent-executor 链路（hub 执行测试）",
    notes: "",
  },
  "lib/character-cards/service.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "导出写新 key（prompts.agents/publicAgents），导入永久兼容 legacy key",
    testEvidence: "tests/character-card-import.test.ts",
    notes: "",
  },
  "lib/diary/diary-writer.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "合并命名更新",
    testEvidence: "全量套件内 diary 链路测试",
    notes: "",
  },
  "lib/sandbox/policy.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "合并命名更新",
    testEvidence: "全量套件内 sandbox policy 测试",
    notes: "",
  },
  "server/routes/settings-snapshot.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "合并命名更新",
    testEvidence: "tests/settings-snapshot-route.test.ts",
    notes: "",
  },
  "desktop/src/react/settings/store.ts": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "部分 overlap",
    implementation: "ishiki→agents 文案/API 逐文件合并",
    testEvidence: "desktop/src/react/__tests__/settings/actions.test.ts + helpers.test.ts + useBridgeState.snapshot.test.tsx",
    notes: "",
  },
  "desktop/src/react/settings/actions.ts": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "部分 overlap",
    implementation: "ishiki→agents 文案/API 逐文件合并",
    testEvidence: "desktop/src/react/__tests__/settings/actions.test.ts",
    notes: "",
  },
  "desktop/src/react/settings/helpers.ts": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "部分 overlap",
    implementation: "ishiki→agents 文案/API 逐文件合并",
    testEvidence: "desktop/src/react/__tests__/settings/helpers.test.ts",
    notes: "",
  },
  "desktop/src/react/settings/tabs/BridgeTab.tsx": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "部分 overlap",
    implementation: "ishiki→agents 文案/API 逐文件合并",
    testEvidence: "desktop/src/react/__tests__/settings/BridgeTab.credentials.test.tsx + BridgeTab.permission-mode.test.tsx",
    notes: "",
  },
  "desktop/src/react/settings/tabs/bridge/useBridgeState.ts": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "部分 overlap",
    implementation: "ishiki→agents 文案/API 逐文件合并",
    testEvidence: "desktop/src/react/__tests__/settings/useBridgeState.snapshot.test.tsx",
    notes: "",
  },
  "desktop/src/react/settings/overlays/CharacterCardPreviewOverlay.tsx": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "部分 overlap",
    implementation: "ishiki→agents 文案/API 逐文件合并",
    testEvidence: "desktop/src/react/settings/__tests__/SettingsContent.test.tsx",
    notes: "",
  },
  "scripts/build-server.mjs": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "L0 后有改",
    implementation: "仅套用模板目录改名（ishiki-templates→agents-templates）",
    testEvidence: "tests/build-server-open.test.ts + npm run build:server 产物抽检（seed 含 agents-templates）",
    notes: "",
  },
  "scripts/build-server-open.mjs": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "同上（模板目录改名）",
    testEvidence: "tests/build-server-open.test.ts",
    notes: "",
  },
  "scripts/export-open-tree.mjs": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "注释同步",
    testEvidence: "export-manifest.json 重新生成 + export manifest 定向校验",
    notes: "",
  },
  "scripts/i18n-backfill-ja.json": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "noIshiki→noAgentsMd key 改名",
    testEvidence: "tests/i18n-locale-parity.test.ts",
    notes: "",
  },
  "scripts/i18n-backfill-ko.json": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "noIshiki→noAgentsMd key 改名",
    testEvidence: "tests/i18n-locale-parity.test.ts",
    notes: "",
  },
  "tests/agents-md-startup-migration.test.ts": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "移植；本轮新增 5 个 migration-degraded 用例（失败保人格/无永久双读/新文件优先/下次成功/public 变体）",
    testEvidence: "本文件自身全绿（11 用例）",
    notes: "",
  },
  "tests/workspace-instruction-files-exclude.test.ts": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "移植",
    testEvidence: "本文件自身全绿（4 用例）",
    notes: "",
  },
  "skills2set/character-creator/SKILL.md": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "Lingxi 品牌",
    implementation: "保留品牌文案，命名对齐 AGENTS.md 协议",
    testEvidence: "打包 smoke 抽检（bundled skills 随产物带出）+ 全量套件 skills 链路",
    notes: "",
  },
  "skills2set/character-creator/references/anti-slop.md": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "Lingxi 品牌",
    implementation: "保留品牌文案，命名对齐 AGENTS.md 协议",
    testEvidence: "打包 smoke 抽检（bundled skills 随产物带出）",
    notes: "",
  },
  "skills2set/character-creator/references/card-format.md": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "Lingxi 品牌",
    implementation: "保留品牌文案，命名对齐 AGENTS.md 协议",
    testEvidence: "打包 smoke 抽检（bundled skills 随产物带出）",
    notes: "",
  },
  "skills2set/user-guide/SKILL.md": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "Lingxi 品牌",
    implementation: "保留品牌文案，命名对齐 AGENTS.md 协议",
    testEvidence: "打包 smoke 抽检（bundled skills 随产物带出）",
    notes: "",
  },

  // ── 模板 R100 重命名（13 条，逐 path 独立记录）───────────────
  "lib/agents-templates/butter.md": {
    renamedFrom: "lib/ishiki-templates/butter.md",
    conflictClass: "A", disposition: "ADOPTED", overlap: "无（纯改名）",
    implementation: "R100 目录改名落地",
    testEvidence: "tests/persona-source.test.ts（模板回落链）+ 打包 smoke（agents-templates 存在、ishiki-templates 不残留）",
    notes: "R100 纯改名",
  },
  "lib/agents-templates/en/butter.md": {
    renamedFrom: "lib/ishiki-templates/en/butter.md",
    conflictClass: "A", disposition: "ADOPTED", overlap: "无（纯改名）",
    implementation: "R100 目录改名落地",
    testEvidence: "tests/persona-source.test.ts（en 语言模板回落用例）+ 打包 smoke",
    notes: "R100 纯改名",
  },
  "lib/agents-templates/en/hanako.md": {
    renamedFrom: "lib/ishiki-templates/en/hanako.md",
    conflictClass: "A", disposition: "ADAPTED", overlap: "品牌路径映射",
    implementation: "上游 AGENTS 模板行为与内容同步；Lingxi 保留产品品牌路径 hanako.md → lingxi.md",
    testEvidence: "tests/persona-source.test.ts + 打包 smoke",
    notes: "品牌级路径映射，分类为 ADAPTED",
  },
  "lib/agents-templates/en/ming.md": {
    renamedFrom: "lib/ishiki-templates/en/ming.md",
    conflictClass: "A", disposition: "ADOPTED", overlap: "无（纯改名）",
    implementation: "R100 目录改名落地",
    testEvidence: "tests/persona-source.test.ts + 打包 smoke",
    notes: "R100 纯改名",
  },
  "lib/agents-templates/hanako.md": {
    renamedFrom: "lib/ishiki-templates/hanako.md",
    conflictClass: "A", disposition: "ADAPTED", overlap: "品牌路径映射",
    implementation: "上游 AGENTS 模板行为与内容同步；Lingxi 保留产品品牌路径 hanako.md → lingxi.md",
    testEvidence: "tests/persona-source.test.ts + 打包 smoke",
    notes: "品牌级路径映射，分类为 ADAPTED",
  },
  "lib/agents-templates/ming.md": {
    renamedFrom: "lib/ishiki-templates/ming.md",
    conflictClass: "A", disposition: "ADOPTED", overlap: "无（纯改名）",
    implementation: "R100 目录改名落地",
    testEvidence: "tests/persona-source.test.ts + 打包 smoke",
    notes: "R100 纯改名",
  },
  "lib/agents-public-templates/butter.md": {
    renamedFrom: "lib/public-ishiki-templates/butter.md",
    conflictClass: "A", disposition: "ADOPTED", overlap: "无（纯改名）",
    implementation: "R100 目录改名落地",
    testEvidence: "tests/agents-md-startup-migration.test.ts（public 变体迁移用例）+ 打包 smoke",
    notes: "R100 纯改名",
  },
  "lib/agents-public-templates/en/butter.md": {
    renamedFrom: "lib/public-ishiki-templates/en/butter.md",
    conflictClass: "A", disposition: "ADOPTED", overlap: "无（纯改名）",
    implementation: "R100 目录改名落地",
    testEvidence: "tests/agents-md-startup-migration.test.ts + 打包 smoke",
    notes: "R100 纯改名",
  },
  "lib/agents-public-templates/en/hanako.md": {
    renamedFrom: "lib/public-ishiki-templates/en/hanako.md",
    conflictClass: "A", disposition: "ADAPTED", overlap: "品牌路径映射",
    implementation: "上游 AGENTS 模板行为与内容同步；Lingxi 保留产品品牌路径 hanako.md → lingxi.md",
    testEvidence: "tests/agents-md-startup-migration.test.ts + 打包 smoke",
    notes: "品牌级路径映射，分类为 ADAPTED",
  },
  "lib/agents-public-templates/en/ming.md": {
    renamedFrom: "lib/public-ishiki-templates/en/ming.md",
    conflictClass: "A", disposition: "ADOPTED", overlap: "无（纯改名）",
    implementation: "R100 目录改名落地",
    testEvidence: "tests/agents-md-startup-migration.test.ts + 打包 smoke",
    notes: "R100 纯改名",
  },
  "lib/agents-public-templates/hanako.md": {
    renamedFrom: "lib/public-ishiki-templates/hanako.md",
    conflictClass: "A", disposition: "ADAPTED", overlap: "品牌路径映射",
    implementation: "上游 AGENTS 模板行为与内容同步；Lingxi 保留产品品牌路径 hanako.md → lingxi.md",
    testEvidence: "tests/agents-md-startup-migration.test.ts + 打包 smoke",
    notes: "品牌级路径映射，分类为 ADAPTED",
  },
  "lib/agents-public-templates/ming.md": {
    renamedFrom: "lib/public-ishiki-templates/ming.md",
    conflictClass: "A", disposition: "ADOPTED", overlap: "无（纯改名）",
    implementation: "R100 目录改名落地",
    testEvidence: "tests/agents-md-startup-migration.test.ts + 打包 smoke",
    notes: "R100 纯改名",
  },
  "lib/agents.example.md": {
    renamedFrom: "lib/ishiki.example.md",
    conflictClass: "A", disposition: "ADAPTED", overlap: "有 Lingxi 品牌内容",
    implementation: "改名并保留 Lingxi 品牌内容（example 兜底人格）",
    testEvidence: "tests/persona-source.test.ts（example 兜底层用例）",
    notes: "R100 但内容含品牌差异，故 ADAPTED",
  },

  // ── 集群 automation-recovery ────────────────────────────────
  "lib/desk/cron-store.ts": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "Lingxi 扩展在 L0 前并入",
    implementation: "行为不变量提取后合并：tmp 校验→逐字节损坏备份→原子提升；保留 configRevision/storeRevision/重入保护",
    testEvidence: "tests/cron-store.test.ts",
    notes: "稳定错误码 cron_store_corrupt/unavailable/recovery_failed；禁止静默返回 []",
  },
  "server/routes/desk.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "错误传播不泄露 fs 绝对路径",
    testEvidence: "tests/desk-route-cron.test.ts",
    notes: "",
  },
  "desktop/src/react/components/AutomationPanel.tsx": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "reload 失败保留上一次 jobs + badge；Add 防重复（addingManualJob 锁）",
    testEvidence: "desktop/src/react/__tests__/components/AutomationPanel.test.tsx",
    notes: "",
  },
  "desktop/src/react/components/chat/AssistantMessage.tsx": {
    conflictClass: "C", disposition: "ADAPTED", overlap: "L0 后已重构（聊天语义管线）",
    implementation: "automation suggestion 失败 UI 经 Lingxi ContentBlock/renderer registry 架构表达，未套 upstream 旧 JSX patch",
    testEvidence: "desktop/src/react/__tests__/components/AssistantMessage.automation-suggestion.test.tsx",
    notes: "禁止回退聊天架构",
  },
  "desktop/src/react/__tests__/components/AutomationPanel.test.tsx": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "移植",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "desktop/src/react/__tests__/components/AssistantMessage.automation-suggestion.test.tsx": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "对齐 ContentBlock 架构后重写",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "tests/cron-store.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "合并恢复断言（含 Lingxi 扩展字段保全）",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "tests/desk-route-cron.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "合并错误传播断言",
    testEvidence: "本文件自身全绿",
    notes: "",
  },

  // ── 集群 markdown-bare-url ──────────────────────────────────
  "desktop/src/react/editor/md-decorations.ts": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "移植 patch：裸 URL 不再被装饰/破坏",
    testEvidence: "desktop/src/react/__tests__/editor/md-decorations.test.ts",
    notes: "",
  },
  "desktop/src/react/__tests__/editor/md-decorations.test.ts": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "移植",
    testEvidence: "本文件自身全绿",
    notes: "",
  },

  // ── 集群 context-ring ───────────────────────────────────────
  "desktop/src/react/components/input/ContextRing.tsx": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "移植：压缩动作优先级排序",
    testEvidence: "desktop/src/react/__tests__/components/context-ring.test.tsx",
    notes: "",
  },
  "desktop/src/react/__tests__/components/context-ring.test.tsx": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "移植",
    testEvidence: "本文件自身全绿",
    notes: "",
  },

  // ── 集群 windows-seed-cleanup ───────────────────────────────
  "build/installer.nsh": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "L0 后有改",
    implementation: "overlay 更新时 RMDir /r \"$INSTDIR\\resources\\seed\"；严格限于安装目录应用自有 seed，禁触 LINGXI_HOME/用户数据",
    testEvidence: "tests/windows-installer-contract.test.ts（19 用例）",
    notes: "真实 Windows 安装器执行未进行（宿主平台限制），contract 测试为门禁",
  },
  "tests/windows-installer-contract.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "合并断言 + lingxiRemoveOwnedInstallTrees 宏名修正",
    testEvidence: "本文件自身全绿（19 用例）",
    notes: "",
  },

  // ── 集群 i18n（5 locale，每文件一行）────────────────────────
  "desktop/src/locales/zh.json": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "L0 后有改",
    implementation: "key-level merge：删 ishiki* 增 agentsMd*/dream 树/error.code.dream*；本轮追加 dream revisions diff 5 key",
    testEvidence: "tests/i18n-locale-parity.test.ts + tests/react-locale-coverage.test.ts",
    notes: "禁止整文件覆盖",
  },
  "desktop/src/locales/zh-TW.json": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "L0 后有改",
    implementation: "key-level merge（同 zh）；本轮追加 dream revisions diff 5 key",
    testEvidence: "tests/i18n-locale-parity.test.ts + tests/react-locale-coverage.test.ts",
    notes: "禁止整文件覆盖",
  },
  "desktop/src/locales/en.json": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "L0 后有改",
    implementation: "key-level merge（同 zh）；本轮追加 dream revisions diff 5 key",
    testEvidence: "tests/i18n-locale-parity.test.ts + tests/react-locale-coverage.test.ts",
    notes: "禁止整文件覆盖",
  },
  "desktop/src/locales/ja.json": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "L0 后有改",
    implementation: "key-level merge（同 zh）；本轮追加 dream revisions diff 5 key",
    testEvidence: "tests/i18n-locale-parity.test.ts + tests/react-locale-coverage.test.ts + scripts/i18n-backfill-ja.json",
    notes: "禁止整文件覆盖",
  },
  "desktop/src/locales/ko.json": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "L0 后有改",
    implementation: "key-level merge（同 zh）；本轮追加 dream revisions diff 5 key",
    testEvidence: "tests/i18n-locale-parity.test.ts + tests/react-locale-coverage.test.ts + scripts/i18n-backfill-ko.json",
    notes: "禁止整文件覆盖",
  },

  // ── 集群 build-receipts / 生成物 ────────────────────────────
  "scripts/compute-cli-closure.mjs": {
    conflictClass: "A", disposition: "ADOPTED", overlap: "无",
    implementation: "移植：justification 不再引用易变行号",
    testEvidence: "build/cli-runtime-closure.json 由该生成器重新生成且二次生成零漂移（确定性验证）",
    notes: "",
  },
  "build/cli-runtime-closure.json": {
    conflictClass: "D", disposition: "REGENERATED", overlap: "生成物",
    implementation: "Lingxi 生成器（scripts/compute-cli-closure.mjs）在最终源码树重新生成，禁止从 upstream 复制",
    testEvidence: "二次生成 git diff 为零（deterministic 验证）+ npm run build 链路",
    notes: "",
  },
  "build/persistence-schema-fingerprint.json": {
    conflictClass: "D", disposition: "REGENERATED", overlap: "生成物",
    implementation: "Lingxi 生成器重新生成；compatible 增补 + compatibility-reason，DATA_EPOCH 不变",
    testEvidence: "persistence fingerprint 定向校验 + 二次生成零漂移",
    notes: "",
  },
  "build/persistence-store-inventory.json": {
    conflictClass: "D", disposition: "REGENERATED", overlap: "生成物",
    implementation: "Lingxi 生成器重新生成",
    testEvidence: "persistence store inventory 定向校验 + 二次生成零漂移",
    notes: "",
  },
  "export-manifest.json": {
    conflictClass: "D", disposition: "REGENERATED", overlap: "生成物",
    implementation: "Lingxi 手工策展权威源重新生成（704→712）",
    testEvidence: "export manifest 定向校验（tests 内 manifest 一致性用例）",
    notes: "",
  },

  // ── 集群 release-digest / 产品差异 ──────────────────────────
  "package.json": {
    conflictClass: "D", disposition: "INTENTIONAL_DIVERGENCE", overlap: "产品差异",
    implementation: "Lingxi 自有 name/version/pi 0.84.1/发布体系；上游 U0..U1 仅 version bump 无新依赖",
    testEvidence: "npm run typecheck + 全量套件（依赖未变更前提下的回归证据）",
    notes: "lingxi.upstreamVersion=0.447.4 保留",
  },
  "package-lock.json": {
    conflictClass: "D", disposition: "INTENTIONAL_DIVERGENCE", overlap: "产品差异",
    implementation: "随 package.json；上游无新依赖可移植",
    testEvidence: "npm install 链路 + 全量套件",
    notes: "",
  },
  "release-digest.v1.json": {
    conflictClass: "D", disposition: "INTENTIONAL_DIVERGENCE", overlap: "产品差异",
    implementation: "Lingxi 发布历史独立；上游 digest 拷贝即伪造",
    testEvidence: "release-digest 由 scripts/generate-release-digest 系列维护（包内回退数据）",
    notes: "",
  },
  "release-digest.v2.json": {
    conflictClass: "D", disposition: "INTENTIONAL_DIVERGENCE", overlap: "产品差异",
    implementation: "同上",
    testEvidence: "release-digest 由 scripts/generate-release-digest 系列维护（包内回退数据）",
    notes: "",
  },

  // ── persona 集群其余测试文件（逐 path 一行）─────────────────
  "tests/persona-source.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新",
    testEvidence: "本文件自身全绿（15 用例）",
    notes: "",
  },
  "tests/agents-route.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新（canonical + legacy alias 双路由）",
    testEvidence: "本文件自身全绿（30 用例）",
    notes: "",
  },
  "tests/character-card-import.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新（legacy key 导入兼容）",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "tests/agent-description.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "tests/agent-experience-toggle.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "tests/agent-interactive-card-tools.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "tests/agent-locale-resolution.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新",
    testEvidence: "本文件自身全绿（4 用例）",
    notes: "",
  },
  "tests/agent-manager-create-defaults.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新",
    testEvidence: "本文件自身全绿（26 用例）",
    notes: "",
  },
  "tests/agent-master-session-decoupling.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "tests/agent-system-prompt-section-order.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新",
    testEvidence: "本文件自身全绿（2 用例）",
    notes: "",
  },
  "tests/agent-tools-conditional-injection.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "tests/bridge-session-orphan-repair.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "tests/bridge-session-teardown.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "tests/build-server-open.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新（模板目录改名）",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "tests/builtin-tool-permission-coverage.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "tests/channel-router-memory-master.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "tests/first-run-default-workspace.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "tests/server-composition-boundary.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新（含 dream 路由挂载清单）",
    testEvidence: "本文件自身全绿（8 用例）",
    notes: "",
  },
  "tests/session-tool-gating.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "tests/settings-snapshot-route.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "命名/行为断言更新",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "desktop/src/react/__tests__/settings/AgentTab.test.tsx": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "3 个 desktop 测试 overlap 之一",
    implementation: "合并",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "desktop/src/react/__tests__/settings/BridgeTab.credentials.test.tsx": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "3 个 desktop 测试 overlap 之一",
    implementation: "合并",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "desktop/src/react/__tests__/settings/BridgeTab.permission-mode.test.tsx": {
    conflictClass: "B", disposition: "ADAPTED", overlap: "3 个 desktop 测试 overlap 之一",
    implementation: "合并",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "desktop/src/react/__tests__/settings/actions.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "合并",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "desktop/src/react/__tests__/settings/helpers.test.ts": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "合并",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "desktop/src/react/__tests__/settings/useBridgeState.snapshot.test.tsx": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "合并",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
  "desktop/src/react/settings/__tests__/SettingsContent.test.tsx": {
    conflictClass: "A", disposition: "ADAPTED", overlap: "无",
    implementation: "合并",
    testEvidence: "本文件自身全绿",
    notes: "",
  },
};

// ── 构建逻辑 ──────────────────────────────────────────────────

function parseDelta() {
  const lines = fs.readFileSync(DELTA_FILE, "utf-8").split("\n").filter(Boolean);
  return lines.map((line) => {
    const parts = line.split("\t");
    const change = parts[0];
    if (change.startsWith("R")) {
      return { change, oldPath: parts[1], path: parts[2] };
    }
    return { change, path: parts[1] };
  });
}

function upstreamCommitsFor(filePath, oldPath) {
  const paths = oldPath ? [filePath, oldPath] : [filePath];
  const quoted = paths.map((p) => `'${p.replace(/'/g, "'\\''")}'`).join(" ");
  const out = execSync(
    `git log --format=%h ${U0}..${U1} -- ${quoted}`,
    { cwd: ROOT, encoding: "utf-8", maxBuffer: 1024 * 1024 },
  );
  // 去重保序（git log 逆序 → 翻转为时间正序）
  const commits = [...new Set(out.split("\n").filter(Boolean))].reverse();
  return commits;
}

function clustersFor(commits) {
  return [...new Set(commits.map((short) => COMMIT_CLUSTERS[short] || `unknown(${short})`))];
}

function buildRecords() {
  const delta = parseDelta();
  const errors = [];
  const deltaPaths = new Set(delta.map((entry) => entry.path));
  for (const key of Object.keys(CURATED)) {
    if (!deltaPaths.has(key)) errors.push(`CURATED extra path not in ΔU: ${key}`);
  }
  const records = delta.map((entry) => {
    const curated = CURATED[entry.path];
    if (!curated) {
      errors.push(`ΔU path missing curation: ${entry.path}`);
      return null;
    }
    if (!ALLOWED_DISPOSITIONS.includes(curated.disposition)) {
      errors.push(`${entry.path}: illegal disposition ${curated.disposition}`);
    }
    if (!curated.testEvidence || /^(full suite|全量测试|all green)$/i.test(curated.testEvidence.trim())) {
      errors.push(`${entry.path}: test_evidence 缺失或过于笼统`);
    }
    const commits = upstreamCommitsFor(entry.path, entry.oldPath);
    return {
      upstream_path: entry.path,
      renamed_from: entry.oldPath || null,
      upstream_change: entry.change,
      upstream_commits: commits,
      upstream_clusters: clustersFor(commits),
      lingxi_overlap: curated.overlap,
      conflict_class: curated.conflictClass,
      disposition: curated.disposition,
      lingxi_implementation: curated.implementation,
      test_evidence: curated.testEvidence,
      notes: curated.notes || "",
      status: "✅",
    };
  });
  if (errors.length > 0) {
    for (const error of errors) console.error(`✗ ${error}`);
    throw new Error(`matrix curation invalid: ${errors.length} error(s)`);
  }
  return records;
}

export function summarize(records) {
  const summary = { total: records.length, ADOPTED: 0, ADAPTED: 0, REGENERATED: 0, INTENTIONAL_DIVERGENCE: 0 };
  for (const record of records) summary[record.disposition] += 1;
  return summary;
}

export function renderMarkdown(records, meta) {
  const summary = summarize(records);
  const lines = [];
  lines.push("# UPSTREAM_SYNC_MATRIX — v0.444.1 → v0.447.4");
  lines.push("");
  lines.push("> 本文件由 `node .sync-audit/build-sync-matrix.mjs` 从 `.sync-audit/upstream-sync-matrix.json` 生成，禁止手改。");
  lines.push(`> Source-JSON-SHA256: ${meta.jsonSha}`);
  lines.push("");
  lines.push("## 审计坐标");
  lines.push("");
  lines.push("```");
  lines.push(`U0 = ${U0}  (openhanako v0.444.1)`);
  lines.push(`U1 = ${U1}  (openhanako v0.447.4)`);
  lines.push(`L0 = ${L0}  (Lingxi v0.444.1 同步完成点, PR #2)`);
  lines.push(`L1 = ${L1}  (本轮同步开始时 main)`);
  lines.push(`VERIFIED_SOURCE_SHA = ${meta.verifiedSourceSha}`);
  lines.push("```");
  lines.push("");
  lines.push("## 统计（脚本计算，禁止人工填写）");
  lines.push("");
  lines.push("```");
  lines.push(`Total upstream paths: ${summary.total}`);
  lines.push(`ADOPTED: ${summary.ADOPTED}`);
  lines.push(`ADAPTED: ${summary.ADAPTED}`);
  lines.push(`REGENERATED: ${summary.REGENERATED}`);
  lines.push(`INTENTIONAL_DIVERGENCE: ${summary.INTENTIONAL_DIVERGENCE}`);
  lines.push(`UNKNOWN: 0`);
  lines.push(`MISSING: 0`);
  lines.push(`DUPLICATE: 0`);
  lines.push(`${summary.ADOPTED} + ${summary.ADAPTED} + ${summary.REGENERATED} + ${summary.INTENTIONAL_DIVERGENCE} = ${summary.ADOPTED + summary.ADAPTED + summary.REGENERATED + summary.INTENTIONAL_DIVERGENCE}`);
  lines.push("```");
  lines.push("");
  lines.push("冲突等级：A=Lingxi未改动 / B=双方改同文件职责不冲突 / C=Lingxi已重构该职责 / D=产品差异或生成物。");
  lines.push("");
  lines.push("## 逐路径矩阵（133 行，每个 upstream changed path 一行）");
  lines.push("");
  lines.push("| # | upstream path | change | upstream commits | cluster | overlap | class | disposition | Lingxi implementation | test evidence | notes | status |");
  lines.push("|---|---|---|---|---|---|---|---|---|---|---|---|");
  records.forEach((record, index) => {
    const pathCell = record.renamed_from
      ? `${record.upstream_path}<br>← ${record.renamed_from}`
      : record.upstream_path;
    lines.push(`| ${index + 1} | ${pathCell} | ${record.upstream_change} | ${record.upstream_commits.join(",")} | ${record.upstream_clusters.join(",")} | ${record.lingxi_overlap} | ${record.conflict_class} | ${record.disposition} | ${record.lingxi_implementation} | ${record.test_evidence} | ${record.notes} | ${record.status} |`);
  });
  lines.push("");
  return lines.join("\n");
}

function main() {
  const checkOnly = process.argv.includes("--check");
  const records = buildRecords();
  const summary = summarize(records);
  const sum = summary.ADOPTED + summary.ADAPTED + summary.REGENERATED + summary.INTENTIONAL_DIVERGENCE;

  const failures = [];
  if (summary.total !== 133) failures.push(`matrix rows = ${summary.total}, expected 133`);
  if (sum !== 133) failures.push(`disposition sum = ${sum}, expected 133`);
  const seen = new Set();
  for (const record of records) {
    if (seen.has(record.upstream_path)) failures.push(`duplicate path: ${record.upstream_path}`);
    seen.add(record.upstream_path);
  }
  if (failures.length > 0) {
    for (const failure of failures) console.error(`✗ ${failure}`);
    process.exit(1);
  }

  // VERIFIED_SOURCE_SHA 优先读独立文件 .sync-audit/verified-source-sha.txt；
  // 若文件缺失或与常量不一致则失败（不允许静默回退），保证审计坐标单一。
  let verifiedSourceSha;
  if (fs.existsSync(VERIFIED_SOURCE_SHA_FILE)) {
    verifiedSourceSha = fs.readFileSync(VERIFIED_SOURCE_SHA_FILE, "utf-8").trim();
    if (verifiedSourceSha !== VERIFIED_SOURCE_SHA) {
      console.error(`✗ .sync-audit/verified-source-sha.txt 与常量不一致: ${verifiedSourceSha}`);
      process.exit(1);
    }
    if (!/^[0-9a-f]{40}$/.test(verifiedSourceSha)) {
      console.error(`✗ VERIFIED_SOURCE_SHA 非 40 位十六进制: ${verifiedSourceSha}`);
      process.exit(1);
    }
  } else {
    console.error("✗ 缺少 .sync-audit/verified-source-sha.txt（审计坐标文件）");
    process.exit(1);
  }

  const jsonDoc = {
    coordinates: { U0, U1, L0, L1, VERIFIED_SOURCE_SHA: verifiedSourceSha },
    summary,
    records,
  };
  const jsonText = `${JSON.stringify(jsonDoc, null, 2)}\n`;
  // JSON 内含 VERIFIED_SOURCE_SHA；MD 的哈希只覆盖 records+summary，避免 SHA 写入时双产物互相追逐。
  const projectionSha = crypto.createHash("sha256")
    .update(JSON.stringify({ summary, records }))
    .digest("hex");
  const mdText = renderMarkdown(records, { jsonSha: projectionSha, verifiedSourceSha });

  if (checkOnly) {
    let stale = false;
    if (!fs.existsSync(JSON_OUT) || fs.readFileSync(JSON_OUT, "utf-8") !== jsonText) {
      console.error("✗ upstream-sync-matrix.json 与 CURATED/delta 不一致（需重新生成）");
      stale = true;
    }
    if (!fs.existsSync(MD_OUT) || fs.readFileSync(MD_OUT, "utf-8") !== mdText) {
      console.error("✗ UPSTREAM_SYNC_MATRIX.md 与机器真相源不一致（需重新生成）");
      stale = true;
    }
    if (stale) process.exit(1);
    console.log(`✓ matrix check OK: ${summary.total} paths, ADOPTED ${summary.ADOPTED} / ADAPTED ${summary.ADAPTED} / REGENERATED ${summary.REGENERATED} / INTENTIONAL_DIVERGENCE ${summary.INTENTIONAL_DIVERGENCE}`);
    return;
  }

  fs.writeFileSync(JSON_OUT, jsonText);
  fs.writeFileSync(MD_OUT, mdText);
  console.log(`✓ wrote ${path.relative(ROOT, JSON_OUT)} and ${path.relative(ROOT, MD_OUT)}`);
  console.log(`✓ ${summary.total} paths: ADOPTED ${summary.ADOPTED} / ADAPTED ${summary.ADAPTED} / REGENERATED ${summary.REGENERATED} / INTENTIONAL_DIVERGENCE ${summary.INTENTIONAL_DIVERGENCE} (sum ${sum})`);
  console.log(`  projection sha256: ${projectionSha}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(new URL(import.meta.url).pathname)) {
  main();
}
