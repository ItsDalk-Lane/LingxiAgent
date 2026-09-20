/**
 * git-command — 解析器纯函数单测 + 真实临时 git 仓库集成测试
 *
 * 集成部分在临时目录里跑真实 git（init/commit/worktree），锁定
 * collectGitStatus / worktreeInfo / fileDiff / commitChanges / pushChanges
 * 以及 createBranch / stashChanges / createWorktree 的行为契约：环境信息卡的
 * 各行数据与操作全部来自这些函数。
 */
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  collectGitStatus,
  commitChanges,
  amendCommit,
  stagePaths,
  unstagePaths,
  checkoutBranch,
  createBranch,
  createWorktree,
  discardPaths,
  fileDiff,
  isSafeRelPath,
  isValidBranchName,
  isValidWorktreeName,
  listBranches,
  listCommits,
  listCommitStats,
  listStashes,
  listWorktrees,
  parseForEachBranchRef,
  parseLogRecords,
  parseNumstatZ,
  parseWorktreePorcelain,
  popStash,
  pullChanges,
  pushChanges,
  restoreStashedPath,
  runGit,
  stashChanges,
  tryGit,
  worktreeInfo,
} from "../server/git/git-command.ts";

const execFileAsync = promisify(execFile);

async function git(dir: string, args: string[]): Promise<void> {
  await execFileAsync("git", args, { cwd: dir });
}

function write(dir: string, rel: string, content: string): void {
  const abs = path.join(dir, ...rel.split("/"));
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, content);
}

let mainDir = "";
let linkedDir = "";

beforeAll(async () => {
  mainDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-cmd-main-"));
  linkedDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-cmd-linked-"));

  try {
    await git(mainDir, ["init", "-b", "main"]);
  } catch {
    await git(mainDir, ["init"]);
  }
  await git(mainDir, ["config", "user.name", "Lingxi Test"]);
  await git(mainDir, ["config", "user.email", "test@lingxi.local"]);

  write(mainDir, "a.md", "line1\nline2\n");
  await git(mainDir, ["add", "-A"]);
  await git(mainDir, ["commit", "-m", "init"]);

  // 未暂存修改：line2 改写 + 追加两行 → +3 -1
  write(mainDir, "a.md", "line1\nline2x\nline3\nline4\n");
  // 已暂存新增：b.md 两行 → +2
  write(mainDir, "b.md", "hello\nworld\n");
  await git(mainDir, ["add", "b.md"]);
  // 未跟踪：c.md 三行 → +3
  write(mainDir, "c.md", "a\nb\nc\n");

  await git(mainDir, ["branch", "feature"]);
  await git(mainDir, ["worktree", "add", linkedDir, "-b", "wt-branch"]);
});

afterAll(() => {
  // 临时仓无需 worktree prune，直接整体删除
  fs.rmSync(mainDir, { recursive: true, force: true });
  fs.rmSync(linkedDir, { recursive: true, force: true });
});

// ────────────────────────── 解析器 ──────────────────────────

describe("parseNumstatZ", () => {
  it("parses add/del/path records including binary", () => {
    const out = parseNumstatZ("3\t1\tsrc/a.ts\u0000-\t-\timg/logo.png\u000010\t0\tb.md\u0000");
    expect(out).toEqual([
      { additions: 3, deletions: 1, path: "src/a.ts", binary: false },
      { additions: 0, deletions: 0, path: "img/logo.png", binary: true },
      { additions: 10, deletions: 0, path: "b.md", binary: false },
    ]);
  });

  it("returns empty for empty output", () => {
    expect(parseNumstatZ("")).toEqual([]);
  });
});

describe("parseForEachBranchRef", () => {
  it("marks the current branch via the * marker", () => {
    const branches = parseForEachBranchRef("*\u0000feat/x\n\u0000main\n\u0000wt-branch\n");
    expect(branches).toEqual([
      { name: "feat/x", current: true, checkedOutElsewhere: false },
      { name: "main", current: false, checkedOutElsewhere: false },
      { name: "wt-branch", current: false, checkedOutElsewhere: false },
    ]);
  });

  it("returns empty for empty output", () => {
    expect(parseForEachBranchRef("")).toEqual([]);
  });
});

describe("parseWorktreePorcelain", () => {
  it("parses blocks, first being the main worktree", () => {
    const entries = parseWorktreePorcelain(
      [
        "worktree /repo/main",
        "HEAD 1111111111111111111111111111111111111111",
        "branch refs/heads/main",
        "",
        "worktree /repo/linked",
        "HEAD 2222222222222222222222222222222222222222",
        "branch refs/heads/wt-branch",
        "detached",
        "",
      ].join("\n"),
    );
    expect(entries).toHaveLength(2);
    expect(entries[0]).toMatchObject({ path: "/repo/main", branch: "main", bare: false });
    expect(entries[1]).toMatchObject({ path: "/repo/linked", branch: "wt-branch", detached: true });
  });
});

