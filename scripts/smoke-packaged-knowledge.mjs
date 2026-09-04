#!/usr/bin/env node

/**
 * 启动刚构建的闭集服务器种子两次，证明 Knowledge 在真实包内运行时能够：
 * 1. 从空数据目录建库、创建 Notebook、导入并解析粘贴文本；
 * 2. 进程退出后用同一数据目录重启；
 * 3. 读回同一个来源、冻结快照原文与安全响应头。
 *
 * 崩溃中断后的研究续跑由 test:knowledge-platform-smoke 在同一宿主先行验证；
 * 本脚本专注于不能由源码测试替代的“包内运行时 + 持久化重启”边界。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { assertKnowledgeVectorRuntime } from "./build-server-runtime-assets.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SMOKE_TEXT = "Knowledge packaged runtime smoke text. 跨平台冻结正文。";
const BUILD_SECRET_ENV_KEYS = [
  "LINGXI_SIGN_KEY",
  "LINGXI_SIGN_KEYSET",
  "LINGXI_SIGN_KEY_PEM",
  "LINGXI_MACHO_SIGN_IDENTITY",
  "CSC_LINK",
  "CSC_KEY_PASSWORD",
  "WIN_CSC_LINK",
  "WIN_CSC_KEY_PASSWORD",
  "APPLE_ID",
  "APPLE_ID_PASSWORD",
  "APPLE_APP_SPECIFIC_PASSWORD",
  "APPLE_TEAM_ID",
  "GH_TOKEN",
  "GITHUB_TOKEN",
];

export function resolvePackagedArtifactDir({ rootDir, platform, arch }) {
  const osDirName = platform === "darwin" ? "mac" : platform === "win32" ? "win" : platform;
  return path.join(rootDir, "dist-server-artifact", `${osDirName}-${arch}`);
}

export function assertNativeTarget({
  platform,
  arch,
  actualPlatform = process.platform,
  actualArch = process.arch,
}) {
  if (platform !== actualPlatform || arch !== actualArch) {
    throw new Error(
      `native target mismatch: requested ${platform}-${arch}, running on ${actualPlatform}-${actualArch}`,
    );
  }
  return { platform, arch };
}

export function buildRuntimeEnvironment(baseEnvironment = process.env) {
  const environment = { ...baseEnvironment };
  for (const key of BUILD_SECRET_ENV_KEYS) delete environment[key];
  return environment;
}

function runtimePath(serverDir, platform) {
  return path.join(serverDir, platform === "win32" ? "hana-server.exe" : "node");
}

function extractServerArchive({ artifactDir, serverDir }) {
  if (!fs.existsSync(artifactDir)) {
    throw new Error("packaged server artifact directory is missing");
  }
  const archives = fs.readdirSync(artifactDir)
    .filter(name => /^server-.+\.tar\.gz$/.test(name))
    .sort();
  if (archives.length !== 1) {
    throw new Error(`expected exactly one packaged server archive, found ${archives.length}`);
  }
  fs.mkdirSync(serverDir, { recursive: true });
  const result = spawnSync("tar", ["-xzf", path.join(artifactDir, archives[0]), "-C", serverDir], {
    encoding: "utf8",
    maxBuffer: 10 * 1024 * 1024,
  });
  if (result.error || result.status !== 0) {
    throw new Error(
      `failed to extract packaged server archive: ${result.error?.message || result.stderr || `exit ${result.status}`}`,
    );
  }
}

function waitForExit(child, timeoutMs) {
  if (child.exitCode !== null || child.signalCode !== null) {
    return Promise.resolve({ code: child.exitCode, signal: child.signalCode });
  }
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("packaged server did not exit in time")), timeoutMs);
    child.once("exit", (code, signal) => {
      clearTimeout(timer);
      resolve({ code, signal });
    });
  });
}

async function stopServer(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  child.kill("SIGTERM");
  try {
    await waitForExit(child, 15_000);
  } catch {
    child.kill("SIGKILL");
    await waitForExit(child, 5_000).catch(() => {});
  }
}

function boundedAppend(current, chunk) {
  const next = current + chunk.toString();
  return next.length <= 40_000 ? next : next.slice(-40_000);
}

function spawnServer({ serverDir, platform, lingxiHome }) {
  const runtime = runtimePath(serverDir, platform);
  const bootstrap = path.join(serverDir, "bootstrap.js");
  const entry = path.join(serverDir, "bundle", "index.js");
  for (const required of [runtime, bootstrap, entry]) {
    if (!fs.existsSync(required)) {
      throw new Error(`packaged Knowledge smoke prerequisite is missing: ${path.relative(serverDir, required)}`);
    }
  }
  const serverInfoPath = path.join(lingxiHome, "server-info.json");
  fs.rmSync(serverInfoPath, { force: true });
  let stdout = "";
  let stderr = "";
  const child = spawn(runtime, [bootstrap], {
    cwd: serverDir,
    env: {
      ...buildRuntimeEnvironment(),
      LINGXI_ROOT: serverDir,
      LINGXI_SERVER_ENTRY: entry,
      LINGXI_HOME: lingxiHome,
      LINGXI_PORT: "0",
      LINGXI_CREATE_STARTUP_SESSION: "0",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  child.stdout.on("data", chunk => { stdout = boundedAppend(stdout, chunk); });
  child.stderr.on("data", chunk => { stderr = boundedAppend(stderr, chunk); });
  return { child, serverInfoPath, getOutput: () => ({ stdout, stderr }) };
}

async function waitForServerInfo({ child, serverInfoPath, getOutput, timeoutMs = 90_000 }) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() <= deadline) {
    if (child.exitCode !== null || child.signalCode !== null) {
      const output = getOutput();
      throw new Error(
        `packaged server exited before readiness (code=${child.exitCode}, signal=${child.signalCode})\n`
          + `--- stdout ---\n${output.stdout}\n--- stderr ---\n${output.stderr}`,
      );
    }
    try {
      const parsed = JSON.parse(fs.readFileSync(serverInfoPath, "utf8"));
      if (Number.isInteger(parsed.port) && parsed.port > 0 && typeof parsed.token === "string" && parsed.token) {
        return parsed;
      }
    } catch {
      // 文件尚未写完时继续等待。
    }
    await new Promise(resolve => setTimeout(resolve, 200));
  }
  const output = getOutput();
  throw new Error(
    `timed out waiting for packaged server readiness\n--- stdout ---\n${output.stdout}\n--- stderr ---\n${output.stderr}`,
  );
}

function serverBaseUrl(info) {
  const host = !info.host || info.host === "0.0.0.0" || info.host === "::"
    ? "127.0.0.1"
    : info.host;
  return `http://${host}:${info.port}`;
}

async function request({ baseUrl, token, pathname, method = "GET", body }) {
  const response = await fetch(`${baseUrl}${pathname}`, {
    method,
    headers: {
      Authorization: `Bearer ${token}`,
      ...(body === undefined ? {} : { "Content-Type": "application/json" }),
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  return response;
}

async function requestJson(input, expectedStatus) {
  const response = await request(input);
  const text = await response.text();
  if (response.status !== expectedStatus) {
    throw new Error(
      `${input.method || "GET"} ${input.pathname} returned ${response.status}, expected ${expectedStatus}: ${text.slice(0, 2000)}`,
    );
  }
  try {
    return JSON.parse(text);
  } catch {
    throw new Error(`${input.pathname} returned invalid JSON`);
  }
}

async function startServer(input) {
  const running = spawnServer(input);
  try {
    const info = await waitForServerInfo(running);
    return { ...running, info, baseUrl: serverBaseUrl(info) };
  } catch (error) {
    await stopServer(running.child);
    throw error;
  }
}

/** 子进程只导入解包目录内的真实后端和依赖。 */
export function packagedKnowledgeVectorScript() {
  return `import assert from "node:assert/strict";
import path from "node:path";
import crypto from "node:crypto";
import { createKnowledgeVectorSearchBackend, PortableVectorIndexAdapter, knowledgeChunkIndexVariantId,
  searchVectorBackend } from "./bundle/knowledge-vector.js";
const [home, expected] = process.argv.slice(2);
const portable = new PortableVectorIndexAdapter({ dbPath: path.join(home, "knowledge-vector.db") });
const model = { provider: "packaged-smoke", modelId: "fixed", protocol: "openai", dimensions: 3,
  key: crypto.createHash("sha256").update("packaged-smoke").digest("hex") };
const input = { parseArtifactId: "packaged-smoke", chunkIndexVariantId: knowledgeChunkIndexVariantId("packaged-smoke", "fixed"),
  model, chunkFingerprint: "fixed-three-vectors", entries: [[1,0,0],[0,1,0],[0,0,1]].map((vector, ordinal) => ({
    parseArtifactId: "packaged-smoke", chunkId: "chunk-" + ordinal, ordinal, vector })) };
const variant = expected === "hnsw" ? portable.buildOrReplaceArtifact(input).vectorIndexVariantId
  : portable.listReadyVectorVariantIds()[0];
assert.ok(variant);
const backend = createKnowledgeVectorSearchBackend({ indexesRoot: home, portable });
try {
  if (backend.whenIdle) await backend.whenIdle();
  const result = await searchVectorBackend(backend, { vectorIndexVariantIds: [variant], model, queryVector: [1,0,0], limit: 1 });
  assert.equal(result.vectorBackend, expected);
  assert.equal(result.results[0]?.chunkId, "chunk-0");
  // 余弦实现存在浮点舍入，单位向量只允许两个机器精度单位的误差。
  assert.ok(Math.abs(result.results[0]?.score - 1) <= 2 * Number.EPSILON);
  if (expected === "portable") assert.ok(result.degradedReasons.includes("ANN_NATIVE_UNAVAILABLE:" + variant));
  assert.deepEqual(portable.readReadyVectorBatch(variant, -1).map(entry => entry.vector), input.entries.map(entry => entry.vector));
  console.log(JSON.stringify({ vectorBackend: result.vectorBackend, degradedReasons: result.degradedReasons, hits: result.results.length }));
} finally { await backend.close(); portable.close(); }
`;
}

