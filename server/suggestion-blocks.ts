function text(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

export function buildAutomationSuggestionBlock({
  confirmId = "",
  suggestionId = "",
  suggestionShortCode = "",
  jobData,
  operation = "create",
  status = "pending",
}: {
  confirmId?: string;
  suggestionId?: string;
  suggestionShortCode?: string;
  jobData: Record<string, unknown>;
  operation?: "create" | "update";
  status?: "pending" | "approved" | "rejected";
}) {
  const executor = asRecord(jobData.executor);
  const agentId = text(jobData.actorAgentId) || text(executor.agentId);
  const prompt = text(jobData.prompt);
  const title = text(jobData.label) || prompt.slice(0, 50) || "Automation draft";
  return {
    type: "suggestion_card",
    kind: "automation_draft",
    ...(confirmId ? { confirmId } : {}),
    ...(suggestionId ? { suggestionId } : {}),
    ...(suggestionShortCode ? { suggestionShortCode } : {}),
    status,
    operation,
    title,
    description: prompt,
    target: agentId ? { type: "agent", id: agentId } : undefined,
    detail: {
      kind: "automation_draft",
      operation,
      jobData,
    },
    actions: [
      { id: "view", kind: "open" },
    ],
  };
}

/**
 * 踩坑自动沉淀的建议卡：确认后才经 learn_lesson 落盘核心写技能。
 * 卡片实时有效（不落盘），ConfirmStore 超时后 confirm 接口 404，前端据此刻画过期。
 */
export function buildAutolearnSuggestionBlock({
  confirmId = "",
  name = "",
  description = "",
  lesson = "",
  status = "pending",
}: {
  confirmId?: string;
  name?: string;
  description?: string;
  lesson?: string;
  status?: "pending" | "approved" | "rejected";
}) {
  return {
    type: "suggestion_card",
    kind: "autolearn_lesson",
    ...(confirmId ? { confirmId } : {}),
    status,
    title: text(name),
    description: text(description),
    detail: {
      kind: "autolearn_lesson",
      name: text(name),
      description: text(description),
      lesson: text(lesson),
    },
    actions: [],
  };
}
