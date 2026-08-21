import {
  runAgentLoop,
  type AgentContext,
  type AgentEvent,
  type AgentLoopConfig,
  type AgentMessage,
  type AgentTool,
  type StreamFn,
} from "../pi-sdk/index.ts";
import {
  CACHE_STRATEGIES,
  buildCacheStrategyMetadata,
} from "./cache-strategy-contract.ts";
import { stripClosedInternalNarrationBlocks } from "../text/internal-narration.ts";
import { mintModelCallId } from "./model-call-identity.ts";
import { modelCallFieldsFromUsageContext } from "./model-call-observer.ts";
import { runWithModelCallScope } from "./model-call-scope.ts";
import { resolveModelTraceContext } from "./model-trace-scope.ts";
import {
  createSemanticInputProvenance,
  provenanceSection,
} from "./semantic-input-provenance.ts";

const SUMMARY_HEADINGS = [
  "Goal",
  "Constraints & Preferences",
  "Progress",
  "Key Decisions",
  "Next Steps",
  "Critical Context",
];
const PROGRESS_HEADINGS = ["Done", "In Progress", "Blocked"];
const SUMMARY_HEADING_SEQUENCE = [
  "## Goal",
  "## Constraints & Preferences",
  "## Progress",
  "### Done",
  "### In Progress",
  "### Blocked",
  "## Key Decisions",
  "## Next Steps",
  "## Critical Context",
];
const INTERNAL_NARRATION_TYPES = ["mood", "pulse", "reflect"] as const;
const THINKING_LEVELS = new Set(["off", "minimal", "low", "medium", "high", "xhigh"]);

type InternalNarrationType = typeof INTERNAL_NARRATION_TYPES[number];

export type CachePreservingCompactionAgentRunDiagnostics = {
  providerRequests: number;
  toolIntentCount: number;
  repaired: boolean;
  sanitizedNarrationTypes: InternalNarrationType[];
  toolResults: Array<{
    toolCallId: string;
    toolName: string;
    text: string;
    isError: boolean;
  }>;
};

export interface CachePreservingCompactionCacheMetadata {
  cacheStrategy?: string;
  cacheGroup?: string;
  templateVersion?: string;
  cachePrefixHash?: string;
  parentCachePrefixHash?: string;
  strict?: boolean;
  degradeReason?: string;
  [key: string]: unknown;
}

export interface CachePreservingCompactionUsageLedger {
  start?(meta: {
    model: { provider: string | null; modelId: string | null; api: string | null };
    usageContext: unknown;
    metadata: CachePreservingCompactionCacheMetadata | null;
    costRates: unknown;
  }): { requestId?: string } | null | undefined;
  finish?(requestId: string, result: unknown): unknown;
  recordError?(requestId: string, error: unknown, status?: string, result?: unknown): unknown;
}

export interface CachePreservingCompactionAgentRunOptions {
  liveMessages?: AgentMessage[];
  systemPrompt?: string;
  tools?: AgentTool<any>[];
  model: AgentLoopConfig["model"];
  instruction: AgentMessage;
  streamFn: StreamFn;
  streamOptions?: Record<string, unknown>;
  convertToLlm: AgentLoopConfig["convertToLlm"];
  transformContext?: AgentLoopConfig["transformContext"];
  signal?: AbortSignal;
  usageLedger?: CachePreservingCompactionUsageLedger | null;
  usageContext?: unknown;
  cacheMetadata?: CachePreservingCompactionCacheMetadata | null;
  /** Deliberately ignored: temporary AgentRun events never enter the live event lane. */
  emit?: (event: AgentEvent) => unknown;
}

export interface CachePreservingCompactionAgentRunResult {
  summary: string;
  diagnostics: CachePreservingCompactionAgentRunDiagnostics;
}

type RunnerDiagnostics = CachePreservingCompactionAgentRunDiagnostics;
type RunnerError = Error & { diagnostics?: RunnerDiagnostics };

