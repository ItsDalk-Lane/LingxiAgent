/**
 * ExecutionRouter -- per-agent 角色路由（通用 resolve）。
 *
 * 辅助模型角色（utility/utility_large）已迁移到语义 Slot 体系
 *（core/auxiliary-model-resolver.ts）。本文件仅保留通用的角色→执行参数
 * 解析方法 resolve()，供 chat/embed 等非辅助模型角色使用。
 *
 * 辅助模型的解析请使用 AuxiliaryModelResolver。
 */

import { t } from "../lib/i18n.ts";
import { isLocalBaseUrl } from "../shared/net-utils.ts";

function hasCredentialHeaders(cred: any) {
  return !!cred?.headers && typeof cred.headers === "object" && Object.keys(cred.headers).length > 0;
}

export class ExecutionRouter {
  declare _resolveModel: (ref: string) => any;
  declare _providerRegistry: any;

  /**
   * @param {(ref: string) => object|null} resolveModel - 从 _availableModels 解析模型的函数
   * @param {import('./provider-registry.ts').ProviderRegistry} providerRegistry
   */
  constructor(resolveModel: any, providerRegistry: any, _resolveProviderCredentialsFresh: any = null) {
    this._resolveModel = resolveModel;
    this._providerRegistry = providerRegistry;
  }

  /**
   * 解析角色 -> 完整执行参数（chat/embed 或裸模型引用）
   *
   * @param {string} roleOrRef
   *   角色名（"chat"/"embed"）或直接是模型引用（"provider/model" 或裸 modelId）
   * @param {object} agentConfig - agent config 对象（来自 config.yaml）
   * @param {object} [sharedModels] - 全局共享角色模型（来自 preferences）
   * @returns {{ modelId: string, providerId: string, api: string, apiKey: string, baseUrl: string }}
   * @throws 找不到模型或凭证时抛出
   */
  resolve(roleOrRef, agentConfig, sharedModels) {
    const modelRef = this._resolveRef(roleOrRef, agentConfig, sharedModels);
    if (!modelRef) {
      throw new Error(t("error.modelNotFound", { id: roleOrRef }));
    }

    const model = this._resolveModel(modelRef);
    if (!model) {
      throw new Error(t("error.modelNotFound", { id: modelRef }));
    }

    const cred = this._providerRegistry.getCredentials(model.provider);
    if (!cred) {
      throw new Error(t("error.providerMissingCreds", { provider: model.provider }));
    }
    const effectiveApi = model.api || cred.api;
    if (!effectiveApi) {
      throw new Error(t("error.providerMissingApi", { provider: model.provider }));
    }
    if (!cred.baseUrl || (!cred.apiKey && !hasCredentialHeaders(cred) && !this._allowsMissingApiKey(model.provider, cred.baseUrl))) {
      throw new Error(t("error.providerMissingCreds", { provider: model.provider }));
    }

    return {
      modelId: model.id,
      providerId: model.provider,
      api: effectiveApi,
      apiKey: cred.apiKey,
      baseUrl: cred.baseUrl,
      headers: cred.headers || {},
      ...(cred.accountId ? { accountId: cred.accountId } : {}),
    };
  }

  /**
   * 将角色名或模型引用解析为实际模型 ref 字符串
   * @private
   */
  _resolveRef(roleOrRef, agentConfig, sharedModels) {
    const cfg = agentConfig || {};
    switch (roleOrRef) {
      case "chat":
        return cfg.models?.chat || null;
      case "embed":
        return cfg.embedding_api?.model || null;
      default:
        // 不是内置角色名，当作模型引用直接用
        return roleOrRef;
    }
  }

  _allowsMissingApiKey(provider, baseUrl) {
    return this._providerRegistry?.allowsMissingApiKey?.(provider, baseUrl)
      ?? isLocalBaseUrl(baseUrl);
  }
}
