import { homedir } from "node:os";
import { join } from "node:path";

export function defaultDevHanaHome() {
  return join(homedir(), ".lingxi-dev");
}

export function applyDevEnvironment(env = process.env, {
  nodeBin = process.execPath,
} = {}) {
  env.LINGXI_HOME = defaultDevHanaHome();
  env.LINGXI_DEV_NODE_BIN = nodeBin;
  return env;
}
