/**
 * git-environment route — HTTP 面集成测试
 *
 * 真实临时 git 仓库 + mock engine（目录准入）+ 可选 hub（AI 提交信息走
 * utility:call-text 总线）。锁定：dir 校验/准入、只读端点对非 git 目录的
 * 降级、checkout/commit/push 的结果契约、AI 输出净化。
 */
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { Hono } from "hono";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createGitEnvironmentRoute } from "../server/routes/git-environment.ts";

const execFileAsync = promisify(execFile);

async function git(dir: string, args: string[]): Promise<void> {
  await execFileAsync("git", args, { cwd: dir });
}

let repoDir = "";
let plainDir = "";

beforeAll(async () => {
  repoDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-route-repo-"));
  plainDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-route-plain-"));

  try {
    await git(repoDir, ["init", "-b", "main"]);
  } catch {
    await git(repoDir, ["init"]);
  }
  await git(repoDir, ["config", "user.name", "Lingxi Test"]);
  await git(repoDir, ["config", "user.email", "test@lingxi.local"]);
  fs.writeFileSync(path.join(repoDir, "a.md"), "one\ntwo\n");
  await git(repoDir, ["add", "-A"]);
  await git(repoDir, ["commit", "-m", "init"]);
  fs.writeFileSync(path.join(repoDir, "a.md"), "one\ntwo\nthree\n");
  fs.writeFileSync(path.join(repoDir, "new.txt"), "hello\n");
  await git(repoDir, ["branch", "feature"]);
});

afterAll(() => {
  fs.rmSync(repoDir, { recursive: true, force: true });
  fs.rmSync(plainDir, { recursive: true, force: true });
});

function makeApp(hub?: unknown) {
  // homeCwd/deskCwd 属性是 desk 系目录准入的兜底来源（与真实 engine 同名）
  const engine = {
    getExplicitHomeCwd: vi.fn((agentId: string) => (agentId === "hana" ? repoDir : null)),
    getHomeCwd: vi.fn(() => null),
    homeCwd: repoDir,
    deskCwd: repoDir,
  };
  const app = new Hono();
  app.route("/api", createGitEnvironmentRoute(engine, hub));
  return app;
}

function get(app: ReturnType<typeof makeApp>, url: string) {
  return app.request(url);
}

