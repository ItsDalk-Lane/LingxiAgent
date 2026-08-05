/**
 * Internal one-shot release probe bundled into the full Server entry.
 * It exercises the same exec_command and Windows sandbox modules as a real
 * server build without starting HTTP, WebSocket, schedulers, or an LLM turn.
 */
import path from "path";

import { createExecCommandTools } from "../lib/exec-command/tool.ts";
import { deriveSandboxPolicy } from "../lib/sandbox/policy.ts";
import { createWin32Exec } from "../lib/sandbox/win32-exec.ts";
import { resolveWin32SandboxHelper } from "../lib/sandbox/win32-sandbox-helper.ts";

function requiredEnv(env: Record<string, string | undefined>, name: string) {
  const value = typeof env[name] === "string" ? env[name].trim() : "";
  if (!value) throw new Error(`[standalone-exec-smoke] ${name} is required`);
  return value;
}

function normalizeWindowsPath(value: string) {
  return path.win32.resolve(value).toLowerCase();
}

function firstText(result: any) {
  return result?.content?.find?.((item: any) => item?.type === "text")?.text || "";
}

function assertExecResult(result: any, label: string, expectedText: string) {
  if (result?.details?.execCommand?.ok !== true) {
    throw new Error(`[standalone-exec-smoke] ${label} failed\n${firstText(result)}`);
  }
  const output = firstText(result);
  if (!output.includes(expectedText)) {
    throw new Error(
      `[standalone-exec-smoke] ${label} did not emit ${JSON.stringify(expectedText)}\n${output}`,
    );
  }
  return {
    shell: result.details.execCommand.shell,
    exitCode: result.details.execCommand.exitCode,
    output: output.trim(),
  };
}

export async function runPackagedStandaloneRuntimeSmoke({
  env = process.env,
}: {
  env?: NodeJS.ProcessEnv | Record<string, string | undefined>;
} = {}) {
  if (process.platform !== "win32") {
    throw new Error("[standalone-exec-smoke] this probe requires Windows");
  }
  const workDir = requiredEnv(env, "HANA_STANDALONE_EXEC_WORK");
  const hanakoHome = requiredEnv(env, "HANA_HOME");
  const expectedHelper = requiredEnv(env, "HANA_STANDALONE_EXPECTED_HELPER");
  const expectedRoot = requiredEnv(env, "HANA_STANDALONE_EXPECTED_ROOT");
  const expectedEntry = path.win32.join(expectedRoot, "bundle", "index.js");
  const resolvedHelper = resolveWin32SandboxHelper({ env });

  if (!env.HANA_ROOT || normalizeWindowsPath(env.HANA_ROOT) !== normalizeWindowsPath(expectedRoot)) {
    throw new Error(`[standalone-exec-smoke] hana-server.cmd did not set HANA_ROOT: ${String(env.HANA_ROOT || "")}`);
  }
  if (!env.HANA_SERVER_ENTRY || normalizeWindowsPath(env.HANA_SERVER_ENTRY) !== normalizeWindowsPath(expectedEntry)) {
    throw new Error(
      `[standalone-exec-smoke] hana-server.cmd did not set HANA_SERVER_ENTRY: ${String(env.HANA_SERVER_ENTRY || "")}`,
    );
  }
  if (
    !env.HANA_WIN32_SANDBOX_HELPER
    || normalizeWindowsPath(env.HANA_WIN32_SANDBOX_HELPER) !== normalizeWindowsPath(expectedHelper)
  ) {
    throw new Error(
      "[standalone-exec-smoke] hana-server.cmd did not set HANA_WIN32_SANDBOX_HELPER: "
        + String(env.HANA_WIN32_SANDBOX_HELPER || ""),
    );
  }
  if (!resolvedHelper || normalizeWindowsPath(resolvedHelper) !== normalizeWindowsPath(expectedHelper)) {
    throw new Error(
      `[standalone-exec-smoke] packaged helper resolved to ${String(resolvedHelper)}; expected ${expectedHelper}`,
    );
  }

  const agentDir = path.join(hanakoHome, "agents", "standalone-smoke");
  const policy = deriveSandboxPolicy({
    agentDir,
    cwd: workDir,
    workspace: workDir,
    workspaceFolders: [],
    hanakoHome,
    mode: "standard",
  });
  const commandExec = createWin32Exec({ sandbox: { policy, hanakoHome } });
  const execCommand = createExecCommandTools({
    commandExec,
    getCwd: () => workDir,
    isOneShotSandboxEnforced: () => true,
    platform: "win32",
    env,
  }).find((tool: any) => tool.name === "exec_command");
  if (!execCommand) throw new Error("[standalone-exec-smoke] exec_command tool is unavailable");

  const ctx = { sessionManager: { getCwd: () => workDir } };
  const invoke = (id: string, params: Record<string, any>) => execCommand.execute(id, {
    workdir: workDir,
    sandbox_permissions: "use_default",
    timeout: 30,
    max_output_tokens: 2000,
    ...params,
  }, undefined, undefined, ctx);

  const git = assertExecResult(
    await invoke("standalone-git", { cmd: "git --version" }),
    "exec_command Git route",
    "git version ",
  );
  const cmd = assertExecResult(
    await invoke("standalone-cmd", {
      cmd: "echo HANA_EXEC_COMMAND_OK",
    }),
    "exec_command default cmd route",
    "HANA_EXEC_COMMAND_OK",
  );

  const receipt = {
    ok: true,
    hanaRoot: env.HANA_ROOT,
    helper: resolvedHelper,
    git,
    cmd,
  };
  process.stdout.write(`HANA_STANDALONE_EXEC_RECEIPT=${JSON.stringify(receipt)}\n`);
  return receipt;
}
