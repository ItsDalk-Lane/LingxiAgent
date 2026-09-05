/**
 * LingxiEngine — Hanako 的核心引擎（Thin Facade）
 *
 * 持有所有 Manager，对外暴露统一 API。
 * 具体逻辑委托给：
 *   - AgentManager       — agent CRUD / init / switch
 *   - SessionCoordinator — session 生命周期 / listing
 *   - ConfigCoordinator  — 配置读写 / 模型 / 搜索 / utility
 *   - ChannelManager     — 频道 CRUD / 成员管理
 *   - BridgeSessionManager — 外部平台 session
 *   - ModelManager        — 模型注册 / 发现
 *   - PreferencesManager  — 全局偏好
 *   - SkillManager        — 技能注册 / 同步
 */
import fs from "fs";
import { createHash } from "node:crypto";
import os from "os";
import path from "path";
import { runMigrations } from "./migrations.ts";
import { buildFailedPersonaRenameIndex, migrateAgentPersonaFileNames } from "./agents-md-migration.ts";
import { healCredentialFileModes } from "./credential-file-healer.ts";
import { PLUGIN_DATA_DIRNAME } from "./plugin-config.ts";
import { pruneStaleCredentialBackups } from "./credential-backup-retention.ts";
import { createServerRuntimeContext } from "./server-runtime-context.ts";
import { StudioCronService } from "./studio-cron-service.ts";
import { createRuntimeExecutionBoundary } from "./execution-boundary.ts";
import { ResourceAccessService } from "./resource-access-service.ts";
import { ResourceService } from "./resource-service.ts";
import { appendSecurityAuditEvent } from "./security-audit-log.ts";
import { findModel } from "../shared/model-ref.ts";
import { lookupKnown } from "../shared/known-models.ts";
import { isLocalBaseUrl } from "../shared/net-utils.ts";
import {
  resolveWorkspaceSkillCatalogPaths,
  resolveWorkspaceSkillPaths,
  workspaceSkillPolicyFromConfig,
} from "../shared/workspace-skill-paths.ts";
import {
  resolveLingxiPiSdkResourceLoaderAgentDir,
  resolveLingxiPiSdkResourceLoaderCwd,
} from "../shared/hana-runtime-paths.ts";
import { PluginManager } from "./plugin-manager.ts";
import { EnvChangeLedger } from "./env-change-ledger.ts";
import { PluginDevService } from "./plugin-dev-service.ts";
import { createPluginDevTools } from "./plugin-dev-tools.ts";
import { DefaultResourceLoader, SessionManager, SettingsManager } from "../lib/pi-sdk/index.ts";
import { compactSessionWithCachePreservationRecoveringRuntime } from "./session-compactor.ts";
import { resolveRequestReasoningLevelForContext } from "./request-reasoning-level.ts";
import { getFreshCompactNoopReason } from "../lib/fresh-compact/policy.ts";
import { DeferredResultCoordinator } from "../lib/deferred-result-coordinator.ts";
import { LoopAlarmService } from "../lib/loop/alarm-service.ts";
import { LoopController } from "../lib/loop/loop-controller.ts";
import {
  getToolSessionPath,
  normalizeToolRuntimeContext,
  resolveToolSessionRef,
} from "../lib/tools/tool-session.ts";
import { loadLocale } from "../lib/i18n.ts";
import { createApprovalGateway, createModelApprovalReviewer } from "../lib/approval-gateway.ts";
import { runWithNewModelTrace } from "../lib/llm/model-trace-scope.ts";
import { callText } from "./llm-client.ts";
import { SESSION_APPROVAL_POLICIES } from "./session-permission-mode.ts";
import { readCompiledResetAt } from "../lib/memory/compiled-memory-state.ts";

/** 已知的外部 AI 工具技能目录（相对 $HOME） */
export const WELL_KNOWN_SKILL_PATHS = [
  { suffix: ".claude/skills",     label: "Claude Code" },
  { suffix: ".codex/skills",      label: "Codex" },
  { suffix: ".openclaw/skills",   label: "OpenClaw" },
  { suffix: ".pi/agent/skills",   label: "Pi" },
  { suffix: ".agents/skills",     label: "Agents" },
];

function findUniqueModelById(models, id) {
  if (!id || !Array.isArray(models)) return null;
  const matches = models.filter(m => m.id === id);
  return matches.length === 1 ? matches[0] : null;
}

function resolveChannelsEnabledForToolAvailability(engine) {
  try {
    if (
      Object.prototype.hasOwnProperty.call(engine, "isChannelsEnabled")
      && typeof engine.isChannelsEnabled === "function"
    ) {
      return engine.isChannelsEnabled();
    }
    if (typeof engine._configCoord?.getChannelsEnabled === "function") {
      return engine._configCoord.getChannelsEnabled();
    }
    if (typeof engine._prefs?.getChannelsEnabled === "function") {
      return engine._prefs.getChannelsEnabled();
    }
  } catch {
    return undefined;
  }
  return undefined;
}

import { PreferencesManager } from "./preferences-manager.ts";
import { InputDraftsStore } from "./input-drafts-store.ts";
import { ModelManager } from "./model-manager.ts";
import { AuxiliaryModelResolver } from "./auxiliary-model-resolver.ts";
import { ModelOperationConfigurationError, ModelOperationResolver } from "./model-operation-resolver.ts";
import { EmbeddingClient, RerankClient } from "./model-operation-client.ts";
import type { ModelOperation } from "../shared/model-operations.ts";
import { isAuxiliaryConfigError } from "./auxiliary-slots.ts";
import { SessionProjectCatalogStore } from "./session-project-catalog-store.ts";
import { SkillManager } from "./skill-manager.ts";
import { BridgeSessionManager } from "./bridge-session-manager.ts";
import { createSlashSystem } from "./slash-commands/index.ts";
import { AgentManager } from "./agent-manager.ts";
import { sanitizeMessagesForModel, stripHistoricalInlineMediaForReplay } from "./message-sanitizer.ts";
import { normalizeProviderContextMessages, normalizeProviderPayload } from "./provider-compat.ts";
import { currentProviderCompatPurpose, runWithProviderCompatPurpose } from "./provider-compat/purpose-scope.ts";
import { VisionBridge } from "./vision-bridge.ts";
import { SessionCoordinator } from "./session-coordinator.ts";
import { SessionManifestResolver } from "./session-manifest/resolver.ts";
import { SessionManifestStore } from "./session-manifest/store.ts";
import { ensureSessionRefForPath as establishSessionRefForPath } from "./session-manifest/ref.ts";
import {
  moveSessionManifestDbFilesAside,
  sanitizeSessionManifestFileSuffix,
} from "./session-manifest/db-files.ts";
import { ConfigCoordinator, SHARED_MODEL_KEYS } from "./config-coordinator.ts";
import { ChannelManager } from "./channel-manager.ts";
import {
  summarizeTitle as _summarizeTitle,
  translateSkillNames as _translateSkillNames,
  summarizeActivity as _summarizeActivity,
  summarizeActivityQuick as _summarizeActivityQuick,
} from "./llm-utils.ts";
import { debugLog, createModuleLogger } from "../lib/debug-log.ts";
import { createSandboxedTools } from "../lib/sandbox/index.ts";
import { createSandboxResourceIO } from "../lib/resource-io/sandbox-resource-io.ts";
import { ResourceEventBus } from "../lib/resource-io/resource-event-bus.ts";
import { resourceKeyForRef } from "../lib/resource-io/resource-refs.ts";
import { ResourceWatchRegistry } from "../lib/resource-io/resource-watch-registry.ts";
import { FileHistoryService } from "../lib/file-history/file-history-service.ts";
import { externalReadPathsFromSessionFiles } from "../lib/sandbox/win32-policy.ts";
import { t } from "../lib/i18n.ts";
import { CheckpointStore } from "../lib/checkpoint-store.ts";
import {
  assertAllBuiltInToolsPermissionCovered,
  assertAllToolsCategorized,
} from "../shared/tool-categories.ts";
import { workspaceRootsForSandbox } from "../shared/workspace-scope.ts";
import { wrapWithCheckpoint } from "../lib/checkpoint-wrapper.ts";
import { wrapWithSessionPermission } from "../lib/tools/session-permission-wrapper.ts";
import { createToolCatalog } from "./tool-catalog.ts";
import { hashCacheContractValue } from "../lib/llm/cache-prefix-contract.ts";
import { resolveReferenceBudgetTokens } from "./session-reminders.ts";
import { createBridgeTools, registerBridgeCapabilityDelegates } from "./tool-catalog-bridge.ts";
import { summarizeToolParameters } from "./mcp/manager.ts";
import { ToolTargetRegistry } from "./tool-target-registry.ts";
import { ToolInvocationGateway } from "./tool-invocation-gateway.ts";
import {
  createToolSchemaValidator,
} from "../lib/tools/invocation/index.ts";

/** Matches the MCP config default; used when no manager config is available. */
const DEFAULT_TOOL_DEFER_THRESHOLD = 10;

/**
 * Snapshots the catalog listing for the session that is being built.
 *
 * The fingerprint covers the catalog's tool names, so a later refresh that adds
 * or drops a tool produces a different value and the owning session can notice
 * it has been holding a stale listing. It deliberately ignores descriptions and
 * schemas: a reworded description is not news worth interrupting a session for.
 */
function buildToolCatalogManifestSnapshot(catalog, modelContextWindowTokens) {
  // Only MCP-origin names are fingerprinted. Those are the ones a connector
  // refresh can change underneath a running session; builtin rows move only
  // when the application itself changes, and including them here would make a
  // builtin-defer session look like it had lost every tool.
  const names = catalog.all()
    .filter((entry) => entry.origin === "mcp")
    .map((entry) => entry.name)
    .sort((left, right) => left.localeCompare(right));
  const budgetTokens = resolveReferenceBudgetTokens(modelContextWindowTokens);
  const { tier, text } = catalog.manifest(budgetTokens);
  return {
    text,
    tier,
    // Carried so the session renders against the budget the tier was chosen
    // for, rather than re-deriving it from whatever model state is reachable
    // at render time.
    budgetTokens,
    fingerprint: hashCacheContractValue(names),
    names,
  };
}
import {
  evaluateToolAvailability,
  filterToolObjectsByAvailability,
} from "./tool-availability.ts";
import { TaskRegistry } from "../lib/task-registry.ts";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { KnowledgeResearchOrchestrator } from "../lib/knowledge/research/knowledge-research-orchestrator.ts";
import { ResearchStore } from "../lib/knowledge/research/research-store.ts";
import type { KnowledgeEvidenceSpan } from "../shared/knowledge-evidence.ts";
import type { CompiledKnowledgeScope } from "../lib/knowledge/scope-snapshot-compiler.ts";
import { KNOWLEDGE_CANDIDATE_GENERATION_BUDGET } from "../lib/knowledge/knowledge-query-service.ts";
import { KnowledgeError } from "../lib/knowledge/errors.ts";
import { KnowledgeEmbeddingProviderGate } from "../lib/knowledge/ingestion-service.ts";
import {
  factEmbeddingModelKey,
  serializeVector,
} from "../lib/memory/fact-embeddings.ts";
import type { KnowledgeRetrievalStats } from "../shared/knowledge-refs.ts";
import { resolveKnowledgeExecutionPolicy } from "../shared/knowledge-execution.ts";
import {
  assembleKnowledgeEvidenceManifestEntries,
  type KnowledgeInjectionEvidence,
} from "../lib/knowledge/knowledge-context-injector.ts";
import {
  KNOWLEDGE_COVERAGE_CLASSIFY_SYSTEM_PROMPT,
  planKnowledgeCoverage,
} from "../lib/knowledge/knowledge-coverage-planner.ts";
import type { KnowledgeRollupModel } from "../lib/knowledge/knowledge-rollup.ts";
import { estimateTextTokens } from "../lib/llm/estimate-text-tokens.ts";
import { BrowserManager } from "../lib/browser/browser-manager.ts";
import { TerminalSessionManager } from "../lib/terminal/terminal-session-manager.ts";
import {
  SessionExecutionRegistry,
  wrapWithSessionExecutionCancellation,
} from "../lib/session-execution-registry.ts";
import { PluginInstallRecords } from "../lib/plugin-install-records.ts";
import { ComputerHost } from "./computer-use/computer-host.ts";
import { ComputerProviderRegistry } from "./computer-use/provider-registry.ts";
import { createMockComputerProvider } from "./computer-use/providers/mock-provider.ts";
import { createMacosCuaProvider } from "./computer-use/providers/macos-cua-provider.ts";
import { createWindowsUiaProvider } from "./computer-use/providers/windows-uia-provider.ts";
import {
  effectiveComputerUseSettings,
  isComputerUsePlatformSupported,
} from "./computer-use/platform-support.ts";
import { SessionFileRegistry } from "../lib/session-files/session-file-registry.ts";
import { serializeSessionFile } from "../lib/session-files/session-file-response.ts";
import { AutomationSuggestionStore } from "../lib/tools/automation-suggestion-store.ts";
import { SessionCollabDraftStore } from "../lib/session-collab/draft-store.ts";
import { NotificationService } from "../lib/notifications/notification-service.ts";
import { SpeechRecognitionService } from "./speech-recognition-service.ts";
import { UniversalMediaManager } from "./media/universal-media-manager.ts";
import { McpManager } from "./mcp/manager.ts";
import { createCurrentTurnNativeMediaStore } from "./current-turn-native-media.ts";
import {
  getSkillNameTranslationCachePath,
  translateSkillNamesWithCache,
} from "../lib/skills/skill-name-translation-cache.ts";
import { createUsageLedger } from "../lib/llm/usage-ledger.ts";
import { createModelObservabilityGenerationManager } from "../lib/llm/model-observability-engine.ts";
import { createModelObservabilityQueryService } from "../lib/llm/model-observability-query.ts";
import {
  DEFAULT_MODEL_OBSERVABILITY_PREFERENCE,
  modelObservabilityPreferenceToPolicy,
} from "../lib/llm/model-observability-preferences.ts";
import type {
  ModelObservabilitySettingsResponse,
  ModelObservabilitySettingsUpdateRequest,
  ModelObservabilitySettingsUpdateResponse,
  ModelObservabilityHealthResponse,
} from "../shared/model-observability-api-contract.ts";
import {
  autoProjectIdForCwd,
  isAutoProjectId,
  normalizeSessionProjectId,
  UNCATEGORIZED_PROJECT_ID,
} from "../shared/session-projects.ts";
import { assertValidAgentId, isValidAgentId } from "../shared/agent-id.ts";

const moduleLog = createModuleLogger("engine");
const mcpLog = createModuleLogger("mcp");
const toolAvailabilityLog = createModuleLogger("tool-availability");

/** 滚动注入中间轮的兜底超时（ms）：纯消化轮不无限挂；超时=该轮失败（模块内重试一次）。 */
const KNOWLEDGE_ROLLUP_ROUND_TIMEOUT_MS = 240_000;
/** 近期对话摘录的字符上限（滚动中间轮防指代丢失；只取尾部纯文本）。 */
const KNOWLEDGE_ROLLUP_EXCERPT_MAX_CHARS = 4_000;

/**
 * 近期对话摘录（滚动注入中间轮）：取尾部 user/assistant 消息的纯文本，渲染成
 * "user: ..." / "assistant: ..." 行；从尾部截断到 maxChars（保序）。消息形状
 * 防御性读取（agent.state.messages 的 AgentMessage），任何缺失返回 null。
 */
function buildRecentTurnsExcerpt(messages: unknown, maxChars: number): string | null {
  if (!Array.isArray(messages)) return null;
  const lines: string[] = [];
  for (let index = messages.length - 1; index >= 0 && lines.join("\n").length < maxChars; index -= 1) {
    const message = messages[index] as { role?: unknown; content?: unknown } | null;
    if (!message || typeof message !== "object") continue;
    const role = message.role === "user" ? "user" : message.role === "assistant" ? "assistant" : null;
    if (!role) continue;
    const text = Array.isArray(message.content)
      ? (message.content as Array<{ type?: unknown; text?: unknown }>)
        .filter(block => block?.type === "text" && typeof block.text === "string")
        .map(block => block.text as string)
        .join("\n")
        .trim()
      : typeof message.content === "string"
        ? message.content.trim()
        : "";
    if (!text) continue;
    lines.unshift(`${role}: ${text}`);
  }
  const excerpt = lines.join("\n").trim();
  if (!excerpt) return null;
  return excerpt.length > maxChars ? excerpt.slice(excerpt.length - maxChars) : excerpt;
}

export function runBestEffortStartupMigrationStep(label, operation, log: any = () => {}) {
  try {
    return { ok: true, value: operation() };
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    moduleLog.error(`startup migration ${label} failed: ${message}`);
    log(`[migrations] ${label} 失败，应用继续启动；该步骤将在下次启动重试`);
    return { ok: false, error };
  }
}

// getSessionMetadataRecoveryStatus() 聚合三源 meta 恢复信号供 /api/health 消费。
// detail 只保留"所属 agent 目录名 / 文件 basename"两段，绝不透传绝对路径或
// 底层 error.message（两者都可能带 lingxiHome 绝对路径前缀）——/api/health 可能
// 被远程前端（mobile PWA / Bridge）读取，隐私边界比调试可读性优先。
function describeMetaSourcePath(filePath: any) {
  const base = path.basename(String(filePath || ""));
  const agentDirName = path.basename(path.dirname(path.dirname(String(filePath || ""))));
  return agentDirName ? `${agentDirName}/${base}` : base;
}

function sessionBelongsToProject(projectId) {
  return (session) => {
    const explicitProjectId = normalizeSessionProjectId(session?.projectId);
    if (explicitProjectId) return explicitProjectId === projectId;
    if (!isAutoProjectId(projectId)) return false;
    return autoProjectIdForCwd(session?.cwd || null) === projectId;
  };
}

