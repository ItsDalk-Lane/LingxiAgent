/**
 * model-call-payload-capture.ts — Sensitive Payload Capture 通道（Phase 6）。
 *
 * 与 ModelCallObserver 平行的第二契约（§九）：
 *
 *     Model Call Runtime ─┬─ ModelCallObserver（SAFE METADATA，事件无正文）
 *                         └─ ModelCallPayloadCapture（SENSITIVE DATA）
 *                                ↓ 统一 Redaction（redaction 先于 sink，§十四）
 *                              Sanitized Payload Sink（detached copy only）
 *
 * 关键不变量：
 *   - 生产默认 sink = NOOP_MODEL_CALL_PAYLOAD_SINK（§十一）：不安装 sink 时
 *     createModelCallPayloadCaptureSession 返回 null，集成点以 null 短路——
 *     不深遍历、不脱敏、不复制，接近 O(1)（§四十五）。
 *   - Sink 只能收到 sanitized detached copy；本模块内部对 sink 投递的任何异常
 *     就地吞掉——Observability failure must never become model-call failure（§十三）。
 *   - providerRequestOrdinal：session 内单调计数（§十九），Codex image 401
 *     refresh 两次 fetch = 同 call 两条 provider_request、两个 ordinal。
 *   - 不持有 Prompt history（§一百二十）：capture 后即释放原始引用，
 *     session 只存身份 + 计数器 + sink 引用。
 *   - 本轮只有 Noop/Test sink；未来 Payload Store 只能作为新 sink 接入（§十二）。
 */

import {
  MODEL_CALL_PAYLOAD_CAPTURE_LIMITS,
  MODEL_CALL_PAYLOAD_SCHEMA_VERSION,
  NO_SANITIZATION,
  type ModelCallPayloadFidelity,
  type ModelCallPayloadKind,
  type ModelCallPayloadRecord,
  type ModelCallPayloadSanitization,
  type ModelCallPayloadVisibility,
  type ModelCallProviderTransport,
  type ModelSemanticResponse,
  type ProviderRequestProvenance,
  type SemanticRequestCaptureInput,
} from "./model-call-payload-types.ts";
import type {
  ModelCallAttribution,
  ModelCallModelIdentity,
  ModelCallSource,
} from "./model-call-observer.ts";
import type {
  ModelSemanticInputProvenance,
  SemanticInputProvenanceSection,
} from "./semantic-input-provenance.ts";
import {
  redactTextWithMap,
  remapSpanAfterRedaction,
  sanitizeCapturedUrl,
  sanitizeValueForCapture,
  secretPathsForProtocol,
  type SanitizeValueResult,
  type TextRedactionReplacement,
} from "./model-call-payload-redaction.ts";

/* ── Sink 注册表（进程级注入点，默认 noop）───────────────────────────── */

export interface ModelCallPayloadSink {
  handleModelCallPayloadRecord(record: ModelCallPayloadRecord): void;
}

export const NOOP_MODEL_CALL_PAYLOAD_SINK: ModelCallPayloadSink = Object.freeze({
  handleModelCallPayloadRecord() { /* no-op */ },
});

let currentSink: ModelCallPayloadSink = NOOP_MODEL_CALL_PAYLOAD_SINK;

/** 测试/调试注入；生产保持 noop（不因 Phase 6 让所有 prompt 常驻内存）。 */
export function setModelCallPayloadSink(sink: ModelCallPayloadSink | null | undefined): void {
  currentSink = typeof sink === "object" && sink !== null
    && typeof sink.handleModelCallPayloadRecord === "function"
    ? sink
    : NOOP_MODEL_CALL_PAYLOAD_SINK;
}

export function getModelCallPayloadSink(): ModelCallPayloadSink {
  return currentSink;
}

/* ── 内部投递类型 ────────────────────────────────────────────────────── */

