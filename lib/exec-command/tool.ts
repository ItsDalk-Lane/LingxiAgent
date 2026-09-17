import { Type } from "../pi-sdk/index.ts";
import { getToolSessionPath } from "../tools/tool-session.ts";
import { execCommandDescription, writeStdinDescription } from "./guidance.ts";
import { classifyExecCommand } from "./policy.ts";
import {
  EXEC_COMMAND_DEFAULT_TIMEOUT_SECONDS,
  EXEC_COMMAND_MAX_TIMEOUT_SECONDS,
  EXEC_COMMAND_SANDBOX_PERMISSIONS,
  jsonResult,
  normalizeExecCommandParams,
  normalizeExecCommandSandboxPermissions,
  normalizeWriteStdinParams,
  textResult,
} from "./schema.ts";
import { runExecCommandDirect, runExecCommandOnce, startExecCommandTty } from "./runner.ts";
import {
  waitForTtyWindow,
  registerBackgroundExec,
  normalizeTtyOutput,
  EXEC_BACKGROUND_WINDOW_MS_DEFAULT,
} from "./background.ts";
import {
  WIN32_DEFAULT_ONE_SHOT_SHELL,
  renderCommandForExecShell,
  renderCommandWithWorkdir,
  resolveExecShell,
} from "./shell.ts";

const JUSTIFICATION_MAX_LENGTH = 300;

function truncateJustification(value: any) {
  const trimmed = typeof value === "string" ? value.trim() : "";
  return trimmed.length > JUSTIFICATION_MAX_LENGTH ? trimmed.slice(0, JUSTIFICATION_MAX_LENGTH) : trimmed;
}