describe("parseLogRecords", () => {
  it("parses fields, full message, refs decorations and multi-parents (NUL separated, record separator \\x1e)", () => {
    const record = (hash: string, short: string, subject: string, message: string, refs: string, parents: string) =>
      `${hash}\u0000${short}\u0000${subject}\u0000${message}\u0000张三\u00001700000000\u0000${refs}\u0000${parents}\x1e`;
    const out = [
      record("aaa111", "aaa1", "feat: 头", "feat: 头\n\n- 要点一\n- 要点二\n", "HEAD -> main, origin/main, tag: v1.0", "bbb222 ccc333"),
      record("bbb222", "bbb2", "chore: 中", "chore: 中", "", "ddd444"),
      record("ddd444", "ddd4", "init", "init\n", "", ""),
    ].join("\n");
    const commits = parseLogRecords(out);
    expect(commits).toHaveLength(3);
    expect(commits[0]).toMatchObject({
      hash: "aaa111", shortHash: "aaa1", subject: "feat: 头",
      message: "feat: 头\n\n- 要点一\n- 要点二",
      authorName: "张三", committedAt: 1700000000,
      parents: ["bbb222", "ccc333"],
    });
    expect(commits[0].refs).toEqual([
      { kind: "head", name: "main" },
      { kind: "remote", name: "origin/main" },
      { kind: "tag", name: "v1.0" },
    ]);
    expect(commits[1].refs).toEqual([]);
    expect(commits[1].message).toBe("chore: 中");
    expect(commits[2].parents).toEqual([]);
  });

  it("treats bare names as branches and skips malformed records", () => {
    const out = "aaa\u0000a1\u0000s\u0000s\u0000n\u00001700000001\u0000dev\u0000\u001e\u0000garbage";
    const commits = parseLogRecords(out);
    expect(commits).toHaveLength(1);
    expect(commits[0].refs).toEqual([{ kind: "branch", name: "dev" }]);
  });
});

describe("isSafeRelPath / isValidBranchName", () => {
  it("rejects traversal, absolute, backslash and leading-dash paths", () => {
    expect(isSafeRelPath("a/b.txt")).toBe(true);
    expect(isSafeRelPath("a b.txt")).toBe(true);
    for (const bad of ["../x", "a/../b", "/etc/passwd", "a\\b", "-flag", "", "."]) {
      expect(isSafeRelPath(bad)).toBe(false);
    }
  });

  it("rejects option-like branch names", () => {
    expect(isValidBranchName("feat/x")).toBe(true);
    expect(isValidBranchName("--exec=evil")).toBe(false);
    expect(isValidBranchName("has space")).toBe(false);
    expect(isValidBranchName("a..b")).toBe(false);
  });
});

// ────────────────────────── 真实仓库集成 ──────────────────────────

describe("collectGitStatus (real repo)", () => {
  it("sums staged + unstaged + untracked with per-file detail", async () => {
    const status = await collectGitStatus(mainDir);
    expect(status.isRepo).toBe(true);
    expect(status.currentBranch).toBe("main");
    expect(status.detached).toBe(false);
    expect(status.total).toEqual({ additions: 8, deletions: 1 });
    expect(status.stagedTotal).toEqual({ additions: 2, deletions: 0 });
    expect(status.unstagedTotal).toEqual({ additions: 6, deletions: 1 });
    expect(status.commitable).toBe(true);

    const byPath = new Map(status.files.map(f => [f.path, f]));
    expect(byPath.get("a.md")).toMatchObject({ additions: 3, deletions: 1, state: "modified", staged: false });
    expect(byPath.get("b.md")).toMatchObject({ additions: 2, deletions: 0, state: "added", staged: true });
    expect(byPath.get("c.md")).toMatchObject({ additions: 3, deletions: 0, state: "untracked", staged: false });
  });

  it("reports no remote → not pushable", async () => {
    const status = await collectGitStatus(mainDir);
    expect(status.hasRemote).toBe(false);
    expect(status.pushable).toBe(false);
  });

  it("returns isRepo:false placeholder for a non-git directory", async () => {
    const plain = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-plain-"));
    try {
      const status = await collectGitStatus(plain);
      expect(status.isRepo).toBe(false);
      expect(status.files).toEqual([]);
      expect(status.commitable).toBe(false);
    } finally {
      fs.rmSync(plain, { recursive: true, force: true });
    }
  });
});