type DeliverExtras = {
  attemptId?: string | null;
  providerRequestOrdinal?: number | null;
  visibility: ModelCallPayloadVisibility;
  fidelity: ModelCallPayloadFidelity;
  sanitization?: ModelCallPayloadSanitization;
  payload?: unknown;
  semanticInputProvenance?: ModelSemanticInputProvenance | null;
  providerRequestProvenance?: ProviderRequestProvenance | null;
};
type Deliver = (kind: ModelCallPayloadKind, extras: DeliverExtras) => void;

const MAX_ACTIONS_PER_RECORD = 128;

/* ── Capture Session ────────────────────────────────────────────────── */

export type PayloadCaptureIdentity = {
  callId: string;
  traceId?: string | null;
  parentCallId?: string | null;
  model?: ModelCallModelIdentity | null;
  source?: ModelCallSource | null;
  attribution?: ModelCallAttribution | null;
};

export type ProviderRequestCaptureInput = {
  attemptId?: string | null;
  protocol?: string | null;
  transport?: ModelCallProviderTransport | null;
  fidelity?: ModelCallPayloadFidelity;
  provenance?: ProviderRequestProvenance | null;
};

export type ProviderResponseCaptureInput = {
  attemptId?: string | null;
  /** 关联的 provider_request ordinal；缺省关联最近一次。 */
  ordinal?: number | null;
  status?: number | null;
  headers?: unknown;
  body?: unknown;
  fidelity: ModelCallPayloadFidelity;
  visibility?: ModelCallPayloadVisibility;
};

export type SemanticResponseCaptureInput = {
  response: Omit<ModelSemanticResponse, "completeness"> & { completeness?: "complete" | "partial" };
};

export type ModelCallPayloadCaptureSession = {
  readonly callId: string;
  readonly traceId: string | null;
  readonly parentCallId: string | null;
  /** 当前 attempt（由集成点在 beginAttempt 后同步）。 */
  setAttempt(attemptId: string | null): void;
  captureSemanticRequest(input: SemanticRequestCaptureInput): void;
  captureProviderRequest(input: ProviderRequestCaptureInput): void;
  captureProviderResponse(input: ProviderResponseCaptureInput): void;
  captureSemanticResponse(input: SemanticResponseCaptureInput): void;
  /**
   * 边界结构已知但内容不可见时（§九十五/§一百零三）：显式 opaque/unavailable
   * record，绝不从 semantic request 重建 provider payload（§八十四）。
   */
  noteProviderWireUnavailable(
    kind: "provider_request" | "provider_response",
    options: { reason: string; visibility: "opaque" | "unavailable"; fidelity: ModelCallPayloadFidelity },
  ): void;
};

/**
 * 创建 capture session。sink 未安装（noop）时返回 null——集成点据此短路，
 * 快路径零成本（§四十五）。身份 callId 非法也返回 null（fail closed）。
 */
