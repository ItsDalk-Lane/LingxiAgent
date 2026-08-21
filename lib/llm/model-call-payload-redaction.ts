/**
 * model-call-payload-redaction.ts — Sensitive Payload Redaction 引擎（Phase 6）。
 *
 * 第一原则（§一百七十）：secret 不应存在于第二份原始副本。本引擎在遍历原始
 * 运行时对象时**直接构造安全副本**（copy-on-capture），原始对象绝不修改
 * （§十五），secret 永不进入返回值（§十四：Sink 只能收到 sanitized detached
 * copy；redaction 先于 sink）。
 *
 * 处理维度（全部有正反例测试锁定）：
 *   - credential 键（headers/body/任意嵌套，归一化整键匹配）→ 值替换
 *   - provider/protocol-specific body credential 路径（如 Volcengine ASR
 *     body.user.uid）→ 结构化规则，不依赖 generic key 名（§二十五/§一百四十九）
 *   - 高置信 inline secret（Bearer/Basic/JWT/sk-/ghp_/AIza/AKIA/PEM/key=value）
 *     —— 保守集合，普通 UUID/文件 id/研究文本不受影响（§三十五/§三十六）
 *   - URL：query credential（含 X-Amz- 与 X-Goog- 签名参数）→ external_reference
 *     descriptor；普通 endpoint 保留（§二十八/§二十九）
 *   - 本地绝对路径（/Users/…、/home/…、C:\Users\…）→ local_file_reference
 *     descriptor / inline 替换，不保留用户目录（§三十）
 *   - 二进制（Buffer/TypedArray/ArrayBuffer/Blob/base64/data URL/FormData 文件）
 *     → external_blob descriptor，不保存字节、不 hash 大媒体（§三十一/§三十二）
 *   - 资源上限（maxDepth/maxNodes/maxArrayItems/maxObjectKeys/maxStringChars/
 *     maxRecordChars）——超限显式 truncate/omit 并记录 action（§四十二/§四十三）
 *   - 循环引用 → 该引用 null + unsupported action，绝不 stack overflow（§一百三十一）
 *
 * Text offset mapping（§四十九/§五十）：redactTextWithMap 返回 replacements，
 * capture 侧用它把 Phase 5 provenance span remap 到脱敏后文本；span 与 redaction
 * 重叠时 span 降级为 null（precision 语义由 capture 侧标注），绝不保留错位位置。
 *
 * 性能（§三十三/§一百六十六）：base64 检测只做 data: 前缀 + 已知媒体键 +
 * 「长度门槛 + 256 字符有界采样」，不对任意大字符串做完整正则/解码；inline
 * secret 扫描仅作用于截断保留前缀。
 */

import path from "node:path";
import {
  MODEL_CALL_PAYLOAD_CAPTURE_LIMITS,
  type ModelCallPayloadSanitization,
  type ModelCallRedactionActionEntry,
} from "./model-call-payload-types.ts";

/* ── 内部状态 ────────────────────────────────────────────────────────── */

const MAX_ACTIONS_RECORDED = 128;

class CaptureBudget {
  nodes = 0;
  chars = 0;
  exhausted = false;
  node(): boolean {
    this.nodes += 1;
    if (this.nodes > MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxNodes) this.exhausted = true;
    if (this.chars > MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxRecordChars) this.exhausted = true;
    return !this.exhausted;
  }
}

class Sanitizer {
  readonly actions: ModelCallRedactionActionEntry[] = [];
  redacted = false;
  truncated = false;
  degraded = false;

  private secretPathSet: ReadonlySet<string>;

  constructor(options: { secretPaths?: ReadonlySet<string> } = {}) {
    this.secretPathSet = options.secretPaths ?? EMPTY_SET;
  }

  action(path: Array<string | number>, action: ModelCallRedactionActionEntry["action"], reason: string): void {
    if (action === "replaced" || action === "removed") this.redacted = true;
    if (action === "truncated") this.truncated = true;
    if (action === "unsupported") this.degraded = true;
    if (this.actions.length >= MAX_ACTIONS_RECORDED) return;
    this.actions.push({ path: [...path], action, reason });
  }