export async function runPackagedKnowledgeVectorSmoke({ serverDir, platform = process.platform, arch = process.arch,
  onNativeRemoved } = {}) {
  assertNativeTarget({ platform, arch });
  assertKnowledgeVectorRuntime(serverDir, platform, arch);
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-package-vector-"));
  const script = path.join(serverDir, ".knowledge-vector-smoke.mjs");
  const moved = [];
  const home = path.join(temporary, "indexes");
  fs.mkdirSync(home);
  function run(expected) {
    const result = spawnSync(runtimePath(serverDir, platform), [script, home, expected], {
      cwd: serverDir, env: buildRuntimeEnvironment(), encoding: "utf8", timeout: 60_000,
      maxBuffer: 2 * 1024 * 1024,
    });
    if (result.error || result.status !== 0) throw new Error(
      `packaged vector ${expected} smoke failed: ${result.error?.message || result.stderr || result.status}`);
    return JSON.parse(result.stdout.trim().split("\n").at(-1));
  }
  try {
    fs.writeFileSync(script, packagedKnowledgeVectorScript(), { flag: "wx" });
    const native = run("hnsw");
    // 真正移走解包副本中的全部原生扩展，再启动一个全新进程，避免模块缓存掩盖缺包问题。
    const packageRoot = path.join(serverDir, "node_modules/usearch");
    for (const relative of fs.readdirSync(packageRoot, { recursive: true })) {
      if (!relative.endsWith(".node")) continue;
      const original = path.join(packageRoot, relative), backup = path.join(temporary, `native-${moved.length}`);
      fs.renameSync(original, backup); moved.push({ original, backup });
    }
    if (!moved.length) throw new Error("packaged native-removal smoke removed no extension");
    const fallback = run("portable");
    if (onNativeRemoved) await onNativeRemoved();
    console.log(`packaged knowledge vector smoke: native=${native.vectorBackend}, removed-native=${fallback.vectorBackend}`);
    return { native, fallback };
  } finally {
    for (const { original, backup } of moved) fs.renameSync(backup, original);
    fs.rmSync(script, { force: true });
    fs.rmSync(temporary, { recursive: true, force: true });
  }
}

