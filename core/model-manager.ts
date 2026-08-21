/**
 * ModelManager -- 模型发现、切换、凭证解析
 *
 * 管理 Pi SDK AuthStorage / ModelRegistry 基础设施，
 * 以及模型选择、provider 凭证查找、utility 配置解析。
 * 从 Engine 提取，Engine 通过 manager 访问模型状态。
 *
 * _availableModels 是唯一的模型真理源。所有模型解析、enrichment
 * 都在这个数组上完成，不再经过中间层。
 */
import path from "path";
import {
  AuthStorage,
  FileAuthStorageBackend,
  createModelRegistry,
  registerModelProvider,
  unregisterModelProvider,
  SdkAuthFacade,
} from "../lib/pi-sdk/index.ts";
import { forceRefreshOAuthApiKey } from "./oauth-force-refresh.ts";
import { t } from "../lib/i18n.ts";
import { ProviderRegistry } from "./provider-registry.ts";
import { composeResolvedModelExecution } from "./model-execution-config.ts";
import { findModel, parseModelRef } from "../shared/model-ref.ts";
import { isLocalBaseUrl } from "../shared/net-utils.ts";
import { normalizeProviderHeaders, stripCredentialHeaders } from "../shared/provider-auth.ts";
import { syncModels } from "./model-sync.ts";
import { enrichModelFromKnownMetadata } from "./model-known-enrichment.ts";
import { lookupKnownProvider } from "../shared/known-models.ts";
import { readModalityListLoose } from "../shared/modality.ts";
import { migrateLegacyApiKeyAuthToProviders } from "./provider-auth-migration.ts";
import {
  normalizePiSdkThinkingLevel,
  normalizeSessionThinkingLevel,
  normalizeThinkingLevelChoices,
  normalizeThinkingLevelForModel,
  resolveModelDefaultThinkingLevel,
} from "./session-thinking-level.ts";

function isRecord(value): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function modelEntryId(modelEntry: unknown) {
  return isRecord(modelEntry) ? modelEntry.id : modelEntry;
}

function modelMetadataKey(provider, modelId) {
  return `${provider || ""}\0${modelId || ""}`;
}

function providerModelDefault(rawProvider: Record<string, unknown>, modelId: string) {
  const modelDefaults = isRecord(rawProvider.model_defaults) ? rawProvider.model_defaults : null;
  const entry = isRecord(modelDefaults?.[modelId]) ? modelDefaults[modelId] : null;
  const level = entry?.thinking_level ?? entry?.thinkingLevel;
  return typeof level === "string" ? level : undefined;
}