  summary(): ModelCallPayloadSanitization {
    return {
      redacted: this.redacted,
      truncated: this.truncated,
      degraded: this.degraded,
      actions: this.actions,
    };
  }

  isSecretPath(path: Array<string | number>): boolean {
    if (this.secretPathSet.size === 0 || path.length === 0) return false;
    return this.secretPathSet.has(path.join("."));
  }
}

const EMPTY_SET: ReadonlySet<string> = new Set();

export type SanitizeValueResult = {
  value: unknown;
  sanitization: ModelCallPayloadSanitization;
};

export type SanitizeValueOptions = {
  /** provider/protocol-specific body credential 路径（相对 body 根，"." 连接）。 */
  secretPaths?: ReadonlySet<string>;
};

/**
 * 入口：遍历原始对象构造 sanitized detached copy。任何内部异常 fail safe——
 * 返回 null + degraded（绝不把原始引用漏给 sink，也绝不 throw 影响业务）。
 */
export function sanitizeValueForCapture(
  value: unknown,
  options: SanitizeValueOptions = {},
): SanitizeValueResult {
  const sanitizer = new Sanitizer({ secretPaths: options.secretPaths });
  const budget = new CaptureBudget();
  const seen = new WeakMap<object, true>();
  let out: unknown = null;
  try {
    out = walk(value, [], 0, sanitizer, budget, seen);
  } catch {
    sanitizer.degraded = true;
    sanitizer.action([], "unsupported", "sanitizer-internal-error");
    out = null;
  }
  return { value: out, sanitization: sanitizer.summary() };
}

function walk(
  value: unknown,
  trail: Array<string | number>,
  depth: number,
  sanitizer: Sanitizer,
  budget: CaptureBudget,
  seen: WeakMap<object, true>,
): unknown {
  if (!budget.node()) {
    sanitizer.action(trail, "truncated", "record-budget");
    return null;
  }
  if (value === null || value === undefined) return null;
  const type = typeof value;

  if (type === "string") return walkString(value as string, trail, sanitizer, budget);
  if (type === "number") return Number.isFinite(value as number) ? value : null;
  if (type === "boolean") return value;
  if (type === "bigint" || type === "function" || type === "symbol") {
    sanitizer.action(trail, "unsupported", `type-${type}`);
    return null;
  }

  // type === "object"
  // 二进制与特殊容器先于普通对象处理。
  const binary = describeBinary(value);
  if (binary) {
    sanitizer.action(trail, "externalized", binary.reason);
    return binary.descriptor;
  }
  if (value instanceof Date) return value.toISOString();
  if (typeof AbortSignal !== "undefined" && value instanceof AbortSignal) {
    sanitizer.action(trail, "unsupported", "abort-signal");
    return null;
  }
  if (typeof URL !== "undefined" && value instanceof URL) {
    const sanitized = sanitizeCapturedUrl(value.toString());
    if (typeof sanitized !== "string") sanitizer.action(trail, "replaced", "url-query-credential");
    return sanitized;
  }
  if (typeof Headers !== "undefined" && value instanceof Headers) {
    const flat: Record<string, string> = {};
    value.forEach((v, k) => { flat[k] = v; });
    return walk(flat, trail, depth, sanitizer, budget, seen);
  }
  if (value instanceof FormData) return walkFormData(value, trail, depth, sanitizer, budget, seen);
  if (value instanceof Map || value instanceof Set) {
    sanitizer.action(trail, "unsupported", "map-set-container");
    return null;
  }

  if (seen.has(value as object)) {
    sanitizer.action(trail, "unsupported", "cyclic-reference");
    return null;
  }
  if (depth >= MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxDepth) {
    sanitizer.action(trail, "truncated", "depth-limit");
    return null;
  }
  // 标记在「递归栈上」；子树构建完成后解除——共享引用（DAG）各自独立拷贝，
  // 只有真正的环（仍在栈上）才判 cyclic-reference。
  seen.set(value as object, true);

  if (Array.isArray(value)) {
    const limit = MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxArrayItems;
    const items: unknown[] = [];
    const count = Math.min(value.length, limit);
    for (let i = 0; i < count; i++) {
      if (budget.exhausted) break;
      items.push(walk(value[i], [...trail, i], depth + 1, sanitizer, budget, seen));
    }
    if (value.length > limit) {
      sanitizer.action(trail, "truncated", `array-items:${value.length}`);
    }
    seen.delete(value as object);
    return items;
  }

  const keys = Object.keys(value as Record<string, unknown>);
  const limit = MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxObjectKeys;
  const out: Record<string, unknown> = {};
  const count = Math.min(keys.length, limit);
  for (let i = 0; i < count; i++) {
    if (budget.exhausted) break;
    const key = keys[i];
    const childTrail = [...trail, key];
    // provider-specific 结构化 credential 路径（如 Volcengine body.user.uid）。
    if (sanitizer.isSecretPath(childTrail)) {
      sanitizer.action(childTrail, "replaced", "protocol-body-credential");
      out[key] = REDACTED_CREDENTIAL;
      continue;
    }
    if (isCredentialKey(key)) {
      sanitizer.action(childTrail, "replaced", "credential-key");
      out[key] = REDACTED_CREDENTIAL;
      continue;
    }
    if (isBinaryDataKey(key, (value as Record<string, unknown>)[key])) {
      sanitizer.action(childTrail, "externalized", "binary-field");
      out[key] = binaryDescriptorFor((value as Record<string, unknown>)[key], key);
      continue;
    }
    out[key] = walk((value as Record<string, unknown>)[key], childTrail, depth + 1, sanitizer, budget, seen);
  }
  if (keys.length > limit) {
    sanitizer.action(trail, "truncated", `object-keys:${keys.length}`);
  }
  seen.delete(value as object);
  return out;
}

