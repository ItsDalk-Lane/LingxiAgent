import { randomBytes } from "node:crypto";
import { EventEmitter } from "node:events";
import { spawn, type ChildProcessWithoutNullStreams, type SpawnOptions } from "node:child_process";
import fsp from "node:fs/promises";
import path from "node:path";
import { LocalModelError, localModelAbortError, throwIfAborted } from "./errors.ts";

export const LOCAL_MODEL_SIDECAR_PROTOCOL_VERSION = 1;

export interface SidecarReadyMessage {
  type: "ready";
  protocol: 1;
  token: string;
  runtimeId: string;
  runtimeVersion: string;
  backend: string;
  pid?: number;
}

interface SidecarResponseMessage {
  type: "response";
  id: number;
  ok: boolean;
  result?: unknown;
  error?: { code?: string; message?: string };
}

interface SidecarPendingRequest {
  resolve(value: unknown): void;
  reject(error: unknown): void;
  timer: NodeJS.Timeout;
  removeAbort(): void;
}

interface SidecarSession {
  child: ChildProcessWithoutNullStreams;
  token: string;
  ready: SidecarReadyMessage | null;
  stdoutBuffer: string;
  pending: Map<number, SidecarPendingRequest>;
  exited: Promise<{ code: number | null; signal: NodeJS.Signals | null }>;
  resolveExited(value: { code: number | null; signal: NodeJS.Signals | null }): void;
  stopping: boolean;
}

export interface SidecarManagerEvent {
  kind: "starting" | "ready" | "restart" | "request" | "request_done" | "stopping" | "stopped" | "failed";
  attempt?: number;
  requestId?: number;
  method?: string;
  durationMs?: number;
  error?: string;
}

export interface SidecarManagerOptions {
  executable: string;
  args?: string[];
  cwd: string;
  runtimeId: string;
  runtimeVersion: string;
  logDir: string;
  environment?: Record<string, string>;
  startupTimeoutMs?: number;
  requestTimeoutMs?: number;
  shutdownGraceMs?: number;
  maxLogBytes?: number;
  maxLogFiles?: number;
  // 原生语音以进程退出作为停算确认，同一实例的请求必须串行。
  terminateOnRequestFailure?: boolean;
  maxResponseBytes?: number;
  spawnImpl?: (
    executable: string,
    args: readonly string[],
    options: SpawnOptions,
  ) => ChildProcessWithoutNullStreams;
  tokenFactory?: () => string;
  platform?: NodeJS.Platform;
  killTree?: (pid: number, platform: NodeJS.Platform) => Promise<void>;
  onEvent?: (event: SidecarManagerEvent) => void;
}

export class SidecarManager extends EventEmitter<{ event: [event: SidecarManagerEvent] }> {
  private readonly options: Required<Pick<SidecarManagerOptions,
    "startupTimeoutMs" | "requestTimeoutMs" | "shutdownGraceMs" | "maxLogBytes" | "maxLogFiles">>
    & SidecarManagerOptions;
  private session: SidecarSession | null = null;
  private starting: Promise<SidecarReadyMessage> | null = null;
  private stopping: Promise<void> | null = null;
  private requestSequence = 0;
  private logQueue: Promise<void> = Promise.resolve();
  private requestQueue: Promise<void> = Promise.resolve();

  constructor(options: SidecarManagerOptions) {
    super();
    if (!path.isAbsolute(options.executable)) {
      throw new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", "sidecar executable must use an absolute path");
    }
    if (!path.isAbsolute(options.cwd) || !path.isAbsolute(options.logDir)) {
      throw new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", "sidecar cwd and log directory must use absolute paths");
    }
    this.options = {
      ...options,
      startupTimeoutMs: boundedInteger(options.startupTimeoutMs, 100, 120_000, 15_000),
      requestTimeoutMs: boundedInteger(options.requestTimeoutMs, 100, 30 * 60_000, 120_000),
      shutdownGraceMs: boundedInteger(options.shutdownGraceMs, 50, 30_000, 3_000),
      maxLogBytes: boundedInteger(options.maxLogBytes, 64 * 1024, 100 * 1024 * 1024, 5 * 1024 * 1024),
      maxLogFiles: boundedInteger(options.maxLogFiles, 1, 10, 3),
    };
  }