function buildProviderModelMetadataMap(projectionPlans: unknown) {
  const map = new Map<string, Record<string, unknown>>();
  const plans = Array.isArray(projectionPlans) ? projectionPlans : [];
  for (const plan of plans) {
    const provider = plan?.sourceProviderId;
    const runtimeProvider = plan?.runtimeProviderId || provider;
    if (typeof provider !== "string" || !provider) continue;
    const rawProvider = isRecord(plan?.config) ? plan.config : {};
    const models = Array.isArray(rawProvider.models) ? rawProvider.models : [];
    for (const modelEntry of models) {
      const modelId = modelEntryId(modelEntry);
      if (typeof modelId !== "string" || !modelId) continue;
      const known = lookupKnownProvider(provider, modelId);
      const meta: Record<string, unknown> = {};
      if (isRecord(modelEntry)) {
        if (modelEntry.xhigh !== undefined) meta.xhigh = modelEntry.xhigh === true;
        if (modelEntry.defaultThinkingLevel !== undefined) meta.defaultThinkingLevel = modelEntry.defaultThinkingLevel;
        const thinkingLevels = normalizeThinkingLevelChoices(modelEntry.thinkingLevels);
        if (thinkingLevels) meta.thinkingLevels = thinkingLevels;
        if (modelEntry.thinkingLevelMap !== undefined && isRecord(modelEntry.thinkingLevelMap)) {
          meta.thinkingLevelMap = structuredClone(modelEntry.thinkingLevelMap);
        }
        if (modelEntry.toolUse !== undefined) meta.toolUse = structuredClone(modelEntry.toolUse);
        if (modelEntry.visionCapabilities !== undefined) meta.visionCapabilities = structuredClone(modelEntry.visionCapabilities);
        // 统一模态/能力 metadata：Pi modelFromJson 会丢弃这些字段，这里统一挂回
        const explicitInputs = readModalityListLoose(modelEntry.inputs);
        if (explicitInputs) meta.inputs = explicitInputs;
        const explicitOutputs = readModalityListLoose(modelEntry.outputs);
        if (explicitOutputs) meta.outputs = explicitOutputs;
        if (modelEntry.web !== undefined) meta.web = modelEntry.web === true;
        if (modelEntry.structuredOutput !== undefined) meta.structuredOutput = modelEntry.structuredOutput === true;
      }
      if (meta.inputs === undefined) {
        const knownInputs = readModalityListLoose(known?.inputs);
        if (knownInputs) meta.inputs = knownInputs;
      }
      if (meta.outputs === undefined) {
        const knownOutputs = readModalityListLoose(known?.outputs);
        if (knownOutputs) meta.outputs = knownOutputs;
      }
      if (meta.web === undefined && known?.web === true) meta.web = true;
      if (meta.structuredOutput === undefined && known?.structuredOutput === true) meta.structuredOutput = true;
      const executionHeaders = normalizeProviderHeaders(plan?.modelExecutionHeaders?.[modelId]);
      if (Object.keys(executionHeaders).length > 0) meta.headers = executionHeaders;
      if (meta.defaultThinkingLevel === undefined && typeof known?.defaultThinkingLevel === "string") {
        meta.defaultThinkingLevel = known.defaultThinkingLevel;
      }
      if (meta.thinkingLevels === undefined) {
        const knownThinkingLevels = normalizeThinkingLevelChoices(known?.thinkingLevels);
        if (knownThinkingLevels) meta.thinkingLevels = knownThinkingLevels;
      }
      if (meta.thinkingLevelMap === undefined && known?.thinkingLevelMap && isRecord(known.thinkingLevelMap)) {
        meta.thinkingLevelMap = structuredClone(known.thinkingLevelMap);
      }
      if (typeof known?.maxContext === "number") meta.maxContext = known.maxContext;
      const defaultThinkingLevel = providerModelDefault(rawProvider, modelId);
      if (defaultThinkingLevel !== undefined) meta.defaultThinkingLevel = defaultThinkingLevel;
      if ((Array.isArray(meta.thinkingLevels) && meta.thinkingLevels.includes("max"))
        || isRecord(meta.thinkingLevelMap) && typeof meta.thinkingLevelMap.xhigh === "string") {
        if (meta.xhigh === undefined) meta.xhigh = true;
      }
      if (Object.keys(meta).length > 0) {
        map.set(modelMetadataKey(runtimeProvider, modelId), meta);
        map.set(modelMetadataKey(provider, modelId), meta);
      }
    }
  }
  return map;
}

function applyProviderModelMetadata(model, metadataByModel) {
  const meta = metadataByModel.get(modelMetadataKey(model?.provider, model?.id));
  if (!meta) return model;
  const merged = { ...model, ...meta };
  const thinkingLevels = normalizeThinkingLevelChoices(merged.thinkingLevels);
  if (thinkingLevels) {
    merged.thinkingLevels = thinkingLevels;
    if (thinkingLevels.includes("max")) merged.xhigh = true;
  } else {
    delete merged.thinkingLevels;
  }
  if (typeof merged.defaultThinkingLevel === "string") {
    merged.defaultThinkingLevel = normalizeThinkingLevelForModel(merged.defaultThinkingLevel, merged);
  }
  return merged;
}

export class ModelManager {
  declare _authBackend: any;
  declare _authStorage: any;
  declare _availableModels: any;
  declare _defaultModel: any;
  declare _lingxiHome: any;
  declare _modelRegistry: any;
  declare _modelRuntime: any;
  declare _registeredSdkProviderIds: Set<string>;
  declare providerRegistry: any;
  /**
   * @param {object} opts
   * @param {string} opts.lingxiHome - 用户数据根目录
   */
  constructor({ lingxiHome }) {
    this._lingxiHome = lingxiHome;
    this._authStorage = null;
    this._authBackend = null;
    this._modelRegistry = null;
    this._modelRuntime = null;
    this._registeredSdkProviderIds = new Set();
    this._defaultModel = null;   // 设置页面选的，持久化，bridge 用这个
    this._availableModels = [];

    // 新架构模块（init() 后可用）
    this.providerRegistry = new ProviderRegistry(lingxiHome);
  }

