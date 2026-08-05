import { afterEach, describe, expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import {
  MINGIT_SHA256,
  MINGIT_URL,
  MINGIT_VERSION,
  REQUIRED_RUNTIME_FILES,
  missingRuntimeFiles,
  hasMinGitRuntime,
} from "../scripts/mingit-runtime.js";

const tempDirs = [];

function makeRuntimeRoot(relativeFiles) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-mingit-runtime-"));
  tempDirs.push(dir);
  for (const relative of relativeFiles) {
    const filePath = path.join(dir, ...relative.split("/"));
    fs.mkdirSync(path.dirname(filePath), { recursive: true });
    fs.writeFileSync(filePath, "", "utf-8");
  }
  return dir;
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

describe("download-mingit runtime contract", () => {
  it("pins the official MinGit artifact with a SHA-256", () => {
    expect(MINGIT_URL).toBe(
      `https://github.com/git-for-windows/git/releases/download/v${MINGIT_VERSION}.windows.1/MinGit-${MINGIT_VERSION}-64-bit.zip`,
    );
    expect(MINGIT_SHA256).toMatch(/^[0-9a-f]{64}$/);
  });

  it("requires git entrypoints and the MinGit POSIX shell", () => {
    expect(REQUIRED_RUNTIME_FILES).toEqual([
      "cmd/git.exe",
      "mingw64/bin/git.exe",
      "usr/bin/sh.exe",
    ]);
  });

  it("reports every missing runtime file instead of failing on the first", () => {
    const root = makeRuntimeRoot(["cmd/git.exe"]);
    expect(missingRuntimeFiles(root)).toEqual([
      "mingw64/bin/git.exe",
      "usr/bin/sh.exe",
    ]);
    expect(hasMinGitRuntime(root)).toBe(false);
  });

  it("accepts a complete MinGit runtime tree", () => {
    const root = makeRuntimeRoot([
      "cmd/git.exe",
      "mingw64/bin/git.exe",
      "usr/bin/sh.exe",
    ]);
    expect(missingRuntimeFiles(root)).toEqual([]);
    expect(hasMinGitRuntime(root)).toBe(true);
  });

  it("treats a legacy PortableGit tree with bash but no sh as incomplete", () => {
    const root = makeRuntimeRoot([
      "cmd/git.exe",
      "mingw64/bin/git.exe",
      "usr/bin/bash.exe",
    ]);
    expect(hasMinGitRuntime(root)).toBe(false);
  });

  it("treats a missing root as fully missing", () => {
    const root = path.join(os.tmpdir(), "hana-mingit-does-not-exist");
    expect(missingRuntimeFiles(root)).toEqual(REQUIRED_RUNTIME_FILES);
  });
});
