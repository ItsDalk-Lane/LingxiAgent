/** 两种现役服务的授权协议在此归一；不把登录状态当成普通模型目录保存。 */
import { lingxiFetchJson } from '../../api';
import { DEFAULT_OAUTH_LOGIN_METHOD } from '../../../../../../shared/oauth-login.ts';

export interface OAuthAttempt {
  rust: boolean;
  provider: string;
  transaction: string;
  url: string;
  deviceCode: string | null;
  polling: boolean;
  intervalMs: number;
  expiresAt: number;
}
export interface OAuthProgress { done: boolean; persisted: boolean; intervalMs?: number }
function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('授权响应格式无效');
  return value as Record<string, unknown>;
}
function text(value: unknown): string {
  if (typeof value !== 'string' || !value.trim()) throw new Error('授权响应缺少必要字段');
  return value;
}
function browserUrl(value: unknown): URL {
  const url = new URL(text(value));
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) throw new Error('授权地址无效');
  return url;
}
function duration(value: unknown): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 1_000 || value > 300_000) throw new Error('授权轮询间隔无效');
  return value;
}
function deadline(value: unknown): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value <= Date.now()) throw new Error('授权已过期');
  return value;
}
const route = (provider: string, suffix: string) => `/lingxi/v1/models/credentials/${encodeURIComponent(provider)}/${suffix}`;
const post = (signal: AbortSignal, body?: unknown) => ({ method: 'POST', signal, headers: { 'Content-Type': 'application/json' }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
export async function startOAuth(provider: string, rust: boolean, signal: AbortSignal): Promise<OAuthAttempt> {
  const data = object(await lingxiFetchJson(rust ? route(provider, 'login') : '/api/auth/oauth/start', post(signal, rust ? undefined : { provider, loginMethod: DEFAULT_OAUTH_LOGIN_METHOD })));
  if (!rust) return { rust, provider, transaction: text(data.sessionId), url: browserUrl(data.url).href, deviceCode: typeof data.instructions === 'string' ? data.instructions : null, polling: !!data.instructions || data.polling === true, intervalMs: 3000, expiresAt: Date.now() + 300_000 };
  if (data.ok !== true) throw new Error('授权未启动');
  const start = object(data.start);
  const expiresAt = deadline(start.expiresAtUnixMs);
  if (start.flow === 'authorizationCodePkce') {
    const url = browserUrl(start.authorizeUrl);
    return { rust, provider, transaction: text(url.searchParams.get('state')), url: url.href, deviceCode: null, polling: false, intervalMs: 3000, expiresAt };
  }
  if (start.flow === 'deviceCode') return { rust, provider, transaction: '', url: browserUrl(start.verificationUri).href, deviceCode: text(start.userCode), polling: true, intervalMs: duration(start.intervalMs), expiresAt };
  throw new Error('授权流程不受支持');
}
function progress(data: Record<string, unknown>, rust: boolean): OAuthProgress {
  if (!rust) {
    if (data.status === 'error') throw new Error(typeof data.error === 'string' ? data.error : '授权失败');
    if (data.status === 'done') return { done: true, persisted: true };
    // 旧 Node 对未知/暂缺状态继续等到原五分钟预算，不改变旧协议含义。
    return { done: false, persisted: false, intervalMs: 3000 };
  }
  if (data.ok !== true) throw new Error('授权未完成');
  const outcome = object(data.outcome);
  if (outcome.status === 'loggedIn' && typeof outcome.persisted === 'boolean') return { done: true, persisted: outcome.persisted };
  if (outcome.status === 'pending' && typeof outcome.slowDown === 'boolean') return { done: false, persisted: false, intervalMs: duration(outcome.intervalMs) };
  throw new Error('授权状态响应无效');
}
export async function pollOAuth(attempt: OAuthAttempt, signal: AbortSignal): Promise<OAuthProgress> {
  const data = object(await lingxiFetchJson(attempt.rust ? route(attempt.provider, 'login/poll') : `/api/auth/oauth/poll/${encodeURIComponent(attempt.transaction)}`, attempt.rust ? post(signal) : { signal }));
  return progress(data, attempt.rust);
}
export async function completeOAuth(attempt: OAuthAttempt, code: string, signal: AbortSignal): Promise<OAuthProgress> {
  const data = object(await lingxiFetchJson(attempt.rust ? route(attempt.provider, 'login/callback') : '/api/auth/oauth/callback', post(signal, attempt.rust ? { state: attempt.transaction, code } : { sessionId: attempt.transaction, code })));
  // 旧服务手输码以非错误成功响应确认；Rust 必须携带真实安装结果。
  return attempt.rust ? progress(data, true) : { done: true, persisted: true };
}
export async function logoutOAuth(provider: string, rust: boolean, signal: AbortSignal): Promise<void> {
  const data = object(await lingxiFetchJson(rust ? route(provider, 'logout') : '/api/auth/oauth/logout', post(signal, rust ? undefined : { provider })));
  if (rust && data.ok !== true) throw new Error('登出未完成');
}
