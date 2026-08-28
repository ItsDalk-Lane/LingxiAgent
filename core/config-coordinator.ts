/**
 * ConfigCoordinator — 运行时配置管理
 *
 * 负责 per-agent 模型选择、共享模型角色、搜索/utility 配置、
 * session meta 持久化、updateConfig 联动。
 * 不持有 engine 引用，通过构造器注入依赖。
 */
import { createModuleLogger } from "../lib/debug-log.ts";
import { findModel, parseModelRef, requireModelRef } from "../shared/model-ref.ts";
import { t } from "../lib/i18n.ts";
import { resolveDefaultWorkspacePath } from "../shared/default-workspace.ts";
import { AUXILIARY_SLOT_PREF_ENTRIES } from "./auxiliary-slots.ts";
import {
  AUTO_SEARCH_PROVIDER,
  isSearchApiProvider,
  mergeSearchApiKeys,
  normalizeSearchApiKeys,
  normalizeSearchProvider,
} from "../shared/search-providers.ts";
import {
  classifyWorkspacePathForGc,
  pruneMissingWorkspaceConfig,
} from "../shared/workspace-persistence-gc.ts";

const log = createModuleLogger("config");

export const ACCESS_MODE_OPERATE = "operate";
export const ACCESS_MODE_READ_ONLY = "read_only";

// COMPAT: 旧 access-mode 枚举，仅 computer-host 仍用（待迁到 canonical session-permission-mode 后删）。
// 只读工具白名单（READ_ONLY_TOOL_NAMES / filterReadOnlyToolNames 等）已删——零调用方的死代码，
// 真正的只读判定收口在 core/session-permission-mode.ts 的 classifySessionPermission。
export function normalizeAccessMode(mode, { legacyPlanMode = false } = {}) {
  if (mode === ACCESS_MODE_READ_ONLY) return ACCESS_MODE_READ_ONLY;
  if (mode === ACCESS_MODE_OPERATE) return ACCESS_MODE_OPERATE;
  return legacyPlanMode ? ACCESS_MODE_READ_ONLY : ACCESS_MODE_OPERATE;
}

/**
 * 语义 Slot 字段 → preferences key 映射。
 * 单一真理源：从 canonical auxiliary-slots 的 AUXILIARY_SLOT_PREF_ENTRIES 派生，
 * 禁止在此处手写第二份 Slot→prefKey 映射。新增 Slot 时 canonical 处加一条即可，
 * ConfigCoordinator / preferences / UI 全部自动跟随。
 */
export const AUXILIARY_MODEL_PREF_KEYS: ReadonlyArray<readonly [string, string]> =
  AUXILIARY_SLOT_PREF_ENTRIES;

/**
 * @deprecated 使用 AUXILIARY_MODEL_PREF_KEYS。仅为向后兼容 re-export。
 */
export const SHARED_MODEL_KEYS = AUXILIARY_MODEL_PREF_KEYS;

/** 操作模型不是语义 Slot；这里只共享同一种 ModelRef 持久化形状。 */
export const MODEL_OPERATION_PREF_KEYS: ReadonlyArray<readonly [string, string]> = [
  ["embedding", "knowledge_embedding_model"],
  ["rerank", "knowledge_rerank_model"],
];

export const VISION_AUXILIARY_ENABLED_PREF_KEY = "vision_auxiliary_enabled";

function hasOwn(obj, key) {
  return Object.prototype.hasOwnProperty.call(obj, key);
}

export function sharedModelsPatchRequiresModelSync(patch) {
  if (!patch || typeof patch !== "object") return false;
  return AUXILIARY_MODEL_PREF_KEYS.some(([field]) => hasOwn(patch, field));
}

/**
 * 允许的非 Slot model settings 字段（feature toggle 等，与 Slot 模型引用是不同概念）。
 */
const ALLOWED_NON_SLOT_FIELDS = new Set([
  "vision_enabled",
  ...MODEL_OPERATION_PREF_KEYS.map(([field]) => field),
]);

