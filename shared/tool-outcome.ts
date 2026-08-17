export type ToolOutcomeStatus = "succeeded" | "failed" | "unknown";

export type ToolOutcome = {
  status: ToolOutcomeStatus;
  success: boolean;
  error?: string;
  details?: {
    output?: string;
    outputDeferred?: unknown;
    execCommand?: Record<string, unknown>;
    skillInvocation?: {
      content: string;
      truncated?: boolean;
      deferred?: unknown;
    };
  };
};

export type ToolInvocationContext = {
  toolName?: unknown;
  args?: unknown;
};

type ToolResultLike = {
  role?: unknown;
  toolCallId?: unknown;
  toolName?: unknown;
  isError?: unknown;
  content?: unknown;
  details?: unknown;
};

const ERROR_TEXT_MAX_LENGTH = 240;
const EXEC_OUTPUT_MAX_LENGTH = 64 * 1024;
const SKILL_CONTENT_MAX_LENGTH = 64 * 1024;
const LEGACY_ERROR_CODE_RE = /^(?:TOOL_|STOP_TASK_|EXEC_COMMAND_|WRITE_STDIN_)/;

function recordOf(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function nonEmptyText(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed || null;
}

function shortText(value: string): string {
  if (value.length <= ERROR_TEXT_MAX_LENGTH) return value;
  return `${value.slice(0, ERROR_TEXT_MAX_LENGTH - 1)}…`;
}

function soleTextBlock(content: unknown): string | null {
  if (!Array.isArray(content) || content.length !== 1) return null;
  const block = recordOf(content[0]);
  if (block?.type !== "text") return null;
  return nonEmptyText(block.text);
}

function soleRawTextBlock(content: unknown): string | null {
  if (!Array.isArray(content) || content.length !== 1) return null;
  const block = recordOf(content[0]);
  return block?.type === "text" && typeof block.text === "string" && block.text.length > 0
    ? block.text
    : null;
}

function invocationPath(args: unknown): string | null {
  const record = recordOf(args);
  const value = nonEmptyText(record?.path) || nonEmptyText(record?.file_path);
  return value ? value.replace(/\\/g, "/") : null;
}

export function skillInvocationName(context: ToolInvocationContext): string | null {
  if (context.toolName !== "read") return null;
  const filePath = invocationPath(context.args);
  if (!filePath) return null;
  const parts = filePath.split("/").filter(Boolean);
  if (parts.at(-1) !== "SKILL.md") return null;
  return parts.at(-2) || "SKILL.md";
}

function projectedExecDetails(result: ToolResultLike): ToolOutcome['details'] | undefined {
  const details = recordOf(result.details);
  const execCommand = recordOf(details?.execCommand);
  if (!execCommand) return undefined;
  const safeExecCommand: Record<string, unknown> = {};
  for (const key of [
    'cmd',
    'commandWithWorkdir',
    'renderedCommand',
    'workdir',
    'shell',
    'tty',
    'ok',
    'exitCode',
    'terminalId',
    'processId',
  ]) {
    if (execCommand[key] !== undefined) safeExecCommand[key] = execCommand[key];
  }
  const output = execCommand.tty === true ? null : soleRawTextBlock(result.content);
  return {
    execCommand: safeExecCommand,
    ...(output ? { output: output.slice(-EXEC_OUTPUT_MAX_LENGTH) } : {}),
  };
}

function projectedSkillDetails(
  result: ToolResultLike,
  context: ToolInvocationContext | undefined,
): ToolOutcome['details'] | undefined {
  if (result.isError === true || !context || !skillInvocationName(context)) return undefined;
  const content = soleRawTextBlock(result.content);
  if (!content) return undefined;
  const truncated = content.length > SKILL_CONTENT_MAX_LENGTH;
  return {
    skillInvocation: {
      content: content.slice(0, SKILL_CONTENT_MAX_LENGTH),
      ...(truncated ? { truncated: true } : {}),
    },
  };
}

function projectedDetails(
  result: ToolResultLike,
  context: ToolInvocationContext | undefined,
): ToolOutcome['details'] | undefined {
  return projectedExecDetails(result) || projectedSkillDetails(result, context);
}

function resultErrorText(result: ToolResultLike): string | null {
  const details = recordOf(result.details);
  return nonEmptyText(details?.error) || soleTextBlock(result.content);
}

/**
 * Read-time compatibility for Hana results written before toolError carried
 * Pi's explicit isError bit. Keep this deliberately narrow: plugin-specific
 * details.error values are diagnostics unless they match Hana's old helper
 * shape exactly.
 */
export function isKnownLegacyLingxiToolFailure(result: ToolResultLike): boolean {
  const details = recordOf(result.details);
  const errorCode = nonEmptyText(details?.errorCode);
  if (errorCode && (LEGACY_ERROR_CODE_RE.test(errorCode) || errorCode === "mcp_unavailable")) {
    return true;
  }

  const confirmation = recordOf(details?.confirmation);
  if (confirmation?.status === "needs_user_approval_but_unavailable") return true;

  const execCommand = recordOf(details?.execCommand);
  if (execCommand?.ok === false) return true;

  const error = nonEmptyText(details?.error);
  const contentText = soleTextBlock(result.content);
  return !!error && error === contentText;
}

export function projectLiveToolResultOutcome(
  result: ToolResultLike,
  context?: ToolInvocationContext,
): ToolOutcome {
  const details = projectedDetails(result, context);
  if (result?.isError !== true) {
    return { status: "succeeded", success: true, ...(details ? { details } : {}) };
  }
  const error = resultErrorText(result);
  return {
    status: "failed",
    success: false,
    ...(error ? { error: shortText(error) } : {}),
    ...(details ? { details } : {}),
  };
}

export function projectToolResultOutcome(
  result: ToolResultLike,
  context?: ToolInvocationContext,
): ToolOutcome {
  if (result?.isError === true) return projectLiveToolResultOutcome(result, context);
  if (!isKnownLegacyLingxiToolFailure(result)) return projectLiveToolResultOutcome(result, context);
  const error = resultErrorText(result);
  const details = projectedExecDetails(result);
  return {
    status: "failed",
    success: false,
    ...(error ? { error: shortText(error) } : {}),
    ...(details ? { details } : {}),
  };
}

function toolCallContextById(messages: unknown[]): Map<string, ToolInvocationContext> {
  const contexts = new Map<string, ToolInvocationContext>();
  for (const message of messages) {
    const record = recordOf(message);
    if (record?.role !== "assistant" || !Array.isArray(record.content)) continue;
    for (const rawBlock of record.content) {
      const block = recordOf(rawBlock);
      if (!block || (block.type !== "toolCall" && block.type !== "tool_use")) continue;
      const id = nonEmptyText(block.id);
      const toolName = nonEmptyText(block.name);
      if (!id || !toolName) continue;
      const args = recordOf(block.input) || recordOf(block.arguments) || recordOf(block.args) || undefined;
      contexts.set(id, { toolName, ...(args ? { args } : {}) });
    }
  }
  return contexts;
}

export function collectToolOutcomesByCallId(messages: unknown): Map<string, ToolOutcome> {
  const outcomes = new Map<string, ToolOutcome>();
  if (!Array.isArray(messages)) return outcomes;
  const contexts = toolCallContextById(messages);
  for (const message of messages) {
    const result = recordOf(message) as ToolResultLike | null;
    if (!result || result.role !== "toolResult") continue;
    const toolCallId = nonEmptyText(result.toolCallId);
    if (!toolCallId) continue;
    const paired = contexts.get(toolCallId);
    const context = paired || {
      toolName: result.toolName,
    };
    outcomes.set(toolCallId, projectToolResultOutcome(result, context));
  }
  return outcomes;
}

export function projectKnownLegacyToolFailures(messages: unknown): unknown {
  if (!Array.isArray(messages)) return messages;
  let changed = false;
  const projected = messages.map((message) => {
    const result = recordOf(message) as ToolResultLike | null;
    if (
      !result
      || result.role !== "toolResult"
      || result.isError === true
      || !isKnownLegacyLingxiToolFailure(result)
    ) {
      return message;
    }
    changed = true;
    return { ...result, isError: true };
  });
  return changed ? projected : messages;
}
