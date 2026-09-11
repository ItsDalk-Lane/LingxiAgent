/**
 * 基准运行环境的模块级仪表（仅 scripts/benchmark-history-read-directory.mjs 与
 * scripts/collect-history-read-directory-reference.mjs 使用）。
 *
 * 通过 node:module registerHooks 的 resolve 钩子，把三个目标模块的解析结果重定向到
 * scripts/lib/instr-*.mjs 计数包装（包装模块再转发到真实实现）。生产文件零改动；
 * 重定向只在基准进程内生效。钩子安装失败直接抛错，不静默降级。
 */

import { registerHooks } from "node:module";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const INSTR_MARKER = `${path.sep}scripts${path.sep}lib${path.sep}instr-`;

const TARGETS = [
  { suffix: `${path.sep}core${path.sep}message-utils.ts`, wrapper: "instr-message-utils.mjs" },
  { suffix: `${path.sep}shared${path.sep}tool-outcome.ts`, wrapper: "instr-tool-outcome.mjs" },
  { suffix: `${path.sep}lib${path.sep}tools${path.sep}todo-compat.ts`, wrapper: "instr-todo-compat.mjs" },
];

// 具名 fs 导入方（Node 内置模块 ESM 具名导出为启动快照，必须重定向到委托模块才能被计数覆盖）：
// - Pi SDK dist（SessionManager.loadEntriesFromFile / readSessionHeader 的 openSync+readSync 全量装载）
// - 仓库应用代码（core/lib/server/shared/hub）中少量具名导入
const FS_NAMED_PARENT_MARKERS = [
  `${path.sep}@earendil-works${path.sep}pi-coding-agent${path.sep}dist${path.sep}`,
  `${path.sep}core${path.sep}`,
  `${path.sep}lib${path.sep}`,
  `${path.sep}server${path.sep}`,
  `${path.sep}shared${path.sep}`,
  `${path.sep}hub${path.sep}`,
];

let installed = false;

export function installModuleWrappers() {
  if (installed) {
    throw new Error("[history-read-instrumentation] 模块包装钩子已安装，拒绝重复安装");
  }
  const here = path.dirname(fileURLToPath(import.meta.url));
  const targets = TARGETS.map((t) => ({ ...t, url: pathToFileURL(path.join(here, t.wrapper)).href }));
  const instrFsUrl = pathToFileURL(path.join(here, "instr-fs.mjs")).href;

  const deregister = registerHooks({
    resolve(specifier, context, next) {
      const parent = context?.parentURL ?? "";
      // 具名 fs 导入重定向：父模块在仓库应用代码或 Pi SDK dist 内、且导入的是 "fs"/"node:fs"
      if (
        (specifier === "fs" || specifier === "node:fs") &&
        parent.startsWith("file:") &&
        !parent.includes(INSTR_MARKER) &&
        FS_NAMED_PARENT_MARKERS.some((m) => parent.includes(m))
      ) {
        return { url: instrFsUrl, shortCircuit: true };
      }
      const res = next(specifier, context);
      const url = res && typeof res === "object" ? res.url : res;
      if (typeof url !== "string" || !url.startsWith("file:")) return res;
      if (parent.includes(INSTR_MARKER)) return res; // 包装模块自身的 import 直达真实实现
      const hit = targets.find((t) => url.endsWith(t.suffix) && !url.includes(INSTR_MARKER));
      if (hit) return { url: hit.url, shortCircuit: true };
      return res;
    },
  });

  installed = true;
  return function uninstallModuleWrappers() {
    if (typeof deregister === "function") deregister();
    installed = false;
  };
}