  /**
   * 初始化 AuthStorage + ModelRuntime/ModelRegistry + 新架构模块。
   *
   * 0.83.0 起 ModelRuntime.create 是 async（内部刷新模型目录）；本方法随之
   * 改 async。同步阶段的校验（provider collision / thinkingLevelMap）仍在
   * await 之前先抛，保持“init 期配置错误立即暴露”的旧契约。
   */
  async init() {
    const rawAuthStorage = AuthStorage.create(path.join(this._lingxiHome, "auth.json"));
    // Same file, same lock: forced OAuth rotation writes through this backend.
    this._authBackend = new FileAuthStorageBackend(path.join(this._lingxiHome, "auth.json"));
    this.providerRegistry.reload();
    // 先把 _authStorage 装上 rawAuthStorage，让 _removeApiKeyProviderAuthEntries
    // 的 guard（!this._authStorage）通过——否则 0.83.0 改 async 后，migration 在
    // init() 里被整体跳过，rescue 的 api_key 进不了 catalog，紧随其后的 syncModels
    // 会用无 key 的 projection 覆盖 models.json，把待抢救的 key 永久丢掉。
    // raw AuthStorage 提供 reload；has/remove 缺失时 optional chaining 跳过删除，
    // syncAndRefresh() 装上 SdkAuthFacade 后会重跑完整 auth.json 清理。
    this._authStorage = rawAuthStorage;
    await this._removeApiKeyProviderAuthEntries();
    const projection = this._buildChatProjectionInputs();
    syncModels(projection.providers, {
      modelsJsonPath: this.modelsJsonPath,
      chatProjectionPlans: projection.planMap,
    });
    // 0.83.0：ModelRuntime 是新装配点，ModelRegistry 变成它的同步 facade。
    // credentials 直接传 rawAuthStorage（AuthStorage 实现 CredentialStore），
    // auth.json 同一把锁语义保住。
    const { modelRuntime, modelRegistry } = await createModelRegistry(
      rawAuthStorage,
      path.join(this._lingxiHome, "models.json"),
    );
    this._modelRuntime = modelRuntime;
    this._modelRegistry = modelRegistry;
    // 装上 SdkAuthFacade：把旧 AuthStorage 形状（getOAuthProviders/getApiKey/
    // setRuntimeApiKey 等）桥接到 ModelRuntime，下游与测试按旧形状调用即可。
    this._authStorage = new SdkAuthFacade({ authStorage: rawAuthStorage, modelRuntime });
    await this._applyRuntimeApiKeyOverrides(projection);
    this._syncSdkProviderRegistrations();

  }

  // ── Getters ──

  get authStorage() { return this._authStorage; }
  get modelRegistry() { return this._modelRegistry; }
  /** 0.83.0 新装配点：createAgentSession 收 modelRuntime 而非 authStorage/modelRegistry。 */
  get modelRuntime() { return this._modelRuntime; }
  get defaultModel() { return this._defaultModel; }
  set defaultModel(m) { this._defaultModel = m; }
  get currentModel() { return this._defaultModel; }
  get availableModels() { return this._availableModels; }
  get modelsJsonPath() { return path.join(this._lingxiHome, "models.json"); }
  get authJsonPath() { return path.join(this._lingxiHome, "auth.json"); }

  // ── 模型解析：_availableModels 唯一真理源 ──

  /**
   * 从 _availableModels 解析模型引用。
   *
   * 合法输入（通过 parseModelRef 规整后必须带 provider）：
   *   - {id, provider} 对象
   *   - "provider/id" 字符串
   *
   * 裸 id 字符串**不合法**——历史数据走 migrations #5，运行期调用方必须显式带 provider。
   * ref 无法解析出 provider 时返 null（不按 id 降级猜）。
   *
   * @param {string|object} ref - 模型引用
   * @returns {object|null} SDK 模型对象
   */
  _resolveFromAvailable(ref) {
    const parsed = parseModelRef(ref);
    if (!parsed?.id || !parsed.provider) return null;
    return findModel(this._availableModels, parsed.id, parsed.provider) || null;
  }

  // ── 刷新 ──

