/**
 * bundled-bins.ts — 随软件分发的内置二进制（rg / fd / ast-grep）的定位。
 *
 * 打包链路：scripts/fetch-bundled-binaries.cjs 在构建期把当前平台的三件
 * 抓到 bundled-bin/{mac|linux|win}-{arch}/ 暂存目录，electron-builder
 * extraResources 按目标平台摊平进 app Resources 的 bundled-bin/。
 *
 * 运行时候选链与 win32-sandbox-helper 的 resourceSiblingCandidates 同一惯例：
 * LINGXI_DESKTOP_RESOURCES_PATH（打包 server 由主进程注入）→
 * process.resourcesPath（Electron 内直跑时天然可见）→
 * LINGXI_ROOT（打包态查上级到 Resources 根；dev 态指仓库根，可命中暂存目录）。
 *
 * 解析不到返回 null，调用方回落「托管目录下载 → PATH」——内置缺失不是错误，
 * 但这里绝不做网络下载，下载是 ast-grep-binary / search-tools 的事。
 */
import fs from "node:fs";
import path from "node:path";

/** Node 平台名 → 打包目录名（electron-builder ${os} 宏同名体系）。 */
function builderOsName(platform: NodeJS.Platform): string {
  if (platform === "darwin") return "mac";
  if (platform === "win32") return "win";
  return "linux";
}

/**
 * 依次尝试的 bundled-bin 根目录（绝对路径，去重）。
 * - LINGXI_DESKTOP_RESOURCES_PATH：打包 server 由 Electron 主进程注入；
 * - process.resourcesPath：Electron 内直跑时天然可见；
 * - LINGXI_ROOT/..：打包态的 versioned server root 上级即 Resources 根；
 * - LINGXI_ROOT：dev 态指仓库根，可命中构建期暂存目录 bundled-bin/{os}-{arch}/。
 */
function bundledBinRootCandidates(
  env: NodeJS.ProcessEnv,
  resourcesPath: string | undefined,
): string[] {
  const roots: string[] = [];
  const push = (p: string | undefined | null) => {
    if (p && typeof p === "string" && path.isAbsolute(p) && !roots.includes(p)) roots.push(p);
  };
  push(env.LINGXI_DESKTOP_RESOURCES_PATH);
  push(resourcesPath);
  if (env.LINGXI_ROOT) {
    push(path.resolve(env.LINGXI_ROOT, ".."));
    push(env.LINGXI_ROOT);
  }
  return roots;
}

/**
 * 定位内置二进制。按候选根 × 布局（摊平的 packaged 布局优先，其次构建期
 * 暂存的 {os}-{arch}/ 布局）逐个试存在性 + 可执行位；找不到返回 null。
 */
export function findBundledBin(
  binName: string,
  options: {
    env?: NodeJS.ProcessEnv;
    resourcesPath?: string;
    platform?: NodeJS.Platform;
    arch?: string;
    existsSync?: typeof fs.existsSync;
    accessSync?: typeof fs.accessSync;
  } = {},
): string | null {
  const env = options.env ?? process.env;
  const platform = options.platform ?? process.platform;
  const arch = options.arch ?? process.arch;
  const existsSync = options.existsSync ?? fs.existsSync;
  const accessSync = options.existsSync ? (options.accessSync ?? (() => {})) : fs.accessSync;
  const filename = platform === "win32" ? `${binName}.exe` : binName;
  const layoutRels = [
    path.join("bundled-bin", filename),
    path.join("bundled-bin", `${builderOsName(platform)}-${arch}`, filename),
    path.join("bundled-bin", `${platform}-${arch}`, filename),
  ];
  for (const root of bundledBinRootCandidates(env, options.resourcesPath)) {
    for (const rel of layoutRels) {
      const candidate = path.join(root, rel);
      if (!existsSync(candidate)) continue;
      try {
        accessSync(candidate, fs.constants.X_OK);
        return candidate;
      } catch { /* 不可执行视为未命中，继续下一个候选 */ }
    }
  }
  return null;
}
