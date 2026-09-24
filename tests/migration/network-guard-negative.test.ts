/**
 * R00-T08（T05 R1-F01 修复的负向回归）：验证 network-guard 覆盖面与声明一致。
 *
 * 此前守卫存在三类实证绕过：net/tls 的 createConnection 另一导出未被补丁
 * （http.Agent 即走此路径）、无冒号字符串被误判为管道、dns.promises.lookup 未覆盖。
 * 本文件逐类验证「意外外连立即失败」对下列调用形成立；全部为零真实外发。
 */

import { describe, expect, it } from "vitest";
import { installExternalNetworkGuard } from "./network-guard.ts";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const net = require("node:net") as typeof import("node:net");
const tls = require("node:tls") as typeof import("node:tls");
const dns = require("node:dns") as typeof import("node:dns");
const http = require("node:http") as typeof import("node:http");

installExternalNetworkGuard("guard-negative");

const BLOCKED = "external network blocked";

function captureSync(fn: () => unknown): { threw: boolean; message: string } {
  try {
    fn();
    return { threw: false, message: "" };
  } catch (e) {
    return { threw: true, message: e instanceof Error ? e.message : String(e) };
  }
}

describe("R00-T08 network-guard negative regression (T05 R1-F01)", () => {
  it("blocks net.createConnection export (bypass class ①)", () => {
    const r = captureSync(() => net.createConnection({ host: "example.com", port: 443 }));
    expect(r.threw).toBe(true);
    expect(r.message).toContain(BLOCKED);
    expect(r.message).toContain("example.com");
  });

  it("blocks tls.connect export; tls.createConnection patched whenever the export exists (bypass class ①)", () => {
    const r = captureSync(() => tls.connect({ host: "example.com", port: 443 }));
    expect(r.threw).toBe(true);
    expect(r.message).toContain(BLOCKED);
    // node:tls 在当前 Node（v24）只导出 connect；守卫对 createConnection 导出
    // 做了按存在性补丁——存在则拦截、不存在则该面本就不可达。
    const tlsCreateConnection = (tls as unknown as Record<string, unknown>).createConnection;
    if (typeof tlsCreateConnection === "function") {
      const r2 = captureSync(() => (tlsCreateConnection as (...a: unknown[]) => unknown)({ host: "example.com", port: 443 }));
      expect(r2.threw).toBe(true);
      expect(r2.message).toContain(BLOCKED);
    } else {
      expect(tlsCreateConnection).toBeUndefined();
    }
  });

  it("blocks positional host string net.connect(443, host) — no longer misread as pipe (bypass class ③)", () => {
    const r = captureSync(() => net.connect(443, "example.com"));
    expect(r.threw).toBe(true);
    expect(r.message).toContain(BLOCKED);
    expect(r.message).toContain("example.com");
  });

  it("blocks http.request through http.Agent -> net.createConnection (bypass class ②)", async () => {
    // Node 24：agent.createSocket 同步调用 net.createConnection，守卫在构造/发送
    // 路径上同步抛出；若未来 Node 改为异步 error 事件，则走事件面断言。
    const sync = captureSync(() => {
      const req = http.request({ host: "example.com", port: 80, path: "/" });
      req.on("error", () => undefined);
      req.end();
    });
    if (sync.threw) {
      expect(sync.message).toContain(BLOCKED);
      return;
    }
    const error = await new Promise<Error>((resolve, reject) => {
      const req = http.request({ host: "example.com", port: 80, path: "/", timeout: 4000 });
      req.on("error", (e: Error) => resolve(e));
      req.on("timeout", () => reject(new Error("guard did not block http.request (timed out)")));
      req.end();
    });
    expect(error.message).toContain(BLOCKED);
  });

  it("blocks dns.promises.lookup (bypass class ④)", async () => {
    const error = await dns.promises.lookup("example.com").then(
      () => null,
      (e: Error) => e,
    );
    expect(error).toBeInstanceOf(Error);
    expect((error as Error).message).toContain(BLOCKED);
  });

  it("blocks dns.resolve* callback and promises family (bypass class ④)", () => {
    const callback = captureSync(() => dns.resolve4("example.com", () => undefined));
    expect(callback.threw).toBe(true);
    expect(callback.message).toContain(BLOCKED);
    const promiseForm = captureSync(() => void dns.promises.resolve4("example.com"));
    expect(promiseForm.threw).toBe(true);
    expect(promiseForm.message).toContain(BLOCKED);
  });

  it("still allows explicit unix pipe path strings and local hosts", async () => {
    const sock = net.connect("/tmp/r00-guard-nonexistent-pipe.sock");
    const pipeError = await new Promise<Error | null>((resolve) => {
      sock.on("error", (e: Error) => resolve(e));
    });
    expect(pipeError).toBeInstanceOf(Error); // 真实连接不存在管道 → ENOENT，但不是守卫拦截
    expect((pipeError as Error).message).not.toContain(BLOCKED);
    expect((pipeError as Error).message).toContain("ENOENT");

    const local = net.connect(1, "127.0.0.1");
    const outcome = new Promise<{ blocked: boolean }>((resolve) => {
      local.on("error", (e: Error) => resolve({ blocked: e.message.includes(BLOCKED) }));
      local.on("connect", () => {
        local.destroy();
        resolve({ blocked: false });
      });
    });
    await expect(outcome).resolves.toEqual({ blocked: false });
  });
});
