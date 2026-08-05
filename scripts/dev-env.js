import { homedir } from "node:os";
import { join } from "node:path";

export function defaultDevLingxiHome() {
  return join(homedir(), ".lingxi-dev");
}

export function applyDevEnvironment(env = process.env, {
  nodeBin = process.execPath,
} = {}) {
  env.LINGXI_HOME = defaultDevLingxiHome();
  env.LINGXI_DEV_NODE_BIN = nodeBin;
  return env;
}
