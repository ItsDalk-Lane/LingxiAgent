/**
 * PI SDK Adapter — 所有 PI SDK 导入的唯一入口
 *
 * 稳定 API 直接 re-export，不稳定 API 通过适配函数封装。
 * 消费方不应直接 import "@earendil-works/..."，全部从这里导入。
 *
 * 纪律：
 *   - 不接受 engine / agent / config 参数
 *   - 不拼 session options（compaction、thinkingLevel 等）
 *   - 不做工具过滤 / plan mode 逻辑
 *   - 不持有任何状态
 */

import {
  createAgentSession as rawCreateAgentSession,
  ModelRegistry,
  ModelRuntime,
  resizeImage as rawResizeImage,
  formatDimensionNote as rawFormatDimensionNote,
  convertToLlm as rawConvertToLlm,
} from "@earendil-works/pi-coding-agent";
// 0.83.0 起 AuthStorage / FileAuthStorageBackend 不再从包根导出，但仍在
// dist/core/auth-storage.js（深路径相对引，对齐 compaction 的深路径引法）。
// AuthStorage 仍实现 CredentialStore，可作 ModelRuntime.create 的 credentials。
import {
  AuthStorage,
  FileAuthStorageBackend,
} from "../../node_modules/@earendil-works/pi-coding-agent/dist/core/auth-storage.js";
// 0.80.0 起 pi-ai 老全局 API 移到 /compat 子入口（根入口是 createModels 新 API）
import {
  getModel as rawGetPiModel,
  getModels as rawGetPiModels,
  completeSimple as rawCompleteSimple,
} from "@earendil-works/pi-ai/compat";
import {
  normalizeCreateAgentSessionOptions,
  PI_BUILTIN_TOOL_NAMES,
} from "./session-options.ts";
import { installAssistantStreamGuard } from "./stream-guard.ts";
import { installModelCallStreamObserver } from "./model-call-stream-observer.ts";
import { installToolOutcomeAdapter } from "./tool-outcome-adapter.ts";
import {
  createFindTool,
  createGrepTool,
} from "./search-tools.ts";
// 0.83.0 OAuth/凭证能力门面（桥接旧 AuthStorage 形状到 ModelRuntime）
export { SdkAuthFacade } from "./auth-facade.ts";
export type { LegacyOAuthProvider } from "./auth-facade.ts";
// prepareCompaction 0.80.3 仍未从包根导出，深路径保留（升级时必查此文件是否存在）
import {
  prepareCompaction as rawPrepareCompaction,
} from "../../node_modules/@earendil-works/pi-coding-agent/dist/core/compaction/compaction.js";

// ── Session 管理 ──
export { SessionManager, SettingsManager } from "@earendil-works/pi-coding-agent";

// ── 低层 AgentLoop（隔离 side lane 用）──
export { runAgentLoop } from "@earendil-works/pi-agent-core";
export type {
  AgentContext,
  AgentEvent,
  AgentLoopConfig,
  AgentMessage,
  AgentTool,
  StreamFn,
} from "@earendil-works/pi-agent-core";

/**
 * Hana 侧保持稳定的 Tool[] 调用契约，适配层负责转换 Pi SDK 版本差异。
 *
 * Pi SDK 0.68+ 将 `tools` 改成 string[] allowlist；Hana 的沙盒工具仍然
 * 是 session 级对象，必须先注册为同名 customTools，再用名字启用。
 *
 * @param {object} options
 */
export async function createAgentSession(options) {
  const resourceLoaderAgentDir = options?.resourceLoader?.agentDir;
  const sessionOptions = !options?.agentDir && typeof resourceLoaderAgentDir === "string" && resourceLoaderAgentDir
    ? { ...options, agentDir: resourceLoaderAgentDir }
    : options;
  const result = await rawCreateAgentSession(normalizeCreateAgentSessionOptions(sessionOptions));
  installToolOutcomeAdapter(result?.session);
  installAssistantStreamGuard(result?.session);
  installModelCallStreamObserver(result?.session);
  return result;
}

