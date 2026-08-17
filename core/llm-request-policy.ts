const VALID_CALL_PURPOSES = new Set([
  "auxiliary_vision",
  "utility",
  "health_check",
  "summary",
  "chat",
]);

function normalizeCallPurpose(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const normalized = value.trim().toLowerCase();
  return VALID_CALL_PURPOSES.has(normalized) ? normalized : null;
}

export function buildProviderCompatOptions({
  mode = "utility",
  callPurpose,
  purpose,
  explicitMaxTokens,
  outputBudgetSource,
}: {
  mode?: string;
  callPurpose?: unknown;
  purpose?: unknown;
  explicitMaxTokens?: number | null;
  outputBudgetSource?: unknown;
} = {}) {
  const normalizedPurpose = normalizeCallPurpose(callPurpose);
  // callText 是非流式内部调用路径：默认 purpose=utility，模型级
  // 联网/结构化输出开关不作用于 title/summary/memory/approval 等内部用途。
  const normalizedCompatPurpose = purpose === "chat" || purpose === "compaction" ? purpose : "utility";
  return {
    mode,
    purpose: normalizedCompatPurpose,
    ...(mode === "utility" ? { reasoningLevel: "off" } : {}),
    ...(normalizedPurpose ? { callPurpose: normalizedPurpose } : {}),
    ...(explicitMaxTokens !== null && explicitMaxTokens !== undefined ? { outputBudgetSource } : {}),
  };
}
