/**
 * SdkAuthFacade — Hana 的 AuthStorage 兼容门面（pi SDK 0.83.0 桥接层）
 *
 * 背景（0.83.0 已查实事实）：
 *   0.80.3 及更早版本里，AuthStorage 既持有 auth.json 的读写锁，又直接实现
 *   getOAuthProviders / getApiKey / get / has / remove / logout /
 *   setRuntimeApiKey / removeRuntimeApiKey 等 OAuth 与运行时凭证能力。
 *   0.83.0 把后述 OAuth/运行时能力整体迁出 AuthStorage：
 *     - AuthStorage（dist/core/auth-storage.js）只保留 read/modify/delete/
 *       list/reload + create/fromStorage/inMemory；login / getOAuthProviders /
 *       getApiKey 全删。
 *     - getOAuthProviders / login / logout 改由 ModelRuntime + pi-ai Models 提供。
 *     - setRuntimeApiKey / removeRuntimeApiKey 落到 ModelRuntime.credentials。
 *
 *   ModelRuntime.create({ credentials }) 内部把 credentials 包成
 *   RuntimeCredentials，只调 store.read/list/modify/delete——AuthStorage 四个
 *   方法全有，故直接把现有 AuthStorage 实例当 credentials 传进去，auth.json
 *   仍是同一把文件锁（语义保住）。
 *
 * 为什么需要这个门面（最小桥接策略，对齐任务书）：
 *   下游（model-manager / oauth-force-refresh / server routes）大量按旧
 *   AuthStorage 形状调用上述方法，且 model-manager-auth-storage 测试用同样的
 *   形状 mock _authStorage。完整重写下游与测试风险大、diff 大；这里建一个
 *   薄门面把旧形状重新映射到 ModelRuntime 新能力，下游与测试几乎不动。
 *
 * 纪律：
 *   - 不持有业务状态，只转发到 AuthStorage / ModelRuntime。
 *   - 不接受 engine / agent / config 参数。
 *   - 失败一律上抛（与旧 AuthStorage 行为一致）。
 */

import type { AuthStorage } from "../../node_modules/@earendil-works/pi-coding-agent/dist/core/auth-storage.js";

/**
 * 旧 AuthStorage 上的 OAuth provider 对象形状（Hana 下游契约）。
 * 0.83.0 的 composed provider（ModelRuntime.getProvider(id)）暴露的是新形状，
 * 这里按下游用到的字段（id / name / usesCallbackServer）做最小归一。
 */
export interface LegacyOAuthProvider {
  id: string;
  name: string;
  usesCallbackServer?: boolean;
}

export interface SdkAuthFacadeOptions {
  /** 现有 AuthStorage 实例：auth.json 的真理源与文件锁。 */
  authStorage: AuthStorage;
  /**
   * ModelRuntime 实例：OAuth/运行时凭证能力的唯一入口。
   * 必须与 authStorage 指向同一份 auth.json（ModelRuntime.create 用同一
   * AuthStorage 实例当 credentials 即满足）。
   */
  modelRuntime: any;
}

export class SdkAuthFacade {
  /** 底层 AuthStorage（auth.json 锁/真理源）。下游需直接访问时用。 */
  readonly authStorage: AuthStorage;
  /** 底层 ModelRuntime（OAuth/运行时凭证能力）。下游需直接访问时用。 */
  readonly modelRuntime: any;

  constructor(options: SdkAuthFacadeOptions) {
    this.authStorage = options.authStorage;
    this.modelRuntime = options.modelRuntime;
  }

  // ── auth.json 真理源（直接转发底层 AuthStorage）──

  /** 重新从 auth.json 载入内存副本。 */
  reload() {
    return this.authStorage.reload?.();
  }

  /**
   * 同步读 auth.json 里某 provider 的凭证条目（旧 AuthStorage.get 语义）。
   * 0.83.0 的 AuthStorage.read 是 async，这里保留旧的同步契约：直接读底层
   * AuthStorage 的内存副本（.data）。失败/不存在返回 null。
   */
  get(authKey: string): any {
    // AuthStorage.data 是 private 字段；用 unknown 中转的结构化断言（非 as any）访问其内存副本。
    const data = (this.authStorage as unknown as { data?: Record<string, unknown> }).data;
    if (!data || typeof data !== "object") return null;
    const cred = data[authKey];
    return cred ?? null;
  }

