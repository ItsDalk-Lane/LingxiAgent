import { homedir } from "node:os";
import { join } from "node:path";
import runtimePaths from "../shared/hana-runtime-paths.cjs";

export function defaultDevLingxiHome() {
  return join(homedir(), ".lingxi-dev");
}

export function applyDevEnvironment(env = process.env, {
  nodeBin = process.execPath,
} = {}) {
  // 显式指定的演练副本优先；只有未指定时才使用原开发目录。
  env.LINGXI_HOME = runtimePaths.resolveLingxiHome(env.LINGXI_HOME || defaultDevLingxiHome());
  env.LINGXI_DEV_NODE_BIN = nodeBin;
  return env;
}