function walkString(text: string, trail: Array<string | number>, sanitizer: Sanitizer, budget: CaptureBudget): unknown {
  // 整串形态判定（field-aware，不做全文正则）。
  if (isDataUrl(text)) {
    sanitizer.action(trail, "externalized", "data-url");
    return dataUrlDescriptor(text);
  }
  if (looksLikeLocalPath(text)) {
    sanitizer.action(trail, "replaced", "local-absolute-path");
    return localPathDescriptor(text);
  }
  if (looksLikeBulkBase64(text)) {
    sanitizer.action(trail, "externalized", "bulk-base64");
    return base64Descriptor(text);
  }
  let retained = text;
  if (text.length > MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxStringChars) {
    retained = text.slice(0, MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxStringChars);
    sanitizer.action(trail, "truncated", `string-length:${text.length}`);
  }
  if (looksLikeSignedUrl(retained) || hasCredentialQuery(retained)) {
    const sanitized = sanitizeCapturedUrl(retained);
    if (typeof sanitized !== "string") sanitizer.action(trail, "replaced", "url-query-credential");
    return sanitized;
  }
  const redacted = redactTextWithMap(retained);
  if (redacted.replacements.length > 0) {
    sanitizer.action(trail, "replaced", "inline-secret");
  }
  budget.chars += redacted.text.length;
  return redacted.text;
}

function walkFormData(
  form: FormData,
  trail: Array<string | number>,
  depth: number,
  sanitizer: Sanitizer,
  budget: CaptureBudget,
  seen: WeakMap<object, true>,
): unknown {
  const fields: Record<string, unknown> = {};
  const files: Array<Record<string, unknown>> = [];
  try {
    for (const [key, raw] of form.entries()) {
      if (typeof raw === "string") {
        fields[key] = walkString(raw, [...trail, "fields", key], sanitizer, budget);
        continue;
      }
      const binary = describeBinary(raw);
      if (binary) {
        sanitizer.action([...trail, "fields", key], "externalized", "form-file");
        files.push({ field: key, ...binary.descriptor as Record<string, unknown> });
      }
    }
  } catch {
    sanitizer.action(trail, "unsupported", "form-iteration-error");
    return null;
  }
  // FormData 内字符串字段也可能携带 credential 键名。
  for (const key of Object.keys(fields)) {
    if (isCredentialKey(key)) {
      sanitizer.action([...trail, "fields", key], "replaced", "credential-key");
      fields[key] = REDACTED_CREDENTIAL;
    }
  }
  if (depth >= MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxDepth) {
    sanitizer.action(trail, "truncated", "depth-limit");
  }
  return { kind: "multipart_form_data", fields, files };
}

/* ── credential 键（归一化整键匹配）──────────────────────────────────── */

