import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { runWithProviderCompatPurpose } from "./provider-compat/purpose-scope.ts";
import {
  buildNativeCompactionRequestShapes,
  convertAgentMessagesToLlm,
  estimateTokens,
  prepareCompaction,
} from "../lib/pi-sdk/index.ts";
import { computeHardTruncation } from "./compaction-utils.ts";
import { stripAllInlineMediaForHistory } from "./message-sanitizer.ts";
import {
  assertSessionSnapshotRequest,
  buildSessionCacheSnapshot,
  buildSessionSnapshotRequestContract,
  normalizeProviderVisibleTools,
} from "./session-cache-snapshot.ts";
import {
  CACHE_STRATEGIES,
  buildCacheStrategyMetadata,
} from "../lib/llm/cache-strategy-contract.ts";
import { stableSerialize } from "../lib/llm/cache-prefix-contract.ts";
import { estimateTextTokens } from "../lib/llm/estimate-text-tokens.ts";
import { runCachePreservingCompactionAgentRun } from "../lib/llm/cache-preserving-compaction-agent-run.ts";
import {
  normalizeProviderContextMessages,
  normalizeProviderPayload,
} from "./provider-compat.ts";
import { resolveOutputCapCapability } from "./provider-compat/output-budget.ts";
import {
  isReasoningReplayUnavailable,
  reasoningReplayCanClear,
} from "./provider-compat/reasoning-content-replay.ts";
import { normalizeRequestThinkingLevel } from "./session-thinking-level.ts";
import { resolveRequestReasoningLevel } from "./request-reasoning-level.ts";
import { createLossyLocalCompactionResult } from "./lossy-local-compaction.ts";
import { INSTANT_SIMPLE_COMPACTION_RUNTIME_MODE } from "../shared/compaction-mode.ts";
import { planFilePathForSession } from "../lib/plan-mode/plan-file.ts";
import { readContextNotes, CONTEXT_NOTES_MAX_BYTES, CONTEXT_NOTES_TRUNCATION_MARKER } from "../lib/tools/context-notes-tool.ts";

const DEFAULT_HARD_TRUNCATE_THRESHOLD = 0.85;

/**
 * Marks a session as being compacted by this module. It lives on the session
 * because that is what is being compacted, next to the SDK's own isCompacting
 * flag, so neither path can start while the other is running.
 */
const DIRECT_COMPACTION_IN_PROGRESS = Symbol("hanaDirectCompactionInProgress");

/** True while this module is compacting the given session. */
export function isDirectCompactionInProgress(session: any) {
  return session?.[DIRECT_COMPACTION_IN_PROGRESS] === true;
}
const COMPACTION_REQUEST_BUFFER_TOKENS = 1024;
const OUTPUT_CAP_FIELDS = ["max_completion_tokens", "max_tokens", "max_output_tokens", "maxOutputTokens"];
const OUTPUT_CAP_FIELD_SET = new Set(OUTPUT_CAP_FIELDS);

export const COMPACTION_OUTPUT_POLICIES = Object.freeze({
  PROVIDER_DEFAULT: "provider-default",
  BOUNDED: "bounded",
});

function messageTimestamp(value: any): number | null {
  if (typeof value === "number") return Number.isFinite(value) ? value : null;
  if (typeof value !== "string") return null;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function usageConstrainsContext(usage: any): boolean {
  if (!usage || typeof usage !== "object") return false;
  return [
    usage.totalTokens,
    usage.input,
    usage.output,
    usage.cacheRead,
    usage.cacheWrite,
  ].some((value) => Number.isFinite(Number(value)) && Number(value) > 0);
}

function clearContextUsage(usage: any) {
  return {
    ...usage,
    input: 0,
    output: 0,
    cacheRead: 0,
    cacheWrite: 0,
    totalTokens: 0,
  };
}

/**
 * Project live messages into the usage epoch opened by the latest compaction.
 *
 * Pi's provider-side output clamp uses the most recent assistant usage it can
 * find. A retained assistant appears after the compaction summary in message
 * order, but its timestamp and usage still belong to the larger, pre-summary
 * prompt. Keeping that usage can make the next answer look as if only one or
 * two tokens remain. Clear only the projected usage for assistants that cannot
 * be proven newer than the latest summary; the persisted messages are never
 * mutated. Without a compaction summary, return the original array unchanged.
 */
export function projectMessagesToLatestCompactionUsageEpoch(messages: any[]) {
  if (!Array.isArray(messages)) return messages;

  let summaryIndex = -1;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (messages[index]?.role === "compactionSummary") {
      summaryIndex = index;
      break;
    }
  }
  if (summaryIndex < 0) return messages;

  const summaryTimestamp = messageTimestamp(messages[summaryIndex]?.timestamp);
  let changed = false;
  const projected = messages.map((message, index) => {
    if (message?.role !== "assistant" || !usageConstrainsContext(message.usage)) return message;

    const assistantTimestamp = messageTimestamp(message.timestamp);
    const isProvenCurrentEpoch = index > summaryIndex
      && summaryTimestamp !== null
      && assistantTimestamp !== null
      && assistantTimestamp > summaryTimestamp;
    if (isProvenCurrentEpoch) return message;

    changed = true;
    return {
      ...message,
      usage: clearContextUsage(message.usage),
    };
  });

  return changed ? projected : messages;
}

export const CACHE_PRESERVING_COMPACTION_PREFIX_CONTRACT_ERROR =
  "CACHE_PRESERVING_COMPACTION_PREFIX_CONTRACT";

export class CachePreservingCompactionPrefixContractError extends Error {
  code = CACHE_PRESERVING_COMPACTION_PREFIX_CONTRACT_ERROR;
  details: Record<string, any>;

  constructor(message: string, details: Record<string, any> = {}) {
    super(message);
    this.name = "CachePreservingCompactionPrefixContractError";
    this.details = details;
  }
}

export const COMPACTION_HISTORY_REPLAY_UNPROCESSABLE =
  "COMPACTION_HISTORY_REPLAY_UNPROCESSABLE";

export class CompactionHistoryReplayError extends Error {
  code = COMPACTION_HISTORY_REPLAY_UNPROCESSABLE;
  status = 422;
  statusCode = 422;
  details: Record<string, any>;

  constructor(details: Record<string, any> = {}) {
    const retained = details.boundaryRegion === "retained";
    super(
      retained
        ? "This session cannot be compacted safely because its recent tool-call history is missing reasoning data required by the model. Start a new session, or continue with a model or mode that does not require this reasoning history to be replayed."
        : "This session cannot be compacted safely because older tool-call history is missing reasoning data required by the model, and no complete tool transaction boundary can be used to remove only the damaged prefix. Start a new session, or continue with a model or mode that does not require this reasoning history to be replayed.",
    );
    this.name = "CompactionHistoryReplayError";
    this.details = details;
  }
}

export function isCompactionHistoryReplayError(error: any) {
  return error?.code === COMPACTION_HISTORY_REPLAY_UNPROCESSABLE;
}

function prefixContractError(message: string, details: Record<string, any> = {}) {
  return new CachePreservingCompactionPrefixContractError(
    `Cache-preserving compaction prefix contract is not proven: ${message}`,
    details,
  );
}

async function convertMessagePartition(convertToLlm, messages, label) {
  const converted = await convertToLlm(messages);
  if (!Array.isArray(converted)) {
    throw prefixContractError(`${label} conversion did not return a message array`);
  }
  return converted;
}

export async function deriveCachePreservingCompactionBoundary({
  liveMessages,
  preparation,
  convertToLlm = convertAgentMessagesToLlm,
}: {
  liveMessages?: any[];
  preparation?: any;
  convertToLlm?: any;
} = {}) {
  if (!Array.isArray(liveMessages)) {
    throw prefixContractError("live context messages are unavailable");
  }
  if (!preparation || typeof preparation !== "object") {
    throw prefixContractError("Pi compaction preparation is unavailable");
  }
  if (typeof convertToLlm !== "function") {
    throw prefixContractError("convertToLlm is unavailable");
  }

  const messagesToSummarize = Array.isArray(preparation.messagesToSummarize)
    ? preparation.messagesToSummarize
    : [];
  const turnPrefixMessages = Array.isArray(preparation.turnPrefixMessages)
    ? preparation.turnPrefixMessages
    : [];
  const hasPreviousSummary = typeof preparation.previousSummary === "string";
  const firstLiveMessage = liveMessages[0];
  const previousSummaryRepresented = hasPreviousSummary
    && firstLiveMessage?.role === "compactionSummary"
    && firstLiveMessage?.summary === preparation.previousSummary;

  if (hasPreviousSummary && !previousSummaryRepresented) {
    throw prefixContractError("previousSummary is not represented by the live compactionSummary");
  }

  const expectedOldRegion = [
    ...(previousSummaryRepresented ? [firstLiveMessage] : []),
    ...messagesToSummarize,
    ...turnPrefixMessages,
  ];
  if (expectedOldRegion.length > liveMessages.length) {
    throw prefixContractError("Pi preparation extends beyond the live context", {
      expectedOldRegionLength: expectedOldRegion.length,
      liveMessageCount: liveMessages.length,
    });
  }
  const actualOldRegion = liveMessages.slice(0, expectedOldRegion.length);
  if (stableSerialize(actualOldRegion) !== stableSerialize(expectedOldRegion)) {
    throw prefixContractError("Pi preparation does not match the live context prefix", {
      expectedOldRegionLength: expectedOldRegion.length,
      liveMessageCount: liveMessages.length,
    });
  }

  const retainedRawMessages = liveMessages.slice(expectedOldRegion.length);
  const providerOldMessages = await convertMessagePartition(
    convertToLlm,
    actualOldRegion,
    "old-region",
  );
  const providerRetainedMessages = await convertMessagePartition(
    convertToLlm,
    retainedRawMessages,
    "retained-region",
  );
  const providerLiveMessages = await convertMessagePartition(
    convertToLlm,
    liveMessages,
    "live-prefix",
  );
  if (
    stableSerialize(providerLiveMessages)
    !== stableSerialize([...providerOldMessages, ...providerRetainedMessages])
  ) {
    throw prefixContractError("provider-visible partitions do not reconstruct the live prefix", {
      providerLiveMessageCount: providerLiveMessages.length,
      providerOldMessageCount: providerOldMessages.length,
      providerRetainedMessageCount: providerRetainedMessages.length,
    });
  }

  return {
    rawBoundaryIndex: expectedOldRegion.length,
    oldMessageCount: providerOldMessages.length,
    retainedMessageCount: providerRetainedMessages.length,
    previousSummaryRepresented,
  };
}

