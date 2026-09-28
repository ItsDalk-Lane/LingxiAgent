const COMMANDS = new Set(["serve", "status", "sessions", "continue", "chat", "bundle", "data", "help"]);
const BUNDLE_SUBCOMMANDS = new Set(["pull", "status"]);
const DATA_SUBCOMMANDS = new Set(["diagnose", "checkpoints", "restore"]);
const CHANNELS = new Set(["stable", "beta"]);
const RUNTIMES = new Set(["node", "rust"]);

export function parseCliArgs(argv = []) {
  const args = Array.from(argv);
  const command = args[0] && !args[0].startsWith("-") ? args.shift() : "help";
  if (!COMMANDS.has(command)) {
    return { command: "help", error: `unknown command: ${command}` };
  }

  const result = {
    command,
    subcommand: null,
    channel: "stable",
    runtime: "node",
    plain: false,
    url: null,
    token: null,
    session: null,
    target: null,
    allowDataDowngrade: false,
    confirmToken: null,
    passthrough: [],
  };
  const usedOptions = new Set();
  const markOption = (option) => {
    if (usedOptions.has(option)) throw new Error(`${option} was given more than once`);
    usedOptions.add(option);
  };

  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === "--help" || arg === "-h") {
      return { command: "help", error: null };
    } else if (arg === "--plain") {
      markOption(arg);
      result.plain = true;
    } else if (arg === "--allow-data-downgrade") {
      markOption(arg);
      result.allowDataDowngrade = true;
    } else if (arg === "--url") {
      markOption(arg);
      result.url = requireValue(args, ++i, "--url");
    } else if (arg === "--token") {
      markOption(arg);
      result.token = requireValue(args, ++i, "--token");
    } else if (arg === "--session") {
      markOption(arg);
      result.session = requireValue(args, ++i, "--session");
    } else if (arg === "--confirm-token") {
      markOption(arg);
      result.confirmToken = requireValue(args, ++i, "--confirm-token");
    } else if (arg === "--channel") {
      markOption(arg);
      const value = requireValue(args, ++i, "--channel");
      if (!CHANNELS.has(value)) {
        throw new Error(`--channel must be one of: stable, beta (got ${value})`);
      }
      result.channel = value;
    } else if (arg === "--runtime") {
      markOption(arg);
      const value = requireValue(args, ++i, "--runtime");
      if (!RUNTIMES.has(value)) {
        throw new Error(`--runtime must be one of: node, rust (got ${value})`);
      }
      result.runtime = value;
    } else if (arg === "--") {
      if (command !== "serve") {
        return { command: "help", error: `unknown argument: ${arg}` };
      }
      result.passthrough = args.slice(i + 1);
      break;
    } else if (command === "continue" && !result.target && !arg.startsWith("-")) {
      result.target = arg;
    } else if (command === "bundle" && !result.subcommand && !arg.startsWith("-")) {
      result.subcommand = arg;
    } else if (command === "data" && !result.subcommand && !arg.startsWith("-")) {
      result.subcommand = arg;
    } else if (command === "data" && result.subcommand === "restore" && !result.target && !arg.startsWith("-")) {
      result.target = arg;
    } else {
      return { command: "help", error: `unknown argument: ${arg}` };
    }
  }

  if (command === "bundle" && !BUNDLE_SUBCOMMANDS.has(result.subcommand)) {
    return {
      command: "help",
      error: result.subcommand
        ? `unknown bundle subcommand: ${result.subcommand} (expected pull or status)`
        : "bundle requires a subcommand: pull or status",
    };
  }

  if (command === "data") {
    if (!DATA_SUBCOMMANDS.has(result.subcommand)) {
      return {
        command: "help",
        error: result.subcommand
          ? `unknown data subcommand: ${result.subcommand} (expected diagnose, checkpoints, or restore)`
          : "data requires a subcommand: diagnose, checkpoints, or restore",
      };
    }
    if (result.subcommand === "restore" && !result.target) {
      return { command: "help", error: "data restore requires a transitionId: hana data restore <transitionId>" };
    }
  }

  const allowedOptions = {
    serve: new Set(["--runtime", "--channel", "--allow-data-downgrade"]),
    status: new Set(["--runtime", "--url", "--token"]),
    sessions: new Set(["--runtime", "--url", "--token"]),
    continue: new Set(["--runtime", "--url", "--token", "--plain"]),
    chat: new Set(["--runtime", "--url", "--token", "--session", "--plain"]),
    bundle: new Set(["--channel"]),
    data: new Set(result.subcommand === "restore" ? ["--confirm-token"] : []),
    help: new Set(),
  }[command];
  for (const option of usedOptions) {
    if (!allowedOptions.has(option)) {
      return { command: "help", error: `${option} is not supported for hana ${command}` };
    }
  }
  if (result.token && !result.url) {
    return { command: "help", error: "--token requires --url" };
  }

  return result;
}

function requireValue(args, index, flag) {
  const value = args[index];
  if (!value || value.startsWith("--")) {
    throw new Error(`${flag} requires a value`);
  }
  return value;
}

export function helpText() {
  return `Hana CLI

Usage:
  hana serve [-- server args]        Start a headless LingxiAgent Server (serves the --channel web frontend, if pulled)
  hana status                       Show local server and agent status
  hana sessions                     List recent sessions
  hana continue [index|path]        Continue a recent session
  hana chat [--plain]               Open chat
  hana bundle pull                  Pull and activate the latest web frontend
  hana bundle status                Show the pulled web frontend status
  hana data diagnose                Read-only data-epoch diagnostics (stamp, journal, checkpoints)
  hana data checkpoints             List available data-epoch recovery checkpoints
  hana data restore <transitionId>  Restore data from a checkpoint (asks for confirmation)

Connection options:
  --runtime <node|rust>            Select the existing Node server or the Rust service (default: node)
  --url <baseUrl>                   Connect to a specific LingxiAgent Server
  --token <token>                   Bearer token for that server
  --session <path>                  Chat in a specific session

Serve options:
  --allow-data-downgrade            Allow this kernel to open a data directory a newer
                                     kernel already touched (risk of silent data corruption)

Channel options:
  --channel <stable|beta>           Release channel for hana serve and hana bundle (default: stable)

Data recovery options:
  --confirm-token <token>           Non-interactive confirmation for \`hana data restore\`.
                                     Must exactly equal "restore <transitionId>". Required
                                     when stdin is not a TTY; there is no way to skip this.
`;
}
