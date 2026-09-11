import { isSyntheticToolPresentation, safeToolArguments } from './tool-presentation.ts';

/**
 * Tool invocation argument summary shared by live WS events and history hydration.
 *
 * Keep this list intentionally small: these values are rendered in chat UI, so
 * large/sensitive payloads such as file contents must stay out of the summary.
 */
export const TOOL_ARG_SUMMARY_KEYS = [
  "file_path",
  "path",
  "command",
  "cmd",
  "chars",
  "process_id",
  "pattern",
  "url",
  "query",
  "key",
  "value",
  "action",
  "type",
  "schedule",
  "prompt",
  "label",
  "description",
  "offset",
  "limit",
  "glob",
  "ignoreCase",
  "literal",
  "context",
  "fileId",
  "mountId",
  "resourceId",
] as const;

export type ToolArgSummaryKey = typeof TOOL_ARG_SUMMARY_KEYS[number];
export type ToolArgSummary = Partial<Record<ToolArgSummaryKey, unknown>>;

export function summarizeToolArgs(rawArgs: unknown, toolName?: unknown): ToolArgSummary | undefined {
  if (isSyntheticToolPresentation(toolName)) return undefined;
  if (!rawArgs || typeof rawArgs !== "object" || Array.isArray(rawArgs)) return undefined;
  const record = safeToolArguments(rawArgs) as Record<string, unknown>;
  const args: ToolArgSummary = {};
  for (const key of TOOL_ARG_SUMMARY_KEYS) {
    if (record[key] !== undefined) {
      const value = record[key];
      args[key] = typeof value === "string" && value.length > 2048 ? value.slice(0, 2047) + "…" : value;
    }
  }
  return Object.keys(args).length ? args : undefined;
}