function positiveInteger(value) {
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? Math.floor(number) : null;
}

function normalizeCompactionOutputPolicy(value) {
  const policy = value || COMPACTION_OUTPUT_POLICIES.PROVIDER_DEFAULT;
  if (policy === COMPACTION_OUTPUT_POLICIES.PROVIDER_DEFAULT || policy === COMPACTION_OUTPUT_POLICIES.BOUNDED) {
    return policy;
  }
  throw new Error(`Unknown compaction output policy: ${String(value)}`);
}

function hasValidOutputCap(payload) {
  return OUTPUT_CAP_FIELDS.some((field) => positiveInteger(payload?.[field]) !== null);
}

function safeRequiredOutputCap(model, boundedMaxTokens) {
  const modelCandidates = [
    positiveInteger(model?.maxTokens || model?.maxOutput),
    positiveInteger(model?.contextWindow),
  ].filter((value) => value !== null);
  if (modelCandidates.length > 0) return Math.min(...modelCandidates);
  return positiveInteger(boundedMaxTokens) ?? 1;
}

function requiredOutputCapField(payload, model) {
  const explicit = model?.compat?.outputCapField;
  if (typeof explicit === "string" && OUTPUT_CAP_FIELD_SET.has(explicit)) return explicit;
  return OUTPUT_CAP_FIELDS.find((field) => Object.prototype.hasOwnProperty.call(payload || {}, field)) || "max_tokens";
}

export function normalizeCompactionProviderPayload(payload, model, {
  outputPolicy = COMPACTION_OUTPUT_POLICIES.PROVIDER_DEFAULT,
  boundedMaxTokens,
  ...providerOptions
}: Record<string, any> = {}) {
  const policy = normalizeCompactionOutputPolicy(outputPolicy);
  const hadValidOutputCap = hasValidOutputCap(payload);
  let normalized = normalizeProviderPayload(payload, model, {
    ...providerOptions,
    mode: "chat",
    // compaction 不继承模型级联网/结构化输出开关（模型开关只作用于普通聊天）
    purpose: "compaction",
    outputBudgetSource: policy === COMPACTION_OUTPUT_POLICIES.BOUNDED ? "system" : "sdk-default",
  });
  const capability = resolveOutputCapCapability(model);

  if (policy === COMPACTION_OUTPUT_POLICIES.BOUNDED) {
    if (hasValidOutputCap(normalized)) return normalized;
    const bounded = positiveInteger(boundedMaxTokens);
    if (bounded === null) throw new Error("Bounded compaction output requires a positive token limit");
    return { ...normalized, [requiredOutputCapField(normalized, model)]: bounded };
  }
  if (!capability.required) {
    for (const field of OUTPUT_CAP_FIELDS) {
      if (!Object.prototype.hasOwnProperty.call(normalized, field)) continue;
      if (normalized === payload) normalized = { ...normalized };
      delete normalized[field];
    }
    return normalized;
  }

  if (!hadValidOutputCap) {
    const field = requiredOutputCapField(normalized, model);
    const next = { ...normalized };
    for (const outputField of OUTPUT_CAP_FIELDS) delete next[outputField];
    next[field] = safeRequiredOutputCap(model, boundedMaxTokens);
    normalized = next;
  }
  return normalized;
}

/**
 * The thinking level a compaction runs at, and the reasoning level its request
 * carries. The second is not decided here: it comes from the same function the
 * live pipeline asks, because a compaction request that reasons differently
 * than the live requests it follows cannot ride their cache prefix. This used
 * to gate on the model's own reasoning capability and answer "no reasoning"
 * where the live request answered a level, which cost a cold prefix on every
 * such compaction and reported nothing.
 */
export function resolveCompactionReasoningPolicy(thinkingLevel) {
  const normalizedThinkingLevel = normalizeRequestThinkingLevel(thinkingLevel, "off");
  return {
    thinkingLevel: normalizedThinkingLevel,
    reasoningLevel: resolveRequestReasoningLevel({ sessionThinkingLevel: normalizedThinkingLevel }),
  };
}

function textBlock(text: string) {
  return { type: "text" as const, text };
}

export { estimateTextTokens };

export function getCachePreservingCompactionMaxTokens(preparation) {
  return Math.max(512, Math.floor((preparation?.settings?.reserveTokens ?? 4096) * 0.8));
}

function buildCachePreservingCompactionInstructionValue({
  preparation,
  customInstructions,
  liveMessageCount = 0,
  retainedMessageCount = 0,
  boundaryPlaceholder = null,
}: {
  preparation?: any;
  customInstructions?: any;
  liveMessageCount?: number;
  retainedMessageCount?: number;
  boundaryPlaceholder?: string | null;
} = {}) {
  if (!Number.isInteger(retainedMessageCount) || retainedMessageCount < 0 || retainedMessageCount > liveMessageCount) {
    throw prefixContractError("retainedMessageCount is outside the live provider prefix", {
      liveMessageCount,
      retainedMessageCount,
    });
  }
  const oldRegionEnd = liveMessageCount - retainedMessageCount;
  const scopeLines = [
    "Internal compaction-only run.",
    "Do not call tools. Do not address the user.",
    "Do not output <mood>, <pulse>, <reflect>, or any other internal narration.",
    "Return only the exact structured checkpoint format below.",
    boundaryPlaceholder || buildCompactionBoundaryScope(oldRegionEnd),
    "Use recent-tail content only to understand continuity; never restate it as though it will be removed.",
    "If the live prefix begins with an existing compaction checkpoint, incorporate it from that position without duplicating it.",
  ];
  if (preparation?.isSplitTurn) {
    scopeLines.push(
      "This is a split-turn compaction: preserve the original request and early progress needed to understand the retained suffix.",
    );
  }
  if (customInstructions) {
    scopeLines.push(`Additional focus for the checkpoint only: ${customInstructions}`);
  }
  const prompt = `${scopeLines.join("\n")}

Use this EXACT format:

## Goal
[What is the user trying to accomplish? Can be multiple items if the session covers different tasks.]

## Constraints & Preferences
- [Any constraints, preferences, or requirements mentioned by user]
- [Or "(none)" if none were mentioned]

## Progress
### Done
- [x] [Completed tasks/changes]

### In Progress
- [ ] [Current work]

### Blocked
- [Issues preventing progress, if any]

## Key Decisions
- **[Decision]**: [Brief rationale]

## Next Steps
1. [Ordered list of what should happen next]

## Critical Context
- [Only old-region context needed to continue from the retained suffix]
- [Or "(none)" if not applicable]

Keep each section concise. Preserve exact file paths, function names, and error messages.`;
  return {
    role: "user" as const,
    content: [textBlock(prompt)],
    timestamp: Date.now(),
  };
}

export function buildCachePreservingCompactionInstruction({
  preparation,
  customInstructions,
  liveMessageCount = 0,
  retainedMessageCount = 0,
}: {
  preparation?: any;
  customInstructions?: any;
  liveMessageCount?: number;
  retainedMessageCount?: number;
} = {}) {
  return buildCachePreservingCompactionInstructionValue({
    preparation,
    customInstructions,
    liveMessageCount,
    retainedMessageCount,
  });
}

const COMPACTION_TRANSFORM_PROOF_KEY_PREFIX = "__hana_compaction_transform_proof_";
const COMPACTION_TRANSFORM_PROOF_KEY_ATTEMPTS = 16;
let compactionBoundaryPlaceholderSequence = 0;

function buildCompactionBoundaryScope(oldRegionEnd: number) {
  return [
    `Old region: live message indexes [0, ${oldRegionEnd}). Summarize only that old region.`,
    `Retained boundary: live message index ${oldRegionEnd}. Messages from that boundary onward remain verbatim in the session.`,
  ].join("\n");
}

function createCompactionBoundaryPlaceholder() {
  compactionBoundaryPlaceholderSequence += 1;
  return `<lingxi.compaction.boundary:${Date.now().toString(36)}:${compactionBoundaryPlaceholderSequence}>`;
}

function countStringTokenOccurrences(value: any, token: string): number {
  if (typeof value === "string") return value.split(token).length - 1;
  if (Array.isArray(value)) {
    return value.reduce(
      (count, item) => count + countStringTokenOccurrences(item, token),
      0,
    );
  }
  if (!value || typeof value !== "object") return 0;
  let count = 0;
  for (const key of Reflect.ownKeys(value)) {
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    if (descriptor && "value" in descriptor) {
      count += countStringTokenOccurrences(descriptor.value, token);
    }
  }
  return count;
}

function replaceStringToken(value: any, token: string, replacement: string): any {
  if (typeof value === "string") return value.replace(token, replacement);
  if (!value || typeof value !== "object") return value;
  if (countStringTokenOccurrences(value, token) === 0) return value;

  const clone = Array.isArray(value)
    ? []
    : Object.create(Object.getPrototypeOf(value));
  for (const key of Reflect.ownKeys(value)) {
    if (Array.isArray(value) && key === "length") continue;
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    if (!descriptor) continue;
    const nextDescriptor = "value" in descriptor
      ? {
          ...descriptor,
          value: replaceStringToken(descriptor.value, token, replacement),
        }
      : descriptor;
    Object.defineProperty(clone, key, nextDescriptor);
  }
  if (Array.isArray(value)) {
    Object.defineProperty(
      clone,
      "length",
      Object.getOwnPropertyDescriptor(value, "length"),
    );
    Object.setPrototypeOf(clone, Object.getPrototypeOf(value));
  }
  return clone;
}

function assertTransformProofInputMessages(messages: any[], instruction: any) {
  for (const [index, message] of [...messages, instruction].entries()) {
    if (!message || typeof message !== "object" || Array.isArray(message)) {
      throw prefixContractError("transform input contains a non-message value", { index });
    }
  }
}

function createTransformProofCarrier(messages: any[], instruction: any) {
  assertTransformProofInputMessages(messages, instruction);
  const inputs = [...messages, instruction];
  for (let attempt = 0; attempt < COMPACTION_TRANSFORM_PROOF_KEY_ATTEMPTS; attempt += 1) {
    const nonce = randomUUID();
    const key = `${COMPACTION_TRANSFORM_PROOF_KEY_PREFIX}${nonce}`;
    const collides = inputs.some((message) => Reflect.ownKeys(message).includes(key));
    if (collides) continue;
    return {
      key,
      messageValues: messages.map((_, index) => `${nonce}:message:${index}`),
      instructionValue: `${nonce}:instruction`,
    };
  }
  throw prefixContractError("could not allocate a collision-free transform proof carrier");
}