export class LingxiEngine {
  declare _activityHub: any;
  declare _agentMgr: any;
  declare _approvalGateway: any;
  declare _automationSuggestionStore: any;
  declare _sessionCollabDraftStore: any;
  declare _bridge: any;
  declare _channels: any;
  declare _checkpointStore: any;
  declare _fileHistory: any;
  declare _computerHost: any;
  /** 蒸馏模型实测吞吐（ms/token EMA），按 provider/model 分键；驱动批预算动态推算。 */
  declare _computerProviders: any;
  declare _configCoord: any;
  declare _auxResolver: AuxiliaryModelResolver;
  declare _modelOperationResolver: ModelOperationResolver;
  declare _rerankClient: RerankClient;
  /** 记忆事实向量回填：每 agent 单飞标记 + 共享供应商限流闸 */
  declare _memoryBackfillInFlight: Set<string>;
  declare _memoryEmbeddingGate: KnowledgeEmbeddingProviderGate;
  declare _confirmStore: any;
  declare _coreExtensionFactories: any;
  declare _currentTurnNativeMedia: any;
  declare _deferredResultCoordinator: any;
  declare _deferredResultStore: any;
  declare _devLogs: any;
  declare _devLogsMax: any;
  declare _discoveredExternalPaths: any;
  declare _eventBus: any;
  declare _envChangeLedger: EnvChangeLedger;
  declare _extensionFactories: any;
  declare _frameworkExtFactories: any;
  declare _hubCallbacks: any;
  declare _imageStripNotified: any;
  declare _listeners: any;
  declare _loopStore: any;
  declare _loopAlarm: any;
  declare _loopController: any;
  declare _loopBridgeHooks: any;
  declare _knowledge: KnowledgeManager;
  declare _media: any;
  declare _mcp: any;
  declare _models: any;
  declare _notifications: any;
  declare _outboundProxyRuntime: any;
  declare _pluginDevEventBusCleanup: any;
  declare _pluginDevService: any;
  declare _pluginInstallRecords: any;
  declare _pluginManager: any;
  declare _prefs: any;
  declare _resourceAccess: any;
  declare _resourceEventBus: any;
  declare _resourceLoader: any;
  declare _resourceIO: any;
  declare _resourceWatchRegistry: any;
  declare _resources: any;
  declare _runtimeContext: any;
  /** 启动迁移 agents-md-rename 的失败索引：agentId → (新文件名 → 旧文件名)。 */
  declare _failedPersonaRenames: Map<string, Map<string, string>>;
  declare _sessionCoord: any;
  declare _sessionFiles: any;
  declare _sessionExecutions: any;
  declare _sessionManifestResolver: any;
  declare _sessionManifestStore: any;
  declare _sessionManifestStoreRecovery: any;
  declare _modelObservability: any;
  declare _modelObservabilityGenerations: any;
  /** Phase 8：稳定 query facade（read-only 连接随 reconfigure invalidate/reopen）。 */
  declare _modelObservabilityQuery: any;
  declare _sessionProjects: any;
  declare _skills: any;
  declare _slashSystem: any;
  declare _speechRecognition: any;
  declare _studioCronService: any;
  declare _subagentControllers: any;
  declare _subagentRunStore: any;
  declare _subagentThreadStore: any;
  declare _taskRegistry: any;
  declare _terminalSessions: any;
  declare _uiContextBySession: any;
  declare _usageLedger: any;
  declare _videoStripNotified: any;
  declare _visionBridge: any;
  declare agentsDir: any;
  declare appVersion: any;
  declare channelsDir: any;
  declare lingxiHome: any;
  declare _inputDrafts: any;
  declare productDir: any;
  declare userDir: any;
  /**
   * @param {object} dirs
   * @param {string} dirs.lingxiHome
   * @param {string} dirs.productDir
   * @param {string} [dirs.agentId]
   * @param {string} [dirs.appVersion]
   * @param {any[]} [dirs.builtinMediaAdapters] Closed-content media adapter
   *   implementations (core/media-adapters/), supplied by the composition
   *   root. Absent/empty means an open composition: the media runtime
   *   constructs with zero built-in adapters, never an implicit import.
   * @param {object} [dirs.modelObservability] Durable Model Observatory
   *   persistence policy (Phase 7). Absent or enabled !== true keeps
   *   persistence disabled — production behavior is identical to Phase 6.
   *   Storage open/migration failure disables observability persistence and
   *   never fails engine startup (MODEL_OBSERVABILITY_STORAGE_AUDIT.md Q9).
   */
  constructor({ lingxiHome, productDir, agentId, appVersion, builtinMediaAdapters, modelObservability }) {
    this.lingxiHome = lingxiHome;
    this.productDir = productDir;
    this.appVersion = appVersion || "0.0.0";
    this._runtimeContext = null;
    this._resources = null;
    this._resourceAccess = null;
    this._resourceIO = null;
    this._resourceEventBus = null;
    // 启动级 migration-degraded 状态：agentId → (新文件名 → 旧文件名)。
    // 仅记录本次启动 agents-md-rename 明确失败的条目；改名成功后（下次启动）
    // 该条目自然消失，旧文件读取不是永久协议。
    this._failedPersonaRenames = new Map();
    this.agentsDir = path.join(lingxiHome, "agents");
    this.userDir = path.join(lingxiHome, "user");
    this.channelsDir = path.join(lingxiHome, "channels");
    fs.mkdirSync(this.channelsDir, { recursive: true });
    this._studioCronService = new StudioCronService({
      lingxiHome: this.lingxiHome,
      agentsDir: this.agentsDir,
      getStudioId: () => {
        const studioId = this._runtimeContext?.studioId;
        if (!studioId) throw new Error("runtime studioId unavailable");
        return studioId;
      },
    });
    this._sessionFiles = new SessionFileRegistry({
      managedCacheRoot: path.join(lingxiHome, "session-files"),
      getSessionIdForPath: (sessionPath) => this.getSessionIdForPath(sessionPath),
    });
    this._resourceWatchRegistry = new ResourceWatchRegistry({
      eventBus: this._resourceEvents(),
      resolveWatchTarget: (resource) => this.getResourceIO().resolveWatchTarget(resource),
    });
    this._fileHistory = new FileHistoryService({
      historyRoot: path.join(lingxiHome, "file-history"),
      log: (message) => console.warn(`[file-history] ${message}`),
    });
    this._sessionManifestStoreRecovery = null;
    this._sessionManifestStore = this._openSessionManifestStore();
    this._sessionManifestResolver = this._sessionManifestStore
      ? new SessionManifestResolver({ store: this._sessionManifestStore })
      : null;
    // ── Phase 7/8：Durable Model Observatory persistence。安装点在
    // PreferencesManager 之后（audit Q10/补充决策 1：单一 parser，无第四套
    // default），仍早于任何模型调用。默认 disabled（preference 未开启且无
    // 显式 option）；打开失败 → disabled handle，engine 正常继续。──
    this._modelObservability = null;
    this._modelObservabilityGenerations = createModelObservabilityGenerationManager({
      lingxiHome: this.lingxiHome,
      drainTimeoutMs: 5000,
    });
    this._modelObservabilityQuery = null;
    this._currentTurnNativeMedia = createCurrentTurnNativeMediaStore();
    this._pluginInstallRecords = new PluginInstallRecords({ lingxiHome });
    this._automationSuggestionStore = new AutomationSuggestionStore();
    this._sessionCollabDraftStore = new SessionCollabDraftStore();
    // §四十二/§四十三/§四十四 Approval gateway uses the semantic "approval" slot.
    // approval slot has NO fallback: unconfigured or misconfigured → reviewer
    // unavailable → fail-closed (ask_user). Never silently falls back to chat.
    this._approvalGateway = createApprovalGateway({
      intentAuthorizationReviewer: createModelApprovalReviewer({
        resolveApprovalModel: (options) => this.resolveAuxiliaryModelFresh("approval", options || {}),
        callText: (options) => this._callApprovalReviewerText(options),
        getUsageLedger: () => this._usageLedger,
      }),
    });

    // Process-local append-only environment ledger. This is created before
    // every producer/consumer and passed by dependency injection.
    this._envChangeLedger = new EnvChangeLedger();

    // ── Core managers ──
    this._prefs = new PreferencesManager({ userDir: this.userDir, agentsDir: this.agentsDir });
    // _models 必须先于 _knowledge 构造：KnowledgeManager 构造期会同步跑 knowledge.db
    // 迁移（chunk profile 回填），经 getEmbeddingModelContextWindow 闭包读
    // providerRegistry；晚赋值会在存量库迁移时读到 undefined 直接崩。
    this._models = new ModelManager({ lingxiHome });
    this._knowledge = new KnowledgeManager({
      lingxiHome: this.lingxiHome,
      rerank: (request) => this._rerankKnowledgeTexts(request),
      // 查询侧 rerank 按笔记本解析链的显式引用路由（v8：全局 rerank 槽已退役，
      // 引用不可解析 → null → 检索显式降级 RRF 名次，见 _rerankKnowledgeTextsForModel）。
      rerankForModel: (request) => this._rerankKnowledgeTextsForModel(request),
      // 摄入管线按笔记本解析链拿到的显式引用执行嵌入；
      // 与查询侧懒构建共用同一套 ModelOperationResolver/EmbeddingClient 基础设施。
      embedTextsForModel: (request) => this._embedKnowledgeTextsForModel(request),
      canEmbedWithModel: (ref) => this._canResolveKnowledgeEmbeddingRef(ref),
      // 修订只在内存中作为缓存身份使用；凭证原文不进入缓存键、日志或持久化。
      getModelConfigurationRevision: (ref) => createHash("sha256").update(JSON.stringify({
        embedding: this._models.providerRegistry.getOperationModel("embedding", ref),
        rerank: this._models.providerRegistry.getOperationModel("rerank", ref),
        credentials: this._models.providerRegistry.getCredentials(ref.provider),
      })).digest("hex"),
      // 嵌入模型上下文窗口（自动分块 ×80% 口径）：目录条目归一化的
      // contextWindow/context 优先，known-models 静态目录兜底。
      getEmbeddingModelContextWindow: (ref) => {
        const entry = this._models.providerRegistry.getOperationModel("embedding", ref);
        for (const candidate of [entry?.contextWindow, entry?.context]) {
          if (typeof candidate === "number" && Number.isFinite(candidate) && candidate > 0) return candidate;
        }
        return lookupKnown(ref?.provider, ref?.id)?.context ?? null;
      },
      ingestionLog: (message) => moduleLog.log(message),
    });
    this._installModelObservability(modelObservability);
    this._inputDrafts = new InputDraftsStore({ lingxiHome: this.lingxiHome });
    this._speechRecognition = new SpeechRecognitionService({
      providerRegistry: this._models.providerRegistry,
      resolveProviderCredentialsFresh: (providerId) => this._models.resolveProviderCredentialsFresh(providerId),
      preferences: this._prefs,
      sessionFiles: this._sessionFiles,
      emitEvent: (event, sessionPath) => this._emitEvent(event, sessionPath),
      getUsageLedger: () => this._usageLedger,
    });
    this._media = new UniversalMediaManager({
      lingxiHome: this.lingxiHome,
      providerRegistry: this._models.providerRegistry,
      preferences: this._prefs,
      speechRecognition: this._speechRecognition,
      sessionFiles: this._sessionFiles,
      registerSessionFile: (entry) => this.serializeSessionFile(this.registerSessionFile(entry)),
      onProviderChanged: () => this.onProviderChanged(),
      builtinAdapters: builtinMediaAdapters,
      getUsageLedger: () => this._usageLedger,
    });
    // The data directory keeps the historical `plugin-data/mcp` location: it is
    // where existing installs already store their connector config.
    this._mcp = new McpManager({
      dataDir: path.join(this.lingxiHome, PLUGIN_DATA_DIRNAME, "mcp"),
      log: mcpLog,
    }, {
      // A connector tool may come back asking the user a question. The store is
      // read lazily because it is installed after this manager is built.
      getConfirmStore: () => this._confirmStore,
      emitEvent: (event, sessionPath) => this._emitEvent(event, sessionPath),
    });
    this._sessionProjects = new SessionProjectCatalogStore({ userDir: this.userDir });

    // 确定启动时焦点 agent
    let startId;
    if (agentId !== undefined && agentId !== null) {
      // Explicit constructor input is an external contract: reject it rather
      // than silently switching the caller to another identity.
      assertValidAgentId(agentId);
      startId = agentId;
    } else {
      const primaryAgentId = this._prefs.getPrimaryAgent();
      const primaryConfig = isValidAgentId(primaryAgentId)
        ? path.join(this.agentsDir, primaryAgentId, "config.yaml")
        : null;
      if (primaryConfig && fs.existsSync(primaryConfig)) {
        startId = primaryAgentId;
      } else {
        if (primaryAgentId) {
          moduleLog.warn(
            `ignoring unavailable primary agent ID ${JSON.stringify(primaryAgentId)}; `
            + "the saved preference was preserved",
          );
        }
        startId = this._prefs.findFirstAgent();
      }
    }
    if (!startId) throw new Error(t("error.noAgentsFound"));

    // ── Channel Manager ──
    this._channels = new ChannelManager({
      channelsDir: this.channelsDir,
      agentsDir: this.agentsDir,
      userDir: this.userDir,
      getHub: () => this._hubCallbacks,
    });

    // ── Agent Manager ──
    this._agentMgr = new AgentManager({
      lingxiHome: this.lingxiHome,
      agentsDir: this.agentsDir,
      productDir: this.productDir,
      userDir: this.userDir,
      channelsDir: this.channelsDir,
      getPrefs: () => this._prefs,
      getModels: () => this._models,
      getHub: () => this._hubCallbacks,
      getSkills: () => this._skills,
      getSearchConfig: () => this.getSearchConfig(),
      resolveAuxiliaryModelFresh: (slot, options) => this.resolveAuxiliaryModelFresh(slot, options),
      getSharedModels: () => this._configCoord.getSharedModels(),
      getChannelManager: () => this._channels,
      getSessionCoordinator: () => this._sessionCoord,
      getEngine: () => this,
      getResourceLoader: () => this._resourceLoader,
    });

    this._sessionExecutions = new SessionExecutionRegistry();

    // ── Session Coordinator ──
    this._sessionCoord = new SessionCoordinator({
      agentsDir: this.agentsDir,
      getAgent: () => this.agent,
      getActiveAgentId: () => this.currentAgentId,
      getModels: () => this._models,
      getResourceLoader: () => this._resourceLoader,
      getSkills: () => this._skills,
      buildTools: (cwd, ct, opts) => this.buildTools(cwd, ct, opts),
      getLiveToolCatalogNames: () => this.getLiveToolCatalogNames(),
      emitEvent: (e, sp) => this._emitEvent(e, sp),
      emitDevLog: (t, l) => this.emitDevLog(t, l),
      getHomeCwd: (agentId) => this.getHomeCwd(agentId),
      agentIdFromSessionPath: (p) => this.agentIdFromSessionPath(p),
      switchAgentOnly: (id) => this._agentMgr.switchAgentOnly(id),
      getConfig: () => this.config,
      getPrefs: () => this._prefs,
      getAgents: () => this._agentMgr.agents,
      getActivityStore: (id) => this.getActivityStore(id),
      getAgentById: (id) => this._agentMgr.getAgent(id),
      ensureAgentRuntime: (id, opts) => this.ensureAgentRuntime(id, opts),
      listAgents: () => this.listAgents(),
      listDeletedAgents: () => this.listDeletedAgents(),
      isAgentDeleted: (id) => this.isAgentDeleted(id),
      getDeletedAgentInfo: (id) => this.getDeletedAgentInfo(id),
      getConfirmStore: () => this._confirmStore,
      getDeferredResultStore: () => this._deferredResultStore,
      getTaskRegistry: () => this._taskRegistry,
      getSubagentRunStore: () => this._subagentRunStore,
      getSubagentThreadStore: () => this._subagentThreadStore,
      getActivityHub: () => this._activityHub,
      forkSessionDeferredTasks: (options) => this.forkSessionDeferredTasks(options),
      discardForkedSessionDeferredTasks: (options) => this.discardForkedSessionDeferredTasks(options),
      getSessionIdForPath: (sessionPath) => this.getSessionIdForPath(sessionPath),
      forkSessionFiles: (options) => this.forkSessionFiles(options),
      discardForkedSessionFiles: (options) => this.discardForkedSessionFiles(options),
      forkSessionVisionNotes: (options) => this.forkSessionVisionNotes(options),
      discardForkedSessionVisionNotes: (options) => this.discardForkedSessionVisionNotes(options),
      forkSessionPluginConfig: (options) => this.forkSessionPluginConfig(options),
      discardForkedSessionPluginConfig: (options) => this.discardForkedSessionPluginConfig(options),
      forkSessionBrowserState: (options) => this.forkSessionBrowserState(options),
      discardForkedSessionBrowserState: (options) => this.discardForkedSessionBrowserState(options),
      forkSessionMediaTasks: (options) => this.forkSessionMediaTasks(options),
      discardForkedSessionMediaTasks: (options) => this.discardForkedSessionMediaTasks(options),
      forkSessionCollabDrafts: (options) => this._sessionCollabDraftStore.forkSessionDrafts(options),
      discardForkedSessionCollabDrafts: (options) => this._sessionCollabDraftStore.discardForkedSessionDrafts(options),
      initializeSessionMemoryForkBaseline: (options) => {
        const agent = this._agentMgr.getAgent(options?.agentId);
        if (typeof agent?.summaryManager?.initializeForkBaseline !== "function") {
          throw new Error("session memory fork baseline support is unavailable");
        }
        return agent.summaryManager.initializeForkBaseline(options.sessionId, options);
      },
      notifySessionMemoryForkCreated: (options) => {
        const agent = this._agentMgr.getAgent(options?.agentId);
        if (typeof agent?.memoryTicker?.notifyForkCreated !== "function") {
          moduleLog.warn(`session fork memory materialization unavailable for ${options?.sessionId || "unknown"}`);
          return;
        }
        void Promise.resolve()
          .then(() => agent.memoryTicker.notifyForkCreated(options.sessionPath))
          .catch((error) => {
            moduleLog.warn(
              `session fork memory materialization failed for ${options?.sessionId || "unknown"}: ${error?.message || error}`,
            );
          });
      },
      discardSessionMemoryForkBaseline: (options) => {
        const agent = this._agentMgr.getAgent(options?.agentId);
        if (typeof agent?.summaryManager?.invalidateSession !== "function") {
          throw new Error("session memory fork baseline cleanup is unavailable");
        }
        return agent.summaryManager.invalidateSession(options.sessionId);
      },
      abortToolExecutionsForSession: (sessionRef, reason) => (
        this._sessionExecutions.abortBySession(sessionRef, reason)
      ),
      getEngine: () => this,
      getUsageLedger: () => this._usageLedger,
      sessionManifestStore: this._sessionManifestStore,
      closeTerminalsForSession: (sessionPath) => this._terminalSessions.closeForSession(sessionPath),
      closeAllTerminals: () => this._terminalSessions.closeAll(),
      onSessionRuntimeDiscarded: (sessionPath, reason) => this.clearSessionRuntimeState(sessionPath, reason),
      onBeforeSessionCreate: async (cwd, { agent = null, agentId = null } = {}) => {
        const targetAgent = agent || (agentId ? this._agentMgr.getAgent(agentId) : null) || this.agent;
        await this.syncWorkspaceSkillPaths(cwd, {
          reload: true,
          emitEvent: false,
          agent: targetAgent,
          agentId: targetAgent?.id || agentId || null,
        });
        return {
          workspacePaths: this._getWorkspaceExternalSkillPaths(cwd),
          policy: workspaceSkillPolicyFromConfig(targetAgent?.config?.workspace_context),
        };
      },
      envChangeLedger: this._envChangeLedger,
    });

    // ── Config Coordinator ──
    this._configCoord = new ConfigCoordinator({
      lingxiHome,
      agentsDir: this.agentsDir,
      getAgent: () => this.agent,
      getAgentById: (id) => this._agentMgr.getAgent(id),
      getActiveAgentId: () => this._agentMgr.activeAgentId,
      getAgents: () => this._agentMgr.agents,
      getModels: () => this._models,
      getPrefs: () => this._prefs,
      getSkills: () => this._skills,
      getSessionCoordinator: () => this._sessionCoord,
      getHub: () => this._hubCallbacks,
      emitEvent: (e, sp) => this._emitEvent(e, sp),
      emitDevLog: (t, l) => this.emitDevLog(t, l),
      getCurrentModel: () => this.currentModel?.name,
    });

    // ── Auxiliary Model Resolver ──
    // 统一的语义 Slot 解析器：业务层只认识 title/summarize/memory/vision/
    // knowledge/approval/guard，不再关心 utility/utility_large。Slot 不拥有 credential；
    // Provider credential 基础设施是唯一执行凭证来源。
    this._auxResolver = new AuxiliaryModelResolver({
      resolveModel: (ref) => this._models._resolveFromAvailable(ref),
      getChatModel: (agentId) => {
        const ag = (agentId && this._agentMgr.getAgent(agentId)) || this.agent;
        return this._models.resolveExecutionModel(ag?.config?.models?.chat || null);
      },
      getSlotModelRef: (slot) => this._configCoord.getSharedModels()?.[slot] || null,
      resolveProviderCredentialsFresh: (provider) =>
        this._models.resolveProviderCredentialsFresh(provider),
      getProviderCredentials: (provider) =>
        this._models.providerRegistry.getCredentials(provider),
      allowsMissingApiKey: (provider, baseUrl) =>
        this._models.providerRegistry?.allowsMissingApiKey?.(provider, baseUrl) ??
        isLocalBaseUrl(baseUrl),
    });
    this._modelOperationResolver = new ModelOperationResolver(
      this._modelOperationResolverDeps((operation) =>
        this._configCoord.getSharedModels()?.[operation] || null),
    );

    this._visionBridge = new VisionBridge({
      resolveVisionConfig: () => this.resolveVisionConfigFresh(),
      getUsageLedger: () => this._usageLedger,
      getActiveAgentId: () => this._agentMgr.activeAgentId,
      getSessionIdForPath: (sessionPath) => this.getSessionIdForPath(sessionPath),
    });

    // ── Bridge Session Manager ──
    this._bridge = new BridgeSessionManager({
      getAgent: () => this.agent,
      getAgentById: (id) => this._agentMgr.getAgent(id),
      getAgents: () => this._agentMgr.agents,
      getModelManager: () => this._models,
      getResourceLoader: () => this._resourceLoader,
      getSkills: () => this._skills,
      getPreferences: () => this._readPreferences(),
      buildTools: (cwd, customTools, opts) => this.buildTools(cwd, customTools, opts),
      getHomeCwd: (agentId) => this.getHomeCwd(agentId),
      getVisionBridge: () => this._visionBridge,
      isVisionAuxiliaryEnabled: () => this.isVisionAuxiliaryEnabled(),
      getLingxiHome: () => this.lingxiHome,
      registerSessionFile: (entry) => this.registerSessionFile(entry),
      getSessionFile: (fileId, options) => this.getSessionFile(fileId, options),
      getSessionFileByPath: (filePath, options) => this.getSessionFileByPath(filePath, options),
      getSessionIdForPath: (sessionPath) => this.getSessionIdForPath(sessionPath),
      ensureSessionRefForPath: (sessionPath, defaults) => this.ensureSessionRefForPath(sessionPath, defaults),
      applySessionBranchHead: (sessionPath, manager, defaults) => (
        this._sessionCoord.applySessionBranchHead(sessionPath, manager, defaults)
      ),
      syncSessionBranchHead: (sessionPath, manager, reason) => (
        this._sessionCoord._syncSessionBranchHeadQuiet(sessionPath, manager, reason)
      ),
      beginCurrentTurnNativeMedia: (sessionPath, opts) => this.beginCurrentTurnNativeMedia(sessionPath, opts),
      endCurrentTurnNativeMedia: (token) => this.endCurrentTurnNativeMedia(token),
      emitEvent: (event, sessionPath) => this._emitEvent(event, sessionPath),
      ensureAgentRuntime: (id, opts) => this.ensureAgentRuntime(id, opts),
      getUsageLedger: () => this._usageLedger,
    });
    this._notifications = new NotificationService({
      emitDesktop: ({ title, body, agentId, desktopFocusPolicy, sessionPath }) => {
        this._hubCallbacks?.eventBus?.emit({
          type: "notification",
          title,
          body,
          agentId: agentId || null,
          desktopFocusPolicy,
          ...(sessionPath ? { sessionPath } : {}),
        }, sessionPath || null);
      },
      getBridgeManager: () => this._hubCallbacks?.hub?.bridgeManager || null,
    });

    // ── Slash Command System ──
    // hub 尚未注入，dispatcher 的 hub 字段先为 null；setHubCallbacks 时通过 setHub() 补齐
    this._slashSystem = createSlashSystem({ engine: this, hub: null });

    // 任务注册表（外部 abort 用）；handler 是运行时函数，任务元数据持久化供插件重启恢复和诊断使用。
    this._taskRegistry = new TaskRegistry({
      persistencePath: path.join(this.lingxiHome, ".ephemeral", "plugin-tasks.json"),
      getSessionIdForPath: (sessionPath) => this.getSessionIdForPath(sessionPath),
    });
    // v4 来源名称有界回填：延后到构造完成后执行，不阻塞启动，也不读取消息正文。
    setImmediate(() => {
      void this.refreshModelObservabilitySourceSnapshots().catch((error) => {
        moduleLog.warn(`model observability source snapshot backfill failed: ${error?.message ?? error}`);
      });
    }).unref?.();

    // subagent AbortController 存储（engine 级别，跨 agent 共享）
    this._subagentControllers = new Map();
    this._subagentRunStore = null;

    // 循环服务：由 server 层在 store 与桥接投递就绪后经 setLoopServices 注入
    this._loopStore = null;
    this._loopAlarm = null;
    this._loopController = null;
    this._loopBridgeHooks = null;
    this._taskRegistry.registerHandler("subagent", {
      abort: (taskId) => {
        const ctrl = this._subagentControllers.get(taskId);
        if (ctrl) ctrl.abort();
      },
    });

    this._terminalSessions = new TerminalSessionManager({
      lingxiHome: this.lingxiHome,
      getSessionIdForPath: (sessionPath) => this.getSessionIdForPath(sessionPath),
      emitEvent: (event, sessionPath) => this._emitEvent(event, sessionPath),
    });

    // Checkpoint 备份存储
    this._checkpointStore = new CheckpointStore(
      path.join(this.lingxiHome, "checkpoints")
    );

    // Computer Use runtime is deliberately lazy. Constructing the provider
    // registry resolves native helper paths and wires platform-specific
    // runners; keep startup cold until the global switch is enabled or a
    // Computer Use endpoint/tool explicitly needs the host.
    this._computerProviders = null;
    this._computerHost = null;

    // ── Plugin Manager ──
    this._pluginManager = null;  // initialized async in initPlugins()
    this._pluginDevService = null;
    this._pluginDevEventBusCleanup = null;

    // Pi SDK resources（init 时填充）
    this._resourceLoader = null;

    // Hub 回调（由 Hub 构造后通过 setHubCallbacks 注入，替代旧的 engine._hub 双向引用）
    this._hubCallbacks = null;

    // 事件系统
    this._listeners = new Set();
    this._eventBus = null;
    this._usageLedger = createUsageLedger({
      storagePath: path.join(this.lingxiHome, "usage-ledger.json"),
      eventBus: {
        emit: (event, sessionPath) => this._emitEvent(event, sessionPath),
      },
      logger: moduleLog,
    });
    this._rerankClient = new RerankClient({
      resolveOperationFresh: (operation) => this.resolveModelOperationFresh(operation),
      getUsageLedger: () => this._usageLedger,
    });
    // Phase 8：ledger 就绪后接线 accounting projection（构造期 install 时
    // ledger 尚未创建，_wireModelObservabilityAccounting 会提前返回）。
    this._wireModelObservabilityAccounting();

    // 首次剥媒体通知去重：sessionPath → 已通知。由 context extension handler 维护，
    // 避免每一轮对话都重复广播 stripped_notice 事件。
    this._imageStripNotified = new Set();
    this._videoStripNotified = new Set();

    // UI context（用户当前视野）：sessionPath → { currentViewed, activeFile,
    // activePreview, pinnedFiles }。由前端每次发 prompt 时带过来，经 server/routes/chat.ts
    // 写入；current_status 工具按需读取 ui_context 来解析“这个 / 当前打开的”等指代。
    this._uiContextBySession = new Map();

    // DevTools 日志
    this._devLogs = [];
    this._devLogsMax = 200;

    this._outboundProxyRuntime = null;

    // 设置起始 agentId
    this._agentMgr.activeAgentId = startId;
  }

  // ════════════════════════════
  //  Agent 代理（→ AgentManager）
  // ════════════════════════════

  /** @ui-focus-only 返回 UI 焦点 agent 实例，后端逻辑应通过 getAgent(agentId) 查询 */
  get agent() { return this._agentMgr.agent; }
  get usageLedger() { return this._usageLedger; }
  getAgent(agentId) { return this._agentMgr.getAgent(agentId); }
  async ensureAgentRuntime(agentId, opts: any = {}) {
    const targetId = agentId || this.currentAgentId;
    return this._agentMgr.ensureAgentRuntime(targetId, opts);
  }
  /** @ui-focus-only 返回 UI 焦点 agent 的 ID */
  get currentAgentId() { return this._agentMgr.activeAgentId; }
  get confirmStore() { return this._confirmStore; }
  get automationSuggestionStore() { return this._automationSuggestionStore; }
  getAutomationSuggestionStore() { return this._automationSuggestionStore; }
  get sessionCollabDraftStore() { return this._sessionCollabDraftStore; }
  get approvalGateway() { return this._approvalGateway; }
  getStudioCronStore() { return this._studioCronService; }

  /** @deprecated 工具应通过 emitEvent(event, sessionPath) 传入显式 sessionPath */
  emitSessionEvent(event) {
    this._emitEvent(event, this.currentSessionPath);
  }

  setConfirmStore(store) {
    this._confirmStore = store;
    if (store) {
      store.onResolved = (confirmId, action) => {
        this._emitEvent({ type: "confirmation_resolved", confirmId, action }, null);
      };
    }
  }

  setDeferredResultStore(store) {
    this._deferredResultCoordinator?.dispose?.();
    this._deferredResultStore = store;
    this._deferredResultCoordinator = null;
    if (store) {
      this._deferredResultCoordinator = new DeferredResultCoordinator({
        store,
        sessionCoordinator: this._sessionCoord,
        recordCustomEntry: (sessionPath, customType, data) => (
          this.recordSessionCustomEntry(sessionPath, customType, data)
        ),
      });
      this._deferredResultCoordinator.start();
    }
  }

  get deferredResults() {
    return this._deferredResultStore || null;
  }

  /**
   * Phase 7 Durable Model Observatory persistence handle（disabled/未安装时
   * null）。只读诊断用途；close 归 engine.dispose() 所有。
   */
  get modelObservabilityPersistence() {
    return this._modelObservability || null;
  }

  /**
   * Phase 8 §五十四：安装 observability persistence。优先级：CompositionRoot
   * 显式 option（enabled=true，测试/dev wiring）> 已保存用户 preference
   * （preferences.json model_observability，canonical normalizer 单一来源）。
   * 产品偏好固定全开；打开失败 → disabled handle，主程序不受影响并诚实暴露原因。
   */
  _installModelObservability(explicitOption) {
    this._modelObservability = null;
    let policy = null;
    if (explicitOption && explicitOption.enabled === true) {
      policy = explicitOption;
    } else if (this._prefs?.getModelObservability) {
      const preference = this._prefs.getModelObservability();
      // 启动即把旧版关闭值写回为全开；只改开关，用户已有保留天数由 normalizer 原样带回。
      try {
        this._prefs.setModelObservability?.(preference);
      } catch (error) {
        moduleLog.warn(`model observability preference migration failed: ${error?.message ?? error}`);
      }
      if (preference.enabled) policy = modelObservabilityPreferenceToPolicy(preference);
    }
    if (!policy) {
      this._modelObservabilityGenerations.reconfigure(null);
      return;
    }
    try {
      this._modelObservability = this._modelObservabilityGenerations.reconfigure(policy);
      if (!this._modelObservability) return;
      const health = this._modelObservability.getHealth();
      if (health.status === "disabled") {
        moduleLog.warn(
          `model observability persistence disabled (${health.storeDisabledReasonCode}); model runtime continues`,
        );
        this._modelObservability = null;
      } else {
        this._wireModelObservabilityAccounting();
      }
    } catch (error) {
      moduleLog.warn(`model observability persistence install failed: ${error?.message ?? error}`);
      this._modelObservability = null;
    }
  }

  /**
   * Phase 8 §十四：Usage Ledger append → llm_usage 事件 → accounting projection。
   * 复用既有 ledger append 通道，不改任何模型调用点；含 bounded ledger
   * best-effort backfill（§十五，只做一次）。usageLedger 创建后调用。
   */
  _wireModelObservabilityAccounting() {
    const handle = this._modelObservability;
    if (!handle?.initializeAccounting || !this._usageLedger) return;
    try {
      handle.initializeAccounting({
        listLedgerEntries: () => this._usageLedger.list({}).entries ?? [],
        subscribeUsage: (consumer) => this.subscribe((event) => consumer(event)),
      });
    } catch (error) {
      moduleLog.warn(`model observability accounting wiring failed: ${error?.message ?? error}`);
    }
  }

  /** Phase 8 §十七：统一 Query Service（稳定 facade；route 只经此查询）。 */
  getModelObservabilityQueryService() {
    if (!this._modelObservabilityQuery) {
      this._modelObservabilityQuery = createModelObservabilityQueryService({
        lingxiHome: this.lingxiHome,
      });
    }
    return this._modelObservabilityQuery;
  }

  _recordModelObservabilitySourceSnapshot(kind, entityId, title) {
    if (!kind || !entityId) return false;
    return this._modelObservability?.upsertSourceIdentitySnapshot?.({
      kind,
      entityId,
      title: typeof title === "string" && title.trim() ? title.trim() : null,
    }) === true;
  }

  /**
   * 把仍存在的会话和任务名称同步进 v4 快照。只读取列表摘要，数量有界，
   * 不读取完整消息正文；实体后来删除时，查询层仍能使用最后快照。
   */
  async refreshModelObservabilitySourceSnapshots() {
    if (!this._modelObservability?.upsertSourceIdentitySnapshot) return { sessions: 0, tasks: 0 };
    const sessions = (await this.listSessions({ includePluginPrivate: true })).slice(0, 5000);
    const sessionIdByPath = new Map();
    let sessionCount = 0;
    for (const session of sessions) {
      const title = typeof session?.title === "string" && session.title.trim()
        ? session.title.trim()
        : typeof session?.firstMessage === "string" && session.firstMessage.trim()
          ? session.firstMessage.trim().replace(/\s+/g, " ").slice(0, 80)
          : null;
      const ids = new Set([session?.sessionId, session?.path].filter(Boolean));
      if (session?.path && session?.sessionId) sessionIdByPath.set(session.path, session.sessionId);
      for (const entityId of ids) {
        if (this._recordModelObservabilitySourceSnapshot("chat", entityId, title)) sessionCount += 1;
      }
    }
    // 自动化调用历史上常只带隔离会话路径；桌面活动账本保存了这条路径与业务摘要。
    let activityCount = 0;
    let taskCount = 0;
    for (const [agentId] of this._agentMgr.agents) {
      if (activityCount >= 5000) break;
      const activities = this.getActivityStore(agentId)?.list?.() ?? [];
      for (const activity of activities) {
        if (activityCount >= 5000) break;
        const rawTitle = [activity?.label, activity?.summary, activity?.type]
          .find((value) => typeof value === "string" && value.trim());
        const title = typeof rawTitle === "string"
          ? rawTitle.trim().replace(/\s+/g, " ").slice(0, 160)
          : null;
        const sessionPath = activity?.sessionFile
          ? path.join(this.agentsDir, agentId, "activity", activity.sessionFile)
          : null;
        const ids = new Set([
          activity?.id,
          sessionPath,
          sessionPath ? sessionIdByPath.get(sessionPath) : null,
        ].filter(Boolean));
        for (const entityId of ids) {
          if (this._recordModelObservabilitySourceSnapshot("automation", entityId, title)) taskCount += 1;
        }
        activityCount += 1;
      }
    }
    const tasks = this._taskRegistry?.listAll?.({})?.slice?.(0, 5000) ?? [];
    for (const task of tasks) {
      const meta = task?.meta && typeof task.meta === "object" ? task.meta : {};
      const title = [meta.label, meta.name, meta.title, meta.summary]
        .find((value) => typeof value === "string" && value.trim());
      const kind = task?.type === "subagent"
        ? "subagent"
        : /automation|cron|schedule|heartbeat/.test(String(task?.type || ""))
          ? "automation"
          : "background_task";
      if (task?.taskId && this._recordModelObservabilitySourceSnapshot(
        kind,
        task.taskId,
        typeof title === "string" ? title.trim().replace(/\s+/g, " ").slice(0, 160) : task.type,
      )) taskCount += 1;
    }
    for (const schedule of this._taskRegistry?.listSchedules?.({})?.slice?.(0, 5000) ?? []) {
      const meta = schedule?.meta && typeof schedule.meta === "object" ? schedule.meta : {};
      const rawTitle = [meta.label, meta.name, meta.title, meta.summary, schedule?.type]
        .find((value) => typeof value === "string" && value.trim());
      if (schedule?.scheduleId && this._recordModelObservabilitySourceSnapshot(
        "automation",
        schedule.scheduleId,
        typeof rawTitle === "string" ? rawTitle.trim().replace(/\s+/g, " ").slice(0, 160) : null,
      )) taskCount += 1;
    }
    this._modelObservabilityQuery?.invalidate?.();
    return { sessions: sessionCount, tasks: taskCount };
  }

  /** 当前实体名称优先于快照；返回值只改显示投影，不碰原始来源字段。 */
  async enrichModelObservabilitySources(value) {
    const sessions = (await this.listSessions({ includePluginPrivate: true })).slice(0, 5000);
    const current = new Map();
    const sessionIdByPath = new Map();
    for (const session of sessions) {
      const title = typeof session?.title === "string" && session.title.trim()
        ? session.title.trim()
        : typeof session?.firstMessage === "string" && session.firstMessage.trim()
          ? session.firstMessage.trim().replace(/\s+/g, " ").slice(0, 80)
          : null;
      for (const id of [session?.sessionId, session?.path]) {
        if (id && title) current.set(`chat\0${id}`, title);
      }
      if (session?.path && session?.sessionId) sessionIdByPath.set(session.path, session.sessionId);
    }
    let activityCount = 0;
    for (const [agentId] of this._agentMgr.agents) {
      if (activityCount >= 5000) break;
      for (const activity of this.getActivityStore(agentId)?.list?.() ?? []) {
        if (activityCount >= 5000) break;
        const rawTitle = [activity?.label, activity?.summary, activity?.type]
          .find((candidate) => typeof candidate === "string" && candidate.trim());
        const title = typeof rawTitle === "string"
          ? rawTitle.trim().replace(/\s+/g, " ").slice(0, 160)
          : null;
        const sessionPath = activity?.sessionFile
          ? path.join(this.agentsDir, agentId, "activity", activity.sessionFile)
          : null;
        for (const id of [activity?.id, sessionPath, sessionPath ? sessionIdByPath.get(sessionPath) : null]) {
          if (id && title) current.set(`automation\0${id}`, title);
        }
        activityCount += 1;
      }
    }
    for (const task of this._taskRegistry?.listAll?.({})?.slice?.(0, 5000) ?? []) {
      const meta = task?.meta && typeof task.meta === "object" ? task.meta : {};
      const rawTitle = [meta.label, meta.name, meta.title, meta.summary]
        .find((candidate) => typeof candidate === "string" && candidate.trim());
      const title = typeof rawTitle === "string"
        ? rawTitle.trim().replace(/\s+/g, " ").slice(0, 160)
        : task?.type || null;
      for (const kind of ["background_task", "automation", "subagent"]) {
        if (task?.taskId && title) current.set(`${kind}\0${task.taskId}`, title);
      }
    }
    for (const schedule of this._taskRegistry?.listSchedules?.({})?.slice?.(0, 5000) ?? []) {
      const meta = schedule?.meta && typeof schedule.meta === "object" ? schedule.meta : {};
      const rawTitle = [meta.label, meta.name, meta.title, meta.summary, schedule?.type]
        .find((candidate) => typeof candidate === "string" && candidate.trim());
      if (schedule?.scheduleId && typeof rawTitle === "string") {
        current.set(`automation\0${schedule.scheduleId}`, rawTitle.trim().replace(/\s+/g, " ").slice(0, 160));
      }
    }
    const visit = (node) => {
      if (!node || typeof node !== "object") return;
      const identity = node.sourceIdentity;
      if (identity?.entityId && identity?.kind) {
        const title = current.get(`${identity.kind}\0${identity.entityId}`);
        if (title) {
          identity.title = title;
          identity.resolution = "current";
          this._recordModelObservabilitySourceSnapshot(identity.kind, identity.entityId, title);
        }
      }
      for (const child of Array.isArray(node) ? node : Object.values(node)) visit(child);
    };
    visit(value);
    return value;
  }

  /**
   * Phase 8 §五十八/五十九：settings 读取。desired（用户偏好）与 effective
   * （运行态，含 schema_newer 等 disable 原因）是两个概念，分开返回。
   */
  getModelObservabilitySettings(): ModelObservabilitySettingsResponse {
    const desired = this._prefs?.getModelObservability
      ? this._prefs.getModelObservability()
      : DEFAULT_MODEL_OBSERVABILITY_PREFERENCE;
    const health = this._modelObservability?.getHealth?.() ?? null;
    return {
      desired,
      effective: {
        recordingStatus: health ? health.status : "disabled",
        storeDisabledReasonCode: health?.storeDisabledReasonCode ?? (desired.enabled ? "not_installed" : "disabled_by_policy"),
        persistTraceMetadata: health ? health.persistTraceMetadata : false,
        persistPayloads: health ? health.persistPayloads : false,
        persistBlobs: health ? health.persistBlobs : false,
        schemaVersion: health?.schemaVersion ?? null,
      },
      // §六十二：诚实暴露 at-rest 安全事实——filesystem permissions，非加密。
      cryptographicallyEncryptedAtRest: false,
    };
  }

  /**
   * 运行中只允许调整保留期并重配 persistence generation。
   * 顺序：normalize → persist desired preference → 旧 generation 退出全局入口并
   * 排水 → install 新 generation（不删除历史数据，§六十）→ invalidate query
   * reader → 返回 desired + effective。设置变化只影响新开始的调用。
   */
  async setModelObservabilitySettings(partial: ModelObservabilitySettingsUpdateRequest): Promise<ModelObservabilitySettingsUpdateResponse> {
    const desired = this._prefs.setModelObservability(partial);
    // preference（days）→ policy（ms）经 canonical 转换，不能直接把 preference
    // 形状喂给 install（retention 字段名不同，会被静默换成 safe fallback）。
    this._installModelObservability(
      desired.enabled ? modelObservabilityPreferenceToPolicy(desired) : { enabled: false },
    );
    this._modelObservabilityQuery?.invalidate?.();
    const settings = this.getModelObservabilitySettings();
    const queryHealth = this.getModelObservabilityQueryService().getHealth();
    return {
      ...settings,
      queryHealth: queryHealth.ok === true ? queryHealth.value : null,
      queryError: queryHealth.ok === true ? null : queryHealth.error,
    };
  }