interface BuildLoopConfigOptions {
  model: AgentLoopConfig["model"];
  convertToLlm: AgentLoopConfig["convertToLlm"];
  transformContext?: AgentLoopConfig["transformContext"];
  streamOptions: Record<string, unknown>;
  shouldStopAfterTurn: NonNullable<AgentLoopConfig["shouldStopAfterTurn"]>;
}

function textContent(text: string) {
  return [{ type: "text" as const, text }];
}

function clonePlaceholderTools(tools: any[]): AgentTool<any>[] {
  return tools.map((tool) => ({
    ...tool,
    async execute() {
      return {
        content: textContent(
          "Tool intent was preserved for protocol continuity. No live tool was executed. "
          + "Continue by returning the structured compaction summary without tools.",
        ),
        details: { placeholder: true },
      };
    },
  }));
}

function normalizeThinkingLevel(value: unknown) {
  const normalized = String(value ?? "").trim().toLowerCase();
  return THINKING_LEVELS.has(normalized) ? normalized : "off";
}

function createRepairInstruction(issues: string[], draft: string): AgentMessage {
  return {
    role: "user",
    content: textContent([
      "Internal compaction summary repair.",
      "The previous draft cannot be accepted.",
      "Do not call tools. Do not address the user.",
      `Validation failures:\n${issues.map((issue) => `- ${issue}`).join("\n")}`,
      `Return only the repaired summary with these level-two headings exactly once and in order:\n${
        SUMMARY_HEADINGS.map((heading) => `## ${heading}`).join("\n")
      }`,
      `Inside "## Progress", use exactly these level-three headings once and in order:\n${
        PROGRESS_HEADINGS.map((heading) => `### ${heading}`).join("\n")
      }`,
      `<draft-summary>\n${draft}\n</draft-summary>`,
    ].join("\n\n")),
    timestamp: Date.now(),
  };
}

function sanitizeSummary(rawText: string) {
  let text = String(rawText || "");
  const removed = new Set<InternalNarrationType>();

  for (const type of INTERNAL_NARRATION_TYPES) {
    const completeXmlBlock = new RegExp(
      `<${type}(?:\\s[^>]*)?>[\\s\\S]*?<\\/${type}\\s*>`,
      "gi",
    );
    const completeFencedBlock = new RegExp(
      `\`\`\`${type}\\b[\\s\\S]*?\`\`\``,
      "gi",
    );
    if (completeXmlBlock.test(text) || completeFencedBlock.test(text)) {
      removed.add(type);
    }
  }
  text = stripClosedInternalNarrationBlocks(text);
  text = text.replace(/\r?\n[ \t]*\r?\n[ \t]*\r?\n+/g, "\n\n");

  const unmatched = new Set<InternalNarrationType>();
  const remainingTag = /<\/?(mood|pulse|reflect)\b/gi;
  for (const match of text.matchAll(remainingTag)) {
    unmatched.add(match[1].toLowerCase() as InternalNarrationType);
  }
  const remainingFence = /```(mood|pulse|reflect)\b/gi;
  for (const match of text.matchAll(remainingFence)) {
    unmatched.add(match[1].toLowerCase() as InternalNarrationType);
  }

  return {
    text: text.trim(),
    removed: [...removed],
    unmatched: [...unmatched],
  };
}

function validateSummary(text: string, unmatchedNarration: InternalNarrationType[]) {
  const issues: string[] = [];
  if (!text.trim()) issues.push("summary is empty");
  if (unmatchedNarration.length > 0) {
    issues.push(`unmatched internal narration tag(s): ${unmatchedNarration.join(", ")}`);
  }

  const headings = [...text.matchAll(/^(#{2,3})[ \t]+(.+?)[ \t]*#*[ \t]*$/gm)]
    .map((match) => `${match[1]} ${match[2].trim()}`);
  if (headings.length !== SUMMARY_HEADING_SEQUENCE.length) {
    issues.push(
      `expected ${SUMMARY_HEADING_SEQUENCE.length} structured headings, received ${headings.length}`,
    );
  }
  for (let index = 0; index < SUMMARY_HEADING_SEQUENCE.length; index += 1) {
    if (headings[index] !== SUMMARY_HEADING_SEQUENCE[index]) {
      issues.push(
        `heading ${index + 1} must be "${SUMMARY_HEADING_SEQUENCE[index]}"`,
      );
    }
  }

  return { ok: issues.length === 0, issues };
}

function extractAssistantText(message: any) {
  return message?.content
    ?.filter((block: any) => block?.type === "text" && typeof block.text === "string")
    ?.map((block: any) => block.text)
    ?.join("\n")
    ?.trim() || "";
}

function latestAssistant(messages: AgentMessage[]) {
  return [...messages].reverse().find((message: any) => message?.role === "assistant") as any;
}

function diagnosticsSnapshot(diagnostics: RunnerDiagnostics): RunnerDiagnostics {
  return {
    ...diagnostics,
    sanitizedNarrationTypes: [...diagnostics.sanitizedNarrationTypes],
    toolResults: diagnostics.toolResults.map((result) => ({ ...result })),
  };
}

function laneError(message: string, diagnostics: RunnerDiagnostics): RunnerError {
  const error: RunnerError = new Error(message);
  error.diagnostics = diagnosticsSnapshot(diagnostics);
  return error;
}

function abortError(message: string, diagnostics: RunnerDiagnostics): RunnerError {
  const error = laneError(message, diagnostics);
  error.name = "AbortError";
  return error;
}

function recoveryMetadata(
  cacheMetadata: CachePreservingCompactionCacheMetadata | null,
  degradeReason: string,
) {
  return buildCacheStrategyMetadata({
    cacheStrategy: CACHE_STRATEGIES.CACHE_RECOVERY,
    cacheGroup: cacheMetadata?.cacheGroup || "compaction.history",
    templateVersion: `${cacheMetadata?.templateVersion || "agent-run.v1"}.repair`,
    cachePrefixHash: "",
    parentCachePrefixHash: cacheMetadata?.cachePrefixHash || "",
    strict: false,
    degradeReason,
  });
}

function modelLedgerIdentity(model: AgentLoopConfig["model"]) {
  return {
    provider: model?.provider ?? null,
    modelId: model?.id ?? null,
    api: model?.api ?? null,
  };
}

function buildLoopConfig({
  model,
  convertToLlm,
  transformContext,
  streamOptions,
  shouldStopAfterTurn,
}: BuildLoopConfigOptions): AgentLoopConfig {
  const options: Record<string, any> = { ...(streamOptions || {}) };
  const requestedThinking = options.thinkingLevel ?? options.reasoning;
  delete options.thinkingLevel;
  delete options.toolChoice;
  delete options.beforeToolCall;
  delete options.afterToolCall;
  delete options.prepareNextTurn;
  delete options.shouldStopAfterTurn;
  delete options.getSteeringMessages;
  delete options.getFollowUpMessages;

  if (requestedThinking !== undefined) {
    const thinkingLevel = normalizeThinkingLevel(requestedThinking);
    if (model?.reasoning && thinkingLevel !== "off") {
      options.reasoning = thinkingLevel;
    } else {
      delete options.reasoning;
    }
  }

  const config: AgentLoopConfig = {
    ...options,
    model,
    convertToLlm,
    toolExecution: "sequential",
    shouldStopAfterTurn,
    getSteeringMessages: async () => [],
    getFollowUpMessages: async () => [],
  };
  if (typeof transformContext === "function") config.transformContext = transformContext;
  return config;
}

export async function runCachePreservingCompactionAgentRun({
  liveMessages = [],
  systemPrompt = "",
  tools = [],
  model,
  instruction,
  streamFn,
  streamOptions = {},
  convertToLlm,
  transformContext,
  signal,
  usageLedger = null,
  usageContext = null,
  cacheMetadata = null,
}: CachePreservingCompactionAgentRunOptions): Promise<CachePreservingCompactionAgentRunResult> {
  if (!model) throw new Error("Cache-preserving compaction AgentRun requires a model");
  if (!instruction) throw new Error("Cache-preserving compaction AgentRun requires an instruction");
  if (typeof streamFn !== "function") {
    throw new Error("Cache-preserving compaction AgentRun requires an isolated stream function");
  }
  if (typeof convertToLlm !== "function") {
    throw new Error("Cache-preserving compaction AgentRun requires convertToLlm");
  }

  const diagnostics: RunnerDiagnostics = {
    providerRequests: 0,
    toolIntentCount: 0,
    repaired: false,
    sanitizedNarrationTypes: [],
    toolResults: [],
  };
  if (signal?.aborted) throw abortError("Cache-preserving compaction AgentRun aborted", diagnostics);

  const placeholderTools = clonePlaceholderTools(Array.isArray(tools) ? tools : []);
  const context: AgentContext = {
    systemPrompt,
    messages: [...(Array.isArray(liveMessages) ? liveMessages : [])],
    tools: placeholderTools,
  };
  const pendingUsage: Array<{ requestId?: string; settled: boolean }> = [];
  let requestPhase: "strict" | "tool_recovery" | "format_repair" = "strict";
  let toolViolation = "";

  const settleUsage = (message: any) => {
    const pending = pendingUsage.find((entry) => !entry.settled);
    if (!pending) return;
    pending.settled = true;
    if (!pending.requestId) return;
    const result = {
      usage: message?.usage,
      model: modelLedgerIdentity(model),
      costRates: model?.cost,
    };
    if (message?.stopReason === "error" || message?.stopReason === "aborted") {
      const error = new Error(message?.errorMessage || message.stopReason);
      usageLedger?.recordError?.(pending.requestId, error, "error", result);
      return;
    }
    usageLedger?.finish?.(pending.requestId, result);
  };

  const localEmit = async (event: AgentEvent) => {
    if (event.type === "message_end" && (event.message as any)?.role === "assistant") {
      settleUsage(event.message);
    }
    if (event.type === "message_end" && (event.message as any)?.role === "toolResult") {
      const message: any = event.message;
      diagnostics.toolResults.push({
        toolCallId: message.toolCallId,
        toolName: message.toolName,
        text: message.content
          ?.filter((block: any) => block?.type === "text")
          ?.map((block: any) => block.text)
          ?.join("\n")
          ?.trim() || "",
        isError: Boolean(message.isError),
      });
    }
  };

  const isolatedStreamFn: StreamFn = async (selectedModel: any, providerContext: any, options: any) => {
    diagnostics.providerRequests += 1;
    const metadata = requestPhase === "strict" && diagnostics.providerRequests === 1
      ? (cacheMetadata ? { ...cacheMetadata } : null)
      : recoveryMetadata(
        cacheMetadata,
        requestPhase === "format_repair" ? "summary_format_repair" : "tool_intent_recovery",
      );
    // 每次 isolatedStreamFn 调用 = 一次真实的 logical model call（业务级
    // recovery/repair 是新 call，不复用 callId——§十七）。callId 先于
    // ledger.start 铸造，经 metadata.modelCallId 建立 callId↔ledger 关联，
    // 并经 ALS scope 交给 streamFn wrapper 接管为同一身份（单点发射）。
    const modelCallId = mintModelCallId();
    // Trace 身份走统一解析（§四十一）：mid-turn compaction 继承当前任务 trace，
    // 独立 fresh-compact/deleted-continuation 在无 scope 处铸 singleton。每个
    // recovery turn 解析一次——同链顺序 turn 的 parent 由 scope.lastCallId 推进。
    const traceContext = resolveModelTraceContext();
    const usageRequest = usageLedger?.start?.({
      model: modelLedgerIdentity(model),
      usageContext,
      metadata: {
        ...(metadata || {}),
        modelCallId,
        traceId: traceContext.traceId,
        parentCallId: traceContext.parentCallId,
      },
      costRates: model?.cost,
    }) || {};
    const pending = { requestId: usageRequest.requestId, settled: false };
    pendingUsage.push(pending);
    // Phase 5：per-call Semantic Input Provenance（§五十五/§五十六）。
    // 在 isolatedStreamFn 边界用**实际 providerContext** 构造：system 整段
    // structural（runner 不做快照前缀证明）；messages 按 role 分类，本 run 内
    // 最后一条 user 消息 = runner 推入的 instruction/repair（loop 只在其后追加
    // assistant/toolResult，runtime 不变量可证）——strict → task_instruction、
    // repair → format_constraint，两次 logical call 的 provenance 可区分；
    // placeholder recovery toolResult → tool_result。构造失败不影响业务。
    let semanticInputProvenance = null;
    try {
      const sections = [];
      const systemPromptText = typeof providerContext?.systemPrompt === "string"
        ? providerContext.systemPrompt
        : "";
      if (systemPromptText.length > 0) {
        sections.push(provenanceSection(
          { root: "systemPrompt", span: { start: 0, end: systemPromptText.length } },
          "session_instruction",
          { precision: "structural", role: "system", source: { type: "snapshot", id: "session.systemPrompt" } },
        ));
      }
      const messages = Array.isArray(providerContext?.messages) ? providerContext.messages : [];
      let lastUserIndex = -1;
      for (let i = messages.length - 1; i >= 0; i--) {
        if ((messages[i] as any)?.role === "user") { lastUserIndex = i; break; }
      }
      for (let i = 0; i < messages.length; i++) {
        const message = messages[i] as any;
        const role = typeof message?.role === "string" ? message.role : null;
        const locator = { root: "messages" as const, path: [i] };
        if (role === "toolResult") {
          const toolName = typeof message.toolName === "string" && message.toolName.trim()
            ? message.toolName.trim()
            : null;
          sections.push(provenanceSection(
            locator,
            "tool_result",
            { role: "tool", source: { type: "tool", ...(toolName ? { id: toolName } : {}) } },
          ));
        } else if (role === "user" && i === lastUserIndex) {
          sections.push(provenanceSection(
            locator,
            requestPhase === "format_repair" ? "format_constraint" : "task_instruction",
            { role: "user", source: { type: "runtime", id: requestPhase === "format_repair" ? "compaction.repair-instruction" : "compaction.instruction" } },
          ));
        } else if (role === "user" || role === "assistant") {
          sections.push(provenanceSection(locator, "conversation_history", { role }));
        } else {
          sections.push(provenanceSection(locator, "conversation_history", { precision: "structural", role: null }));
        }
      }
      const tools = Array.isArray(providerContext?.tools) ? providerContext.tools : [];
      for (let i = 0; i < tools.length; i++) {
        const name = typeof (tools[i] as any)?.name === "string" && (tools[i] as any).name.trim()
          ? (tools[i] as any).name.trim()
          : null;
        sections.push(provenanceSection(
          { root: "tools", path: [i] },
          "tool_definition",
          { source: { type: "tool", ...(name ? { id: name } : {}) } },
        ));
      }
      semanticInputProvenance = createSemanticInputProvenance("chat_context", sections);
    } catch {
      semanticInputProvenance = null;
    }
    const modelCallScope = {
      callId: modelCallId,
      model: modelLedgerIdentity(model),
      ...modelCallFieldsFromUsageContext(usageContext),
      traceId: traceContext.traceId,
      parentCallId: traceContext.parentCallId,
      semanticInputProvenance,
      details: {
        compactionPhase: requestPhase,
        providerRequestOrdinal: diagnostics.providerRequests,
        ...(metadata?.cacheStrategy ? { cacheStrategy: metadata.cacheStrategy } : {}),
      },
    };
    try {
      return await runWithModelCallScope(modelCallScope, () => streamFn(selectedModel, providerContext, options));
    } catch (error) {
      pending.settled = true;
      if (pending.requestId) usageLedger?.recordError?.(pending.requestId, error);
      throw error;
    }
  };

  const shouldStopAfterTurn = ({ message }: any) => {
    const toolCalls = message?.content?.filter((block: any) => block?.type === "toolCall") || [];
    if (toolCalls.length === 0) return false;
    const previousIntentCount = diagnostics.toolIntentCount;
    diagnostics.toolIntentCount += toolCalls.length;
    if (previousIntentCount > 0 || diagnostics.providerRequests > 1) {
      toolViolation = "Tool intent appeared after the first placeholder recovery turn";
      return true;
    }
    if (toolCalls.length > 1 || diagnostics.toolIntentCount > 1) {
      toolViolation = "Compaction AgentRun tool intent ceiling exceeded";
      return true;
    }
    requestPhase = "tool_recovery";
    return false;
  };

  const config = buildLoopConfig({
    model,
    convertToLlm,
    transformContext,
    streamOptions,
    shouldStopAfterTurn,
  });

  const runTurn = async (prompt: AgentMessage) => {
    try {
      const newMessages = await runAgentLoop(
        [prompt],
        context,
        config,
        localEmit,
        signal,
        isolatedStreamFn,
      );
      context.messages.push(...newMessages);
      return newMessages;
    } catch (error) {
      for (const pending of pendingUsage.filter((entry) => !entry.settled)) {
        pending.settled = true;
        if (pending.requestId) usageLedger?.recordError?.(pending.requestId, error);
      }
      if ((error as RunnerError)?.diagnostics) throw error;
      const wrapped = laneError(
        error instanceof Error ? error.message : String(error),
        diagnostics,
      );
      if (signal?.aborted) wrapped.name = "AbortError";
      throw wrapped;
    }
  };

  const firstMessages = await runTurn(instruction);
  if (toolViolation) throw laneError(toolViolation, diagnostics);
  let finalMessage = latestAssistant(firstMessages);
  if (!finalMessage) throw laneError("Compaction AgentRun returned no assistant message", diagnostics);
  if (finalMessage.stopReason !== "stop") {
    if (signal?.aborted || finalMessage.stopReason === "aborted" && signal) {
      throw abortError("Cache-preserving compaction AgentRun aborted", diagnostics);
    }
    throw laneError(
      `Cache-preserving compaction AgentRun failed with stop reason: ${finalMessage.stopReason}`,
      diagnostics,
    );
  }

  let rawText = extractAssistantText(finalMessage);
  let sanitized = sanitizeSummary(rawText);
  for (const type of sanitized.removed) {
    if (!diagnostics.sanitizedNarrationTypes.includes(type)) {
      diagnostics.sanitizedNarrationTypes.push(type);
    }
  }
  let validation = validateSummary(sanitized.text, sanitized.unmatched);

  if (!validation.ok) {
    diagnostics.repaired = true;
    requestPhase = "format_repair";
    const repairMessages = await runTurn(createRepairInstruction(validation.issues, rawText));
    if (toolViolation) throw laneError(toolViolation, diagnostics);
    finalMessage = latestAssistant(repairMessages);
    if (!finalMessage) {
      throw laneError("Compaction AgentRun repair returned no assistant message", diagnostics);
    }
    if (finalMessage.stopReason !== "stop") {
      if (signal?.aborted || finalMessage.stopReason === "aborted" && signal) {
        throw abortError("Cache-preserving compaction AgentRun repair aborted", diagnostics);
      }
      throw laneError(
        `Cache-preserving compaction AgentRun repair failed with stop reason: ${finalMessage.stopReason}`,
        diagnostics,
      );
    }
    rawText = extractAssistantText(finalMessage);
    sanitized = sanitizeSummary(rawText);
    for (const type of sanitized.removed) {
      if (!diagnostics.sanitizedNarrationTypes.includes(type)) {
        diagnostics.sanitizedNarrationTypes.push(type);
      }
    }
    validation = validateSummary(sanitized.text, sanitized.unmatched);
    if (!validation.ok) {
      throw laneError(
        `Compaction summary invalid after one repair: ${validation.issues.join("; ")}`,
        diagnostics,
      );
    }
  }

  return {
    summary: sanitized.text,
    diagnostics: diagnosticsSnapshot(diagnostics),
  };
}
