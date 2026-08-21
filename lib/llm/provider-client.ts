/**
 * lib/llm/provider-client.ts — Provider 认证 header 和连通性探测 URL 构造
 *
 * callProviderText 已迁移到 core/llm-client.ts（走 Pi SDK），
 * 本文件只保留 test/health 路由需要的辅助函数。
 */

import { t } from "../i18n.ts";
import { normalizeProviderHeaders } from "../../shared/provider-auth.ts";
import { withModelRequestAccounting } from "./model-request-accounting.ts";
import { beginObservedModelCall, failObservedModelCall, observedProviderFetch } from "./model-call-integration.ts";

export const DEFAULT_PROVIDER_USER_AGENT = "LingxiAgent/1.0";
const DEFAULT_ANTHROPIC_PROBE_MODEL = "claude-sonnet-4-6";

function hasHeader(headers, name) {
  const target = name.toLowerCase();
  return Object.keys(headers || {}).some((key) => key.toLowerCase() === target);
}

export function withDefaultProviderHeaders(headers = {}) {
  if (hasHeader(headers, "User-Agent")) return headers;
  return {
    ...headers,
    "User-Agent": DEFAULT_PROVIDER_USER_AGENT,
  };
}

export function appendProviderApiPath(baseUrl, apiPath) {
  const rawBase = String(baseUrl || "").trim();
  const normalizedApiPath = `/${String(apiPath || "").replace(/^\/+/, "")}`;
  try {
    const parsedBase = new URL(rawBase);
    const parsedApiPath = new URL(normalizedApiPath, "https://provider.invalid");
    const basePath = parsedBase.pathname.replace(/\/+$/, "");
    const targetPath = parsedApiPath.pathname.replace(/\/+$/, "");
    if (basePath.toLowerCase().endsWith(targetPath.toLowerCase())) {
      return rawBase.replace(/\/+$/, "");
    }
    parsedBase.pathname = targetPath.startsWith("/v1/") && /\/v1$/i.test(basePath)
      ? `${basePath}${targetPath.slice(3)}`
      : `${basePath}${targetPath}`;
    if (parsedApiPath.search) parsedBase.search = parsedApiPath.search;
    return parsedBase.toString().replace(/\/+$/, "");
  } catch {
    const base = rawBase.replace(/\/+$/, "");
    if (base.toLowerCase().endsWith(normalizedApiPath.toLowerCase())) return base;
    if (normalizedApiPath.startsWith("/v1/") && /\/v1$/i.test(base)) {
      return `${base}${normalizedApiPath.slice(3)}`;
    }
    return `${base}${normalizedApiPath}`;
  }
}

function stripTerminalProviderPath(baseUrl, suffixes) {
  const raw = String(baseUrl || "").trim();
  try {
    const parsed = new URL(raw);
    let pathname = parsed.pathname.replace(/\/+$/, "");
    for (const suffix of suffixes) {
      if (pathname.toLowerCase().endsWith(suffix.toLowerCase())) {
        pathname = pathname.slice(0, -suffix.length).replace(/\/+$/, "");
        break;
      }
    }
    parsed.pathname = pathname || "/";
    parsed.search = "";
    parsed.hash = "";
    return parsed.toString().replace(/\/+$/, "");
  } catch {
    let normalized = raw.replace(/\/+$/, "");
    for (const suffix of suffixes) {
      if (normalized.toLowerCase().endsWith(suffix.toLowerCase())) {
        normalized = normalized.slice(0, -suffix.length).replace(/\/+$/, "");
        break;
      }
    }
    return normalized;
  }
}

function normalizeExactEndpointForSdk(baseUrl, api) {
  if (api === "anthropic-messages") {
    return stripTerminalProviderPath(baseUrl, ["/v1/messages", "/v1"]);
  }
  if (api === "openai-completions") {
    return stripTerminalProviderPath(baseUrl, ["/chat/completions"]);
  }
  if (api === "openai-responses" || api === "openai-codex-responses") {
    return stripTerminalProviderPath(baseUrl, ["/responses"]);
  }
  return String(baseUrl || "").trim().replace(/\/+$/, "");
}

/**
 * 构建 provider 认证 header
 * 被 /api/providers/test 和 /api/models/health 路由使用
 */
export function buildProviderAuthHeaders(api, apiKey, opts: { allowMissingApiKey?: boolean } = {}) {
  const allowMissingApiKey = opts.allowMissingApiKey === true;
  if (!api) {
    throw new Error(t("error.missingApiProtocol"));
  }
  if (!apiKey && !allowMissingApiKey) {
    throw new Error(t("error.missingApiKey"));
  }

  if (api === "anthropic-messages") {
    const headers = {
      "Content-Type": "application/json",
      "anthropic-version": "2023-06-01",
    };
    if (apiKey) headers["x-api-key"] = apiKey;
    return withDefaultProviderHeaders(headers);
  }

  if (api === "openai-completions" || api === "openai-codex-responses" || api === "openai-responses") {
    const headers = {
      "Content-Type": "application/json",
    };
    if (apiKey) headers["Authorization"] = `Bearer ${apiKey}`;
    return withDefaultProviderHeaders(headers);
  }

  if (api === "google-generative-ai") {
    const headers = {
      "Content-Type": "application/json",
    };
    if (apiKey) headers["x-goog-api-key"] = apiKey;
    return withDefaultProviderHeaders(headers);
  }

  throw new Error(t("error.unsupportedApiProtocol", { api }));
}