export function normalizeSharedModelsPatch(partial) {
  if (!partial || typeof partial !== "object" || Array.isArray(partial)) {
    throw new Error("shared models patch must be an object");
  }

  const validSlotFields = new Set(AUXILIARY_MODEL_PREF_KEYS.map(([f]) => f));

  // 拒绝未知字段——防止拼写错误（如 summarzie）被静默接受。
  // 契约：字段 omitted → no change；字段 null → clear；字段 valid ModelRef → set；字段 unknown → reject。
  for (const key of Object.keys(partial)) {
    if (!validSlotFields.has(key) && !ALLOWED_NON_SLOT_FIELDS.has(key)) {
      throw new Error(`unknown shared model field "${key}"`);
    }
  }

  const result: any = {};
  for (const [field] of AUXILIARY_MODEL_PREF_KEYS) {
    if (!hasOwn(partial, field)) continue;
    const raw = partial[field];
    if (raw === undefined) continue;
    if (raw === null || raw === "") {
      result[field] = null;
      continue;
    }
    try {
      result[field] = requireModelRef(raw);
    } catch (err) {
      throw new Error(`shared model ${field}: ${err.message}`);
    }
  }
  for (const [field] of MODEL_OPERATION_PREF_KEYS) {
    if (!hasOwn(partial, field)) continue;
    const raw = partial[field];
    if (raw === undefined) continue;
    if (raw === null || raw === "") {
      result[field] = null;
      continue;
    }
    try {
      result[field] = requireModelRef(raw);
    } catch (err) {
      throw new Error(`model operation ${field}: ${err.message}`);
    }
  }
  if (hasOwn(partial, "vision_enabled")) {
    const raw = partial.vision_enabled;
    if (raw !== undefined) {
      if (typeof raw !== "boolean") {
        throw new Error("shared model vision_enabled must be a boolean");
      }
      result.vision_enabled = raw;
    }
  }
  return result;
}

export class ConfigCoordinator {
  declare _d: any;
  /**
   * @param {object} deps
   * @param {string} deps.lingxiHome
   * @param {string} deps.agentsDir
   * @param {() => object} deps.getAgent - 当前焦点 agent
   * @param {(id: string) => object|null} deps.getAgentById - 按 ID 查找 agent
   * @param {() => string} deps.getActiveAgentId - 当前焦点 agent ID
   * @param {() => Map} deps.getAgents - 所有 agent Map
   * @param {() => import('./model-manager.ts').ModelManager} deps.getModels
   * @param {() => import('./preferences-manager.ts').PreferencesManager} deps.getPrefs
   * @param {() => import('./skill-manager.ts').SkillManager} deps.getSkills
   * @param {() => import('./session-coordinator.ts').SessionCoordinator|null} deps.getSessionCoordinator
   * @param {() => object|null} deps.getHub
   * @param {(event, sp) => void} deps.emitEvent
   * @param {(text, level?) => void} deps.emitDevLog
   * @param {() => string|null} deps.getCurrentModel - currentModel name
   */
  constructor(deps) {
    this._d = deps;
  }

  // ── Home Folder ──

  /**
   * @param {string} [agentId] - 指定 agent；省略时查主 agent
   * @returns {string|null} 该 agent 自己显式绑定且仍存在的工作目录
   */
  getExplicitHomeFolder(agentId) {
    const targetId = agentId || this._getPrimaryAgentId();
    if (!targetId) return null;
    const agent = this._d.getAgentById(targetId);
    const folder = agent?.config?.desk?.home_folder;
    const status = classifyWorkspacePathForGc(folder);
    if (status.status === "present" || status.status === "unknown") return status.path;
    if (status.status === "missing") {
      agent?.updateConfig?.({ desk: { home_folder: null } });
    }
    return null;
  }

  /**
   * @param {string} [agentId] - 指定 agent；省略时查主 agent
   * @returns {string} 工作目录路径（纯解析，不创建目录）
   */
  getHomeFolder(agentId) {
    const explicit = this.getExplicitHomeFolder(agentId);
    if (explicit) return explicit;

    // 显式默认工作区，避免把整个桌面暴露成工作目录。
    // 不从别的 agent 继承 home_folder；跨 agent fallback 会让状态归属变成隐式焦点推导。
    return resolveDefaultWorkspacePath();
  }

