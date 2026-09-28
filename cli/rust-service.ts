import fs from "fs";
import path from "path";
import { execFileSync } from "child_process";
import { fileURLToPath } from "url";
import crypto from "crypto";

type RustInstanceRecord = {
  serverKind: string;
  instanceId: string;
  homePath: string;
  bindAddr: string;
  transport?: unknown;
};

type RustLocalToken = {
  kind: string;
  instanceId: string;
  token: string;
};

export type RustConnection = {
  ok: true;
  backend: "rust";
  baseUrl: string;
  token: string;
  source: "rust-local-token" | "explicit";
};

export type RustConnectionFailure = {
  ok: false;
  reason: string;
  message: string;
};

// 服务响应会显示在终端；去掉控制字符并限制长度，避免远端文本改写终端状态。
export function safeRustTerminalText(value: unknown, limit = 256): string {
  return String(value ?? "").replace(/[\u0000-\u001f\u007f-\u009f]/g, " ").slice(0, limit);
}

function rustSourceDigest(root: string): string {
  const hash = crypto.createHash("sha256");
  hash.update("rust-toolchain.toml").update("\0");
  hash.update(fs.readFileSync(path.join(root, "rust-toolchain.toml"))).update("\0");
  const rustRoot = path.join(root, "rust");
  const files: string[] = [];
  function walk(dir: string, relative = "") {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const name = relative ? `${relative}/${entry.name}` : entry.name;
      if (!relative && entry.name === "target") continue;
      if (entry.isSymbolicLink()) throw new Error("Rust source symlink refused");
      if (entry.isDirectory()) walk(path.join(dir, entry.name), name);
      else if (entry.isFile()) files.push(name);
    }
  }
  walk(rustRoot);
  for (const name of files.sort()) {
    hash.update(name).update("\0");
    hash.update(fs.readFileSync(path.join(rustRoot, name))).update("\0");
  }
  return hash.digest("hex");
}