export function buildProviderRequestHeaders({ api, apiKey, headers, allowMissingApiKey = false }: { api?: string; apiKey?: string; headers?: Record<string, string>; allowMissingApiKey?: boolean } = {}) {
  const customHeaders = normalizeProviderHeaders(headers);
  let requestHeaders;
  if (api) {
    requestHeaders = buildProviderAuthHeaders(api, apiKey, {
      allowMissingApiKey: allowMissingApiKey || Object.keys(customHeaders).length > 0,
    });
  } else {
    if (apiKey && !allowMissingApiKey) {
      throw new Error(t("error.missingApiProtocol"));
    }
    requestHeaders = withDefaultProviderHeaders({ "Content-Type": "application/json" });
  }
  return withDefaultProviderHeaders({ ...requestHeaders, ...customHeaders });
}

export function normalizeProviderBaseUrlForApi({ provider, baseUrl, api }: { provider?: string; baseUrl?: string; api?: string } = {}) {
  const raw = String(baseUrl || "").trim();
  if (!raw) return raw;
  if (provider === "opencode") {
    const root = stripTerminalProviderPath(raw, ["/v1/messages", "/v1"]);
    return api === "anthropic-messages"
      ? root
      : appendProviderApiPath(root, "/v1");
  }
  const sdkBaseUrl = normalizeExactEndpointForSdk(raw, api);
  if (provider === "kimi-coding" && api === "openai-completions") {
    try {
      const parsed = new URL(sdkBaseUrl);
      if (parsed.hostname !== "api.kimi.com") return sdkBaseUrl;
      const pathname = parsed.pathname.replace(/\/+$/, "");
      if (pathname === "/coding/v1") return sdkBaseUrl;
      if (pathname === "" || pathname === "/coding") {
        parsed.pathname = "/coding/v1";
        parsed.search = "";
        parsed.hash = "";
        return parsed.toString().replace(/\/+$/, "");
      }
    } catch {
      const base = sdkBaseUrl;
      if (base === "https://api.kimi.com/coding") return "https://api.kimi.com/coding/v1";
    }
    return sdkBaseUrl;
  }
  if (provider === "ollama" && api === "openai-completions") {
    try {
      const parsed = new URL(sdkBaseUrl);
      const pathname = parsed.pathname.replace(/\/+$/, "");
      if (/\/v1$/i.test(pathname)) {
        parsed.pathname = pathname;
        parsed.search = "";
        parsed.hash = "";
        return parsed.toString().replace(/\/+$/, "");
      }
      parsed.pathname = `${pathname || ""}/v1`;
      parsed.search = "";
      parsed.hash = "";
      return parsed.toString().replace(/\/+$/, "");
    } catch {
      const base = sdkBaseUrl;
      return /\/v1$/i.test(base) ? base : `${base}/v1`;
    }
  }
  if (provider !== "minimax" && provider !== "minimax-token-plan") return sdkBaseUrl;
  if (api !== "anthropic-messages") return sdkBaseUrl;

  let parsed;
  try {
    parsed = new URL(sdkBaseUrl);
  } catch {
    return sdkBaseUrl;
  }
  if (parsed.hostname !== "api.minimaxi.com" && parsed.hostname !== "api.minimax.io") {
    return sdkBaseUrl;
  }

  const parts = parsed.pathname.split("/").filter(Boolean);
  if (parts[0] === "anthropic") return sdkBaseUrl;
  if (parts.length === 0 || (parts.length === 1 && parts[0] === "v1")) {
    parsed.pathname = "/anthropic";
    parsed.search = "";
    parsed.hash = "";
    return parsed.toString().replace(/\/+$/, "");
  }
  return sdkBaseUrl;
}

/**
 * 构建连通性探测 URL（统一 test/health 两条路由的 URL 逻辑）
 *
 * Anthropic 协议：POST baseUrl/v1/messages（和 Pi SDK Anthropic provider 一致）
 * OpenAI 兼容协议：GET baseUrl/models
 * Google native 协议：GET baseUrl/models
 *
 * @param {string} baseUrl
 * @param {string} api
 * @returns {{ url: string, method: string }}
 */
export function buildProbeUrl(baseUrl, api) {
  if (api === "anthropic-messages") {
    return { url: appendProviderApiPath(baseUrl, "/v1/messages"), method: "POST" };
  }
  const base = String(baseUrl || "").replace(/\/+$/, "");
  return { url: `${base}/models`, method: "GET" };
}