  /**
   * @param {string} agentId
   * @param {string|null} folder
   */
  setHomeFolder(agentId, folder) {
    const agent = this._d.getAgentById(agentId);
    if (!agent) {
      log.warn(`setHomeFolder: agent ${agentId} not found`);
      return;
    }
    if (folder) {
      agent.updateConfig({ desk: { home_folder: folder } });
    } else {
      // null 值触发 deepMerge 的 key 删除逻辑
      agent.updateConfig({ desk: { home_folder: null } });
    }
    log.log(`setHomeFolder(${agentId}): ${folder || "(cleared)"}`);
  }

  gcWorkspaceConfig(agentId, options: any = {}) {
    const targetId = agentId || this._getPrimaryAgentId();
    if (!targetId) return { changed: false, patch: {} };
    const agent = this._d.getAgentById(targetId);
    if (!agent) return { changed: false, patch: {} };
    const result = pruneMissingWorkspaceConfig(agent.config || {}, options);
    if (result.changed) {
      agent.updateConfig(result.patch);
    }
    return result;
  }

  gcAllWorkspaceConfigs( options: any = {}) {
    const agents = this._d.getAgents?.();
    const ids = agents instanceof Map ? [...agents.keys()] : [];
    if (ids.length === 0) {
      return [this.gcWorkspaceConfig(undefined, options)];
    }
    return ids.map((id) => this.gcWorkspaceConfig(id, options));
  }

  // ── Shared Models ──

  getSharedModels() {
    const prefs = this._prefs();
    const result: any = {};
    for (const [field, prefKey] of AUXILIARY_MODEL_PREF_KEYS) {
      const raw = prefs[prefKey];
      if (typeof raw === "object" && raw?.id) {
        result[field] = raw;
      } else if (raw) {
        result[field] = raw;
      } else {
        result[field] = null;
      }
    }
    for (const [field, prefKey] of MODEL_OPERATION_PREF_KEYS) {
      const raw = prefs[prefKey];
      result[field] = raw || null;
    }
    result.vision_enabled = prefs[VISION_AUXILIARY_ENABLED_PREF_KEY] === true;
    return result;
  }

  setSharedModels(partial) {
    const normalized = normalizeSharedModelsPatch(partial);
    const prefs = this._prefs();
    const changed = [];
    let shouldSyncAgentRuntimeModels = false;
    for (const [field, prefKey] of AUXILIARY_MODEL_PREF_KEYS) {
      if (hasOwn(normalized, field)) {
        if (normalized[field] !== null && normalized[field] !== "") prefs[prefKey] = normalized[field];
        else delete prefs[prefKey];
        const v = normalized[field];
        const repr = !v ? "(cleared)"
          : typeof v === "object" ? `${v.provider || "?"}/${v.id || "?"}`
          : String(v);
        changed.push(`${field}=${repr}`);
        if (field === "memory") {
          shouldSyncAgentRuntimeModels = true;
        }
      }
    }
    for (const [field, prefKey] of MODEL_OPERATION_PREF_KEYS) {
      if (!hasOwn(normalized, field)) continue;
      if (normalized[field]) prefs[prefKey] = normalized[field];
      else delete prefs[prefKey];
      const value = normalized[field];
      changed.push(
        `${field}=${value ? `${value.provider || "?"}/${value.id || "?"}` : "(cleared)"}`,
      );
    }
    if (hasOwn(normalized, "vision_enabled")) {
      if (normalized.vision_enabled) prefs[VISION_AUXILIARY_ENABLED_PREF_KEY] = true;
      else delete prefs[VISION_AUXILIARY_ENABLED_PREF_KEY];
      changed.push(`vision_enabled=${normalized.vision_enabled ? "on" : "off"}`);
    }
    this._savePrefs(prefs);
    if (shouldSyncAgentRuntimeModels) {
      const fresh = this.getSharedModels();
      this._syncSharedModelsToAgents(fresh);
    }
    if (changed.length) {
      log.log(`setSharedModels: ${changed.join(", ")}`);
    }
  }