export async function runPackagedKnowledgeSmoke({
  rootDir = ROOT,
  platform = process.platform,
  arch = process.arch,
} = {}) {
  assertNativeTarget({ platform, arch });
  const artifactDir = resolvePackagedArtifactDir({ rootDir, platform, arch });
  const smokeRoot = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-package-smoke-"));
  const serverDir = path.join(smokeRoot, "server");
  const lingxiHome = path.join(smokeRoot, "home");
  let first;
  let second;
  try {
    extractServerArchive({ artifactDir, serverDir });
    fs.mkdirSync(lingxiHome, { recursive: true });
    first = await startServer({ serverDir, platform, lingxiHome });
    const auth = { baseUrl: first.baseUrl, token: first.info.token };
    const initial = await requestJson({ ...auth, pathname: "/api/knowledge/notebooks" }, 200);
    if (!Array.isArray(initial.notebooks) || initial.notebooks.length !== 0) {
      throw new Error("fresh packaged Knowledge home did not start with an empty Notebook list");
    }
    const created = await requestJson({
      ...auth,
      pathname: "/api/knowledge/notebooks",
      method: "POST",
      body: { name: "Packaged Knowledge Smoke" },
    }, 201);
    const notebookId = created?.notebook?.id;
    if (typeof notebookId !== "string" || !notebookId) {
      throw new Error("packaged Knowledge create response is missing Notebook id");
    }
    const imported = await requestJson({
      ...auth,
      pathname: `/api/knowledge/notebooks/${encodeURIComponent(notebookId)}/sources`,
      method: "POST",
      body: { kind: "pasted_text", displayName: "packaged-smoke.txt", text: SMOKE_TEXT },
    }, 201);
    const sourceId = imported?.source?.id;
    const snapshotId = imported?.snapshot?.id;
    if (
      typeof sourceId !== "string"
      || typeof snapshotId !== "string"
      || imported?.parseArtifact?.status !== "ready"
    ) {
      throw new Error("packaged Knowledge source was not stored and parsed as READY");
    }
    await stopServer(first.child);
    first = null;

    second = await startServer({ serverDir, platform, lingxiHome });
    const secondAuth = { baseUrl: second.baseUrl, token: second.info.token };
    const notebooks = await requestJson({ ...secondAuth, pathname: "/api/knowledge/notebooks" }, 200);
    if (!Array.isArray(notebooks.notebooks) || !notebooks.notebooks.some(entry => entry?.id === notebookId)) {
      throw new Error("Notebook did not survive packaged server restart");
    }
    const sources = await requestJson({
      ...secondAuth,
      pathname: `/api/knowledge/notebooks/${encodeURIComponent(notebookId)}/sources`,
    }, 200);
    if (!Array.isArray(sources.sources) || !sources.sources.some(entry => entry?.source?.id === sourceId)) {
      throw new Error("Knowledge source did not survive packaged server restart");
    }
    const content = await request({
      ...secondAuth,
      pathname: `/api/knowledge/snapshots/${encodeURIComponent(snapshotId)}/content`,
    });
    const body = await content.text();
    if (content.status !== 200 || body !== SMOKE_TEXT) {
      throw new Error(`frozen Knowledge snapshot did not survive restart (status=${content.status})`);
    }
    if (
      content.headers.get("x-content-type-options") !== "nosniff"
      || content.headers.get("content-security-policy") !== "sandbox; default-src 'none'"
    ) {
      throw new Error("frozen Knowledge snapshot is missing required security headers");
    }
    await stopServer(second.child); second = null;
    const vector = await runPackagedKnowledgeVectorSmoke({ serverDir, platform, arch, onNativeRemoved: async () => {
      second = await startServer({ serverDir, platform, lingxiHome });
      const fallbackContent = await request({ baseUrl: second.baseUrl, token: second.info.token,
        pathname: `/api/knowledge/snapshots/${encodeURIComponent(snapshotId)}/content` });
      if (fallbackContent.status !== 200 || await fallbackContent.text() !== SMOKE_TEXT) {
        throw new Error("packaged server cannot read Knowledge after native extension removal");
      }
      await stopServer(second.child); second = null;
    } });
    return { ok: true, platform, arch, vector };
  } finally {
    if (first) await stopServer(first.child);
    if (second) await stopServer(second.child);
    fs.rmSync(smokeRoot, { recursive: true, force: true });
  }
}

async function main() {
  const platform = process.argv[2] || process.platform;
  const arch = process.argv[3] || process.arch;
  console.log(`packaged Knowledge smoke: ${platform}-${arch}`);
  await runPackagedKnowledgeSmoke({ platform, arch });
  console.log("packaged Knowledge smoke passed: signed archive extraction, fresh install, source parse, restart, immutable snapshot read");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  });
}