export function createExecCommandTools({
  bashTool,
  escalatedBashTool,
  commandExec,
  escalatedCommandExec,
  getTerminalSessionManager,
  getAgentId,
  getCwd,
  isOneShotSandboxEnforced,
  platform = process.platform,
  env = process.env,
  // Called at most once, right here, when this tool-set is built for a
  // session. The result is baked into `description` below as a plain string
  // literal — not re-read later — so the description stays fixed for the
  // rest of this session's lifetime even if the underlying machine state
  // (e.g. pwsh getting installed or removed) changes afterward. See the
  // rationale on detectWin32PowerShellFlavor in
  // ../sandbox/win32-runtime-cache.ts for why that function itself must not
  // cache across separate tool-set builds.
  detectPowerShellFlavor,
  getDeferredStore,
  getTaskRegistry,}: any = {}) {
  const execCommandTool = {
    name: "exec_command",
    label: "Exec Command",
    description: execCommandDescription({ platform, powershellFlavor: detectPowerShellFlavor?.() ?? null }),
    sessionPermission: {
      sideEffect: { kind: "command", commandParam: "cmd" },
      describeSideEffect: (params: any = {}) => {
        const justification = truncateJustification(params.justification);
        return {
          kind: params.tty ? "interactive_command" : "command",
          command: params.cmd || params.command || "",
          ...(justification ? { justification } : {}),
        };
      },
      resolveInvocation: (params: any = {}) => {
        const command = typeof (params.cmd || params.command) === "string"
          ? (params.cmd || params.command).trim()
          : "";
        if (!command) return null;
        const sandboxPermissions = normalizeExecCommandSandboxPermissions(params.sandbox_permissions);
        if (!sandboxPermissions.ok) return null;
        const tty = params.tty === true;
        const sandboxed = !tty && isOneShotSandboxEnforced?.() === true;
        const requiresEscalated = sandboxPermissions.value
          === EXEC_COMMAND_SANDBOX_PERMISSIONS.REQUIRE_ESCALATED;
        const networkIsolated = sandboxed && platform !== "win32" && !requiresEscalated;
        const containedOneShot = sandboxed && networkIsolated && !requiresEscalated;
        const justification = truncateJustification(params.justification);
        return {
          action: "run",
          kind: containedOneShot ? "routine" : "review",
          capability: "exec_command.run",
          sideEffect: {
            kind: tty ? "interactive_command" : "command",
            command,
            sandboxed,
            sandboxPermissions: sandboxPermissions.value,
            networkAccess: networkIsolated ? "blocked" : "review_required",
            hostIpcAccess: containedOneShot ? "available" : "review_required",
            ...(justification ? { justification } : {}),
          },
        };
      },
    },
    parameters: Type.Object({
      cmd: Type.String({ description: "Command to execute in the session's default shell; see tool description for the platform default." }),
      description: Type.Optional(Type.String({ description: "Short description of what this command does, shown as the chat summary only. It does not replace cmd or the approval justification." })),
      workdir: Type.Optional(Type.String({ description: "Working directory. Defaults to the current session cwd." })),
      shell: Type.Optional(Type.String({ description: "Optional shell override: auto, powershell, pwsh, cmd, bash." })),
      tty: Type.Optional(Type.Boolean({ description: "Start an interactive PTY-backed process instead of a one-shot command." })),
      sandbox_permissions: Type.Optional(Type.Union([
        Type.Literal(EXEC_COMMAND_SANDBOX_PERMISSIONS.USE_DEFAULT),
        Type.Literal(EXEC_COMMAND_SANDBOX_PERMISSIONS.REQUIRE_ESCALATED),
      ], {
        description: "Use use_default for the normal contained command path. Use require_escalated only when the command needs reviewed network-capable execution.",
      })),
      justification: Type.Optional(Type.String({
        description: "One-sentence approval question shown to the user; required with require_escalated (e.g. \"Run a WMI read query to inspect the GPU driver?\").",
      })),
      yield_time_ms: Type.Optional(Type.Number({ description: "Requested initial wait budget in milliseconds. Recorded for scheduling; not a command timeout." })),
      max_output_tokens: Type.Optional(Type.Number({ description: "Approximate maximum output token budget returned by this call." })),
      timeout: Type.Optional(Type.Number({ description: `One-shot timeout in seconds. Defaults to ${EXEC_COMMAND_DEFAULT_TIMEOUT_SECONDS}; values above ${EXEC_COMMAND_MAX_TIMEOUT_SECONDS} are capped at ${EXEC_COMMAND_MAX_TIMEOUT_SECONDS}. For interactive or unbounded processes use tty=true instead.` })),
      wait_mode: Type.Optional(Type.Union([Type.Literal("wait"), Type.Literal("auto")], {
        description: "wait (default): synchronous one-shot. auto: run in a PTY session and wait up to background_after_seconds; if still running by then the task moves to the background — you immediately get a task id (check_pending_tasks / stop_task work) and the full result is delivered automatically when the process exits. Prefer auto for long builds/tests/installs.",
      })),
      background_after_seconds: Type.Optional(Type.Number({ description: "Foreground window for wait_mode=auto before handing off to the background (default 60)." })),
    }),
    execute: async (toolCallId: any, params: any = {}, signal: any, onUpdate: any, ctx: any) => {
      const normalized = normalizeExecCommandParams(params, ctx, {
        defaultCwd: getCwd?.() || process.cwd(),
      });
      if (!normalized.ok) return normalized.error;

      const value = normalized.value;
      const classification = classifyExecCommand(value.cmd, { platform });
      if (classification.unsupportedSyntax) {
        return textResult(
          `This command uses POSIX heredoc syntax, but Windows exec_command defaults to ${WIN32_DEFAULT_ONE_SHOT_SHELL.display}. Use ${WIN32_DEFAULT_ONE_SHOT_SHELL.display} syntax, python -c, or write a temporary script file instead.`,
          {
            errorCode: classification.errorCode,
            execCommand: {
              ok: false,
              cmd: value.cmd,
              ...(value.description ? { description: value.description } : {}),
              workdir: value.workdir,
              shell: WIN32_DEFAULT_ONE_SHOT_SHELL.family,
              platform,
              classification,
            },
          },
        );
      }

      const shell = resolveExecShell({ shell: value.shell, platform });
      const defaultCwd = ctx?.sessionManager?.getCwd?.() || getCwd?.() || process.cwd();
      const commandWithWorkdir = renderCommandWithWorkdir(value.cmd, shell, {
        workdir: value.workdir,
        defaultCwd,
        platform,
      });
      const renderedCommand = renderCommandForExecShell(commandWithWorkdir, shell, { platform });
      const execDetails = {
        cmd: value.cmd,
        ...(value.description ? { description: value.description } : {}),
        commandWithWorkdir,
        renderedCommand,
        workdir: value.workdir,
        shell: shell.label,
        shellFamily: shell.family,
        shellRequested: shell.requested,
        tty: value.tty,
        sandboxPermissions: value.sandboxPermissions,
        ...(value.justification ? { justification: value.justification } : {}),
        platform,
        classification,
        yieldTimeMs: value.yieldTimeMs,
        maxOutputTokens: value.maxOutputTokens,
        timeout: value.timeout,
        timeoutDefaulted: value.timeoutDefaulted,
        timeoutClamped: value.timeoutClamped,
      };

      // ── wait_mode=auto：PTY 起跑 + 前台窗口等待；窗口内完成=同步返回，
      // 到窗口仍在跑=转后台（任务号立即返回，完成经延迟结果链自动回送续跑）。
      if (value.waitMode === "auto" && !value.tty) {
        const manager = getTerminalSessionManager?.();
        const sessionPath = getToolSessionPath(ctx);
        if (!manager || !sessionPath) {
          // auto 不可用（无会话/无 PTY）：如实回落同步路径并注明
          const fallbackExec = value.sandboxPermissions === EXEC_COMMAND_SANDBOX_PERMISSIONS.REQUIRE_ESCALATED
            ? escalatedCommandExec || commandExec
            : commandExec;
          if (!fallbackExec) {
            return textResult("wait_mode=auto requires a terminal session or a command executor; neither is available here", {
              errorCode: "EXEC_COMMAND_AUTO_UNAVAILABLE",
              execCommand: { ...execDetails, ok: false, waitMode: "auto" },
            });
          }
          const fallback = await runExecCommandDirect({
            commandExec: fallbackExec,
            command: renderedCommand,
            workdir: value.workdir,
            timeout: value.timeout,
            timeoutDefaulted: value.timeoutDefaulted,
            signal,
            onUpdate,
            execDetails: { ...execDetails, waitMode: "auto-fallback-wait" },
            maxOutputTokens: value.maxOutputTokens,
            platform,
          });
          return fallback;
        }
        const ttyStart = await startExecCommandTty({
          toolCallId,
          manager,
          getAgentId,
          getCwd,
          command: renderedCommand,
          workdir: value.workdir,
          label: params.label || value.cmd.slice(0, 64),
          ctx,
          execDetails,
        });
        const terminalId = ttyStart?.details?.processId || ttyStart?.details?.terminalId || null;
        if (!terminalId) return ttyStart;
        const windowMs = Number.isFinite(params?.background_after_seconds) && Number(params.background_after_seconds) > 0
          ? Number(params.background_after_seconds) * 1000
          : EXEC_BACKGROUND_WINDOW_MS_DEFAULT;
        const outcome = await waitForTtyWindow(manager, { sessionPath, terminalId, windowMs });
        if (outcome.finished) {
          const output = normalizeTtyOutput(outcome.output);
          return textResult(
            output.trim()
              ? `${output.trim()}\n\n[exit ${outcome.exitCode ?? "?"}]`
              : `[no output, exit ${outcome.exitCode ?? "?"}]`,
            {
              waitMode: "auto",
              backgrounded: false,
              exitCode: outcome.exitCode,
              execCommand: { ...execDetails, ok: outcome.exitCode === 0, exitCode: outcome.exitCode, terminalId, transportError: false },
            },
          );
        }
        // 转后台：登记两套账本（deferred=回送续跑；registry=可见/可停）
        const registration = registerBackgroundExec(
          {
            manager,
            deferredStore: getDeferredStore?.() || null,
            taskRegistry: getTaskRegistry?.() || null,
          },
          { terminalId, sessionPath, agentId: getAgentId?.() || null, command: value.cmd },
        );
        const tailPreview = normalizeTtyOutput(outcome.output).trim().slice(-2000);
        return textResult(
          [
            `still running after ${Math.round(windowMs / 1000)}s — moved to background.`,
            `task_id: ${terminalId} (check_pending_tasks lists it; stop_task can stop it; the full result arrives automatically when it exits)`,
            ...(tailPreview ? ["", "recent output:", tailPreview] : []),
            ...(registration.registered ? [] : [`note: background delivery unavailable (${registration.reason}) — poll with check_pending_tasks or write_stdin`]),
          ].join("\n"),
          {
            waitMode: "auto",
            backgrounded: true,
            taskId: terminalId,
            deliveryRegistered: registration.registered,
            execCommand: { ...execDetails, ok: true, exitCode: null, terminalId, transportError: false },
          },
        );
      }

      if (value.tty) {
        return startExecCommandTty({
          toolCallId,
          manager: getTerminalSessionManager?.(),
          getAgentId,
          getCwd,
          command: renderedCommand,
          workdir: value.workdir,
          label: params.label || value.cmd.slice(0, 64),
          ctx,
          execDetails,
          cols: params.cols,
          rows: params.rows,
        });
      }

      const selectedCommandExec = value.sandboxPermissions
        === EXEC_COMMAND_SANDBOX_PERMISSIONS.REQUIRE_ESCALATED
        ? escalatedCommandExec || commandExec
        : commandExec;
      if (selectedCommandExec) {
        return runExecCommandDirect({
          commandExec: selectedCommandExec,
          command: renderedCommand,
          workdir: value.workdir,
          timeout: value.timeout,
          timeoutDefaulted: value.timeoutDefaulted,
          signal,
          onUpdate,
          execDetails,
          maxOutputTokens: value.maxOutputTokens,
          platform,
        });
      }

      const selectedBashTool = value.sandboxPermissions
        === EXEC_COMMAND_SANDBOX_PERMISSIONS.REQUIRE_ESCALATED
        ? escalatedBashTool || bashTool
        : bashTool;
      if (!selectedBashTool?.execute) {
        return textResult("exec_command runner unavailable", {
          errorCode: "EXEC_COMMAND_RUNNER_UNAVAILABLE",
          execCommand: execDetails,
        });
      }

      return runExecCommandOnce({
        bashTool: selectedBashTool,
        toolCallId,
        command: renderedCommand,
        timeout: value.timeout,
        timeoutDefaulted: value.timeoutDefaulted,
        signal,
        onUpdate,
        ctx,
        execDetails,
        maxOutputTokens: value.maxOutputTokens,
      });
    },
  };

  const writeStdinTool = {
    name: "write_stdin",
    label: "Write Stdin",
    description: writeStdinDescription(),
    sessionPermission: {
      sideEffect: { kind: "terminal_input" },
      describeSideEffect: (params: any = {}) => ({
        kind: "terminal_input",
        processId: params.process_id || params.processId || "",
      }),
      resolveInvocation: (params: any = {}) => {
        const processId = typeof (params.process_id || params.processId) === "string"
          ? (params.process_id || params.processId).trim()
          : "";
        if (!processId) return null;
        const isPoll = typeof params.chars !== "string" || params.chars.length === 0;
        return {
          action: isPoll ? "poll" : "write",
          kind: isPoll ? "read" : "review",
          capability: isPoll ? "write_stdin.poll" : "write_stdin.write",
          target: { type: "terminal_process", id: processId, label: processId },
        };
      },
    },
    parameters: Type.Object({
      process_id: Type.String({ description: "process_id returned by exec_command with tty=true." }),
      chars: Type.Optional(Type.String({ description: "Characters to write to stdin, including newline if needed." })),
    }),
    execute: async (_toolCallId: any, params: any = {}, _signal: any, _onUpdate: any, ctx: any) => {
      const normalized = normalizeWriteStdinParams(params);
      if (!normalized.ok) return normalized.error;
      const sessionPath = getToolSessionPath(ctx);
      if (!sessionPath) {
        return textResult("current session is required to write stdin", {
          errorCode: "WRITE_STDIN_SESSION_REQUIRED",
        });
      }
      const manager = getTerminalSessionManager?.();
      if (!manager) {
        return textResult("terminal manager unavailable", {
          errorCode: "WRITE_STDIN_TERMINAL_MANAGER_UNAVAILABLE",
        });
      }
      const value = normalized.value;
      return jsonResult(manager.write({
        sessionPath,
        terminalId: value.processId,
        chars: value.chars,
      }));
    },
  };

  void env;
  return [execCommandTool, writeStdinTool];
}