// ── 内置工具名常量 ──
export { PI_BUILTIN_TOOL_NAMES };

// ── 工具工厂（沙盒用）──
export {
  createReadTool, createWriteTool, createEditTool, createBashTool,
  createLsTool,
} from "@earendil-works/pi-coding-agent";
export { createGrepTool, createFindTool };

// ── 资源加载 ──
export { DefaultResourceLoader } from "@earendil-works/pi-coding-agent";

// ── Utilities ──
export { formatSkillsForPrompt, getLastAssistantUsage } from "@earendil-works/pi-coding-agent";
export { AuthStorage };
// The file-backed store is exported alongside AuthStorage because forcing a
// credential rotation has to take the same auth.json lock the SDK takes.
export { FileAuthStorageBackend };

// 0.83.0 起 OAuth 登录从 AuthStorage.login 迁到 ModelRuntime.login(providerId,
// type, interaction)。pi-ai 的 compat/extension-oauth-types 仍保留这套旧回调
// 形状（Hana 的 server/routes/auth.ts 按它构造回调），这里按下游契约显式声明。
export interface OAuthLoginCallbacks {
  onAuth(info: { url: string; instructions?: string }): void;
  onDeviceCode(info: {
    userCode: string;
    verificationUri: string;
    intervalSeconds?: number;
    expiresInSeconds?: number;
  }): void;
  onPrompt(prompt: { message: string; placeholder?: string; allowEmpty?: boolean }): Promise<string>;
  onProgress?(message: string): void;
  onManualCodeInput?(): Promise<string>;
  onSelect(prompt: { message: string; options: { id: string; label: string }[] }): Promise<string | undefined>;
  signal?: AbortSignal;
}

export type SdkProviderRegistrationConfig = Parameters<ModelRegistry["registerProvider"]>[1];
export type SdkOAuthProvider = NonNullable<SdkProviderRegistrationConfig["oauth"]>;

/**
 * OAuth login adapter.
 *
 * 0.83.0 起 AuthStorage.login 已删，登录走 ModelRuntime.login(providerId, type,
 * interaction)。本函数把 Hana 下游的旧 OAuthLoginCallbacks 形状适配成新的
 * AuthInteraction（prompt/notify/signal），语义双向保持：
 *   onAuth        ↔ notify({type:"auth_url"})
 *   onDeviceCode  ↔ notify({type:"device_code"})
 *   onPrompt      ↔ prompt({type:"text"})
 *   onSelect      ↔ prompt({type:"select"})
 *   onManualCodeInput ↔ prompt({type:"manual_code"})
 *   onProgress    ↔ notify({type:"progress"})
 *   signal        ↔ signal
 * 第二参数收 SdkAuthFacade | ModelRuntime：facade 直接读 .modelRuntime，
 * 兼容下游两种拿到的对象。
 */
export async function loginOAuthProvider(
  modelRuntimeOrFacade: any,
  providerId: string,
  callbacks: OAuthLoginCallbacks,
): Promise<void> {
  const modelRuntime: any = modelRuntimeOrFacade?.modelRuntime ?? modelRuntimeOrFacade;
  const interaction = buildAuthInteraction(callbacks);
  await modelRuntime.login(providerId, "oauth", interaction);
}

/**
 * 把旧的 OAuthLoginCallbacks 适配成 0.83.0 pi-ai AuthInteraction。
 * 仅本模块内部用，下游不直接调。
 */
