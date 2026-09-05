/**
 * git-environment — 「环境信息」卡的 HTTP 面（/api/git/*）
 *
 * 所有端点以 dir（工作台本地目录绝对路径）定位仓库，校验沿用 desk 路由的
 * 目录准入惯例（agent 工作区根 / engine 已知根，解析 symlink 后比较）。
 * 只读端点对非 git 目录返回 isRepo:false 的占位（200，前端降级展示），
 * 操作端点（checkout/commit/push）对非 git 目录直接 400。
 *
 * AI 提交信息复用 utility:call-text 总线处理器（auxiliary summarize 槽 +
 * callText + usage/trace 记账），路由侧只负责收集 diff 上下文与净化输出。
 */
import { Hono } from "hono";
import fs from "node:fs";
import path from "node:path";
import { loadStudioMountRegistry } from "../../core/studio-mounts.ts";
import {
  GitError,
  collectGitStatus,
  worktreeInfo,
  fileDiff,
  checkoutBranch,
  commitChanges,
  pushChanges,
  listBranches,
  tryGit,
} from "../git/git-command.ts";

function realPath(p: string): string | null {
  try {
    return fs.realpathSync(p);
  } catch {
    return null;
  }
}

function isInsideAnyRoot(dir: string, roots: unknown[]): boolean {
  const resolved = realPath(dir);
  if (!resolved) return false;
  return roots.filter(Boolean).some(root => {
    const r = realPath(String(root));
    if (!r) return false;
    return resolved === r || resolved.startsWith(r + path.sep);
  });
}

function selectedAgentDeskRoots(engine: any, agentId: string | null): string[] {
  if (!agentId || typeof agentId !== "string") return [];
  return [
    typeof engine.getExplicitHomeCwd === "function" ? engine.getExplicitHomeCwd(agentId) : null,
    typeof engine.getHomeCwd === "function" ? engine.getHomeCwd(agentId) : null,
  ].filter(Boolean) as string[];
}

/**
 * 工作台注册表（studio-mounts.json）里 active 的 local_fs 挂载根目录。
 * 工作台切换器注册的目录大多不在 cwd_history 里，git 面必须把注册表
 * 纳入准入，否则用户切到 LingxiAgent 工作台这类仓库时整卡被 403 拒绝。
 */
function studioMountRoots(engine: any): string[] {
  const lingxiHome = engine?.lingxiHome;
  if (typeof lingxiHome !== "string" || !lingxiHome) return [];
  try {
    const registry = loadStudioMountRegistry(lingxiHome);
    const mounts = Array.isArray(registry?.mounts) ? registry.mounts : [];
    return mounts
      .filter((m: any) => m?.status === "active"
        && m?.sourceKind === "storage"
        && m?.provider === "local_fs"
        && typeof m?.rootLocator?.path === "string"
        && m.rootLocator.path)
      .map((m: any) => m.rootLocator.path);
  } catch {
    return [];
  }
}

/** 与 desk 路由同款目录准入：engine 已知根 + 工作台挂载注册表（解析 symlink 后比较） */
function isApprovedDir(dir: string, engine: any, agentId: string | null): boolean {
  if (isInsideAnyRoot(dir, selectedAgentDeskRoots(engine, agentId))) return true;
  if (typeof engine.isApprovedDeskDir === "function" && engine.isApprovedDeskDir(dir, { agentId })) {
    return true;
  }
  if (typeof engine.isApprovedWorkspaceDir === "function" && engine.isApprovedWorkspaceDir(dir, { agentId })) {
    return true;
  }
  const approved = [
    engine.deskCwd,
    engine.homeCwd,
    ...studioMountRoots(engine),
    ...(Array.isArray(engine.config?.cwd_history) ? engine.config.cwd_history : []),
  ].filter(Boolean);
  return isInsideAnyRoot(dir, approved);
}

function resolveDir(raw: unknown): string | null {
  if (typeof raw !== "string" || !raw.trim()) return null;
  const dir = path.resolve(raw);
  if (!path.isAbsolute(dir)) return null;
  try {
    if (!fs.statSync(dir).isDirectory()) return null;
  } catch {
    return null;
  }
  return dir;
}

function isRepoDir(dir: string): Promise<boolean> {
  return tryGit(dir, ["rev-parse", "--is-inside-work-tree"])
    .then(r => r.ok && r.stdout.trim() === "true");
}

const AI_MESSAGE_TIMEOUT_MS = 45_000;
const DIFF_EXCERPT_CHAR_CAP = 12_000;
const UNTRACKED_EXCERPT_TOTAL_CAP = 4_000;
const UNTRACKED_EXCERPT_LINE_CAP = 60;
const MAX_PROMPT_FILES = 80;