  /**
   * Phase 8 §四十九：Query Health read model（query side + recording side，
   * 绝不包含正文）。recording disabled 时仍可读历史（§五 desired/effective
   * 解耦：queryStatus 独立于 recordingStatus）。
   */
  getModelObservabilityHealth(): ModelObservabilityHealthResponse {
    const recording = this._modelObservability?.getHealth?.() ?? null;
    const query = this.getModelObservabilityQueryService().getHealth();
    const queryHealth = query.ok === true ? query.value : null;
    const dataCompletenessKnown = queryHealth?.dataCompleteness.status === "known";
    return {
      recordingStatus: recording ? recording.status : "disabled",
      storeDisabledReasonCode: recording?.storeDisabledReasonCode ?? null,
      persistTraceMetadata: recording ? recording.persistTraceMetadata : false,
      persistPayloads: recording ? recording.persistPayloads : false,
      persistBlobs: recording ? recording.persistBlobs : false,
      queuedTraceEvents: recording?.queuedTraceEvents ?? 0,
      queuedPayloadRecords: recording?.queuedPayloadRecords ?? 0,
      queuedBlobs: recording?.queuedBlobs ?? 0,
      queuedUsageEntries: recording?.queuedUsageEntries ?? 0,
      droppedTraceEvents: dataCompletenessKnown
        ? (recording?.droppedTraceEvents ?? queryHealth.dataCompleteness.droppedTraceEvents)
        : null,
      droppedPayloadRecords: dataCompletenessKnown
        ? (recording?.droppedPayloadRecords ?? queryHealth.dataCompleteness.droppedPayloadRecords)
        : null,
      droppedBlobs: dataCompletenessKnown
        ? (recording?.droppedBlobs ?? queryHealth.dataCompleteness.droppedBlobs)
        : null,
      droppedUsageEntries: recording?.droppedUsageEntries ?? 0,
      writeFailures: recording?.writeFailures ?? 0,
      maintenanceErrors: recording?.maintenanceErrors ?? 0,
      lastSuccessfulFlushAt: recording?.lastSuccessfulFlushAt ?? null,
      interruptedByRestartCalls: dataCompletenessKnown
        ? queryHealth.dataCompleteness.interruptedByRestartCalls
        : null,
      atRestEncryption: false,
      query: queryHealth ?? {
        queryStatus: "unavailable",
        queryStatusReason: query.ok === true ? null : query.error.code,
        schemaVersion: null,
        accountingProjectionAvailable: false,
        oldestCallAt: null,
        newestCallAt: null,
        callCount: 0,
        traceCount: 0,
        payloadRecordCount: 0,
        usageProjectionCount: 0,
        dataCompleteness: {
          status: "unknown",
          droppedTraceEvents: null,
          droppedPayloadRecords: null,
          droppedBlobs: null,
          interruptedByRestartCalls: null,
        },
      },
    };
  }

  /**
   * 循环服务接线。bridgeHooks 由 server 层在 bridge-manager 就绪后注入：
   * { executeLoopTurn(sessionKey, agentId, text), sendNotice(sessionKey, agentId, text),
   *   resolveSessionId(sessionKey, agentId), ensureSessionId(sessionKey, agentId) }
   * 未注入时桥接循环的投递按服务不可用抛错（fail-closed），桌面循环不受影响。
   */
  setLoopServices({ store, bridgeHooks = null }) {
    this._loopAlarm?.dispose?.();
    this._loopController?.dispose?.();
    this._loopStore = store || null;
    this._loopBridgeHooks = bridgeHooks;
    this._loopAlarm = null;
    this._loopController = null;
    if (!store) return;

    const requireBridgeHooks = () => {
      if (!this._loopBridgeHooks) throw new Error("loop: bridge delivery is not wired");
      return this._loopBridgeHooks;
    };
    const targetResetError = (detail) => {
      const err: any = new Error(`loop target session was reset: ${detail}`);
      err.code = "loop_target_reset";
      return err;
    };
    // 桌面：sessionId → 当前活跃路径；换代/归档 → loop_target_reset
    const resolveDesktopPath = (sessionId) => {
      // resolveSessionRef 经 SessionManifestResolver 按 sessionId 解析 manifest，
      // 当前 locator 路径在 manifest 上是 currentLocator.path；查无此 id 时抛
      // session_manifest_not_found，对循环而言即目标已换代。清单服务本身不可用
      // 是另一类故障，原样上抛而不伪装成换代。
      let manifest;
      try {
        manifest = this.resolveSessionRef({ sessionId });
      } catch (error: any) {
        if (error?.code === "session_manifest_not_found" || error?.code === "session_manifest_ref_required") {
          throw targetResetError(sessionId);
        }
        throw error;
      }
      const sessionPath = manifest?.currentLocator?.path ?? null;
      if (!sessionPath || !this._sessionCoord.isRunnableSessionPath(sessionPath)) {
        throw targetResetError(sessionId);
      }
      return sessionPath;
    };
    const resolveTargetSessionPathSoft = (target) => {
      // 守恒检查用的软解析：解析不到返回 null（视为无后台任务），不抛错
      try {
        if (target.kind === "desktop") return resolveDesktopPath(target.sessionId);
        const hooks = this._loopBridgeHooks;
        if (!hooks) return null;
        if (hooks.resolveSessionId(target.sessionKey, target.agentId) !== target.sessionId) return null;
        const agent = this.getAgent?.(target.agentId);
        return agent
          ? this.bridgeSessionManager?.resolveSessionPathForSessionKey?.(target.sessionKey, agent) ?? null
          : null;
      } catch {
        return null;
      }
    };
    const hasLiveBackgroundWork = (target) => {
      const sessionPath = resolveTargetSessionPathSoft(target);
      if (!sessionPath) return false;
      if (this.taskRegistry?.hasActiveForParentSession?.(sessionPath)) return true;
      return (this._deferredResultStore?.listPending?.(sessionPath)?.length ?? 0) > 0;
    };

    const controller = new LoopController({
      store,
      hasLiveBackgroundWork,
      deliverLoopMessage: (target, message) => {
        if (target.kind === "desktop") {
          const sessionPath = resolveDesktopPath(target.sessionId);
          return this._sessionCoord.deliverCustomMessage(sessionPath, message, { triggerTurn: true });
        }
        const hooks = requireBridgeHooks();
        if (hooks.resolveSessionId(target.sessionKey, target.agentId) !== target.sessionId) {
          throw targetResetError(target.sessionKey);
        }
        return hooks.executeLoopTurn(target.sessionKey, target.agentId, message.content);
      },
      recordNotice: (target, message) => {
        if (target.kind === "desktop") {
          const sessionPath = resolveDesktopPath(target.sessionId);
          return this._sessionCoord.deliverCustomMessage(sessionPath, message, { triggerTurn: false });
        }
        return requireBridgeHooks().sendNotice(target.sessionKey, target.agentId, message.content);
      },
      recordUserPrompt: (target, prompt) => {
        // 把用户发起循环时的任务 prompt 记成展示型 user 气泡（appendCustomEntry 写
        // type:"custom" 无 content → buildSessionContext 阶段即被排除 → 永不进 model
        // 输入）。仅桌面目标。
        if (target.kind !== "desktop") return Promise.resolve();
        const sessionPath = resolveDesktopPath(target.sessionId);
        return this._sessionCoord.recordLoopUserPrompt(sessionPath, prompt);
      },
      emitLoopStatus: (target, loop) => {
        // 状态变更广播。用软解析：stop/completed/会话换代时 locator 可能已失效，
        // 解析不到则 path=null —— chat.ts 的 loop_status 分支走 broadcast 且前端按
        // sessionId 路由（event.target.sessionId 随事件携带），path 缺失不影响投递，
        // 绝不因推送抛错。
        const path = resolveTargetSessionPathSoft(target);
        this._emitEvent({ type: "loop_status", target, loop }, path);
      },
      isTargetMidStream: (target) => {
        if (target.kind !== "desktop") return false;
        const sessionPath = resolveTargetSessionPathSoft(target);
        return sessionPath ? this._sessionCoord.isSessionStreaming(sessionPath) : false;
      },
      isTargetRunnable: (target) => {
        try {
          if (target.kind === "desktop") { resolveDesktopPath(target.sessionId); return true; }
          return this._loopBridgeHooks?.resolveSessionId?.(target.sessionKey, target.agentId) === target.sessionId;
        } catch {
          return false;
        }
      },
      resolveSessionIdForPath: (sessionPath) => this.getSessionIdForPath?.(sessionPath) ?? null,
      resolveTargetFromSessionRef: async (ref, { ensure = false } = {}) => {
        if (ref?.kind === "desktop" && (ref.sessionId || ref.sessionPath)) {
          const sessionId = ref.sessionId || this.getSessionIdForPath?.(ref.sessionPath);
          return sessionId ? { kind: "desktop", sessionId } : null;
        }
        if (ref?.kind === "bridge" && ref.sessionKey) {
          const hooks = requireBridgeHooks();
          let sessionId = hooks.resolveSessionId(ref.sessionKey, ref.agentId);
          if (!sessionId && ensure) sessionId = await hooks.ensureSessionId(ref.sessionKey, ref.agentId);
          return sessionId
            ? { kind: "bridge", sessionId, sessionKey: ref.sessionKey, agentId: ref.agentId }
            : null;
        }
        return null;
      },
    });
    const alarm = new LoopAlarmService({
      store,
      hasLiveBackgroundWork,
      deliverWakeup: (key, reason) => controller.deliverWakeupTurn(key, reason),
      hooks: {
        onDeliveryExhausted: (key, err) => controller.pauseForDeliveryFailure(key, err),
      },
    });
    controller.attachAlarm(alarm);
    this._loopAlarm = alarm;
    this._loopController = controller;
  }

  get loopController() { return this._loopController; }

  setSubagentRunStore(store) {
    this._subagentRunStore = store || null;
  }

  get subagentRuns() {
    return this._subagentRunStore || null;
  }

  setSubagentThreadStore(store) {
    this._subagentThreadStore = store || null;
  }

  get subagentThreads() {
    return this._subagentThreadStore || null;
  }

  setActivityHub(hub) {
    this._activityHub = hub || null;
  }

  get activityHub() {
    return this._activityHub || null;
  }

  get taskRegistry() {
    return this._taskRegistry;
  }

  get knowledge() {
    return this._knowledge;
  }

  get runtimeContext() {
    return this._runtimeContext;
  }

  /**
   * 启动迁移（agents-md-rename）明确记录的改名失败：返回该 agent 的某个新
   * 人格文件对应的旧文件名；没有失败记录时返回 null。Agent 凭它在本进程内
   * 临时读取旧文件——这是 migration-degraded 状态，不是永久双读协议。
   */
  getFailedPersonaRename(agentId, currentFileName) {
    return this._failedPersonaRenames?.get(agentId)?.get(currentFileName) ?? null;
  }

  getRuntimeContext() {
    if (!this._runtimeContext) {
      throw new Error("server runtime context is not initialized");
    }
    return this._runtimeContext;
  }

  createExecutionBoundary( options: any = {}) {
    return createRuntimeExecutionBoundary(this.getRuntimeContext(), options);
  }

  get terminalSessions() {
    return this._terminalSessions;
  }

  registerSessionFile(entry) {
    const sessionId = entry?.sessionId || (
      entry?.sessionPath ? this.getSessionIdForPath(entry.sessionPath) : null
    );
    return this._sessionFiles.registerFile({
      ...entry,
      ...(sessionId ? { sessionId } : {}),
    });
  }
  recordSessionFileOperation(entry) {
    const file = this.registerSessionFile(entry);
    this._emitResourceChangedForSessionFileOperation(file, entry);
    return file;
  }
  _resourceEvents() {
    if (!this._resourceEventBus) {
      this._resourceEventBus = new ResourceEventBus({
        emit: (event, sessionPath) => {
          this._emitEvent(event, sessionPath);
          this._fileHistory?.handleResourceEvent(event);
        },
      });
    }
    return this._resourceEventBus;
  }
  emitResourceChanged(input) {
    return this._resourceEvents().changed(input);
  }
  emitResourceDeleted(input) {
    return this._resourceEvents().deleted(input);
  }
  emitResourceRenamed(input) {
    return this._resourceEvents().renamed(input);
  }
  resourceEventsSince(sequence) {
    return this._resourceEvents().since(sequence);
  }
  _emitResourceChangedForSessionFileOperation(file, entry: any = {}) {
    const origin = typeof file?.origin === "string" ? file.origin : entry?.origin;
    if (origin !== "agent_write" && origin !== "agent_edit") return;

    const sessionPath = file?.sessionPath || entry?.sessionPath;
    const filePath = file?.filePath || entry?.filePath;
    if (!sessionPath || !filePath) return;

    const fileId = file?.id || file?.fileId || null;
    const operation = entry?.operation || file?.operation || (
      Array.isArray(file?.operations) ? file.operations[file.operations.length - 1] : null
    );
    this._resourceEvents().changed({
      changeType: operation === "created" ? "created" : "modified",
      resourceKey: resourceKeyForRef({ kind: "local-file", path: filePath }),
      resource: {
        kind: "local-file",
        provider: "local_fs",
        path: filePath,
        filePath,
      },
      version: {
        ...(file?.mtimeMs !== undefined ? { mtimeMs: file.mtimeMs } : {}),
        ...(file?.size !== undefined ? { size: file.size } : {}),
        ...(file?.version ? { sequence: file.version } : {}),
      },
      source: "agent_tool",
      reason: origin,
      sessionPath,
      fileId,
      origin,
      operation,
      sessionFile: file,
    } as any);
  }
  _sessionFileOptionsWithLocator(options: any = {}) {
    const next = { ...(options || {}) };
    if (next.sessionId && !next.sessionPath) {
      const manifest = this.getSessionManifest?.(next.sessionId) || null;
      const locatorPath = manifest?.currentLocator?.path || null;
      if (locatorPath) next.sessionPath = locatorPath;
    }
    return next;
  }
  getSessionFile(fileId, options) { return this._sessionFiles.get(fileId, this._sessionFileOptionsWithLocator(options)); }
  getSessionFileByPath(filePath, options) { return this._sessionFiles.getByFilePath(filePath, this._sessionFileOptionsWithLocator(options)); }
  getSessionFileBySourceKey(sourceKey, options) { return this._sessionFiles.getBySourceKey(sourceKey, this._sessionFileOptionsWithLocator(options)); }
  listSessionFiles(sessionPath, options: any = {}) {
    if (Object.prototype.hasOwnProperty.call(options || {}, "references")) {
      return this._sessionFiles.listReachable(sessionPath, options.references);
    }
    return this._sessionFiles.list(sessionPath);
  }
  listActiveSessionFiles(sessionPath, additionalReferences: any[] = []) {
    let branch = this.getSessionByPath?.(sessionPath)?.sessionManager?.getBranch?.();
    if (!Array.isArray(branch)) {
      if (typeof sessionPath !== "string" || !sessionPath.trim()) return [];
      try {
        branch = SessionManager.open(sessionPath).getBranch();
      } catch (error) {
        moduleLog.warn(
          `active SessionFile projection could not open ${sessionPath}: ${error?.message || error}`,
        );
        return [];
      }
    }
    return this.listSessionFiles(sessionPath, { references: [branch, additionalReferences] });
  }
  resolveActiveSessionFile(input: any = {}) {
    const options = this._sessionFileOptionsWithLocator({
      sessionId: input?.sessionId || null,
      sessionPath: input?.sessionPath || null,
    });
    const sessionPath = options.sessionPath || null;
    if (!sessionPath) return null;

    const activeFiles = this.listActiveSessionFiles(sessionPath, input?.additionalReferences || []);
    if (!Array.isArray(activeFiles) || activeFiles.length === 0) return null;

    let candidate = null;
    if (input?.fileId) {
      candidate = this.getSessionFile(input.fileId, options);
    } else if (input?.filePath) {
      candidate = this.getSessionFileByPath(input.filePath, options);
    }
    if (!candidate) return null;
    const candidateId = candidate.id || candidate.fileId || null;
    return activeFiles.find((file) => (
      file === candidate
      || (!!candidateId && (file?.id === candidateId || file?.fileId === candidateId))
    )) || null;
  }
  updateSessionFileTranscription(fileId, transcription, options) { return this._sessionFiles.updateTranscription(fileId, transcription, this._sessionFileOptionsWithLocator(options)); }
  forkSessionFiles(options) { return this._sessionFiles.forkSessionFiles(options); }
  discardForkedSessionFiles(options) { return this._sessionFiles.discardForkedSessionFiles(options); }
  forkSessionVisionNotes(options) { return this._visionBridge.forkSessionNotes(options); }
  discardForkedSessionVisionNotes(options) { return this._visionBridge.discardForkedSessionNotes(options); }
  forkSessionPluginConfig(options) { return this._pluginManager.forkSessionConfig(options); }
  discardForkedSessionPluginConfig(options) {
    return this._pluginManager.discardSessionConfig({ sessionId: options?.sessionId });
  }
  forkSessionBrowserState(options) { return BrowserManager.instance().forkSessionState(options); }
  discardForkedSessionBrowserState(options) {
    return BrowserManager.instance().discardForkedSessionState({ sessionPath: options?.sessionPath });
  }
  forkSessionMediaTasks(options) { return this._media.forkSessionTasks(options); }
  discardForkedSessionMediaTasks(options) { return this._media.discardForkedSessionTasks(options); }
  forkSessionDeferredTasks(options) {
    return this._deferredResultStore?.forkTerminalTasks?.(options) || { tasks: 0, taskIds: [] };
  }
  discardForkedSessionDeferredTasks(options) {
    return this._deferredResultStore?.discardForkedTerminalTasks?.(options) || { discarded: 0 };
  }
  _sessionRefForPath(sessionPath) {
    return {
      sessionId: this.getSessionIdForPath(sessionPath),
      sessionPath,
    };
  }
  _sessionRefForPathSafe(sessionPath) {
    return typeof this._sessionRefForPath === "function"
      ? this._sessionRefForPath(sessionPath)
      : { sessionId: null, sessionPath };
  }
  beginCurrentTurnNativeMedia(sessionPath, opts) {
    const sessionRef = typeof this._sessionRefForPathSafe === "function"
      ? this._sessionRefForPathSafe(sessionPath)
      : { sessionId: null, sessionPath };
    return this._currentTurnNativeMedia.begin(sessionRef, opts);
  }
  endCurrentTurnNativeMedia(token) { return this._currentTurnNativeMedia.end(token); }
  _sessionRuntimeKeyForPath(sessionPath) {
    if (!sessionPath) return null;
    try {
      return this.getSessionIdForPath?.(sessionPath) || sessionPath;
    } catch {
      return sessionPath;
    }
  }
  _deleteSessionRuntimeMapEntry(map, sessionPath) {
    const key = this._sessionRuntimeKeyForPath(sessionPath);
    if (!key) return false;
    const deleted = map?.delete?.(key) === true;
    const legacyDeleted = key !== sessionPath ? map?.delete?.(sessionPath) === true : false;
    return deleted || legacyDeleted;
  }
  _deleteSessionRuntimeSetEntry(set, sessionPath) {
    const key = this._sessionRuntimeKeyForPath(sessionPath);
    if (!key) return false;
    const deleted = set?.delete?.(key) === true;
    const legacyDeleted = key !== sessionPath ? set?.delete?.(sessionPath) === true : false;
    return deleted || legacyDeleted;
  }
  clearSessionRuntimeState(sessionPath, reason = "discard") {
    if (!sessionPath) return false;
    void reason;
    const sessionRef = typeof this._sessionRefForPathSafe === "function"
      ? this._sessionRefForPathSafe(sessionPath)
      : { sessionId: null, sessionPath };
    this._deleteSessionRuntimeMapEntry(this._uiContextBySession, sessionPath);
    this._deleteSessionRuntimeSetEntry(this._imageStripNotified, sessionPath);
    this._deleteSessionRuntimeSetEntry(this._videoStripNotified, sessionPath);
    if (typeof this._currentTurnNativeMedia?.clearSession === "function") {
      this._currentTurnNativeMedia.clearSession(sessionRef);
    }
    if (typeof this._sessionFiles?.unloadSession === "function") {
      this._sessionFiles.unloadSession(sessionPath);
    }
    if (typeof this._computerHost?.abortSession === "function") {
      this._computerHost.abortSession(sessionRef);
    }
    return true;
  }
  get speechRecognition() { return this._speechRecognition; }
  get media() { return this._media; }
  get mcp() { return this._mcp; }
  get resources() { return this._resources; }
  getResourceService() {
    if (!this._resources) throw new Error("resource service is not initialized");
    return this._resources;
  }
  getResourceAccessService() {
    if (!this._resourceAccess) throw new Error("resource access service is not initialized");
    return this._resourceAccess;
  }
  getResourceIO() {
    if (!this._resourceIO) {
      if (!this._runtimeContext?.studioId) throw new Error("runtime studioId unavailable");
      this._resourceIO = createSandboxResourceIO({
        cwd: this.userDir,
        agentDir: this.agent?.dir || this.agentsDir,
        workspace: null,
        workspaceFolders: [],
        authorizedFolders: [],
        lingxiHome: this.lingxiHome,
        getSandboxEnabled: () => false,
        getSessionPath: () => this.currentSessionPath || null,
        emitEvent: (event, sessionPath) => this._emitEvent(event, sessionPath),
        eventBus: this._resourceEvents(),
        sessionFiles: this._sessionFiles,
        resourceService: this.getResourceService(),
        studioId: this._runtimeContext.studioId,
      });
    }
    return this._resourceIO;
  }
  retainResourceWatch(resource) {
    if (!this._resourceWatchRegistry || typeof this._resourceWatchRegistry.retain !== "function") {
      throw new Error("resource watch unavailable");
    }
    return this._resourceWatchRegistry.retain(resource);
  }
  subscribeResourceWatch(input) {
    if (!this._resourceWatchRegistry || typeof this._resourceWatchRegistry.subscribe !== "function") {
      throw new Error("resource watch unavailable");
    }
    return this._resourceWatchRegistry.subscribe(input);
  }
  unsubscribeResourceWatch(subscriptionId) {
    if (!this._resourceWatchRegistry || typeof this._resourceWatchRegistry.unsubscribe !== "function") {
      throw new Error("resource watch unavailable");
    }
    return this._resourceWatchRegistry.unsubscribe(subscriptionId);
  }
  resourceWatchDiagnostics() {
    return this._resourceWatchRegistry?.diagnostics?.() || { subscriptions: 0, watches: [] };
  }
  getResource(resourceId) { return this.getResourceService().getResource(resourceId); }
  resolveResourceContent(resourceId) { return this.getResourceService().resolveContent(resourceId); }
  serializeSessionFile(file) { return serializeSessionFile(file, { runtimeContext: this.getRuntimeContext() }); }
  async cleanupColdSessionFiles(options) {
    return this._sessionFiles.cleanupColdSessions({
      agentsDir: this.agentsDir,
      ...(options || {}),
    });
  }

  setSubagentController(taskId, controller) { this._subagentControllers.set(taskId, controller); }
  removeSubagentController(taskId) { this._subagentControllers.delete(taskId); }

  /**
   * 写入某 session 当前的 UI context（用户视野）。
   * 前端在发每条 prompt 时带上；current_status(ui_context) 按需读取。
   * 传 null / undefined 等价于删除（显式清空）。
   *
   * @param {string} sessionPath
   * @param {{currentViewed?: string|null, activeFile?: string|null, activePreview?: string|null, pinnedFiles?: string[]}|null|undefined} ctx
   */
  setUiContext(sessionPath, ctx) {
    if (!sessionPath) return;
    const key = this._sessionRuntimeKeyForPath(sessionPath);
    if (!key) return;
    if (ctx == null) {
      this._deleteSessionRuntimeMapEntry(this._uiContextBySession, sessionPath);
    } else {
      this._uiContextBySession.set(key, ctx);
      if (key !== sessionPath) this._uiContextBySession.delete(sessionPath);
    }
  }

  /** 读取某 session 当前的 UI context。无则返回 null。 */
  getUiContext(sessionPath) {
    if (!sessionPath) return null;
    const key = this._sessionRuntimeKeyForPath(sessionPath);
    if (!key) return null;
    return this._uiContextBySession.get(key)
      || (key !== sessionPath ? this._uiContextBySession.get(sessionPath) : null)
      || null;
  }

  // 向后兼容 getter
  get agentDir() { return this.agent?.agentDir || path.join(this.agentsDir, this.currentAgentId); }
  get baseDir() { return this.agentDir; }
  get activityDir() { return path.join(this.agentDir, "activity"); }
  get activityStore() { return this.getActivityStore(this.currentAgentId); }
  getActivityStore(agentId) { return this._agentMgr.getActivityStore(agentId); }

  get agents() { return this._agentMgr.agents; }
  listAgents(options = {}) { return this._agentMgr.listAgents(options); }
  listDeletedAgents() { return this._agentMgr.listDeletedAgents(); }
  isAgentDeleted(agentId) { return this._agentMgr.isAgentDeleted(agentId); }
  getDeletedAgentInfo(agentId) { return this._agentMgr.getDeletedAgentInfo(agentId); }
  invalidateAgentListCache() { this._agentMgr.invalidateAgentListCache(); }
  async createAgent(opts) { return this._agentMgr.createAgent(opts); }
  async switchAgent(agentId) {
    return this._agentMgr.switchAgent(agentId);
  }
  async deleteAgent(agentId) { return this._agentMgr.deleteAgent(agentId); }
  setPrimaryAgent(agentId) { return this._agentMgr.setPrimaryAgent(agentId); }
  agentIdFromSessionPath(p) { return this._agentMgr.agentIdFromSessionPath(p); }
  resolveSessionOwnership(ref) { return this._sessionCoord.resolveSessionOwnership(ref); }
  isDeletedAgentSession(ref) { return this._sessionCoord.resolveSessionOwnership(ref).agentDeleted; }
  async createSessionForAgent(agentId, cwd, mem, model, opts: any = {}) {
    return this._agentMgr.createSessionForAgent(agentId, cwd, mem, model, opts);
  }

  // 向后兼容：agent 属性代理
  get agentName() { return this.agent.agentName; }
  set agentName(v) { this.agent.agentName = v; }
  get userName() { return this.agent.userName; }
  set userName(v) { this.agent.userName = v; }
  get configPath() { return this.agent.configPath; }
  get sessionDir() { return this.agent.sessionDir; }
  get factsDbPath() { return this.agent.factsDbPath; }
  get memoryMdPath() { return this.agent.memoryMdPath; }

  // ════════════════════════════
  //  Session 代理（→ SessionCoordinator）
  // ════════════════════════════

  get session() { return this._sessionCoord.session; }
  get messages() { return this._sessionCoord.session?.messages ?? []; }
  get isStreaming() { return this._sessionCoord.session?.isStreaming ?? false; }
  /** @ui-focus-only 返回 UI 焦点 session 的路径，后端逻辑不应依赖此值 */
  get currentSessionPath() { return this._sessionCoord.currentSessionPath; }
  get cwd() { return this._sessionCoord.session?.sessionManager?.getCwd?.() ?? process.cwd(); }
  get deskCwd() { return this._sessionCoord.session?.sessionManager?.getCwd?.() || this.homeCwd || null; }

  async createSession(mgr, cwd, mem, model, opts: any = {}) {
    return this._sessionCoord.createSession(mgr, cwd, mem, model, opts);
  }
  resolveSessionRef(ref, opts = {}) {
    if (!this._sessionManifestResolver) {
      const error: any = new Error("Session manifest store is unavailable.");
      error.code = "session_manifest_unavailable";
      error.status = 503;
      throw error;
    }
    return this._sessionManifestResolver.resolve(ref, opts);
  }
  ensureSessionRefForPath(sessionPath, defaults: any = {}) {
    return establishSessionRefForPath(this._sessionManifestStore, sessionPath, defaults);
  }
  tombstoneSessionRef(sessionRef, reason = "session_cleanup") {
    const sessionId = typeof sessionRef?.sessionId === "string" && sessionRef.sessionId.trim()
      ? sessionRef.sessionId.trim()
      : null;
    if (!this._sessionManifestStore) {
      const error: any = new Error("Session manifest store is unavailable.");
      error.code = "session_manifest_unavailable";
      throw error;
    }
    if (!sessionId) {
      const error: any = new Error("tombstoneSessionRef requires sessionId");
      error.code = "session_manifest_ref_required";
      throw error;
    }
    const manifest = this._sessionManifestStore.getBySessionId(sessionId);
    if (!manifest) {
      const error: any = new Error(`Session manifest not found for sessionId=${sessionId}`);
      error.code = "session_manifest_not_found";
      throw error;
    }
    if (manifest.lifecycle === "deleted") {
      return {
        sessionId,
        sessionPath: manifest.currentLocator?.path || sessionRef?.sessionPath || null,
      };
    }
    const sessionPath = manifest.currentLocator?.path || sessionRef?.sessionPath || null;
    if (!sessionPath) throw new Error(`tombstoneSessionRef: locator unavailable for ${sessionId}`);
    const updated = this._sessionManifestStore.updateLocatorLifecycle(
      sessionId,
      sessionPath,
      "deleted",
      reason,
    );
    return {
      sessionId,
      sessionPath: updated?.currentLocator?.path || sessionPath,
    };
  }
  getSessionManifest(sessionId) {
    return this._sessionManifestStore?.getBySessionId(sessionId) || null;
  }
  getSessionBranchHead(sessionId) {
    return this._sessionManifestStore?.getBranchHead?.(sessionId) || null;
  }
  getSessionBranchHeadForPath(sessionPath) {
    const sessionId = this.getSessionIdForPath(sessionPath);
    return sessionId ? this.getSessionBranchHead(sessionId) : null;
  }
  setSessionBranchHead(sessionPath, state) {
    return this._sessionCoord.setSessionBranchHead(sessionPath, state);
  }
  getSessionBranchProjection(sessionPath, opts = {}) {
    return this._sessionCoord.getSessionBranchProjection(sessionPath, opts);
  }
  openSessionManagerAtCurrentBranch(sessionPath, sessionDir) {
    return this._sessionCoord.openSessionManagerAtCurrentBranch(sessionPath, sessionDir);
  }
  syncSessionBranchHead(sessionPath, sessionManager, reason = "append_sync") {
    return this._sessionCoord._syncSessionBranchHeadQuiet(sessionPath, sessionManager, reason);
  }
  getSessionIdForPath(sessionPath) {
    if (!this._sessionManifestResolver) return null;
    try {
      return this._sessionManifestResolver.resolveOptional({ sessionPath })?.sessionId || null;
    } catch (error) {
      moduleLog.warn(`Session manifest lookup failed for ${path.basename(sessionPath || "")}: ${error?.message || error}`);
      return null;
    }
  }

  getLossyLocalCompactionSummarySource(sessionPath) {
    const sessionId = this.getSessionIdForPath(sessionPath);
    if (!sessionId) {
      throw new Error("Instant local compaction could not resolve the session identity");
    }
    const ownership = this.resolveSessionOwnership({ sessionId, sessionPath });
    const agent = ownership?.agentId ? this._agentMgr.getAgent(ownership.agentId) : null;
    if (!agent?.summaryManager) {
      throw new Error("Instant local compaction could not resolve the session's summary owner");
    }
    const record = agent.summaryManager.getSummary(sessionId);
    return {
      summary: record?.summary || "",
      cursor: record?.cursor || null,
      createdAt: record?.created_at || null,
      updatedAt: record?.updated_at || null,
      resetAt: readCompiledResetAt(path.dirname(agent.summariesDir)),
    };
  }