describe("worktreeInfo (real repo)", () => {
  it("identifies the main worktree", async () => {
    const info = await worktreeInfo(mainDir);
    expect(info.isRepo).toBe(true);
    expect(info.isMain).toBe(true);
    expect(info.name).toBeNull();
  });

  it("identifies a linked worktree with its branch as name", async () => {
    const info = await worktreeInfo(linkedDir);
    expect(info.isRepo).toBe(true);
    expect(info.isMain).toBe(false);
    expect(info.name).toBe("wt-branch");
    expect(info.mainPath).toBeTruthy();
  });

  it("marks branches checked out in another worktree", async () => {
    const { branches, current, detached } = await listBranches(mainDir);
    expect(current).toBe("main");
    expect(detached).toBe(false);
    const byName = new Map(branches.map(b => [b.name, b]));
    expect(byName.get("wt-branch")).toMatchObject({ current: false, checkedOutElsewhere: true });
    expect(byName.get("feature")).toMatchObject({ current: false, checkedOutElsewhere: false });
    expect(byName.get("main")).toMatchObject({ current: true, checkedOutElsewhere: false });
  });
});

describe("fileDiff (real repo)", () => {
  it("returns unified patch for a modified tracked file", async () => {
    const diff = await fileDiff(mainDir, "a.md");
    expect(diff.binary).toBe(false);
    expect(diff.patch).toContain("-line2");
    expect(diff.patch).toContain("+line2x");
    expect(diff.patch).toContain("+line4");
  });

  it("synthesizes a new-file patch for an untracked file", async () => {
    const diff = await fileDiff(mainDir, "c.md");
    expect(diff.binary).toBe(false);
    expect(diff.patch).toContain("new file mode");
    expect(diff.patch).toContain("+a");
    expect(diff.patch).toContain("+c");
    expect(diff.patch).toContain("@@ -0,0 +1,3 @@");
  });

  it("rejects unsafe paths", async () => {
    await expect(fileDiff(mainDir, "../outside.txt")).rejects.toThrow(/invalid file path/);
    await expect(fileDiff(mainDir, "-flag")).rejects.toThrow(/invalid file path/);
  });
});

describe("checkout / commit / push (real repo)", () => {
  it("switches branch and reports it via status", async () => {
    await checkoutBranch(mainDir, "feature");
    const status = await collectGitStatus(mainDir);
    expect(status.currentBranch).toBe("feature");
    await checkoutBranch(mainDir, "main");
  });

  it("refuses branch names that do not exist", async () => {
    await expect(checkoutBranch(mainDir, "no-such-branch")).rejects.toThrow(/branch not found/);
  });

  it("commits everything when includeUnstaged, leaving a clean tree", async () => {
    const result = await commitChanges(mainDir, "test: 环境信息卡提交", true);
    expect(result.ok).toBe(true);
    expect(result.head).toMatch(/^[0-9a-f]{7,40}$/);
    const status = await collectGitStatus(mainDir);
    expect(status.files).toEqual([]);
    expect(status.commitable).toBe(false);
  });

  it("returns nothing_staged when there is nothing to commit", async () => {
    const result = await commitChanges(mainDir, "empty", true);
    expect(result.ok).toBe(false);
    expect(result.code).toBe("nothing_to_commit");
  });

  it("push reports no_remote without any remote configured", async () => {
    const result = await pushChanges(mainDir);
    expect(result.ok).toBe(false);
    expect(result.code).toBe("no_remote");
  });

  it("runGit surfaces stderr in GitError", async () => {
    await expect(runGit(mainDir, ["checkout", "no-such-branch"])).rejects.toThrow(/git checkout failed/);
  });

  it("lists commits newest-first with HEAD ref, full message and parent linkage", async () => {
    const commits = await listCommits(mainDir, 50);
    expect(commits.length).toBeGreaterThanOrEqual(2);
    const head = commits[0];
    expect(head.refs.some(r => r.kind === "head")).toBe(true);
    expect(head.message).toContain(head.subject);
    // 线性区间父子衔接
    const second = commits[1];
    expect(head.parents[0]).toBe(second.hash);
    expect(head.shortHash).toMatch(/^[0-9a-f]{7,}$/);
    // 元数据查询不含统计（两段加载：统计走 listCommitStats）
    expect(head.additions).toBe(0);
    expect(head.deletions).toBe(0);
    expect(head.changedFiles).toBe(0);
  });

  it("listCommitStats batch-fetches per-commit stats keyed by hash with cache", async () => {
    const commits = await listCommits(mainDir, 5);
    expect(commits.length).toBeGreaterThan(0);
    const stats = await listCommitStats(mainDir, commits.map(c => c.hash));
    for (const c of commits) {
      const s = stats.get(c.hash);
      expect(s).toBeDefined();
      expect(s!.additions).toBeGreaterThanOrEqual(0);
      expect(s!.deletions).toBeGreaterThanOrEqual(0);
      expect(s!.changedFiles).toBeGreaterThan(0);
    }
    // 非 40 位十六进制的输入被忽略；命中缓存的哈希仍返回
    const again = await listCommitStats(mainDir, ["not-a-hash", "--upload-pack=evil", commits[0].hash]);
    expect(again.has(commits[0].hash)).toBe(true);
    expect(again.size).toBe(1);
    // 空输入不触发 git 调用，直接返回空表
    expect((await listCommitStats(mainDir, [])).size).toBe(0);
  });
});

