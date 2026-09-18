/**
 * bundled-bins — 内置二进制定位：候选链顺序、布局变体、平台后缀。
 * 全部走 tmpdir + 注入 env，不碰真实资源目录。
 */
import { describe, expect, it } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { findBundledBin } from "../lib/bundled-bins.ts";
import { resolveAstGrepBinary } from "../lib/sandbox/ast-grep-binary.ts";

function stageFile(root: string, rel: string, mode = 0o755): string {
  const p = path.join(root, rel);
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, "#!/bin/sh\nexit 0\n");
  if (process.platform !== "win32") fs.chmodSync(p, mode);
  return p;
}

function tmpdir(): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), "bundled-bins-"));
}

describe("findBundledBin", () => {
  it("LINGXI_DESKTOP_RESOURCES_PATH：打包摊平布局", () => {
    const tmp = tmpdir();
    try {
      const expected = stageFile(tmp, "bundled-bin/rg");
      expect(findBundledBin("rg", {
        env: { LINGXI_DESKTOP_RESOURCES_PATH: tmp } as NodeJS.ProcessEnv,
        platform: "darwin",
        arch: "arm64",
      })).toBe(expected);
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("LINGXI_ROOT：构建期暂存布局 {builderOs}-{arch}（darwin→mac）", () => {
    const tmp = tmpdir();
    try {
      const expected = stageFile(tmp, "bundled-bin/mac-arm64/fd");
      expect(findBundledBin("fd", {
        env: { LINGXI_ROOT: tmp } as NodeJS.ProcessEnv,
        platform: "darwin",
        arch: "arm64",
      })).toBe(expected);
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("LINGXI_ROOT 上级：打包态 versioned server root 指向 Resources 根", () => {
    const tmp = tmpdir();
    try {
      const expected = stageFile(tmp, "resources/bundled-bin/win-x64/ast-grep.exe");
      const serverRoot = path.join(tmp, "resources", "server-v1");
      fs.mkdirSync(serverRoot, { recursive: true });
      expect(findBundledBin("ast-grep", {
        env: { LINGXI_ROOT: serverRoot } as NodeJS.ProcessEnv,
        platform: "win32",
        arch: "x64",
      })).toBe(expected);
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("无候选命中返回 null", () => {
    expect(findBundledBin("rg", { env: {} as NodeJS.ProcessEnv, platform: "linux", arch: "x64" })).toBeNull();
  });

  it("无 env 注入且真实目录为空时返回 null（测试进程卫生）", () => {
    const saved = process.env.LINGXI_ROOT;
    const savedRes = process.env.LINGXI_DESKTOP_RESOURCES_PATH;
    delete process.env.LINGXI_ROOT;
    delete process.env.LINGXI_DESKTOP_RESOURCES_PATH;
    try {
      expect(findBundledBin("definitely-not-staged-xyz", {
        resourcesPath: undefined,
        platform: process.platform,
        arch: process.arch,
      })).toBeNull();
    } finally {
      if (saved !== undefined) process.env.LINGXI_ROOT = saved;
      if (savedRes !== undefined) process.env.LINGXI_DESKTOP_RESOURCES_PATH = savedRes;
    }
  });
});

describe("resolveAstGrepBinary 内置优先", () => {
  const savedRoot = process.env.LINGXI_ROOT;

  function withRoot<T>(root: string | undefined, fn: () => T): T {
    if (root === undefined) delete process.env.LINGXI_ROOT;
    else process.env.LINGXI_ROOT = root;
    try {
      return fn();
    } finally {
      if (savedRoot === undefined) delete process.env.LINGXI_ROOT;
      else process.env.LINGXI_ROOT = savedRoot;
    }
  }

  it("内置命中时优先于托管目录", () => {
    const tmp = tmpdir();
    try {
      const builderOs = process.platform === "darwin" ? "mac" : process.platform === "win32" ? "win" : "linux";
      const expected = stageFile(tmp, `bundled-bin/${builderOs}-${process.arch}/ast-grep${process.platform === "win32" ? ".exe" : ""}`);
      withRoot(tmp, () => {
        expect(resolveAstGrepBinary("/nonexistent-managed-dir")).toBe(expected);
      });
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("内置缺失时回落托管目录", () => {
    const tmp = tmpdir();
    try {
      const managedDir = path.join(tmp, "managed");
      const expected = stageFile(managedDir, process.platform === "win32" ? "ast-grep.exe" : "ast-grep");
      withRoot(tmp, () => {
        expect(resolveAstGrepBinary(managedDir)).toBe(expected);
      });
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });
});