export function resolveWindowsRustReader(moduleFile = fileURLToPath(import.meta.url), arch = process.arch): string {
  const moduleDir = path.dirname(moduleFile);
  let directory: string;
  let expectedVersion: string;
  let sourceRoot: string | null = null;
  if (path.basename(moduleDir) === "cli") {
    sourceRoot = path.resolve(moduleDir, "..");
    directory = path.join(sourceRoot, "dist-rust-service", `win-${arch}`);
    expectedVersion = JSON.parse(fs.readFileSync(path.join(sourceRoot, "package.json"), "utf8")).version;
  } else {
    if (path.basename(moduleDir) !== "bundle") throw new Error("unknown Rust CLI installation layout");
    // CLI 与 reader 必须来自同一个 server 树，覆盖当前、升级和回退版。
    const serverRoot = path.resolve(moduleDir, "..");
    directory = path.join(serverRoot, "rust-service");
    if (path.basename(serverRoot) === "server" && path.basename(path.dirname(serverRoot)) === "LingxiCore") {
      expectedVersion = JSON.parse(fs.readFileSync(path.join(serverRoot, "package.json"), "utf8")).version;
    } else if (path.basename(path.dirname(serverRoot)) === "server"
      && path.basename(path.dirname(path.dirname(serverRoot))) === "artifacts") {
      const suffix = `-win32-${arch}`;
      if (!path.basename(serverRoot).endsWith(suffix)) throw new Error("Rust CLI activated version directory mismatch");
      expectedVersion = path.basename(serverRoot).slice(0, -suffix.length);
      const bundledVersion = JSON.parse(fs.readFileSync(path.join(serverRoot, "package.json"), "utf8")).version;
      if (bundledVersion !== expectedVersion) throw new Error("Rust CLI activated package version mismatch");
      const receipt = JSON.parse(fs.readFileSync(path.join(serverRoot, ".verified"), "utf8"));
      const home = path.dirname(path.dirname(path.dirname(serverRoot)));
      const pointers = path.join(home, "artifacts", "pointers");
      if (receipt.version !== expectedVersion || !/^[0-9a-f]{64}$/.test(receipt.sha256 || "")) {
        throw new Error("Rust CLI activated receipt mismatch");
      }
      const found = fs.readdirSync(pointers).filter((name) => /\.(current|previous)\.json$/.test(name))
        .some((name) => {
          const pointer = JSON.parse(fs.readFileSync(path.join(pointers, name), "utf8"));
          return pointer.kind === "server" && pointer.platformArch === `win32-${arch}`
            && pointer.version === expectedVersion && pointer.sha256 === receipt.sha256
            && path.resolve(pointer.versionDir || "") === serverRoot;
        });
      if (!found) throw new Error("Rust CLI activated server pointer mismatch");
    } else {
      throw new Error("unknown Rust CLI package layout");
    }
  }
  if (fs.lstatSync(directory).isSymbolicLink()) throw new Error("Rust reader directory is a link");
  const manifestPath = path.join(directory, "build.json");
  const manifestStat = fs.lstatSync(manifestPath);
  if (!manifestStat.isFile() || manifestStat.isSymbolicLink() || manifestStat.size < 1 || manifestStat.size > 4096) {
    throw new Error("invalid Rust reader manifest");
  }
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  if (manifest.schemaVersion !== 1 || manifest.platform !== "win" || manifest.arch !== arch
    || manifest.binary !== "lingxi-service.exe" || !/^[0-9a-f]{64}$/.test(manifest.sha256 || "")
    || !/^[0-9a-f]{64}$/.test(manifest.contentSha256 || "")
    || !/^[0-9a-f]{64}$/.test(manifest.sourceSha256 || "")
    || !/^\d+\.\d+\.\d+$/.test(manifest.toolchain || "")
    || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(manifest.appVersion || "")
    || !/^[a-zA-Z0-9_-]+$/.test(manifest.target || "")
    || manifest.appVersion !== expectedVersion) {
    throw new Error("Rust reader manifest does not match this Windows CLI");
  }
  if (sourceRoot) {
    const toolchain = /^channel\s*=\s*"(\d+\.\d+\.\d+)"/m.exec(
      fs.readFileSync(path.join(sourceRoot, "rust-toolchain.toml"), "utf8"),
    )?.[1];
    if (manifest.toolchain !== toolchain || manifest.sourceSha256 !== rustSourceDigest(sourceRoot)) {
      throw new Error("Rust reader stage is stale for current CLI source");
    }
  }
  const binary = path.join(directory, "lingxi-service.exe");
  const stat = fs.lstatSync(binary);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size < 1024) throw new Error("invalid Rust reader executable");
  const bytes = fs.readFileSync(binary);
  const pe = bytes.readUInt32LE(0x3c);
  if (bytes.subarray(0, 2).toString() !== "MZ"
    || bytes.subarray(pe, pe + 4).toString() !== "PE\0\0"
    || bytes.readUInt16LE(pe + 4) !== (arch === "arm64" ? 0xaa64 : arch === "x64" ? 0x8664 : 0)) {
    throw new Error("Rust reader is not a matching Windows executable");
  }
  const fullDigest = crypto.createHash("sha256").update(bytes).digest("hex");
  if (fullDigest !== manifest.sha256) throw new Error("Rust reader executable digest mismatch");
  return binary;
}