  _openSessionManifestStore() {
    const dbPath = path.join(this.lingxiHome, "session-manifest.db");
    try {
      return new SessionManifestStore({ dbPath });
    } catch (error) {
      moduleLog.warn(`Session manifest store open failed: ${error?.message || error}`);
      let moved = [];
      try {
        moved = moveSessionManifestDbFilesAside({
          lingxiHome: this.lingxiHome,
          suffix: `quarantine-${sanitizeSessionManifestFileSuffix(new Date().toISOString())}`,
        });
      } catch (moveError) {
        moduleLog.warn(`Session manifest quarantine failed: ${moveError?.message || moveError}`);
        this._sessionManifestStoreRecovery = { status: "unavailable", error, quarantineError: moveError };
        return null;
      }

      if (!moved.length) {
        this._sessionManifestStoreRecovery = { status: "unavailable", error, moved };
        return null;
      }

      try {
        const store = new SessionManifestStore({ dbPath });
        this._sessionManifestStoreRecovery = { status: "quarantined", error, moved };
        moduleLog.warn(`Session manifest database quarantined and recreated (${moved.length} files)`);
        return store;
      } catch (retryError) {
        moduleLog.warn(`Session manifest store reopen failed after quarantine: ${retryError?.message || retryError}`);
        this._sessionManifestStoreRecovery = { status: "unavailable", error: retryError, initialError: error, moved };
        return null;
      }
    }
  }

  /**
   * 聚合两路 session 元数据"待恢复"信号，供 /api/health 附块与侧边栏提示消费。
   * 两源：① manifest store 本身不可用/被隔离重建（_sessionManifestStoreRecovery）
   * ② 运行期发生过的 session-meta 隔离（_sessionCoord.listMetaQuarantines）。
   * 两源全空 → { degraded: false, reasons: [] }。纯读聚合，不产生副作用。
   */
  getSessionMetadataRecoveryStatus() {
    const reasons: Array<{ kind: string; detail: string }> = [];

    const recoveryStatus = this._sessionManifestStoreRecovery?.status;
    if (recoveryStatus === "unavailable") {
      reasons.push({ kind: "store_unavailable", detail: "session manifest database unavailable" });
    } else if (recoveryStatus === "quarantined") {
      reasons.push({ kind: "store_quarantined", detail: "session manifest database was quarantined and recreated" });
    }

    for (const quarantine of this._sessionCoord?.listMetaQuarantines?.() || []) {
      reasons.push({ kind: "meta_quarantined", detail: describeMetaSourcePath(quarantine?.metaPath) });
    }

    return { degraded: reasons.length > 0, reasons };
  }

  async createDetachedSession( opts: any = {}) {
    return this._sessionCoord.createDetachedSession(opts);
  }
  async forkSessionAtNode(input: any = {}) {
    return this._sessionCoord.forkSessionAtNode(input);
  }
  buildSessionCacheSnapshot(p, opts) {
    return this._sessionCoord.buildSessionCacheSnapshot(p, opts);
  }
  getSessionProviderCacheAffinityKey(p) {
    return this._sessionCoord.getSessionProviderCacheAffinityKey(p);
  }
  /**
   * Per-session provider quirks that shape the request body. A live request and
   * the compaction request for the same session share a cache prefix, so both
   * have to normalize with the same options; this is the one place that answers
   * what those options are, so neither path can drift from the other.
   */
  getProviderCompatOptionsForSession(p) {
    return {
      deepseekRoleplayReasoningPatch: this._sessionCoord.isDeepSeekRoleplayReasoningPatchEnabled(p),
      deepseekRoleplayReasoningContext: this._sessionCoord.getDeepSeekRoleplayReasoningContext(p),
    };
  }
  /**
   * The reasoning level a request on this session carries. Same reason as the
   * provider options above: the live request and the compaction request for one
   * session ride the same cache prefix, so they cannot each decide for
   * themselves whether reasoning is on. Both ask this.
   */
  resolveRequestReasoningLevel(ctx) {
    return resolveRequestReasoningLevelForContext(this._models, this._prefs, ctx);
  }
  getSessionStreamFn(p) {
    return this._sessionCoord.getSessionStreamFn(p);
  }
  getSessionAgentRunRuntime(p) {
    return this._sessionCoord.getSessionAgentRunRuntime(p);
  }
  getSessionTransformContext(p) {
    return this._sessionCoord.getSessionTransformContext(p);
  }
  async switchSession(p) {
    const result = await this._sessionCoord.switchSession(p);
    await this.syncWorkspaceSkillPaths(this.cwd, { reload: true, emitEvent: false });
    return result;
  }
  /** @deprecated Phase 2: 使用 promptSession(path, text, opts) */
  async prompt(text, opts) { return this._sessionCoord.prompt(text, opts); }
  /** @deprecated Phase 2: 使用 abortSession(path) */
  async abort(options) { return this._sessionCoord.abort(options); }
  /** @deprecated Phase 2: 使用 steerSession(path, text) */
  steer(text) { return this._sessionCoord.steer(text); }

  // ── Path 感知 API（Phase 2） ──
  async promptSession(p, text, opts, submitOptions) {
    return this._sessionCoord.promptSession(p, text, opts, submitOptions);
  }
  steerSession(p, text) { return this._sessionCoord.steerSession(p, text); }
  async abortSession(p, options) { return this._sessionCoord.abortSession(p, options); }
  async deliverCustomMessage(p, message, options) {
    return this._sessionCoord.deliverCustomMessage(p, message, options);
  }
  getEnvChangeLedger() { return this._envChangeLedger; }
  renderSessionReminderBlock(p) { return this._sessionCoord.renderSessionReminderBlock(p); }
  preflightSessionInput(p) { return this._sessionCoord.preflightSessionInput(p); }
  consumeRenderedSessionReminderBlock(p, receipt) {
    return this._sessionCoord.consumeRenderedSessionReminderBlock(p, receipt);
  }
  consumeSessionReminderBlock(p) { return this._sessionCoord.consumeSessionReminderBlock(p); }
  get focusSessionPath() { return this._sessionCoord.currentSessionPath; }
  getMessages(p) { return this._sessionCoord.getSessionByPath(p)?.messages ?? []; }
  getSessionWorkspaceFolders(p = this.currentSessionPath) {
    return this._sessionCoord.getSessionWorkspaceFolders(p);
  }
  getSessionAuthorizedFolders(p = this.currentSessionPath) {
    return this._sessionCoord.getSessionAuthorizedFolders(p);
  }
  getSessionExecutorMetadata(ref) {
    return this._sessionCoord.getSessionExecutorMetadata(ref);
  }
  setSessionExecutorMetadata(ref, metadata, options = {}) {
    return this._sessionCoord.setSessionExecutorMetadata(ref, metadata, options);
  }
  getSessionMemoryReflectionSnapshot(p = this.currentSessionPath) {
    return this._sessionCoord.getSessionMemoryReflectionSnapshot(p);
  }
  getSessionFolderScope(p = this.currentSessionPath) {
    return this._sessionCoord.getSessionFolderScope(p);
  }
  getSessionMemoryEnabled(p = this.currentSessionPath) {
    return this._sessionCoord.getSessionMemoryEnabled(p);
  }
  async setSessionMemoryEnabled(p, enabled) {
    return this._sessionCoord.setSessionMemoryEnabled(p, enabled);
  }
  setSessionAuthorizedFolders(p, folders) {
    return this._sessionCoord.setSessionAuthorizedFolders(p, folders);
  }
  addSessionAuthorizedFolder(p, folder) {
    return this._sessionCoord.addSessionAuthorizedFolder(p, folder);
  }
  removeSessionAuthorizedFolder(p, folder) {
    return this._sessionCoord.removeSessionAuthorizedFolder(p, folder);
  }

  async abortAllStreaming() { return this._sessionCoord.abortAllStreaming(); }
  isBridgeSessionStreaming(key, opts) { return this._bridge?.isSessionStreaming(key, opts) ?? false; }
  async abortBridgeSession(key) { return this._bridge?.abortSession(key) ?? false; }
  steerBridgeSession(key, text, opts) { return this._bridge?.steerSession(key, text, opts) ?? false; }
  get bridgeSessionManager() { return this._bridge; }
  recordSessionCustomEntry(sessionPath, customType, data) {
    const bridgeResult = this._bridge?.recordCustomEntryForSessionPath?.(sessionPath, customType, data);
    if (bridgeResult?.ok) return bridgeResult;
    return this._sessionCoord.recordCustomEntry(sessionPath, customType, data);
  }
  getBridgeContextForSessionPath(sessionPath, opts: any = {}) {
    return this._bridge?.getBridgeContextForSessionPath?.(sessionPath, opts) || null;
  }
  async deliverNotification(payload, opts: any = {}) {
    return this._notifications.notify(payload, opts);
  }
  get slashRegistry() { return this._slashSystem?.registry ?? null; }
  get slashDispatcher() { return this._slashSystem?.dispatcher ?? null; }
  /** /rc 接管态 + pending-selection 内存 store（Phase 2-A） */
  get rcState() { return this._slashSystem?.rcState ?? null; }
  async closeSession(p) { return this._sessionCoord.closeSession(p); }
  async discardSessionRuntime(p, reason, options) { return this._sessionCoord.discardSessionRuntime(p, reason, options); }
  async moveSessionLifecycle(input) { return this._sessionCoord.moveSessionLifecycle(input); }
  getSessionByPath(p) { return this._sessionCoord.getSessionByPath(p); }
  getSessionContextUsage(p) { return this._sessionCoord.getSessionContextUsage(p); }
  /** 确保桌面 session 已加载进 cache 但不改 UI 焦点（Phase 2-C：/rc 接管态用） */
  async ensureSessionLoaded(p) { return this._sessionCoord.ensureSessionLoaded(p); }
  async reloadSessionRuntime(p, opts = {}) { return this._sessionCoord.reloadSessionRuntime(p, opts); }
  getSessionModelAvailability(p = this.currentSessionPath) {
    return this._sessionCoord.getSessionModelAvailability(p);
  }
  isSessionStreaming(p) { return this._sessionCoord.isSessionStreaming(p); }
  isSessionSwitching(p) { return this._sessionCoord.isSessionSwitching(p); }
  async abortSessionByPath(p, options) { return this._sessionCoord.abortSessionByPath(p, options); }
  async listSessions(options = {}) { return this._sessionCoord.listSessions(options); }
  async continueDeletedAgentSession(p) { return this._sessionCoord.continueDeletedAgentSession(p); }
  getSessionProjectCatalog() { return this._sessionProjects.getCatalog(); }
  createSessionProjectFolder(input) { return this._sessionProjects.createFolder(input); }
  updateSessionProjectFolder(id, patch) { return this._sessionProjects.updateFolder(id, patch); }
  deleteSessionProjectFolder(id) { return this._sessionProjects.deleteFolder(id); }
  reorderSessionProjectFolders(input) { return this._sessionProjects.reorderFolders(input); }
  createSessionProject(input) { return this._sessionProjects.createProject(input); }
  updateSessionProject(id, patch) { return this._sessionProjects.updateProject(id, patch); }
  async deleteSessionProject(id) {
    const projectId = normalizeSessionProjectId(id);
    if (!projectId) throw new Error("project not found");
    const sessions = await this._sessionCoord.listSessions();
    const affectedSessions = sessions.filter(sessionBelongsToProject(projectId));
    const catalog = this._sessionProjects.deleteProject(projectId);
    await Promise.all(affectedSessions.map(session => (
      this._sessionCoord.writeSessionMeta(session.path, { projectId: UNCATEGORIZED_PROJECT_ID })
    )));
    return {
      catalog,
      assignment: {
        projectId: UNCATEGORIZED_PROJECT_ID,
        sessionPaths: affectedSessions.map(session => session.path),
      },
    };
  }
  reorderSessionProjects(input) { return this._sessionProjects.reorderProjects(input); }
  normalizeSessionProjectAssignmentId(projectId) {
    const normalizedProjectId = normalizeSessionProjectId(projectId);
    if (!normalizedProjectId) return null;
    const catalog = this._sessionProjects.getCatalog();
    if (
      !isAutoProjectId(normalizedProjectId)
      && !catalog.projects.some(project => project.id === normalizedProjectId)
    ) {
      throw new Error("project not found");
    }
    return normalizedProjectId;
  }
  async setSessionProjectAssignment({ sessionPath, projectId }) {
    if (!sessionPath || typeof sessionPath !== "string") throw new Error("sessionPath is required");
    const normalizedProjectId = this.normalizeSessionProjectAssignmentId(projectId);
    await this._sessionCoord.writeSessionMeta(sessionPath, { projectId: normalizedProjectId });
    return { sessionPath, projectId: normalizedProjectId };
  }
  async listArchivedSessions() { return this._sessionCoord.listArchivedSessions(); }
  async saveSessionTitle(p, t) {
    const result = await this._sessionCoord.saveSessionTitle(p, t);
    this._recordModelObservabilitySourceSnapshot("chat", this.getSessionIdForPath(p) || p, t);
    if (this.getSessionIdForPath(p)) this._recordModelObservabilitySourceSnapshot("chat", p, t);
    return result;
  }
  async clearSessionTitle(p) { return this._sessionCoord.clearSessionTitle(p); }
  async setSessionPinned(p, pinned) { return this._sessionCoord.setSessionPinned(p, pinned); }
  async setSessionPinOrder(orderedRefs) { return this._sessionCoord.setSessionPinOrder(orderedRefs); }
  async setSessionPluginMeta(p, patch) { return this._sessionCoord.setSessionPluginMeta(p, patch); }
  createSessionContext() { return this._sessionCoord.createSessionContext(); }
  async promoteActivitySession(f, agentId) { return this._sessionCoord.promoteActivitySession(f, agentId); }
  async executeIsolated(prompt, opts) { return this._sessionCoord.executeIsolated(prompt, opts); }

  // ════════════════════════════
  //  Config 代理（→ ConfigCoordinator）
  // ════════════════════════════

  get config() { return this.agent.config; }
  get factStore() { return this.agent.factStore; }
  /** 下一次新对话将使用的模型（UI 选择器绑定此值） */
  get currentModel() {
    return this._sessionCoord.pendingModel
      ?? this._models.currentModel;
  }
  /** 当前活跃 session 实际使用的模型（已创建的对话不随选择器变） */
  get activeSessionModel() {
    return this._sessionCoord.session?.model ?? null;
  }
  get availableModels() { return this._models.availableModels; }
  get memoryEnabled() {
    const sessionPath = this.currentSessionPath;
    return sessionPath ? this._sessionCoord.getSessionMemoryEnabled(sessionPath) : this.agent.memoryEnabled;
  }
  get memoryModelUnavailableReason() { return this.agent.memoryModelUnavailableReason; }
  get planMode() { return this._sessionCoord.getPlanMode(); }
  getPrimaryAgentId() { return this._prefs.getPrimaryAgent(); }
  get homeCwd() { return this.getHomeCwd(this.currentAgentId); }

  getHomeCwd(agentId) {
    return this._configCoord.getHomeFolder(agentId || this.currentAgentId) || null;
  }

  getExplicitHomeCwd(agentId) {
    return this._configCoord.getExplicitHomeFolder(agentId || this.currentAgentId) || null;
  }
  _createResourceLoaderOptions(skillsDir) {
    const cwd = resolveLingxiPiSdkResourceLoaderCwd(this.lingxiHome);
    const agentDir = resolveLingxiPiSdkResourceLoaderAgentDir(this.lingxiHome);
    if (!cwd || typeof cwd !== "string") {
      throw new Error("ResourceLoader init: cwd is required");
    }
    if (!agentDir || typeof agentDir !== "string") {
      throw new Error("ResourceLoader init: agentDir is required");
    }
    return {
      cwd,
      agentDir,
      settingsManager: SettingsManager.inMemory(),
      systemPromptOverride: () => this.agent.systemPrompt,
      appendSystemPromptOverride: () => [],
      agentsFilesOverride: () => ({ agentsFiles: [] }),
      noContextFiles: true,
      noExtensions: true,
      noSkills: true,
      noPromptTemplates: true,
      noThemes: true,
      additionalSkillPaths: [skillsDir],
    };
  }
  get authStorage() { return this._models.authStorage; }
  get modelRegistry() { return this._models.modelRegistry; }
  get modelRuntime() { return this._models.modelRuntime; }
  get providerRegistry() { return this._models.providerRegistry; }
  get preferences() { return this._prefs; }

  /** 刷新可用模型列表（含 OAuth 自定义模型注入） */
  async refreshModels() { return this._models.refreshAvailable(); }

  getHomeFolder(agentId) { return this._configCoord.getHomeFolder(agentId); }
  setHomeFolder(agentId, folder) { return this._configCoord.setHomeFolder(agentId, folder); }
  getHeartbeatMaster() { return this._configCoord.getHeartbeatMaster(); }
  setHeartbeatMaster(v) { return this._configCoord.setHeartbeatMaster(v); }
  getChannelsEnabled() { return this._configCoord.getChannelsEnabled(); }
  async setChannelsEnabled(v) { return this._configCoord.setChannelsEnabled(v); }
  isChannelsEnabled() { return this._configCoord.getChannelsEnabled(); }
  getBridgePermissionMode() { return this._prefs.getBridgePermissionMode(); }
  setBridgePermissionMode(v) { return this._prefs.setBridgePermissionMode(v); }
  getAutomationPermissionMode() { return this._prefs.getAutomationPermissionMode(); }
  setAutomationPermissionMode(v) { return this._prefs.setAutomationPermissionMode(v); }
  getBridgeReadOnly() { return this._prefs.getBridgeReadOnly(); }
  setBridgeReadOnly(v) { this._prefs.setBridgeReadOnly(v); }
  getBridgeReceiptEnabled() { return this._prefs.getBridgeReceiptEnabled(); }
  setBridgeReceiptEnabled(v) { this._prefs.setBridgeReceiptEnabled(v); }
  getBridgeRichStreamingEnabled() { return this._prefs.getBridgeRichStreamingEnabled(); }
  setBridgeRichStreamingEnabled(v) { this._prefs.setBridgeRichStreamingEnabled(v); }
  setOutboundProxyRuntime(runtime) { this._outboundProxyRuntime = runtime || null; }
  getNetworkProxy() { return this._prefs.getNetworkProxy(); }
  setNetworkProxy(v) {
    const config = this._prefs.setNetworkProxy(v);
    this._outboundProxyRuntime?.apply?.(config);
    return config;
  }
  getBridgeMediaPublicBaseUrl() { return this._prefs.getBridgeMediaPublicBaseUrl(); }
  setBridgeMediaPublicBaseUrl(v) { return this._prefs.setBridgeMediaPublicBaseUrl(v); }
  getSharedModels() { return this._configCoord.getSharedModels(); }
  setSharedModels(p) {
    const result = this._configCoord.setSharedModels(p);
    // 嵌入/重排模型偏好变更的收敛点（preferences 路由唯一入口）：
    // knowledge_embedding_model 就绪或变化 → 通知知识摄入管线补跑待嵌入 job。
    this._knowledge?.onModelConfigMayHaveChanged?.();
    return result;
  }
  isVisionAuxiliaryEnabled() { return this.getSharedModels()?.vision_enabled === true; }
  getVisionBridge() { return this._visionBridge; }
  _ensureComputerRuntime() {
    if (!this.isComputerUseSupported()) {
      throw new Error("Computer Use is not supported on this platform.");
    }
    if (!this._computerProviders || !this._computerHost) {
      this._computerProviders = new ComputerProviderRegistry();
      this._computerProviders.register(createMockComputerProvider({ providerId: "mock" }));
      this._computerProviders.register(createMacosCuaProvider());
      this._computerProviders.register(createWindowsUiaProvider());
      this._computerHost = new ComputerHost({
        providers: this._computerProviders,
        defaultProviderId: "mock",
        getSettings: () => this.getComputerUseSettings(),
        getAccessMode: (sessionPath) => this._sessionCoord.getAccessMode(sessionPath),
        getPrimaryAgentId: () => this._prefs.getPrimaryAgent(),
      });
    }
    return { providers: this._computerProviders, host: this._computerHost };
  }
  getComputerHost() { return this._ensureComputerRuntime().host; }
  getComputerProviders() { return this._ensureComputerRuntime().providers; }
  isComputerUseSupported(platform = process.platform) { return isComputerUsePlatformSupported(platform); }
  getComputerUseSettings() {
    return effectiveComputerUseSettings(this._prefs.getComputerUseSettings(), { platform: process.platform });
  }
  setComputerUseSettings(partial) {
    if (!this.isComputerUseSupported() && partial?.enabled === true) {
      throw new Error("Computer Use is not supported on this platform.");
    }
    const settings = this._prefs.setComputerUseSettings(partial);
    const effectiveSettings = effectiveComputerUseSettings(settings, { platform: process.platform });
    if (effectiveSettings.enabled === true) this._ensureComputerRuntime();
    return effectiveSettings;
  }
  async updateComputerUseSettings(partial) {
    const effectiveSettings = this.setComputerUseSettings(partial);
    if (effectiveSettings.enabled !== true) {
      await this.disposeComputerRuntime();
    }
    return effectiveSettings;
  }
  async disposeComputerRuntime() {
    const host = this._computerHost;
    try {
      await host?.dispose?.();
    } finally {
      this._computerHost = null;
      this._computerProviders = null;
    }
  }
  approveComputerUseApp(approval) { return this._prefs.approveComputerUseApp(approval); }
  revokeComputerUseApp(approval) { return this._prefs.revokeComputerUseApp(approval); }
  resolveVisionConfig() {
    if (!this.isVisionAuxiliaryEnabled()) return null;
    return this.resolveAuxiliaryModel("vision");
  }
  async resolveVisionConfigFresh() {
    if (!this.isVisionAuxiliaryEnabled()) return null;
    return this.resolveAuxiliaryExecution("vision");
  }
  getSearchConfig() { return this._configCoord.getSearchConfig(); }
  setSearchConfig(p) { return this._configCoord.setSearchConfig(p); }
  _callApprovalReviewerText(options) { return callText(options); }

  _knowledgeOperationUsageContext(operation, runId) {
    return {
      source: {
        subsystem: "knowledge",
        operation,
        surface: "knowledge",
        trigger: "user",
      },
      attribution: {
        kind: "knowledge",
        taskId: runId,
        agentId: this.currentAgentId || null,
      },
    };
  }

  /**
   * ModelOperationResolver 的依赖集（全局偏好路径与按显式引用路径共用）。
   * getOperationModelRef 是唯一差异：全局路径从 shared models 偏好读，
   * 知识摄入路径用笔记本解析链给出的显式引用。
   */
  _modelOperationResolverDeps(getOperationModelRef) {
    return {
      getOperationModelRef,
      resolveOperationModel: (operation, ref) =>
        this._models.providerRegistry.getOperationModel(operation, ref),
      resolveProviderCredentialsFresh: (provider) =>
        this._models.resolveProviderCredentialsFresh(provider),
      getProviderCredentials: (provider) =>
        this._models.providerRegistry.getCredentials(provider),
      allowsMissingApiKey: (provider, baseUrl) =>
        this._models.providerRegistry?.allowsMissingApiKey?.(provider, baseUrl)
        ?? isLocalBaseUrl(baseUrl),
    };
  }

  /** 摄入管线用：同步判定显式嵌入模型引用当前是否可解析（模型存在/支持嵌入/凭证就绪）。 */
  _canResolveKnowledgeEmbeddingRef(ref) {
    if (!ref?.id || !ref?.provider) return false;
    try {
      const resolver = new ModelOperationResolver(this._modelOperationResolverDeps(() => ref));
      return !!resolver.resolveSync("embedding");
    } catch {
      // ModelOperationConfigurationError（模型不存在/不支持/缺凭证）→ 不可解析。
      return false;
    }
  }

  /**
   * 摄入管线用：按显式模型引用执行嵌入。引用不可解析（未配置/模型缺失/缺凭证）
   * 返回 null，由 ingestion-service 落显式 pending_embedding；请求级错误
   * （HTTP 4xx/5xx、超时）照常抛出，走 job 退避重试。
   */
  async _embedKnowledgeTextsForModel(request) {
    const resolver = new ModelOperationResolver(
      this._modelOperationResolverDeps(() => request?.modelRef || null),
    );
    let execution = null;
    try {
      execution = await resolver.resolveFresh("embedding");
    } catch (error) {
      if (error instanceof ModelOperationConfigurationError) return null;
      throw error;
    }
    if (!execution) return null;
    const client = new EmbeddingClient({
      resolveOperationFresh: async () => execution,
      getUsageLedger: () => this._usageLedger,
    });
    // 模型条目声明的维度（如 qwen3-embedding 的 MRL 截断值）与上下文窗口随请求
    // 下发：维度缺失会拿 provider 原生维度（4096），窗口缺失会让 ollama 按模型
    // 声明最大值（40960）预分配 KV cache——8B 模型上多占数 GB 内存。
    const declaredWindow = execution.model?.contextWindow ?? execution.model?.context ?? null;
    return client.embed({
      texts: request.texts,
      signal: request.signal,
      // 与 _embedKnowledgeTexts 同理：整源批量编码可能远超默认 30s 操作超时。
      timeoutMs: 300_000,
      dimensions: execution.model?.dimensions ?? undefined,
      contextWindow: Number.isSafeInteger(declaredWindow) && declaredWindow > 0 ? declaredWindow : undefined,
      // 嵌入用途穿透（查询侧 "query"、摄入缺省 "document"）：MiniMax db/query 与
      // Voyage input_type 的官方算法按此分离，其余协议忽略。
      inputType: request.inputType,
      usageContext: this._knowledgeOperationUsageContext("embedding", request.runId),
    });
  }

  async _rerankKnowledgeTexts(request) {
    return this._rerankClient.rerank({
      query: request.query,
      documents: request.documents,
      topN: request.topN,
      signal: request.signal,
      timeoutMs: 120_000,
      usageContext: this._knowledgeOperationUsageContext("rerank", request.runId),
    });
  }

  // ── 记忆语义检索（facts.db 向量；模型身份经 memory.embedding_model 显式配置） ──

  _memoryEmbeddingAgent(agentId) {
    return this._agentMgr?.getAgent?.(agentId) || null;
  }

  _memoryOperationUsageContext(operation, agentId) {
    return {
      source: {
        subsystem: "memory",
        operation,
        surface: "desktop",
        trigger: "user",
      },
      attribution: {
        kind: "agent",
        agentId: agentId || this.currentAgentId || null,
      },
    };
  }

  /**
   * 解析记忆嵌入执行（模型引用来自 agent config memory.embedding_model）。
   * 未配置 → { status:"unavailable", reason:"no_model" }；
   * 配置了但模型缺失/缺凭证 → reason:"unresolvable"。
   */
  async _resolveMemoryEmbeddingExecution(agentId) {
    const agent = this._memoryEmbeddingAgent(agentId);
    const ref = agent?.memoryEmbeddingModelRef || null;
    if (!ref) return { status: "unavailable", reason: "no_model" };
    const resolver = new ModelOperationResolver(this._modelOperationResolverDeps(() => ref));
    let execution = null;
    try {
      execution = await resolver.resolveFresh("embedding");
    } catch (error) {
      if (error instanceof ModelOperationConfigurationError) {
        return { status: "unavailable", reason: "unresolvable" };
      }
      throw error;
    }
    if (!execution) return { status: "unavailable", reason: "unresolvable" };
    return {
      status: "ok",
      ref,
      execution,
      modelKey: factEmbeddingModelKey(ref, execution.model?.operationProtocol || ""),
    };
  }

  /**
   * search_memory 查询侧闭包：查询文本 → 向量 + model_key。
   * 15s 期限（对齐知识库查询侧 KNOWLEDGE_EMBEDDING_DEADLINE_MS 范式）；
   * 超时/不可用显式返回状态，绝不静默假装没配。
   */
  async embedMemoryQuery(agentId, query) {
    const resolved = await this._resolveMemoryEmbeddingExecution(agentId);
    if (resolved.status !== "ok") {
      return { status: "unavailable", reason: resolved.reason };
    }
    let response;
    try {
      const client = new EmbeddingClient({
        resolveOperationFresh: async () => resolved.execution,
        getUsageLedger: () => this._usageLedger,
      });
      response = await client.embed({
        texts: [String(query || "")],
        timeoutMs: 15_000,
        dimensions: resolved.execution.model?.dimensions ?? undefined,
        inputType: "query",
        usageContext: this._memoryOperationUsageContext("search_memory", agentId),
      });
    } catch (error) {
      if (error?.name === "TimeoutError" || error?.name === "AbortError" || /timeout/i.test(String(error?.message || ""))) {
        return { status: "timeout" };
      }
      throw error;
    }
    const vector = Array.isArray(response?.embeddings?.[0])
      ? response.embeddings[0]
      : Array.isArray(response?.[0]) ? response[0] : null;
    if (!vector) return { status: "timeout" };
    return { status: "ok", vector, modelKey: resolved.modelKey };
  }

  /**
   * 记忆事实向量回填：每 agent 单飞，单次批量 ≤64 条、32 条/请求，
   * KnowledgeEmbeddingProviderGate 限流。失败不抛——留待下轮 ticker 重试
   * （缺向量期间检索侧自动走 FTS 单路并留痕）。
   */
  async backfillMemoryFactEmbeddings(agentId, limit = 64) {
    const agent = this._memoryEmbeddingAgent(agentId);
    const factStore = agent?.factStore;
    if (!agent || !factStore) return { skipped: true, reason: "no_agent_or_store" };
    if (!this._memoryBackfillInFlight) this._memoryBackfillInFlight = new Set();
    if (this._memoryBackfillInFlight.has(agentId)) return { skipped: true, reason: "in_flight" };
    this._memoryBackfillInFlight.add(agentId);
    try {
      const resolved = await this._resolveMemoryEmbeddingExecution(agentId);
      if (resolved.status !== "ok") return { skipped: true, reason: resolved.reason };
      const { modelKey, execution, ref } = resolved;
      const pendingFacts = factStore.factsNeedingEmbedding(modelKey, limit);
      if (pendingFacts.length === 0) {
        const coverage0 = factStore.embeddingCoverage(modelKey);
        return { done: true, embedded: 0, remaining: coverage0.total - coverage0.embedded };
      }

      const client = new EmbeddingClient({
        resolveOperationFresh: async () => execution,
        getUsageLedger: () => this._usageLedger,
      });
      const gateKey = `${ref.provider}/${ref.id}`;
      const gate = this._memoryEmbeddingGate
        || (this._memoryEmbeddingGate = new KnowledgeEmbeddingProviderGate());
      let embedded = 0;
      for (let i = 0; i < pendingFacts.length; i += 32) {
        const batch = pendingFacts.slice(i, i + 32);
        const vectors = await gate.run(gateKey, async () => {
          const response = await client.embed({
            texts: batch.map((f) => (
              `${f.fact}${f.tags && f.tags.length ? ` ${f.tags.join(" ")}` : ""}`
            )),
            timeoutMs: 300_000,
            dimensions: execution.model?.dimensions ?? undefined,
            inputType: "document",
            usageContext: this._memoryOperationUsageContext("fact_backfill", agentId),
          });
          return Array.isArray(response?.embeddings) ? response.embeddings : response;
        });
        const entries = batch
          .map((f, idx) => ({ f, vector: vectors?.[idx] }))
          .filter(({ vector }) => Array.isArray(vector) && vector.length > 0)
          .map(({ f, vector }) => ({
            factId: f.id,
            modelKey,
            dimensions: vector.length,
            vector: serializeVector(vector),
          }));
        embedded += factStore.upsertFactEmbeddings(entries);
      }
      const coverage = factStore.embeddingCoverage(modelKey);
      return {
        done: coverage.embedded >= coverage.total,
        embedded,
        remaining: coverage.total - coverage.embedded,
      };
    } catch (error) {
      moduleLog.warn(`memory fact embedding backfill failed (agent=${agentId}): ${error?.message || error}`);
      return { skipped: true, reason: "error", error: String(error?.message || error) };
    } finally {
      this._memoryBackfillInFlight.delete(agentId);
    }
  }

