/**
 * git-command — 解析器纯函数单测 + 真实临时 git 仓库集成测试
 *
 * 集成部分在临时目录里跑真实 git（init/commit/worktree），锁定
 * collectGitStatus / worktreeInfo / fileDiff / commitChanges / pushChanges
 * 的行为契约：环境信息卡的四行数据全部来自这些函数。
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
  checkoutBranch,
  fileDiff,
  isSafeRelPath,
  isValidBranchName,
  listBranches,
  listCommits,
  parseForEachBranchRef,
  parseLogRecords,
  parseNumstatZ,
  parseWorktreePorcelain,
  pushChanges,
  runGit,
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
  });
});