function buildAuthInteraction(callbacks: OAuthLoginCallbacks) {
  return {
    signal: callbacks.signal,
    prompt(prompt: any): Promise<string> {
      if (prompt?.type === "select") {
        return callbacks.onSelect({
          message: prompt.message,
          options: (prompt.options || []).map((opt: any) => ({
            id: opt.id,
            label: opt.label,
            ...(opt.description ? { description: opt.description } : {}),
          })),
        }).then((id: string | undefined) => (typeof id === "string" ? id : ""));
      }
      if (prompt?.type === "manual_code") {
        return callbacks.onManualCodeInput
          ? callbacks.onManualCodeInput()
          : callbacks.onPrompt({ message: prompt.message ?? "Paste the authorization code" });
      }
      // text / secret 都走 onPrompt（Hana 不区分，密码由浏览器侧收集）
      return callbacks.onPrompt({
        message: prompt?.message ?? "",
        ...(prompt?.placeholder ? { placeholder: prompt.placeholder } : {}),
        ...(prompt?.allowEmpty ? { allowEmpty: prompt.allowEmpty } : {}),
      });
    },
    notify(event: any): void {
      if (!event) return;
      switch (event.type) {
        case "auth_url":
          callbacks.onAuth({ url: event.url, ...(event.instructions ? { instructions: event.instructions } : {}) });
          return;
        case "device_code":
          callbacks.onDeviceCode({
            userCode: event.userCode,
            verificationUri: event.verificationUri,
            ...(event.intervalSeconds != null ? { intervalSeconds: event.intervalSeconds } : {}),
            ...(event.expiresInSeconds != null ? { expiresInSeconds: event.expiresInSeconds } : {}),
          });
          return;
        case "progress":
        case "info":
          callbacks.onProgress?.(event.message);
          return;
        default:
          return;
      }
    },
  };
}

// ── Session/history utilities ──
export {
  calculateContextTokens,
  estimateTokens, findCutPoint,
  serializeConversation, shouldCompact,
  parseSessionEntries, buildSessionContext,
} from "@earendil-works/pi-coding-agent";

// Diary material summarization only. Context compaction must go through core/session-compactor.ts.
export { generateSummary } from "@earendil-works/pi-coding-agent";
export {
  buildNativeCompactionRequestShapes,
  NATIVE_SUMMARIZATION_SYSTEM_PROMPT,
} from "./compaction-request-shape.ts";

export const completeSimple = rawCompleteSimple;
export const convertAgentMessagesToLlm = rawConvertToLlm;
export const prepareCompaction = rawPrepareCompaction;

// ── pi-ai（直接依赖，版本与 pi-coding-agent 锁死同版本。注意：上游发布物
// 携带 overrides 致 npm 子树隔离，pi-coding-agent 下必然嵌套第二份 pi-ai，
// 根级同版本挡不住这份拷贝；typebox schema 为字符串键、事件流为鸭子类型，
// 跨实例安全，但任何"模块级单例注册表"类 API（如 pi-ai/oauth 的 provider
// registry）都会双实例互不可见，禁止经由本门面暴露）──
export { StringEnum } from "@earendil-works/pi-ai";

export function getPiModel(provider, modelId) {
  return rawGetPiModel(provider, modelId);
}

export function getPiModels(provider) {
  return rawGetPiModels(provider);
}

// ── Schema 构造（typebox 的 Type 透过 adapter，避免工具直接依赖第三方包名）──
export { Type } from "typebox";

// ── 类型 re-export（供 JSDoc 引用）──
/** @typedef {import('@earendil-works/pi-coding-agent').ToolDefinition} ToolDefinition */

// ── Lifecycle helpers ──

/**
 * Emit `session_shutdown` event to the session's extension runner.
 *
 * 为什么在 adapter 层实现而不从 SDK 导出:
 *   SDK 的 emitSessionShutdownEvent 辅助函数只在 core/extensions/runner.js
 *   内部暴露, 顶级 index.js 未 re-export。直接 import 深层路径会违反
 *   adapter 纪律。实现本身仅 7 行, 自己实现更干净。
 *
 * 契约: AgentSession.dispose() 本身不 emit shutdown, 调用方必须在
 *   dispose 前显式 emit, 否则监听 session_shutdown 的扩展(如
 *   deferred-result-ext) 无法清理自身的 setInterval 和 store 订阅,
 *   导致长期运行进程的内存泄漏。
 *
 * @param {object} session - AgentSession 实例
 * @returns {Promise<boolean>} 事件是否被 emit (false = 无 handler)
 */
