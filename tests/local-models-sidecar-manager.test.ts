import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { afterEach, describe, expect, it } from "vitest";
import {
  buildSidecarEnvironment,
  SidecarManager,
  type SidecarManagerEvent,
  type SidecarManagerOptions,
} from "../lib/local-models/index.ts";

const roots: string[] = [];
const fixture = path.resolve("tests/fixtures/local-model-sidecar.mjs");

afterEach(() => {
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

function tempRoot(): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-local-model-sidecar-"));
  roots.push(root);
  return root;
}

function createManager(options: {
  environment?: Record<string, string>;
  events?: SidecarManagerEvent[];
  startupTimeoutMs?: number;
  requestTimeoutMs?: number;
  shutdownGraceMs?: number;
  terminateOnRequestFailure?: boolean;
  maxResponseBytes?: number;
  killTree?: SidecarManagerOptions["killTree"];
} = {}): { manager: SidecarManager; root: string } {
  const root = tempRoot();
  return {
    root,
    manager: new SidecarManager({
      executable: process.execPath,
      args: [fixture],
      cwd: root,
      runtimeId: "fixture-runtime",
      runtimeVersion: "1",
      logDir: path.join(root, "logs"),
      environment: options.environment,
      startupTimeoutMs: options.startupTimeoutMs ?? 2_000,
      requestTimeoutMs: options.requestTimeoutMs ?? 1_000,
      shutdownGraceMs: options.shutdownGraceMs ?? 200,
      terminateOnRequestFailure: options.terminateOnRequestFailure,
      maxResponseBytes: options.maxResponseBytes,
      killTree: options.killTree,
      tokenFactory: () => "abcdefghijklmnopqrstuvwxyz0123456789TOKEN",
      onEvent: (event) => options.events?.push(event),
    }),
  };
}

describe("SidecarManager", () => {
  it("语音取消等待真实退出，排队取消不误伤活动请求，下一条重新加载", async () => {
    const events: SidecarManagerEvent[] = [];
    let allowKill!: () => void;
    const killGate = new Promise<void>((resolve) => { allowKill = resolve; });
    const { manager } = createManager({ events, terminateOnRequestFailure: true,
      killTree: async (pid) => { await killGate; process.kill(pid, "SIGKILL"); } });
    const signal = new AbortController().signal;
    const ready = await manager.start({ signal });
    const controller = new AbortController();
    let settled = false;
    const active = manager.request("hang", {}, { signal: controller.signal }).catch((error: unknown) => {
      settled = true; return error;
    });
    while (!events.some((event) => event.kind === "request")) await new Promise((resolve) => setTimeout(resolve, 5));
    const queuedAbort = new AbortController();
    const withdrawn = manager.request("echo", { withdrawn: true }, { signal: queuedAbort.signal });
    queuedAbort.abort();
    await expect(withdrawn).rejects.toMatchObject({ code: "LOCAL_MODEL_ABORTED" });
    expect(manager.diagnostics()?.pid).toBe(ready.pid);
    const next = manager.request("echo", { next: true }, { signal });
    controller.abort();
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(settled).toBe(false);
    expect(() => process.kill(ready.pid!, 0)).not.toThrow();
    allowKill();
    expect(await active).toMatchObject({ code: "LOCAL_MODEL_ABORTED" });
    expect(() => process.kill(ready.pid!, 0)).toThrow();
    await expect(next).resolves.toEqual({ next: true });
    expect(manager.diagnostics()?.pid).not.toBe(ready.pid);
    expect(events.filter((event) => event.kind === "request")).toHaveLength(2);
    await manager.stop();
  });

  it("语音超时同样确认退出，且能传输超过旧一兆限制的有界音频", async () => {
    const { manager } = createManager({ terminateOnRequestFailure: true, requestTimeoutMs: 100, maxResponseBytes: 4 * 1024 * 1024 });
    const signal = new AbortController().signal;
    const ready = await manager.start({ signal });
    await expect(manager.request("hang", {}, { signal })).rejects.toMatchObject({ code: "LOCAL_MODEL_SIDECAR_FAILED" });
    expect(() => process.kill(ready.pid!, 0)).toThrow();
    const large = "a".repeat(2 * 1024 * 1024);
    await expect(manager.request("echo", large, { signal, timeoutMs: 2000 })).resolves.toBe(large);
    await manager.stop();
  });

  it("超过语音响应上限时拒绝并停止进程", async () => {
    const { manager } = createManager({ terminateOnRequestFailure: true });
    const signal = new AbortController().signal;
    const ready = await manager.start({ signal });
    await expect(manager.request("echo", "a".repeat(2 * 1024 * 1024), { signal })).rejects.toMatchObject({ code: "LOCAL_MODEL_SIDECAR_FAILED" });
    expect(() => process.kill(ready.pid!, 0)).toThrow();
    await manager.stop();
  });

  it("clears the startup deadline after a successful handshake so the host can exit", async () => {
    const root = tempRoot();
    const source = `
      import { createJiti } from 'jiti';
      const jiti = createJiti(import.meta.url);
      const { SidecarManager } = await jiti.import(${JSON.stringify(path.resolve("lib/local-models/sidecar-manager.ts"))});
      const manager = new SidecarManager({
        executable: process.execPath, args: [${JSON.stringify(fixture)}],
        cwd: ${JSON.stringify(root)}, logDir: ${JSON.stringify(path.join(root, "logs"))},
        runtimeId: 'fixture-runtime', runtimeVersion: '1', startupTimeoutMs: 60000, shutdownGraceMs: 50,
      });
      await manager.start({ signal: new AbortController().signal });
      await manager.stop();
    `;
    await expect(promisify(execFile)(process.execPath, ["--input-type=module", "--eval", source], {
      cwd: process.cwd(), timeout: 3000,
    })).resolves.toBeDefined();
  });

  it("uses an authenticated handshake, serves requests, filters environment, and redacts logs", async () => {
    const { manager, root } = createManager({
      environment: { LINGXI_LOCAL_MODEL_FIXTURE: "visible" },
    });
    const result = await manager.request<{ leaked: string | null; allowed: string | null }>(
      "inspect_env",
      {},
      { signal: new AbortController().signal },
    );

    expect(result).toEqual({ leaked: null, allowed: "visible" });
    expect(manager.diagnostics()).toMatchObject({
      protocol: 1,
      runtimeId: "fixture-runtime",
      runtimeVersion: "1",
      backend: "cpu",
    });
    await manager.stop();
    const log = fs.readFileSync(path.join(root, "logs", "sidecar.log"), "utf8");
    expect(log).toContain("fixture token=[REDACTED]");
    expect(log).not.toContain("abcdefghijklmnopqrstuvwxyz0123456789TOKEN");
  });

  it("restarts only once after a failed startup handshake", async () => {
    const events: SidecarManagerEvent[] = [];
    const { manager, root } = createManager({
      events,
      environment: {
        LINGXI_LOCAL_MODEL_FAIL_MODE: "first",
        LINGXI_LOCAL_MODEL_ATTEMPT_FILE: path.join(tempRoot(), "attempt.txt"),
      },
    });

    const ready = await manager.start({ signal: new AbortController().signal });
    expect(ready.backend).toBe("cpu");
    expect(events.filter((event) => event.kind === "starting")).toHaveLength(2);
    expect(events.filter((event) => event.kind === "restart")).toHaveLength(1);
    expect(fs.existsSync(path.join(root, "logs", "sidecar.log"))).toBe(true);
    await manager.stop();
  });

  it("fails closed when both startup attempts have invalid handshakes", async () => {
    const events: SidecarManagerEvent[] = [];
    const { manager } = createManager({
      events,
      environment: { LINGXI_LOCAL_MODEL_FAIL_MODE: "always" },
    });

    await expect(manager.start({ signal: new AbortController().signal }))
      .rejects.toMatchObject({ code: "LOCAL_MODEL_SIDECAR_FAILED" });
    expect(events.filter((event) => event.kind === "starting")).toHaveLength(2);
    expect(events.filter((event) => event.kind === "restart")).toHaveLength(1);
    expect(manager.diagnostics()).toBeNull();
  });

  it("times out and cancels individual requests without corrupting the session", async () => {
    const { manager } = createManager({ requestTimeoutMs: 100 });

    await expect(manager.request("hang", {}, {
      signal: new AbortController().signal,
    })).rejects.toMatchObject({ code: "LOCAL_MODEL_SIDECAR_FAILED" });

    const controller = new AbortController();
    const cancelled = manager.request("hang", {}, { signal: controller.signal, timeoutMs: 1_000 });
    controller.abort();
    await expect(cancelled).rejects.toMatchObject({ code: "LOCAL_MODEL_ABORTED" });

    await expect(manager.request("echo", { ok: true }, {
      signal: new AbortController().signal,
    })).resolves.toEqual({ ok: true });
    await manager.stop();
  });

  it("kills an uncooperative process tree and waits for confirmed exit", async () => {
    const events: SidecarManagerEvent[] = [];
    const { manager } = createManager({
      events,
      shutdownGraceMs: 100,
      environment: { LINGXI_LOCAL_MODEL_IGNORE_SHUTDOWN: "1" },
    });
    await manager.start({ signal: new AbortController().signal });
    await manager.stop();
    expect(manager.diagnostics()).toBeNull();
    expect(events.at(-1)?.kind).toBe("stopped");
  });

  it("never copies unrelated host secrets into the child environment", () => {
    const environment = buildSidecarEnvironment(
      { LINGXI_LOCAL_MODEL_BACKEND: "cpu" },
      "abcdefghijklmnopqrstuvwxyz0123456789TOKEN",
      { PATH: "/bin", SHOULD_NOT_LEAK: "secret", OPENAI_API_KEY: "secret" },
    );
    expect(environment.PATH).toBe("/bin");
    expect(environment.LINGXI_LOCAL_MODEL_BACKEND).toBe("cpu");
    expect(environment.SHOULD_NOT_LEAK).toBeUndefined();
    expect(environment.OPENAI_API_KEY).toBeUndefined();
  });

  it("rejects non-whitelisted explicit environment keys", () => {
    expect(() => buildSidecarEnvironment(
      { OPENAI_API_KEY: "must-not-pass" },
      "abcdefghijklmnopqrstuvwxyz0123456789TOKEN",
      {},
    )).toThrowError(expect.objectContaining({ code: "LOCAL_MODEL_SIDECAR_FAILED" }));
  });
});