/**
 * stagePaths / unstagePaths / amendCommit 契约：Git图谱 面板的暂存与
 * 提交（修改）底座。独立临时仓，不与上方共享状态。
 */
describe("stagePaths / unstagePaths / amendCommit (real repo)", () => {
  let dir = "";

  async function stagedPaths(): Promise<string[]> {
    const res = await tryGit(dir, ["-c", "core.quotepath=false", "diff", "--cached", "--name-only"]);
    return res.ok ? res.stdout.split("\n").map(s => s.trim()).filter(Boolean) : [];
  }

  beforeAll(async () => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-cmd-stage-"));
    try {
      await git(dir, ["init", "-b", "main"]);
    } catch {
      await git(dir, ["init"]);
    }
    await git(dir, ["config", "user.name", "Lingxi Test"]);
    await git(dir, ["config", "user.email", "test@lingxi.local"]);
    write(dir, "a.md", "one\n");
    write(dir, "b.md", "bee\n");
    await git(dir, ["add", "-A"]);
    await git(dir, ["commit", "-m", "init"]);

    // 工作树脏状态：a.md 未暂存修改 + c.md 未跟踪
    write(dir, "a.md", "one\ntwo\n");
    write(dir, "c.md", "cee\n");
  });

  afterAll(() => {
    if (dir) fs.rmSync(dir, { recursive: true, force: true });
  });

  it("stages everything (including untracked) with no pathspec", async () => {
    const result = await stagePaths(dir);
    expect(result).toEqual({ ok: true, paths: [] });
    expect(await stagedPaths()).toEqual(expect.arrayContaining(["a.md", "c.md"]));
    const status = await collectGitStatus(dir);
    expect(status.files.every(f => f.staged)).toBe(true);
  });

  it("unstages everything back to HEAD, keeping worktree contents", async () => {
    const result = await unstagePaths(dir);
    expect(result).toEqual({ ok: true, paths: [] });
    expect(await stagedPaths()).toEqual([]);
    // a.md 的修改与 c.md 仍未跟踪地留在工作树
    const status = await collectGitStatus(dir);
    expect(status.unstagedTotal.additions).toBeGreaterThan(0);
  });

  it("stages a single path only", async () => {
    const result = await stagePaths(dir, ["a.md"]);
    expect(result).toEqual({ ok: true, paths: ["a.md"] });
    expect(await stagedPaths()).toEqual(["a.md"]);
    await unstagePaths(dir, ["a.md"]);
    expect(await stagedPaths()).toEqual([]);
  });

  it("rejects invalid pathspecs", async () => {
    expect(await stagePaths(dir, ["../escape"])).toEqual({ ok: false, code: "invalid_path" });
    expect(await unstagePaths(dir, ["../escape"])).toEqual({ ok: false, code: "invalid_path" });
  });

  it("amends with the staged change, keeping the original message when empty", async () => {
    write(dir, "a.md", "one\ntwo\nthree\n");
    await stagePaths(dir, ["a.md"]);
    const before = (await listCommits(dir, 1))[0];

    const result = await amendCommit(dir, null);
    expect(result.ok).toBe(true);
    expect(result.head).toMatch(/^[0-9a-f]{7,40}$/);

    const after = (await listCommits(dir, 1))[0];
    expect(after.hash).not.toBe(before.hash);
    expect(after.message).toBe(before.message); // --no-edit 保留原信息
    expect(after.parents).toEqual(before.parents); // 不新增提交
    // 暂存内容已并入：暂存区干净，工作树也不再有 a.md 的改动
    expect(await stagedPaths()).toEqual([]);
  });

  it("amend replaces the message when one is given", async () => {
    const result = await amendCommit(dir, "fix: 改写提交信息");
    expect(result.ok).toBe(true);
    expect((await listCommits(dir, 1))[0].subject).toBe("fix: 改写提交信息");
  });

  it("refuses to amend a repo without any commit", async () => {
    const empty = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-cmd-amend-empty-"));
    try {
      await git(empty, ["init"]);
      const result = await amendCommit(empty, "x");
      expect(result).toMatchObject({ ok: false, code: "nothing_to_commit" });
    } finally {
      fs.rmSync(empty, { recursive: true, force: true });
    }
  });

  it("unstages on a zero-commit repo by clearing the index, not the file", async () => {
    const empty = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-cmd-unstage-empty-"));
    try {
      await git(empty, ["init"]);
      await git(empty, ["config", "user.name", "Lingxi Test"]);
      await git(empty, ["config", "user.email", "test@lingxi.local"]);
      write(empty, "only.md", "data\n");
      await stagePaths(empty);
      // 零提交仓库探测暂存区用 ls-files（diff --cached 依赖 HEAD）
      const lsFiles = async (): Promise<string[]> => {
        const res = await tryGit(empty, ["ls-files"]);
        return res.ok ? res.stdout.split("\n").map(s => s.trim()).filter(Boolean) : [];
      };
      expect(await lsFiles()).toEqual(["only.md"]);

      const result = await unstagePaths(empty);
      expect(result).toEqual({ ok: true, paths: [] });
      expect(await lsFiles()).toEqual([]);
      // 文件本体还在，退成未跟踪
      expect(fs.existsSync(path.join(empty, "only.md"))).toBe(true);
      const status = await collectGitStatus(empty);
      expect(status.files[0]).toMatchObject({ path: "only.md", state: "untracked" });
    } finally {
      fs.rmSync(empty, { recursive: true, force: true });
    }
  });

  it("classifies states by real name-status: pure-deletion edits are modified, not deleted", async () => {
    // 用户场景复现：只删掉一行内容（0 增 1 删），文件还在 → 必须是 modified 而不是 deleted
    write(dir, "a.md", "one\ntwo\n");
    await stagePaths(dir, ["a.md"]);

    let entry = (await collectGitStatus(dir)).files.find(f => f.path === "a.md");
    expect(entry).toMatchObject({ staged: true, state: "modified" });

    await unstagePaths(dir, ["a.md"]);
    entry = (await collectGitStatus(dir)).files.find(f => f.path === "a.md");
    expect(entry).toMatchObject({ staged: false, state: "modified" });

    // 真删除：worktree 里把 b.md 整个删掉 → 未暂存 deleted
    fs.rmSync(path.join(dir, "b.md"));
    entry = (await collectGitStatus(dir)).files.find(f => f.path === "b.md");
    expect(entry).toMatchObject({ staged: false, state: "deleted" });
  });
});