  _getPersistedModelDefaultThinkingLevel(model) {
    if (!model?.provider || !model.id) return null;
    if (typeof this.providerRegistry.getModelDefaultThinkingLevel !== "function") return null;
    return this.providerRegistry.getModelDefaultThinkingLevel(model.provider, model.id);
  }

  _withPersistedModelDefaultThinkingLevel(model) {
    const level = this._getPersistedModelDefaultThinkingLevel(model);
    return level ? { ...model, defaultThinkingLevel: level } : model;
  }

  /** 刷新可用模型列表，用 Provider Catalog v2 过滤 */
  async refreshAvailable() {
    // 0.83.0：ModelRegistry.getAvailable() 改成同步读 snapshot，不再 await 内部
    // availability 刷新（logout/registerProvider 后的 refresh 是 fire-and-forget）。
    // 要拿到刷新后的可用模型，必须走 ModelRuntime.getAvailable()（async，等刷新）。
    // 兜底：_modelRuntime 未装配时（如部分单元测试只 mock _modelRegistry）回退到
    // _modelRegistry.getAvailable()，保持旧契约可用。
    const allModels = this._modelRuntime
      ? await this._modelRuntime.getAvailable()
      : await this._modelRegistry.getAvailable();
    const plans = this.providerRegistry.getChatProjectionPlans();
    const effectiveModelSets = new Map();
    const legacyRuntimeCatalogProviders = new Set();
    for (const plan of plans) {
      if (plan.projection === "none") continue;
      if (plan.projection === "sdk-auth-alias" && plan.selectionMode === "runtime-catalog") {
        legacyRuntimeCatalogProviders.add(plan.runtimeProviderId);
        continue;
      }
      const ids = new Set((plan.config?.models || []).map(modelEntryId).filter(Boolean));
      effectiveModelSets.set(plan.runtimeProviderId, ids);
    }
    const metadataByModel = buildProviderModelMetadataMap(plans);
    this._availableModels = allModels.filter(m => {
      if (legacyRuntimeCatalogProviders.has(m.provider)) return true;
      const allowed = effectiveModelSets.get(m.provider);
      return !!allowed && allowed.has(m.id);
    })
      .map(m => applyProviderModelMetadata(m, metadataByModel))
      .map(enrichModelFromKnownMetadata)
      .map(m => this._withPersistedModelDefaultThinkingLevel(m));
    return this._availableModels;
  }

  /**
   * 同步 Provider Catalog provider configs → models.json，然后刷新 ModelRegistry。
   *
   * ⚠ 刷新后 _availableModels 是全新数组，旧的 model 对象引用（含烤在字段里的
   * 过期 baseUrl）会失效。本方法负责把 _defaultModel 指针也重新定位到新数组里
   * 的对应对象——否则新建 session 会继续用旧 baseUrl 发请求（provider 改端点后
   * 出现 429 的根因）。
   *
   * @returns {boolean} 是否有变化
   */
  async syncAndRefresh() {
    await this._removeApiKeyProviderAuthEntries();
    const projection = this._buildChatProjectionInputs();
    const changed = syncModels(projection.providers, {
      modelsJsonPath: this.modelsJsonPath,
      chatProjectionPlans: projection.planMap,
    });
    await this._applyRuntimeApiKeyOverrides(projection);
    if (changed) {
      // 0.83.0：ModelRegistry.refresh() 改 async（await runtime.refresh()）。
      // 不 await 会让下面的 refreshAvailable() 读到旧 availability 快照。
      await this._modelRegistry.refresh();
    }
    await this.refreshAvailable();
    this._rebindDefaultModel();
    return changed;
  }

  /**
   * Reconcile ProviderRegistry-owned dynamic SDK declarations with this
   * ModelRegistry instance. The set belongs to ModelManager because dynamic
   * registration is lifecycle state of this concrete SDK registry.
   */
  _syncSdkProviderRegistrations() {
    if (!this._modelRegistry) return;
    const registrations = this.providerRegistry.getSdkProviderRegistrations();
    const nextIds = new Set<string>(registrations.map((registration) => registration.providerId));
    // ModelRegistry.registerProvider is an upsert: omitted fields survive from
    // the prior config. Remove every previously owned declaration first so a
    // catalog/plugin reload has exact replacement semantics, including deleted
    // oauth, headers, or hooks.
    for (const providerId of this._registeredSdkProviderIds) {
      unregisterModelProvider(this._modelRegistry, providerId);
    }
    const appliedIds: string[] = [];
    try {
      for (const registration of registrations) {
        registerModelProvider(
          this._modelRegistry,
          registration.providerId,
          registration.config,
        );
        appliedIds.push(registration.providerId);
      }
    } catch (error) {
      for (const providerId of appliedIds) {
        unregisterModelProvider(this._modelRegistry, providerId);
      }
      this._registeredSdkProviderIds = new Set();
      throw error;
    }
    this._registeredSdkProviderIds = nextIds;
  }