function post(app: ReturnType<typeof makeApp>, url: string, body: unknown) {
  return app.request(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
}

describe("git-environment route", () => {
  it("returns a full status summary for an approved workspace dir", async () => {
    const app = makeApp();
    const res = await get(app, `/api/git/status?dir=${encodeURIComponent(repoDir)}`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.isRepo).toBe(true);
    expect(body.currentBranch).toBe("main");
    expect(body.commitable).toBe(true);
    const byPath = new Map(body.files.map((f: any) => [f.path, f]));
    expect(byPath.get("a.md")).toMatchObject({ additions: 1, deletions: 0, staged: false });
    expect(byPath.get("new.txt")).toMatchObject({ additions: 1, deletions: 0, state: "untracked" });
  });

  it("degrades read endpoints for a non-git dir instead of erroring", async () => {
    // 引擎只认可 repoDir；用 query 直接指向 plainDir 需要走准入 —— 改为允许它
    const engine = {
      getExplicitHomeCwd: vi.fn(() => null),
      getHomeCwd: vi.fn(() => plainDir),
      homeCwd: plainDir,
      deskCwd: plainDir,
    };
    const app2 = new Hono();
    app2.route("/api", createGitEnvironmentRoute(engine));
    const res = await app2.request(`/api/git/status?dir=${encodeURIComponent(plainDir)}`);
    expect(res.status).toBe(200);
    expect((await res.json()).isRepo).toBe(false);
    const branches = await app2.request(`/api/git/branches?dir=${encodeURIComponent(plainDir)}`);
    expect(branches.status).toBe(200);
    expect((await branches.json())).toMatchObject({ isRepo: false, branches: [] });
  });

  it("rejects invalid or non-approved dirs", async () => {
    const app = makeApp();
    expect((await get(app, "/api/git/status?dir=")).status).toBe(400);
    expect((await get(app, "/api/git/status?dir=relative/path")).status).toBe(400);
    const foreign = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-route-foreign-"));
    try {
      expect((await get(app, `/api/git/status?dir=${encodeURIComponent(foreign)}`)).status).toBe(403);
    } finally {
      fs.rmSync(foreign, { recursive: true, force: true });
    }
  });

  it("approves dirs registered as active studio mounts even outside cwd history", async () => {
    // 真实形态的工作台挂载注册表：active local_fs 挂载指向 repoDir，
    // engine 不提供任何其他准入来源（cwd_history 为空）
    const lingxiHome = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-mount-home-"));
    fs.writeFileSync(path.join(lingxiHome, "studio-mounts.json"), JSON.stringify({
      schemaVersion: 1,
      createdAt: "2026-09-05T00:00:00.000Z",
      updatedAt: "2026-09-05T00:00:00.000Z",
      mounts: [{
        schemaVersion: 1,
        mountId: "local_fs_test0000000000",
        hostStudioId: "studio_test",
        sourceKind: "storage",
        label: "repo",
        presentation: "folder",
        capabilities: ["list", "read", "write"],
        grantId: null,
        status: "active",
        createdAt: "2026-09-05T00:00:00.000Z",
        updatedAt: "2026-09-05T00:00:00.000Z",
        provider: "local_fs",
        rootLocator: { path: repoDir },
      }],
    }));
    const engine = {
      getExplicitHomeCwd: vi.fn(() => null),
      getHomeCwd: vi.fn(() => null),
      homeCwd: null,
      deskCwd: null,
      config: {},
      lingxiHome,
    };
    const app = new Hono();
    app.route("/api", createGitEnvironmentRoute(engine));
    try {
      const res = await app.request(`/api/git/status?dir=${encodeURIComponent(repoDir)}`);
      expect(res.status).toBe(200);
      expect((await res.json()).isRepo).toBe(true);
      // disabled 挂载不享受准入
      const disabledHome = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-mount-home2-"));
      try {
        fs.writeFileSync(path.join(disabledHome, "studio-mounts.json"), JSON.stringify({
          schemaVersion: 1,
          createdAt: "2026-09-05T00:00:00.000Z",
          updatedAt: "2026-09-05T00:00:00.000Z",
          mounts: [{
            schemaVersion: 1,
            mountId: "local_fs_off00000000000",
            hostStudioId: "studio_test",
            sourceKind: "storage",
            label: "repo",
            presentation: "folder",
            capabilities: ["list", "read", "write"],
            grantId: null,
            status: "disabled",
            createdAt: "2026-09-05T00:00:00.000Z",
            updatedAt: "2026-09-05T00:00:00.000Z",
            provider: "local_fs",
            rootLocator: { path: repoDir },
          }],
        }));
        const engine2 = { ...engine, lingxiHome: disabledHome };
        const app2 = new Hono();
        app2.route("/api", createGitEnvironmentRoute(engine2));
        const denied = await app2.request(`/api/git/status?dir=${encodeURIComponent(repoDir)}`);
        expect(denied.status).toBe(403);
      } finally {
        fs.rmSync(disabledHome, { recursive: true, force: true });
      }
    } finally {
      fs.rmSync(lingxiHome, { recursive: true, force: true });
    }
  });

  it("lists branches with current marker and worktree info", async () => {
    const app = makeApp();
    const branches = await get(app, `/api/git/branches?dir=${encodeURIComponent(repoDir)}&agentId=hana`);
    expect(branches.status).toBe(200);
    const body = await branches.json();
    expect(body.isRepo).toBe(true);
    expect(body.current).toBe("main");
    expect(body.branches.map((b: any) => b.name).sort()).toEqual(["feature", "main"]);

    const worktree = await get(app, `/api/git/worktree-info?dir=${encodeURIComponent(repoDir)}`);
    expect((await worktree.json())).toMatchObject({ isRepo: true, isMain: true });
  });

  it("serves untracked file diff as a synthesized new-file patch", async () => {
    const app = makeApp();
    const res = await get(app, `/api/git/file-diff?dir=${encodeURIComponent(repoDir)}&file=new.txt`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.binary).toBe(false);
    expect(body.patch).toContain("new file mode");
    expect(body.patch).toContain("+hello");
  });

  it("serves commit history with refs and parents", async () => {
    const app = makeApp();
    // 自造第二个提交，使断言不依赖文件内用例顺序
    const commit = await post(app, "/api/git/commit", { dir: repoDir, message: "log: 历史探针提交", includeUnstaged: true });
    expect(commit.status).toBe(200);
    const res = await get(app, `/api/git/log?dir=${encodeURIComponent(repoDir)}&limit=50`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.isRepo).toBe(true);
    expect(body.commits.length).toBeGreaterThanOrEqual(2);
    expect(body.commits[0].subject).toBe("log: 历史探针提交");
    expect(body.commits[0].message).toContain("log: 历史探针提交");
    expect(body.commits[0].refs.some((r: any) => r.kind === "head")).toBe(true);
    expect(body.commits[0].parents[0]).toBe(body.commits[1].hash);
  });

  it("checks out a branch and reflects it in status", async () => {
    const app = makeApp();
    const res = await post(app, "/api/git/checkout", { dir: repoDir, branch: "feature" });
    expect(res.status).toBe(200);
    expect((await res.json()).ok).toBe(true);
    const status = await get(app, `/api/git/status?dir=${encodeURIComponent(repoDir)}`);
    expect((await status.json()).currentBranch).toBe("feature");
    await post(app, "/api/git/checkout", { dir: repoDir, branch: "main" });
  });

  it("commits with includeUnstaged and leaves a clean tree", async () => {
    const app = makeApp();
    // 自带改动，不依赖其他用例留下的工作树状态
    fs.writeFileSync(path.join(repoDir, "route-commit.txt"), "x\n");
    const res = await post(app, "/api/git/commit", { dir: repoDir, message: "route: 提交", includeUnstaged: true });
    expect(res.status).toBe(200);
    expect((await res.json()).ok).toBe(true);
    const status = await get(app, `/api/git/status?dir=${encodeURIComponent(repoDir)}`);
    expect((await status.json()).files).toEqual([]);
  });

  it("reports structured failures for empty commit and missing remote push", async () => {
    const app = makeApp();
    const commit = await post(app, "/api/git/commit", { dir: repoDir, message: "x", includeUnstaged: true });
    expect(commit.status).toBe(400);
    expect((await commit.json()).code).toBe("nothing_to_commit");

    const push = await post(app, "/api/git/push", { dir: repoDir });
    expect(push.status).toBe(400);
    expect((await push.json()).code).toBe("no_remote");
  });

  it("generates and sanitizes an AI commit message (title + body) through the hub", async () => {
    const request = vi.fn(async (_type: string, payload: any) => {
      // 上下文应包含刚写入的 ai.md 的变更统计；提示词要求 Conventional Commits 标题+正文
      expect(payload.messages[0].content).toContain("ai.md");
      expect(payload.operation).toBe("git-commit-message");
      expect(payload.systemPrompt).toContain("Conventional Commits");
      return {
        text: "```\nfeat(server): 环境信息卡 git 面接入\n\n- 新增 /api/git/* 八端点\n- 目录准入纳入工作台挂载注册表\n- AI 提交信息支持标题+正文\n\n\n```",
      };
    });
    const app = makeApp({ eventBus: { request } });

    fs.writeFileSync(path.join(repoDir, "ai.md"), "x\n");
    const res = await post(app, "/api/git/ai-commit-message", { dir: repoDir, includeUnstaged: true });
    expect(res.status).toBe(200);
    // 围栏剥除、前缀剥除、连续空行压缩，标题+空行+要点结构完整保留
    expect((await res.json()).message).toBe(
      "feat(server): 环境信息卡 git 面接入\n\n- 新增 /api/git/* 八端点\n- 目录准入纳入工作台挂载注册表\n- AI 提交信息支持标题+正文",
    );
    expect(request).toHaveBeenCalledTimes(1);
  });

  it("fails AI generation fast when no hub is wired", async () => {
    const app = makeApp();
    fs.writeFileSync(path.join(repoDir, "ai2.md"), "y\n");
    const res = await post(app, "/api/git/ai-commit-message", { dir: repoDir, includeUnstaged: true });
    expect(res.status).toBe(503);
  });

  it("returns 400 for ai-commit-message when the tree is clean", async () => {
    const engine = { getExplicitHomeCwd: vi.fn(() => repoDir), getHomeCwd: vi.fn(() => null), homeCwd: repoDir, deskCwd: repoDir };
    const app = new Hono();
    app.route("/api", createGitEnvironmentRoute(engine, { eventBus: { request: async () => ({ text: "x" }) } }));
    // 先清空变更
    await post(app, "/api/git/commit", { dir: repoDir, message: "clean", includeUnstaged: true });
    const res = await post(app, "/api/git/ai-commit-message", { dir: repoDir, includeUnstaged: true });
    expect(res.status).toBe(400);
    expect((await res.json()).error).toBe("no changes");
  });
});

// ────────────────────────── 分支创建 / 暂存 / worktree ──────────────────────────

/**
 * 独立父目录 + 独立仓：worktree 落地根是 <主工作树父级>/worktrees，
 * 与其它测试文件共用 os.tmpdir() 会互相踩。
 */
describe("git-environment route — branch / stash / worktree endpoints", () => {
  let parentDir = "";
  let opsDir = "";
  let app!: ReturnType<typeof makeApp>;

  function makeOpsApp(root: string) {
    const engine = {
      getExplicitHomeCwd: vi.fn(() => root),
      getHomeCwd: vi.fn(() => null),
      homeCwd: root,
      deskCwd: root,
    };
    const hono = new Hono();
    hono.route("/api", createGitEnvironmentRoute(engine));
    return hono;
  }

  beforeAll(async () => {
    parentDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-route-ops-"));
    opsDir = path.join(parentDir, "repo");
    fs.mkdirSync(opsDir);
    try {
      await git(opsDir, ["init", "-b", "main"]);
    } catch {
      await git(opsDir, ["init"]);
    }
    await git(opsDir, ["config", "user.name", "Lingxi Test"]);
    await git(opsDir, ["config", "user.email", "test@lingxi.local"]);
    fs.writeFileSync(path.join(opsDir, "a.md"), "one\n");
    await git(opsDir, ["add", "-A"]);
    await git(opsDir, ["commit", "-m", "init"]);
    app = makeOpsApp(parentDir);
  });

  afterAll(() => {
    if (parentDir) fs.rmSync(parentDir, { recursive: true, force: true });
  });

  it("creates and checks out a branch, then refuses the duplicate name", async () => {
    const created = await post(app, "/api/git/create-branch", { dir: opsDir, name: "feat/from-route" });
    expect(created.status).toBe(200);
    expect(await created.json()).toMatchObject({ ok: true, branch: "feat/from-route" });

    const status = await get(app, `/api/git/status?dir=${encodeURIComponent(opsDir)}`);
    expect((await status.json()).currentBranch).toBe("feat/from-route");

    const again = await post(app, "/api/git/create-branch", { dir: opsDir, name: "feat/from-route" });
    expect(again.status).toBe(400);
    expect((await again.json()).code).toBe("already_exists");

    const bad = await post(app, "/api/git/create-branch", { dir: opsDir, name: "--upload-pack=evil" });
    expect(bad.status).toBe(400);
    expect((await bad.json()).code).toBe("invalid_name");
  });

  it("stashes the working tree and reports nothing_to_stash when clean", async () => {
    fs.writeFileSync(path.join(opsDir, "a.md"), "one\ntwo\n");
    fs.writeFileSync(path.join(opsDir, "untracked.txt"), "x\n");

    const stashed = await post(app, "/api/git/stash", { dir: opsDir, message: "route: 暂存" });
    expect(stashed.status).toBe(200);
    expect((await stashed.json()).ok).toBe(true);

    const status = await get(app, `/api/git/status?dir=${encodeURIComponent(opsDir)}`);
    expect((await status.json()).files).toEqual([]);

    const again = await post(app, "/api/git/stash", { dir: opsDir });
    expect(again.status).toBe(400);
    expect((await again.json()).code).toBe("nothing_to_stash");
  });

  it("lists worktrees with the landing root", async () => {
    const res = await get(app, `/api/git/worktrees?dir=${encodeURIComponent(opsDir)}`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.isRepo).toBe(true);
    expect(body.root).toBe(path.join(path.dirname(body.mainPath), "worktrees"));
    expect(body.worktrees).toHaveLength(1);
    expect(body.worktrees[0]).toMatchObject({ isMain: true, current: true });
  });

  it("creates an isolated worktree with a wt/<name> branch", async () => {
    const res = await post(app, "/api/git/worktree-create", { dir: opsDir, name: "fix-login-race" });
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body).toMatchObject({ ok: true, branch: "wt/fix-login-race" });
    expect(fs.existsSync(path.join(body.path, "a.md"))).toBe(true);

    // 当前检出不受影响
    const status = await get(app, `/api/git/status?dir=${encodeURIComponent(opsDir)}`);
    expect((await status.json()).currentBranch).toBe("feat/from-route");

    const list = await get(app, `/api/git/worktrees?dir=${encodeURIComponent(opsDir)}`);
    const linked = (await list.json()).worktrees
      .find((entry: any) => entry.branch === "wt/fix-login-race");
    expect(linked).toMatchObject({ isMain: false, current: false });

    const again = await post(app, "/api/git/worktree-create", { dir: opsDir, name: "fix-login-race" });
    expect(again.status).toBe(400);
    expect((await again.json()).code).toBe("exists");
  });

  it("stashes a single file, lists it, and takes it back through the route", async () => {
    fs.writeFileSync(path.join(opsDir, "a.md"), "one\nroute-single\n");
    const stashed = await post(app, "/api/git/stash", { dir: opsDir, paths: ["a.md"], message: "route single" });
    expect(stashed.status).toBe(200);
    expect(await stashed.json()).toMatchObject({ ok: true, paths: ["a.md"] });

    const list = await get(app, `/api/git/stashes?dir=${encodeURIComponent(opsDir)}`);
    expect(list.status).toBe(200);
    const stashes = (await list.json()).stashes;
    expect(stashes[0]).toMatchObject({ ref: "stash@{0}", tracked: ["a.md"] });

    const back = await post(app, "/api/git/unstash", { dir: opsDir, path: "a.md" });
    expect(back.status).toBe(200);
    expect(await back.json()).toMatchObject({ ok: true, path: "a.md" });
    expect(fs.readFileSync(path.join(opsDir, "a.md"), "utf-8")).toBe("one\nroute-single\n");
  });

  it("refuses to take back a dirty file, unknown paths and unsafe paths", async () => {
    const dirty = await post(app, "/api/git/unstash", { dir: opsDir, path: "a.md" });
    expect(dirty.status).toBe(400);
    expect((await dirty.json()).code).toBe("path_dirty");

    const missing = await post(app, "/api/git/unstash", { dir: opsDir, path: "nope.txt" });
    expect(missing.status).toBe(400);
    expect((await missing.json()).code).toBe("not_in_stash");

    const badStash = await post(app, "/api/git/stash", { dir: opsDir, paths: ["../escape"] });
    expect(badStash.status).toBe(400);
    expect((await badStash.json()).code).toBe("invalid_path");

    const badDiscard = await post(app, "/api/git/discard", { dir: opsDir, paths: ["/etc/passwd"] });
    expect(badDiscard.status).toBe(400);
    expect((await badDiscard.json()).code).toBe("invalid_path");
  });

  it("discards a single file and reports nothing_to_discard afterwards", async () => {
    const discarded = await post(app, "/api/git/discard", { dir: opsDir, paths: ["a.md"] });
    expect(discarded.status).toBe(200);
    expect(await discarded.json()).toMatchObject({ ok: true, paths: ["a.md"] });
    expect(fs.readFileSync(path.join(opsDir, "a.md"), "utf-8")).toBe("one\n");

    const again = await post(app, "/api/git/discard", { dir: opsDir, paths: ["a.md"] });
    expect(again.status).toBe(400);
    expect((await again.json()).code).toBe("nothing_to_discard");
  });

  it("pops the stash stack and reports no_stash once it is empty", async () => {
    let guard = 0;
    for (;;) {
      const list = await (await get(app, `/api/git/stashes?dir=${encodeURIComponent(opsDir)}`)).json();
      if (list.stashes.length === 0) break;
      // 先回退残留改动：整条弹出是三方合并，工作区脏了会被 git 拒绝
      await post(app, "/api/git/discard", { dir: opsDir });
      const popped = await post(app, "/api/git/unstash", { dir: opsDir });
      expect(popped.status).toBe(200);
      guard += 1;
      expect(guard).toBeLessThan(10);
    }
    const none = await post(app, "/api/git/unstash", { dir: opsDir });
    expect(none.status).toBe(400);
    expect((await none.json()).code).toBe("no_stash");
  });

  it("rejects unapproved dirs and missing names on the new endpoints", async () => {
    const outside = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-route-outside-"));
    try {
      for (const [url, payload] of [
        ["/api/git/create-branch", { dir: outside, name: "x" }],
        ["/api/git/stash", { dir: outside }],
        ["/api/git/worktree-create", { dir: outside, name: "x" }],
      ] as [string, Record<string, unknown>][]) {
        const res = await post(app, url, payload);
        expect(res.status).toBe(400);
        expect((await res.json()).error).toBe("invalid dir");
      }
      const noName = await post(app, "/api/git/worktree-create", { dir: opsDir });
      expect(noName.status).toBe(400);
      expect((await noName.json()).error).toBe("name required");
    } finally {
      fs.rmSync(outside, { recursive: true, force: true });
    }
  });
});
