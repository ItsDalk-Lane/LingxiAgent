import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it } from "vitest";

import { collectWorkspaceInstructionFiles, formatWorkspaceInstructionFiles } from "../core/workspace-instruction-files.ts";

const tempDirs: string[] = [];

afterEach(() => {
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

function temporaryDir() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-workspace-instructions-"));
  tempDirs.push(root);
  return root;
}

const bothEnabled = { inject_agents_md: true, inject_claude_md: true };

describe("workspace instruction files: excluding the agent's own persona files", () => {
  it("skips an excluded AGENTS.md so a session rooted in the agent directory does not inject the persona twice", () => {
    const agentDir = temporaryDir();
    fs.writeFileSync(path.join(agentDir, "AGENTS.md"), "persona prompt", "utf-8");

    const withoutExclusion = collectWorkspaceInstructionFiles({
      cwd: agentDir,
      workspaceContext: bothEnabled,
    });
    expect(withoutExclusion.map((file) => file.filename)).toEqual(["AGENTS.md"]);

    const withExclusion = collectWorkspaceInstructionFiles({
      cwd: agentDir,
      workspaceContext: bothEnabled,
      excludeFiles: [path.join(agentDir, "AGENTS.md")],
    });
    expect(withExclusion).toEqual([]);
  });

  it("leaves CLAUDE.md in the same directory untouched", () => {
    const agentDir = temporaryDir();
    fs.writeFileSync(path.join(agentDir, "AGENTS.md"), "persona prompt", "utf-8");
    fs.writeFileSync(path.join(agentDir, "CLAUDE.md"), "project rules", "utf-8");

    const files = collectWorkspaceInstructionFiles({
      cwd: agentDir,
      workspaceContext: bothEnabled,
      excludeFiles: [
        path.join(agentDir, "AGENTS.md"),
        path.join(agentDir, "AGENTS.public.md"),
      ],
    });

    expect(files.map((file) => file.filename)).toEqual(["CLAUDE.md"]);
    expect(files[0].content).toBe("project rules");
  });

  it("only excludes the named paths, not same-named files elsewhere in the directory chain", () => {
    const root = temporaryDir();
    // A .git marker makes the root the search root, so the walk covers both levels.
    fs.mkdirSync(path.join(root, ".git"), { recursive: true });
    fs.writeFileSync(path.join(root, "AGENTS.md"), "workspace rules", "utf-8");
    const agentDir = path.join(root, "nested");
    fs.mkdirSync(agentDir, { recursive: true });
    fs.writeFileSync(path.join(agentDir, "AGENTS.md"), "persona prompt", "utf-8");

    const files = collectWorkspaceInstructionFiles({
      cwd: agentDir,
      workspaceContext: bothEnabled,
      excludeFiles: [path.join(agentDir, "AGENTS.md")],
    });

    expect(files.map((file) => file.content)).toEqual(["workspace rules"]);
  });

  it("ignores empty and malformed exclusion entries rather than dropping every file", () => {
    const agentDir = temporaryDir();
    fs.writeFileSync(path.join(agentDir, "AGENTS.md"), "persona prompt", "utf-8");

    const files = collectWorkspaceInstructionFiles({
      cwd: agentDir,
      workspaceContext: bothEnabled,
      excludeFiles: ["", null as any, undefined as any],
    });

    expect(files.map((file) => file.filename)).toEqual(["AGENTS.md"]);
  });
});

describe("workspace instruction files: custom-named instruction file", () => {
  it("injects the custom-named file from the cwd, same search rule as AGENTS.md / CLAUDE.md", () => {
    const dir = temporaryDir();
    fs.writeFileSync(path.join(dir, "QWEN.md"), "custom rules", "utf-8");

    const files = collectWorkspaceInstructionFiles({
      cwd: dir,
      workspaceContext: {
        inject_custom_file: true,
        custom_file_name: "QWEN.md",
      },
    });

    expect(files).toHaveLength(1);
    expect(files[0].filename).toBe("QWEN.md");
    expect(files[0].custom).toBe(true);
    expect(files[0].content).toBe("custom rules");
  });

  it("picks up the custom-named file at every level of the directory chain, root first", () => {
    const root = temporaryDir();
    fs.mkdirSync(path.join(root, ".git"), { recursive: true });
    fs.writeFileSync(path.join(root, "QWEN.md"), "root rules", "utf-8");
    const nested = path.join(root, "nested");
    fs.mkdirSync(nested, { recursive: true });
    fs.writeFileSync(path.join(nested, "QWEN.md"), "nested rules", "utf-8");

    const files = collectWorkspaceInstructionFiles({
      cwd: nested,
      workspaceContext: {
        inject_custom_file: true,
        custom_file_name: "QWEN.md",
      },
    });

    expect(files.map((file) => file.content)).toEqual(["root rules", "nested rules"]);
  });

  it("does not inject while the toggle is off", () => {
    const dir = temporaryDir();
    fs.writeFileSync(path.join(dir, "QWEN.md"), "custom rules", "utf-8");

    const files = collectWorkspaceInstructionFiles({
      cwd: dir,
      workspaceContext: {
        inject_custom_file: false,
        custom_file_name: "QWEN.md",
      },
    });

    expect(files).toEqual([]);
  });

  it("silently skips a custom name that matches no file in the chain", () => {
    const dir = temporaryDir();
    fs.writeFileSync(path.join(dir, "AGENTS.md"), "workspace rules", "utf-8");

    const files = collectWorkspaceInstructionFiles({
      cwd: dir,
      workspaceContext: {
        inject_agents_md: true,
        inject_custom_file: true,
        custom_file_name: "NOPE.md",
      },
    });

    expect(files.map((file) => file.filename)).toEqual(["AGENTS.md"]);
  });

  it("rejects names containing path separators, which would break the chain search", () => {
    const dir = temporaryDir();
    fs.mkdirSync(path.join(dir, "sub"), { recursive: true });
    fs.writeFileSync(path.join(dir, "sub", "QWEN.md"), "custom rules", "utf-8");

    for (const badName of ["sub/QWEN.md", "..", ".", "sub\\QWEN.md"]) {
      const files = collectWorkspaceInstructionFiles({
        cwd: path.join(dir, "sub"),
        workspaceContext: {
          inject_custom_file: true,
          custom_file_name: badName,
        },
      });
      expect(files).toEqual([]);
    }
  });

  it("deduplicates when the custom name equals a fixed file name already enabled", () => {
    const dir = temporaryDir();
    fs.writeFileSync(path.join(dir, "AGENTS.md"), "workspace rules", "utf-8");

    const files = collectWorkspaceInstructionFiles({
      cwd: dir,
      workspaceContext: {
        inject_agents_md: true,
        inject_custom_file: true,
        custom_file_name: "AGENTS.md",
      },
    });

    expect(files).toHaveLength(1);
    expect(files[0].content).toBe("workspace rules");
  });

  it("honors the exclusion list for custom-named files too", () => {
    const dir = temporaryDir();
    const filePath = path.join(dir, "QWEN.md");
    fs.writeFileSync(filePath, "custom rules", "utf-8");

    const files = collectWorkspaceInstructionFiles({
      cwd: dir,
      workspaceContext: {
        inject_custom_file: true,
        custom_file_name: "QWEN.md",
      },
      excludeFiles: [filePath],
    });

    expect(files).toEqual([]);
  });

  it("appends custom-named files after AGENTS.md / CLAUDE.md and mentions them in the formatted header", () => {
    const dir = temporaryDir();
    fs.mkdirSync(path.join(dir, ".git"), { recursive: true });
    fs.writeFileSync(path.join(dir, "AGENTS.md"), "workspace rules", "utf-8");
    fs.writeFileSync(path.join(dir, "QWEN.md"), "custom rules", "utf-8");

    const files = collectWorkspaceInstructionFiles({
      cwd: dir,
      workspaceContext: {
        inject_agents_md: true,
        inject_custom_file: true,
        custom_file_name: "QWEN.md",
      },
    });

    expect(files.map((file) => file.filename)).toEqual(["AGENTS.md", "QWEN.md"]);

    const prompt = formatWorkspaceInstructionFiles(files, { locale: "zh-CN" });
    expect(prompt).toContain("自定义文件名");
    expect(prompt.indexOf("### AGENTS.md")).toBeLessThan(prompt.indexOf("### QWEN.md"));
  });
});