function tagTransformProofMessage(message: any, key: string, value: string) {
  const tagged = Object.create(Object.getPrototypeOf(message));
  Object.defineProperties(tagged, Object.getOwnPropertyDescriptors(message));
  Object.defineProperty(tagged, key, {
    configurable: true,
    enumerable: true,
    value,
    writable: true,
  });
  return tagged;
}

function transformProofTokens(proof: {
  key: string;
  messageValues: string[];
  instructionValue: string;
}) {
  return [proof.key, ...proof.messageValues, proof.instructionValue];
}

function containsTransformProofToken(value: unknown, tokens: string[]) {
  return typeof value === "string" && tokens.some((token) => value.includes(token));
}

function assertTransformProofArtifactsOnlyAtTopLevel({
  messages,
  allowedTopLevelValues,
  proof,
}: {
  messages: any[];
  allowedTopLevelValues: Array<string | undefined>;
  proof: {
    key: string;
    messageValues: string[];
    instructionValue: string;
  };
}) {
  const tokens = transformProofTokens(proof);
  const visited = new WeakSet<object>();

  const visitStrict = (value: unknown) => {
    if (containsTransformProofToken(value, tokens)) {
      throw prefixContractError("transformContext copied a transform proof token outside its allowed top-level slot");
    }
    if (!value || typeof value !== "object") return;
    if (visited.has(value)) return;
    visited.add(value);
    for (const key of Reflect.ownKeys(value)) {
      if (containsTransformProofToken(key, tokens)) {
        throw prefixContractError("transformContext copied a transform proof key outside its allowed top-level slot");
      }
      const descriptor = Object.getOwnPropertyDescriptor(value, key);
      if (!descriptor || !("value" in descriptor)) continue;
      visitStrict(descriptor.value);
    }
  };

  for (const [index, message] of messages.entries()) {
    const allowedValue = allowedTopLevelValues[index];
    if (allowedValue === undefined) {
      visitStrict(message);
      continue;
    }
    for (const key of Reflect.ownKeys(message)) {
      const descriptor = Object.getOwnPropertyDescriptor(message, key);
      if (
        key === proof.key
        && descriptor
        && "value" in descriptor
        && descriptor.enumerable === true
        && descriptor.value === allowedValue
      ) {
        continue;
      }
      if (containsTransformProofToken(key, tokens)) {
        throw prefixContractError("transformContext copied a transform proof key outside its allowed top-level slot");
      }
      if (!descriptor || !("value" in descriptor)) continue;
      visitStrict(descriptor.value);
    }
  }
}

function assertNoTransformProofArtifacts(
  messages: any[],
  proof: {
    key: string;
    messageValues: string[];
    instructionValue: string;
  },
) {
  assertTransformProofArtifactsOnlyAtTopLevel({
    messages,
    allowedTopLevelValues: messages.map(() => undefined),
    proof,
  });
}

function stripTransformProofMarker(
  message: any,
  markerKey: string,
  expectedValue: string | undefined,
) {
  const clean = Object.create(Object.getPrototypeOf(message));
  for (const key of Reflect.ownKeys(message)) {
    const descriptor = Object.getOwnPropertyDescriptor(message, key);
    if (key === markerKey) {
      if (
        expectedValue === undefined
        || !descriptor
        || !("value" in descriptor)
        || descriptor.enumerable !== true
        || descriptor.value !== expectedValue
      ) {
        throw prefixContractError("transformContext altered an allowed top-level proof slot");
      }
      continue;
    }
    if (descriptor) Object.defineProperty(clean, key, descriptor);
  }
  return clean;
}

async function applyTransformProofPass({
  liveMessages,
  instruction,
  boundaryPlaceholder,
  rawBoundaryIndex,
  transformContext,
  signal,
}: {
  liveMessages: any[];
  instruction: any;
  boundaryPlaceholder: string;
  rawBoundaryIndex: number;
  transformContext: any;
  signal?: any;
}) {
  const proof = createTransformProofCarrier(liveMessages, instruction);
  const taggedLive = liveMessages.map((message, index) => (
    tagTransformProofMessage(message, proof.key, proof.messageValues[index])
  ));
  const taggedInstruction = tagTransformProofMessage(
    instruction,
    proof.key,
    proof.instructionValue,
  );
  const transformed = await transformContext([...taggedLive, taggedInstruction], signal);
  if (!Array.isArray(transformed)) {
    throw prefixContractError("transformContext did not return a message array");
  }
  for (const [index, message] of transformed.entries()) {
    if (!message || typeof message !== "object" || Array.isArray(message)) {
      throw prefixContractError("transformContext returned a non-message value", { index });
    }
  }
  const proofValueToMessageIndex = new Map(
    proof.messageValues.map((value, index) => [value, index]),
  );
  const observedProofSequence: Array<number | "instruction" | "unknown"> = [];
  const instructionIndexes: number[] = [];
  const allowedTopLevelProofValues: Array<string | undefined> = transformed.map(() => undefined);
  const transformedProofMessageIndexes: Array<number | undefined> = transformed.map(() => undefined);
  for (const [index, message] of transformed.entries()) {
    const descriptor = Object.getOwnPropertyDescriptor(message, proof.key);
    if (!descriptor) continue;
    if (!("value" in descriptor) || descriptor.enumerable !== true) {
      observedProofSequence.push("unknown");
      continue;
    }
    const value = descriptor.value;
    if (value === proof.instructionValue) {
      observedProofSequence.push("instruction");
      instructionIndexes.push(index);
      allowedTopLevelProofValues[index] = value;
      continue;
    }
    const messageIndex = proofValueToMessageIndex.get(value);
    observedProofSequence.push(messageIndex ?? "unknown");
    if (messageIndex !== undefined) {
      allowedTopLevelProofValues[index] = value;
      transformedProofMessageIndexes[index] = messageIndex;
    }
  }
  const expectedProofSequence: Array<number | "instruction"> = [
    ...liveMessages.map((_, index) => index),
    "instruction",
  ];
  if (stableSerialize(observedProofSequence) !== stableSerialize(expectedProofSequence)) {
    throw prefixContractError("transformContext filtered, duplicated, reordered, or altered proof-carrying messages", {
      expectedMessageCount: liveMessages.length,
      observedProofKinds: observedProofSequence.map((value) => (
        value === "instruction" ? "instruction" : typeof value === "number" ? "message" : "unknown"
      )),
    });
  }
  if (instructionIndexes.length !== 1 || instructionIndexes[0] !== transformed.length - 1) {
    throw prefixContractError("transformContext did not preserve the final compaction instruction marker", {
      instructionIndexes,
      transformedMessageCount: transformed.length,
    });
  }
  assertTransformProofArtifactsOnlyAtTopLevel({
    messages: transformed,
    allowedTopLevelValues: allowedTopLevelProofValues,
    proof,
  });
  const transformedPrefix = transformed.slice(0, -1);
  const prefixPlaceholderCount = countStringTokenOccurrences(
    transformedPrefix,
    boundaryPlaceholder,
  );
  const instructionPlaceholderCount = countStringTokenOccurrences(
    transformed.at(-1),
    boundaryPlaceholder,
  );
  if (prefixPlaceholderCount !== 0 || instructionPlaceholderCount !== 1) {
    throw prefixContractError("transformContext did not preserve one boundary placeholder inside the instruction", {
      prefixPlaceholderCount,
      instructionPlaceholderCount,
    });
  }
  const firstRetainedIndex = transformedPrefix.findIndex((_, index) => {
    const proofMessageIndex = transformedProofMessageIndexes[index];
    return proofMessageIndex !== undefined && proofMessageIndex >= rawBoundaryIndex;
  });
  const transformedBoundaryIndex = firstRetainedIndex >= 0
    ? firstRetainedIndex
    : transformedPrefix.length;
  const cleanPrefix = transformedPrefix.map((message, index) => (
    stripTransformProofMarker(
      message,
      proof.key,
      allowedTopLevelProofValues[index],
    )
  ));
  const cleanInstruction = stripTransformProofMarker(
    transformed.at(-1),
    proof.key,
    allowedTopLevelProofValues.at(-1),
  );
  assertNoTransformProofArtifacts([...cleanPrefix, cleanInstruction], proof);
  return {
    prefix: cleanPrefix,
    oldRegion: cleanPrefix.slice(0, transformedBoundaryIndex),
    retainedRegion: cleanPrefix.slice(transformedBoundaryIndex),
    instruction: cleanInstruction,
  };
}

async function convertAndNormalizePartition({
  messages,
  convertToLlm,
  normalizeMessages,
  label,
}: {
  messages: any[];
  convertToLlm: any;
  normalizeMessages: any;
  label: string;
}) {
  const converted = await convertToLlm(messages);
  if (!Array.isArray(converted)) {
    throw prefixContractError(`${label} conversion did not return a message array`);
  }
  const normalized = await normalizeMessages(converted);
  if (!Array.isArray(normalized)) {
    throw prefixContractError(`${label} normalization did not return a message array`);
  }
  return normalized;
}

function assertNormalizedPartition({
  full,
  oldRegion,
  retainedRegion,
}: {
  full: any[];
  oldRegion: any[];
  retainedRegion: any[];
}) {
  const reconstructed = [...oldRegion, ...retainedRegion];
  if (full.length !== reconstructed.length) {
    throw prefixContractError("normalized old/retained partition changed cardinality", {
      fullMessageCount: full.length,
      oldMessageCount: oldRegion.length,
      retainedMessageCount: retainedRegion.length,
    });
  }
  if (stableSerialize(full) !== stableSerialize(reconstructed)) {
    throw prefixContractError("normalized old/retained partition does not reconstruct the live prefix", {
      fullMessageCount: full.length,
      oldMessageCount: oldRegion.length,
      retainedMessageCount: retainedRegion.length,
    });
  }
}