/**
 * 探测 provider 连通性（统一 health check + test 的唯一实现）
 *
 * 判断标准：只有 2xx 视为成功。错误响应会提取简短的结构化消息，
 * 不把代理层 404 HTML 当作连通，也不把整页 HTML 暴露给设置页。
 * Codex Responses API 因 Cloudflare 反爬无法探测，直接跳过返回 ok。
 *
 * @param {{ providerId: string, baseUrl: string, api: string, credentialBoundary: {consume: Function}, modelId?: string }} params
 * @returns {Promise<{ ok: boolean, status: number, skipped?: string, error?: string }>}
 */
export async function probeProvider({
  providerId,
  baseUrl,
  api,
  credentialBoundary,
  modelId,
  usageLedger = null,
  usageContext = null,
}: {
  providerId: string;
  baseUrl: string;
  api: string;
  credentialBoundary: { consume: (request: {providerId: string; operation: "connectivity-probe"}) => {apiKey: string; headers: Record<string, string>} };
  modelId?: string;
  usageLedger?: any;
  usageContext?: any;
}) {
  if (api === "openai-codex-responses") {
    return { ok: true, status: 0, skipped: t("error.codexNoHealthCheck") };
  }

  if (!credentialBoundary || typeof credentialBoundary.consume !== "function") {
    throw new Error("Provider probe requires a temporary credential boundary");
  }
  const { apiKey, headers: customHeaders } = credentialBoundary.consume({
    providerId,
    operation: "connectivity-probe",
  });

  const probe = buildProbeUrl(baseUrl, api);

  const headers = buildProviderRequestHeaders({
    api,
    apiKey,
    headers: customHeaders,
    allowMissingApiKey: true,
  });

  const effectiveModelId = modelId || (api === "anthropic-messages" ? DEFAULT_ANTHROPIC_PROBE_MODEL : null);

  // MC-05 语义拆分（§十八/§二十一）：Anthropic 分支是真实最小生成调用
  // （POST /v1/messages，固定 prompt、max_tokens=1）→ 进入 ModelCallObserver，
  // 请求前铸 callId 并写进 ledger metadata。其它协议只是 GET /models 模型
  // 目录发现 → CONTROL_PLANE：0 observer 事件、不进 Usage Ledger。
  if (api !== "anthropic-messages") {
    const res = await fetch(probe.url, { headers, signal: AbortSignal.timeout(10000) });
    return buildProbeResult(res);
  }

  const recorder = beginObservedModelCall({
    model: { provider: providerId, modelId: effectiveModelId, api },
    usageContext,
    details: {
      path: "provider_probe",
      probeKind: "generation",
      protocol: api,
      operation: "connectivity-probe",
    },
  });
  try {
    const result = await withModelRequestAccounting({
      usageLedger,
      model: { provider: providerId, modelId: effectiveModelId, api },
      usageContext,
      metadata: { operation: "connectivity-probe", modelCallId: recorder.callId },
    }, async () => {
      const res = await observedProviderFetch({ modelCall: recorder }, () => fetch(probe.url, {
        method: probe.method,
        headers,
        body: JSON.stringify({
          model: effectiveModelId,
          max_tokens: 1,
          messages: [{ role: "user", content: "." }],
        }),
        signal: AbortSignal.timeout(10000),
      }), { requestDetails: { protocol: api, messageCount: 1 } });
      return buildProbeResult(res);
    });
    if (result?.ok) {
      recorder.semanticResponseCompleted({
        details: { probeAccepted: true, httpStatus: result?.status ?? null },
      });
      recorder.endLogicalCall("ok");
    } else {
      recorder.logicalCallError(new Error(`connectivity probe rejected`), {
        details: { probeAccepted: false, httpStatus: result?.status ?? null, errorKind: "http_error" },
      });
      recorder.endLogicalCall("error", { details: { errorKind: "http_error" } });
    }
    return result;
  } catch (err) {
    const errorKind = err?.name === "TimeoutError" || err?.name === "AbortError" || err?.type === "aborted"
      ? "timeout"
      : "provider_or_network";
    failObservedModelCall(recorder, err, { errorKind });
    throw err;
  }
}

async function buildProbeResult(res) {
  if (res.ok) return { ok: true, status: res.status };
  const fallback = `HTTP ${res.status}${res.statusText ? ` ${res.statusText}` : ""}`;
  const contentType = res.headers.get("content-type") || "";
  try {
    if (contentType.includes("application/json")) {
      const payload = await res.json();
      const message = payload?.error?.message || payload?.error || payload?.message;
      if (typeof message === "string" && message.trim()) {
        return { ok: false, status: res.status, error: message.trim().slice(0, 500) };
      }
    } else if (contentType.startsWith("text/") && !contentType.includes("html")) {
      const message = (await res.text()).trim();
      if (message) return { ok: false, status: res.status, error: message.slice(0, 500) };
    }
  } catch {
    // Malformed error bodies fall back to the HTTP status below.
  }
  return { ok: false, status: res.status, error: fallback };
}
