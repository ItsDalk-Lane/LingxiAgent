#!/usr/bin/env node

import path from "path";
import { fileURLToPath } from "url";
import { parseCliArgs, helpText } from "./args.ts";
import { resolveCliLingxiHome, resolveConnection } from "./local-server.ts";
import { LingxiCliClient } from "./client.ts";
import { printSessions, printStatus, startChat } from "./chat.ts";
import { spawnRustServerForeground, spawnServerForeground, startLocalServerAndWait } from "./server-runner.ts";
import { explicitRustConnection, readRustLocalService, RustCliClient, safeRustTerminalText } from "./rust-service.ts";
import { runBundlePull, runBundleStatus } from "./bundle.ts";
import { runDataDiagnose, runDataCheckpoints, runDataRestore } from "./data.ts";
import { ansi } from "./terminal-theme.ts";
import { printRustStatus, startRustChat } from "./rust-chat.ts";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PROJECT_ROOT = path.resolve(__dirname, "..");

export async function main(argv = process.argv.slice(2)) {
  let args;
  try {
    args = parseCliArgs(argv);
  } catch (err) {
    console.error(`${ansi.red}${err.message}${ansi.reset}`);
    console.log(helpText());
    return 1;
  }

  if (args.command === "help") {
    if (args.error) console.error(`${ansi.yellow}${args.error}${ansi.reset}\n`);
    console.log(helpText());
    return args.error ? 1 : 0;
  }

  if (args.command === "serve") {
    if (args.runtime === "rust") {
      try {
        if (args.url || args.token) throw new Error("serve does not accept --url or --token");
        return await spawnRustServerForeground({
          projectRoot: PROJECT_ROOT,
          extraArgs: args.passthrough,
          channel: args.channel,
          allowDataDowngrade: args.allowDataDowngrade,
        });
      } catch (err) {
        console.error(`${ansi.red}${safeRustTerminalText(err instanceof Error ? err.message : err)}${ansi.reset}`);
        return 1;
      }
    }
    await spawnServerForeground({
      projectRoot: PROJECT_ROOT,
      extraArgs: args.passthrough,
      channel: args.channel,
      allowDataDowngrade: args.allowDataDowngrade,
    });
    return 0;
  }

  if (args.command === "bundle") {
    // Pure local + network operation against the release shelf — never
    // needs (or starts) a running server, so it skips resolveConnection.
    if (args.subcommand === "pull") {
      return await runBundlePull({ channel: args.channel });
    }
    return await runBundleStatus({ channel: args.channel });
  }

  if (args.command === "data") {
    // Local filesystem maintenance surface for the data-epoch safety chain
    // — never talks to a running server, so it also skips resolveConnection.
    if (args.subcommand === "diagnose") {
      return await runDataDiagnose();
    }
    if (args.subcommand === "checkpoints") {
      return await runDataCheckpoints();
    }
    return await runDataRestore({ transitionId: args.target, confirmToken: args.confirmToken });
  }

  if (args.runtime === "rust") {
    const connection = args.url
      ? explicitRustConnection(args.url, args.token || "")
      : readRustLocalService({ lingxiHome: resolveCliLingxiHome() });
    if (connection.ok === false) {
      console.error(`${ansi.red}${safeRustTerminalText(connection.message)}${ansi.reset}`);
      // 断点 11：无实例时明确失败并给出启动路径，不静默回退 Node、不自动启动。
      if (!args.url) console.error(`${ansi.dim}Start one with: hana serve${ansi.reset}`);
      return 1;
    }
    const client = new RustCliClient(connection);
    try {
      if (args.command === "status") {
        return await printRustStatus(client);
      }
      if (args.command === "sessions") {
        const sessions = await client.sessions();
        if (sessions.length === 0) {
          console.log("No sessions yet.");
        } else {
          for (const [index, session] of sessions.slice(0, 20).entries()) {
            console.log(`${String(index + 1).padStart(2, " ")}. ${safeRustTerminalText(session.title, 72)} · ${safeRustTerminalText(session.agentId || "Agent", 72)}`);
          }
        }
        return 0;
      }
      if (args.command === "chat" || args.command === "continue") {
        return await startRustChat(client, { target: args.target || undefined, session: args.session || undefined, plain: args.plain });
      }
    } catch (err) {
      console.error(`${ansi.red}${safeRustTerminalText(err instanceof Error ? err.message : err)}${ansi.reset}`);
      return 1;
    }
  }

  let connection: any = resolveConnection({ url: args.url, token: args.token });
  if (!connection.ok && shouldAutoStartServer(args)) {
    console.error(`${ansi.dim}Starting local LingxiAgent Server...${ansi.reset}`);
    connection = await startLocalServerAndWait({ projectRoot: PROJECT_ROOT });
  }
  if (!connection.ok) {
    console.error(`${ansi.red}${connection.message}${ansi.reset}`);
    console.error(`${ansi.dim}Start one with: hana serve${ansi.reset}`);
    return 1;
  }

  const client = new LingxiCliClient(connection);
  try {
    if (args.command === "status") {
      await printStatus(client, connection);
      return 0;
    }
    if (args.command === "sessions") {
      await printSessions(client);
      return 0;
    }
    if (args.command === "continue") {
      await startChat(client, connection, { target: args.target, plain: args.plain });
      return 0;
    }
    if (args.command === "chat") {
      await startChat(client, connection, { session: args.session, plain: args.plain });
      return 0;
    }
  } catch (err) {
    console.error(`${ansi.red}${err instanceof Error ? err.message : String(err)}${ansi.reset}`);
    return 1;
  }

  console.log(helpText());
  return 0;
}

function shouldAutoStartServer(args) {
  if (args.url) return false;
  return args.command === "chat" || args.command === "continue";
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const code = await main();
  if (code) process.exit(code);
}