  /** auth.json 里是否存有该 provider 的凭证。 */
  has(authKey: string): boolean {
    return this.get(authKey) != null;
  }

  /** 从 auth.json 删除某 provider 的凭证（写盘）。 */
  async remove(authKey: string): Promise<void> {
    await this.authStorage.delete?.(authKey);
  }

  // ── OAuth / 运行时凭证能力（转发到 ModelRuntime）──

  /**
   * 列出当前注册的 OAuth provider（旧 AuthStorage.getOAuthProviders 语义）。
   * 0.83.0 的 OAuth provider 注册表在 pi-ai Models（由 ModelRuntime.compose）；
   * 这里从 ModelRuntime.getProviders() 取出 auth.oauth 非空的，归一到下游契约。
   */
  getOAuthProviders(): LegacyOAuthProvider[] {
    const providers = this.modelRuntime?.getProviders?.() || [];
    return providers
      .filter((provider: any) => provider?.auth?.oauth)
      .map((provider: any) => normalizeLegacyOAuthProvider(provider));
  }

  /**
   * 取某 provider 当前可用的 API key（旧 AuthStorage.getApiKey 语义）。
   * OAuth provider 会在 token 即将/已过期时按 ModelRuntime 的双重检查锁自动换新。
   * options.includeFallback 仅旧契约的形态参数，新链路无等价开关，忽略即可
   * （下游用 includeFallback:false 的语义=“别拿环境变量兜底”，新链路对 stored
   * OAuth 凭证本就不会走 env 兜底，行为一致）。
   * 返回 undefined 表示该 provider 未配置凭证（与旧 getApiKey 一致）。
   */
  async getApiKey(
    authKey: string,
    _options?: { includeFallback?: boolean },
  ): Promise<string | undefined> {
    const resolution = await this.modelRuntime?.getAuth?.(authKey);
    if (!resolution?.auth) return undefined;
    const apiKey = resolution.auth.apiKey;
    return typeof apiKey === "string" && apiKey.length > 0 ? apiKey : undefined;
  }

  /**
   * 登出某 provider：删除其 auth.json 凭证并刷新 runtime 快照
   * （旧 AuthStorage.logout 语义）。
   */
  async logout(authKey: string): Promise<void> {
    await this.modelRuntime?.logout?.(authKey);
  }

  /**
   * 设置运行时（内存级，不落 auth.json）API key 覆盖
   * （旧 AuthStorage.setRuntimeApiKey 语义）。
   */
  async setRuntimeApiKey(providerId: string, apiKey: string): Promise<void> {
    await this.modelRuntime?.setRuntimeApiKey?.(providerId, apiKey);
  }

  /** 清除某 provider 的运行时 API key 覆盖（旧 AuthStorage.removeRuntimeApiKey 语义）。 */
  async removeRuntimeApiKey(providerId: string): Promise<void> {
    await this.modelRuntime?.removeRuntimeApiKey?.(providerId);
  }
}

/**
 * 把 0.83.0 composed provider 归一到下游使用的旧 OAuth provider 契约。
 * usesCallbackServer：旧 AuthStorage 上的 provider 用它区分回调服务器流程
 * （如 OpenAI Codex）。0.83.0 的 provider 形状不再直接暴露该布尔；OpenAI Codex
 * 的回调服务器流程在新 oauth flow 里通过 onManualCodeInput 交互体现。这里对已知
 * 回调服务器 provider（openai-codex）显式标注，其余默认 false，保持下游行为不变。
 */
function normalizeLegacyOAuthProvider(provider: any): LegacyOAuthProvider {
  const id = String(provider?.id ?? "");
  const oauth = provider?.auth?.oauth || {};
  const name = String(oauth.name || provider?.name || id);
  const usesCallbackServer = id === "openai-codex";
  return { id, name, ...(usesCallbackServer ? { usesCallbackServer: true } : {}) };
}