export async function emitSessionShutdown(session) {
  const runner = session?.extensionRunner;
  if (runner?.hasHandlers?.("session_shutdown")) {
    await runner.emit({ type: "session_shutdown" });
    return true;
  }
  return false;
}

// ── 不稳定 API 适配 ──

/**
 * 图片缩放适配。
 *
 * 0.80.3 起上游签名为 `resizeImage(inputBytes: Uint8Array, mimeType, options?)`
 * （0.70.x 是 `(img: ImageContent, options?)`），且内部吞错返回 null。
 * Hana 消费侧（core/model-image-preprocess.ts）契约保持不变：
 * 传 `{data: base64, mimeType}` 对象，本层负责解码与拆参。
 * 返回结构 `ResizedImage` 两版一致，null 仍表示"压不进 maxBytes / 解码失败"。
 *
 * @param {{type?: string, data: string, mimeType?: string}} image
 * @param {{maxWidth?: number, maxHeight?: number, maxBytes?: number, jpegQuality?: number}} options
 */
export async function resizeModelImageInput(image, options) {
  const inputBytes = Buffer.from(String(image?.data ?? ""), "base64");
  return rawResizeImage(inputBytes, image?.mimeType, options);
}

/**
 * @param {{wasResized?: boolean, originalWidth: number, originalHeight: number, width: number, height: number}} result
 */
export function formatModelImageDimensionNote(result) {
  return rawFormatDimensionNote(result);
}

/**
 * ModelRuntime 工厂（0.83.0 的新装配点）。
 * credentials 可直接传现有 AuthStorage 实例（内部包 RuntimeCredentials，调
 * store.read/list/modify/delete，AuthStorage 全有 → auth.json 同一把锁语义保住）。
 * @param {import('@earendil-works/pi-coding-agent/dist/core/auth-storage.js').AuthStorage} authStorage
 * @param {string} [authPath] auth.json 路径
 * @param {string} [modelsJsonPath] models.json 路径
 * @returns {Promise<import('@earendil-works/pi-coding-agent').ModelRuntime>}
 */
export async function createModelRuntime(authStorage, authPath, modelsJsonPath) {
  const modelRuntime = await ModelRuntime.create({
    credentials: authStorage,
    ...(authPath ? { authPath } : {}),
    ...(modelsJsonPath ? { modelsPath: modelsJsonPath } : {}),
  });
  return withLingxiCredentialBoundary(
    withLegacyAvailabilityScoping(withSerializedModelRefresh(modelRuntime)),
  );
}

/**
 * Pi SDK 的 registerProvider / unregisterProvider 会在内部 fire-and-forget
 * 调用 refresh()。供应商配置连续变化时，较早启动的 refresh 可能在较晚一次
 * 之后才结束，并把旧 models.json 快照重新写回 runtime。
 *
 * 这里把同一个 ModelRuntime 上的 refresh 串行化。显式 await 的下一次刷新会
 * 自然成为前面所有隐式刷新的完成屏障；某次失败不会阻断后续刷新。
 */
function withSerializedModelRefresh(modelRuntime) {
  const originalRefresh = modelRuntime.refresh.bind(modelRuntime);
  let refreshTail = Promise.resolve();
  modelRuntime.refresh = (...args) => {
    const refresh = refreshTail
      .catch(() => undefined)
      .then(() => originalRefresh(...args));
    refreshTail = refresh.then(
      () => undefined,
      () => undefined,
    );
    return refresh;
  };
  return modelRuntime;
}