export function createModelCallPayloadCaptureSession(
  identity: PayloadCaptureIdentity | null | undefined,
): ModelCallPayloadCaptureSession | null {
  const sink = getModelCallPayloadSink();
  if (!identity || sink === NOOP_MODEL_CALL_PAYLOAD_SINK) return null;
  if (typeof identity.callId !== "string" || !identity.callId.trim()) return null;
  const frozen = {
    callId: identity.callId.trim(),
    traceId: identity.traceId ?? null,
    parentCallId: identity.parentCallId ?? null,
    model: identity.model ?? null,
    source: identity.source ?? null,
    attribution: identity.attribution ?? null,
  };
  let currentAttemptId: string | null = null;
  let nextOrdinal = 0;
  let lastOrdinal: number | null = null;

  const deliver: Deliver = (kind, extras) => {
    try {
      const record: ModelCallPayloadRecord = {
        schemaVersion: MODEL_CALL_PAYLOAD_SCHEMA_VERSION,
        kind,
        capturedAt: new Date().toISOString(),
        callId: frozen.callId,
        traceId: frozen.traceId,
        parentCallId: frozen.parentCallId,
        attemptId: extras.attemptId ?? currentAttemptId,
        providerRequestOrdinal: extras.providerRequestOrdinal ?? null,
        model: frozen.model,
        source: frozen.source,
        attribution: frozen.attribution,
        visibility: extras.visibility,
        fidelity: extras.fidelity,
        sanitization: extras.sanitization ?? NO_SANITIZATION,
        payload: extras.payload ?? null,
      };
      if (kind === "semantic_request") record.semanticInputProvenance = extras.semanticInputProvenance ?? null;
      if (kind === "provider_request") record.providerRequestProvenance = extras.providerRequestProvenance ?? null;
      sink.handleModelCallPayloadRecord(record);
    } catch {
      // Sink 故障不得影响模型调用（§十三）。
    }
  };

  return {
    callId: frozen.callId,
    traceId: frozen.traceId,
    parentCallId: frozen.parentCallId,
    setAttempt(attemptId) {
      currentAttemptId = typeof attemptId === "string" && attemptId.trim() ? attemptId.trim() : null;
    },
    captureSemanticRequest(input) {
      try {
        captureSemanticRequestImpl(deliver, input);
      } catch { /* never break */ }
    },
    captureProviderRequest(input) {
      try {
        nextOrdinal += 1;
        const ordinal = nextOrdinal;
        lastOrdinal = ordinal;
        captureProviderRequestImpl(deliver, ordinal, input);
      } catch { /* never break */ }
    },
    captureProviderResponse(input) {
      try {
        captureProviderResponseImpl(deliver, input, lastOrdinal);
      } catch { /* never break */ }
    },
    captureSemanticResponse(input) {
      try {
        captureSemanticResponseImpl(deliver, input);
      } catch { /* never break */ }
    },
    noteProviderWireUnavailable(kind, options) {
      try {
        deliver(kind, {
          providerRequestOrdinal: kind === "provider_response" ? lastOrdinal : null,
          visibility: options.visibility,
          fidelity: options.fidelity,
          sanitization: { ...NO_SANITIZATION },
          payload: null,
        });
      } catch { /* never break */ }
    },
  };
}

/* ── 各层级 capture 实现 ────────────────────────────────────────────── */

function captureSemanticRequestImpl(deliver: Deliver, input: SemanticRequestCaptureInput): void {
  const actions: ModelCallPayloadSanitization["actions"] = [];
  let redacted = false;
  let truncated = false;
  let degraded = false;

  const payload: Record<string, unknown> = { inputShape: input.inputShape };

  // systemPrompt 单独走 text redactor，保留 replacements 供 provenance remap。
  let systemReplacements: TextRedactionReplacement[] = [];
  let retainedSystemLength: number | null = null;
  if (typeof input.systemPrompt === "string" && input.systemPrompt.length > 0) {
    const redactedText = redactTextWithMap(input.systemPrompt);
    if (redactedText.replacements.length > 0) {
      redacted = true;
      pushAction(actions, { path: ["systemPrompt"], action: "replaced", reason: "inline-secret" });
    }
    let text = redactedText.text;
    if (text.length > MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxStringChars) {
      retainedSystemLength = MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxStringChars;
      truncated = true;
      pushAction(actions, {
        path: ["systemPrompt"],
        action: "truncated",
        reason: `string-length:${input.systemPrompt.length}`,
      });
      text = text.slice(0, MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxStringChars);
    }
    systemReplacements = redactedText.replacements;
    payload.systemPrompt = text;
  } else if (input.systemPrompt !== undefined) {
    payload.systemPrompt = input.systemPrompt ?? null;
  }

  for (const key of ["messages", "tools", "parameters"] as const) {
    const value = input[key];
    if (value === undefined || value === null) continue;
    const result = sanitizeValueForCapture(value);
    redacted ||= result.sanitization.redacted;
    truncated ||= result.sanitization.truncated;
    degraded ||= result.sanitization.degraded;
    for (const entry of result.sanitization.actions) {
      pushAction(actions, { ...entry, path: [key, ...entry.path] as Array<string | number> });
    }
    payload[key] = result.value;
  }

  // Phase 5 locator 作用于捕获副本（§四十六/§四十七）：span remap；redaction
  // 重叠/截断越界的 span 降级 null（§四十八～§五十），降级事实进 actions。
  let provenanceCopy: ModelSemanticInputProvenance | null = null;
  if (input.provenance && Array.isArray(input.provenance.sections)) {
    const sections: SemanticInputProvenanceSection[] = input.provenance.sections.map((section, ordinal) => {
      if (section.locator.root !== "systemPrompt" || !section.locator.span) return section;
      const remapped = remapSpanAfterRedaction(section.locator.span, systemReplacements);
      if (!remapped.degraded && retainedSystemLength !== null && section.locator.span.end > retainedSystemLength) {
        remapped.degraded = true;
        remapped.span = null;
      }
      if (remapped.degraded) {
        redacted = true;
        pushAction(actions, {
          path: ["semanticInputProvenance", ordinal],
          action: "replaced",
          reason: "span-redaction-overlap",
        });
        return {
          ...section,
          precision: section.precision === "exact" ? ("structural" as const) : section.precision,
          locator: { ...section.locator, span: null },
        };
      }
      return { ...section, locator: { ...section.locator, span: remapped.span } };
    });
    provenanceCopy = { ...input.provenance, sections };
  }

  deliver("semantic_request", {
    visibility: "full",
    fidelity: "runtime_exact",
    sanitization: { redacted, truncated, degraded, actions },
    payload,
    semanticInputProvenance: provenanceCopy,
  });
}

