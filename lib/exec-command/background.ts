/**
 * background.ts — exec_command 长任务自动转后台（阶段二·12）。
 *
 * wait_mode=auto：命令经 PTY 会话起跑（沿用 terminal-session-manager 的
 * 存活/transcript/沙盒通道，不另起进程机制），调用方在前台窗口内等待；
 * 窗口内退出 → 就地返回完整结果（体验等同前台）；到窗口仍在跑 →
 * 「转后台」——立即返回任务号，同时把任务登记进 DeferredResultStore
 * （完成自动回送并续跑一轮）与 TaskRegistry（check_pending_tasks 可见、
 * stop_task 可停）。输出复用 runner 的保头保尾截断。
 *
 * 诚实边界：转后台后结果走延迟结果链，到达时间=进程退出时间；中途会话
 * 被回档会把该任务的投递屏蔽（retry 事务既有语义）。
 */
import type { TerminalSessionManager } from "../terminal/terminal-session-manager.ts";
import { registerTaskExecution, type TaskExecution, type TaskRegistryClient } from "../tasks/task-execution.ts";
import { truncateHeadTail } from "./runner.ts";

export const EXEC_BACKGROUND_WINDOW_MS_DEFAULT = 60_000;
const POLL_MS = 200;

export interface TtyWaitOutcome {
  finished: boolean;
  exitCode: number | null;
  output: string;
}

/** 在前台窗口内轮询 PTY 会话；窗口内退出则带全部输出返回。 */
export async function waitForTtyWindow(
  manager: TerminalSessionManager,
  args: { sessionPath: string; terminalId: string; windowMs: number },
): Promise<TtyWaitOutcome> {
  const deadline = Date.now() + Math.max(1000, args.windowMs);
  for (;;) {
    const listResult = manager.list(args.sessionPath) as any;
    const terminals = Array.isArray(listResult) ? listResult : (listResult?.terminals || []);
    const entry = terminals.find((e: any) => e.terminalId === args.terminalId) || null;
    const status = entry?.status;
    if (status && status !== "running") {
      const tail = manager.readTail({ sessionPath: args.sessionPath, terminalId: args.terminalId }) as any;
      return { finished: true, exitCode: entry.exitCode ?? null, output: String(tail?.output ?? tail?.text ?? "") };
    }
    if (Date.now() >= deadline || !status) {
      const tail = manager.readTail({ sessionPath: args.sessionPath, terminalId: args.terminalId }) as any;
      return { finished: false, exitCode: null, output: String(tail?.output ?? tail?.text ?? "") };
    }
    await new Promise((r) => setTimeout(r, POLL_MS));
  }
}

export interface BackgroundExecDeps {
  manager: TerminalSessionManager;
  deferredStore: any;
  taskRegistry: TaskRegistryClient | null;
  /** 输出截断预算（与前台路径同一常量口径）。 */
  maxOutputTokens?: number;
  maxOutputChars?: number;
  log?: { warn?: (msg: string) => void };
}

/**
 * 转后台：登记 deferred + task registry，监听 terminal_exited →
 * deferred resolve（coordinator 自动回送并续跑）。返回登记结果。
 */
export function registerBackgroundExec(
  deps: BackgroundExecDeps,
  args: {
    terminalId: string;
    sessionPath: string;
    agentId: string | null;
    command: string;
  },
): { registered: boolean; reason?: string } {
  const { terminalId, sessionPath } = args;
  if (!deps.deferredStore?.defer || !deps.deferredStore?.resolve) {
    return { registered: false, reason: "deferred store unavailable" };
  }
  let execution: TaskExecution | undefined;
  try {
    deps.deferredStore.defer(terminalId, sessionPath, {
      type: "exec_command_background",
      command: args.command,
      deliveryIntent: "trigger_parent_turn",
    });
    // task registry：运行可见性 + stop_task 终止通道（abort handler=关会话）。
    deps.taskRegistry?.registerHandler?.("exec_command_background", {
      abort: async (taskId: string) => {
        try {
          // 同类型处理器由多会话共用，不能借用最后一次登记的父会话。
          const owner = deps.taskRegistry?.query?.(taskId);
          const ownerPath = owner?.parentSessionPath || (taskId === terminalId ? sessionPath : null);
          if (ownerPath) deps.manager.close({ sessionPath: ownerPath, terminalId: taskId });
        } catch { /* 已退出：无事 */ }
      },
    });
    execution = registerTaskExecution(deps.taskRegistry, terminalId, {
      type: "exec_command_background",
      parentSessionPath: sessionPath,
      agentId: args.agentId || undefined,
      meta: { command: args.command },
      persist: false,
    });
  } catch (err) {
    return { registered: false, reason: (err as any)?.message || String(err) };
  }

  // 监听退出（总线订阅一次性：用 manager 的 onExit 侧事件最稳是重查轮询；
  // 这里挂一个轻量定时轮询，进程退出即 resolve 并停表。）
  const timer = setInterval(() => {
    try {
      const listResult = deps.manager.list(sessionPath) as any;
      const terminals = Array.isArray(listResult) ? listResult : (listResult?.terminals || []);
      const entry = terminals.find((e: any) => e.terminalId === terminalId) || null;
      if (!entry) {
        clearInterval(timer);
        return;
      }
      if (entry.status && entry.status !== "running") {
        clearInterval(timer);
        const tail = deps.manager.readTail({ sessionPath, terminalId }) as any;
        const output = String(tail?.output ?? tail?.text ?? "");
        const exitCode = entry.exitCode ?? null;
        const truncated = truncateHeadTail(output, { maxBytes: 50_000 }) as any;
        const truncatedText = typeof truncated === "string" ? truncated : String(truncated?.content ?? output);
        try {
          const settled = execution?.complete({ exitCode });
          if (settled === null) return;
        } catch { /* registry 只管可见性 */ }
        try {
          deps.deferredStore.resolve(terminalId, {
            type: "exec_command_background",
            command: args.command,
            exitCode,
            ok: exitCode === 0,
            output: truncatedText,
          });
        } catch (err) {
          deps.log?.warn?.(`[exec-background] resolve failed for ${terminalId}: ${(err as any)?.message || err}`);
        }
      }
    } catch {
      clearInterval(timer);
    }
  }, 1000);
  // 会话进程退出兜底：30 分钟上限防泄漏
  setTimeout(() => clearInterval(timer), 30 * 60_000).unref?.();

  return { registered: true };
}

/** PTY 输出规范化（\r\n → \n），供前台窗口内完成的直接返回。 */
export function normalizeTtyOutput(text: string): string {
  return text.replace(/\r\n/g, "\n");
}
