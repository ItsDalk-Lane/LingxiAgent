/**
 * AuxiliaryModelResolver — 统一的语义 Slot 解析器。
 *
 * 业务层唯一的辅助模型入口：
 *
 *   resolveAuxiliaryModel("summarize")          → { api, apiKey, baseUrl, headers, model }
 *   resolveAuxiliaryModelFresh("memory", {...})  → 同上（请求边界刷新凭证）
 *   resolveAuxiliaryModelExecution("vision")     → 完整 execution 对象（含 model/provider/credentialSource）
 *
 * 一次 resolve 只解析一个 Slot。Slot 不拥有 credential；
 * Provider credential 基础设施是唯一执行凭证来源。
 *
 * “未配置”和“配置错误”严格区分：
 *   未配置 → 按 Slot 策略 fallback（chat / image_capable_chat / none）
 *   已配置但不可用 → 报告配置错误，不 fallback
 */

import {
  AUXILIARY_SLOTS,
  type AuxiliarySlot,
  validateAuxiliaryModelCapability,
  AuxiliaryConfigurationError,
  isAuxiliaryConfigError,
} from "./auxiliary-slots.ts";
import { modelSupportsImageInput } from "../shared/model-capabilities.ts";
import { callTextConfigFromResolvedModel, composeResolvedModelExecution } from "./model-execution-config.ts";
import { t } from "../lib/i18n.ts";
import { createModuleLogger } from "../lib/debug-log.ts";

const log = createModuleLogger("aux-resolver");

export interface AuxiliaryResolveContext {
  agentId?: string | null;
  sessionPath?: string | null;
  sessionId?: string | null;
}

export interface ResolvedAuxiliaryExecution {
  model: any;
  provider: string;
  api: string;
  apiKey: string;
  baseUrl: string;
  headers: Record<string, string>;
  credentialSource?: string;
  accountId?: string;
}

export interface AuxiliaryResolveDeps {
  /** 从 _availableModels 解析模型对象。返回 null 表示模型不可解析。 */
  resolveModel: (ref: any) => any | null;
  /** 同步解析 chat 模型（用于 fallback）。返回完整 model 对象或 null。 */
  getChatModel: (agentId?: string | null) => any | null;
  /** 异步刷新 provider 凭证（请求边界）。 */
  resolveProviderCredentialsFresh: (
    provider: string,
  ) => Promise<any>;
  /** 同步读 provider 缓存凭证。 */
  getProviderCredentials: (provider: string) => any | null;
  /** 读取 Slot 对应的 ModelRef（来自 preferences），返回 null 表示未配置。 */
  getSlotModelRef: (slot: AuxiliarySlot) => any | null;
  /** 是否允许缺失 api_key（本地 endpoint）。 */
  allowsMissingApiKey?: (provider: string, baseUrl: string) => boolean;
}

function isLocalBaseUrl(baseUrl: string): boolean {
  if (!baseUrl) return false;
  try {
    const u = new URL(baseUrl);
    return u.hostname === "localhost" || u.hostname === "127.0.0.1" || u.hostname === "0.0.0.0";
  } catch {
    return false;
  }
}

function hasCredentialHeaders(cred: any): boolean {
  return (
    !!cred?.headers &&
    typeof cred.headers === "object" &&
    Object.keys(cred.headers).length > 0
  );
}

// re-export 配置错误类型，便于消费方从 resolver 模块一并引入。
export { AuxiliaryConfigurationError, isAuxiliaryConfigError };


export class AuxiliaryModelResolver {
  declare _deps: AuxiliaryResolveDeps;

  constructor(deps: AuxiliaryResolveDeps) {
    this._deps = deps;
  }

  /**
   * 解析 chat fallback 模型。
   * 对 vision slot：chat 模型必须支持 image input，否则返回 null（vision unavailable）。
   */
  private _resolveChatFallback(
    slot: AuxiliarySlot,
    agentId?: string | null,
  ): any | null {
    const descriptor = AUXILIARY_SLOTS[slot];
    const chatModel = this._deps.getChatModel(agentId);
    if (!chatModel) return null;
    if (descriptor.fallback === "image_capable_chat") {
      if (!modelSupportsImageInput(chatModel)) return null;
    }
    return chatModel;
  }

  /**
   * 确定最终要用的 model 对象。
   * - Slot 已配置：解析该 ModelRef。解析失败（模型不存在）→ 报配置错误。
   * - Slot 未配置：按 fallback 策略。
   */
  private _selectModel(
    slot: AuxiliarySlot,
    agentId?: string | null,
  ): { model: any; fromSlot: boolean } {
    const descriptor = AUXILIARY_SLOTS[slot];
    const slotRef = this._deps.getSlotModelRef(slot);

    if (slotRef) {
      // 已配置：解析该模型
      const model = this._deps.resolveModel(slotRef);
      if (!model) {
        // 已配置但模型不存在 → 配置错误，不 fallback
        const refStr =
          typeof slotRef === "object"
            ? `${slotRef.provider || "?"}/${slotRef.id || "?"}`
            : String(slotRef);
        throw new AuxiliaryConfigurationError(
          t("error.auxiliarySlotModelNotFound", { slot, ref: refStr }),
          "model_not_found",
          slot,
        );
      }
      // capability 校验（vision 模型必须支持 image input）
      validateAuxiliaryModelCapability(slot, model);
      return { model, fromSlot: true };
    }

    // 未配置：fallback
    if (descriptor.fallback === "none") {
      return { model: null, fromSlot: false };
    }
    const fallbackModel = this._resolveChatFallback(slot, agentId);
    return { model: fallbackModel, fromSlot: false };
  }