/**
 * pullChanges 契约：本地路径远端 + 真实 clone，锁定各结构化分桶。
 * it 之间有状态依赖（同一 clone 递进），vitest 同文件内顺序执行。
 */
describe("pullChanges (real repo)", () => {
  let originDir = "";
  let cloneDir = "";

  beforeAll(async () => {
    originDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-pull-origin-"));
    cloneDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-pull-clone-"));

    try {
      await git(originDir, ["init", "-b", "main"]);
    } catch {
      await git(originDir, ["init"]);
    }
    await git(originDir, ["config", "user.name", "Lingxi Test"]);
    await git(originDir, ["config", "user.email", "test@lingxi.local"]);
    write(originDir, "seed.md", "v1\n");
    await git(originDir, ["add", "-A"]);
    await git(originDir, ["commit", "-m", "init"]);

    await execFileAsync("git", ["clone", originDir, cloneDir]);
    await git(cloneDir, ["config", "user.name", "Lingxi Test"]);
    await git(cloneDir, ["config", "user.email", "test@lingxi.local"]);
  });

  afterAll(() => {
    fs.rmSync(originDir, { recursive: true, force: true });
    fs.rmSync(cloneDir, { recursive: true, force: true });
  });

  it("reports no_remote without any remote configured", async () => {
    const result = await pullChanges(mainDir);
    expect(result.ok).toBe(false);
    expect(result.code).toBe("no_remote");
  });

  it("reports already_up_to_date when the remote has nothing new", async () => {
    const result = await pullChanges(cloneDir);
    expect(result.ok).toBe(false);
    expect(result.code).toBe("already_up_to_date");
  });

  it("fast-forwards and reports the pulled commit count", async () => {
    write(originDir, "news.txt", "from remote\n");
    await git(originDir, ["add", "-A"]);
    await git(originDir, ["commit", "-m", "remote update"]);

    const result = await pullChanges(cloneDir);
    expect(result.ok).toBe(true);
    expect(result.pulled).toBe(1);
    expect(fs.readFileSync(path.join(cloneDir, "news.txt"), "utf8")).toContain("from remote");
  });

  it("returns local_changes when dirty files block the fast-forward", async () => {
    // clone 侧 news.txt 有未提交改动，origin 又改了同一文件
    write(cloneDir, "news.txt", "local edit\n");
    write(originDir, "news.txt", "remote edit\n");
    await git(originDir, ["add", "-A"]);
    await git(originDir, ["commit", "-m", "remote news"]);

    const result = await pullChanges(cloneDir);
    expect(result.ok).toBe(false);
    expect(result.code).toBe("local_changes");
    // 本地改动未被覆盖
    expect(fs.readFileSync(path.join(cloneDir, "news.txt"), "utf8")).toContain("local edit");

    // 收尾：恢复干净，给下一个用例留干净树
    await git(cloneDir, ["checkout", "--", "news.txt"]);
    const result2 = await pullChanges(cloneDir);
    expect(result2.ok).toBe(true);
  });

  it("returns diverged instead of creating a merge commit", async () => {
    await git(cloneDir, ["commit", "--allow-empty", "-m", "local only"]);
    await git(originDir, ["commit", "--allow-empty", "-m", "remote only"]);

    const result = await pullChanges(cloneDir);
    expect(result.ok).toBe(false);
    expect(result.code).toBe("diverged");
    // 没有自动造出 merge 提交：HEAD 仍是本地那条空提交
    const log = await execFileAsync("git", ["log", "-1", "--pretty=%s"], { cwd: cloneDir });
    expect(log.stdout.trim()).toBe("local only");
  });
});