function readToolCallIds(message: any) {
  if (message?.role !== "assistant") return null;
  const canonicalCalls = Array.isArray(message.content)
    ? message.content.filter((block) => block?.type === "toolCall")
    : [];
  const providerCalls = Array.isArray(message.tool_calls) ? message.tool_calls : [];
  const calls = canonicalCalls.length > 0 ? canonicalCalls : providerCalls;
  if (calls.length === 0) return null;
  const ids = calls.map((call) => (
    typeof call?.id === "string" && call.id.trim() ? call.id.trim() : null
  ));
  return ids.every((id) => id !== null) ? ids : null;
}

function readToolResultId(message: any) {
  if (message?.role === "toolResult") {
    return typeof message.toolCallId === "string" && message.toolCallId.trim()
      ? message.toolCallId.trim()
      : null;
  }
  if (message?.role === "tool") {
    return typeof message.tool_call_id === "string" && message.tool_call_id.trim()
      ? message.tool_call_id.trim()
      : null;
  }
  return undefined;
}

function isConversationRestartBoundary(message: any) {
  return message?.role === "user"
    || message?.role === "system"
    || message?.role === "compactionSummary";
}

/**
 * Find prefixes that can be removed without separating a tool call from any of
 * its results. We wait until the next conversation turn (or the old-region
 * edge), so an assistant's post-tool answer stays with the request that caused
 * it. Missing or duplicate call ids make every later boundary unprovable.
 */
function completeToolTransactionTrimBoundaries(messages: any[]) {
  const pending = new Set<string>();
  const boundaries: number[] = [];
  let transactionSeen = false;
  let transactionCompleted = false;
  let unprovable = false;

  for (let index = 0; index < messages.length; index += 1) {
    const message = messages[index];
    const callIds = readToolCallIds(message);
    if (callIds) {
      transactionSeen = true;
      for (const callId of callIds) {
        if (pending.has(callId)) {
          unprovable = true;
          continue;
        }
        pending.add(callId);
      }
    } else if (
      message?.role === "assistant"
      && (
        (Array.isArray(message.content) && message.content.some((block) => block?.type === "toolCall"))
        || (Array.isArray(message.tool_calls) && message.tool_calls.length > 0)
      )
    ) {
      transactionSeen = true;
      unprovable = true;
    }

    const resultId = readToolResultId(message);
    if (resultId !== undefined) {
      if (!resultId || !pending.delete(resultId)) {
        unprovable = true;
      } else if (pending.size === 0) {
        transactionCompleted = true;
      }
    }

    const nextMessage = messages[index + 1];
    const atTurnBoundary = nextMessage === undefined || isConversationRestartBoundary(nextMessage);
    if (
      transactionSeen
      && transactionCompleted
      && !unprovable
      && pending.size === 0
      && atTurnBoundary
    ) {
      boundaries.push(index + 1);
    }
  }

  return boundaries;
}

function compactionHistoryReplayError({
  boundaryRegion,
  safeBoundaryCount = 0,
  cause,
}: {
  boundaryRegion: "old" | "retained" | "final-request";
  safeBoundaryCount?: number;
  cause?: any;
}) {
  return new CompactionHistoryReplayError({
    boundaryRegion,
    safeBoundaryCount,
    providerError: cause instanceof Error ? cause.message : String(cause || ""),
  });
}

async function normalizeCompactionPartitionsWithHistoryRecovery({
  transformed,
  convertToLlm,
  normalizeMessages,
  model,
}: {
  transformed: {
    prefix: any[];
    oldRegion: any[];
    retainedRegion: any[];
  };
  convertToLlm: any;
  normalizeMessages: any;
  model: any;
}) {
  let normalizedRetained;
  try {
    normalizedRetained = await convertAndNormalizePartition({
      messages: transformed.retainedRegion,
      convertToLlm,
      normalizeMessages,
      label: "transformed-retained-region",
    });
  } catch (error) {
    if (!isReasoningReplayUnavailable(error) || reasoningReplayCanClear(model)) throw error;
    throw compactionHistoryReplayError({
      boundaryRegion: "retained",
      cause: error,
    });
  }

  try {
    const [normalizedFull, normalizedOld] = await Promise.all([
      convertAndNormalizePartition({
        messages: transformed.prefix,
        convertToLlm,
        normalizeMessages,
        label: "transformed-live-prefix",
      }),
      convertAndNormalizePartition({
        messages: transformed.oldRegion,
        convertToLlm,
        normalizeMessages,
        label: "transformed-old-region",
      }),
    ]);
    assertNormalizedPartition({
      full: normalizedFull,
      oldRegion: normalizedOld,
      retainedRegion: normalizedRetained,
    });
    return {
      transformedPrefix: transformed.prefix,
      normalizedFull,
      normalizedOld,
      normalizedRetained,
      historyRecovery: null,
    };
  } catch (error) {
    if (!isReasoningReplayUnavailable(error) || reasoningReplayCanClear(model)) throw error;

    const boundaries = completeToolTransactionTrimBoundaries(transformed.oldRegion);
    for (const boundary of boundaries) {
      const candidateOld = transformed.oldRegion.slice(boundary);
      const candidatePrefix = [...candidateOld, ...transformed.retainedRegion];
      try {
        const [normalizedFull, normalizedOld] = await Promise.all([
          convertAndNormalizePartition({
            messages: candidatePrefix,
            convertToLlm,
            normalizeMessages,
            label: "recovered-live-prefix",
          }),
          convertAndNormalizePartition({
            messages: candidateOld,
            convertToLlm,
            normalizeMessages,
            label: "recovered-old-region",
          }),
        ]);
        assertNormalizedPartition({
          full: normalizedFull,
          oldRegion: normalizedOld,
          retainedRegion: normalizedRetained,
        });
        return {
          transformedPrefix: candidatePrefix,
          normalizedFull,
          normalizedOld,
          normalizedRetained,
          historyRecovery: {
            kind: "reasoning-replay-prefix-trim",
            removedMessageCount: boundary,
          },
        };
      } catch (candidateError) {
        if (!isReasoningReplayUnavailable(candidateError)) throw candidateError;
      }
    }

    throw compactionHistoryReplayError({
      boundaryRegion: "old",
      safeBoundaryCount: boundaries.length,
      cause: error,
    });
  }
}

export async function buildCachePreservingCompactionPrefix({
  liveMessages,
  preparation,
  model,
  customInstructions,
  transformContext = async (messages) => messages,
  convertToLlm = convertAgentMessagesToLlm,
  normalizeMessages = (messages) => normalizeProviderContextMessages(messages, model, { mode: "chat" }),
  signal,
}: {
  liveMessages?: any[];
  preparation?: any;
  model?: any;
  customInstructions?: any;
  transformContext?: any;
  convertToLlm?: any;
  normalizeMessages?: any;
  signal?: any;
} = {}) {
  const rawBoundary = await deriveCachePreservingCompactionBoundary({
    liveMessages,
    preparation,
    convertToLlm,
  });
  const boundaryPlaceholder = createCompactionBoundaryPlaceholder();
  const instructionTemplate = buildCachePreservingCompactionInstructionValue({
    preparation,
    customInstructions,
    liveMessageCount: 0,
    retainedMessageCount: 0,
    boundaryPlaceholder,
  });
  const transformed = await applyTransformProofPass({
    liveMessages,
    instruction: instructionTemplate,
    boundaryPlaceholder,
    rawBoundaryIndex: rawBoundary.rawBoundaryIndex,
    transformContext,
    signal,
  });
  const normalized = await normalizeCompactionPartitionsWithHistoryRecovery({
    transformed,
    convertToLlm,
    normalizeMessages,
    model,
  });
  const {
    transformedPrefix,
    normalizedFull,
    normalizedOld,
    normalizedRetained,
    historyRecovery,
  } = normalized;

  const oldRegionEnd = normalizedFull.length - normalizedRetained.length;
  const instruction = replaceStringToken(
    transformed.instruction,
    boundaryPlaceholder,
    buildCompactionBoundaryScope(oldRegionEnd),
  );
  if (countStringTokenOccurrences(instruction, boundaryPlaceholder) !== 0) {
    throw prefixContractError("boundary placeholder survived instruction materialization");
  }
  let finalFull;
  let finalPrefix;
  let finalInstruction;
  try {
    [finalFull, finalPrefix, finalInstruction] = await Promise.all([
      convertAndNormalizePartition({
        messages: [...transformedPrefix, instruction],
        convertToLlm,
        normalizeMessages,
        label: "transformed-final-request",
      }),
      convertAndNormalizePartition({
        messages: transformedPrefix,
        convertToLlm,
        normalizeMessages,
        label: "transformed-final-prefix",
      }),
      convertAndNormalizePartition({
        messages: [instruction],
        convertToLlm,
        normalizeMessages,
        label: "transformed-final-instruction",
      }),
    ]);
  } catch (error) {
    if (!isReasoningReplayUnavailable(error) || reasoningReplayCanClear(model)) throw error;
    throw compactionHistoryReplayError({
      boundaryRegion: "final-request",
      cause: error,
    });
  }
  if (
    finalInstruction.length !== 1
    || stableSerialize(finalFull) !== stableSerialize([...finalPrefix, ...finalInstruction])
  ) {
    throw prefixContractError("normalized final instruction does not preserve prefix append-only parity", {
      finalMessageCount: finalFull.length,
      prefixMessageCount: finalPrefix.length,
      instructionMessageCount: finalInstruction.length,
    });
  }
  if (stableSerialize(finalPrefix) !== stableSerialize(normalizedFull)) {
    throw prefixContractError("final transformed prefix differs from the proven normalized prefix");
  }

  return {
    messages: finalPrefix,
    instruction: finalInstruction[0],
    oldMessageCount: normalizedOld.length,
    retainedMessageCount: normalizedRetained.length,
    previousSummaryRepresented: rawBoundary.previousSummaryRepresented && !historyRecovery,
    historyRecovery,
  };
}

export function stripInlineMediaFromCompactionPreparation(preparation) {
  if (!preparation || typeof preparation !== "object") return preparation;

  const messagesToSummarize = Array.isArray(preparation.messagesToSummarize)
    ? stripAllInlineMediaForHistory(preparation.messagesToSummarize)
    : null;
  const turnPrefixMessages = Array.isArray(preparation.turnPrefixMessages)
    ? stripAllInlineMediaForHistory(preparation.turnPrefixMessages)
    : null;

  const next: Record<string, any> = {};
  let changed = false;
  if (messagesToSummarize && messagesToSummarize.messages !== preparation.messagesToSummarize) {
    next.messagesToSummarize = messagesToSummarize.messages;
    changed = true;
  }
  if (turnPrefixMessages && turnPrefixMessages.messages !== preparation.turnPrefixMessages) {
    next.turnPrefixMessages = turnPrefixMessages.messages;
    changed = true;
  }

  return changed ? { ...preparation, ...next } : preparation;
}