  _syncSharedModelsToAgents(sharedModels) {
    const agents = this._d.getAgents?.();
    if (agents instanceof Map && agents.size) {
      for (const agent of agents.values()) {
        this._syncSharedModelsToAgent(agent, sharedModels);
      }
      return;
    }
    this._syncSharedModelsToAgent(this._d.getAgent?.(), sharedModels);
  }

  _syncSharedModelsToAgent(agent, sharedModels) {
    // 辅助模型角色不再缓存在 Agent 实例上。
    // MemoryTicker 等消费方在调用边界通过 engine.resolveAuxiliaryExecution("memory")
    // 现场解析，用户修改 memory_model 后下一次 tick 自动生效，无需同步。
    if (!agent) return;
  }

  // ── Search Config ──

  getSearchConfig() {
    const prefs = this._prefs();
    const provider = normalizeSearchProvider(prefs.search_provider) || AUTO_SEARCH_PROVIDER;
    const apiKeys = normalizeSearchApiKeys(prefs.search_api_keys);
    const legacyProvider = normalizeSearchProvider(prefs.search_provider);
    if (isSearchApiProvider(legacyProvider) && typeof prefs.search_api_key === "string" && prefs.search_api_key.trim()) {
      apiKeys[legacyProvider] = apiKeys[legacyProvider] || prefs.search_api_key.trim();
    }
    const apiKey = isSearchApiProvider(provider)
      ? apiKeys[provider] || (typeof prefs.search_api_key === "string" ? prefs.search_api_key.trim() : "") || null
      : null;
    return {
      provider,
      api_key: apiKey,
      api_keys: apiKeys,
    };
  }

  setSearchConfig(partial) {
    const prefs = this._prefs();
    const previousProvider = normalizeSearchProvider(prefs.search_provider);
    let apiKeys = normalizeSearchApiKeys(prefs.search_api_keys);
    if (isSearchApiProvider(previousProvider) && typeof prefs.search_api_key === "string" && prefs.search_api_key.trim()) {
      apiKeys[previousProvider] = apiKeys[previousProvider] || prefs.search_api_key.trim();
    }
    const nextProvider = partial.provider !== undefined
      ? normalizeSearchProvider(partial.provider)
      : previousProvider || AUTO_SEARCH_PROVIDER;

    if (partial.provider !== undefined) {
      if (nextProvider) prefs.search_provider = nextProvider;
      else delete prefs.search_provider;
    }
    if (partial.api_keys !== undefined) {
      apiKeys = mergeSearchApiKeys(apiKeys, partial.api_keys);
    }
    if (partial.api_key !== undefined) {
      if (isSearchApiProvider(nextProvider)) {
        const apiKey = typeof partial.api_key === "string" ? partial.api_key.trim() : "";
        if (apiKey) apiKeys[nextProvider] = apiKey;
        else delete apiKeys[nextProvider];
      }
    }
    if (Object.keys(apiKeys).length > 0) prefs.search_api_keys = apiKeys;
    else delete prefs.search_api_keys;

    if (isSearchApiProvider(nextProvider) && apiKeys[nextProvider]) {
      prefs.search_api_key = apiKeys[nextProvider];
    } else {
      delete prefs.search_api_key;
    }
    this._savePrefs(prefs);
    log.log(`setSearchConfig: provider=${nextProvider || "(cleared)"}`);
  }

  // ── Agent Order ──

  readAgentOrder() {
    return this._prefs().agentOrder || [];
  }

  saveAgentOrder(order) {
    const prefs = this._prefs();
    prefs.agentOrder = order;
    this._savePrefs(prefs);
  }

  // ── Model / Thinking ──

  async syncAndRefresh() {
    const models = this._d.getModels();
    return await models.syncAndRefresh();
  }