function readPrivateJson(file: string, requirePrivate = false): unknown {
  if (process.platform === "win32") {
    const before = fs.lstatSync(file);
    if (!before.isFile() || before.isSymbolicLink()) throw new Error("unsafe Rust runtime record");
    const runtime = path.dirname(file);
    const name = path.basename(file) === "instance.json" ? "instance"
      : path.basename(file) === "local-token.json" ? "local-token" : null;
    if (!name || path.basename(runtime) !== "lingxi-service") {
      throw new Error("unknown Rust runtime record");
    }
    const reader = resolveWindowsRustReader();
    try {
      const bytes = execFileSync(reader,
        ["--read-private-runtime-json", path.dirname(runtime), name], {
          encoding: "utf8", timeout: 10000, maxBuffer: 65536,
          windowsHide: true, stdio: ["ignore", "pipe", "pipe"],
        });
      return JSON.parse(bytes);
    } catch {
      throw new Error("Windows Rust runtime record failed private handle verification or read");
    }
  }
  const flags = fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0);
  const fd = fs.openSync(file, flags);
  try {
    const stat = fs.fstatSync(fd);
    if (!stat.isFile() || stat.size > 65536) throw new Error("not a bounded regular file");
    if (requirePrivate && (
      (stat.mode & 0o077) !== 0 || (typeof process.getuid === "function" && stat.uid !== process.getuid())
    )) {
      throw new Error("local token file is not owner-only");
    }
    return JSON.parse(fs.readFileSync(fd, "utf8"));
  } finally {
    fs.closeSync(fd);
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

async function readBoundedText(response: Response): Promise<string> {
  if (!response.body) return "";
  const reader = response.body.getReader();
  const chunks: Buffer[] = [];
  let length = 0;
  try {
    while (true) {
      const next = await reader.read();
      if (next.done) break;
      length += next.value.byteLength;
      if (length > 8 * 1024 * 1024) {
        await reader.cancel();
        throw new Error("Rust service response exceeded 8 MiB");
      }
      chunks.push(Buffer.from(next.value));
    }
    return Buffer.concat(chunks, length).toString("utf8");
  } finally {
    reader.releaseLock();
  }
}

function loopbackUrl(bindAddr: string, transport: "http" | "https"): string | null {
  try {
    const url = new URL(`${transport}://${bindAddr}`);
    if (!(["127.0.0.1", "[::1]"].includes(url.hostname))
      || url.username || url.password || url.pathname !== "/" || url.search || url.hash
      || !Number.isInteger(Number(url.port)) || Number(url.port) < 1) return null;
    return url.origin;
  } catch {
    return null;
  }
}

// instance.json 是诊断记录，不是存活证明；调用方还须真实请求服务并验身份。
export function readRustLocalService({ lingxiHome }: { lingxiHome: string }): RustConnection | RustConnectionFailure {
  const runtime = path.join(lingxiHome, "lingxi-service");
  const recordPath = path.join(runtime, "instance.json");
  const tokenPath = path.join(runtime, "local-token.json");
  try {
    fs.lstatSync(recordPath);
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === "ENOENT") {
      return { ok: false, reason: "missing_rust_instance", message: `No Rust service instance record was found at ${recordPath}` };
    }
    return { ok: false, reason: "invalid_rust_instance", message: "Rust service instance record could not be inspected" };
  }
  try {
    const recordRaw = readPrivateJson(recordPath);
    const tokenRaw = readPrivateJson(tokenPath, true);
    if (!isRecord(recordRaw) || !isRecord(tokenRaw)) throw new Error("invalid instance or token record");
    const record = recordRaw as RustInstanceRecord;
    const token = tokenRaw as RustLocalToken;
    const expectedHome = fs.realpathSync(lingxiHome);
    if (record.serverKind !== "lingxi-service" || record.homePath !== expectedHome
      || typeof record.instanceId !== "string" || !record.instanceId
      || token.kind !== "local_token" || token.instanceId !== record.instanceId
      || typeof token.token !== "string" || !/^[0-9a-f]{32}$/.test(token.token)) {
      throw new Error("instance identity, home, or local token does not match");
    }
    // 旧实例记录没有 transport，按旧协议识别为 HTTP；未知值必须拒绝。
    const transport = record.transport === undefined ? "http" : record.transport;
    if (transport !== "http" && transport !== "https") {
      throw new Error("instance transport is unknown");
    }
    const baseUrl = typeof record.bindAddr === "string" ? loopbackUrl(record.bindAddr, transport) : null;
    if (!baseUrl) throw new Error("instance address is not a loopback service address");
    return { ok: true, backend: "rust", baseUrl, token: token.token, source: "rust-local-token" };
  } catch (err) {
    return {
      ok: false,
      reason: "invalid_rust_instance",
      message: `Cannot use Rust service instance at ${recordPath}: ${err instanceof Error ? err.message : String(err)}`,
    };
  }
}