/** 收集用于 AI 提交信息的变更上下文：numstat 摘要 + 截断 diff + 未跟踪文件开头 */
async function collectChangeContext(dir: string, includeUnstaged: boolean): Promise<string> {
  const status = await collectGitStatus(dir);
  const files = includeUnstaged
    ? status.files
    : status.files.filter(f => f.staged);
  if (files.length === 0) return "";

  const statLines = files
    .slice(0, MAX_PROMPT_FILES)
    .map(f => `+${f.additions} -${f.deletions} [${f.state}] ${f.path}`)
    .join("\n");
  const moreNote = files.length > MAX_PROMPT_FILES ? `\n… 另有 ${files.length - MAX_PROMPT_FILES} 个文件` : "";

  const parts: string[] = [`变更文件（共 ${files.length} 个）：\n${statLines}${moreNote}`];

  const trackedDiff = await tryGit(dir, ["-c", "core.quotepath=false", "diff", "HEAD", "-U3"]);
  if (trackedDiff.ok && trackedDiff.stdout.trim()) {
    parts.push(`已跟踪文件 diff（截断）：\n${trackedDiff.stdout.slice(0, DIFF_EXCERPT_CHAR_CAP)}`);
  }

  if (includeUnstaged) {
    const untracked = files.filter(f => f.state === "untracked").slice(0, 8);
    let budget = UNTRACKED_EXCERPT_TOTAL_CAP;
    const excerpts: string[] = [];
    for (const f of untracked) {
      if (budget <= 0) break;
      const res = await tryGit(dir, ["-c", "core.quotepath=false", "diff", "--no-index", "--", "/dev/null", f.path]);
      if (!res.ok || !res.stdout) continue;
      const body = res.stdout
        .split("\n")
        .filter(line => line.startsWith("+") && !line.startsWith("+++"))
        .slice(0, UNTRACKED_EXCERPT_LINE_CAP)
        .join("\n");
      const clipped = body.slice(0, budget);
      budget -= clipped.length;
      excerpts.push(`新文件 ${f.path} 开头：\n${clipped}`);
    }
    if (excerpts.length > 0) parts.push(excerpts.join("\n\n"));
  }

  return parts.join("\n\n");
}