/**
 * 把 0.83.0 ModelRuntime 的可用性收敛回 0.80.3 ModelRegistry.create 的语义。
 *
 * 背景：0.83.0 的 ModelRuntime.getAvailable()/getAvailableSnapshot() 把"环境凭据
 * 可用"的内置 provider（如设了 ANTHROPIC_AUTH_TOKEN 的 anthropic）也算进 availability
 * （runAvailabilityRefresh 用 checkAuth 判定 configuredProviders，含 env 解析的 key）。
 * 0.80.3 的 ModelRegistry.create 不算这些——可用性只覆盖 models.json 显式配置的
 * provider + 经 registerProvider 注册的 extension/native provider，环境兜底的内置项
 * 不进入可用集合。model-manager.refreshAvailable() 与 model-sync 测试都依赖这个旧语义。
 *
 * 修法：包装 getAvailable / getAvailableSnapshot，只保留 provider 命中"显式配置集合"的
 * 模型——显式配置集合 = models.json 的 config provider ∪ extensionProviders ∪
 * nativeExtensionProviders。仅靠 env 凭据激活的内置 provider 不在该集合内，被滤掉。
 * 不改 SDK 内部状态，只在外层收口；registerProvider 等仍正常生效（注册即进 extension
 * 集合，下次读 availability 即纳入）。
 */
function withLegacyAvailabilityScoping(modelRuntime) {
  const intendedProviderIds = () => new Set([
    ...(modelRuntime.config?.getProviderIds?.() || []),
    ...(modelRuntime.extensionProviders?.keys?.() || []),
    ...(modelRuntime.nativeExtensionProviders?.keys?.() || []),
  ]);
  const scope = (models) => {
    const allowed = intendedProviderIds();
    return Array.isArray(models) ? models.filter((m) => m && allowed.has(m.provider)) : models;
  };
  const originalGetAvailable = modelRuntime.getAvailable.bind(modelRuntime);
  const originalGetAvailableSnapshot = modelRuntime.getAvailableSnapshot?.bind(modelRuntime);
  modelRuntime.getAvailable = async (...args) => scope(await originalGetAvailable(...args));
  if (typeof originalGetAvailableSnapshot === "function") {
    modelRuntime.getAvailableSnapshot = (...args) => scope(originalGetAvailableSnapshot(...args));
  }
  return modelRuntime;
}

/**
 * 禁止 Pi SDK 在 Lingxi 已声明 Provider、但统一凭证边界没有凭证时，
 * 偷偷读取宿主环境变量、云配置文件或运行时身份继续发送请求。
 *
 * 请求级显式 Key 只由已经完成 Fresh Resolve 的 Lingxi 调用传入，需要保留；
 * 空字符串不算有效覆盖，避免 SDK 再次回退到宿主环境。
 */
function withLingxiCredentialBoundary(modelRuntime) {
  const originalGetAuth = modelRuntime.getAuth.bind(modelRuntime);
  const originalGetAvailable = modelRuntime.getAvailable.bind(modelRuntime);
  const originalGetAvailableSnapshot = modelRuntime.getAvailableSnapshot?.bind(modelRuntime);

  const providerIdOf = (providerOrModel) => (
    typeof providerOrModel === "string" ? providerOrModel : providerOrModel?.provider
  );
  const hasExplicitKey = (overrides) => (
    typeof overrides?.apiKey === "string" && overrides.apiKey.length > 0
  );
  const usesAmbientCredential = (providerId) => (
    !!providerId && modelRuntime.getProviderAuthStatus?.(providerId)?.source === "environment"
  );
  const allowedModels = (models) => (
    Array.isArray(models)
      ? models.filter(model => model && !usesAmbientCredential(model.provider))
      : models
  );

  modelRuntime.getAuth = async (providerOrModel, overrides = {}) => {
    const resolution = await originalGetAuth(providerOrModel, overrides);
    const providerId = providerIdOf(providerOrModel);
    if (resolution && !hasExplicitKey(overrides) && usesAmbientCredential(providerId)) {
      const error: any = new Error(`Lingxi credential boundary rejected ambient credentials for provider "${providerId}"`);
      error.code = "LINGXI_AMBIENT_CREDENTIAL_FORBIDDEN";
      error.providerId = providerId;
      throw error;
    }
    return resolution;
  };
  modelRuntime.getAvailable = async (...args) => allowedModels(await originalGetAvailable(...args));
  if (typeof originalGetAvailableSnapshot === "function") {
    modelRuntime.getAvailableSnapshot = (...args) => allowedModels(originalGetAvailableSnapshot(...args));
  }
  return modelRuntime;
}