  /**
   * 暂存用户选择的模型，用于下次 createSession。
   * 不修改当前活跃 session 的模型，不持久化到 config.yaml。
   */
  setPendingModel(modelId, provider) {
    if (!modelId || !provider) {
      throw new Error(`setPendingModel: modelId and provider both required (got ${modelId}, ${provider})`);
    }
    const models = this._d.getModels();
    const model = findModel(models.availableModels, modelId, provider);
    if (!model) throw new Error(t("error.modelNotFound", { id: `${provider}/${modelId}` }));
    const sessionCoord = this._d.getSessionCoordinator();
    sessionCoord?.setPendingModel(model);
    return model;
  }

  /**
   * 设置 agent 默认模型（设置页面操作）。
   * 更新 ModelManager._defaultModel + 持久化到 config.yaml。
   * 不修改任何已有 session 的模型。
   *
   * provider 必填——setDefaultModel 不做按 id 猜 provider 的兜底。
   */
  async setDefaultModel(modelId, provider, { agentId }: any = {}) {
    if (!modelId || !provider) {
      throw new Error(`setDefaultModel: modelId and provider both required (got ${modelId}, ${provider})`);
    }
    const models = this._d.getModels();
    const model = findModel(models.availableModels, modelId, provider);
    if (!model) throw new Error(t("error.modelNotFound", { id: `${provider}/${modelId}` }));
    await this.updateConfig(
      { models: { chat: { id: modelId, provider } } },
      agentId ? { agentId } : {} as any,
    );
    log.log(`default model set to: ${model.provider}/${model.id}${agentId ? ` agentId=${agentId}` : ""}`);
    return model;
  }

  setThinkingLevel(level) {
    // 全局 preference 只作为新 session 默认值；已有 session 的实际值归 SessionCoordinator。
    this._d.getPrefs().setThinkingLevel(level);
  }

  /** 从 preference 读取用户设定的 thinking level */
  getThinkingLevel() {
    return this._d.getPrefs().getThinkingLevel();
  }

  // ── Memory ──

  // 这里曾经有一个 setMemoryEnabled(val)：它从"当前焦点会话"反推要写哪个
  // session 的记忆开关，和 persistSessionMeta 当年的毛病是同一种。全仓生产
  // 路径没有任何调用方（界面上切记忆开关只改前端草稿态，随新建会话的请求体
  // 落盘），所以直接删掉而不是给死代码改签名。将来若要支持"会话进行中切记忆
  // 开关"，调用方必须显式说明写哪个 session：
  // sessionCoord.setSessionMemoryEnabled(sessionPath, val)。

  setMemoryMasterEnabled(agentId, val) {
    const ag = this._d.getAgents().get(agentId);
    if (ag) ag.setMemoryMasterEnabled(val);
  }

  // sessionPath 必传：这个函数曾经从全局焦点指针读要写哪个 session，而新建
  // 分离会话的路径会在创建结束时把焦点还给上一个会话，于是新会话的记忆开关
  // 被写到了上一个会话头上。要写哪个 session 只能由调用方显式说明。
  persistSessionMeta(sessionPath) {
    if (!sessionPath) throw new Error("persistSessionMeta: sessionPath is required");
    const sessionCoord = this._d.getSessionCoordinator();
    const memoryEnabled = typeof sessionCoord?.getSessionMemoryEnabled === "function"
      ? sessionCoord.getSessionMemoryEnabled(sessionPath)
      : this._d.getAgent().sessionMemoryEnabled;
    return sessionCoord.writeSessionMeta(sessionPath, {
      // session-meta 持久化的是 session 自身冻结下来的记忆参与态，
      // 不能写 master && session 的临时组合态，否则会把运行时 gate
      // 错写成 session 身份，打穿 prefix cache 前提。
      memoryEnabled,
    });
  }

  // ── updateConfig ──

