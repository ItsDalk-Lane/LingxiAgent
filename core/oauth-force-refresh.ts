/**
 * forceRefreshOAuthApiKey — 无视本地到期时间，立刻旋转 OAuth 凭证
 *
 * 为什么需要它：
 *   凭证存储（auth.json）里每条 OAuth 记录都带一个本地记下来的到期时间，
 *   常规取 token 的路径只在"本地时间已过期"时才去换新 token。但服务端可以
 *   在这个时间之前就把 access token 作废（换设备、撤销会话、服务端缩短有效
 *   期等），此时本地账本还认为 token 有效，常规路径会一直把已经被拒收的旧
 *   token 交出去，调用方只能一路撞 401。
 *
 *   这个原语表达的是另一件事：**服务端刚刚拒收了这个 token，现在就换**。
 *   它不看本地到期时间，只看"你手上那个 token 是不是仍然是存储里那个"。
 *
 * 为什么必须走 ModelRuntime 上的 provider + 同一把文件锁：
 *   - 0.83.0 起 OAuth provider 注册表与刷新能力整体迁出 AuthStorage，落到
 *     pi-ai Models（由 ModelRuntime.compose）。provider 的 refresh/toAuth 在
 *     ModelRuntime.getProvider(authKey).auth.oauth 上。provider 不能再从
 *     AuthStorage 取（getOAuthProviders 已删）。
 *   - 换 token 是"读—改—写"，同一台机器上可能有多个进程同时在换。整个
 *     决策和写盘都放在存储自己的文件锁里（backend.withLockAsync），锁的是
 *     auth.json 这个路径本身，所以和 SDK 自己的刷新路径（pi-ai Models 也走
 *     credentials.modify→store.modify，同一把锁）天然互斥；后进锁的人会看到
 *     前一个人已经写好的新凭证，于是直接复用，不会把刚换来的 refresh token
 *     再换一次作废掉。
 *
 * 失败一律上抛：解析不了、条目不存在、不是 OAuth、provider 未注册、服务端
 * 拒绝换新，都直接报错，不写盘、不降级、不返回旧 token。
 */

interface ForceRefreshOptions {
  /**
   * ModelRuntime 实例（OAuth provider 注册表与刷新能力的唯一入口）。
   * 兼容传 SdkAuthFacade：自动读 .modelRuntime。
   */
  modelRuntime: any;
  /**
   * 与 ModelRuntime 同一份 auth.json 的存储后端，提供 withLockAsync。
   * 0.83.0 AuthStorage 不再暴露 backend；调用方持有 FileAuthStorageBackend 传入。
   */
  backend: any;
  /** auth.json 里的凭证键，例如 "openai-codex" */
  authKey: string;
  /** 调用方手上那个被拒收的 token；用于判断别人是否已经换过 */
  staleApiKey?: string;
}

export async function forceRefreshOAuthApiKey({
  modelRuntime,
  backend,
  authKey,
  staleApiKey,
}: ForceRefreshOptions): Promise<string> {
  const runtime: any = modelRuntime?.modelRuntime ?? modelRuntime;
  if (!runtime || typeof runtime.getProvider !== "function") {
    throw new Error(`Cannot rotate OAuth credential for "${authKey}": model runtime unavailable`);
  }
  if (!backend || typeof backend.withLockAsync !== "function") {
    throw new Error(`Cannot rotate OAuth credential for "${authKey}": auth storage backend unavailable`);
  }

  const apiKey: string = await backend.withLockAsync(async (current) => {
    // 解析失败必须抛：宁可这次刷新失败，也不能拿一个空对象覆盖掉用户的凭证文件。
    const data = current ? JSON.parse(current) : {};
    const cred = data[authKey];
    if (!cred || cred.type !== "oauth" || !cred.access || !cred.refresh) {
      throw new Error(`Cannot rotate OAuth credential for "${authKey}": no OAuth credential stored`);
    }

    const provider = runtime.getProvider(authKey);
    const oauth = provider?.auth?.oauth;
    if (!oauth || typeof oauth.refresh !== "function") {
      throw new Error(`Cannot rotate OAuth credential for "${authKey}": no OAuth provider registered`);
    }

    // 存储里的 token 已经不是调用方手上那个了，说明别的执行流刚换过。
    // 直接用新的，不再发一次刷新请求（那会把刚换来的 refresh token 作废）。
    if (staleApiKey && cred.access !== staleApiKey) {
      const reused = await deriveOAuthApiKey(oauth, cred);
      return { result: reused };
    }

    // 0.83.0：provider.auth.oauth.refresh(cred) 换新 token（网络调用，失败抛）。
    // 返回的 OAuthCredential 形状 { access, refresh, expires, ... } 与存储一致。
    const refreshed = await oauth.refresh(cred);
    const nextCred = { type: "oauth", ...refreshed };
    const rotated = await deriveOAuthApiKey(oauth, nextCred);
    return {
      result: rotated,
      next: JSON.stringify({ ...data, [authKey]: nextCred }, null, 2),
    };
  });

  return apiKey;
}

/**
 * 从 OAuth credential 派生请求用 API key（旧 provider.getApiKey 语义）。
 * 0.83.0 对应 provider.auth.oauth.toAuth(credential).apiKey；toAuth 主要是
 * credential.access，对个别 provider（如 GitHub Copilot）有 baseUrl 归一。
 */
async function deriveOAuthApiKey(oauth: any, credential: any): Promise<string> {
  if (typeof oauth.toAuth === "function") {
    const auth = await oauth.toAuth(credential);
    return auth?.apiKey;
  }
  return credential?.access;
}