function computeFileDetails(fileOps) {
  const read = fileOps?.read instanceof Set ? fileOps.read : new Set(fileOps?.read || []);
  const written = fileOps?.written instanceof Set ? fileOps.written : new Set(fileOps?.written || []);
  const edited = fileOps?.edited instanceof Set ? fileOps.edited : new Set(fileOps?.edited || []);
  const modified = new Set([...edited, ...written]);
  return {
    readFiles: [...read].filter((file) => !modified.has(file)).sort(),
    modifiedFiles: [...modified].sort(),
  };
}

function appendFileOperationContext(summary, details) {
  const sections = [];
  if (details.readFiles.length > 0) {
    sections.push(`<read-files>\n${details.readFiles.join("\n")}\n</read-files>`);
  }
  if (details.modifiedFiles.length > 0) {
    sections.push(`<modified-files>\n${details.modifiedFiles.join("\n")}\n</modified-files>`);
  }
  if (sections.length === 0) return summary;
  return `${summary.trimEnd()}\n\n${sections.join("\n\n")}`;
}

function appendHistoryRecoveryContext(summary, historyRecovery) {
  if (!historyRecovery) return summary;
  return `${summary.trimEnd()}\n\n<history-recovery>\n`
    + `Earlier messages were removed at a complete tool-transaction boundary because the model-required reasoning data was unavailable. `
    + `Removed provider-visible messages: ${historyRecovery.removedMessageCount}.\n`
    + `</history-recovery>`;
}

// ── 技能回顾（skill recall）────────────────────────────────────────────
// 技能正文一经 read 就随旧区一起被摘要掉，模型只剩 <read-files> 里的一个路径。
// 这里在压缩摘要尾部追加 <skill-recall> 段：扫描被摘要区里 `[Use skill: …]`
// 前缀与 read 工具对 SKILL.md 的调用，把技能正文（从磁盘现读、按预算截头）
// 重新贴回上下文，并从上一份摘要的 <skill-recall> 解析续传，跨多次压缩存活。
// 对应的系统提示读法约定在 core/agent.ts 的「技能使用纪律」段，改动须同步。

const SKILL_RECALL_MAX_CHARS_PER_SKILL = 6_000;
const SKILL_RECALL_MAX_CHARS_TOTAL = 24_000;
const SKILL_RECALL_TRUNCATION_MARKER =
  "\n[... skill instructions truncated for compaction; read the Path above for the full text]";
const SKILL_FILE_PATH_RE = /(?:^|[/\\])SKILL\.md$/;
const SKILL_INVOCATION_PREFIX_RE = /^\[Use skill: ([^\]]+)\]/gm;

interface SkillRecallEntry {
  name: string;
  path: string | null;
  content: string | null;
  note?: "unavailable" | "name-only" | "omitted-budget";
}

function userMessageText(message: any): string {
  if (message?.role !== "user") return "";
  if (typeof message.content === "string") return message.content;
  if (!Array.isArray(message.content)) return "";
  return message.content
    .filter((block: any) => block?.type === "text" && typeof block.text === "string")
    .map((block: any) => block.text)
    .join("\n");
}

function collectInvokedSkillNames(messages: any[]): string[] {
  const names: string[] = [];
  for (const message of Array.isArray(messages) ? messages : []) {
    const text = userMessageText(message);
    if (!text) continue;
    for (const match of text.matchAll(SKILL_INVOCATION_PREFIX_RE)) {
      const name = match[1].trim();
      if (name && !names.includes(name)) names.push(name);
    }
  }
  return names;
}

function collectSkillFileReads(messages: any[]): Array<{ name: string; path: string }> {
  const reads: Array<{ name: string; path: string }> = [];
  for (const message of Array.isArray(messages) ? messages : []) {
    if (message?.role !== "assistant" || !Array.isArray(message.content)) continue;
    for (const block of message.content) {
      if (block?.type !== "toolCall" || block.name !== "read") continue;
      const readPath = typeof block?.arguments?.path === "string" ? block.arguments.path : "";
      if (!SKILL_FILE_PATH_RE.test(readPath)) continue;
      if (reads.some((read) => read.path === readPath)) continue;
      reads.push({ name: path.basename(path.dirname(readPath)), path: readPath });
    }
  }
  return reads;
}