const REDACTED_CREDENTIAL = "<redacted:credential>";

const CREDENTIAL_KEYS = new Set([
  "authorization", "proxyauthorization", "proxyauthorizationheader",
  "cookie", "cookie2", "setcookie",
  "apikey", "xapikey", "apikeys", "openaiapikey", "anthropicapikey",
  "googleapikey", "xgoogapikey", "xapisecret", "apisecret",
  "accesstoken", "accesstokenvalue", "refreshtoken", "idtoken",
  "tokensecret", "sessiontoken", "sessionsecret", "sessionkey",
  "clientsecret", "clientsecretkey", "devicesecret", "devicepassword",
  "privatetoken", "privatekey", "privatekeypem", "signingkey", "secretkey",
  "secretaccesskey", "sharedsecret", "secret", "secrets",
  "password", "passwd", "pwd", "credentials", "credential", "signature",
]);

function normalizeCredentialKey(key: string): string {
  return key.toLowerCase().replace(/[^a-z0-9]/g, "");
}

export function isCredentialKey(key: unknown): boolean {
  if (typeof key !== "string" || key.length === 0 || key.length > 128) return false;
  return CREDENTIAL_KEYS.has(normalizeCredentialKey(key));
}

/**
 * Provider/protocol-specific body credential 路径（§二十五）。旧审计实锤的
 * Volcengine Speech/ASR `body.user.uid`（adapters.ts:145）在此登记——generic
 * key 名（uid）不是 secret 特征，必须按协议路径结构化处理。
 */
export const PROVIDER_BODY_CREDENTIAL_PATHS: Readonly<Record<string, readonly string[]>> = {
  "volcengine-bigasr-transcription": ["user.uid"],
};

export function secretPathsForProtocol(protocol: string | null | undefined): ReadonlySet<string> {
  if (typeof protocol !== "string" || !protocol) return EMPTY_SET;
  const paths = PROVIDER_BODY_CREDENTIAL_PATHS[protocol];
  return paths ? new Set<string>(paths) : EMPTY_SET;
}

/* ── 二进制 externalization（§三十一/§三十二：不保存字节、不 hash）──── */

type BinaryOutcome = {
  descriptor: Record<string, unknown>;
  reason: string;
};

function describeBinary(value: unknown): BinaryOutcome | null {
  if (typeof ArrayBuffer !== "undefined" && value instanceof ArrayBuffer) {
    return {
      descriptor: {
        kind: "external_blob",
        mediaType: "application/octet-stream",
        byteLength: value.byteLength,
        encoding: "binary",
        captureStatus: "externalized",
      },
      reason: "arraybuffer",
    };
  }
  if (ArrayBuffer.isView(value)) {
    const view = value as ArrayBufferView & { length?: number };
    return {
      descriptor: {
        kind: "external_blob",
        mediaType: "application/octet-stream",
        byteLength: typeof view.byteLength === "number" ? view.byteLength : (view.length ?? null),
        encoding: "binary",
        captureStatus: "externalized",
      },
      reason: "typed-array",
    };
  }
  if (typeof Blob !== "undefined" && value instanceof Blob) {
    const blob = value as Blob & { name?: unknown };
    return {
      descriptor: {
        kind: "external_blob",
        mediaType: blob.type || "application/octet-stream",
        byteLength: blob.size,
        encoding: "binary",
        captureStatus: "externalized",
        ...(typeof blob.name === "string" && blob.name ? { filename: safeBasename(blob.name) } : {}),
      },
      reason: "blob",
    };
  }
  return null;
}

/** 已知承载媒体/音频 base64 的字段名（field-aware 检测，§三十三；存归一化键）。 */
const BINARY_DATA_KEYS = new Set([
  "b64json", "imagebase64", "base64", "audio", "inputaudio", "inlinedata", "data", "result",
]);

function isBinaryDataKey(key: string, value: unknown): boolean {
  if (!BINARY_DATA_KEYS.has(normalizeCredentialKey(key))) return false;
  return containsBinaryString(value, 0);
}