  async start(options: { signal: AbortSignal }): Promise<SidecarReadyMessage> {
    await this.stopping;
    throwIfAborted(options.signal);
    if (this.session?.ready) return this.session.ready;
    if (this.starting) return waitWithAbort(this.starting, options.signal);
    this.starting = this.startWithSingleRestart(options.signal).finally(() => {
      this.starting = null;
    });
    return this.starting;
  }

  async request<T>(
    method: string,
    payload: unknown,
    options: { signal: AbortSignal; timeoutMs?: number },
  ): Promise<T> {
    if (!this.options.terminateOnRequestFailure) return this.performRequest(method, payload, options);
    // 排队取消只撤回尚未发送的请求；活动请求必须经退出确认后才向外完成。
    return new Promise<T>((resolve, reject) => {
      const onAbort = () => reject(localModelAbortError());
      options.signal.addEventListener("abort", onAbort, { once: true });
      const result = this.requestQueue.then(async () => {
        options.signal.removeEventListener("abort", onAbort);
        throwIfAborted(options.signal);
        return this.performRequest<T>(method, payload, options);
      });
      this.requestQueue = result.then(() => {}, () => {});
      result.then(resolve, reject);
    });
  }

  private async performRequest<T>(
    method: string,
    payload: unknown,
    options: { signal: AbortSignal; timeoutMs?: number },
  ): Promise<T> {
    if (!/^[a-z][a-z0-9_.-]{0,127}$/.test(method)) {
      throw new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", "invalid sidecar method name");
    }
    const ready = await this.start({ signal: options.signal });
    const session = this.session;
    if (!session || session.ready !== ready || session.stopping) {
      throw new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", "sidecar is not available");
    }
    throwIfAborted(options.signal);
    const requestId = ++this.requestSequence;
    const timeoutMs = boundedInteger(options.timeoutMs, 100, 30 * 60_000, this.options.requestTimeoutMs);
    const startedAt = Date.now();
    this.report({ kind: "request", requestId, method });
    const promise = new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        const pending = session.pending.get(requestId);
        if (!pending) return;
        session.pending.delete(requestId);
        pending.removeAbort();
        reject(new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", `sidecar request timed out: ${method}`, {
          requestId,
          timeoutMs,
        }));
      }, timeoutMs);
      const onAbort = () => {
        const pending = session.pending.get(requestId);
        if (!pending) return;
        session.pending.delete(requestId);
        clearTimeout(timer);
        reject(localModelAbortError());
        if (!this.options.terminateOnRequestFailure) this.writeMessage(session, { type: "cancel", id: requestId });
      };
      options.signal.addEventListener("abort", onAbort, { once: true });
      session.pending.set(requestId, {
        resolve: resolve as (value: unknown) => void,
        reject,
        timer,
        removeAbort: () => options.signal.removeEventListener("abort", onAbort),
      });
      try {
        this.writeMessage(session, { type: "request", id: requestId, method, payload });
      } catch (error) {
        session.pending.delete(requestId);
        clearTimeout(timer);
        options.signal.removeEventListener("abort", onAbort);
        reject(error);
      }
    });
    try {
      const result = await promise;
      this.report({ kind: "request_done", requestId, method, durationMs: Date.now() - startedAt });
      return result;
    } catch (error) {
      if (this.options.terminateOnRequestFailure) {
        session.stopping = true;
        const stopped = this.terminateSession(session).then(() => {
          if (this.session === session) this.session = null;
        });
        this.stopping = stopped;
        try { await stopped; }
        finally { if (this.stopping === stopped) this.stopping = null; }
      }
      this.report({
        kind: "failed",
        requestId,
        method,
        durationMs: Date.now() - startedAt,
        error: safeErrorMessage(error),
      });
      throw error;
    }
  }

  async stop(): Promise<void> {
    if (this.stopping) return this.stopping;
    const session = this.session;
    if (!session) return;
    this.stopping = this.stopSession(session).finally(() => {
      this.stopping = null;
    });
    return this.stopping;
  }

  diagnostics(): SidecarReadyMessage | null {
    return this.session?.ready ? { ...this.session.ready } : null;
  }

  private async startWithSingleRestart(signal: AbortSignal): Promise<SidecarReadyMessage> {
    let firstError: unknown;
    for (let attempt = 1; attempt <= 2; attempt += 1) {
      throwIfAborted(signal);
      try {
        return await this.spawnSession(attempt, signal);
      } catch (error) {
        if (signal.aborted) throw localModelAbortError();
        if (attempt === 1) {
          firstError = error;
          this.report({ kind: "restart", attempt: 2, error: safeErrorMessage(error) });
          continue;
        }
        throw new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", "sidecar failed after one startup restart", {
          firstFailure: safeErrorMessage(firstError),
          secondFailure: safeErrorMessage(error),
        });
      }
    }
    throw new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", "sidecar failed to start");
  }

  private async spawnSession(attempt: number, signal: AbortSignal): Promise<SidecarReadyMessage> {
    await ensureSafeDirectory(this.options.cwd);
    await ensureSafeDirectory(this.options.logDir);
    const token = (this.options.tokenFactory ?? defaultTokenFactory)();
    if (!/^[A-Za-z0-9_-]{32,256}$/.test(token)) {
      throw new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", "sidecar token factory returned an unsafe token");
    }
    const platform = this.options.platform ?? process.platform;
    const spawnImpl = this.options.spawnImpl ?? ((executable, args, spawnOptions) =>
      spawn(executable, args, spawnOptions) as ChildProcessWithoutNullStreams);
    const child = spawnImpl(this.options.executable, this.options.args ?? [], {
      cwd: this.options.cwd,
      env: buildSidecarEnvironment(this.options.environment, token),
      detached: platform !== "win32",
      windowsHide: true,
      stdio: ["pipe", "pipe", "pipe"],
    });
    let resolveExited!: (value: { code: number | null; signal: NodeJS.Signals | null }) => void;
    const exited = new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((resolve) => {
      resolveExited = resolve;
    });
    const session: SidecarSession = {
      child,
      token,
      ready: null,
      stdoutBuffer: "",
      pending: new Map(),
      exited,
      resolveExited,
      stopping: false,
    };
    this.session = session;
    this.report({ kind: "starting", attempt });
    child.stderr.on("data", (chunk: Buffer) => this.queueLog(chunk.toString("utf8"), token));
    child.stdout.on("data", (chunk: Buffer) => this.consumeStdout(session, chunk));
    child.once("error", (error) => this.failSession(session, error));
    child.once("exit", (code, exitSignal) => this.handleExit(session, code, exitSignal));
    const onAbort = () => { void this.terminateSession(session); };
    signal.addEventListener("abort", onAbort, { once: true });
    let startupTimer: ReturnType<typeof setTimeout> | undefined;
    try {
      const ready = await Promise.race([
        waitForReady(session),
        new Promise<never>((_, reject) => {
          startupTimer = setTimeout(() => reject(new LocalModelError(
            "LOCAL_MODEL_SIDECAR_FAILED", "sidecar startup handshake timed out",
          )), this.options.startupTimeoutMs);
        }),
      ]);
      if (ready.runtimeId !== this.options.runtimeId || ready.runtimeVersion !== this.options.runtimeVersion) {
        throw new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", "sidecar runtime identity mismatch", {
          expected: `${this.options.runtimeId}@${this.options.runtimeVersion}`,
          actual: `${ready.runtimeId}@${ready.runtimeVersion}`,
        });
      }
      this.report({ kind: "ready", attempt });
      return ready;
    } catch (error) {
      await this.terminateSession(session);
      throw error;
    } finally {
      clearTimeout(startupTimer);
      signal.removeEventListener("abort", onAbort);
    }
  }

  private consumeStdout(session: SidecarSession, chunk: Buffer): void {
    if (this.session !== session) return;
    session.stdoutBuffer += chunk.toString("utf8");
    const maxBytes = boundedInteger(this.options.maxResponseBytes, 1024 * 1024, 64 * 1024 * 1024, 1024 * 1024);
    if (Buffer.byteLength(session.stdoutBuffer, "utf8") > maxBytes) {
      this.failSession(session, new Error("sidecar protocol line exceeded response limit"));
      void this.terminateSession(session);
      return;
    }
    while (true) {
      const newline = session.stdoutBuffer.indexOf("\n");
      if (newline < 0) break;
      const line = session.stdoutBuffer.slice(0, newline).trim();
      session.stdoutBuffer = session.stdoutBuffer.slice(newline + 1);
      if (!line) continue;
      try {
        this.handleMessage(session, JSON.parse(line) as unknown);
      } catch (error) {
        this.failSession(session, error);
        void this.terminateSession(session);
        return;
      }
    }
  }

  private handleMessage(session: SidecarSession, value: unknown): void {
    if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid sidecar message");
    const message = value as Record<string, unknown>;
    if (!session.ready) {
      if (message.type !== "ready" || message.protocol !== LOCAL_MODEL_SIDECAR_PROTOCOL_VERSION
        || message.token !== session.token || typeof message.runtimeId !== "string"
        || typeof message.runtimeVersion !== "string" || typeof message.backend !== "string") {
        throw new Error("invalid sidecar ready handshake");
      }
      session.ready = message as unknown as SidecarReadyMessage;
      return;
    }
    if (message.type !== "response" || !Number.isSafeInteger(message.id) || typeof message.ok !== "boolean") {
      throw new Error("invalid sidecar response message");
    }
    const response = message as unknown as SidecarResponseMessage;
    const pending = session.pending.get(response.id);
    if (!pending) return;
    session.pending.delete(response.id);
    clearTimeout(pending.timer);
    pending.removeAbort();
    if (response.ok) pending.resolve(response.result);
    else pending.reject(new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", response.error?.message || "sidecar request failed", {
      sidecarCode: response.error?.code,
      requestId: response.id,
    }));
  }

  private writeMessage(session: SidecarSession, message: Record<string, unknown>): void {
    if (this.session !== session || session.stopping || session.child.stdin.destroyed) {
      throw new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", "sidecar input is closed");
    }
    const body = `${JSON.stringify(message)}\n`;
    if (Buffer.byteLength(body, "utf8") > 16 * 1024 * 1024) {
      throw new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", "sidecar request exceeded 16 MiB");
    }
    session.child.stdin.write(body);
  }

  private async stopSession(session: SidecarSession): Promise<void> {
    if (this.session !== session) return;
    session.stopping = true;
    this.report({ kind: "stopping" });
    rejectPending(session, new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", "sidecar is stopping"));
    if (!session.child.stdin.destroyed) {
      try {
        session.child.stdin.write(`${JSON.stringify({ type: "shutdown" })}\n`);
      } catch {}
    }
    const graceful = await waitForExit(session, this.options.shutdownGraceMs);
    if (!graceful) await this.terminateSession(session);
    await session.exited;
    await this.logQueue;
    if (this.session === session) this.session = null;
    this.report({ kind: "stopped" });
  }

  private async terminateSession(session: SidecarSession): Promise<void> {
    if (session.child.exitCode !== null || session.child.signalCode !== null) return;
    const pid = session.child.pid;
    if (!pid) {
      session.child.kill("SIGKILL");
      return;
    }
    await (this.options.killTree ?? defaultKillTree)(pid, this.options.platform ?? process.platform);
    const exited = await waitForExit(session, this.options.shutdownGraceMs);
    if (!exited) {
      throw new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", "sidecar did not confirm process exit after termination", { pid });
    }
  }

  private handleExit(session: SidecarSession, code: number | null, signal: NodeJS.Signals | null): void {
    session.resolveExited({ code, signal });
    rejectPending(session, new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", "sidecar process exited", { code, signal }));
    if (this.session === session && !session.stopping) {
      this.session = null;
      this.report({ kind: "failed", error: `sidecar exited (${String(code ?? signal)})` });
    }
  }

  private failSession(session: SidecarSession, error: unknown): void {
    rejectPending(session, new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", safeErrorMessage(error)));
    this.report({ kind: "failed", error: safeErrorMessage(error) });
  }

  private queueLog(text: string, token: string): void {
    const redacted = text.replaceAll(token, "[REDACTED]");
    this.logQueue = this.logQueue.then(() => appendRotatingLog(
      this.options.logDir,
      redacted,
      this.options.maxLogBytes,
      this.options.maxLogFiles,
    )).catch(() => {});
  }

  private report(event: SidecarManagerEvent): void {
    this.options.onEvent?.(event);
    this.emit("event", event);
  }
}

export function buildSidecarEnvironment(
  extra: Record<string, string> | undefined,
  token: string,
  source: NodeJS.ProcessEnv = process.env,
): Record<string, string> {
  const allowedHostKeys = process.platform === "win32"
    ? ["PATH", "SystemRoot", "WINDIR", "TEMP", "TMP"]
    : ["PATH", "TMPDIR", "LANG", "LC_ALL", "LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH"];
  const environment: Record<string, string> = {};
  for (const key of allowedHostKeys) {
    const value = source[key];
    if (typeof value === "string" && value) environment[key] = value;
  }
  for (const [key, value] of Object.entries(extra ?? {})) {
    if (!/^LINGXI_LOCAL_MODEL_[A-Z0-9_]+$/.test(key) || typeof value !== "string") {
      throw new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", `sidecar environment key is not allowed: ${key}`);
    }
    environment[key] = value;
  }
  environment.LINGXI_LOCAL_MODEL_TOKEN = token;
  environment.LINGXI_LOCAL_MODEL_PROTOCOL = String(LOCAL_MODEL_SIDECAR_PROTOCOL_VERSION);
  return environment;
}

function waitForReady(session: SidecarSession): Promise<SidecarReadyMessage> {
  return new Promise((resolve, reject) => {
    const interval = setInterval(() => {
      if (session.ready) {
        clearInterval(interval);
        resolve(session.ready);
      }
    }, 5);
    session.exited.then(({ code, signal }) => {
      clearInterval(interval);
      if (!session.ready) reject(new Error(`sidecar exited before ready (${String(code ?? signal)})`));
    });
  });
}

function rejectPending(session: SidecarSession, error: unknown): void {
  for (const pending of session.pending.values()) {
    clearTimeout(pending.timer);
    pending.removeAbort();
    pending.reject(error);
  }
  session.pending.clear();
}

async function appendRotatingLog(directory: string, text: string, maxBytes: number, maxFiles: number): Promise<void> {
  if (!text) return;
  await ensureSafeDirectory(directory);
  const active = path.join(directory, "sidecar.log");
  const stat = await fsp.lstat(active).catch(() => null);
  if (stat?.isSymbolicLink() || (stat && !stat.isFile())) return;
  if ((stat?.size ?? 0) + Buffer.byteLength(text, "utf8") > maxBytes) {
    for (let index = maxFiles - 1; index >= 1; index -= 1) {
      const from = index === 1 ? active : path.join(directory, `sidecar.log.${index - 1}`);
      const to = path.join(directory, `sidecar.log.${index}`);
      await fsp.rm(to, { force: true });
      await fsp.rename(from, to).catch(() => {});
    }
  }
  await fsp.appendFile(active, text, { mode: 0o600 });
}

async function ensureSafeDirectory(directory: string): Promise<void> {
  await fsp.mkdir(directory, { recursive: true, mode: 0o700 });
  const stat = await fsp.lstat(directory);
  if (!stat.isDirectory() || stat.isSymbolicLink()) {
    throw new LocalModelError("LOCAL_MODEL_SIDECAR_FAILED", "sidecar path is not a safe directory", { directory });
  }
}

async function defaultKillTree(pid: number, platform: NodeJS.Platform): Promise<void> {
  if (platform === "win32") {
    await new Promise<void>((resolve) => {
      const killer = spawn("taskkill.exe", ["/PID", String(pid), "/T", "/F"], {
        stdio: "ignore",
        windowsHide: true,
      });
      killer.once("error", () => resolve());
      killer.once("close", () => resolve());
    });
    return;
  }
  try {
    process.kill(-pid, "SIGKILL");
  } catch {
    try { process.kill(pid, "SIGKILL"); } catch {}
  }
}

function defaultTokenFactory(): string {
  return randomBytes(32).toString("base64url");
}

async function waitForExit(session: SidecarSession, milliseconds: number): Promise<boolean> {
  let timer: NodeJS.Timeout | undefined;
  try {
    return await Promise.race([
      session.exited.then(() => true),
      new Promise<boolean>((resolve) => { timer = setTimeout(() => resolve(false), milliseconds); }),
    ]);
  } finally { clearTimeout(timer); }
}

function waitWithAbort<T>(promise: Promise<T>, signal: AbortSignal): Promise<T> {
  throwIfAborted(signal);
  return new Promise<T>((resolve, reject) => {
    const onAbort = () => reject(localModelAbortError());
    signal.addEventListener("abort", onAbort, { once: true });
    promise.then(
      (value) => {
        signal.removeEventListener("abort", onAbort);
        resolve(value);
      },
      (error) => {
        signal.removeEventListener("abort", onAbort);
        reject(error);
      },
    );
  });
}

function safeErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function boundedInteger(value: unknown, min: number, max: number, fallback: number): number {
  return Number.isSafeInteger(value) && Number(value) >= min && Number(value) <= max ? Number(value) : fallback;
}
