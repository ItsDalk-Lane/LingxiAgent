/**
 * Agent 工具跑飞守卫。
 *
 * 状态只存在当前扩展实例内：连续同参调用、同一文件片段重读和同工具失败
 * 分别计数。提醒通过 tool_result 前置文本送给模型，触顶后由 tool_call 阻断。
 * 同一扩展还扫描非知识工具的文本输出，只加警告，不改写原内容。
 */

import { createModuleLogger } from "../debug-log.ts";
import { buildWarningLine, scan as scanInjection } from "../security/injection-scan.ts";

const log = createModuleLogger("agent-loop-guard");

interface ConsecutiveState {
  signature: string | null;
  count: number;
}

function stableSerialize(value: unknown, seen = new WeakSet<object>()): string {
  if (value === null) return "null";
  if (typeof value === "string" || typeof value === "boolean") return JSON.stringify(value);
  if (typeof value === "number") return Number.isFinite(value) ? JSON.stringify(value) : JSON.stringify(String(value));
  if (typeof value === "bigint") return JSON.stringify(`${value.toString()}n`);
  if (typeof value === "undefined") return "undefined";
  if (typeof value !== "object") return JSON.stringify(String(value));
  if (seen.has(value)) return '"[Circular]"';
  seen.add(value);
  try {
    if (Array.isArray(value)) {
      return `[${value.map(item => stableSerialize(item, seen)).join(",")}]`;
    }
    const record = value as Record<string, unknown>;
    return `{${Object.keys(record).sort().map(key => (
      `${JSON.stringify(key)}:${stableSerialize(record[key], seen)}`
    )).join(",")}}`;
  } finally {
    seen.delete(value);
  }
}

function incrementConsecutive(state: ConsecutiveState, signature: string): number {
  if (state.signature === signature) {
    state.count += 1;
  } else {
    state.signature = signature;
    state.count = 1;
  }
  return state.count;
}

function resetConsecutive(state: ConsecutiveState): void {
  state.signature = null;
  state.count = 0;
}

function readSignature(input: Record<string, unknown>): string {
  return stableSerialize({
    path: input?.path ?? null,
    offset: input?.offset ?? null,
    limit: input?.limit ?? null,
  });
}

function repetitionWarning(kind: "tool" | "read", count: number): string {
  const subject = kind === "read" ? "the same file range" : "this exact tool call";
  const action = count >= 5
    ? "Stop repeating it and change the approach or arguments now; continued repetition will be blocked at the seventh call."
    : "Inspect the result and change the approach or arguments before retrying.";
  return `⚠ Agent loop guard: ${subject} has been requested ${count} consecutive times. ${action}`;
}

function failureWarning(toolName: string, count: number): string {
  const action = count >= 5
    ? "The next call to this tool will be blocked until another tool succeeds."
    : "Inspect the error and change the approach before retrying.";
  return `⚠ Agent loop guard: tool "${toolName}" has failed ${count} consecutive times. ${action}`;
}

function prependWarnings(content: any[], warnings: string[]): { content: any[] } | undefined {
  if (warnings.length === 0) return undefined;
  return {
    content: [
      { type: "text", text: warnings.join("\n") },
      ...content,
    ],
  };
}

export function createAgentLoopGuardExtension() {
  return function (pi: any) {
    const repeated: ConsecutiveState = { signature: null, count: 0 };
    const reread: ConsecutiveState = { signature: null, count: 0 };
    const pendingWarnings = new Map<string, string[]>();
    let failedTool: string | null = null;
    let failedCount = 0;

    const resetState = (): void => {
      resetConsecutive(repeated);
      resetConsecutive(reread);
      pendingWarnings.clear();
      failedTool = null;
      failedCount = 0;
    };

    pi.on("session_start", () => {
      resetState();
    });

    // 用户的新提问应有独立的重试机会，不继承上一轮的重复或失败计数。
    pi.on("agent_start", () => {
      resetState();
    });

    pi.on("session_shutdown", () => {
      resetState();
    });

    pi.on("tool_call", (event: any) => {
      try {
        const toolName = typeof event?.toolName === "string" ? event.toolName : "unknown";
        const toolCallId = typeof event?.toolCallId === "string" ? event.toolCallId : "";
        const input = event?.input && typeof event.input === "object" ? event.input : {};

        if (failedTool === toolName && failedCount >= 5) {
          log.warn(`blocked after consecutive failures: tool=${toolName} count=${failedCount}`);
          return {
            block: true,
            reason: `Agent loop guard blocked tool "${toolName}" after ${failedCount} consecutive failures. Use a different tool or approach before retrying.`,
          };
        }

        let count: number;
        let kind: "tool" | "read";
        if (toolName === "read") {
          resetConsecutive(repeated);
          count = incrementConsecutive(reread, readSignature(input));
          kind = "read";
        } else {
          resetConsecutive(reread);
          count = incrementConsecutive(repeated, `${toolName}\u0000${stableSerialize(input)}`);
          kind = "tool";
        }

        if (count >= 7) {
          log.warn(`blocked repeated call: tool=${toolName} count=${count} kind=${kind}`);
          return {
            block: true,
            reason: kind === "read"
              ? "Agent loop guard blocked the seventh consecutive read of the same file range. Change path, offset, or limit before retrying."
              : `Agent loop guard blocked the seventh consecutive identical call to tool "${toolName}". Change the approach or arguments before retrying.`,
          };
        }
        if ((count === 3 || count === 5) && toolCallId) {
          pendingWarnings.set(toolCallId, [repetitionWarning(kind, count)]);
        }
        return undefined;
      } catch {
        // 事件载荷可能来自外部工具；日志只记固定诊断，不回显正文或参数。
        log.warn("tool_call guard error; allowing the call without logging event data");
        return undefined;
      }
    });

    pi.on("tool_result", (event: any) => {
      try {
        const toolName = typeof event?.toolName === "string" ? event.toolName : "unknown";
        const toolCallId = typeof event?.toolCallId === "string" ? event.toolCallId : "";
        const content = Array.isArray(event?.content) ? event.content : [];
        const warnings = toolCallId ? [...(pendingWarnings.get(toolCallId) ?? [])] : [];
        if (toolCallId) pendingWarnings.delete(toolCallId);

        if (event?.isError === true) {
          if (failedTool === toolName) {
            failedCount += 1;
          } else {
            failedTool = toolName;
            failedCount = 1;
          }
          if (failedCount === 2 || failedCount === 5) {
            warnings.push(failureWarning(toolName, failedCount));
          }
        } else {
          failedTool = null;
          failedCount = 0;
        }

        if (!toolName.startsWith("knowledge_")) {
          const text = content
            .filter(block => block?.type === "text" && typeof block.text === "string")
            .map(block => block.text)
            .join("\n");
          if (text) {
            const result = scanInjection(text);
            const warning = buildWarningLine(result.decision);
            if (warning) warnings.unshift(warning);
          }
        }

        return prependWarnings(content, warnings);
      } catch {
        // 事件载荷可能来自外部工具；日志只记固定诊断，不回显正文或参数。
        log.warn("tool_result guard error; preserving the original result without logging event data");
        return undefined;
      }
    });
  };
}