  _buildChatProjectionInputs() {
    const plans = this.providerRegistry.getChatProjectionPlans();
    const providers: Record<string, any> = {};
    const planMap: Record<string, any> = {};
    for (const plan of plans) {
      providers[plan.sourceProviderId] = plan.config;
      planMap[plan.sourceProviderId] = {
        sourceProviderId: plan.sourceProviderId,
        runtimeProviderId: plan.runtimeProviderId,
        projection: plan.projection,
        credentialSource: plan.credentialSource,
        selectionMode: plan.selectionMode,
        hasExplicitModels: plan.hasExplicitModels,
        modelExecutionHeaders: plan.modelExecutionHeaders,
      };
    }
    return { plans, providers, planMap };
  }

  async _applyRuntimeApiKeyOverrides(projection) {
    if (!this._authStorage?.setRuntimeApiKey) return;
    for (const plan of projection?.plans || []) {
      const provider = projection.providers?.[plan.sourceProviderId] || {};
      const runtimeProviderId = plan.runtimeProviderId || plan.sourceProviderId;
      const cleanupIds = new Set([runtimeProviderId, plan.sourceProviderId]);
      if (plan.credentialSource === "provider-catalog"
        && typeof provider.api_key === "string"
        && provider.api_key.length > 0) {
        await this._authStorage.setRuntimeApiKey(runtimeProviderId, provider.api_key);
        for (const providerId of cleanupIds) {
          if (providerId !== runtimeProviderId) {
            await this._authStorage.removeRuntimeApiKey?.(providerId);
          }
        }
      } else {
        for (const providerId of cleanupIds) {
          await this._authStorage.removeRuntimeApiKey?.(providerId);
        }
      }
    }
  }

  /**
   * _availableModels 重建后，把 _defaultModel 重新绑到新数组里的对应对象。
   * 找不到则置 null（provider 被删、模型消失等）。
   * @private
   */
  _rebindDefaultModel() {
    if (!this._defaultModel) return;
    const { id, provider } = this._defaultModel;
    if (!id || !provider) {
      this._defaultModel = null;
      return;
    }
    this._defaultModel = findModel(this._availableModels, id, provider) || null;
  }

  /**
   * Hana 的 API-key provider 凭证源是 Provider Catalog → models.json。
   * AuthStorage 只保留 OAuth 条目，避免 Pi SDK 优先读取 stale auth.json。
   * @private
   */
  async _removeApiKeyProviderAuthEntries() {
    if (!this._authStorage || !this.providerRegistry) return;
    migrateLegacyApiKeyAuthToProviders({
      lingxiHome: this._lingxiHome,
      providerRegistry: this.providerRegistry,
    });
    this._authStorage.reload?.();

    const entries = [...this.providerRegistry.getAll().values()];
    const oauthOwnedAuthKeys = new Set();
    for (const entry of entries) {
      if (entry.authType !== "oauth") continue;
      if (entry.id) oauthOwnedAuthKeys.add(entry.id);
      if (entry.authJsonKey) oauthOwnedAuthKeys.add(entry.authJsonKey);
    }

    // 0.83.0：AuthStorage.delete 改 async（withLockAsync）。收集待删 key 并行 await，
    // 避免 fire-and-forget 在测试清理 tmpDir 后才落地导致 ENOENT。
    //
    // 0.84.1：raw AuthStorage 不再暴露 has/remove（CredentialStore 契约只剩
    // read/list/modify/delete）。本方法在 init() 早期跑时 this._authStorage 还是
    // raw AuthStorage（has/remove 缺失），syncAndRefresh() 装上 SdkAuthFacade 后重跑
    // 才有 has/remove。因此存在性检查与删除都按"任一可用 API"适配：
    //   has  → facade.has | raw.read?.() | raw.get?.()
    //   删除 → remove?.() | delete?.()
    // 不放宽语义：没有凭证的 key 不调 delete（保持旧的"只在有时才写盘"行为）。
    const authStorage: any = this._authStorage;
    const hasAuthEntry = async (key: string): Promise<boolean> => {
      if (typeof authStorage.has === "function") return !!authStorage.has(key);
      if (typeof authStorage.read === "function") return (await authStorage.read(key)) !== undefined;
      if (typeof authStorage.get === "function") return authStorage.get(key) != null;
      return false;
    };
    const removeAuthEntry = (key: string): Promise<unknown> => {
      if (typeof authStorage.remove === "function") return Promise.resolve(authStorage.remove(key));
      if (typeof authStorage.delete === "function") return Promise.resolve(authStorage.delete(key));
      return Promise.resolve();
    };
    const removals: Promise<unknown>[] = [];
    for (const entry of entries) {
      if (entry.authType === "oauth") continue;
      const authKeys = new Set([entry.id, entry.authJsonKey]);
      for (const authKey of authKeys) {
        // A malformed/synthetic provider may collide with an OAuth runtime alias
        // (for example `openai-codex`). Projection validation will reject that
        // catalog, but cleanup must never delete the OAuth owner's credentials
        // while surfacing the collision.
        if (oauthOwnedAuthKeys.has(authKey)) continue;
        if (!authKey) continue;
        // gate the delete on actual presence (avoid no-op writes / masking bugs)
        removals.push(hasAuthEntry(authKey).then((present) => (present ? removeAuthEntry(authKey) : undefined)));
      }
    }
    await Promise.all(removals);
    this._authStorage.reload?.();
  }