// ────────────────────────── 分支创建 / 暂存 / 工作树 ──────────────────────────

/**
 * 独立临时仓（父级目录也独立）：worktree 落地根是 <主工作树父级>/worktrees，
 * 共用 os.tmpdir() 会让并行测试文件互相踩，所以仓建在自己的一层父目录下。
 */
describe("createBranch / stashChanges / worktrees (real repo)", () => {
  let parentDir = "";
  let opsDir = "";
  let worktreesRoot = "";

  beforeAll(async () => {
    parentDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-cmd-ops-"));
    opsDir = path.join(parentDir, "repo");
    fs.mkdirSync(opsDir);

    try {
      await git(opsDir, ["init", "-b", "main"]);
    } catch {
      await git(opsDir, ["init"]);
    }
    await git(opsDir, ["config", "user.name", "Lingxi Test"]);
    await git(opsDir, ["config", "user.email", "test@lingxi.local"]);
    write(opsDir, "a.md", "one\n");
    await git(opsDir, ["add", "-A"]);
    await git(opsDir, ["commit", "-m", "init"]);

    const list = await listWorktrees(opsDir);
    worktreesRoot = list.root ?? "";
  });

  afterAll(() => {
    // 仓、worktree、worktrees/ 都在这个父目录下，整体删干净
    if (parentDir) fs.rmSync(parentDir, { recursive: true, force: true });
  });

  it("isValidWorktreeName accepts slugs and rejects separators / dotfiles", () => {
    expect(isValidWorktreeName("fix-login-race")).toBe(true);
    expect(isValidWorktreeName("v1.2_x-3")).toBe(true);
    expect(isValidWorktreeName("-nope")).toBe(false);
    expect(isValidWorktreeName(".hidden")).toBe(false);
    expect(isValidWorktreeName("a/b")).toBe(false);
    expect(isValidWorktreeName("../escape")).toBe(false);
    expect(isValidWorktreeName("")).toBe(false);
  });

  it("creates a branch and checks it out", async () => {
    const result = await createBranch(opsDir, "feat/new-thing");
    expect(result).toEqual({ ok: true, branch: "feat/new-thing" });
    expect((await listBranches(opsDir)).current).toBe("feat/new-thing");
  });

  it("rejects existing branches, option-like names and missing bases", async () => {
    expect(await createBranch(opsDir, "main")).toEqual({
      ok: false, code: "already_exists", branch: "main",
    });
    expect(await createBranch(opsDir, "--upload-pack=evil")).toMatchObject({
      ok: false, code: "invalid_name",
    });
    expect(await createBranch(opsDir, "from-missing", "no-such-base")).toMatchObject({
      ok: false, code: "invalid_base",
    });
  });

  it("stashes tracked + untracked changes and reports nothing_to_stash on a clean tree", async () => {
    expect(await stashChanges(opsDir)).toEqual({ ok: false, code: "nothing_to_stash" });

    write(opsDir, "a.md", "one\ntwo\n");
    write(opsDir, "untracked.txt", "x\n");
    expect(await stashChanges(opsDir, "wip: demo")).toEqual({ ok: true });

    const status = await collectGitStatus(opsDir);
    expect(status.files).toEqual([]);
    expect(status.commitable).toBe(false);
  });

  it("lists worktrees with main/current markers and the shared landing root", async () => {
    const list = await listWorktrees(opsDir);
    expect(list.isRepo).toBe(true);
    expect(list.mainPath).toBeTruthy();
    expect(worktreesRoot).toBe(path.join(path.dirname(list.mainPath!), "worktrees"));
    expect(list.worktrees).toHaveLength(1);
    expect(list.worktrees[0]).toMatchObject({ isMain: true, current: true });
  });

  it("creates an isolated worktree on wt/<name> without touching the current checkout", async () => {
    const before = await listBranches(opsDir);
    const created = await createWorktree(opsDir, "fix-login-race");

    expect(created.ok).toBe(true);
    expect(created.branch).toBe("wt/fix-login-race");
    expect(created.path).toBe(path.join(worktreesRoot, "fix-login-race"));
    expect(fs.existsSync(path.join(created.path!, "a.md"))).toBe(true);

    // 当前检出不受影响：仍在原来的分支上
    expect((await listBranches(opsDir)).current).toBe(before.current);

    const linked = (await listWorktrees(opsDir)).worktrees
      .find(entry => entry.branch === "wt/fix-login-race");
    expect(linked).toMatchObject({ isMain: false, current: false });

    // 新分支确实落在新工作树里
    const inWorktree = await listBranches(created.path!);
    expect(inWorktree.current).toBe("wt/fix-login-race");
  });

  it("creates the worktree from an explicit base branch", async () => {
    const created = await createWorktree(opsDir, "from-base", "main");
    expect(created).toMatchObject({ ok: true, branch: "wt/from-base" });
    const inWorktree = await listBranches(created.path!);
    expect(inWorktree.current).toBe("wt/from-base");
  });

  it("refuses duplicate directory names, duplicate branches and bad names", async () => {
    expect(await createWorktree(opsDir, "fix-login-race")).toMatchObject({
      ok: false, code: "exists",
    });
    await git(opsDir, ["branch", "wt/taken"]);
    expect(await createWorktree(opsDir, "taken")).toMatchObject({
      ok: false, code: "branch_exists",
    });
    expect(await createWorktree(opsDir, "../escape")).toMatchObject({
      ok: false, code: "invalid_name",
    });
    expect(await createWorktree(opsDir, "bad-base", "no-such-base")).toMatchObject({
      ok: false, code: "invalid_base",
    });
  });

  it("returns the isRepo:false placeholder for a non-git directory", async () => {
    const plain = path.join(parentDir, "not-a-repo");
    fs.mkdirSync(plain);
    expect(await listWorktrees(plain)).toEqual({
      isRepo: false, worktrees: [], root: null, mainPath: null,
    });
    expect(await createWorktree(plain, "nope")).toMatchObject({ ok: false, code: "create_failed" });
  });

  it("stashes a single path and takes it back without touching the others", async () => {
    write(opsDir, "one.txt", "one\n");
    write(opsDir, "two.txt", "two\n");
    await git(opsDir, ["add", "-A"]);
    await git(opsDir, ["commit", "-m", "two files"]);
    write(opsDir, "one.txt", "one\nCHANGED\n");
    write(opsDir, "two.txt", "two\nCHANGED\n");

    const stashed = await stashChanges(opsDir, "wip one", ["one.txt"]);
    expect(stashed).toMatchObject({ ok: true, paths: ["one.txt"] });
    // 只收走了 one.txt，two.txt 仍在工作区
    expect((await collectGitStatus(opsDir)).files.map(f => f.path)).toEqual(["two.txt"]);

    const { stashes } = await listStashes(opsDir);
    expect(stashes[0]).toMatchObject({ tracked: ["one.txt"], untracked: [] });

    const back = await restoreStashedPath(opsDir, "one.txt");
    expect(back).toMatchObject({ ok: true, path: "one.txt", stash: stashes[0].ref });

    const restored = (await collectGitStatus(opsDir)).files.find(f => f.path === "one.txt");
    expect(restored?.staged).toBe(false);
    expect(fs.readFileSync(path.join(opsDir, "one.txt"), "utf-8")).toContain("CHANGED");
    // 单个取出不动储藏条目本身
    expect((await listStashes(opsDir)).stashes).toHaveLength(stashes.length);
  });

  it("takes back stashed untracked files and refuses dirty / unknown / unsafe paths", async () => {
    write(opsDir, "fresh.txt", "brand new\n");
    expect(await stashChanges(opsDir, "wip fresh", ["fresh.txt"])).toMatchObject({ ok: true });
    expect(fs.existsSync(path.join(opsDir, "fresh.txt"))).toBe(false);

    const { stashes } = await listStashes(opsDir);
    expect(stashes[0]).toMatchObject({ tracked: [], untracked: ["fresh.txt"] });
    expect(await restoreStashedPath(opsDir, "fresh.txt")).toMatchObject({ ok: true, path: "fresh.txt" });
    expect(fs.readFileSync(path.join(opsDir, "fresh.txt"), "utf-8")).toBe("brand new\n");
    // 取回后是未跟踪状态，不是被悄悄暂存
    expect((await collectGitStatus(opsDir)).files.find(f => f.path === "fresh.txt"))
      .toMatchObject({ state: "untracked", staged: false });

    // 现行改动会被覆盖 → 拒绝
    expect(await restoreStashedPath(opsDir, "two.txt")).toMatchObject({ ok: false, code: "path_dirty" });
    // 不在任何储藏里
    expect(await restoreStashedPath(opsDir, "b.md")).toMatchObject({ ok: false, code: "not_in_stash" });
    // 非法路径：暂存 / 取出 / 回退 三处都拦
    expect(await restoreStashedPath(opsDir, "../escape")).toMatchObject({ ok: false, code: "invalid_path" });
    expect(await stashChanges(opsDir, null, ["-oops"])).toMatchObject({ ok: false, code: "invalid_path" });
    expect(await discardPaths(opsDir, ["/etc/passwd"])).toMatchObject({ ok: false, code: "invalid_path" });
  });

  it("discards one file back to HEAD even when it was staged", async () => {
    write(opsDir, "disc.txt", "base\n");
    // 只提交这一个文件：one.txt / two.txt 的改动留着给「全部回退」用
    await git(opsDir, ["add", "disc.txt"]);
    await git(opsDir, ["commit", "-m", "disc base"]);
    write(opsDir, "disc.txt", "base\nstaged\n");
    await git(opsDir, ["add", "disc.txt"]);
    write(opsDir, "disc.txt", "base\nstaged\nunstaged\n");

    expect(await discardPaths(opsDir, ["disc.txt"])).toMatchObject({ ok: true, paths: ["disc.txt"] });
    expect(fs.readFileSync(path.join(opsDir, "disc.txt"), "utf-8")).toBe("base\n");
    expect((await collectGitStatus(opsDir)).files.find(f => f.path === "disc.txt")).toBeUndefined();

    // 已经没有可回退的改动
    expect(await discardPaths(opsDir, ["disc.txt"])).toMatchObject({ ok: false, code: "nothing_to_discard" });
  });

  it("discards one untracked file by deleting it", async () => {
    write(opsDir, "untracked-discard.txt", "temp\n");
    expect((await collectGitStatus(opsDir)).files.find(f => f.path === "untracked-discard.txt"))
      .toMatchObject({ state: "untracked" });

    expect(await discardPaths(opsDir, ["untracked-discard.txt"])).toMatchObject({ ok: true, paths: ["untracked-discard.txt"] });
    // 回退新文件 = 删除：文件不在了，状态里也没有了
    expect(fs.existsSync(path.join(opsDir, "untracked-discard.txt"))).toBe(false);
    expect((await collectGitStatus(opsDir)).files.find(f => f.path === "untracked-discard.txt")).toBeUndefined();
  });

  it("discards every change at once, deleting untracked files and directories too", async () => {
    write(opsDir, "leave-me.txt", "new\n");
    fs.mkdirSync(path.join(opsDir, "scratch-dir"), { recursive: true });
    write(opsDir, "scratch-dir/nested.txt", "nested\n");
    expect((await discardPaths(opsDir)).ok).toBe(true);

    const status = await collectGitStatus(opsDir);
    // 已跟踪改动 + 未跟踪文件/目录一起清掉
    expect(status.files).toEqual([]);
    expect(fs.existsSync(path.join(opsDir, "leave-me.txt"))).toBe(false);
    expect(fs.existsSync(path.join(opsDir, "scratch-dir"))).toBe(false);
    expect(fs.existsSync(path.join(opsDir, "fresh.txt"))).toBe(false);
    expect(fs.readFileSync(path.join(opsDir, "one.txt"), "utf-8")).toBe("one\n");
  });

  it("pops the stash stack newest-first and reports no_stash when empty", async () => {
    // 先把工作区彻底清干净：否则弹出含未跟踪文件的储藏会撞上同名文件
    for (const name of fs.readdirSync(opsDir)) {
      if (name === ".git") continue;
      fs.rmSync(path.join(opsDir, name), { recursive: true, force: true });
    }
    await git(opsDir, ["checkout", "--", "."]);

    const before = await listStashes(opsDir);
    expect(before.stashes.length).toBeGreaterThanOrEqual(2);

    let remaining = before.stashes.length;
    while (remaining > 0) {
      const newest = (await listStashes(opsDir)).stashes[0];
      expect((await popStash(opsDir)).ok).toBe(true);
      remaining -= 1;
      expect((await listStashes(opsDir)).stashes).toHaveLength(remaining);
      for (const rel of [...newest.tracked, ...newest.untracked]) {
        expect(fs.existsSync(path.join(opsDir, ...rel.split("/")))).toBe(true);
      }
    }
    expect(await popStash(opsDir)).toMatchObject({ ok: false, code: "no_stash" });
  });
});