/** 净化模型输出：剥代码围栏后取首行，去引号/前缀、限长 */
function sanitizeCommitMessage(raw: string): string {
  const withoutFences = raw.replace(/```[a-zA-Z]*\n?/g, "");
  const firstLine = withoutFences
    .split("\n")
    .map(s => s.trim())
    .filter(Boolean)[0] ?? "";
  const cleaned = firstLine
    .replace(/^["'`]|["'`]$/g, "")
    .replace(/^(commit( message)?|提交信息)\s*[:：]\s*/i, "")
    .trim();
  return cleaned.slice(0, 100);
}

export function createGitEnvironmentRoute(engine: any, hub?: any) {
  const route = new Hono();

  const resolveQueryDir = (c: any): { dir: string; agentId: string | null } | Response => {
    const dir = resolveDir(c.req.query("dir"));
    if (!dir) return c.json({ error: "invalid dir" }, 400);
    const agentId = typeof c.req.query("agentId") === "string" && c.req.query("agentId").trim()
      ? c.req.query("agentId").trim()
      : null;
    if (!isApprovedDir(dir, engine, agentId)) return c.json({ error: "dir not allowed" }, 403);
    return { dir, agentId };
  };

  const resolveBodyDir = (body: any): { dir: string; agentId: string | null } | null => {
    const dir = resolveDir(body?.dir);
    if (!dir) return null;
    const agentId = typeof body?.agentId === "string" && body.agentId.trim() ? body.agentId.trim() : null;
    return isApprovedDir(dir, engine, agentId) ? { dir, agentId } : null;
  };

  route.get("/git/status", async (c) => {
    try {
      const resolved = resolveQueryDir(c);
      if (resolved instanceof Response) return resolved;
      return c.json(await collectGitStatus(resolved.dir));
    } catch (err: any) {
      return c.json({ error: err instanceof GitError ? err.stderr || err.message : err.message }, 500);
    }
  });

  route.get("/git/worktree-info", async (c) => {
    try {
      const resolved = resolveQueryDir(c);
      if (resolved instanceof Response) return resolved;
      return c.json(await worktreeInfo(resolved.dir));
    } catch (err: any) {
      return c.json({ error: err instanceof GitError ? err.stderr || err.message : err.message }, 500);
    }
  });

  route.get("/git/branches", async (c) => {
    try {
      const resolved = resolveQueryDir(c);
      if (resolved instanceof Response) return resolved;
      if (!(await isRepoDir(resolved.dir))) {
        return c.json({ isRepo: false, branches: [], detached: false, current: null });
      }
      const { branches, detached, current } = await listBranches(resolved.dir);
      return c.json({ isRepo: true, branches, detached, current });
    } catch (err: any) {
      return c.json({ error: err instanceof GitError ? err.stderr || err.message : err.message }, 500);
    }
  });

  route.post("/git/checkout", async (c) => {
    try {
      const body = await c.req.json().catch(() => ({}));
      const resolved = resolveBodyDir(body);
      if (!resolved) return c.json({ error: "invalid dir" }, 400);
      if (typeof body?.branch !== "string" || !body.branch.trim()) {
        return c.json({ error: "branch required" }, 400);
      }
      if (!(await isRepoDir(resolved.dir))) return c.json({ error: "not a git repo" }, 400);
      await checkoutBranch(resolved.dir, body.branch.trim());
      return c.json({ ok: true, branch: body.branch.trim() });
    } catch (err: any) {
      return c.json({ error: err instanceof GitError ? err.stderr || err.message : err.message }, 500);
    }
  });

  route.get("/git/file-diff", async (c) => {
    try {
      const resolved = resolveQueryDir(c);
      if (resolved instanceof Response) return resolved;
      const file = c.req.query("file") ?? "";
      if (!(await isRepoDir(resolved.dir))) return c.json({ error: "not a git repo" }, 400);
      const diff = await fileDiff(resolved.dir, file);
      return c.json(diff);
    } catch (err: any) {
      const status = err instanceof GitError && /invalid file path/.test(err.message) ? 400 : 500;
      return c.json({ error: err instanceof GitError ? err.stderr || err.message : err.message }, status);
    }
  });

  route.post("/git/commit", async (c) => {
    try {
      const body = await c.req.json().catch(() => ({}));
      const resolved = resolveBodyDir(body);
      if (!resolved) return c.json({ error: "invalid dir" }, 400);
      if (!(await isRepoDir(resolved.dir))) return c.json({ error: "not a git repo" }, 400);
      const message = typeof body?.message === "string" ? body.message : "";
      const includeUnstaged = body?.includeUnstaged !== false;
      const result = await commitChanges(resolved.dir, message, includeUnstaged);
      if (!result.ok) return c.json(result, 400);
      return c.json(result);
    } catch (err: any) {
      return c.json({ error: err instanceof GitError ? err.stderr || err.message : err.message }, 500);
    }
  });

  route.post("/git/push", async (c) => {
    try {
      const body = await c.req.json().catch(() => ({}));
      const resolved = resolveBodyDir(body);
      if (!resolved) return c.json({ error: "invalid dir" }, 400);
      if (!(await isRepoDir(resolved.dir))) return c.json({ error: "not a git repo" }, 400);
      const result = await pushChanges(resolved.dir);
      if (!result.ok) return c.json(result, 400);
      return c.json(result);
    } catch (err: any) {
      return c.json({ error: err instanceof GitError ? err.stderr || err.message : err.message }, 500);
    }
  });

  route.post("/git/ai-commit-message", async (c) => {
    try {
      const body = await c.req.json().catch(() => ({}));
      const resolved = resolveBodyDir(body);
      if (!resolved) return c.json({ error: "invalid dir" }, 400);
      if (!(await isRepoDir(resolved.dir))) return c.json({ error: "not a git repo" }, 400);
      const includeUnstaged = body?.includeUnstaged !== false;

      const changeContext = await collectChangeContext(resolved.dir, includeUnstaged);
      if (!changeContext) return c.json({ error: "no changes" }, 400);

      if (!hub?.eventBus?.request) return c.json({ error: "llm unavailable" }, 503);
      const answer = await Promise.race([
        hub.eventBus.request("utility:call-text", {
          systemPrompt: [
            "你是 git 提交信息生成器。根据给定的变更统计与 diff 摘要，输出一条简短的中文提交信息。",
            "要求：只输出提交信息本身一行文字（不超过 50 个字），概括这次变更做了什么；",
            "不要解释、不要引号、不要 markdown、不要署名。",
          ].join("\n"),
          messages: [{ role: "user", content: changeContext }],
          temperature: 0.3,
          maxTokens: 200,
          operation: "git-commit-message",
          sessionPath: typeof body?.sessionPath === "string" ? body.sessionPath : null,
          agentId: resolved.agentId,
        }),
        new Promise<never>((_, reject) =>
          setTimeout(() => reject(new Error("ai commit message timed out")), AI_MESSAGE_TIMEOUT_MS)),
      ]) as { text?: string } | { text?: string }[];

      const raw = Array.isArray(answer) ? answer[0]?.text : answer?.text;
      const message = sanitizeCommitMessage(typeof raw === "string" ? raw : "");
      if (!message) return c.json({ error: "ai produced empty message" }, 502);
      return c.json({ message });
    } catch (err: any) {
      return c.json({ error: err.message }, 500);
    }
  });

  return route;
}