  /**
   * 同步解析辅助模型，返回 callText 格式的执行配置。
   *
   * 注意：同步版本使用缓存的 provider 凭证，不刷新 OAuth。
   * 真实推理调用应使用 resolveAuxiliaryModelFresh。
   */
  resolveAuxiliaryModel(
    slot: AuxiliarySlot,
    context: AuxiliaryResolveContext = {},
  ): any | null {
    const execution = this.resolveAuxiliaryExecutionSync(slot, context);
    if (!execution) return null;
    return callTextConfigFromResolvedModel(execution);
  }

  /**
   * 同步解析辅助模型，返回完整 execution 对象。
   */
  resolveAuxiliaryExecutionSync(
    slot: AuxiliarySlot,
    context: AuxiliaryResolveContext = {},
  ): ResolvedAuxiliaryExecution | null {
    const { model } = this._selectModel(slot, context.agentId);
    if (!model) {
      log.log(`resolveAuxiliaryExecutionSync(${slot}): no model (unconfigured, fallback=${AUXILIARY_SLOTS[slot].fallback})`);
      return null;
    }

    const cred = this._deps.getProviderCredentials(model.provider);
    const api = model.api || cred?.api;
    if (!api) {
      throw new AuxiliaryConfigurationError(
        t("error.providerMissingApi", { provider: model.provider }),
        "provider_missing_api",
        slot,
      );
    }
    const allowsMissingApiKey =
      this._deps.allowsMissingApiKey?.(model.provider, cred?.baseUrl || "") ??
      isLocalBaseUrl(cred?.baseUrl || "");
    if (
      !cred?.baseUrl ||
      (!cred.apiKey && !hasCredentialHeaders(cred) && !allowsMissingApiKey)
    ) {
      throw new AuxiliaryConfigurationError(
        t("error.providerMissingCreds", { provider: model.provider }),
        "provider_missing_creds",
        slot,
      );
    }

    const composed = composeResolvedModelExecution({ model, credential: cred });
    return {
      model: composed.model,
      provider: composed.provider,
      api: composed.api,
      apiKey: composed.apiKey,
      baseUrl: composed.baseUrl,
      headers: composed.headers,
      ...(composed.credentialSource ? { credentialSource: composed.credentialSource } : {}),
      ...(composed.accountId ? { accountId: composed.accountId } : {}),
    };
  }

  /**
   * 异步解析辅助模型（请求边界刷新凭证）。
   * 返回 callText 格式的执行配置。
   */
  async resolveAuxiliaryModelFresh(
    slot: AuxiliarySlot,
    context: AuxiliaryResolveContext = {},
  ): Promise<any | null> {
    const execution = await this.resolveAuxiliaryExecution(slot, context);
    if (!execution) return null;
    return callTextConfigFromResolvedModel(execution);
  }

  /**
   * 异步解析辅助模型，返回完整 execution 对象。
   * VisionBridge 等需要完整 model 对象的消费方使用此方法。
   */
  async resolveAuxiliaryExecution(
    slot: AuxiliarySlot,
    context: AuxiliaryResolveContext = {},
  ): Promise<ResolvedAuxiliaryExecution | null> {
    const { model, fromSlot } = this._selectModel(slot, context.agentId);
    if (!model) {
      log.log(`resolveAuxiliaryExecution(${slot}): no model (unconfigured, fallback=${AUXILIARY_SLOTS[slot].fallback})`);
      return null;
    }

    const cred = await this._deps.resolveProviderCredentialsFresh(model.provider);
    const api = model.api || cred?.api;
    if (!api) {
      throw new AuxiliaryConfigurationError(
        t("error.providerMissingApi", { provider: model.provider }),
        "provider_missing_api",
        slot,
      );
    }
    const allowsMissingApiKey =
      this._deps.allowsMissingApiKey?.(model.provider, cred?.baseUrl || "") ??
      isLocalBaseUrl(cred?.baseUrl || "");
    if (
      !cred?.baseUrl ||
      (!cred.apiKey && !hasCredentialHeaders(cred) && !allowsMissingApiKey)
    ) {
      throw new AuxiliaryConfigurationError(
        t("error.providerMissingCreds", { provider: model.provider }),
        "provider_missing_creds",
        slot,
      );
    }

    const composed = composeResolvedModelExecution({ model, credential: cred });
    return {
      model: composed.model,
      provider: composed.provider,
      api: composed.api,
      apiKey: composed.apiKey,
      baseUrl: composed.baseUrl,
      headers: composed.headers,
      ...(composed.credentialSource ? { credentialSource: composed.credentialSource } : {}),
      ...(composed.accountId ? { accountId: composed.accountId } : {}),
    };
  }
}
