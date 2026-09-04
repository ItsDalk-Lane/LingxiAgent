import path from "node:path";
import { describe, expect, it } from "vitest";

import {
  assertNativeTarget,
  buildRuntimeEnvironment,
  resolvePackagedArtifactDir,
} from "../scripts/smoke-packaged-knowledge.mjs";

describe("包内 Knowledge 烟测边界", () => {
  it("按发布目录规则解析四个平台的服务器种子", () => {
    expect(resolvePackagedArtifactDir({ rootDir: "/repo", platform: "darwin", arch: "arm64" }))
      .toBe(path.join("/repo", "dist-server-artifact", "mac-arm64"));
    expect(resolvePackagedArtifactDir({ rootDir: "/repo", platform: "darwin", arch: "x64" }))
      .toBe(path.join("/repo", "dist-server-artifact", "mac-x64"));
    expect(resolvePackagedArtifactDir({ rootDir: "/repo", platform: "win32", arch: "x64" }))
      .toBe(path.join("/repo", "dist-server-artifact", "win-x64"));
    expect(resolvePackagedArtifactDir({ rootDir: "/repo", platform: "linux", arch: "x64" }))
      .toBe(path.join("/repo", "dist-server-artifact", "linux-x64"));
  });

  it("拒绝用当前宿主冒充另一平台或架构执行包内烟测", () => {
    expect(() => assertNativeTarget({
      platform: "win32",
      arch: "x64",
      actualPlatform: "darwin",
      actualArch: "arm64",
    })).toThrow(/native target mismatch/);
  });

  it("启动包内服务器前剥离构建签名和发布凭证", () => {
    expect(buildRuntimeEnvironment({
      PATH: "/bin",
      LINGXI_SIGN_KEY: "/tmp/private.pem",
      LINGXI_SIGN_KEYSET: "/tmp/keyset.json",
      CSC_LINK: "certificate",
      APPLE_ID_PASSWORD: "password",
      GH_TOKEN: "token",
    })).toEqual({ PATH: "/bin" });
  });
});

it("从独立包目录执行真实原生检索，并在移除扩展后用原向量回退", async () => {
  const fs = await import("node:fs");
  const os = await import("node:os");
  const { createRequire } = await import("node:module");
  const { buildKnowledgeVectorRuntime } = await import("../scripts/build-server-runtime-assets.mjs");
  const { runPackagedKnowledgeVectorSmoke } = await import("../scripts/smoke-packaged-knowledge.mjs");
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-vector-package-test-"));
  const require = createRequire(import.meta.url);
  try {
    fs.writeFileSync(path.join(root, "package.json"), JSON.stringify({ type: "module" }));
    fs.copyFileSync(process.execPath, path.join(root, process.platform === "win32" ? "hana-server.exe" : "node"));
    fs.mkdirSync(path.join(root, "node_modules"));
    for (const name of ["usearch", "node-gyp-build", "bindings", "file-uri-to-path", "better-sqlite3"]) {
      let packageRoot = path.dirname(require.resolve(name));
      while (!fs.existsSync(path.join(packageRoot, "package.json"))
        || JSON.parse(fs.readFileSync(path.join(packageRoot, "package.json"), "utf8")).name !== name) {
        const parent = path.dirname(packageRoot);
        if (parent === packageRoot) throw new Error(`package root not found: ${name}`);
        packageRoot = parent;
      }
      fs.cpSync(packageRoot, path.join(root, "node_modules", name), { recursive: true });
    }
    buildKnowledgeVectorRuntime({ rootDir: path.resolve(import.meta.dirname, ".."), bundleOutDir: path.join(root, "bundle") });
    let missing = false;
    const report = await runPackagedKnowledgeVectorSmoke({ serverDir: root, onNativeRemoved: () => {
      missing = !fs.readdirSync(path.join(root, "node_modules/usearch"), { recursive: true }).some(file => file.endsWith(".node"));
    } });
    expect(missing).toBe(true);
    expect(report.native.vectorBackend).toBe("hnsw");
    expect(report.fallback.vectorBackend).toBe("portable");
    expect(fs.readdirSync(path.join(root, "node_modules/usearch"), { recursive: true }).some(file => file.endsWith(".node"))).toBe(true);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
}, 60_000);