  /**
   * 设置 agent 默认模型
   * @returns {object} 新模型对象
   */
  setDefaultModel(modelId, provider) {
    const model = findModel(this._availableModels, modelId, provider);
    if (!model) throw new Error(t("error.modelNotFound", { id: modelId }));
    this._defaultModel = model;
    return model;
  }

  /** Convert Hana-visible thinking levels to the Pi SDK session contract. */
  resolveThinkingLevel(level) {
    return normalizePiSdkThinkingLevel(level);
  }

  _resolveModelForThinkingDefault(modelRef) {
    if (!modelRef) return this.currentModel;
    if (typeof modelRef === "object" && modelRef.id && modelRef.provider) {
      return findModel(this._availableModels, modelRef.id, modelRef.provider) || modelRef;
    }
    return this.resolveExecutionModel(modelRef);
  }

  getModelDefaultThinkingLevel(modelRef = null, fallback = "medium") {
    const model = this._resolveModelForThinkingDefault(modelRef);
    const effectiveModel = this._withPersistedModelDefaultThinkingLevel(model);
    return resolveModelDefaultThinkingLevel(effectiveModel, normalizeSessionThinkingLevel(fallback));
  }

  async setModelDefaultThinkingLevel(modelRef, level) {
    const model = this._resolveModelForThinkingDefault(modelRef);
    if (!model?.id || !model.provider) {
      throw new Error("setModelDefaultThinkingLevel: model id and provider required");
    }
    const nextLevel = normalizeThinkingLevelForModel(level, model);
    this.providerRegistry.setModelDefaultThinkingLevel(model.provider, model.id, nextLevel);
    await this.syncAndRefresh();
    await this.refreshAvailable();
    this._rebindDefaultModel();
    const refreshed = findModel(this._availableModels, model.id, model.provider)
      || { ...model, defaultThinkingLevel: nextLevel };
    return {
      model: refreshed,
      thinkingLevel: resolveModelDefaultThinkingLevel(refreshed, nextLevel),
    };
  }

  /**
   * 将模型引用（provider/id 或 {id, provider}）解析成 SDK 可用的模型对象
   * 只查 _availableModels（唯一真理源）
   */
  resolveExecutionModel(modelRef) {
    if (!modelRef) return this.currentModel;
    if (typeof modelRef === "string" && !modelRef.trim()) return this.currentModel;

    const parsed = parseModelRef(modelRef);
    const model = parsed?.id && parsed.provider
      ? findModel(this._availableModels, parsed.id, parsed.provider)
      : null;
    if (model) return model;

    const id = parsed?.id
      ? (parsed.provider ? `${parsed.provider}/${parsed.id}` : parsed.id)
      : String(modelRef);
    throw new Error(t("error.modelNotFound", { id }));
  }