/** Parse `<skill-recall>` entries back out of a previous compaction summary. */
export function parseExistingSkillRecall(previousSummary: any): SkillRecallEntry[] {
  if (typeof previousSummary !== "string" || !previousSummary.includes("<skill-recall>")) return [];
  const entries: SkillRecallEntry[] = [];
  for (const blockMatch of previousSummary.matchAll(/<skill-recall>([\s\S]*?)(?:<\/skill-recall>|$)/g)) {
    for (const part of blockMatch[1].split(/^### Skill: /m).slice(1)) {
      const lines = part.split("\n");
      const name = lines[0].trim();
      if (!name) continue;
      const pathMatch = lines[1]?.match(/^Path: (.+)$/);
      const contentStart = pathMatch ? 2 : 1;
      const content = lines.slice(contentStart).join("\n").trim();
      entries.push({ name, path: pathMatch ? pathMatch[1].trim() : null, content: content || null });
    }
  }
  return entries;
}

/** Merge previous-recall entries with skills detected in the region being summarized. */
function mergeSkillRecallEntries(
  previous: SkillRecallEntry[],
  invokedNames: string[],
  fileReads: Array<{ name: string; path: string }>,
): SkillRecallEntry[] {
  const entries: SkillRecallEntry[] = [];
  const byPath = new Map<string, SkillRecallEntry>();
  const byName = new Map<string, SkillRecallEntry>();
  const push = (entry: SkillRecallEntry) => {
    if (byName.has(entry.name) && !entry.path) return;
    entries.push(entry);
    if (entry.path) byPath.set(entry.path, entry);
    byName.set(entry.name, entry);
  };
  for (const prev of previous) push({ name: prev.name, path: prev.path, content: prev.content });
  for (const read of fileReads) {
    if (byPath.has(read.path)) continue;
    const existing = byName.get(read.name);
    if (existing && !existing.path) {
      existing.path = read.path;
      if (existing.note === "name-only") delete existing.note;
      byPath.set(read.path, existing);
      continue;
    }
    push({ name: read.name, path: read.path, content: null });
  }
  for (const name of invokedNames) {
    if (byName.has(name)) continue;
    push({ name, path: null, content: null, note: "name-only" });
  }
  return entries;
}

/**
 * Fill in skill bodies from disk at compaction time. History copies may already
 * be truncated by the tool-result guards, so the file is re-read; a file that
 * vanished is recorded explicitly instead of silently dropped.
 */
async function hydrateSkillRecallContents(entries: SkillRecallEntry[]): Promise<SkillRecallEntry[]> {
  let totalBudget = SKILL_RECALL_MAX_CHARS_TOTAL;
  for (const entry of entries) {
    if (!entry.path) continue;
    if (typeof entry.content === "string") {
      totalBudget -= entry.content.length;
      continue;
    }
    if (totalBudget <= 0) {
      entry.note = "omitted-budget";
      continue;
    }
    try {
      const raw = await readFile(entry.path, "utf-8");
      const budget = Math.min(SKILL_RECALL_MAX_CHARS_PER_SKILL, totalBudget);
      entry.content = raw.length > budget
        ? raw.slice(0, Math.max(0, budget - SKILL_RECALL_TRUNCATION_MARKER.length)) + SKILL_RECALL_TRUNCATION_MARKER
        : raw;
      totalBudget -= entry.content.length;
    } catch {
      entry.content = null;
      entry.note = "unavailable";
    }
  }
  return entries;
}

async function buildSkillRecallEntries(preparation: any): Promise<SkillRecallEntry[]> {
  const summarizedRegion = [
    ...(Array.isArray(preparation?.messagesToSummarize) ? preparation.messagesToSummarize : []),
    ...(Array.isArray(preparation?.turnPrefixMessages) ? preparation.turnPrefixMessages : []),
  ];
  const entries = mergeSkillRecallEntries(
    parseExistingSkillRecall(preparation?.previousSummary),
    collectInvokedSkillNames(summarizedRegion),
    collectSkillFileReads(summarizedRegion),
  );
  if (entries.length === 0) return entries;
  return await hydrateSkillRecallContents(entries);
}

function renderSkillRecallSection(entries: SkillRecallEntry[]): string | null {
  if (!Array.isArray(entries) || entries.length === 0) return null;
  const lines = [
    "<skill-recall>",
    "The following skills were invoked earlier in this session. Their instructions were compacted away; "
    + "the excerpts below are authoritative — keep following them. Re-read the Path for the full text.",
  ];
  for (const entry of entries) {
    lines.push(`### Skill: ${entry.name}`);
    if (entry.path) lines.push(`Path: ${entry.path}`);
    if (typeof entry.content === "string" && entry.content.length > 0) {
      lines.push(entry.content);
    } else if (entry.note === "unavailable") {
      lines.push("(skill file could not be read at compaction time; re-load it from the skill catalog if still needed)");
    } else if (entry.note === "name-only") {
      lines.push("(invoked by name only, never read in the compacted history; load it from the skill catalog if still needed)");
    } else {
      lines.push("(content omitted to stay within the recall budget; read the Path above)");
    }
  }
  lines.push("</skill-recall>");
  return lines.join("\n");
}

function appendSkillRecallContext(summary: string, entries: SkillRecallEntry[]): string {
  const section = renderSkillRecallSection(entries);
  if (!section) return summary;
  return `${summary.trimEnd()}\n\n${section}`;
}

// ── 计划文件保护（plan-file）────────────────────────────────────────────
// 计划模式的唯一交付物是会话旁 plan 文件；它的 read 结果随旧区被摘要掉后，
// 模型就忘了计划内容。照 skill-recall 的同一模式：压缩时从磁盘现读正文，
// 追加 <plan-file> 段进摘要尾部。每次压缩都现读，跨压缩天然保鲜。
// 路径契约在 lib/plan-mode/plan-file.ts，三处消费方共用。

const PLAN_FILE_RECALL_MAX_CHARS = 8_000;
const PLAN_FILE_TRUNCATION_MARKER =
  "\n[... plan truncated for compaction; read the file for the full text]";

async function buildPlanFileSection(preparation: any): Promise<string | null> {
  const planPath = typeof preparation?.planFilePath === "string" ? preparation.planFilePath : "";
  if (!planPath) return null;
  let content: string;
  try {
    content = await readFile(planPath, "utf8");
  } catch {
    return null; // 没有计划文件 = 没有要保护的东西
  }
  if (!content.trim()) return null;
  const truncated = content.length > PLAN_FILE_RECALL_MAX_CHARS;
  const body = truncated ? content.slice(0, PLAN_FILE_RECALL_MAX_CHARS) : content;
  return [
    "<plan-file>",
    `The session plan file (${planPath}) is this session's plan-mode deliverable. Its current content, read fresh from disk at compaction time:`,
    body,
    ...(truncated ? [PLAN_FILE_TRUNCATION_MARKER.trim()] : []),
    "</plan-file>",
  ].join("\n");
}

function appendPlanFileContext(summary: string, section: string | null): string {
  if (!section) return summary;
  return `${summary.trimEnd()}\n\n${section}`;
}

// ── 上下文笔记保护（context-notes）──────────────────────────────────────
// 模型写给未来自己的便签（约束/决定/坐标）。压缩时从磁盘现读全文注入摘要
// 尾部，跨压缩存活——照 skill-recall / plan-file 的同一保护族。
async function buildContextNotesSection(preparation): Promise<string | null> {
  const notesPath = (preparation as any)?.contextNotesPath;
  if (typeof notesPath !== "string" || !notesPath) return null;
  let notes = "";
  try {
    notes = readContextNotes(notesPath);
  } catch {
    return null;
  }
  if (!notes.trim()) return null;
  if (Buffer.byteLength(notes, "utf8") > CONTEXT_NOTES_MAX_BYTES) {
    notes = Buffer.from(notes, "utf8").subarray(0, CONTEXT_NOTES_MAX_BYTES).toString("utf8") + CONTEXT_NOTES_TRUNCATION_MARKER;
  }
  return [
    "<context-notes>",
    "Your own session notes (survive compaction; keep following them, update via the context_notes tool):",
    notes,
    "</context-notes>",
  ].join("\n");
}

function appendContextNotesContext(summary: string, section: string | null): string {
  if (!section) return summary;
  return `${summary.trimEnd()}\n\n${section}`;
}

function cacheKeyParamsFromSnapshot(snapshot) {
  if (snapshot?.cacheKeyParams && typeof snapshot.cacheKeyParams === "object" && !Array.isArray(snapshot.cacheKeyParams)) {
    return snapshot.cacheKeyParams;
  }
  return null;
}

export function isStaleExtensionContextError(error) {
  const message = error instanceof Error ? error.message : String(error || "");
  return message.includes("This extension ctx is stale after session replacement or reload");
}

export const CACHE_PRESERVING_COMPACTION_EXTENSION_MISSING_MESSAGE =
  "Cache-preserving compaction extension is not installed for this session";

export function isMissingCachePreservingCompactionExtensionError(error) {
  const message = error instanceof Error ? error.message : String(error || "");
  return message.includes(CACHE_PRESERVING_COMPACTION_EXTENSION_MISSING_MESSAGE);
}

function isRecoverableCachePreservingCompactionRuntimeError(error) {
  return isStaleExtensionContextError(error)
    || isMissingCachePreservingCompactionExtensionError(error);
}

export function estimateCachePreservingCompactionRequest({
  preparation,
  messages = [],
  retainedMessageCount = 0,
  model,
  instruction = null,
  systemPrompt = "",
  tools = [],
  customInstructions,
}: {
  preparation?: any;
  messages?: any[];
  retainedMessageCount?: number;
  model?: any;
  instruction?: any;
  systemPrompt?: string;
  tools?: any[];
  customInstructions?: any;
} = {}) {
  const nativePreparation = preparation;
  preparation = stripInlineMediaFromCompactionPreparation(preparation);
  const systemPromptTokens = estimateTextTokens(systemPrompt);
  const liveMessages = Array.isArray(messages) ? messages : [];
  const cacheInstruction = instruction || buildCachePreservingCompactionInstruction({
    preparation,
    customInstructions,
    liveMessageCount: liveMessages.length,
    retainedMessageCount,
  });
  const cacheMessageTokens = liveMessages.reduce((sum, message) => sum + estimateTokens(message), 0);
  const cacheInstructionTokens = estimateTokens(cacheInstruction);
  const providerVisibleTools = normalizeProviderVisibleTools(tools);
  const toolSchemaTokens = providerVisibleTools.length > 0
    ? estimateTextTokens(stableSerialize(providerVisibleTools))
    : 0;
  const cacheMaxTokens = getCachePreservingCompactionMaxTokens(preparation);
  const cachePromptTokens = cacheMessageTokens
    + cacheInstructionTokens
    + systemPromptTokens
    + toolSchemaTokens
    + COMPACTION_REQUEST_BUFFER_TOKENS;
  const cachePreservingBudget = {
    kind: "cache-preserving",
    promptTokens: cachePromptTokens,
    maxTokens: cacheMaxTokens,
    totalTokens: cachePromptTokens + cacheMaxTokens,
    messageTokens: cacheMessageTokens,
    instructionTokens: cacheInstructionTokens,
    systemPromptTokens,
    toolSchemaTokens,
    bufferTokens: COMPACTION_REQUEST_BUFFER_TOKENS,
  };

  const nativeRequests = buildNativeCompactionRequestShapes({
    preparation: nativePreparation,
    model,
    customInstructions,
  }).requests.map((request) => {
    const messageTokens = request.messages.reduce(
      (sum, message) => sum + estimateTokens(message),
      0,
    );
    const systemPromptTokens = estimateTextTokens(request.systemPrompt);
    const promptTokens = messageTokens
      + systemPromptTokens
      + COMPACTION_REQUEST_BUFFER_TOKENS;
    return {
      kind: request.kind,
      promptTokens,
      maxTokens: request.maxTokens,
      totalTokens: promptTokens + request.maxTokens,
      messageTokens,
      instructionTokens: estimateTextTokens(request.promptText),
      systemPromptTokens,
      toolSchemaTokens: 0,
      bufferTokens: COMPACTION_REQUEST_BUFFER_TOKENS,
    };
  });
  const nativeSummaryBudget = nativeRequests.reduce((largest, current) => (
    !largest || current.totalTokens > largest.totalTokens ? current : largest
  ), null) || {
    kind: "history",
    promptTokens: COMPACTION_REQUEST_BUFFER_TOKENS,
    maxTokens: cacheMaxTokens,
    totalTokens: COMPACTION_REQUEST_BUFFER_TOKENS + cacheMaxTokens,
    messageTokens: 0,
    instructionTokens: 0,
    systemPromptTokens: 0,
    toolSchemaTokens: 0,
    bufferTokens: COMPACTION_REQUEST_BUFFER_TOKENS,
  };
  return {
    cachePreservingBudget,
    nativeSummaryBudget,
    nativeRequests,
  };
}

export function shouldHardTruncateCachePreservingCompaction({
  preparation,
  messages,
  retainedMessageCount,
  model,
  instruction,
  systemPrompt,
  tools,
  customInstructions,
  hardTruncateThreshold = DEFAULT_HARD_TRUNCATE_THRESHOLD,
}: {
  preparation?: any;
  messages?: any[];
  retainedMessageCount?: number;
  model?: any;
  instruction?: any;
  systemPrompt?: any;
  tools?: any[];
  customInstructions?: any;
  hardTruncateThreshold?: number;
} = {}) {
  const contextWindow = model?.contextWindow ?? 0;
  const budgets = estimateCachePreservingCompactionRequest({
    preparation,
    messages,
    retainedMessageCount,
    model,
    instruction,
    systemPrompt,
    tools,
    customInstructions,
  });
  const { cachePreservingBudget, nativeSummaryBudget } = budgets;
  if (contextWindow <= 0) {
    return {
      ...budgets,
      cachePreservingFits: false,
      nativeSummaryFits: false,
      shouldUseNativeFallback: false,
      shouldHardTruncate: true,
      threshold: 0,
      contextWindow,
    };
  }
  const threshold = Math.floor(contextWindow * hardTruncateThreshold);
  const cachePreservingFits = cachePreservingBudget.totalTokens <= threshold;
  const nativeSummaryFits = nativeSummaryBudget.totalTokens <= threshold;
  return {
    ...budgets,
    cachePreservingFits,
    nativeSummaryFits,
    shouldUseNativeFallback: !cachePreservingFits && nativeSummaryFits,
    shouldHardTruncate: !cachePreservingFits && !nativeSummaryFits,
    threshold,
    contextWindow,
  };
}

function hardTruncateCachePreservingCompaction(branchEntries, preparation, {
  reason = "cache-preserving-compaction-hard-truncate",
  summary = "[由于对话过长且压缩请求本身会超限，早期对话历史已被硬截断（hana-cache-preserving-compaction）]",
} = {}) {
  const keepRecentTokens = preparation?.settings?.keepRecentTokens ?? 20_000;
  return computeHardTruncation(branchEntries, keepRecentTokens, {
    summary,
    reason,
  });
}

function emitCompactionProgress(session, event) {
  session?._emit?.(event);
}

async function emitSessionCompactEvent(session, compactionEntryId, fromExtension) {
  const runner = session?.extensionRunner;
  if (!runner?.hasHandlers?.("session_compact")) return;
  const compactionEntry = session.sessionManager?.getEntry?.(compactionEntryId)
    || session.sessionManager?.getEntries?.()?.find((entry) => entry?.id === compactionEntryId);
  if (!compactionEntry) return;
  await runner.emit({
    type: "session_compact",
    compactionEntry,
    fromExtension,
  });
}

export async function appendCompactionResultToSession(session, result, {
  fromExtension = true,
  onCompacted,
}: {
  fromExtension?: boolean;
  onCompacted?: (session: any) => void;
} = {}) {
  const compactionEntryId = session.sessionManager.appendCompaction(
    result.summary,
    result.firstKeptEntryId,
    result.tokensBefore,
    result.details,
    fromExtension,
  );
  replaceSessionMessages(session);
  await emitSessionCompactEvent(session, compactionEntryId, fromExtension);
  onCompacted?.(session);
  return result;
}

export async function createCachePreservingCompactionResult({
  preparation,
  model,
  systemPrompt,
  messages,
  retainedMessageCount,
  instruction: preparedInstruction = null,
  messagesAreNormalized = false,
  tools = [],
  sessionSnapshot = null,
  cacheKeyParams = {},
  cacheMetadataOverride = null,
  customInstructions,
  signal,
  thinkingLevel,
  reasoningLevel,
  outputPolicy = COMPACTION_OUTPUT_POLICIES.PROVIDER_DEFAULT,
  streamFn,
  streamOptions = {},
  convertToLlm = convertAgentMessagesToLlm,
  usageLedger,
  usageContext,
  historyRecovery = null,
}: {
  preparation: any;
  model: any;
  systemPrompt: any;
  messages?: any[];
  retainedMessageCount: number;
  instruction?: any;
  messagesAreNormalized?: boolean;
  tools?: any[];
  sessionSnapshot?: any;
  cacheKeyParams?: Record<string, any>;
  cacheMetadataOverride?: any;
  customInstructions: any;
  signal: any;
  thinkingLevel: any;
  reasoningLevel?: string | null;
  outputPolicy?: "provider-default" | "bounded";
  streamFn: any;
  streamOptions?: Record<string, any>;
  convertToLlm?: any;
  usageLedger: any;
  usageContext: any;
  historyRecovery?: any;
}) {
  if (!preparation) throw new Error("Cache-preserving compaction requires preparation");
  if (!model) throw new Error("Cache-preserving compaction requires a model");
  if (!Array.isArray(messages)) {
    throw prefixContractError("full provider-visible live messages are unavailable");
  }
  preparation = stripInlineMediaFromCompactionPreparation(preparation);
  const seedCacheKeyParams = !cacheMetadataOverride
    ? (cacheKeyParamsFromSnapshot(sessionSnapshot) || cacheKeyParams)
    : cacheKeyParams;
  const resolvedOutputPolicy = normalizeCompactionOutputPolicy(outputPolicy);
  const rawThinkingLevel = seedCacheKeyParams.thinkingLevel ?? thinkingLevel ?? "off";
  const reasoningPolicy = resolveCompactionReasoningPolicy(rawThinkingLevel);
  const effectiveCacheKeyParams = {
    ...seedCacheKeyParams,
    thinkingLevel: reasoningPolicy.thinkingLevel,
  };
  const effectiveThinkingLevel = !cacheMetadataOverride
    ? reasoningPolicy.thinkingLevel
    : resolveCompactionReasoningPolicy(thinkingLevel).thinkingLevel;
  // A caller that already knows what the live requests on this session reason at
  // says so, and that answer wins: the compaction request has to match the body
  // the cached prefix was built from, not just its own thinking level.
  const effectiveReasoningLevel = reasoningLevel !== undefined
    ? reasoningLevel
    : resolveCompactionReasoningPolicy(effectiveThinkingLevel).reasoningLevel;
  const rawLlmMessages = messagesAreNormalized
    ? messages
    : await convertToLlm(messages);
  const llmMessages = messagesAreNormalized
    ? rawLlmMessages
    : normalizeProviderContextMessages(rawLlmMessages, model, {
        mode: "chat",
        reasoningLevel: effectiveReasoningLevel,
        reasoningReplay: effectiveCacheKeyParams.reasoningReplay,
      });
  if (!Array.isArray(llmMessages)) {
    throw prefixContractError("provider normalization did not return the full live message array");
  }
  const instruction = preparedInstruction || buildCachePreservingCompactionInstruction({
    preparation,
    customInstructions,
    liveMessageCount: llmMessages.length,
    retainedMessageCount,
  });
  const snapshotForRequest = sessionSnapshot || buildSessionCacheSnapshot({
    sessionPath: "",
    reason: "compaction.history",
    model,
    cacheKeyParams: effectiveCacheKeyParams,
    systemPrompt,
    tools,
    messages: llmMessages,
  });
  const requestTools = Array.isArray(tools) ? tools : [];
  let cacheMetadata;
  if (cacheMetadataOverride) {
    cacheMetadata = buildCacheStrategyMetadata(cacheMetadataOverride);
  } else {
    const requestContract = buildSessionSnapshotRequestContract({
      snapshot: snapshotForRequest,
      model,
      cacheKeyParams: effectiveCacheKeyParams,
      systemPrompt,
      tools: requestTools,
      messages: llmMessages,
      prefixMessageCount: llmMessages.length,
    });
    const assertion = assertSessionSnapshotRequest(snapshotForRequest, requestContract);
    if (!assertion.ok) {
      const fields = assertion.diffs.map((diff) => diff.field).join(", ");
      throw new Error(`Session snapshot request is not strict: ${fields}`);
    }
    cacheMetadata = buildCacheStrategyMetadata({
      cacheStrategy: CACHE_STRATEGIES.SESSION_SNAPSHOT,
      cacheGroup: "compaction.history",
      templateVersion: "agent-run.v1",
      cachePrefixHash: requestContract.cachePrefixHash,
      parentCachePrefixHash: snapshotForRequest.cachePrefixHash,
      strict: true,
    });
  }
  const options: Record<string, any> = {
    ...streamOptions,
  };
  delete options.maxTokens;
  delete options.reasoning;
  delete options.toolChoice;
  if (resolvedOutputPolicy === COMPACTION_OUTPUT_POLICIES.BOUNDED) {
    options.maxTokens = getCachePreservingCompactionMaxTokens(preparation);
  }
  if (effectiveReasoningLevel) options.reasoning = effectiveReasoningLevel;

  const runResult = await runCachePreservingCompactionAgentRun({
    liveMessages: llmMessages,
    systemPrompt,
    tools: requestTools,
    model,
    instruction,
    streamFn,
    streamOptions: options,
    convertToLlm: async (input: any[]) => input,
    signal,
    usageLedger,
    usageContext,
    cacheMetadata,
  });

  const details = {
    ...computeFileDetails(preparation.fileOps),
    ...(historyRecovery ? { historyRecovery } : {}),
  };
  const skillRecallEntries = await buildSkillRecallEntries(preparation);
  const planFileSection = await buildPlanFileSection(preparation);
  const contextNotesSection = await buildContextNotesSection(preparation);
  return {
    summary: appendContextNotesContext(
      appendPlanFileContext(
        appendHistoryRecoveryContext(
          appendSkillRecallContext(appendFileOperationContext(runResult.summary, details), skillRecallEntries),
          historyRecovery,
        ),
        planFileSection,
      ),
      contextNotesSection,
    ),
    firstKeptEntryId: preparation.firstKeptEntryId,
    tokensBefore: preparation.tokensBefore,
    details,
  };
}

export async function createColdUtilitySummaryResult({
  preparation,
  transcriptMessages,
  model,
  systemPrompt,
  customInstructions,
  signal,
  thinkingLevel,
  outputPolicy = COMPACTION_OUTPUT_POLICIES.PROVIDER_DEFAULT,
  streamFn,
  streamOptions = {},
  convertToLlm = convertAgentMessagesToLlm,
  usageLedger,
  usageContext,
}: {
  preparation: any;
  transcriptMessages: any[];
  model: any;
  systemPrompt: any;
  customInstructions?: any;
  signal?: any;
  thinkingLevel?: any;
  outputPolicy?: "provider-default" | "bounded";
  streamFn: any;
  streamOptions?: Record<string, any>;
  convertToLlm?: any;
  usageLedger?: any;
  usageContext?: any;
}) {
  if (!Array.isArray(transcriptMessages)) {
    throw new Error("Cold utility summary requires an explicit transcript");
  }
  return await createCachePreservingCompactionResult({
    preparation,
    model,
    systemPrompt,
    messages: transcriptMessages,
    retainedMessageCount: 0,
    tools: [],
    cacheMetadataOverride: {
      cacheStrategy: CACHE_STRATEGIES.UTILITY_TEMPLATE,
      cacheGroup: "compaction.deleted-agent-continuation",
      templateVersion: "cold-transcript.v1",
      cachePrefixHash: "",
      parentCachePrefixHash: "",
      strict: false,
    },
    customInstructions,
    signal,
    thinkingLevel,
    outputPolicy,
    streamFn,
    streamOptions,
    convertToLlm,
    usageLedger,
    usageContext,
  });
}

function replaceSessionMessages(session) {
  const context = session.sessionManager.buildSessionContext();
  if (session.agent?.replaceMessages) {
    session.agent.replaceMessages(context.messages);
  } else if (session.agent?.state) {
    session.agent.state.messages = context.messages;
  }
}

export async function runCachePreservingCompactionForSession(session: any, {
  settings,
  model = session?.model,
  customInstructions,
  signal,
  hardTruncateThreshold = DEFAULT_HARD_TRUNCATE_THRESHOLD,
  emitLifecycle = false,
  lifecycleReason = "manual",
  usageLedger,
  usageContext,
  onCompacted,
}: { settings?: any; model?: any; customInstructions?: any; signal?: any; hardTruncateThreshold?: number; emitLifecycle?: boolean; lifecycleReason?: string; usageLedger?: any; usageContext?: any; onCompacted?: (session: any) => void } = {}) {
  if (!session?.sessionManager) throw new Error("runCachePreservingCompactionForSession: missing session manager");
  if (!session?.agent) throw new Error("runCachePreservingCompactionForSession: missing agent");
  if (!model) throw new Error("runCachePreservingCompactionForSession: missing model");

  const compactionSettings = settings || session.settingsManager?.getCompactionSettings?.();
  if (!compactionSettings) throw new Error("runCachePreservingCompactionForSession: missing compaction settings");

  // One session, one compaction at a time. Two of them rewrite the same history
  // into two summaries, and the loser silently discards the winner's work. The
  // SDK's own compaction flags itself through isCompacting; this path flags
  // itself the same way so either one blocks the other. Queueing instead would
  // run the second compaction against history the first already replaced.
  if (session.isCompacting === true || session[DIRECT_COMPACTION_IN_PROGRESS] === true) {
    throw new Error("runCachePreservingCompactionForSession: compaction already in progress for this session");
  }
  session[DIRECT_COMPACTION_IN_PROGRESS] = true;

  try {
  const branchEntries = session.sessionManager.getBranch();
  if (emitLifecycle) {
    emitCompactionProgress(session, { type: "compaction_start", reason: lifecycleReason });
  }

  try {
    const preparation = prepareCompaction(branchEntries, compactionSettings);
    if (!preparation) {
      const lastEntry = branchEntries[branchEntries.length - 1];
      if (lastEntry?.type === "compaction") throw new Error("Already compacted");
      throw new Error("Nothing to compact (session too small)");
    }
    // 计划文件压缩保护：会话旁 plan.md 的正文在摘要尾部随 <plan-file> 段存活。
    // planFilePath 是本轮新增的旁挂字段，pi-sdk 的 CompactionPreparation 类型不含它，
    // 这里以 any 单点挂载（下游 createCachePreservingCompactionResult 本就把 preparation 当 any 读）。
    (preparation as any).planFilePath = planFilePathForSession(session.sessionManager.getSessionFile?.());
    // context-notes 同一旁挂模式：压缩时按会话文件现读笔记全文
    (preparation as any).contextNotesPath = session.sessionManager.getSessionFile?.() || null;

    const systemPrompt = session.agent.state?.systemPrompt ?? session.systemPrompt;
    const rawLiveMessages = session.sessionManager.buildSessionContext()?.messages;
    const convertToLlm = session.agent.convertToLlm || convertAgentMessagesToLlm;
    const thinkingLevel = session.thinkingLevel ?? session.agent.state?.thinkingLevel ?? "off";
    const reasoningPolicy = resolveCompactionReasoningPolicy(thinkingLevel);
    const prefix = await buildCachePreservingCompactionPrefix({
      liveMessages: rawLiveMessages,
      preparation,
      model,
      customInstructions,
      transformContext: session.agent.transformContext || (async (messages) => messages),
      convertToLlm,
      normalizeMessages: (messages) => normalizeProviderContextMessages(messages, model, {
        mode: "chat",
        reasoningLevel: reasoningPolicy.reasoningLevel,
      }),
      signal,
    });
    const cacheKeyParams = { thinkingLevel: reasoningPolicy.thinkingLevel };
    const providerMessages = prefix.messages;
    const tools = session.agent.state?.tools || [];
    const sessionSnapshot = buildSessionCacheSnapshot({
      sessionPath: session.sessionManager.getSessionFile?.() || "",
      reason: "compaction.history",
      model,
      cacheKeyParams,
      systemPrompt,
      tools,
      messages: providerMessages,
    });
    const fit = shouldHardTruncateCachePreservingCompaction({
      preparation,
      messages: providerMessages,
      retainedMessageCount: prefix.retainedMessageCount,
      model,
      instruction: prefix.instruction,
      systemPrompt,
      tools,
      customInstructions,
      hardTruncateThreshold,
    });
    if (fit.shouldHardTruncate) {
      const truncation = hardTruncateCachePreservingCompaction(branchEntries, preparation);
      if (!truncation) {
        throw new Error(
          `Cache-preserving and native compaction requests exceed the model window ` +
          `(A=${fit.cachePreservingBudget.totalTokens}, B=${fit.nativeSummaryBudget.totalTokens}, ` +
          `threshold=${fit.threshold}) and hard truncation is unavailable`
        );
      }
      const result = await appendCompactionResultToSession(session, truncation, { fromExtension: true, onCompacted });
      if (emitLifecycle) {
        emitCompactionProgress(session, {
          type: "compaction_end",
          reason: lifecycleReason,
          result,
          aborted: false,
          willRetry: false,
        });
      }
      return result;
    }
    if (fit.shouldUseNativeFallback) {
      throw new Error(
        `Cache-preserving compaction request exceeds the model window while Pi native compaction fits ` +
        `(A=${fit.cachePreservingBudget.totalTokens}, B=${fit.nativeSummaryBudget.totalTokens}, ` +
        `threshold=${fit.threshold})`
      );
    }

    const result = await createCachePreservingCompactionResult({
      preparation,
      model,
      systemPrompt,
      messages: providerMessages,
      retainedMessageCount: prefix.retainedMessageCount,
      instruction: prefix.instruction,
      messagesAreNormalized: true,
      customInstructions,
      signal,
      thinkingLevel: reasoningPolicy.thinkingLevel,
      tools,
      sessionSnapshot,
      cacheKeyParams,
      cacheMetadataOverride: prefix.historyRecovery
        ? buildCacheStrategyMetadata({
            cacheStrategy: CACHE_STRATEGIES.CACHE_RECOVERY,
            cacheGroup: "compaction.history",
            templateVersion: "v1",
            strict: false,
            degradeReason: "malformed_reasoning_history_trim",
          })
        : null,
      outputPolicy: COMPACTION_OUTPUT_POLICIES.PROVIDER_DEFAULT,
      streamFn: session.agent.streamFunction,
      streamOptions: {
        sessionId: session.agent.sessionId,
        // 标记 compaction 用途：session onPayload 扩展链（engine before_provider_request）
        // 读取该作用域，避免模型级联网/结构化输出开关注入压缩请求。
        onPayload: (payload, requestModel) => runWithProviderCompatPurpose(
          "compaction",
          () => session.agent.onPayload?.(payload, requestModel),
        ),
        onResponse: session.agent.onResponse,
        transport: session.agent.transport,
        thinkingBudgets: session.agent.thinkingBudgets,
        maxRetryDelayMs: session.agent.maxRetryDelayMs,
      },
      convertToLlm: async (input: any[]) => input,
      usageLedger,
      usageContext,
      historyRecovery: prefix.historyRecovery,
    });

    const saved = await appendCompactionResultToSession(session, result, { fromExtension: true, onCompacted });
    if (emitLifecycle) {
      emitCompactionProgress(session, {
        type: "compaction_end",
        reason: lifecycleReason,
        result: saved,
        aborted: false,
        willRetry: false,
      });
    }
    return saved;
  } catch (error) {
    if (emitLifecycle) {
      const message = error instanceof Error ? error.message : String(error);
      const aborted = signal?.aborted || message === "Compaction cancelled" || error?.name === "AbortError";
      emitCompactionProgress(session, {
        type: "compaction_end",
        reason: lifecycleReason,
        result: undefined,
        aborted,
        willRetry: false,
        errorMessage: aborted ? undefined : `Compaction failed: ${message}`,
      });
    }
    throw error;
  }
  } finally {
    delete session[DIRECT_COMPACTION_IN_PROGRESS];
  }
}

export async function runLossyLocalCompactionForSession(session: any, {
  settings,
  summarySource = null,
  getSummarySource,
  signal,
  emitLifecycle = false,
  lifecycleReason = "manual",
  onCompacted,
}: {
  settings?: any;
  summarySource?: any;
  getSummarySource?: ((session: any) => any | Promise<any>) | null;
  signal?: any;
  emitLifecycle?: boolean;
  lifecycleReason?: string;
  onCompacted?: (session: any) => void;
} = {}) {
  if (!session?.sessionManager) throw new Error("runLossyLocalCompactionForSession: missing session manager");
  if (!session?.agent) throw new Error("runLossyLocalCompactionForSession: missing agent");

  const compactionSettings = settings || session.settingsManager?.getCompactionSettings?.();
  if (!compactionSettings) throw new Error("runLossyLocalCompactionForSession: missing compaction settings");
  if (session.isCompacting === true || session[DIRECT_COMPACTION_IN_PROGRESS] === true) {
    throw new Error("runLossyLocalCompactionForSession: compaction already in progress for this session");
  }
  session[DIRECT_COMPACTION_IN_PROGRESS] = true;

  const branchEntries = session.sessionManager.getBranch();
  if (emitLifecycle) {
    emitCompactionProgress(session, {
      type: "compaction_start",
      reason: lifecycleReason,
      mode: INSTANT_SIMPLE_COMPACTION_RUNTIME_MODE,
    });
  }

  try {
    if (signal?.aborted) {
      const error: any = new Error("Compaction cancelled");
      error.name = "AbortError";
      throw error;
    }
    const preparation = prepareCompaction(branchEntries, compactionSettings);
    if (!preparation) {
      const lastEntry = branchEntries[branchEntries.length - 1];
      if (lastEntry?.type === "compaction") throw new Error("Already compacted");
      throw new Error("Nothing to compact (session too small)");
    }
    const resolvedSummarySource = typeof getSummarySource === "function"
      ? await getSummarySource(session)
      : summarySource;
    const result = createLossyLocalCompactionResult({
      branchEntries,
      preparation,
      summarySource: resolvedSummarySource,
    });
    const saved = await appendCompactionResultToSession(session, result, {
      fromExtension: true,
      onCompacted,
    });
    if (emitLifecycle) {
      emitCompactionProgress(session, {
        type: "compaction_end",
        reason: lifecycleReason,
        mode: INSTANT_SIMPLE_COMPACTION_RUNTIME_MODE,
        result: saved,
        aborted: false,
        willRetry: false,
      });
    }
    return saved;
  } catch (error) {
    if (emitLifecycle) {
      const message = error instanceof Error ? error.message : String(error);
      const aborted = signal?.aborted || message === "Compaction cancelled" || error?.name === "AbortError";
      emitCompactionProgress(session, {
        type: "compaction_end",
        reason: lifecycleReason,
        mode: INSTANT_SIMPLE_COMPACTION_RUNTIME_MODE,
        result: undefined,
        aborted,
        willRetry: false,
        errorMessage: aborted ? undefined : `Compaction failed: ${message}`,
      });
    }
    throw error;
  } finally {
    delete session[DIRECT_COMPACTION_IN_PROGRESS];
  }
}

export async function compactSessionWithCachePreservation(session, customInstructions) {
  session?.extensionRunner?.assertActive?.();
  if (!session?.extensionRunner?.hasHandlers?.("session_before_compact")) {
    throw new Error(CACHE_PRESERVING_COMPACTION_EXTENSION_MISSING_MESSAGE);
  }
  return await session.compact(customInstructions);
}

export async function compactSessionWithCachePreservationRecoveringRuntime({
  session,
  sessionPath,
  customInstructions,
  reloadSessionRuntime,
  onRuntimeReload,
}: any) {
  try {
    const result = await compactSessionWithCachePreservation(session, customInstructions);
    return { result, session, recovered: false };
  } catch (error) {
    if (!isRecoverableCachePreservingCompactionRuntimeError(error) || typeof reloadSessionRuntime !== "function") {
      throw error;
    }
    const reloadedSession = await reloadSessionRuntime(sessionPath);
    if (!reloadedSession) throw error;
    await onRuntimeReload?.({ error, session: reloadedSession });
    const result = await compactSessionWithCachePreservation(reloadedSession, customInstructions);
    return { result, session: reloadedSession, recovered: true };
  }
}