  /**
   * 知识查询侧 rerank（v8）：按笔记本解析链给出的显式引用路由执行。
   * 配置类错误（模型被移出清单/缺凭证/无操作协议）→ 记日志后返回 null，
   * 检索显式降级为 RRF 名次（与嵌入侧 unresolvable → 纯 FTS 同一语义，
   * 禁静默降级红线：日志留痕 + 调用方按 null 跳过）；请求级错误照常抛出。
   */
  async _rerankKnowledgeTextsForModel(request) {
    const resolver = new ModelOperationResolver(
      this._modelOperationResolverDeps(() => request?.modelRef || null),
    );
    let execution = null;
    try {
      execution = await resolver.resolveFresh("rerank");
    } catch (error) {
      if (error instanceof ModelOperationConfigurationError) {
        moduleLog.log(
          `knowledge rerank skipped (explicit degradation): ${request?.modelRef?.provider ?? "?"}/${request?.modelRef?.id ?? "?"} — ${error.message}`,
        );
        return null;
      }
      throw error;
    }
    if (!execution) return null;
    const client = new RerankClient({
      resolveOperationFresh: async () => execution,
      getUsageLedger: () => this._usageLedger,
    });
    return client.rerank({
      query: request.query,
      documents: request.documents,
      topN: request.topN,
      signal: request.signal,
      timeoutMs: 120_000,
      usageContext: this._knowledgeOperationUsageContext("rerank", request.runId),
    });
  }

  /** 只编译当前会话已经冻结的范围，不接受调用方自造来源列表。 */
  async compileKnowledgeTurnScope(input: { scopeId: string; sessionPath: string }) {
    const scope = this._knowledge?.getTurnScope({ scopeId: input.scopeId });
    if (!scope || scope.status !== "active" || scope.studioId !== this._runtimeContext?.studioId
      || scope.sessionPath !== input.sessionPath) {
      throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Knowledge scope is unavailable in this session");
    }
    return this._knowledge.compileTurnScope(scope);
  }

  /** 冻结本轮资料范围后，只走本机检索与原文证据加工。 */
  async buildFastKnowledgeContext(input: {
    question: string;
    knowledgeRefs: { notebookIds: string[]; mode: "fast" };
    sessionPath: string;
    turnId?: string | null;
    signal?: AbortSignal;
  }): Promise<{ block: string; stats: KnowledgeRetrievalStats; evidence: KnowledgeInjectionEvidence }> {
    input.signal?.throwIfAborted();
    const knowledge = this._knowledge;
    const studioId = this._runtimeContext?.studioId;
    if (!knowledge || !studioId) {
      throw new Error("Knowledge is not accessible in this runtime");
    }
    const scope = knowledge.createTurnScope({
      studioId,
      sessionPath: input.sessionPath,
      turnId: input.turnId ?? null,
      notebookIds: input.knowledgeRefs.notebookIds,
    });
    const { block, stats, evidence } = await knowledge.runFastKnowledgePipeline({
      question: input.question, scope, signal: input.signal,
    });
    return { block, stats, evidence };
  }