  /** 同步设置校验只返回模型目录对象，不读取或返回任何凭据。 */
  resolveModelForValidation(modelRef) {
    return this.resolveExecutionModel(modelRef);
  }

  /** 仅供受权限保护的设置明文查看端点读取 Provider Catalog 中已保存的 API Key。 */
  readSavedProviderApiKey(provider) {
    if (!provider) return "";
    const entry = this.providerRegistry.get?.(provider);
    const allRawProviders = this.providerRegistry.getAllProvidersRaw?.() || {};
    const raw = allRawProviders[entry?.id || provider] || allRawProviders[provider] || {};
    return typeof raw.api_key === "string" ? raw.api_key : "";
  }

  /**
   * OAuth-aware provider credential resolution for non-chat runtimes.
   *
   * Chat execution goes through Pi SDK ModelRegistry, whose AuthStorage path
   * refreshes OAuth tokens. Media adapters historically bypassed that path by
   * reading ProviderRegistry credentials directly, so they could keep using an
   * expired access token until a chat request refreshed it. This method makes
   * the refresh boundary explicit without moving adapter-specific semantics into
   * ProviderRegistry.
   *
   * `options.forceRefresh` goes one step further: it rotates the OAuth
   * credential regardless of the locally recorded expiry, for callers whose
   * request was just rejected by the provider. `options.staleApiKey` is the
   * token that got rejected, so a concurrent rotation is not repeated.
   *
   * @param {string} provider
   * @param {{ forceRefresh?: boolean, staleApiKey?: string }} [options]
   * @returns {Promise<{ api_key: string, base_url: string, api: string, accountId?: string }>}
   */
  async resolveProviderCredentialsFresh(provider, options: { forceRefresh?: boolean, staleApiKey?: string } = {}) {
    if (!provider) return { api_key: "", base_url: "", api: "" };
    const chatProvider = this.providerRegistry.resolveChatProvider?.(provider);
    const entry = chatProvider?.entry || this.providerRegistry.get(provider);
    const credentialSource = chatProvider?.credentialSource
      || (this.providerRegistry.getAuthType(provider) === "oauth" ? "auth-storage" : "provider-catalog");
    if (!entry) return { api_key: "", base_url: "", api: "" };
    let refreshedOAuthKey = null;
    if (credentialSource === "auth-storage") {
      if (!entry) {
        throw new Error(t("error.providerMissingCreds", { provider }));
      }
      const rawProvider = this.providerRegistry.getAllProvidersRaw?.()[entry.id] || {};
      const authKey = this.providerRegistry.getAuthJsonKey(provider);
      if (!this._authStorage || typeof this._authStorage.getApiKey !== "function") {
        throw new Error(`${t("error.providerMissingCreds", { provider })} (auth: ${authKey})`);
      }
      refreshedOAuthKey = options.forceRefresh
        ? await forceRefreshOAuthApiKey({
            modelRuntime: this._modelRuntime,
            backend: this._authBackend,
            authKey,
            staleApiKey: options.staleApiKey,
          })
        : await this._authStorage.getApiKey(authKey, { includeFallback: false });
      this._authStorage.reload?.();
      this.providerRegistry.clearAuthCache?.();
      if (!refreshedOAuthKey) {
        throw new Error(`${t("error.providerMissingCreds", { provider })} (auth: ${authKey})`);
      }
      const authEntry = this._authStorage.get?.(authKey) || null;
      const cred = this.providerRegistry.getCredentials(provider);
      const accountId = authEntry?.accountId || authEntry?.account_id || cred?.accountId || "";
      return {
        // OAuth execution credentials belong exclusively to AuthStorage. In particular,
        // never let a stale Provider Catalog api_key or Authorization/Cookie header
        // override the token that was just refreshed under the AuthStorage lock.
        api_key: refreshedOAuthKey || "",
        base_url: authEntry?.resourceUrl
          || authEntry?.resource_url
          || rawProvider.base_url
          || cred?.baseUrl
          || entry.baseUrl
          || "",
        api: rawProvider.api || entry.api || cred?.api || "",
        headers: stripCredentialHeaders(entry.headers || {}),
        credential_source: "auth-storage",
        ...(accountId ? { accountId } : {}),
      };
    }
    if (credentialSource === "provider-catalog" && entry) {
      const allRawProviders = this.providerRegistry.getAllProvidersRaw?.() || {};
      const rawProvider = allRawProviders[entry.id] || allRawProviders[provider] || {};
      return {
        api_key: rawProvider.api_key || "",
        base_url: rawProvider.base_url || entry.baseUrl || "",
        api: rawProvider.api || entry.api || "",
        headers: rawProvider.headers || entry.headers || {},
        credential_source: "provider-catalog",
      };
    }
    if (credentialSource === "none" && entry) {
      const allRawProviders = this.providerRegistry.getAllProvidersRaw?.() || {};
      const rawProvider = allRawProviders[entry.id] || allRawProviders[provider] || {};
      return {
        api_key: "",
        base_url: rawProvider.base_url || entry.baseUrl || "",
        api: rawProvider.api || entry.api || "",
        headers: rawProvider.headers || entry.headers || {},
        credential_source: "none",
      };
    }
    throw new Error(`Unsupported credentialSource "${credentialSource}" for provider "${provider}"`);
  }