/** 有界深度扫描（数组/嵌套对象里的 base64 字符串也算，如 image_base64:[b64]）。 */
function containsBinaryString(value: unknown, depth: number): boolean {
  if (typeof value === "string") return isBinaryString(value);
  if (depth >= 3) return false;
  if (Array.isArray(value)) return value.some((item) => containsBinaryString(item, depth + 1));
  if (value && typeof value === "object") {
    return Object.values(value as Record<string, unknown>).some((item) => containsBinaryString(item, depth + 1));
  }
  return false;
}

/** 已知媒体键的宽松判定：≥64 字符 + 64 字符采样 ≥95% base64 字符集（路径/URL 除外）。 */
function isBinaryString(value: string): boolean {
  if (isDataUrl(value)) return true;
  if (looksLikeLocalPath(value) || /^https?:\/\//i.test(value)) return false;
  if (value.length < 64) return false;
  const sample = value.slice(0, 64);
  let matches = 0;
  for (const ch of sample) {
    if (BASE64_CHARS.test(ch)) matches += 1;
  }
  return matches / sample.length >= 0.95;
}

function binaryDescriptorFor(value: unknown, key: string): unknown {
  if (typeof value === "string") {
    return isDataUrl(value) ? dataUrlDescriptor(value) : base64Descriptor(value);
  }
  if (Array.isArray(value)) {
    return value.map((item) => binaryDescriptorFor(item, key));
  }
  if (value && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const [childKey, child] of Object.entries(value as Record<string, unknown>)) {
      out[childKey] = binaryDescriptorFor(child, key);
    }
    return out;
  }
  return { kind: "external_blob", mediaType: "application/octet-stream", byteLength: null, encoding: key, captureStatus: "externalized" };
}

function isDataUrl(text: string): boolean {
  return text.length > 8 && text.slice(0, 5).toLowerCase() === "data:" && text.includes(",");
}

function dataUrlDescriptor(text: string): Record<string, unknown> {
  const comma = text.indexOf(",");
  const meta = text.slice(5, comma).toLowerCase();
  const mediaType = meta.split(";")[0] || "application/octet-stream";
  const isBase64 = meta.includes("base64");
  const payload = text.length - comma - 1;
  return {
    kind: "external_blob",
    mediaType,
    byteLength: isBase64 ? Math.floor(payload * 3 / 4) : payload,
    encoding: isBase64 ? "base64-data-url" : "data-url",
    captureStatus: "externalized",
  };
}

const BASE64_SAMPLE_CHARS = 256;
const BULK_BASE64_MIN_CHARS = 1024;
const BASE64_CHARS = /[A-Za-z0-9+/=\r\n]/;

/** 长度门槛 + 256 字符有界采样（§三十三），不做完整解码。 */
function looksLikeBulkBase64(text: string): boolean {
  if (text.length < BULK_BASE64_MIN_CHARS) return false;
  const sample = text.slice(0, BASE64_SAMPLE_CHARS);
  let matches = 0;
  for (const ch of sample) {
    if (BASE64_CHARS.test(ch)) matches += 1;
  }
  return matches / sample.length >= 0.98;
}

function base64Descriptor(text: string): Record<string, unknown> {
  return {
    kind: "external_blob",
    mediaType: "application/octet-stream",
    byteLength: Math.floor(text.length * 3 / 4),
    encoding: "base64",
    captureStatus: "externalized",
  };
}

/* ── 本地绝对路径（§三十）────────────────────────────────────────────── */

const FULL_LOCAL_PATH = /^([a-zA-Z]:[\\/][^\0]*|\/(?:Users|home|private|var\/folders)\/[^\0]+|\\\\[^\0]+)$/;
const INLINE_LOCAL_PATH = /(?<![\w])((?:[a-zA-Z]:\\Users\\|\/(?:Users|home|private)\/)[A-Za-z0-9._\\/-]{2,240})/g;

export function looksLikeLocalPath(text: string): boolean {
  const trimmed = text.trim();
  return trimmed.length > 0 && trimmed.length <= 1024 && FULL_LOCAL_PATH.test(trimmed);
}

function localPathDescriptor(text: string): Record<string, unknown> {
  return {
    kind: "local_file_reference",
    basename: safeBasename(text.trim()),
  };
}