  /** 详细提问先在只读隔离会话中调查，只有完整或部分完成的已验证证据才交给主会话。 */
  async buildDetailedKnowledgeResearchContext(input: {
    question: string;
    knowledgeRefs: { notebookIds: string[]; mode: "detailed" };
    sessionId: string;
    sessionPath: string;
    agentId: string;
    turnId: string;
    signal?: AbortSignal;
  }): Promise<{ block: string; stats: KnowledgeRetrievalStats; evidence: KnowledgeInjectionEvidence }> {
    input.signal?.throwIfAborted();
    const started = performance.now(), knowledge = this._knowledge, studioId = this._runtimeContext?.studioId;
    const manifest = this.getSessionManifest(input.sessionId);
    if (!knowledge || !studioId || input.knowledgeRefs.mode !== "detailed" || !input.turnId?.trim()
      || this.getSessionIdForPath(input.sessionPath) !== input.sessionId || manifest?.lifecycle !== "active"
      || manifest.ownerAgentId !== input.agentId || path.resolve(manifest.currentLocator.path) !== path.resolve(input.sessionPath)) {
      throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Detailed research requires the active parent session and its Agent");
    }
    const previousScope = knowledge.store.db.prepare(`SELECT id FROM knowledge_turn_scopes
      WHERE studio_id = ? AND session_path = ? AND turn_id = ? ORDER BY created_at DESC, id LIMIT 1`)
      .get(studioId, input.sessionPath, input.turnId) as { id: string } | undefined;
    const frozen = previousScope ? knowledge.getTurnScope({ scopeId: previousScope.id }) : null;
    const notebookKey = (ids: string[]) => JSON.stringify([...new Set(ids)].sort());
    if (frozen && (frozen.status !== "active" || notebookKey(frozen.notebookIds) !== notebookKey(input.knowledgeRefs.notebookIds))) {
      throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "A repeated turn cannot replace its frozen research scope");
    }
    const scope = frozen ?? knowledge.createTurnScope({ studioId, sessionPath: input.sessionPath, turnId: input.turnId,
      notebookIds: input.knowledgeRefs.notebookIds });
    const compiledScope = await knowledge.compileTurnScope(scope);
    const previousRun = knowledge.store.db.prepare("SELECT question FROM knowledge_research_runs WHERE turn_scope_id = ? LIMIT 1")
      .get(scope.id) as { question: string } | undefined;
    if (previousRun && previousRun.question !== input.question) {
      throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "A repeated research turn cannot replace its question");
    }
    const scopeCompileMs = performance.now() - started;
    const research = new ResearchStore(knowledge.store);
    const modes = new Set<"fts" | "hybrid">(), backends = new Set<"hnsw" | "portable" | "none">();
    const searchedVectorVariants = new Map<string, KnowledgeInjectionEvidence["searchedVectorVariants"][number]>();
    let observedSearches = 0;
    const result = await new KnowledgeResearchOrchestrator({ research,
      executeIsolated: (prompt, options) => this.executeIsolated(prompt, options),
      onProgress: event => this.emitEvent(event, input.sessionPath),
      onSearchCompleted: summary => {
        observedSearches++; modes.add(summary.mode); backends.add(summary.vectorBackend);
        for (const variant of summary.searchedVectorVariants) searchedVectorVariants.set(variant.vectorIndexVariantId, variant);
      },
    }).run({ question: input.question, compiledScope,
      policy: resolveKnowledgeExecutionPolicy({ mode: "detailed", question: input.question,
        selectedNotebookCount: scope.notebookIds.length, selectedSourceCount: scope.sources.length }),
      parentSessionId: input.sessionId, parentSessionPath: input.sessionPath, agentId: input.agentId,
      turnId: input.turnId, signal: input.signal,
    });
    input.signal?.throwIfAborted();
    if (result.run.status === "cancelled") throw new DOMException("Knowledge research was cancelled", "AbortError");
    if (result.run.status !== "completed" && result.run.status !== "partial") {
      throw new KnowledgeError("KNOWLEDGE_RETRIEVAL_UNAVAILABLE", "Detailed research could not produce an answer context", {
        runId: result.run.id, status: result.run.status, stopReason: result.run.stopReason,
      });
    }
    const assembleStarted = performance.now();
    const evidence = this.buildResearchInjectionEvidence(compiledScope, result.packet.canonicalEvidenceSpans);
    evidence.searchedVectorVariants = [...searchedVectorVariants.values()];
    const actions = research.listActions(result.run.id), searches = actions.filter(action => action.actionType === "knowledge_search");
    const missingHistoricalMode = searches.filter(action => action.status === "completed" && action.errorCode === null).length > observedSearches;
    const needs = result.packet.needs, run = result.run;
    const unresolved = needs.filter(need => !["supported", "not_applicable"].includes(need.status));
    const reasons = [...compiledScope.warnings, ...(run.degradedReason ? [run.degradedReason] : []),
      ...(run.status === "partial" ? [run.stopReason ?? "partial"] : []),
      ...(result.packet.truncated ? ["research_evidence_packet_truncated"] : []),
      // 恢复只合成而没有本次检索记录时，不从模型配置猜历史实际检索方式。
      ...(missingHistoricalMode ? ["recovered_retrieval_mode_unavailable"] : [])];
    const stats: KnowledgeRetrievalStats = {
      mode: "detailed", executionPath: "detailed_research", scopeId: scope.id,
      retrievalMode: missingHistoricalMode ? "none" : modes.has("hybrid") ? "hybrid" : modes.has("fts") ? "fts" : "none",
      vectorBackend: missingHistoricalMode ? "none" : backends.has("hnsw") ? "hnsw" : backends.has("portable") ? "portable" : "none",
      searchCalls: run.searchCalls, readCalls: run.readCalls, grepCalls: run.grepCalls,
      subQueries: searches.map(action => String(action.requestSummary.query ?? "")),
      subQueryHits: searches.map(action => Number(action.responseSummary?.count ?? 0)),
      fusedChunks: new Set(searches.flatMap(action => Array.isArray(action.responseSummary?.hitIds) ? action.responseSummary.hitIds : [])).size,
      injectedChunks: result.packet.canonicalEvidenceSpans.length,
      degraded: reasons.length > 0, ...(reasons.length ? { degradeReason: [...new Set(reasons)].join("; ") } : {}),
      truncated: result.packet.truncated, usedTokens: result.usedTokens, budgetTokens: run.budget.finalEvidenceBudgetTokens,
      deadlineMs: run.budget.maxWallClockMs, deadlineExceeded: run.stopReason === "wall_clock_exhausted", scopeCompileMs,
      research: { runId: run.id, status: run.status, completenessPolicy: run.completenessPolicy,
        rounds: run.roundsCompleted, toolCalls: run.toolCallsUsed, delegatedAgents: run.delegatedAgents,
        needsTotal: needs.length, needsSupported: needs.filter(need => need.status === "supported").length,
        needsPartial: needs.filter(need => need.status === "partial").length,
        needsConflicted: needs.filter(need => need.status === "conflicted").length,
        unresolvedNeedIds: unresolved.map(need => need.id), stopReason: run.stopReason ?? "partial" },
      results: result.packet.canonicalEvidenceSpans.map((span, index) => ({ ordinal: index + 1, sourceName: span.sourceName,
        chunkOrdinal: (evidence.entries.find(entry => entry.citationLabels.includes(`K${index + 1}`))?.ordinal ?? 0) + 1,
        firstLine: span.text.split("\n")[0].slice(0, 240) })),
      stageTimings: { assembleMs: performance.now() - assembleStarted, totalMs: performance.now() - started },
    };
    return { block: result.block, stats, evidence };
  }

  /** 扫描凭据可能没有分块编号，只允许映射到冻结解析产物中真实存在且完整覆盖引文的分块。 */
  private buildResearchInjectionEvidence(scope: CompiledKnowledgeScope, spans: KnowledgeEvidenceSpan[]): KnowledgeInjectionEvidence {
    const index = this._knowledge.indexStore;
    const cached = new Map<string, ReturnType<typeof index.listVariantChunks>>();
    const entries = new Map<string, KnowledgeInjectionEvidence["entries"][number]>();
    for (const [ordinal, span] of spans.entries()) {
      const source = scope.sources.find(source => source.sourceId === span.sourceId
        && source.contentSnapshotId === span.contentSnapshotId && source.parseArtifactId === span.parseArtifactId);
      if (!source || span.endOffset - span.startOffset !== span.text.length
        || createHash("sha256").update(span.text).digest("hex") !== span.textSha256) {
        throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Research evidence differs from its frozen source");
      }
      let mapped = false;
      for (const notebook of scope.notebooks.filter(notebook => source.notebookIds.includes(notebook.notebookId) && notebook.chunkProfileHash)) {
        const variant = index.resolveChunkIndexVariant(span.parseArtifactId, notebook.chunkProfileHash!);
        if (!variant || variant.status !== "ready" || !scope.readyChunkVariantIds.includes(variant.id)
          || (span.chunkIndexVariantId && span.chunkIndexVariantId !== variant.id)) continue;
        let chunks = cached.get(variant.id);
        if (!chunks) { chunks = index.listVariantChunks(variant.id); cached.set(variant.id, chunks); }
        const matches = chunks.flatMap(chunk => chunk.spans.filter(location => location.blockId === span.blockId
          && location.blockStartOffset < span.endOffset && location.blockEndOffset > span.startOffset).map(location => {
          const start = Math.max(span.startOffset, location.blockStartOffset), end = Math.min(span.endOffset, location.blockEndOffset);
          return { chunk, start, end, location: { blockId: span.blockId, blockStartOffset: start, blockEndOffset: end,
            chunkStartOffset: location.chunkStartOffset + start - location.blockStartOffset,
            chunkEndOffset: location.chunkStartOffset + end - location.blockStartOffset } };
        })).sort((a, b) => a.start - b.start || a.end - b.end || a.chunk.ordinal - b.chunk.ordinal);
        let covered = span.startOffset;
        for (const match of matches) {
          if (match.start > covered) break;
          if (match.chunk.text.slice(match.location.chunkStartOffset, match.location.chunkEndOffset)
            !== span.text.slice(match.start - span.startOffset, match.end - span.startOffset)) {
            throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Research quote mapping differs from canonical text");
          }
          covered = Math.max(covered, match.end);
        }
        if (covered < span.endOffset) continue;
        for (const match of matches) {
          let entry = entries.get(match.chunk.id);
          if (!entry) {
            entry = { chunkId: match.chunk.id, ordinal: match.chunk.ordinal, sourceId: span.sourceId,
              parseArtifactId: span.parseArtifactId, chunkIndexVariantId: variant.id, chunkProfileHash: notebook.chunkProfileHash,
              notebookId: notebook.notebookId, contextOnly: false, citationLabels: [], blockSpans: [] };
            entries.set(match.chunk.id, entry);
          }
          const label = `K${ordinal + 1}`;
          if (!entry.citationLabels.includes(label)) entry.citationLabels.push(label);
          if (!entry.blockSpans.some(existing => JSON.stringify(existing) === JSON.stringify(match.location))) entry.blockSpans.push(match.location);
        }
        mapped = true; break;
      }
      if (!mapped) throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Validated research evidence has no complete frozen index mapping");
    }
    return { entries: [...entries.values()], searchedVectorVariants: [] };
  }

  /**
   * Phase 8 知识库引用注入门面：覆盖规划（Phase 7 CoveragePlanner，规则层 +
   * knowledge 槽位语义分类，2026-08-31 两档化）+ 拆解（knowledge 槽位，
   * 首次 15s / 纠错重试 8s 超时，temperature 0）+ 受控查询扩展（§三十五，同一
   * 槽位独立提示词）+ retrieveForNotebooks 检索（Phase 8 执行档位：broad 的
   * source/section constrained 二次探测与 §三十六 邻接扩展都在 injector 内编排）
   * + 预算裁剪，产出拼进 prompt 的 [KnowledgeContext] 系统侧注入块与本次检索统计
   * （KnowledgeRetrievalStats，随投影链透出）。证据超预算时走滚动注入
   * （2026-08-31）：中间轮用**会话主模型**经 session.agent.streamFunction 侧线
   * 缓冲调用（凭证/传输/观测链复用，不进消息流不落盘），进度经
   * knowledge_rollup_progress / knowledge_supplement_search 事件广播。
   * desktop-session-submit 在用户可见投影确定之后消费（注入块不进投影）。
   * 内部失败（槽位未配/模型超时/检索错误）在块内显式留痕，不抛错、不阻断聊天
   * （禁静默降级：降级必须可见）；该路径的 stats 带 unavailableReason。
   */
  async buildKnowledgeContextInjection(input: {
    question: string;
    knowledgeRefs: { notebookIds: string[]; mode: "fast" | "detailed" };
    /** 动态注入预算（desktop-session-submit 按会话模型解析）；缺省走 injector 兜底。 */
    budgetTokens?: number;
    /** 会话路径：携带时滚动轮/补充检索广播进度事件。 */
    sessionPath?: string;
    /** 会话轮标识（提交方传 clientMessageId）；缺省时 store 生成。 */
    turnId?: string;
    /**
     * 用户取消信号：desktop-session-submit 检索期 abort 通道传入；滚动中间轮
     * 与补充检索据此中止（提交侧随后丢弃该轮）。
     */
    signal?: AbortSignal;
  }): Promise<{
    block: string;
    stats: KnowledgeRetrievalStats;
    /** 证据身份链（§六十七 EvidenceManifest 数据源）：不进 UI stats，供 manifest 持久化。 */
    evidence: KnowledgeInjectionEvidence;
  }> {
    // 仅供旧历史测试和显式兼容调用；正式详细请求使用研究入口，不加载旧调查编排。
    const { buildKnowledgeContextInjection, KNOWLEDGE_DECOMPOSE_SYSTEM_PROMPT,
      KNOWLEDGE_DECOMPOSE_SPECIALIST_PROMPTS, KNOWLEDGE_EXPANSION_SYSTEM_PROMPT,
      KNOWLEDGE_GAP_ANALYSIS_SYSTEM_PROMPT } = await import("../lib/knowledge/legacy/legacy-knowledge-context-injector.ts");
    const annotateUnavailable = (reason: string) => ({
      block: `[KnowledgeContext]\n[knowledge injection unavailable: ${reason}]\n`
        + `Guidance: Knowledge notebook evidence could not be retrieved for this question.\n[/KnowledgeContext]`,
      stats: {
        mode: input.knowledgeRefs.mode,
        retrievalMode: "none" as const,
        subQueries: [],
        subQueryHits: [],
        degraded: false,
        fusedChunks: 0,
        injectedChunks: 0,
        truncated: false,
        usedTokens: 0,
        budgetTokens: input.budgetTokens ?? 0,
        unavailableReason: reason,
      },
      // 降级路径无任何证据进入模型上下文：身份链为空（不伪造）。
      evidence: { entries: [], searchedVectorVariants: [] } satisfies KnowledgeInjectionEvidence,
    });
    const knowledge = this._knowledge;
    const studioId = this._runtimeContext?.studioId;
    if (!knowledge || !studioId) {
      return annotateUnavailable("Knowledge is not accessible in this runtime");
    }
    try {
      // KnowledgeTurnScope（任务书 §二十/§四十三，Phase 4）：本轮知识权限天花板。
      // 选中 notebooks 的活跃 membership × 各源当前最新 snapshot/artifact 同事务
      // 冻结落库；检索锚定冻结集合，注入块头带出 scopeId 供 knowledge_read 回传。
      // 无 sessionPath 的旧调用方（非会话 surface）不建 scope、不冻结（兼容路径，
      // 注入块不带 scopeId，knowledge_read 侧无 scope 上下文会显式拒绝）。
      const turnScope = input.sessionPath
        ? knowledge.createTurnScope({
          studioId,
          sessionPath: input.sessionPath,
          turnId: input.turnId ?? null,
          notebookIds: input.knowledgeRefs.notebookIds,
        })
        : null;
      const compiledKnowledgeScope = turnScope ? await knowledge.compileTurnScope(turnScope) : null;
      const policy = resolveKnowledgeExecutionPolicy({
        mode: input.knowledgeRefs.mode,
        question: input.question,
        selectedNotebookCount: input.knowledgeRefs.notebookIds.length,
        selectedSourceCount: turnScope?.sources.length ?? 0,
      });
      // 槽位未配置（knowledge 偏好未设）→ 拆解模型为 null，injector 直接单查询并显式标注。
      // 模型解析放在闭包内：槽位配置错误只降级拆解（单查询 + 留痕），检索照常进行。
      const slotConfigured = !!this.getSharedModels()?.knowledge;
      // 滚动注入中间轮闭包（2026-08-31 取代蒸馏）：证据超预算时逐部分喂给
      // **会话主模型**——经 session.agent.streamFunction 侧线缓冲调用（缓存保持
      // 压缩的同款模式：模型凭证/传输/观测链复用 session 装配，不进用户消息流、
      // 不落盘）。会话不存在/streamFn 缺失 → null，injector 显式退回预算截断 +
      // 分片清单降级路径并留痕。
      let rollupModel: KnowledgeRollupModel | null = null;
      let recentTurnsExcerpt: string | null = null;
      if (input.sessionPath) {
        try {
          const session: any = await this.ensureSessionLoaded(input.sessionPath);
          const streamFn = this.getSessionStreamFn(input.sessionPath);
          const sessionModel = session?.model ?? null;
          if (typeof streamFn === "function" && sessionModel) {
            const agent = session?.agent ?? {};
            const streamOptions = {
              sessionId: agent.sessionId,
              // 用途作用域标记（对齐压缩侧线）：滚动中间轮是内部消化，模型级
              // 联网/结构化输出开关不作用于该轮。
              onPayload: (payload: unknown, requestModel: unknown) =>
                runWithProviderCompatPurpose(
                  "knowledge_rollup",
                  () => agent.onPayload?.(payload, requestModel),
                ),
              onResponse: agent.onResponse,
              transport: agent.transport,
              thinkingBudgets: agent.thinkingBudgets,
              maxRetryDelayMs: agent.maxRetryDelayMs,
            };
            rollupModel = async ({ systemPrompt, userPrompt, signal }) => {
              // 取消合并：外部（用户停止）+ 本轮兜底超时（240s——中间笔记轮是
              // 纯消化，超时即失败重试一次再判滚动整体失败，不无限挂）。
              const controller = new AbortController();
              const onExternalAbort = () => controller.abort();
              if (signal) {
                if (signal.aborted) controller.abort();
                else signal.addEventListener("abort", onExternalAbort, { once: true });
              }
              const timer = setTimeout(() => controller.abort(), KNOWLEDGE_ROLLUP_ROUND_TIMEOUT_MS);
              try {
                // 整个调用包进 knowledge_rollup 用途作用域：缓存前缀守卫/Context
                // Ring 分类等请求边界逻辑据此识别侧线轮（不参与主聊天契约）。
                const eventStream = await runWithProviderCompatPurpose("knowledge_rollup", () => Promise.resolve(streamFn(sessionModel, {
                  systemPrompt,
                  messages: [{
                    role: "user",
                    content: [{ type: "text", text: userPrompt }],
                  }],
                }, {
                  ...streamOptions,
                  temperature: 0,
                  signal: controller.signal,
                })));
                const finalMessage: any = await eventStream.result();
                if (finalMessage?.stopReason === "aborted" && (signal?.aborted || controller.signal.aborted)) {
                  const abortError = new Error("knowledge rollup round aborted");
                  abortError.name = "AbortError";
                  throw abortError;
                }
                if (finalMessage?.stopReason !== "stop") {
                  throw new Error(
                    `knowledge rollup round failed (stop reason: ${finalMessage?.stopReason ?? "unknown"}`
                    + `${finalMessage?.errorMessage ? `: ${finalMessage.errorMessage}` : ""})`,
                  );
                }
                const text = (finalMessage?.content ?? [])
                  .filter((block: any) => block?.type === "text")
                  .map((block: any) => typeof block?.text === "string" ? block.text : "")
                  .join("\n")
                  .trim();
                if (!text) throw new Error("knowledge rollup round returned empty output");
                return text;
              } finally {
                clearTimeout(timer);
                signal?.removeEventListener("abort", onExternalAbort);
              }
            };
            // 近期对话摘录（防"那第三章呢"类指代丢失）：尾部几轮的纯文本渲染，
            // 截断到 KNOWLEDGE_ROLLUP_EXCERPT_MAX_CHARS；只进中间轮 prompt。
            recentTurnsExcerpt = buildRecentTurnsExcerpt(
              agent?.state?.messages,
              KNOWLEDGE_ROLLUP_EXCERPT_MAX_CHARS,
            );
          }
        } catch {
          // 会话面不可用（会话不存在/装配失败）→ 滚动面置空，injector 留痕降级。
          rollupModel = null;
        }
      }
      let decomposeModel = null;
      let expandModel = null;
      // Gap Analyzer（§二十二，P2）：与拆解同一 knowledge 槽位、独立系统提示词
      // （specialist="gap" 时选择）；context 携带已有查询与命中摘要进 user 消息。
      let gapAnalysisModel = null;
      if (slotConfigured) {
        decomposeModel = async ({ question, correction, specialist, context }) => {
          const config = await this.resolveAuxiliaryModelFresh("knowledge");
          if (!config) throw new Error("knowledge model slot unavailable");
          // 专业方向（P2 §四）：fact/cause/relation/validation 各自的聚焦提示词；
          // gap 用 Gap Analyzer 提示词；缺省回落通用拆解提示词（兼容直接调用方）。
          const systemPrompt = specialist == null
            ? KNOWLEDGE_DECOMPOSE_SYSTEM_PROMPT
            : specialist === "gap"
              ? KNOWLEDGE_GAP_ANALYSIS_SYSTEM_PROMPT
              : KNOWLEDGE_DECOMPOSE_SPECIALIST_PROMPTS[specialist];
          const contextBlock = context ? `${context}\n\n` : "";
          return callText({
            api: config.api,
            apiKey: config.apiKey,
            baseUrl: config.baseUrl,
            headers: config.headers,
            model: config.model,
            systemPrompt,
            messages: [{
              role: "user",
              content: correction
                ? `Your previous output was invalid and must be corrected.\nError: ${correction.error}\n`
                  + `Previous output: ${correction.previousOutput}\n\n${contextBlock}Question: ${question}\n`
                  + "Return the corrected JSON object following the schema. Plain JSON only, no Markdown fences."
                : `${contextBlock}Question: ${question}`,
            }],
            temperature: 0,
            timeoutMs: correction ? 8_000 : 15_000,
          });
        };
        gapAnalysisModel = decomposeModel;
        // §三十五 受控查询扩展（Phase 8）：与拆解同一 knowledge 槽位、独立系统
        // 提示词；输出非法纠错重试一次，失败不扩展（injector 留痕）。
        expandModel = async ({ question, existingQueries, correction }) => {
          const config = await this.resolveAuxiliaryModelFresh("knowledge");
          if (!config) throw new Error("knowledge model slot unavailable");
          const queriesList = existingQueries.map(query => `- ${query}`).join("\n");
          return callText({
            api: config.api,
            apiKey: config.apiKey,
            baseUrl: config.baseUrl,
            headers: config.headers,
            model: config.model,
            systemPrompt: KNOWLEDGE_EXPANSION_SYSTEM_PROMPT,
            messages: [{
              role: "user",
              content: correction
                ? `Your previous output was invalid and must be corrected.\nError: ${correction.error}\n`
                  + `Previous output: ${correction.previousOutput}\n\nQuestion: ${question}\n`
                  + `Existing queries:\n${queriesList}\n`
                  + "Return the corrected JSON object following the schema. Plain JSON only, no Markdown fences."
                : `Question: ${question}\n\nExisting queries:\n${queriesList}`,
            }],
            temperature: 0,
            timeoutMs: correction ? 8_000 : 15_000,
          });
        };
      }
      // 过程留痕（2026-08-31 二轮）：把拆解/检索阶段逐条以 knowledge_trace 事件
      // 广播，前端在聊天流里实时渲染成过程行（类似编程 Agent 的工具调用卡片：
      // 「已深度思考」「N 个搜索结果」）。只发阶段元数据（查询词/命中数/方向
      // 名），绝不发模型中间输出或 CoT。
      let knowledgeTraceSeq = 0;
      const emitKnowledgeTrace = (entry: {
        id: string;
        kind: "think" | "search";
        phase: "start" | "done" | "failed";
        query?: string;
        hits?: number;
        detail?: string | null;
      }) => {
        if (!input.sessionPath) return;
        this.emitEvent({
          type: "knowledge_trace",
          sessionPath: input.sessionPath,
          ...entry,
        }, input.sessionPath);
      };
      const withThinkTrace = (detail: string | null, call: any) => {
        if (!call) return call;
        return async (args: any) => {
          const id = `think-${++knowledgeTraceSeq}`;
          emitKnowledgeTrace({ id, kind: "think", phase: "start", detail });
          try {
            return await call(args);
          } finally {
            emitKnowledgeTrace({ id, kind: "think", phase: "done", detail });
          }
        };
      };
      decomposeModel = withThinkTrace(null, decomposeModel);
      gapAnalysisModel = decomposeModel;
      expandModel = withThinkTrace("expand", expandModel);
      // CoveragePlanner（任务书 §二十七–§三十二，Phase 7；2026-08-31 两档化）：
      // decompose 前先定覆盖档位；分类闭包复用 knowledge 辅助槽位（与 decompose
      // 同源同超时阶梯）。Phase 8 起 injector 消费 plan：coverageMode 决定执行档位
      // （broad 结构探测 / 自动升级）；planner 失败/未配模型在 plan 内显式留痕
      // 降级 high_recall，不阻断注入。
      let classifyModel = null;
      if (slotConfigured) {
        classifyModel = async ({ question, scopeNote, correction }) => {
          const config = await this.resolveAuxiliaryModelFresh("knowledge");
          if (!config) throw new Error("knowledge model slot unavailable");
          return callText({
            api: config.api,
            apiKey: config.apiKey,
            baseUrl: config.baseUrl,
            headers: config.headers,
            model: config.model,
            systemPrompt: KNOWLEDGE_COVERAGE_CLASSIFY_SYSTEM_PROMPT,
            messages: [{
              role: "user",
              content: correction
                ? `Your previous output was invalid and must be corrected.\nError: ${correction.error}\n`
                  + `Previous output: ${correction.previousOutput}\n\nQuestion: ${question}\n`
                  + "Return the corrected JSON object following the schema. Plain JSON only, no Markdown fences."
                : `Question: ${question}${scopeNote ? `\n${scopeNote}` : ""}`,
            }],
            temperature: 0,
            timeoutMs: correction ? 8_000 : 15_000,
          });
        };
      }
      // planner 与 injector 并行启动：injector 内直检先行（§三十四 安全网），
      // 在拆解前 await 本 promise（planner 先于 decompose）。planKnowledgeCoverage
      // 是总函数（模型失败在 plan 内降级留痕），promise 不会 reject。
      // 快速档（2026-08-31 两档化）整体跳过 planner——零辅助 LLM 轮的一部分，
      // injector 收 coveragePlan: null（无 plan 可消费，stats 不带 coverage 字段）。
      const coveragePlanPromise = policy.path === "fast_local"
        ? null
        : (async () => {
          const plan = await planKnowledgeCoverage({
            question: input.question,
            turnScopeInfo: {
              notebookCount: input.knowledgeRefs.notebookIds.length,
              sourceCount: turnScope ? turnScope.sources.length : null,
            },
            ...(classifyModel ? { classifyModel } : {}),
          });
          // 计划持久化（§二十九 只存结构化结果，禁 CoT）：失败只留日志痕，
          // 不阻断注入（plan 本身已在 stats/块头透出）。
          try {
            knowledge.insertCoveragePlan({
              turnScopeId: turnScope?.id ?? null,
              question: input.question,
              plan,
            });
          } catch (error) {
            moduleLog.log(
              `knowledge coverage plan persistence failed: ${error instanceof Error ? error.message : String(error)}`,
            );
          }
          return plan;
        })();
      const { block, stats, evidence } = await buildKnowledgeContextInjection({
        question: input.question,
        mode: input.knowledgeRefs.mode,
        ...(input.budgetTokens != null ? { budgetTokens: input.budgetTokens } : {}),
        ...(turnScope ? { scopeId: turnScope.id } : {}),
        coveragePlan: coveragePlanPromise,
        deps: {
          decomposeModel,
          expandModel,
          // Gap Analyzer（§二十二，P2）：二轮补证面与拆解同一闭包（specialist
          // 路由提示词）；未配 knowledge 槽位时为 null（不触发）。
          gapAnalysisModel,
          // 滚动注入中间轮（会话主模型侧线）：超预算证据逐部分消化 + 模型自主
          // 补充检索；会话面不可用时为 null（injector 留痕降级预算截断）。
          rollupModel,
          recentTurnsExcerpt,
          ...(input.signal ? { signal: input.signal } : {}),
          // 滚动轮进度 → ws 广播（聊天界面"正在阅读第 X/N 部分"胶囊）；
          // 补充检索 → "模型发起了补充检索"事件（过程可见，不显中间内容）。
          ...(input.sessionPath
            ? {
              onRollupProgress: (event: { current: number; total: number }) => {
                this.emitEvent({
                  type: "knowledge_rollup_progress",
                  sessionPath: input.sessionPath!,
                  current: event.current,
                  total: event.total,
                }, input.sessionPath!);
              },
              onSupplementalSearch: (event: { queries: string[]; round: number }) => {
                this.emitEvent({
                  type: "knowledge_supplement_search",
                  sessionPath: input.sessionPath!,
                  queries: [...event.queries],
                  round: event.round,
                }, input.sessionPath!);
              },
            }
            : {}),
          // Phase 8：sourceIds/sectionsBySourceId 是 broad 结构缺口的约束参数
          // （§三十八/§三十九）；§四十三 冻结对二次检索同样生效。topK 是候选
          // 总预算（§二十一）的每查询分摊，同时约束该查询的 rerank 输入。
          // 每次检索（直检/子查询/扩展/补证/结构探测/滚动补充检索都走这里）
          // 逐行发 knowledge_trace：start 带查询词，done 带命中数。
          retrieve: async ({ query, sourceIds, sectionsBySourceId, topK, rerankPolicy }) => {
            const traceId = `search-${++knowledgeTraceSeq}`;
            emitKnowledgeTrace({ id: traceId, kind: "search", phase: "start", query });
            try {
              const result = compiledKnowledgeScope
                ? (await knowledge.searchService.searchWithEvidence({
                  compiledScope: compiledKnowledgeScope, query, channel: "hybrid", limit: topK ?? KNOWLEDGE_CANDIDATE_GENERATION_BUDGET,
                  rerank: true, signal: input.signal, ...(sourceIds ? { sourceIds } : {}),
                }, sectionsBySourceId)).evidence
                : await knowledge.queryService.retrieveForNotebooks({
                studioId,
                notebookIds: input.knowledgeRefs.notebookIds,
                question: query,
                ...(sourceIds ? { sourceIds } : {}),
                ...(sectionsBySourceId ? { sectionsBySourceId } : {}),
                ...(topK != null ? { topK } : {}),
                ...(rerankPolicy ? { rerankPolicy } : {}),
              });
              emitKnowledgeTrace({
                id: traceId,
                kind: "search",
                phase: "done",
                query,
                hits: result.candidates.length,
              });
              return result;
            } catch (error) {
              emitKnowledgeTrace({ id: traceId, kind: "search", phase: "failed", query, hits: 0 });
              throw error;
            }
          },
          // §三十六 邻接扩展：同 variant 内按锚点 ordinal ± 窗口定点回读。
          readNeighborChunks: ({ anchor, ordinals }) => knowledge.queryService.readAdjacentChunks({
            studioId,
            anchor: {
              notebookId: anchor.notebookId,
              notebookName: anchor.notebookName,
              sourceId: anchor.sourceId,
              sourceName: anchor.sourceName,
              parseArtifactId: anchor.parseArtifactId,
              chunkIndexVariantId: anchor.chunkIndexVariantId,
            },
            ordinals,
          }),
        },
      });
      return { block, stats, evidence };
    } catch (err) {
      return annotateUnavailable(err instanceof Error ? err.message : String(err));
    }
  }

  /**
   * EvidenceManifest 持久化门面（任务书 §六十七，schema v15）：desktop-session-submit
   * 在持久化检索 stats 的同一位置调用。服务端复核（不信任模型/调用方传入 id）：
   * 一切轮身份从 stats.scopeId 反查 TurnScope 行复读；条目经
   * assembleKnowledgeEvidenceManifestEntries 与冻结集合核对后落库。写入失败
   * 抛错，由调用方留痕 warn（与 stats 持久化同纪律，不阻断会话提交）。
   * 只持久化身份链与标签，绝不存 chunk 正文或任何模型输出。
   */
  recordKnowledgeEvidenceManifest(input: {
    sessionPath: string;
    stats: KnowledgeRetrievalStats;
    evidence: KnowledgeInjectionEvidence;
  }): void {
    const knowledge = this._knowledge;
    const studioId = this._runtimeContext?.studioId;
    if (!knowledge || !studioId) {
      throw new Error("Knowledge is not accessible in this runtime");
    }
    if (typeof input?.stats?.scopeId !== "string" || !input.stats.scopeId) {
      throw new Error("knowledge evidence manifest requires a turn scope (stats.scopeId)");
    }
    if (!input?.evidence || !Array.isArray(input.evidence.entries)) {
      throw new Error("knowledge evidence manifest requires the injection evidence payload");
    }
    const scope = knowledge.getTurnScope({ scopeId: input.stats.scopeId });
    if (!scope) {
      throw new Error(`knowledge turn scope not found: ${input.stats.scopeId}`);
    }
    knowledge.insertEvidenceManifest({
      turnScopeId: scope.id,
      coverageMode: input.stats.coverageMode ?? null,
      executedCoverageMode: input.stats.executedCoverageMode ?? null,
      entries: assembleKnowledgeEvidenceManifestEntries({
        turnScope: scope,
        evidence: input.evidence,
      }),
    });
  }

  // ── Auxiliary Model Resolver (semantic slots) ──
  // 统一的语义 Slot 解析入口。业务层只认识 title/summarize/memory/vision/
  // knowledge/approval/guard，不再关心 utility/utility_large。
  resolveAuxiliaryModel(slot, options: any = {}) {
    const ctx = this._auxResolveContext(options);
    const config = this._auxResolver.resolveAuxiliaryModel(slot, ctx);
    return this._withAuxiliaryUsageAttribution(config, ctx);
  }
  async resolveAuxiliaryModelFresh(slot, options: any = {}) {
    const ctx = this._auxResolveContext(options);
    const config = await this._auxResolver.resolveAuxiliaryModelFresh(slot, ctx);
    return this._withAuxiliaryUsageAttribution(config, ctx);
  }
  async resolveAuxiliaryExecution(slot, options: any = {}) {
    const ctx = this._auxResolveContext(options);
    const execution = await this._auxResolver.resolveAuxiliaryExecution(slot, ctx);
    return this._withAuxiliaryUsageAttribution(execution, ctx);
  }
  listModelOperationModels(operation?: ModelOperation) {
    return this._models.providerRegistry.getOperationModelCatalog(operation || null);
  }
  resolveModelOperation(operation: ModelOperation) {
    return this._modelOperationResolver.resolveSync(operation);
  }
  resolveModelOperationFresh(operation: ModelOperation) {
    return this._modelOperationResolver.resolveFresh(operation);
  }
  _auxResolveContext(options: any = {}) {
    const ctx = { ...(options || {}) };
    if (!ctx.agentId && ctx.sessionPath) {
      const ownerAgentId = this.resolveSessionOwnership(ctx.sessionPath).agentId;
      if (ownerAgentId) ctx.agentId = ownerAgentId;
    }
    return ctx;
  }
  _withAuxiliaryUsageAttribution(config, ctx) {
    if (!config) return config;
    let usageSessionId = ctx.sessionId || null;
    if (!usageSessionId && ctx.sessionPath) {
      try {
        usageSessionId = this.getSessionIdForPath?.(ctx.sessionPath) || null;
      } catch {
        usageSessionId = null;
      }
    }
    return {
      ...config,
      usageLedger: this._usageLedger,
      usageAgentId: ctx.agentId || this.currentAgentId || null,
      usageSessionPath: ctx.sessionPath || null,
      usageSessionId,
    };
  }
  get auxResolver() { return this._auxResolver; }
  readAgentOrder() { return this._configCoord.readAgentOrder(); }
  saveAgentOrder(o) { return this._configCoord.saveAgentOrder(o); }
  async syncModelsAndRefresh() { return this._configCoord.syncAndRefresh(); }
  setPendingModel(id, provider) { return this._configCoord.setPendingModel(id, provider); }
  async switchSessionModel(sessionPath, modelId, provider) {
    if (!provider) {
      throw new Error(`switchSessionModel: provider required (modelId=${modelId})`);
    }
    const model = findModel(this._models.availableModels, modelId, provider);
    if (!model) throw new Error(t("error.modelNotFound", { id: `${provider}/${modelId}` }));
    return this._sessionCoord.switchSessionModel(sessionPath, model);
  }
  async setDefaultModel(id, provider, opts) { return this._configCoord.setDefaultModel(id, provider, opts); }
  getThinkingLevel() { return this._configCoord.getThinkingLevel(); }
  setThinkingLevel(l) { return this._configCoord.setThinkingLevel(l); }
  getDefaultThinkingLevel() { return this._sessionCoord.getDefaultThinkingLevel(); }
  setDefaultThinkingLevel(l) { return this._sessionCoord.setDefaultThinkingLevel(l); }
  getSessionThinkingLevel(sessionPath) { return this._sessionCoord.getSessionThinkingLevel(sessionPath); }
  setSessionThinkingLevel(sessionPath, level) { return this._sessionCoord.setSessionThinkingLevel(sessionPath, level); }
  getSandbox() { return this._prefs.getSandbox(); }
  setSandbox(v) { this._prefs.setSandbox(v); }
  getSandboxNetwork() {
    if (process.platform === "win32") return true;
    return this._prefs.getSandboxNetwork();
  }
  setSandboxNetwork(v) {
    const enabled = typeof v === "string" ? v === "true" : !!v;
    if (process.platform === "win32" && !enabled) {
      throw new Error("Windows command sandbox does not support network isolation; sandboxed commands keep network access.");
    }
    this._prefs.setSandboxNetwork(enabled);
  }
  getHardwareAcceleration() { return this._prefs.getHardwareAcceleration(); }
  setHardwareAcceleration(v) { this._prefs.setHardwareAcceleration(v); }
  compareAndDeleteLegacyHardwareAccelerationPreference() {
    return this._prefs.compareAndDeleteLegacyHardwareAccelerationPreference();
  }
  getFileBackup() { return this._prefs.getFileBackup(); }
  setFileBackup(p) { this._prefs.setFileBackup(p); }
  getFileHistoryService() { return this._fileHistory; }
  refreshFileHistoryWorkspaces() {
    try {
      const agents = this._agentMgr?.listAgents?.() || [];
      const roots = [];
      for (const agent of agents) {
        const agentId = agent?.id || agent?.agentId;
        if (!agentId) continue;
        const root = this.getExplicitHomeCwd?.(agentId) || this.getHomeCwd?.(agentId);
        if (root) roots.push(root);
      }
      void this._fileHistory.syncWorkspaces(roots);
    } catch (err) {
      console.warn(`[file-history] workspace sync failed: ${err.message}`);
    }
  }
  listCheckpoints() { return this._checkpointStore.list(); }
  restoreCheckpoint(id) { return this._checkpointStore.restore(id); }
  removeCheckpoint(id) { return this._checkpointStore.remove(id); }
  async createUserEditCheckpoint({ filePath, reason = "edit-start" }) {
    const cfg = this._prefs.getFileBackup();
    const id = await this._checkpointStore.save({
      sessionPath: null,
      tool: "user-edit",
      source: "user-edit",
      reason,
      filePath,
      maxSizeKb: cfg.max_file_size_kb || 1024,
    });
    return id ? { id, path: filePath, reason } : null;
  }
  cleanupCheckpoints() {
    const cfg = this._prefs.getFileBackup();
    return this._checkpointStore.cleanup(cfg.retention_days || 1);
  }
  getLearnSkills() { return this._prefs.getLearnSkills(); }
  setLearnSkills(p) { this._prefs.setLearnSkills(p); }
  getLocale() { return this._prefs.getLocale(); }
  setLocale(l) { this._prefs.setLocale(l); }
  getUserName() { return this._prefs.getUserName(); }
  setUserName(n) {
    this._prefs.setUserName(n);
    // 名字是全局的：改一次，所有已经加载的 agent 都要立刻改口，
    // 而不是等到进程重启才生效。
    this._agentMgr?.refreshResolvedUserNames?.();
  }
  getSetupComplete() { return this._prefs.getSetupComplete(); }
  markSetupComplete() { return this._prefs.markSetupComplete(); }
  getEditor() { return this._prefs.getEditor(); }
  setEditor(p) { return this._prefs.setEditor(p); }
  getAppearance() { return this._prefs.getAppearance(); }
  setAppearance(p) { return this._prefs.setAppearance(p); }
  getNotificationPreferences() { return this._prefs.getNotificationPreferences(); }
  setNotificationPreferences(p) { return this._prefs.setNotificationPreferences(p); }
  getQuickChatPreferences() { return this._prefs.getQuickChatPreferences(); }
  setQuickChatPreferences(p) { return this._prefs.setQuickChatPreferences(p); }
  getBrowserPreferences() { return this._prefs.getBrowserPreferences(); }
  setBrowserPreferences(p) { return this._prefs.setBrowserPreferences(p); }
  getWorkspaceUiState(workspaceRoot, surface) { return this._prefs.getWorkspaceUiState(workspaceRoot, surface); }
  setWorkspaceUiState(workspaceRoot, surface, state) { return this._prefs.setWorkspaceUiState(workspaceRoot, surface, state); }
  getInputDrafts(surface) { return this._inputDrafts.getAll(surface); }
  setHomeInputDraft(surface, entry) { return this._inputDrafts.setHome(surface, entry); }
  setSessionInputDraft(surface, sessionId, entry) { return this._inputDrafts.setSession(surface, sessionId, entry); }
  deleteSessionInputDrafts(sessionId) { return this._inputDrafts.deleteSession(sessionId); }
  gcWorkspacePersistence( options: any = {}) {
    const configResults = options?.agentId
      ? [this._configCoord.gcWorkspaceConfig(options.agentId, options)]
      : this._configCoord.gcAllWorkspaceConfigs(options);
    const workspaceUiState = this._prefs.gcWorkspaceUiState(options);
    return { configResults, workspaceUiState };
  }
  getSidebarUiPrefs() { return this._prefs.getSidebarUiPrefs(); }
  setSidebarUiPrefs(partial) { return this._prefs.setSidebarUiPrefs(partial); }
  getPluginUiPrefs() { return this._prefs.getPluginUiPrefs(); }
  setPluginUiPrefs(partial) { return this._prefs.setPluginUiPrefs(partial); }
  getPluginDevToolsEnabled() { return this._prefs.getPluginDevToolsEnabled(); }
  setPluginDevToolsEnabled(value) { return this._prefs.setPluginDevToolsEnabled(value); }
  getPluginInstallRecord(pluginId) { return this._pluginInstallRecords.get(pluginId); }
  recordPluginInstall(record) { return this._pluginInstallRecords.recordInstall(record); }
  removePluginInstallRecord(pluginId) { return this._pluginInstallRecords.remove(pluginId); }
  getTimezone() { return this._prefs.getTimezone(); }
  setTimezone(tz) { this._prefs.setTimezone(tz); }
  getUpdateChannel() { return this._prefs.getUpdateChannel(); }
  setUpdateChannel(ch) { this._prefs.setUpdateChannel(ch); }
  getAutoCheckUpdates() { return this._prefs.getAutoCheckUpdates(); }
  setAutoCheckUpdates(v) { this._prefs.setAutoCheckUpdates(v); }
  getKeepAwake() { return this._prefs.getKeepAwake(); }
  setKeepAwake(v) { this._prefs.setKeepAwake(v); }
  setMemoryMasterEnabled(id, v) { return this._configCoord.setMemoryMasterEnabled(id, v); }
  persistSessionMeta(sessionPath) { return this._configCoord.persistSessionMeta(sessionPath); }
  get permissionMode() { return this._sessionCoord.getPermissionMode(); }
  getSessionPermissionMode(sessionPath) { return this._sessionCoord.getPermissionMode(sessionPath); }
  setSessionPermissionMode(mode) { return this._sessionCoord.setPermissionMode(mode); }
  setSessionPermissionModeForSession(sessionPath, mode, options) { return this._sessionCoord.setSessionPermissionMode(sessionPath, mode, options); }
  setCurrentSessionPermissionMode(mode) { return this._sessionCoord.setCurrentSessionPermissionMode(mode); }
  setPendingSessionPermissionMode(mode) { return this._sessionCoord.setPendingPermissionMode(mode); }
  allowSessionInvocationCapability(ref, capability) { return this._sessionCoord.allowInvocationCapability(ref, capability); }
  // Read on every permission decision, including on engines built without a
  // session coordinator. No coordinator means no session ever granted anything,
  // so an empty list is the accurate answer and the fail-closed one.
  getSessionAllowedInvocationCapabilities(sessionPath) { return this._sessionCoord?.getAllowedInvocationCapabilities(sessionPath) || []; }
  getSessionPermissionModeDefault() { return this._sessionCoord.getPermissionModeDefault(); }
  setSessionPermissionModeDefault(mode) { return this._sessionCoord.setPermissionModeDefault(mode); }
  get accessMode() { return this._sessionCoord.getAccessMode(); }
  setAccessMode(mode) { return this._sessionCoord.setAccessMode(mode); }
  setPlanMode(enabled) { return this._sessionCoord.setPlanMode(enabled); }
  async updateConfig(p, opts) { return this._configCoord.updateConfig(p, opts); }

  getPreferences() { return this._readPreferences(); }
  savePreferences(p) { return this._writePreferences(p); }

  // ════════════════════════════
  //  Channel 代理（→ ChannelManager）
  // ════════════════════════════

  async createChannelEntry(input) { return this._channels.createChannelEntry(input); }
  async deleteChannelByName(n) { return this._channels.deleteChannelByName(n); }
  async triggerChannelDelivery(n, o) { return this._channels.triggerChannelDelivery(n, o); }
  async triggerChannelTriage(n, o) { return this.triggerChannelDelivery(n, o); }

  // ════════════════════════════
  //  Bridge 代理（→ BridgeSessionManager）
  // ════════════════════════════

  getBridgeIndex(agentId) {
    const agent = agentId ? this.getAgent(agentId) : undefined;
    return this._bridge.readIndex(agent);
  }
  saveBridgeIndex(i, agentId) {
    const agent = agentId ? this.getAgent(agentId) : undefined;
    return this._bridge.writeIndex(i, agent);
  }
  async executeExternalMessage(p, sk, m, o) { return this._bridge.executeExternalMessage(p, sk, m, o); }
  injectBridgeMessage(sk, t) { return this._bridge.injectMessage(sk, t); }
  /** 对指定 bridge session 执行真正的上下文压缩；返回 { tokensBefore, tokensAfter, contextWindow } */
  async compactBridgeSession(sessionKey, opts) { return this._bridge.compactSession(sessionKey, opts); }
  async freshCompactBridgeSession(sessionKey, opts) { return this._bridge.freshCompactSession(sessionKey, opts); }
  /**
   * 对桌面 session 做上下文压缩；返回 { tokensBefore, tokensAfter, contextWindow }
   * 供 /compact 在 /rc 接管态下给出 token delta 反馈（Phase 2-E）
   */
  async compactDesktopSession(sessionPath) {
    let session = this.getSessionByPath(sessionPath);
    if (!session) throw new Error("compactDesktopSession: session not found");
    if (session.isCompacting) throw new Error("compactDesktopSession: already compacting");
    let before = session.getContextUsage?.() ?? null;
    const compacted = await compactSessionWithCachePreservationRecoveringRuntime({
      session,
      sessionPath,
      customInstructions: undefined,
      reloadSessionRuntime: (path) => this.reloadSessionRuntime(path),
      onRuntimeReload: ({ session: reloadedSession }) => {
        if (reloadedSession.isCompacting) throw new Error("compactDesktopSession: already compacting");
        before = reloadedSession.getContextUsage?.() ?? before;
      },
    });
    session = compacted.session;
    this._sessionCoord._markSessionCompacted(sessionPath);
    const after = session.getContextUsage?.() ?? null;
    return {
      tokensBefore: before?.tokens ?? null,
      tokensAfter: after?.tokens ?? null,
      contextWindow: after?.contextWindow ?? before?.contextWindow ?? null,
    };
  }

  /**
   * #1624 显式刷新（fresh compact）：把旧对话压缩成 compact checkpoint，然后用
   * 当前 agent 配置重建 system prompt snapshot + 工具快照重启 session runtime。
   * 这是用户显式触发的升级动作——不点刷新的 session 永远保持冻结快照不变。
   *
   * "Already compacted / Nothing to compact" 视为 noop（快照仍然刷新），
   * 与 bridge fresh compact 的 markFreshCompactSatisfied 语义一致。
   */
  async freshCompactDesktopSession(sessionPath) {
    let session = this.getSessionByPath(sessionPath) || await this.ensureSessionLoaded(sessionPath);
    if (!session) throw new Error("freshCompactDesktopSession: session not found");
    if (session.isCompacting) throw new Error("freshCompactDesktopSession: already compacting");
    if (this.isSessionStreaming(sessionPath)) {
      throw new Error("freshCompactDesktopSession: session is streaming, try again after the reply completes");
    }
    let before = session.getContextUsage?.() ?? null;
    let noopReason = null;
    try {
      const compacted = await compactSessionWithCachePreservationRecoveringRuntime({
        session,
        sessionPath,
        customInstructions: undefined,
        reloadSessionRuntime: (path) => this.reloadSessionRuntime(path),
        onRuntimeReload: ({ session: reloadedSession }) => {
          if (reloadedSession.isCompacting) throw new Error("freshCompactDesktopSession: already compacting");
          before = reloadedSession.getContextUsage?.() ?? before;
        },
      });
      session = compacted.session;
    } catch (error) {
      noopReason = getFreshCompactNoopReason(error);
      if (!noopReason) throw error;
    }
    const after = session.getContextUsage?.() ?? null;
    if (!noopReason) this._sessionCoord._markSessionCompacted(sessionPath);
    // 压缩完成后整体重建 runtime：restore 路径按当前配置重算 prompt/tool 快照并写回 session-meta
    await this._sessionCoord.reloadSessionRuntime(sessionPath, { refreshCapabilitySnapshots: true });
    return {
      tokensBefore: before?.tokens ?? null,
      tokensAfter: noopReason ? (before?.tokens ?? null) : (after?.tokens ?? null),
      contextWindow: after?.contextWindow ?? before?.contextWindow ?? null,
      fresh: true,
      noopReason,
    };
  }

  // ════════════════════════════
  //  Skills（→ SkillManager）
  // ════════════════════════════

  _syncAgentSkills() { this._skills.syncAgentSkills(this.agent); }
  _syncAllAgentSkills() { for (const ag of this._agentMgr.agents.values()) this._skills.syncAgentSkills(ag); }
  getAllSkills(agentId) {
    // 不接受 fallback 到 this.agent — 调用方必须显式指定 agentId，
    // 否则前后端 agent 错位会导致 desk skill toggle 把错位列表写入当前 agent (#397)
    if (!agentId) throw new Error("getAllSkills requires explicit agentId");
    const ag = this._agentMgr.getAgent(agentId);
    if (!ag) throw new Error(`agent not found: ${agentId}`);
    return this._skills.getAllSkills(ag);
  }
  getRuntimeSkills(agentId) {
    if (!agentId) throw new Error("getRuntimeSkills requires explicit agentId");
    const ag = this._agentMgr.getAgent(agentId);
    if (!ag) throw new Error(`agent not found: ${agentId}`);
    return this._skills.getRuntimeSkillInfos(ag);
  }
  _getSkillsForAgent(ag, options = {}) { return this._skills.getSkillsForAgent(ag, options); }
  get skillsDir() { return this._skills?.skillsDir; }
  get userSkillsDir() { return this._skills?.skillsDir; }
  get modelsJsonPath() { return this._models.modelsJsonPath; }
  get authJsonPath() { return this._models.authJsonPath; }

  async reloadSkills() {
    await this._skills.reload(this._resourceLoader, this._agentMgr.agents);
    this._resourceLoader.getSystemPrompt = () => this.agent.systemPrompt;
    this._resourceLoader.getSkills = () => this._getSkillsForAgent(this.agent);
    this._syncAllAgentSkills();
  }

  /** 获取外部技能路径配置（供 API 使用） */
  getExternalSkillPaths() {
    // 刷新 exists 状态，检测运行期间新增的目录
    let newDirAppeared = false;
    for (const d of this._discoveredExternalPaths || []) {
      const nowExists = fs.existsSync(d.dirPath);
      if (nowExists && !d.exists) newDirAppeared = true;
      d.exists = nowExists;
    }
    // 运行期间有新目录出现：重新集成到 SkillManager（watcher + 扫描）
    if (newDirAppeared) {
      this.syncWorkspaceSkillPaths(this.currentSessionPath ? this.cwd : null, {
        reload: true,
        emitEvent: true,
        agentId: this.currentAgentId || null,
      }).catch(() => {});
    }
    return {
      configured: this._prefs.getExternalSkillPaths(),
      discovered: this._discoveredExternalPaths || [],
    };
  }

  /** 更新外部技能路径 + 同步 ResourceLoader + 重载 */
  async setExternalSkillPaths(paths) {
    this._prefs.setExternalSkillPaths(paths);
    await this.syncWorkspaceSkillPaths(this.currentSessionPath ? this.cwd : null, {
      reload: true,
      emitEvent: true,
      agentId: this.currentAgentId || null,
    });
  }

  /** 合并自动发现 + 用户配置的外部路径（去重） */
  _mergeExternalPaths(userConfiguredPaths, extraPaths = []) {
    // 每次合并时重新检测目录是否存在（不依赖初始化快照）
    for (const d of this._discoveredExternalPaths || []) {
      d.exists = fs.existsSync(d.dirPath);
    }
    const discovered = (this._discoveredExternalPaths || [])
      .filter(d => d.exists)
      .map(d => ({ dirPath: d.dirPath, label: d.label }));
    const userParsed = (userConfiguredPaths || []).map(p => ({
      dirPath: path.resolve(p),
      label: path.basename(path.dirname(p)),
    }));
    const merged = [...discovered];
    const seen = new Set(merged.map(m => m.dirPath));
    for (const up of [...userParsed, ...extraPaths]) {
      if (seen.has(up.dirPath)) continue;
      merged.push(up);
      seen.add(up.dirPath);
    }
    return merged;
  }

  _getWorkspaceExternalSkillPaths(cwd) {
    return resolveWorkspaceSkillCatalogPaths(cwd);
  }

  getWorkspaceSkillPolicy(agentOrId = null) {
    const agent = typeof agentOrId === "string"
      ? this._agentMgr.getAgent(agentOrId)
      : (agentOrId || this.agent);
    return workspaceSkillPolicyFromConfig(agent?.config?.workspace_context);
  }

  getActiveWorkspaceSkillPaths(cwd, agentOrId = null) {
    return resolveWorkspaceSkillPaths(cwd, this.getWorkspaceSkillPolicy(agentOrId));
  }

  syncAgentWorkspaceSkills(agentId) {
    const agent = this._agentMgr.getAgent(agentId);
    if (!agent || !this._skills) return false;
    this._skills.syncAgentSkills(agent);
    return true;
  }

  _getResolvedExternalSkillPaths(cwd) {
    const pluginPaths = this._pluginManager?.getSkillPaths?.() || [];
    const workspacePaths = this._getWorkspaceExternalSkillPaths(cwd);
    return this._mergeExternalPaths(this._prefs.getExternalSkillPaths(), [
      ...pluginPaths,
      ...workspacePaths,
    ]);
  }

  _sameExternalSkillPaths(a = [], b = []) {
    if (a.length !== b.length) return false;
    return a.every((entry, index) => {
      const other = b[index];
      return entry?.dirPath === other?.dirPath
        && entry?.label === other?.label
        && (entry?.scope || "") === (other?.scope || "")
        && (entry?.category || "") === (other?.category || "");
    });
  }

  async syncWorkspaceSkillPaths(cwd = null, options: any = {}) {
    const {
      reload = true,
      emitEvent = false,
      force = false,
      agentId = this._agentMgr?.activeAgentId || null,
    } = options;
    if (!this._skills) return false;
    const resolved = this._getResolvedExternalSkillPaths(cwd);
    const changed = !this._sameExternalSkillPaths(this._skills._externalPaths || [], resolved);
    if (!changed && !force) return false;

    this._skills.setExternalPaths(resolved);
    if (reload) await this.reloadSkills();
    if (emitEvent) this._emitAppEvent("skills-changed", { agentId: agentId || null });
    return true;
  }

  // ════════════════════════════
  //  Model 代理
  // ════════════════════════════

  _resolveThinkingLevel(l) { return this._models.resolveThinkingLevel(l); }
  _resolveExecutionModel(r) { return this._models.resolveExecutionModel(r); }
  readSavedProviderApiKey(provider) { return this._models.readSavedProviderApiKey(provider); }
  resolveProviderCredentialsFresh(p, options) { return this._models.resolveProviderCredentialsFresh(p, options); }
  resolveModelForValidation(ref) { return this._models.resolveModelForValidation(ref); }
  resolveModelWithCredentialsFresh(ref) { return this._models.resolveModelWithCredentialsFresh(ref); }
  async refreshAvailableModels() { return this._models.refreshAvailable(); }
  /**
   * Provider 配置变更后的统一操作序列。
   * reload registry → sync models.json → refresh available → normalize utility prefs
   * → 通知 active session 重新解析 model 对象（baseUrl 等字段烤在对象上）
   */
  async onProviderChanged() {
    await this._models.reloadAndSync();
    this._sessionCoord.refreshAllSessionsModels();
    // provider 变更的收敛点（providers 路由/hub/媒体管理都汇到此处）：
    // 凭证与模型目录变化可能改变嵌入模型可解析性 → 通知知识摄入管线补跑待嵌入 job。
    this._knowledge?.onModelConfigMayHaveChanged?.();
  }
  getRegistryModelsForProvider(name) { return this._models.getRegistryModelsForProvider(name); }

  static SHARED_MODEL_KEYS = SHARED_MODEL_KEYS;

  // ════════════════════════════
  //  生命周期
  // ════════════════════════════

  async init(log: any = () => {}) {
    const startupTimer = Date.now();

    // 0. 统一数据迁移（版本号驱动，新迁移统一加在 migrations.js）
    const registryStep = runBestEffortStartupMigrationStep("migration-registry", () => runMigrations({
      lingxiHome: this.lingxiHome,
      agentsDir: this.agentsDir,
      prefs: this._prefs,
      providerRegistry: this._models.providerRegistry,
      log,
    }), log);
    const migrationStatus = registryStep.ok ? registryStep.value : null;
    if (migrationStatus?.pendingIds.length > 0) {
      log(
        `[migrations] 应用继续启动；仍有 ${migrationStatus.pendingIds.length} 条迁移待重试：`
        + `#${migrationStatus.pendingIds.join(", #")}`,
      );
    }

    // 1. 凭证文件权限自愈。放在所有迁移之后，让本轮迁移刚写出的文件也被覆盖。
    // 每次启动都跑：权限会因为备份恢复、跨机拷贝、外部同步而回退，
    // 只跑一次的迁移覆盖不到这些情况。
    // 拆成两步：清理和矫正互不依赖，任一步出意外都不该连累另一步
    runBestEffortStartupMigrationStep("credential-backup-retention", () => {
      pruneStaleCredentialBackups({ lingxiHome: this.lingxiHome, log });
    }, log);
    runBestEffortStartupMigrationStep("credential-custody", () => {
      const healed = healCredentialFileModes({ lingxiHome: this.lingxiHome, log });
      if (healed.failed.length > 0) {
        log(`[credential-custody] ${healed.failed.length} 个文件未能收紧权限，已记录；应用继续启动`);
      }
    }, log);

    // 0f. 人格文件改名（旧 ishiki.md / public-ishiki.md → AGENTS.md /
    // AGENTS.public.md）。必须在 agent 初始化之前跑完，之后任何 persona 读取
    // 都只认新名字，回落链里不留双读。每次启动都跑：从备份或同步恢复回来的
    // 旧目录同样要在边界上被改过来。
    runBestEffortStartupMigrationStep("agents-md-rename", () => {
      const renames = migrateAgentPersonaFileNames({ agentsDir: this.agentsDir, log });
      // 失败记录必须成为运行时可消费状态：agent 初始化时凭它把仍留在磁盘上
      // 的旧人格文件作为本进程的临时 persona source，而不是静默回落到模板。
      this._failedPersonaRenames = buildFailedPersonaRenameIndex(renames.failedDetails);
      if (renames.failed.length > 0) {
        log(`[agents-md-rename] ${renames.failed.length} 个人格文件未能改名，本次运行将临时读取旧文件；应用继续启动，下次启动重试改名`);
      }
    }, log);

    this._runtimeContext = createServerRuntimeContext({
      lingxiHome: this.lingxiHome,
      appVersion: this.appVersion,
    });
    this._resources = new ResourceService({
      agentsDir: this.agentsDir,
      sessionFiles: this._sessionFiles,
      runtimeContext: this._runtimeContext,
    });
    this._resourceAccess = new ResourceAccessService({
      resourceService: this._resources,
      audit: (event) => appendSecurityAuditEvent(this.lingxiHome, event),
    });

    // 频道初始化和 agent 构造会调用 server-side i18n。locale 是 global
    // preference，必须在任何会写持久化文案的初始化逻辑前加载。
    loadLocale(this._prefs.getLocale());

    // 1. Pi SDK + 模型基础设施（必须在 agent init 之前，agent 需要解析记忆模型）
    log(`[init] 1/5 Pi SDK 初始化...`);
    await this._models.init();
    // 预填充 _availableModels，agent init 时需要解析 utility model
    await this._models.refreshAvailable();
    log(`[init] 1/5 AuthStorage + ModelRegistry + ${this._models.availableModels.length} 个模型就绪`);

    // 2. 初始化所有 agent
    log(`[init] 2/5 初始化所有 agent...`);
    await this._agentMgr.initAllAgents(log, this._agentMgr.activeAgentId);
    log(`[init] 2/5 ${this._agentMgr.agents.size} 个 agent 已就绪`);

    // 2b. 补齐频道游标投影（老用户升级兼容）。
    // repairChannelCursorProjection 扫描所有频道，为每个"已是成员且有 config"
    // 的 agent 把缺失的 last-read cursor 补进 channels.md，是按成员真相源
    // 重建投影的单一入口，覆盖缺 channels.md 的老 agent。
    await this._channels.repairChannelCursorProjection();

    // 2c. 知识库全局模型偏好迁移（一次性，幂等）。
    // 全局嵌入/重排配置已退役：读出旧偏好 → 写入所有未单独配置的笔记本列 →
    // 迁移成功后删除偏好键（先删后写会丢配置，顺序不可颠倒）。
    // 迁移值与旧解析链会解析出的引用一致，语义零变化，不触发重建。
    try {
      const legacyRefs = this._configCoord.peekLegacyKnowledgeModelPrefs();
      if (legacyRefs.embedding || legacyRefs.rerank) {
        const result = this._knowledge.migrateLegacyGlobalModelRefs({
          embeddingModelRef: legacyRefs.embedding,
          rerankModelRef: legacyRefs.rerank,
        });
        this._configCoord.clearLegacyKnowledgeModelPrefs();
        log(`[init] 2c 知识库全局模型偏好已迁移至 ${result.notebooksUpdated} 个笔记本`);
      }
    } catch (err) {
      log(`[init] 2c 知识库全局模型偏好迁移失败（下次启动重试）: ${err instanceof Error ? err.message : String(err)}`);
    }

    // 3. ResourceLoader + Skills
    log(`[init] 3/5 ResourceLoader 初始化...`);
    const t_rl = Date.now();
    const skillsDir = path.join(this.lingxiHome, "skills");
    fs.mkdirSync(skillsDir, { recursive: true });

    // 解析外部兼容技能路径
    const homeDir = os.homedir();
    this._discoveredExternalPaths = WELL_KNOWN_SKILL_PATHS.map(w => ({
      dirPath: path.join(homeDir, w.suffix),
      label: w.label,
      exists: fs.existsSync(path.join(homeDir, w.suffix)),
    }));
    const externalPaths = this._getResolvedExternalSkillPaths(null);

    this._skills = new SkillManager({ skillsDir, externalPaths });
    this._coreExtensionFactories = [
      /**
       * Provider payload 兼容化（chat 路径）。与 callText 共享 core/provider-compat.ts，
       * 是两条调用路径唯一的 normalize 入口——末端只在"流式 vs 非流式 fetch"分叉。
       *
       * ctx.model 是 Pi SDK 标准入参，正常 chat session 都会带；少数 edge case
       * （子 session / 工具内调）下偏 SDK 实现可能不带，此时只在 payload.model
       * 唯一匹配时补 model；重复 id 直接不猜 provider，避免错套 provider 兼容。
       */
      (pi) => {
        pi.on("context", (event, ctx) => {
          const model = ctx?.model;
          if (!model) return;
          const reasoningLevel = this.resolveRequestReasoningLevel(ctx);
          const messages = normalizeProviderContextMessages(event.messages, model, {
            mode: "chat",
            reasoningLevel,
          });
          if (messages === event.messages) return;
          return { messages };
        });

        pi.on("before_provider_request", (event, ctx) => {
          const p = event.payload;
          if (!p) return p;
          const requestModel = ctx?.model
            || findUniqueModelById(this._models.availableModels, p.model)
            || null;
          const reasoningLevel = this.resolveRequestReasoningLevel(ctx);
          const sessionPath = ctx?.sessionManager?.getSessionFile?.() || null;
          const {
            deepseekRoleplayReasoningPatch,
            deepseekRoleplayReasoningContext,
          } = this.getProviderCompatOptionsForSession(sessionPath);
          // The SDK hook exposes the serialized body, but not whether maxTokens came
          // from user intent or buildBaseOptions' model-derived default. Keep source
          // unspecified here; output-budget removes only values matching that SDK default.
          return normalizeProviderPayload(p, requestModel, {
            mode: "chat",
            // compaction 复用 session onPayload 扩展链；调用点用
            // runWithProviderCompatPurpose 显式标记，未标记的默认是普通聊天。
            purpose: currentProviderCompatPurpose(),
            reasoningLevel,
            deepseekRoleplayReasoningPatch,
            deepseekRoleplayReasoningContext,
          });
        });
      },
      /**
       * Capability-aware message adaptation：把历史里的 ImageContent block
       * 替换为 TextContent 占位，避免不支持 image 的 provider 反序列化失败
       * （如 issue #441：messages[N]: unknown variant `image_url`, expected `text`）。
       * 非静默降级：每个 session 首次剥图时通过事件总线通知 UI。
       */
      (pi) => {
        pi.on("context", (event, ctx) => {
          const model = ctx?.model;
          if (!model) return;
          const sessionPath = ctx?.sessionManager?.getSessionFile?.();
          const replaySafe = stripHistoricalInlineMediaForReplay(event.messages);
          const sessionRef = typeof this._sessionRefForPathSafe === "function"
            ? this._sessionRefForPathSafe(sessionPath)
            : { sessionId: null, sessionPath };
          const currentTurnMedia = this._currentTurnNativeMedia.inject(sessionRef, replaySafe.messages);
          const { messages, stripped, strippedImages, strippedVideos } = sanitizeMessagesForModel(currentTurnMedia.messages, model);
          if (replaySafe.stripped === 0 && stripped === 0 && !currentTurnMedia.changed) return;
          const sessionRuntimeKey = sessionPath ? this._sessionRuntimeKeyForPath(sessionPath) : null;
          if (sessionRuntimeKey && strippedImages > 0 && !this._imageStripNotified.has(sessionRuntimeKey)) {
            this._imageStripNotified.add(sessionRuntimeKey);
            this._emitEvent({
              type: "image_stripped_notice",
              modelId: model.id,
              modelProvider: model.provider,
              count: strippedImages,
            }, sessionPath);
          }
          if (sessionRuntimeKey && strippedVideos > 0 && !this._videoStripNotified.has(sessionRuntimeKey)) {
            this._videoStripNotified.add(sessionRuntimeKey);
            this._emitEvent({
              type: "video_stripped_notice",
              modelId: model.id,
              modelProvider: model.provider,
              count: strippedVideos,
            }, sessionPath);
          }
          return { messages };
        });
      },
    ];
    this._extensionFactories = [...this._coreExtensionFactories];
    this._resourceLoader = new DefaultResourceLoader({
      ...this._createResourceLoaderOptions(skillsDir),
      extensionFactories: this._extensionFactories,
    });
    await this._resourceLoader.reload();

    const HIDDEN_SKILLS = new Set(["canvas-design", "skill-creator", "skills-translate-temp"]);
    this._skills.init(this._resourceLoader, this._agentMgr.agents, HIDDEN_SKILLS);
    const extCount = this._skills.allSkills.filter(s => s.source === "external").length;
    log(`[init] 3/5 ResourceLoader 完成 (${Date.now() - t_rl}ms, ${this._skills.allSkills.length} skills${extCount ? `, ${extCount} external` : ""})`);

    this._resourceLoader.getSystemPrompt = () => this.agent.systemPrompt;
    this._resourceLoader.getSkills = () => this._getSkillsForAgent(this.agent);

    // 4. 模型发现
    log(`[init] 4/5 发现可用模型...`);
    try { await this.syncModelsAndRefresh(); } catch (err) { moduleLog.warn(`[init] syncModelsAndRefresh failed: ${err?.message}`); }
    await this._models.refreshAvailable();
    const availableModels = this._models.availableModels;
    log(`[init] 4/5 找到 ${availableModels.length} 个模型: ${availableModels.map(m => `${m.provider}/${m.id}`).join(", ")}`);
    if (availableModels.length === 0) {
      moduleLog.warn("⚠ 未找到可用模型，请在设置中配置 API key");
      this._models.defaultModel = null;
    } else {
      // migrations #5 之后 models.chat 必为 {id, provider} 对象；
      // 非对象说明 agent 从未配置过或 migration 未识别（added-models.yaml 里
      // 没对应 provider），保守视为未配置。
      const chatRef = this.agent.config.models?.chat;
      const ref = (typeof chatRef === "object" && chatRef?.id && chatRef?.provider) ? chatRef : null;
      if (!ref) {
        moduleLog.warn("⚠ 未配置 models.chat（或配置缺 provider），defaultModel 为 null");
        this._models.defaultModel = null;
      } else {
        const model = findModel(availableModels, ref.id, ref.provider);
        if (!model) {
          moduleLog.error(`⚠ 配置的模型 "${ref.provider}/${ref.id}" 不在可用列表中，defaultModel 为 null`);
          this._models.defaultModel = null;
        } else {
          this._models.defaultModel = model;
          log(`✿ 使用模型: ${model.name} (${model.provider})`);
        }
      }
    }

    // 5. Sync skills + watch skillsDir
    this._syncAllAgentSkills();
    this._skills.watch(this._resourceLoader, this._agentMgr.agents, () => {
      this._resourceLoader.getSystemPrompt = () => this.agent.systemPrompt;
      this._resourceLoader.getSkills = () => this._getSkillsForAgent(this.agent);
      this._syncAllAgentSkills();
      this._emitAppEvent("skills-changed", { agentId: null });
    });

    // 7. Bridge 孤儿清理
    try { this._bridge.reconcile(); } catch (err) { moduleLog.warn(`[init] bridge reconcile failed: ${err?.message}`); }

    // 8. 沙盒日志
    const sandboxEnabled = this._readPreferences().sandbox !== false;
    log(`✿ 沙盒${sandboxEnabled ? "已启用" : "已关闭"}`);

    // 9. 清理过期的 .ephemeral session 文件（>7 天）
    this._cleanEphemeralSessions();

    // 10. 文件历史：按各 agent 的工作区根目录建立/同步快照 watcher
    this.refreshFileHistoryWorkspaces();

    // 11. 知识摄入管线（替代 Phase 1 删除的 resumeResearchRuns 点位）：启动恢复
    // running 残留、接管 queued 存量；模型已于步骤 4 就绪，立即补跑待嵌入 job。
    // 模型/provider/嵌入偏好的运行期变更经 onProviderChanged / setSharedModels
    // 两个收敛点通知（见各自注释）。同点位启动 file 源 watcher：扫描全部活跃
    // file 源建目录级 watch（变化 → 防抖 → refreshFileSource → 自动入队摄入），
    // 以及生命周期维护（Phase 5：orphan Source 定时 GC + 零引用变体诊断）。
    // 关闭走 engine close → knowledge.close()（先停维护再停 watcher 再停摄入）。
    try {
      this._knowledge.ingestion.start();
      this._knowledge.startSourceFileWatcher();
      this._knowledge.startLifecycleMaintenance();
      this._knowledge.onModelConfigMayHaveChanged();
    } catch (err) {
      moduleLog.warn(`[init] knowledge ingestion start failed: ${err?.message}`);
    }

    const totalTime = ((Date.now() - startupTimer) / 1000).toFixed(1);
    log(`✿ 初始化完成（${totalTime}s）`);
  }

  /** 清理所有 agent 的 .ephemeral/ 目录中超过 7 天的文件 */
  _cleanEphemeralSessions() {
    const maxAge = 7 * 24 * 60 * 60 * 1000;
    const now = Date.now();
    try {
      if (!this.agentsDir || !fs.existsSync(this.agentsDir)) return;
      for (const entry of fs.readdirSync(this.agentsDir, { withFileTypes: true })) {
        if (!entry.isDirectory()) continue;
        const ephDir = path.join(this.agentsDir, entry.name, '.ephemeral');
        if (!fs.existsSync(ephDir)) continue;
        for (const file of fs.readdirSync(ephDir)) {
          if (!file.endsWith('.jsonl')) continue;
          const filePath = path.join(ephDir, file);
          try {
            const stat = fs.statSync(filePath);
            if (now - stat.mtimeMs > maxAge) fs.unlinkSync(filePath);
          } catch {
            // 过期 ephemeral 文件清理 best-effort：单个文件 stat/unlink 失败跳过即可，下次启动再试。
          }
        }
      }
    } catch {
      // 整体清理 best-effort：扫描 agents 目录失败不应阻塞 init 后续步骤。
    }
  }

  async dispose() {
    try {
      // 先卸载 plugins（它们可能依赖 engine 资源）
      if (this._pluginManager) {
        for (const p of this._pluginManager.listPlugins()) {
          if (p.status === "loaded") {
            await this._pluginManager.unloadPlugin(p.id, { pluginKey: p.pluginKey });
          }
        }
      }
      this._pluginDevEventBusCleanup?.();
      this._pluginDevEventBusCleanup = null;
      this._media?.dispose?.();
      await this._mcp?.dispose?.();
      this._skills?.unwatch();
      this._deferredResultCoordinator?.dispose?.();
      this._deferredResultCoordinator = null;
      this._loopAlarm?.dispose?.();
      this._loopController?.dispose?.();
      await this._agentMgr.disposeAll(this._sessionCoord);
      await this._sessionCoord.cleanupSession();
    } finally {
      try {
        await this.disposeComputerRuntime();
      } finally {
        try {
          // Phase 7：bounded flush 后关闭 observability store（任务书 §四十五：
          // 必须有 bounded timeout，不能无限阻塞退出）。
          this._modelObservability = null;
          try {
            await this._modelObservabilityGenerations?.close?.();
          } catch {
            // observability 关闭失败不阻塞退出
          }
          // Phase 8：read-only query facade 一并释放（§九十一生命周期）。
          try {
            this._modelObservabilityQuery?.close?.();
          } catch {
            // query facade 关闭失败不阻塞退出
          }
        } finally {
          try {
            await this._knowledge?.close?.();
          } finally {
            this._sessionManifestStore?.close?.();
          }
        }
      }
    }
  }

  // ════════════════════════════
  //  插件系统
  // ════════════════════════════

  /**
   * Initialize plugin system. Called after Hub construction (EventBus available).
   * @param {import('../hub/event-bus.ts').EventBus} bus
   */
  async initPlugins(bus) {
    this._media?.start?.(bus);
    await this._mcp?.start?.(bus);
    const builtinPluginsDir = path.join(this.productDir, "..", "plugins");
    const userPluginsDir = path.join(this.lingxiHome, "plugins");
    const devPluginsDir = path.join(this.lingxiHome, "plugins-dev");
    const pluginDevRunsDir = path.join(this.lingxiHome, "plugin-dev-runs");
    const pluginDevSourcesDir = path.join(this.lingxiHome, "plugin-dev-sources");
    const pluginDataDir = path.join(this.lingxiHome, PLUGIN_DATA_DIRNAME);
    fs.mkdirSync(pluginDevSourcesDir, { recursive: true });

    // Read app version for plugin compatibility check
    let appVersion = "0.0.0";
    try {
      const pkgPath = path.join(this.productDir, "..", "package.json");
      appVersion = JSON.parse(fs.readFileSync(pkgPath, "utf-8")).version || "0.0.0";
    } catch {
      // package.json 缺失/损坏时沿用上面的 "0.0.0" 默认值，仅用于插件兼容性检查。
    }
    this.appVersion = appVersion;

    this._pluginManager = new PluginManager({
      pluginsDirs: [builtinPluginsDir, userPluginsDir],
      pluginsDir: undefined,
      dataDir: pluginDataDir,
      bus,
      preferencesManager: this._prefs,
      appVersion,
      getSessionPath: () => this.currentSessionPath,
      registerSessionFile: (entry) => this.registerSessionFile(entry),
      emitResourceChanged: (input) => this.emitResourceChanged(input),
      resourceIO: () => this.getResourceIO(),
      resourceWatch: {
        subscribe: (input) => this.subscribeResourceWatch(input),
        unsubscribe: (subscriptionId) => this.unsubscribeResourceWatch(subscriptionId),
      },
      slashRegistry: this._slashSystem?.registry ?? null,
      loadTimeoutMs: undefined,
      lifecycleTimeoutMs: undefined,
      logSink: (entry) => this._pluginDevService?.recordLog(entry),
      runtimeContext: this.getRuntimeContext(),
    });
    const allowedPluginDevSourceRoots = [
      pluginDevSourcesDir,
      this.homeCwd,
      process.cwd(),
      path.resolve(this.productDir, ".."),
    ].filter((dir) => typeof dir === "string" && dir.trim());
    this._pluginDevService = new PluginDevService({
      pluginManager: this._pluginManager,
      devPluginsDir,
      runDataDir: pluginDevRunsDir,
      allowedSourceRoots: allowedPluginDevSourceRoots,
      syncPluginExtensions: () => this.syncPluginExtensions(),
    });
    this._pluginDevEventBusCleanup?.();
    this._pluginDevEventBusCleanup = this._pluginDevService.registerEventBusHandlers(bus);
    this._pluginManager.scan();
    await this._pluginManager.loadAll();

    let providerContributionsChanged = false;
    for (const provider of this._pluginManager.getProviderPlugins()) {
      this._models.providerRegistry.registerProviderContribution(provider);
      providerContributionsChanged = true;
    }
    if (providerContributionsChanged) {
      await this._models.reloadAndSync();
    }

    if (this._skills) {
      await this.syncWorkspaceSkillPaths(this.currentSessionPath ? this.cwd : null, {
        reload: true,
        emitEvent: false,
      });
    }

    // Inject plugin extension factories into ResourceLoader (same array reference)
    await this.syncPluginExtensions();
  }

  /**
   * 同步插件 extension factories 到 ResourceLoader 共享数组。
   * 原地 splice 保持数组引用不变（DefaultResourceLoader 持有同一引用）。
   * 在 initPlugins 以及任何插件热操作后调用。
   */
  _syncExtensionFactories() {
    if (!this._extensionFactories) return;
    const coreFactories = this._coreExtensionFactories || [];
    const frameworkFactories = this._frameworkExtFactories || [];
    const pluginFactories = this._pluginManager?.getExtensionFactories() || [];
    this._extensionFactories.splice(0, Infinity, ...coreFactories, ...frameworkFactories, ...pluginFactories);
  }

  async _reloadResourceLoaderForExtensionFactories() {
    if (!this._resourceLoader?.reload) return;
    await this._resourceLoader.reload();
  }

  /**
   * Register a framework-level extension factory.
   * Tracked separately so _syncExtensionFactories preserves them across plugin hot-reloads.
   * Only affects sessions created after this call.
   */
  async registerExtensionFactory(factory) {
    if (!this._extensionFactories) return;
    if (!this._frameworkExtFactories) this._frameworkExtFactories = [];
    this._frameworkExtFactories.push(factory);
    await this.syncPluginExtensions();
  }

  get pluginManager() { return this._pluginManager; }
  get pluginDevService() { return this._pluginDevService; }

  /** 插件热操作后调用，同步 extension factories 到 ResourceLoader */
  async syncPluginExtensions() {
    this._syncExtensionFactories();
    await this._reloadResourceLoaderForExtensionFactories();
  }

  // ════════════════════════════
  //  工具构建
  // ════════════════════════════

  /**
   * Decide whether this tool set defers, and build the catalog if it does.
   *
   * Returns null for the ordinary case: few enough tools that loading them all
   * costs less than the machinery to avoid it. The count is per tool across all
   * servers, and excludes tools that cannot defer (pinned by the user, or
   * declared non-deferrable), because those stay in the prefix either way.
   *
   * Deferral is all-or-nothing across servers on purpose. Deferring only the
   * larger connectors would make a tool's availability depend on which company
   * shipped it, which is exactly the kind of hidden ranking the catalog avoids.
   */
  _planDeferredToolAssembly(mcpTargets, pluginTargets, invocationGateway) {
    const config = this._mcp?.getConfig?.() || null;
    const deferEnabled = config ? config.deferEnabled !== false : true;
    if (!deferEnabled) return null;
    const threshold = Number.isSafeInteger(config?.deferThreshold) && config.deferThreshold > 0
      ? config.deferThreshold
      : DEFAULT_TOOL_DEFER_THRESHOLD;

    const liveMcpEntries = mcpTargets
      .filter(({ target }) => target.availability.eligible)
      .map(({ descriptor }) => descriptor.catalogMetadata);

    const builtinDeferEnabled = this._prefs?.getBuiltinToolDeferEnabled?.() === true;
    const eligiblePluginTargets = builtinDeferEnabled
      ? pluginTargets.filter(({ target }) => target.availability.eligible).map(({ target }) => target)
      : [];
    const builtinEntries = eligiblePluginTargets
      .filter((target) => target.deferrable)
      .map((target) => ({
        targetId: target.identity.targetId,
        origin: target.identity.origin,
        sourceId: target.identity.sourceId,
        publicName: target.identity.publicName,
        toolName: target.identity.localName,
        capabilityBase: target.identity.capabilityBase,
        description: target.description,
        paramsSummary: summarizeToolParameters(target.parameters),
        serverId: target.identity.sourceId,
        serverLabel: target.identity.sourceId,
        lifecycleGeneration: target.lifecycleGeneration,
        deferrable: target.deferrable,
        pinned: target.pinned,
        schemaRef: () => target.parameters,
      }));

    const deferrable = [...liveMcpEntries, ...builtinEntries]
      .filter((entry) => entry.deferrable !== false && entry.pinned !== true);
    if (deferrable.length <= threshold) return null;

    const catalog = createToolCatalog();
    // Pinned tools are registered too: the model should be able to see that
    // they exist and read their schema, they simply also stay loaded.
    if (liveMcpEntries.length > 0) catalog.registerSource("mcp", liveMcpEntries);
    if (builtinEntries.length > 0) catalog.registerSource("builtin", builtinEntries);

    const deferredPluginTargetIds = new Set(
      eligiblePluginTargets
        .filter((target) => target.deferrable && !target.pinned)
        .map((target) => target.identity.targetId),
    );
    const deferredMcpTargetIds = new Set(
      mcpTargets
        .map(({ target }) => target)
        .filter((target) => target.availability.eligible && target.deferrable && !target.pinned)
        .map((target) => target.identity.targetId),
    );
    const bridgeTools = createBridgeTools({
      catalog,
      gateway: invocationGateway,
      log: toolAvailabilityLog,
    });

    return {
      catalog,
      bridgeTools,
      deferredPluginTargetIds,
      deferredMcpTargetIds,
      invocationGateway,
    };
  }

  /**
   * Catalog rows for the MCP tools currently published. Only tools that are
   * actually published can be deferred, so a row in config that never made it
   * to a live listing is not a catalog entry.
   */
  _liveMcpCatalogEntries(mcpTools = null) {
    const descriptors = typeof this._mcp?.getToolTargetDescriptors === "function"
      ? (this._mcp.getToolTargetDescriptors() || [])
      : [];
    const published = new Set(
      (Array.isArray(mcpTools) ? mcpTools : (this._mcp?.getAllTools?.() || []))
        .map((tool) => tool?.name),
    );
    return descriptors
      .filter((descriptor) => published.has(descriptor.publishedTool?.name))
      .map((descriptor) => descriptor.catalogMetadata);
  }

  /**
   * The catalog's tool names as they stand right now, for a session to compare
   * against the listing it was given. Returns null when there is no MCP manager
   * to ask, which reads as "no basis to claim anything changed".
   */
  getLiveToolCatalogNames() {
    if (!this._mcp) return null;
    return this._liveMcpCatalogEntries()
      .map((entry) => entry.publicName)
      .sort((left, right) => left.localeCompare(right));
  }

  buildTools(cwd, customTools, opts: any = {}) {
    // Executable background runtimes bind one persisted identity snapshot at assembly time.
    // Desktop chat keeps the callback path until it moves into the same session factory.
    const runtimeSessionRef = freezeRuntimeSessionRef(
      opts.runtimeSessionRef,
      opts.requireSessionIdentity === true,
    );
    let ct = customTools;
    const extraCustomTools = Array.isArray(opts.extraCustomTools)
      ? opts.extraCustomTools.filter((tool) => tool && typeof tool.name === "string" && tool.name.trim())
      : [];
    let agentId;
    let toolAgent;
    if (!ct) {
      // 通过 opts.agentDir 反查 agent 实例，避免隐式依赖焦点 agent
      if (opts.agentDir) {
        const dirAgentId = path.basename(opts.agentDir);
        const dirAgent = this.getAgent(dirAgentId);
        if (!dirAgent) throw new Error(`buildTools: agent "${dirAgentId}" not found`);
        ct = dirAgent.tools;
        agentId = dirAgentId;
        toolAgent = dirAgent;
      } else {
        ct = this.agent.tools;
        agentId = this.agent?.id || "";
        toolAgent = this.agent;
      }
    } else {
      agentId = opts.agentDir ? path.basename(opts.agentDir) : (this.agent?.id || "");
      toolAgent = opts.agentDir ? this.getAgent(agentId) : this.agent;
    }
    const baseCustomTools = Array.isArray(ct) ? ct : [];
    ct = [...baseCustomTools, ...extraCustomTools];
    const getSessionPath = runtimeSessionRef
      ? (() => runtimeSessionRef.sessionPath)
      : typeof opts.getSessionPath === "function"
        ? opts.getSessionPath
        : (() => null);
    const getSessionRef = runtimeSessionRef
      ? (() => runtimeSessionRef)
      : typeof opts.getSessionRef === "function"
        ? opts.getSessionRef
        : (() => null);
    const getSessionId = runtimeSessionRef
      ? (() => runtimeSessionRef.sessionId)
      : typeof opts.getSessionId === "function"
        ? opts.getSessionId
        : (() => null);
    const resolveRuntimeSessionRef = (runtimeCtx) => {
      const resolved = resolveToolSessionRef(runtimeCtx, {
        getSessionRef,
        getSessionId,
        getSessionPath,
        getSessionIdForPath: (sessionPath) => this.getSessionIdForPath(sessionPath),
      });
      return runtimeSessionRef || resolved;
    };
    const allowHumanApproval = opts.allowHumanApproval !== false;
    const approvalPolicy = opts.approvalPolicy
      || (allowHumanApproval ? SESSION_APPROVAL_POLICIES.INTERACTIVE : SESSION_APPROVAL_POLICIES.DENY_ON_PROMPT);

    const rawPluginTools = this._pluginManager?.getAllTools() || [];
    const rawMcpTools = this._mcp?.getAllTools() || [];
    const rawMcpDescriptors = typeof this._mcp?.getToolTargetDescriptors === "function"
      ? (this._mcp.getToolTargetDescriptors() || [])
      : [];
    const toolAvailabilityContext = {
      agentId,
      channelsEnabled: resolveChannelsEnabledForToolAvailability(this),
    };
    const toolAvailabilityOptions = { warn: (msg) => toolAvailabilityLog.warn(msg) };
    // 延迟阈值只能看到当前会话真正有资格使用的目标，不能先把禁用项算进去再过滤。
    const pluginTools = filterToolObjectsByAvailability(
      rawPluginTools,
      toolAgent?.config || {},
      toolAvailabilityContext,
      toolAvailabilityOptions,
    );
    const describedMcpTools = new Set(rawMcpDescriptors.map((descriptor) => descriptor.publishedTool));
    const mcpHostTools = filterToolObjectsByAvailability(
      rawMcpTools.filter((tool) => !describedMcpTools.has(tool)),
      toolAgent?.config || {},
      toolAvailabilityContext,
      toolAvailabilityOptions,
    );
    const executionBoundary = this._runtimeContext
      ? this.createExecutionBoundary({ workbenchRoot: cwd })
      : null;
    const executionScope = executionBoundary
      ? { serverNodeId: executionBoundary.serverNodeId, executionBoundary }
      : {};
    const withRuntimeContext = (tool) => {
      if (!tool?.execute) return tool;
      return {
        ...tool,
        execute: (toolCallId, params, signalOrRuntimeCtx, onUpdate, piCtx) => {
          const { ctx: runtimeCtx } = normalizeToolRuntimeContext(signalOrRuntimeCtx, piCtx);
          const runtimeSessionPath = runtimeCtx?.sessionPath
            || getToolSessionPath(runtimeCtx)
            || getSessionPath()
            || null;
          const sessionRef = resolveRuntimeSessionRef(runtimeCtx);
          const sessionPath = runtimeSessionPath || sessionRef?.sessionPath || null;
          const mergedCtx = {
            ...runtimeCtx,
            ...(sessionRef ? { sessionId: sessionRef.sessionId, sessionRef } : {}),
            ...(sessionPath ? { sessionPath } : {}),
            ...(opts.bridgeContext ? { bridgeContext: opts.bridgeContext } : {}),
            ...(opts.notificationContext ? { notificationContext: opts.notificationContext } : {}),
            allowHumanApproval,
            approvalPolicy,
            agentId,
            ...executionScope,
          };
          return tool.execute(toolCallId, params, signalOrRuntimeCtx, onUpdate, mergedCtx);
        },
      };
    };
    const targetRegistry = new ToolTargetRegistry();
    const invocationGateway = new ToolInvocationGateway({
      registry: targetRegistry,
      authorize: () => {
        throw new Error("model-facing plugin calls must be authorized by the session permission wrapper");
      },
    });
    const registeredPluginTargets = pluginTools.map((tool) => {
      const runtimeTool = withRuntimeContext(tool);
      const identity = tool._toolTargetIdentity;
      const permission = tool._normalizedPermissionContract;
      if (!identity || !permission) {
        throw new Error(`buildTools: plugin tool "${tool?.name || "unknown"}" lacks canonical identity or permission`);
      }
      const validator = createToolSchemaValidator(
        tool.parameters || { type: "object", properties: {} },
        identity,
      );
      const availability = evaluateToolAvailability(
        tool,
        toolAgent?.config || {},
        toolAvailabilityContext,
        toolAvailabilityOptions,
      );
      const target = targetRegistry.register({
        identity,
        label: tool.label || tool.name,
        description: tool.description || "",
        parameters: validator.schema,
        deferrable: tool.deferrable !== false,
        pinned: tool.pinned === true,
        permission,
        validator,
        availability: availability.allowed === false
          ? { eligible: false, reason: availability.reason }
          : { eligible: true },
        getCurrentGeneration: () => tool._toolLifecycleGeneration ?? 0,
        isCurrentlyAvailable: (runtimeContext) => evaluateToolAvailability(
          tool,
          toolAgent?.config || {},
          {
            ...toolAvailabilityContext,
            ...(runtimeContext && typeof runtimeContext === "object" ? runtimeContext : {}),
          },
          toolAvailabilityOptions,
        ).allowed,
        executeCanonical: (toolCallId, args, signal, onUpdate, ctx) => (
          runtimeTool.execute(toolCallId, args, signal, onUpdate, ctx)
        ),
        normalizeResult: (result) => result,
      });
      return { target, runtimeTool };
    });

    const registeredMcpTargets = rawMcpDescriptors.map((descriptor) => {
      const tool = descriptor.publishedTool;
      const runtimeTool = withRuntimeContext(tool);
      const identity = descriptor.identity;
      const permission = descriptor.permission;
      if (!tool || !identity || !permission || typeof descriptor.evaluateEligibility !== "function") {
        throw new Error("buildTools: MCP target descriptor is incomplete");
      }
      const validator = createToolSchemaValidator(
        tool.parameters || { type: "object", properties: {} },
        identity,
      );
      const eligibility = descriptor.evaluateEligibility(
        toolAgent?.config || {},
        { surface: "model", ...toolAvailabilityContext },
      );
      const target = targetRegistry.register({
        identity,
        label: tool.label || identity.publicName,
        description: tool.description || "",
        parameters: validator.schema,
        deferrable: descriptor.catalogMetadata.deferrable,
        pinned: descriptor.catalogMetadata.pinned,
        permission,
        validator,
        availability: eligibility?.eligible === true
          ? { eligible: true }
          : {
            eligible: false,
            reason: eligibility?.reason || "mcp_unavailable",
            code: eligibility?.code,
          },
        getCurrentGeneration: descriptor.getCurrentGeneration,
        isCurrentlyAvailable: descriptor.isCurrentlyAvailable,
        executeCanonical: (toolCallId, args, signal, onUpdate, ctx) => (
          runtimeTool.execute(toolCallId, args, signal, onUpdate, ctx)
        ),
        normalizeResult: (result) => result,
      });
      return { target, runtimeTool, descriptor };
    });

    const createDirectTargetFacade = ({ target, runtimeTool }) => ({
      ...runtimeTool,
      name: target.identity.publicName,
      label: target.label,
      description: target.description,
      parameters: target.parameters,
      deferrable: target.deferrable,
      pinned: target.pinned,
      _toolLifecycleGeneration: target.lifecycleGeneration,
      _toolTargetIdentity: target.identity,
      _normalizedPermissionContract: target.permission,
      sessionPermission: Object.freeze({
        resolveInvocation: target.permission.resolveInvocation,
      }),
      execute: (toolCallId, args, signalOrRuntimeCtx, onUpdate, piCtx) => {
        const { ctx: runtimeCtx } = normalizeToolRuntimeContext(signalOrRuntimeCtx, piCtx);
        const sessionRef = resolveRuntimeSessionRef(runtimeCtx);
        const signal = signalOrRuntimeCtx
          && typeof signalOrRuntimeCtx === "object"
          && typeof signalOrRuntimeCtx.aborted === "boolean"
          && typeof signalOrRuntimeCtx.addEventListener === "function"
          ? signalOrRuntimeCtx
          : undefined;
        return invocationGateway.invoke({
          targetId: target.identity.targetId,
          route: "direct",
          arguments: args,
          sessionId: runtimeCtx.sessionId || sessionRef?.sessionId || null,
          sessionPath: runtimeCtx.sessionPath || sessionRef?.sessionPath || getSessionPath(),
          agentId: runtimeCtx.agentId || agentId,
          lifecycleGeneration: target.lifecycleGeneration,
          toolCallId,
          signal,
          onUpdate,
          ctx: runtimeCtx,
          runtimeContext: runtimeCtx,
        });
      },
    });

    // Deferred assembly is decided once, here, and never revisited for the life
    // of this tool set. The session's cacheable prefix is the tool schemas plus
    // the system prompt, and a running session asserts that prefix on every
    // request, so a tool set that changed shape mid-session would break the
    // cache and fail the contract. Everything dynamic goes through the
    // conversation stream instead.
    const deferPlan = this._planDeferredToolAssembly(
      registeredMcpTargets,
      registeredPluginTargets,
      invocationGateway,
    );
    const directMcpTargets = deferPlan
      ? registeredMcpTargets.filter(({ target }) => (
        !deferPlan.deferredMcpTargetIds.has(target.identity.targetId)
      ))
      : registeredMcpTargets;
    const directPluginTargets = deferPlan
      ? registeredPluginTargets.filter(({ target }) => (
        !deferPlan.deferredPluginTargetIds.has(target.identity.targetId)
      ))
      : registeredPluginTargets;
    const directMcpTools = directMcpTargets.map(createDirectTargetFacade);
    const directPluginTools = directPluginTargets.map(createDirectTargetFacade);
    const bridgeTools = deferPlan ? deferPlan.bridgeTools : [];

    const runtimeCustomTools = ct.map(withRuntimeContext);
    const wrappedMcpHostTools = mcpHostTools.map(withRuntimeContext);
    const wrappedBridgeTools = bridgeTools.map(withRuntimeContext);
    if (deferPlan) {
      // withRuntimeContext returns copies, and the permission layer keys its
      // delegation registry on object identity, so the objects that actually
      // reach that layer are the ones that must be registered.
      registerBridgeCapabilityDelegates(wrappedBridgeTools, {
        gateway: deferPlan.invocationGateway,
      });
    }
    const pluginDevTools = this._pluginDevService && this._prefs.getPluginDevToolsEnabled?.() === true
      ? createPluginDevTools({
          pluginDevService: this._pluginDevService,
          getAgentId: () => agentId,
        })
      : [];
    assertUniqueBuiltToolNames([
      { source: "custom tools", tools: baseCustomTools },
      { source: "extra custom tools", tools: extraCustomTools },
      { source: "plugin tools", tools: directPluginTools },
      { source: "mcp tools", tools: [...directMcpTools, ...mcpHostTools] },
      { source: "mcp bridge tools", tools: bridgeTools },
      { source: "plugin development tools", tools: pluginDevTools },
    ]);
    const allTools = filterToolObjectsByAvailability(
      [
        ...runtimeCustomTools,
        ...directPluginTools,
        ...directMcpTools,
        ...wrappedMcpHostTools,
        ...wrappedBridgeTools,
        ...pluginDevTools,
      ],
      toolAgent?.config || {},
      toolAvailabilityContext,
      toolAvailabilityOptions,
    );

    const effectiveAgentDir = opts.agentDir || this.agent.agentDir;
    const effectiveWorkspace = opts.workspace !== undefined ? opts.workspace : this.homeCwd;
    const workspaceFolders = opts.workspaceFolders || [];
    const staticAuthorizedFolders = Array.isArray(opts.authorizedFolders) ? opts.authorizedFolders : [];
    const getAuthorizedFolders = typeof opts.getAuthorizedFolders === "function"
      ? () => {
          const folders = opts.getAuthorizedFolders();
          return Array.isArray(folders) ? folders : [];
        }
      : () => staticAuthorizedFolders;
    const fileReadSessionPaths = Array.isArray(opts.fileReadSessionPaths)
      ? opts.fileReadSessionPaths.filter((sp) => typeof sp === "string" && sp.trim())
      : [];
    const resolveRuntimeSessionFile = (fileId, options: any = {}) => {
      const activeSessionPath = getSessionPath() || null;
      if (activeSessionPath) {
        const activeFile = this.resolveActiveSessionFile({
          fileId,
          sessionPath: activeSessionPath,
        });
        if (activeFile) return activeFile;
      }

      const explicitOptions = this._sessionFileOptionsWithLocator(options);
      const explicitSessionPath = explicitOptions.sessionPath || null;
      if (
        explicitSessionPath
        && explicitSessionPath !== activeSessionPath
        && fileReadSessionPaths.includes(explicitSessionPath)
      ) {
        return this.resolveActiveSessionFile({
          fileId,
          sessionId: explicitOptions.sessionId || null,
          sessionPath: explicitSessionPath,
        });
      }
      return null;
    };
    const getExternalReadPaths = () => {
      const sessionPaths = [];
      const seenSessionPaths = new Set();
      const addSessionPath = (sp) => {
        if (!sp || seenSessionPaths.has(sp)) return;
        seenSessionPaths.add(sp);
        sessionPaths.push(sp);
      };
      addSessionPath(getSessionPath());
      for (const sp of fileReadSessionPaths) addSessionPath(sp);
      if (!sessionPaths.length) return [];
      const files = typeof this.listActiveSessionFiles === "function"
        ? sessionPaths.flatMap((sp) => this.listActiveSessionFiles(sp))
        : [];
      return externalReadPathsFromSessionFiles(files, {
        workspaceRoots: workspaceRootsForSandbox(effectiveWorkspace, workspaceFolders, getAuthorizedFolders()),
        lingxiHome: this.lingxiHome,
      });
    };

    const resourceIO = createSandboxResourceIO({
      cwd,
      agentDir: effectiveAgentDir,
      workspace: effectiveWorkspace,
      workspaceFolders,
      authorizedFolders: staticAuthorizedFolders,
      getAuthorizedFolders,
      lingxiHome: this.lingxiHome,
      getSandboxEnabled: () => this._readPreferences().sandbox !== false,
      getExternalReadPaths,
      getSessionPath,
      emitEvent: (event, sessionPath) => this._emitEvent(event, sessionPath),
      eventBus: this._resourceEvents(),
      resolveSessionFile: resolveRuntimeSessionFile,
      resourceService: this._resources || null,
      studioId: this._runtimeContext?.studioId || null,
    });
    let result = createSandboxedTools(cwd, allTools, {
      agentDir: effectiveAgentDir,
      workspace: effectiveWorkspace,
      workspaceFolders,
      authorizedFolders: staticAuthorizedFolders,
      getAuthorizedFolders,
      lingxiHome: this.lingxiHome,
      executionBoundary,
      getSandboxEnabled: () => this._readPreferences().sandbox !== false,
      getSandboxNetworkEnabled: () => process.platform === "win32"
        ? true
        : this._readPreferences().sandbox_network !== false,
      getExternalReadPaths,
      getSessionPath,
      getSessionIdForPath: (sessionPath) => this.getSessionIdForPath(sessionPath),
      resolveSessionFile: resolveRuntimeSessionFile,
      recordFileOperation: (entry) => this.recordSessionFileOperation(entry),
      getVisionBridge: () => this.getVisionBridge(),
      isVisionAuxiliaryEnabled: () => this.isVisionAuxiliaryEnabled(),
      getTerminalSessionManager: () => this._terminalSessions,
      getAgentId: () => agentId,
      resourceIO,
      emitEvent: (event, sessionPath) => this._emitEvent(event, sessionPath),
    } as any);
    assertUniqueBuiltToolNames([
      { source: "Pi built-in tools", tools: result.tools },
      { source: "runtime custom tools", tools: result.customTools },
    ]);

    // Checkpoint wrapper (outside sandbox layer)
    const backupCfg = this._prefs.getFileBackup();
    if (backupCfg.enabled) {
      result = {
        ...result,
        tools: wrapWithCheckpoint(result.tools, {
          store: this._checkpointStore,
          maxFileSizeKb: backupCfg.max_file_size_kb,
          cwd,
          getSessionPath,
        }),
      };
    }

    const getPermissionMode = typeof opts.getPermissionMode === "function"
      ? opts.getPermissionMode
      : (sessionPath) => this.getSessionPermissionMode(sessionPath);
    // 拦截上下文（如 { isSubagent }）：classify 据此做与 mode 无关的固定边界（防自递归等）。
    //
    // The session's capability grants are exposed as a getter rather than a
    // snapshot: the permission wrapper spreads this object on every tool call,
    // so a grant issued mid-session applies to the very next call without
    // rebuilding the tool set. getSessionPath() resolves the session this tool
    // set was built for, so a grant can never leak into another session.
    const readSessionGrants = (sessionPath) => this.getSessionAllowedInvocationCapabilities(sessionPath);
    const permissionContext = {
      ...(opts.permissionContext || {}),
      get preAuthorizedInvocationCapabilities() {
        return readSessionGrants(getSessionPath());
      },
    };
    result = {
      ...result,
      tools: wrapWithSessionPermission(result.tools, {
        getSessionPath,
        getSessionRef,
        getSessionId,
        getPermissionMode,
        permissionContext,
        agentId,
        cwd,
        workspaceFolders,
        authorizedFolders: staticAuthorizedFolders,
        getAuthorizedFolders,
        allowHumanApproval,
        approvalPolicy,
        getSessionIdForPath: (sessionPath) => this.getSessionIdForPath(sessionPath),
        permissionBoundary: result.permissionBoundary,
        getConfirmStore: () => this._confirmStore,
        getApprovalGateway: () => this._approvalGateway,
        emitEvent: (event, sessionPath) => this._emitEvent(event, sessionPath),
      }),
      customTools: wrapWithSessionPermission(result.customTools, {
        getSessionPath,
        getSessionRef,
        getSessionId,
        getPermissionMode,
        permissionContext,
        agentId,
        cwd,
        workspaceFolders,
        authorizedFolders: staticAuthorizedFolders,
        getAuthorizedFolders,
        allowHumanApproval,
        approvalPolicy,
        getSessionIdForPath: (sessionPath) => this.getSessionIdForPath(sessionPath),
        permissionBoundary: result.permissionBoundary,
        getConfirmStore: () => this._confirmStore,
        getApprovalGateway: () => this._approvalGateway,
        emitEvent: (event, sessionPath) => this._emitEvent(event, sessionPath),
      }),
    };

    // Object.create(LingxiEngine.prototype) is used by focused tool unit tests;
    // every constructed runtime owns the registry from the constructor above.
    if (this._sessionExecutions) {
      result = {
        ...result,
        tools: wrapWithSessionExecutionCancellation(result.tools, {
          registry: this._sessionExecutions,
          getSessionRef,
          getSessionId,
          getSessionPath,
          getSessionIdForPath: (sessionPath) => this.getSessionIdForPath(sessionPath),
        }),
        customTools: wrapWithSessionExecutionCancellation(result.customTools, {
          registry: this._sessionExecutions,
          getSessionRef,
          getSessionId,
          getSessionPath,
          getSessionIdForPath: (sessionPath) => this.getSessionIdForPath(sessionPath),
        }),
      };
    }

    // Startup assertion: every built-in tool must be categorized in
    // shared/tool-categories.ts. All session-creation paths route through
    // this function, so a single check here catches the whole surface.
    assertAllBuiltInToolsPermissionCovered([
      ...result.tools,
      ...result.customTools,
    ]);
    assertAllToolsCategorized([
      ...result.tools.map((t) => t.name).filter(Boolean),
      ...runtimeCustomTools
        .filter((t) => !t._pluginId && !extraCustomTools.some((extra) => extra.name === t.name))
        .map((t) => t.name)
        .filter(Boolean),
    ]);

    // The manifest travels in the build result rather than being stashed here.
    // The session that owns this tool set is the only thing that should hold it,
    // and the engine has no business keeping a map from sessions to catalogs.
    return {
      ...result,
      toolTargetRegistry: targetRegistry,
      toolInvocationGateway: invocationGateway,
      toolCatalogManifest: deferPlan
        ? buildToolCatalogManifestSnapshot(deferPlan.catalog, opts.modelContextWindowTokens)
        : null,
    };
  }

  // ════════════════════════════
  //  事件系统
  // ════════════════════════════

  /**
   * Hub 构造后注入回调，替代旧的 engine._hub = this 双向引用。
   * Manager 通过 getHub() lazy getter 拿到这个对象。
   */
  setHubCallbacks(callbacks) {
    this._hubCallbacks = callbacks;
    // 把 hub 引用补给 slash dispatcher（Phase 3：bridge-manager / WS 入口都靠它路由命令）
    if (callbacks?.hub) this._slashSystem?.dispatcher?.setHub(callbacks.hub);
  }

  registerAgentPhoneAbortHandler(handler, meta: any = {}) {
    return this._hubCallbacks?.registerAgentPhoneAbortHandler?.(handler, meta) || (() => {});
  }

  setEventBus(bus) {
    for (const fn of this._listeners) bus.subscribe(fn);
    this._listeners.clear();
    this._eventBus = bus;
  }

  getEventBus() {
    return this._eventBus;
  }

  subscribe(listener) {
    if (this._eventBus) return this._eventBus.subscribe(listener);
    this._listeners.add(listener);
    return () => this._listeners.delete(listener);
  }

  _emitEvent(event, sessionPath) {
    if (this._eventBus) {
      this._eventBus.emit(event, sessionPath);
    } else {
      for (const fn of this._listeners) {
        // 单个 listener 抛错不能中断对其余 listener 的分发，但要记录避免静默漏处理事件。
        try { fn(event, sessionPath); } catch (err) { moduleLog.warn(`event listener threw for ${event?.type}: ${err?.message}`); }
      }
    }
  }

  emitEvent(event, sessionPath) { this._emitEvent(event, sessionPath); }

  recordSecurityAuditEvent(event) {
    return appendSecurityAuditEvent(this.lingxiHome, event);
  }

  _emitAppEvent(type, payload: any = {}) {
    if (typeof type !== "string" || !type) return;
    const normalizedPayload = payload && typeof payload === "object" && !Array.isArray(payload)
      ? payload
      : {};
    this._emitEvent({
      type: "app_event",
      event: {
        type,
        payload: normalizedPayload,
        source: "server",
      },
    }, null);
  }

  emitDevLog(text, level = "info") {
    const entry = { text, level, ts: Date.now() };
    this._devLogs.push(entry);
    if (this._devLogs.length > this._devLogsMax) {
      this._devLogs.shift();
    }
    const dl = debugLog();
    if (dl) {
      if (level === "error") dl.error("engine", text);
      else dl.log("engine", text);
    }
    this._emitEvent({ type: "devlog", text, level }, null);
  }

  getDevLogs() {
    return this._devLogs;
  }

  // ════════════════════════════
  //  日记 / 工具调用
  // ════════════════════════════

  async writeDiary(opts: any = {}) {
    // /diary 是一项独立用户任务（§四十七）：一次 diary = N 次 MC-10 临时摘要 +
    // 1 次 MC-04 终稿，全部同一 traceId；force-new 切断 REST 路由异步链可能
    // 携带的外层 scope。
    return runWithNewModelTrace(
      { origin: "diary", refs: opts?.targetDate ? { targetDate: opts.targetDate } : null },
      () => this._writeDiaryWithinTrace(opts),
    );
  }

  async _writeDiaryWithinTrace(opts: any = {}) {
    const currentPath = this.currentSessionPath;
    if (currentPath && this.agent.memoryTicker) {
      await this.agent.memoryTicker.flushSession(currentPath);
    }
    const { writeDiary } = await import("../lib/diary/diary-writer.ts");
    // diary 产物是持久记忆材料，模型选择收口在 memory slot：
    //   memory_model 显式配置 → 用该模型
    //   memory_model 未配置   → resolver fallback 到目标 agent 的 chat
    //   memory_model 配置错误 → resolver throw，日记报错而非偷偷改用别的模型
    // 业务层不自行决定 chat/memory 优先级。
    let resolvedModel = null;
    try {
      resolvedModel = await this.resolveAuxiliaryExecution("memory");
    } catch (err) {
      if (isAuxiliaryConfigError(err)) {
        moduleLog.warn(`memory slot 配置错误，日记生成失败（不回退其它模型）: ${err.message}`);
      }
      throw err;
    }
    // 写日记是用户主动触发的「读历史」功能，必须参考记忆，
    // 跟「在对话中潜移默化带入记忆」的 master 开关无关。所以不查 memoryMasterEnabled。
    // per-session 开关只决定缺摘要时是否写回 summaries；关闭时仍可为本次日记临时压缩。
    const agent = this.agent;
    return writeDiary({
      summaryManager: agent.summaryManager,
      resolvedModel,
      agentPersonality: agent.personality,
      memory: (() => {
        try { return fs.readFileSync(agent.memoryMdPath, "utf-8"); } catch { return ""; }
      })(),
      userName: agent.userName,
      agentName: agent.agentName,
      cwd: this.homeCwd || process.cwd(),
      targetDate: opts.targetDate,
      activityStore: this.activityStore,
      sessionDir: agent.sessionDir,
      isSessionMemoryEnabledForPath: (sessionPath) => {
        return agent.isSessionMemoryEnabledFor(sessionPath);
      },
      getSessionIdForPath: (sessionPath) => this.getSessionIdForPath(sessionPath),
      readSessionBranchForPath: (sessionPath, options) => (
        this.getSessionBranchProjection(sessionPath, options)
      ),
    });
  }

  _utilityOptionsForContext( opts: any = {}) {
    if (opts?.agentId) return { agentId: opts.agentId, sessionPath: opts.sessionPath || null };
    if (opts?.sessionPath) {
      const agentId = this.resolveSessionOwnership(opts.sessionPath).agentId;
      if (agentId) return { agentId, sessionPath: opts.sessionPath };
    }
    return undefined;
  }

  async summarizeTitle(ut, at, opts: any = {}) {
    return _summarizeTitle(await this.resolveAuxiliaryModelFresh("title", this._auxResolveContext(opts)), ut, at, opts);
  }

  async translateSkillNames(names, lang, opts: any = {}) {
    const skills = Array.isArray(opts.skills)
      ? opts.skills
      : (opts.agentId ? this.getAllSkills(opts.agentId) : []);
    return translateSkillNamesWithCache({
      cachePath: getSkillNameTranslationCachePath(this.lingxiHome),
      skills,
      names,
      lang,
      translateMissing: async (missingNames) => _translateSkillNames(
        await this.resolveAuxiliaryModelFresh("title", opts.agentId ? { agentId: opts.agentId } : undefined),
        missingNames,
        lang,
      ),
    });
  }

  async summarizeActivity(sp, preloaded, opts: any = {}) {
    const ctx = this._auxResolveContext({ ...opts, sessionPath: opts.sessionPath || sp });
    return _summarizeActivity(await this.resolveAuxiliaryModelFresh("summarize", ctx), sp, (msg) => this.emitDevLog(msg), preloaded);
  }

  async summarizeActivityQuick(activityId) {
    let entry = null, foundAgentId = null;
    for (const [agId] of this._agentMgr.agents) {
      const store = this.getActivityStore(agId);
      const e = store?.get(activityId);
      if (e) { entry = e; foundAgentId = agId; break; }
    }
    if (!entry?.sessionFile) return null;
    const sessionPath = path.join(this.agentsDir, foundAgentId, "activity", entry.sessionFile);
    return _summarizeActivityQuick(await this.resolveAuxiliaryModelFresh("summarize", { agentId: foundAgentId }), sessionPath);
  }

  // ════════════════════════════
  //  Desk 辅助
  // ════════════════════════════

  listDeskFiles() {
    try {
      const dir = this.homeCwd;
      if (!dir || !fs.existsSync(dir)) return [];
      return fs.readdirSync(dir, { withFileTypes: true })
        .filter(e => !e.name.startsWith("."))
        .map(e => {
          const fp = path.join(dir, e.name);
          let mtime = 0;
          try { mtime = fs.statSync(fp).mtimeMs; } catch {
            // 文件在 readdir 之后被删 / 无权限时 stat 失败，mtime 保持 0 兜底。
          }
          return { name: e.name, isDir: e.isDirectory(), mtime };
        });
    } catch {
      return [];
    }
  }

  get defaultDeskCwd() {
    return this.homeCwd || null;
  }

  _realPathForWorkspaceCheck(p) {
    if (!p || typeof p !== "string") return null;
    try {
      return fs.realpathSync(p);
    } catch {
      try {
        return fs.realpathSync(path.dirname(p));
      } catch {
        return null;
      }
    }
  }

  isApprovedWorkspaceDir(dir) {
    const resolved = this._realPathForWorkspaceCheck(dir);
    if (!resolved) return false;
    const roots = [
      this.homeCwd,
      this.deskCwd,
      ...this.getSessionWorkspaceFolders(this.currentSessionPath),
    ].filter(Boolean);
    return roots.some((root) => {
      const base = this._realPathForWorkspaceCheck(root);
      if (!base) return false;
      return resolved === base || resolved.startsWith(base + path.sep);
    });
  }

  isApprovedDeskDir(dir) {
    const resolved = this._realPathForWorkspaceCheck(dir);
    if (!resolved) return false;
    const roots = [
      this.homeCwd,
      this.deskCwd,
      ...this.getSessionWorkspaceFolders(this.currentSessionPath),
      ...(Array.isArray(this.config?.cwd_history) ? this.config.cwd_history : []),
    ].filter(Boolean);
    return roots.some((root) => {
      const base = this._realPathForWorkspaceCheck(root);
      if (!base) return false;
      return resolved === base || resolved.startsWith(base + path.sep);
    });
  }

  // ════════════════════════════
  //  Preferences 代理
  // ════════════════════════════

  _readPreferences() { return this._prefs.getPreferences(); }
  _writePreferences(prefs) { return this._prefs.savePreferences(prefs); }
  _readPrimaryAgent() { return this._prefs.getPrimaryAgent(); }
  _savePrimaryAgent(agentId) { return this._prefs.savePrimaryAgent(agentId); }

  // ════════════════════════════
  //  巡检工具白名单（向后兼容静态引用）
  // ════════════════════════════

  static PATROL_TOOLS_DEFAULT = "*";
}