function captureProviderRequestImpl(
  deliver: Deliver,
  ordinal: number,
  input: ProviderRequestCaptureInput,
): void {
  const transport = input.transport ?? null;
  const hasBody = transport !== null && transport.body !== undefined && transport.body !== null;
  const sanitizedTransport: Record<string, unknown> = {};
  const actions: ModelCallPayloadSanitization["actions"] = [];
  let redacted = false;
  let truncated = false;
  let degraded = false;

  const merge = (result: SanitizeValueResult, prefix: string): void => {
    redacted ||= result.sanitization.redacted;
    truncated ||= result.sanitization.truncated;
    degraded ||= result.sanitization.degraded;
    for (const entry of result.sanitization.actions) {
      pushAction(actions, { ...entry, path: [prefix, ...entry.path] as Array<string | number> });
    }
  };

  if (transport) {
    if (typeof transport.method === "string" && transport.method) sanitizedTransport.method = transport.method;
    if (typeof transport.url === "string" && transport.url) {
      const url = sanitizeCapturedUrl(transport.url);
      if (typeof url !== "string") {
        redacted = true;
        pushAction(actions, { path: ["transport", "url"], action: "replaced", reason: "url-query-credential" });
      }
      sanitizedTransport.url = url;
    }
    if (transport.headers !== undefined && transport.headers !== null) {
      const result = sanitizeValueForCapture(headersToPlain(transport.headers));
      merge(result, "transport");
      sanitizedTransport.headers = result.value;
    }
    if (hasBody) {
      const result = sanitizeValueForCapture(transport.body, {
        secretPaths: secretPathsForProtocol(input.protocol),
      });
      merge(result, "transport");
      sanitizedTransport.body = result.value;
    }
  }

  deliver("provider_request", {
    providerRequestOrdinal: ordinal,
    visibility: hasBody ? "full" : "metadata_only",
    fidelity: input.fidelity ?? "runtime_exact",
    sanitization: { redacted, truncated, degraded, actions },
    payload: transport ? { transport: sanitizedTransport } : null,
    providerRequestProvenance: input.provenance ?? null,
  });
}