function safeBasename(raw: string): string | null {
  try {
    const base = path.basename(raw.replace(/[\\/]+$/, ""));
    if (!base || base === "/" || base === "\\") return null;
    return base.length > 128 ? base.slice(0, 128) : base;
  } catch {
    return null;
  }
}

/* ── URL sanitization（§二十八/§二十九）──────────────────────────────── */

const URL_CREDENTIAL_PARAMS = new Set([
  "key", "apikey", "api_key", "token", "accesstoken", "access_token",
  "refreshtoken", "signature", "sig", "credential", "authorization",
  "secret", "code", "clientsecret", "client_secret", "password", "passwd",
  "xgotoken", "private token",
]);

const URL_CREDENTIAL_PREFIXES = ["x-amz-", "x-goog-", "x-oss-", "x-cos-"];

function isCredentialQueryParam(name: string): boolean {
  const normalized = name.toLowerCase();
  if (URL_CREDENTIAL_PARAMS.has(normalized.replace(/[^a-z0-9_]/g, ""))) return true;
  return URL_CREDENTIAL_PREFIXES.some((prefix) => normalized.startsWith(prefix));
}

function hasCredentialQuery(text: string): boolean {
  if (!/^https?:\/\//i.test(text)) return false;
  const queryStart = text.indexOf("?");
  if (queryStart === -1) return false;
  const params = text.slice(queryStart + 1).split("&");
  return params.some((pair) => {
    const key = pair.split("=")[0];
    return key && isCredentialQueryParam(decodeURIComponentSafe(key));
  });
}

function looksLikeSignedUrl(text: string): boolean {
  return hasCredentialQuery(text);
}

function decodeURIComponentSafe(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

/**
 * 统一 URL 清洗：query 带 credential/signature 参数的 URL 转 external_reference
 * descriptor（不保留 query secret；path 保留——对象 key 风险已在契约文档声明，
 * 如需进一步裁剪由 sink 决定丢弃）。普通 URL 返回原样字符串（endpoint 结构
 * 对协议分析有价值）。
 */
export function sanitizeCapturedUrl(url: string): string | Record<string, unknown> {
  if (typeof url !== "string" || url.length === 0) return url;
  if (isDataUrl(url)) return dataUrlDescriptor(url);
  let parsed: URL | null = null;
  try {
    parsed = new URL(url);
  } catch {
    parsed = null;
  }
  if (!parsed) {
    const redacted = redactTextWithMap(url);
    return redacted.text;
  }
  const credentialParams: string[] = [];
  parsed.searchParams.forEach((_value, key) => {
    if (isCredentialQueryParam(key)) credentialParams.push(key);
  });
  if (credentialParams.length > 0) {
    return {
      kind: "external_reference",
      scheme: parsed.protocol.replace(":", ""),
      host: parsed.host,
      path: parsed.pathname,
      redacted: true,
      redactedQueryParams: credentialParams.slice(0, 16),
    };
  }
  return url;
}

/* ── Inline secret detector（§三十五/§三十六：保守、高置信）──────────── */

export const INLINE_SECRET_PLACEHOLDER = "<redacted:secret>";

type InlinePattern = {
  name: string;
  regex: RegExp;
  /** 替换的 group 下标（默认 0=整match；group>0 时只替换该 group 内容）。 */
  group?: number;
};

const INLINE_SECRET_PATTERNS: readonly InlinePattern[] = [
  { name: "pem-private-key", regex: /-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----[\s\S]*?-----END (?:[A-Z0-9]+ )*PRIVATE KEY-----/g },
  { name: "jwt", regex: /\beyJ[A-Za-z0-9_-]{6,}\.[A-Za-z0-9_-]{6,}\.[A-Za-z0-9_-]{6,}\b/g },
  { name: "anthropic-api-key", regex: /\bsk-ant-[A-Za-z0-9_-]{16,}\b/g },
  { name: "openai-api-key", regex: /\bsk-(?:proj-)?[A-Za-z0-9_-]{20,}\b/g },
  { name: "github-token", regex: /\bgh[pousr]_[A-Za-z0-9]{20,}\b/g },
  { name: "google-api-key", regex: /\bAIza[0-9A-Za-z_-]{30,}\b/g },
  { name: "aws-access-key", regex: /\b(?:AKIA|ASIA)[0-9A-Z]{16}\b/g },
  { name: "bearer-token", regex: /\b(Bearer\s+)([A-Za-z0-9\-._~+/]{16,})/g, group: 2 },
  { name: "basic-auth", regex: /\b(Basic\s+)([A-Za-z0-9+/=]{16,})/g, group: 2 },
  {
    name: "kv-secret",
    regex: /\b(api[_-]?key|apikey|access[_-]?token|refresh[_-]?token|client[_-]?secret|secret|token|password|passwd)\b\s*[:=]\s*["']?([A-Za-z0-9._~+/=-]{16,})["']?/gi,
    group: 2,
  },
  { name: "inline-local-path", regex: INLINE_LOCAL_PATH },
];

export type TextRedactionReplacement = {
  /** 原文本 UTF-16 闭开区间（与 Phase 5 span 语义一致）。 */
  start: number;
  end: number;
  newLength: number;
};

export type RedactedText = {
  text: string;
  replacements: TextRedactionReplacement[];
};

/**
 * 文本内联 secret 检测 + 本地路径替换。返回替换后文本与 replacement map
 * （供 provenance span remap）。保守原则：只命中高置信形态；所有 pattern
 * 都有正例+反例测试（model-call-payload-redaction.test.ts）。
 */
export function redactTextWithMap(text: string): RedactedText {
  if (typeof text !== "string" || text.length === 0) return { text, replacements: [] };
  const candidates: Array<{ start: number; end: number }> = [];
  for (const pattern of INLINE_SECRET_PATTERNS) {
    const regex = new RegExp(pattern.regex.source, pattern.regex.flags.includes("g")
      ? pattern.regex.flags
      : pattern.regex.flags + "g");
    let match: RegExpExecArray | null;
    while ((match = regex.exec(text)) !== null) {
      if (match[0].length === 0) { regex.lastIndex += 1; continue; }
      const group = pattern.group ?? 0;
      if (group > 0 && match[group] !== undefined) {
        // group 前的实际文本长度 = 前面各捕获组的原文拼接（不含 match[0]）。
        const prefixLength = match.slice(1, group).join("").length;
        candidates.push({
          start: match.index + prefixLength,
          end: match.index + prefixLength + match[group].length,
        });
      } else {
        candidates.push({ start: match.index, end: match.index + match[0].length });
      }
    }
  }
  if (candidates.length === 0) return { text, replacements: [] };
  // 按 start 排序；重叠时保留 start 更早（相同 start 保留更长）的候选。
  candidates.sort((a, b) => (a.start - b.start) || (b.end - b.start) - (a.end - a.start));
  const merged: Array<{ start: number; end: number }> = [];
  let cursor = -1;
  for (const candidate of candidates) {
    if (candidate.start < cursor) continue;
    merged.push(candidate);
    cursor = candidate.end;
  }
  let out = "";
  const replacements: TextRedactionReplacement[] = [];
  let offset = 0;
  for (const span of merged) {
    out += text.slice(offset, span.start);
    out += INLINE_SECRET_PLACEHOLDER;
    replacements.push({ start: span.start, end: span.end, newLength: INLINE_SECRET_PLACEHOLDER.length });
    offset = span.end;
  }
  out += text.slice(offset);
  return { text: out, replacements };
}

/* ── Span remap（§四十八～§五十）────────────────────────────────────── */

export type RemappedSpan = {
  span: { start: number; end: number } | null;
  /** span 与 redaction 重叠（该段内容已不可定位）→ 调用方须降级 precision。 */
  degraded: boolean;
};

/**
 * 把原文本上的 [start,end) span 平移到 redaction 后文本：无重叠 → 平移 delta；
 * 有重叠 → span=null + degraded=true（绝不保留错位位置，§五十）。
 */
export function remapSpanAfterRedaction(
  span: { start: number; end: number },
  replacements: readonly TextRedactionReplacement[],
): RemappedSpan {
  let delta = 0;
  for (const replacement of replacements) {
    if (replacement.end <= span.start) {
      delta += replacement.newLength - (replacement.end - replacement.start);
      continue;
    }
    if (replacement.start >= span.end) break;
    return { span: null, degraded: true };
  }
  return { span: { start: span.start + delta, end: span.end + delta }, degraded: false };
}