describe("collectGitStatus untracked counting budget (real repo)", () => {
  let dir = "";
  beforeAll(async () => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-git-cmd-untracked-"));
    try {
      await git(dir, ["init", "-b", "main"]);
    } catch {
      await git(dir, ["init"]);
    }
    await git(dir, ["config", "user.name", "Lingxi Test"]);
    await git(dir, ["config", "user.email", "test@lingxi.local"]);
    write(dir, "tracked.md", "x\n");
    await git(dir, ["add", "-A"]);
    await git(dir, ["commit", "-m", "init"]);
    // 大量未跟踪产物文件：预算内的前段计数，末尾超出预算记 0（显式降级）
    for (let i = 0; i < 2010; i++) write(dir, `junk/junk-${String(i).padStart(4, "0")}.txt`, "line\nline2\n");
  });
  afterAll(() => fs.rmSync(dir, { recursive: true, force: true }));

  it("lists every untracked file but caps line counting within the budget", async () => {
    const status = await collectGitStatus(dir);
    const untracked = status.files.filter(f => f.state === "untracked");
    expect(untracked).toHaveLength(2010);
    // ls-files 输出按路径序：预算内开头有行数，超出预算的末尾为 0
    const first = untracked.find(f => f.path.endsWith("junk-0000.txt"));
    expect(first?.additions).toBeGreaterThan(0);
    const last = untracked.find(f => f.path.endsWith("junk-2009.txt"));
    expect(last?.additions).toBe(0);
  });
});