export function explicitRustConnection(url: string, token: string): RustConnection | RustConnectionFailure {
  try {
    if (typeof token !== "string" || Buffer.byteLength(token, "utf8") > 4096 || /[\u0000-\u001f\u007f]/.test(token)) {
      throw new Error("token contains invalid header characters or exceeds 4096 bytes");
    }
    const parsed = new URL(url);
    if (!["http:", "https:"].includes(parsed.protocol) || parsed.username || parsed.password
      || parsed.search || parsed.hash || parsed.pathname !== "/") {
      throw new Error("expected an http(s) origin without credentials, path, or query");
    }
    return { ok: true, backend: "rust", baseUrl: parsed.origin, token, source: "explicit" };
  } catch (err) {
    return { ok: false, reason: "invalid_url", message: `Invalid Rust service URL: ${err instanceof Error ? err.message : String(err)}` };
  }
}

export class RustCliClient {
  readonly baseUrl: string;
  readonly token: string;
  readonly source: RustConnection["source"];

  constructor(connection: RustConnection) {
    this.baseUrl = connection.baseUrl;
    this.token = connection.token;
    this.source = connection.source;
  }

  async request(endpoint: string): Promise<any> {
    const headers: Record<string, string> = {};
    if (this.token) headers.Authorization = `Bearer ${this.token}`;
    let response: Response;
    try {
      response = await fetch(`${this.baseUrl}${endpoint}`, {
        headers, redirect: "error", signal: AbortSignal.timeout(5000),
      });
    } catch (err) {
      throw new Error(`Rust service at ${this.baseUrl} is unreachable: ${err instanceof Error ? err.message : String(err)}`);
    }
    const raw = await readBoundedText(response);
    let body: any;
    try { body = raw ? JSON.parse(raw) : null; } catch { body = null; }
    if (!response.ok) {
      const detail = body?.message || body?.reason || body?.error?.message || response.statusText;
      const redacted = this.token ? String(detail).replaceAll(this.token, "[redacted]") : detail;
      throw new Error(`Rust service HTTP ${response.status}: ${safeRustTerminalText(redacted)}`);
    }
    if (!isRecord(body)) throw new Error(`Rust service returned an invalid ${endpoint} response`);
    return body;
  }

  async health() {
    const health = await this.request("/lingxi/v1/health");
    if (health.serverKind !== "lingxi-service" || health.status !== "ok"
      || typeof health.serverVersion !== "string" || !health.serverVersion.trim()) {
      throw new Error(`The endpoint at ${this.baseUrl} is not a healthy Rust Lingxi service`);
    }
    return health;
  }

  async identity() {
    const identity = await this.request("/lingxi/v1/me");
    if (typeof identity.principalId !== "string" || typeof identity.credentialKind !== "string") {
      throw new Error("Rust service identity response is incomplete");
    }
    if (this.source === "rust-local-token"
      && (identity.credentialKind !== "loopback_token" || identity.kind !== "local_user")) {
      throw new Error("Rust service did not authenticate the selected local owner token");
    }
    return identity;
  }

  async sessions() {
    if (this.source === "rust-local-token") await this.identity();
    const body = await this.request("/lingxi/v1/sessions");
    if (!Array.isArray(body.sessions) || body.sessions.some((item: unknown) => !isRecord(item)
      || typeof item.sessionId !== "string" || typeof item.title !== "string")) {
      throw new Error("Rust service sessions response is incomplete");
    }
    return body.sessions;
  }

  async session(sessionId: string) {
    if (!sessionId || /[/?#]/.test(sessionId)) throw new Error("Invalid session ID");
    if (this.source === "rust-local-token") await this.identity();
    return this.request(`/lingxi/v1/sessions/${encodeURIComponent(sessionId)}`);
  }
}