  async updateConfig(partial, { agentId, refreshDescription = false }: any = {}) {
    const keys = Object.keys(partial);
    if (keys.length) log.log(`updateConfig: keys=[${keys.join(",")}]${agentId ? ` agentId=${agentId}` : ""}`);

    // 如果指定了 agentId，刷新该 agent；否则刷新焦点 agent
    const agent = (agentId && this._d.getAgentById?.(agentId)) || this._d.getAgent();
    const models = this._d.getModels();
    const isFocusAgent = !agentId || agentId === this._d.getActiveAgentId?.();

    // agent 负责：写磁盘、刷新身份、刷新模块、重建 prompt
    if (refreshDescription) agent.updateConfig(partial, { refreshDescription: true });
    else agent.updateConfig(partial);

    // 模型切换只在焦点 agent 时生效。migration #5 之后 models.chat 必为
    // {id, provider} 对象；缺 provider 直接忽略并告警（调用方应传完整复合键）。
    if (isFocusAgent && partial.models?.chat) {
      const parsed = parseModelRef(partial.models.chat);
      if (!parsed?.id || !parsed?.provider) {
        log.warn(`updateConfig: models.chat 缺少 provider，已忽略 (got ${JSON.stringify(partial.models.chat)})`);
      } else {
        const newModel = findModel(models.availableModels, parsed.id, parsed.provider);
        if (newModel) {
          // 只更新 agent 默认模型，不改活跃 session
          models.defaultModel = newModel;
          log.log(`default model updated to: ${newModel.provider}/${newModel.id}`);
        }
      }
    }

    if (partial.skills) {
      this._d.getSkills().syncAgentSkills(agent);
    }

    // desk（heartbeat 等）联动对应 agent 的 heartbeat
    if (partial.desk) {
      const scheduler = this._d.getHub()?.scheduler;
      const resolvedAgentId = agentId || this._d.getActiveAgentId?.();
      if ("heartbeat_interval" in partial.desk && scheduler) {
        // 间隔变更：需要完整重建 heartbeat（INTERVAL 在创建时固化）
        this._d.emitDevLog(`[heartbeat] 巡检间隔已更新: ${partial.desk.heartbeat_interval} 分钟`);
        await scheduler.reloadHeartbeat(resolvedAgentId);
      } else if ("heartbeat_enabled" in partial.desk) {
        const hb = scheduler?.getHeartbeat(resolvedAgentId);
        if (hb) {
          if (partial.desk.heartbeat_enabled === false) {
            this._d.emitDevLog("[heartbeat] 巡检已关闭");
            await hb.stop();
          } else if (partial.desk.heartbeat_enabled === true && this.getHeartbeatMaster() !== false) {
            this._d.emitDevLog("[heartbeat] 巡检已开启");
            hb.start();
          }
        }
      }
    }
  }

  // ── Channels Master ──

  getChannelsEnabled() {
    return this._d.getPrefs().getChannelsEnabled();
  }

  async setChannelsEnabled(enabled) {
    const next = !!enabled;
    const prefs = this._d.getPrefs();
    const prev = prefs.getChannelsEnabled();
    prefs.setChannelsEnabled(next);
    log.log(`setChannelsEnabled: ${next}`);

    const hub = this._d.getHub();
    if (hub && typeof hub.toggleChannels === "function") {
      await hub.toggleChannels(next);
    }
  }

  // ── Heartbeat Master ──

  getHeartbeatMaster() {
    return this._prefs().heartbeat_master !== false;
  }

  setHeartbeatMaster(enabled) {
    const prefs = this._prefs();
    prefs.heartbeat_master = !!enabled;
    this._savePrefs(prefs);
    log.log(`setHeartbeatMaster: ${enabled}`);

    // 联动 scheduler：启停所有 agent 的 heartbeat
    const scheduler = this._d.getHub()?.scheduler;
    if (!scheduler) return;
    const agents = this._d.getAgents();
    for (const [, agent] of agents) {
      const hb = scheduler.getHeartbeat(agent.id);
      if (!hb) continue;
      if (!enabled) {
        hb.stop();
      } else if (agent.config?.desk?.heartbeat_enabled === true) {
        hb.start();
      }
    }
  }

  // ── helpers ──

  _getPrimaryAgentId() {
    const prefsManager = this._d.getPrefs();
    if (typeof prefsManager.getPrimaryAgent === 'function') {
      return prefsManager.getPrimaryAgent();
    }
    const prefs = this._prefs();
    return prefs.primaryAgent || null;
  }

  _prefs() { return this._d.getPrefs().getPreferences(); }
  _savePrefs(prefs) { return this._d.getPrefs().savePreferences(prefs); }
}