/**
 * ModelRegistry 工厂。
 * 0.83.0 起 ModelRegistry 变成 ModelRuntime 的同步兼容 facade：静态 create
 * 没了，构造器改吃 ModelRuntime。这里先建 ModelRuntime 再包 ModelRegistry，
 * 返回 { modelRuntime, modelRegistry } 供下游分别取用。
 * @param {import('@earendil-works/pi-coding-agent/dist/core/auth-storage.js').AuthStorage} authStorage
 * @param {string} modelsJsonPath
 * @returns {Promise<{ modelRuntime: import('@earendil-works/pi-coding-agent').ModelRuntime, modelRegistry: import('@earendil-works/pi-coding-agent').ModelRegistry }>}
 */
export async function createModelRegistry(authStorage, modelsJsonPath) {
  const modelRuntime = await createModelRuntime(authStorage, undefined, modelsJsonPath);
  const modelRegistry = new ModelRegistry(modelRuntime);
  return { modelRuntime, modelRegistry };
}

/**
 * Register a provider through the ModelRegistry instance that owns Hana's
 * AuthStorage. This is intentionally kept at the adapter boundary: importing
 * pi-ai's module-level OAuth registry would target a different nested package
 * instance and the login provider would be invisible to AuthStorage.
 */
export function registerModelProvider(
  modelRegistry: ModelRegistry,
  providerId: string,
  config: SdkProviderRegistrationConfig,
): void {
  modelRegistry.registerProvider(providerId, config);
}

/** Remove a provider previously registered through registerModelProvider. */
export function unregisterModelProvider(
  modelRegistry: ModelRegistry,
  providerId: string,
): void {
  modelRegistry.unregisterProvider(providerId);
}

/**
 * 强制 session 重新绑定当前 model 对象。
 *
 * 为什么需要：Pi SDK 的 model 对象把 baseUrl 烤在字段里
 * （openai-completions.js 等 provider 直接读 model.baseUrl 构造 client），
 * session 持有的是创建时的对象引用。当 ModelRegistry.refresh() 重建模型
 * 表后，session 仍指向旧对象，导致改完 base_url / api 等字段后 active
 * session 用旧值发请求，必须重启或切换 session 才生效。
 *
 * SDK 内部有 _refreshCurrentModelFromRegistry()，但只在 extension
 * registerProvider/unregisterProvider 时被调用，没有公开包装。
 * 这里走 adapter 纪律统一桥接，下次 SDK 升级改名只改这里。
 *
 * 当 Hana 已经从自己的 allowlist 解析出 `allowedModel` 时，直接绑定该
 * ModelRegistry 刷新后对象；这避免 Pi 的私有刷新方法在 Hana 已禁用模型时
 * 找到并保留 Pi 内置目录中的同名模型。未传第二参数时保留旧 adapter 行为。
 *
 * @param {object} session - AgentSession 实例
 * @param {object} [allowedModel] - Hana 当前 allowlist 中、与 session 同身份的模型对象
 * @returns {boolean} 是否完成了刷新/重绑
 */
export function refreshSessionModelFromRegistry(session, allowedModel) {
  if (allowedModel !== undefined) {
    const currentModel = session?.model;
    if (!currentModel || !allowedModel
      || currentModel.id !== allowedModel.id
      || currentModel.provider !== allowedModel.provider
      || !session?.agent?.state) {
      return false;
    }
    session.agent.state.model = allowedModel;
    return true;
  }
  session?._refreshCurrentModelFromRegistry?.();
  return true;
}