  _resolvedModelCredentialResult(entry, creds) {
    const provider = entry?.provider;
    if (!provider) {
      throw new Error(t("error.modelNoProvider", { role: "resolve", model: String(entry?.id || "") }));
    }
    const declaredCredentialSource = creds?.credential_source || creds?.credentialSource;
    const inferredCredentialSource = declaredCredentialSource
      || this.providerRegistry?.resolveChatProvider?.(provider)?.credentialSource
      || (this.providerRegistry?.getAuthType?.(provider) === "oauth" ? "auth-storage" : "");
    const execution = composeResolvedModelExecution({
      model: entry,
      credential: inferredCredentialSource
        ? { ...creds, credential_source: inferredCredentialSource }
        : creds,
    });
    if (!execution.api) {
      throw new Error(t("error.providerMissingApi", { provider }));
    }
    const allowsMissingApiKey = this.providerRegistry?.allowsMissingApiKey?.(provider, execution.baseUrl)
      ?? isLocalBaseUrl(execution.baseUrl);
    const credentialHeaders = creds?.headers && typeof creds.headers === "object" ? creds.headers : {};
    const hasCredentialHeaders = Object.keys(credentialHeaders).length > 0;
    if (!execution.baseUrl || (!execution.apiKey && !hasCredentialHeaders && !allowsMissingApiKey)) {
      throw new Error(t("error.providerMissingCreds", { provider }));
    }
    return {
      model: execution.model,
      provider,
      api: execution.api,
      api_key: execution.apiKey,
      base_url: execution.baseUrl,
      headers: execution.headers,
      ...(execution.credentialSource ? { credential_source: execution.credentialSource } : {}),
      ...(execution.accountId ? { accountId: execution.accountId } : {}),
    };
  }

  /**
   * Provider 配置变更后 reload registry + 重新同步模型。
   * 由 engine.onProviderChanged() 调用，不要直接用。
   */
  async reloadAndSync() {
    this.providerRegistry.reload();
    this._syncSdkProviderRegistrations();
    await this.syncAndRefresh();
  }

  /**
   * 请求时解析模型与凭证。模型身份始终先从 Hana availableModels 解析，
   * 其协议和规范化 endpoint 保持权威。凭证随后在请求边界刷新并只补充
   * auth/account/headers；OAuth 的动态 resourceUrl 是 endpoint 唯一例外。
   * 不会回退到 ProviderRegistry 缓存中的旧 access token。
   */
  async resolveModelWithCredentialsFresh(modelRef) {
    const entry = this.resolveExecutionModel(modelRef);
    const provider = entry?.provider;
    if (!provider) {
      throw new Error(t("error.modelNoProvider", { role: "resolve", model: String(modelRef) }));
    }
    const creds = await this.resolveProviderCredentialsFresh(provider);
    return this._resolvedModelCredentialResult(entry, creds);
  }

  /**
   * 从 Pi SDK registry 获取某 provider 的所有模型（不经过 Provider Catalog 过滤）
   * 用于模型发现（fetch-models），不影响主应用的 availableModels
   * @param {string} name - provider ID
   * @returns {object[]}
   */
  getRegistryModelsForProvider(name) {
    const authKey = this.providerRegistry.getAuthJsonKey(name);
    const all = this._modelRegistry.getAll();
    return all.filter(m => m.provider === name || m.provider === authKey);
  }
}