function runtimeSessionRefError(message) {
  return Object.assign(new Error(message), { code: "session_manifest_ref_required" });
}

function freezeRuntimeSessionRef(value, required = false) {
  if (value == null) {
    if (required) {
      throw runtimeSessionRefError(
        "buildTools: runtime SessionRef is required before tool assembly",
      );
    }
    return null;
  }
  const sessionId = typeof value?.sessionId === "string" && value.sessionId.trim()
    ? value.sessionId.trim()
    : null;
  const sessionPath = typeof value?.sessionPath === "string" && value.sessionPath.trim()
    ? path.resolve(value.sessionPath)
    : null;
  if (!sessionId || !sessionPath) {
    throw runtimeSessionRefError(
      "buildTools: runtime SessionRef requires both sessionId and sessionPath",
    );
  }
  return Object.freeze({ sessionId, sessionPath });
}

function assertUniqueBuiltToolNames(groups: Array<{ source: string; tools: any[] }>) {
  const seen = new Map<string, string>();
  for (const group of groups) {
    for (const tool of group.tools || []) {
      const name = typeof tool?.name === "string" && tool.name.trim()
        ? tool.name
        : null;
      if (!name) continue;
      const previousSource = seen.get(name);
      if (previousSource) {
        throw new Error(
          `buildTools: duplicate tool name "${name}" across ${previousSource} and ${group.source}`,
        );
      }
      seen.set(name, group.source);
    }
  }
}
