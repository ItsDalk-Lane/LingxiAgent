import fs from "fs";
import path from "path";

const WORKSPACE_INSTRUCTION_FILES = [
  { filename: "AGENTS.md", key: "inject_agents_md" },
  { filename: "CLAUDE.md", key: "inject_claude_md" },
];

/**
 * 校验用户自定义的注入文件名：
 * 与 AGENTS.md / CLAUDE.md 一样按名字在工作目录链路里逐层查找，
 * 所以必须是一个纯文件名，含路径分隔符或 . / .. 都视为无效。
 */
function normalizeCustomFileName(rawName) {
  const name = typeof rawName === "string" ? rawName.trim() : "";
  if (!name || name === "." || name === "..") return null;
  if (name.includes("/") || name.includes("\\")) return null;
  return name;
}

function normalizeComparePath(value) {
  const resolved = path.resolve(value);
  return process.platform === "win32" ? resolved.toLowerCase() : resolved;
}

function samePath(a, b) {
  return normalizeComparePath(a) === normalizeComparePath(b);
}

function existingDirectory(rawPath) {
  if (!rawPath || typeof rawPath !== "string") return null;
  try {
    const resolved = path.resolve(rawPath);
    const stat = fs.statSync(resolved);
    if (stat.isDirectory()) return resolved;
    if (stat.isFile()) return path.dirname(resolved);
    return null;
  } catch {
    return null;
  }
}

function findGitRoot(startDir) {
  let current = path.resolve(startDir);
  while (true) {
    try {
      if (fs.existsSync(path.join(current, ".git"))) return current;
    } catch {
      return null;
    }
    const parent = path.dirname(current);
    if (samePath(parent, current)) return null;
    current = parent;
  }
}

function directoriesFromRootToCwd(rootDir, cwd) {
  const dirs = [];
  let current = path.resolve(cwd);
  while (true) {
    dirs.unshift(current);
    if (samePath(current, rootDir)) break;
    const parent = path.dirname(current);
    if (samePath(parent, current)) return [];
    current = parent;
  }
  return dirs;
}

function readInstructionFile(filePath) {
  try {
    return { content: fs.readFileSync(filePath, "utf-8") };
  } catch (err) {
    if (err?.code === "ENOENT") return null;
    return { error: err?.message || String(err) };
  }
}

/**
 * excludeFiles: absolute paths that must not be injected as workspace
 * instructions. The agent's own persona files (AGENTS.md / AGENTS.public.md)
 * live in its agent directory, and a session whose working directory is that
 * directory would otherwise pick the persona up a second time here, on top of
 * the system prompt that already carries it. Matching is on the exact resolved
 * path, so a same-named file anywhere else in the directory chain is still
 * injected.
 */
export function collectWorkspaceInstructionFiles({ cwd, workspaceContext, excludeFiles }: { cwd?: any; workspaceContext?: any; excludeFiles?: any } = {}) {
  const enabled = new Set();
  const config = workspaceContext && typeof workspaceContext === "object" ? workspaceContext : {};
  for (const item of WORKSPACE_INSTRUCTION_FILES) {
    if (config[item.key] === true) enabled.add(item.filename);
  }

  // 第三路注入：与固定两路同一套查找方式（工作目录 → Git 根目录逐层），
  // 只是文件名由用户自定义。
  const customFileName = config.inject_custom_file === true
    ? normalizeCustomFileName(config.custom_file_name)
    : null;

  if (enabled.size === 0 && !customFileName) return [];

  const startDir = existingDirectory(cwd);
  if (!startDir) return [];

  const excluded = new Set(
    (Array.isArray(excludeFiles) ? excludeFiles : [])
      .filter((entry) => typeof entry === "string" && entry)
      .map(normalizeComparePath),
  );

  const gitRoot = findGitRoot(startDir);
  const searchRoot = gitRoot || startDir;
  const dirs = directoriesFromRootToCwd(searchRoot, startDir);

  // 每个目录里按 AGENTS.md → CLAUDE.md → 自定义名 的顺序查找；
  // seen 防止自定义名与固定名相同时同一路径被注入两次。
  const names = [
    ...WORKSPACE_INSTRUCTION_FILES.map((item) => item.filename).filter((name) => enabled.has(name)),
    ...(customFileName ? [customFileName] : []),
  ];
  const seen = new Set();
  const files = [];
  for (const dir of dirs) {
    for (const name of names) {
      const filePath = path.join(dir, name);
      const key = normalizeComparePath(filePath);
      if (seen.has(key)) continue;
      seen.add(key);
      if (excluded.has(key)) continue;
      const result = readInstructionFile(filePath);
      if (!result) continue;
      files.push({
        path: filePath,
        filename: name,
        ...(name === customFileName ? { custom: true } : {}),
        ...result,
      });
    }
  }
  return files;
}

export function formatWorkspaceInstructionFiles(files: any, { locale }: { locale?: any } = {}) {
  const items = Array.isArray(files) ? files : [];
  if (items.length === 0) return "";
  const isZh = String(locale || "").startsWith("zh");
  const hasCustomFile = items.some((file) => file?.custom === true);
  const sourceLine = isZh
    ? (hasCustomFile
      ? "以下内容来自主工作台目录链路中的 AGENTS.md / CLAUDE.md，以及按自定义文件名读取的说明文件。它们是项目级工作规则，只对当前工作区上下文生效。"
      : "以下内容来自主工作台目录链路中的 AGENTS.md / CLAUDE.md。它们是项目级工作规则，只对当前工作区上下文生效。")
    : (hasCustomFile
      ? "The following content comes from AGENTS.md / CLAUDE.md files in the primary workbench's directory chain, plus files matched by the custom file name from settings. Treat them as project-level working rules for this workspace context."
      : "The following content comes from AGENTS.md / CLAUDE.md files in the primary workbench's directory chain. Treat them as project-level working rules for this workspace context.");
  const body = items.map((file) => {
    const content = typeof file.content === "string"
      ? file.content.trim()
      : (isZh
        ? `无法读取该文件：${file.error || "未知错误"}`
        : `Could not read this file: ${file.error || "unknown error"}`);
    return [
      `### ${file.filename || path.basename(file.path || "")}`,
      file.path ? `Path: ${file.path}` : "",
      "",
      content,
    ].filter((line, index) => index !== 1 || line).join("\n");
  }).join("\n\n");

  return isZh
    ? `\n## 工作区说明\n\n${sourceLine}\n\n${body}`
    : `\n## Workspace Instructions\n\n${sourceLine}\n\n${body}`;
}

export function buildWorkspaceInstructionPrompt({ cwd, workspaceContext, locale, excludeFiles }: { cwd?: string; workspaceContext?: unknown; locale?: string; excludeFiles?: string[] } = {}) {
  return formatWorkspaceInstructionFiles(
    collectWorkspaceInstructionFiles({ cwd, workspaceContext, excludeFiles }),
    { locale },
  );
}