function captureProviderResponseImpl(
  deliver: Deliver,
  input: ProviderResponseCaptureInput,
  lastOrdinal: number | null,
): void {
  const hasBody = input.body !== undefined && input.body !== null;
  const payload: Record<string, unknown> = {};
  const actions: ModelCallPayloadSanitization["actions"] = [];
  let redacted = false;
  let truncated = false;
  let degraded = false;

  if (typeof input.status === "number" && Number.isFinite(input.status)) payload.status = input.status;
  if (input.headers !== undefined && input.headers !== null) {
    const result = sanitizeValueForCapture(headersToPlain(input.headers));
    redacted ||= result.sanitization.redacted;
    truncated ||= result.sanitization.truncated;
    degraded ||= result.sanitization.degraded;
    for (const entry of result.sanitization.actions) {
      pushAction(actions, { ...entry, path: ["headers", ...entry.path] as Array<string | number> });
    }
    payload.headers = result.value;
  }
  if (hasBody) {
    const result = sanitizeValueForCapture(input.body);
    redacted ||= result.sanitization.redacted;
    truncated ||= result.sanitization.truncated;
    degraded ||= result.sanitization.degraded;
    for (const entry of result.sanitization.actions) {
      pushAction(actions, { ...entry, path: ["body", ...entry.path] as Array<string | number> });
    }
    payload.body = result.value;
  }

  deliver("provider_response", {
    providerRequestOrdinal: input.ordinal ?? lastOrdinal,
    visibility: input.visibility ?? (hasBody ? "full" : "metadata_only"),
    fidelity: input.fidelity,
    sanitization: { redacted, truncated, degraded, actions },
    payload,
  });
}

function captureSemanticResponseImpl(deliver: Deliver, input: SemanticResponseCaptureInput): void {
  const source = input.response;
  const actions: ModelCallPayloadSanitization["actions"] = [];
  let redacted = false;
  let truncated = false;
  let degraded = false;

  const out: Record<string, unknown> = { completeness: source.completeness ?? "complete" };
  for (const key of ["text", "reasoning", "transcription"] as const) {
    const value = source[key];
    if (typeof value !== "string" || value.length === 0) continue;
    const redactedText = redactTextWithMap(value);
    if (redactedText.replacements.length > 0) {
      redacted = true;
      pushAction(actions, { path: [key], action: "replaced", reason: "inline-secret" });
    }
    let text = redactedText.text;
    if (text.length > MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxStringChars) {
      truncated = true;
      pushAction(actions, { path: [key], action: "truncated", reason: `string-length:${value.length}` });
      text = text.slice(0, MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxStringChars);
    }
    out[key] = text;
  }
  if (typeof source.finishReason === "string" && source.finishReason.length > 0 && source.finishReason.length <= 64) {
    out.finishReason = source.finishReason;
  }
  for (const key of ["toolCalls", "structuredOutput", "media", "usage"] as const) {
    const value = source[key];
    if (value === undefined || value === null) continue;
    const result = sanitizeValueForCapture(value);
    redacted ||= result.sanitization.redacted;
    truncated ||= result.sanitization.truncated;
    degraded ||= result.sanitization.degraded;
    for (const entry of result.sanitization.actions) {
      pushAction(actions, { ...entry, path: [key, ...entry.path] as Array<string | number> });
    }
    out[key] = result.value;
  }

  deliver("semantic_response", {
    visibility: "full",
    fidelity: "normalized",
    sanitization: { redacted, truncated, degraded, actions },
    payload: out,
  });
}

/* ── 小工具 ─────────────────────────────────────────────────────────── */

function pushAction(
  actions: ModelCallPayloadSanitization["actions"],
  entry: ModelCallPayloadSanitization["actions"][number],
): void {
  if (actions.length >= MAX_ACTIONS_PER_RECORD) return;
  actions.push(entry);
}

function headersToPlain(headers: unknown): unknown {
  if (typeof Headers !== "undefined" && headers instanceof Headers) {
    const flat: Record<string, string> = {};
    headers.forEach((value, key) => { flat[key] = value; });
    return flat;
  }
  return headers;
}

export {
  captureSemanticRequestImpl as captureSemanticRequestForTest,
  captureProviderRequestImpl as captureProviderRequestForTest,
  captureProviderResponseImpl as captureProviderResponseForTest,
  captureSemanticResponseImpl as captureSemanticResponseForTest,
};
