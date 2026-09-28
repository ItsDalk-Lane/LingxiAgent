import { afterEach, describe, expect, it, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { main } from "../cli/entry.ts";
import { explicitRustConnection, readRustLocalService, RustCliClient } from "../cli/rust-service.ts";
import { resolveRustServerSpawnSpec, spawnRustServerForeground } from "../cli/server-runner.ts";

const homes: string[] = [];

function syntheticHome() {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-rust-cli-"));
  homes.push(home);
  const runtime = path.join(home, "lingxi-service");
  fs.mkdirSync(runtime);
  const instance = {
    serverKind: "lingxi-service", instanceId: "instance-1", homePath: fs.realpathSync(home),
    bindAddr: "127.0.0.1:14567",
  };
  const token = { kind: "local_token", instanceId: "instance-1", token: "a".repeat(32) };
  fs.writeFileSync(path.join(runtime, "instance.json"), JSON.stringify(instance));
  fs.writeFileSync(path.join(runtime, "local-token.json"), JSON.stringify(token));
  fs.chmodSync(path.join(runtime, "local-token.json"), 0o600);
  return { home, runtime, instance, token };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true });
});

describe("Rust CLI discovery", () => {
  it("binds a local token to the matching instance and home", () => {
    const fixture = syntheticHome();
    expect(readRustLocalService({ lingxiHome: fixture.home })).toMatchObject({
      ok: true, backend: "rust", baseUrl: "http://127.0.0.1:14567",
      token: "a".repeat(32), source: "rust-local-token",
    });
  });

  it("uses the recorded transport and rejects unknown transport values", () => {
    const fixture = syntheticHome();
    const recordPath = path.join(fixture.runtime, "instance.json");
    fs.writeFileSync(recordPath, JSON.stringify({ ...fixture.instance, transport: "http" }));
    expect(readRustLocalService({ lingxiHome: fixture.home })).toMatchObject({
      ok: true, baseUrl: "http://127.0.0.1:14567",
    });
    fs.writeFileSync(recordPath, JSON.stringify({ ...fixture.instance, transport: "https" }));
    expect(readRustLocalService({ lingxiHome: fixture.home })).toMatchObject({
      ok: true, baseUrl: "https://127.0.0.1:14567",
    });
    for (const transport of ["h2c", "", null, 1]) {
      fs.writeFileSync(recordPath, JSON.stringify({ ...fixture.instance, transport }));
      expect(readRustLocalService({ lingxiHome: fixture.home })).toMatchObject({
        ok: false, reason: "invalid_rust_instance",
      });
    }
  });

  it("rejects a stale token, a different home, and a non-loopback address", () => {
    const fixture = syntheticHome();
    const tokenPath = path.join(fixture.runtime, "local-token.json");
    fs.writeFileSync(tokenPath, JSON.stringify({ ...fixture.token, instanceId: "other" }));
    expect(readRustLocalService({ lingxiHome: fixture.home })).toMatchObject({ ok: false, reason: "invalid_rust_instance" });
    fs.writeFileSync(tokenPath, JSON.stringify(fixture.token));
    const recordPath = path.join(fixture.runtime, "instance.json");
    fs.writeFileSync(recordPath, JSON.stringify({ ...fixture.instance, homePath: "/wrong" }));
    expect(readRustLocalService({ lingxiHome: fixture.home })).toMatchObject({ ok: false, reason: "invalid_rust_instance" });
    fs.writeFileSync(recordPath, JSON.stringify({ ...fixture.instance, bindAddr: "192.0.2.1:14567" }));
    expect(readRustLocalService({ lingxiHome: fixture.home })).toMatchObject({ ok: false, reason: "invalid_rust_instance" });
  });

  it("rejects credential-bearing explicit URLs", () => {
    expect(explicitRustConnection("http://user:secret@127.0.0.1:14567", "token")).toMatchObject({ ok: false });
    expect(explicitRustConnection("http://127.0.0.1:14567/path", "token")).toMatchObject({ ok: false });
    expect(explicitRustConnection("http://127.0.0.1:14567", "token\r\nInjected: value")).toMatchObject({ ok: false });
    expect(explicitRustConnection("http://127.0.0.1:14567", "x".repeat(4097))).toMatchObject({ ok: false });
  });
});

describe("Rust CLI serve planning", () => {
  it("spawns only an explicit executable and preserves the selected data home", async () => {
    const fixture = syntheticHome();
    const binary = path.join(fixture.home, "lingxi-service-bin");
    fs.writeFileSync(binary, "#!/bin/sh\nexit 0\n");
    fs.chmodSync(binary, 0o700);
    const spec = await resolveRustServerSpawnSpec({
      env: { LINGXI_SERVICE_BIN: binary, LINGXI_HOME: fixture.home },
      extraArgs: ["--home", fixture.home],
    });
    expect(spec.command).toBe(binary);
    expect(spec.args).toEqual(["--home", fixture.home]);
    expect(spec.env.LINGXI_HOME).toBe(fixture.home);
  });

  it("does not import a real-home renderer into Rust test mode", async () => {
    const fixture = syntheticHome();
    const binary = path.join(fixture.home, "lingxi-service-bin");
    fs.writeFileSync(binary, "#!/bin/sh\nexit 0\n");
    fs.chmodSync(binary, 0o700);
    const spec = await resolveRustServerSpawnSpec({
      env: {
        LINGXI_SERVICE_BIN: binary, LINGXI_HOME: fixture.home,
        LINGXI_RENDERER_DIST: "/outside/data-home",
      },
      extraArgs: ["--test-mode"],
    });
    expect(spec.rendererDist).toBeNull();
    expect(spec.env.LINGXI_RENDERER_DIST).toBeUndefined();
  });

  it("does not silently serve a different frontend when beta is unavailable", async () => {
    const fixture = syntheticHome();
    const binary = path.join(fixture.home, "lingxi-service-bin");
    fs.writeFileSync(binary, "#!/bin/sh\nexit 0\n");
    fs.chmodSync(binary, 0o700);
    await expect(resolveRustServerSpawnSpec({
      env: { LINGXI_SERVICE_BIN: binary, LINGXI_HOME: fixture.home }, channel: "beta",
    })).rejects.toThrow("No activated beta frontend");
  });

  it("refuses a data-downgrade override instead of silently ignoring it", async () => {
    const fixture = syntheticHome();
    const binary = path.join(fixture.home, "lingxi-service-bin");
    fs.writeFileSync(binary, "#!/bin/sh\nexit 0\n");
    fs.chmodSync(binary, 0o700);
    await expect(resolveRustServerSpawnSpec({
      env: { LINGXI_SERVICE_BIN: binary, LINGXI_HOME: fixture.home, LINGXI_ALLOW_DATA_DOWNGRADE: "1" },
    })).rejects.toThrow("refuses LINGXI_ALLOW_DATA_DOWNGRADE");
  });

  it("does not launch Rust beside an unreadable Node ownership record", async () => {
    const fixture = syntheticHome();
    const binary = path.join(fixture.home, "lingxi-service-bin");
    fs.writeFileSync(binary, "#!/bin/sh\nexit 0\n");
    fs.chmodSync(binary, 0o700);
    fs.writeFileSync(path.join(fixture.home, "server-info.json"), "{invalid");
    await expect(spawnRustServerForeground({
      env: { LINGXI_SERVICE_BIN: binary, LINGXI_HOME: fixture.home },
    })).rejects.toThrow("Cannot rule out an existing Node server");
  });
});

describe("Rust CLI service requests", () => {
  it("reads authenticated sessions and never sends a local token in the URL", async () => {
    const fetchMock = vi.fn(async (_url: string, options: any) => {
      expect(options.headers.Authorization).toBe(`Bearer ${"a".repeat(32)}`);
      return new Response(JSON.stringify({ sessions: [{ sessionId: "s1", title: "A" }] }), { status: 200 });
    });
    vi.stubGlobal("fetch", fetchMock);
    const client = new RustCliClient({ ok: true, backend: "rust", baseUrl: "http://127.0.0.1:14567", token: "a".repeat(32), source: "explicit" });
    expect(await client.sessions()).toEqual([{ sessionId: "s1", title: "A" }]);
    expect(fetchMock.mock.calls[0][0]).toBe("http://127.0.0.1:14567/lingxi/v1/sessions");
  });

  it("does not treat a different principal as the owner of a discovered local token", async () => {
    const fetchMock = vi.fn(async (_url: string) => new Response(JSON.stringify({
      principalId: "foreign", credentialKind: "device_credential", kind: "device",
    }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const client = new RustCliClient({
      ok: true, backend: "rust", baseUrl: "http://127.0.0.1:14567",
      token: "a".repeat(32), source: "rust-local-token",
    });
    await expect(client.identity()).rejects.toThrow("did not authenticate the selected local owner token");
    await expect(client.sessions()).rejects.toThrow("did not authenticate the selected local owner token");
    expect(fetchMock.mock.calls.every(([url]) => String(url).endsWith("/me"))).toBe(true);
  });

  it("surfaces unauthorized sessions and never prints another principal's sessions", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ error: { message: "unauthorized" } }), { status: 401 })));
    const output = vi.spyOn(console, "log").mockImplementation(() => {});
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(await main(["sessions", "--runtime", "rust", "--url", "http://127.0.0.1:14567", "--token", "wrong"])).toBe(1);
    expect(output).not.toHaveBeenCalled();
    expect(error.mock.calls.flat().join(" ")).toContain("HTTP 401");
  });

  it("redacts the local token and terminal control bytes from remote errors and session titles", async () => {
    const token = "a".repeat(32);
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({
      message: `denied\u001b]52;clipboard\u0007 ${token}`,
    }), { status: 401 })));
    const client = new RustCliClient({
      ok: true, backend: "rust", baseUrl: "http://127.0.0.1:14567", token, source: "explicit",
    });
    const error = await client.sessions().catch((cause) => String(cause));
    expect(error).toContain("[redacted]");
    expect(error).not.toContain(token);
    expect(error).not.toContain("\u001b");
    expect(error).not.toContain("\u0007");

    const output = vi.spyOn(console, "log").mockImplementation(() => {});
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ sessions: [
      { sessionId: "s1", title: "Visible\u001b[31mHidden", agentId: "Agent\u0007Name" },
    ] }), { status: 200 })));
    expect(await main(["sessions", "--runtime", "rust", "--url", "http://127.0.0.1:14567", "--token", token])).toBe(0);
    const rendered = output.mock.calls.flat().join("\n");
    expect(rendered).toContain("Visible");
    expect(rendered).not.toContain("\u001b");
    expect(rendered).not.toContain("\u0007");
  });

  it("shows a bounded list and the empty-list message from actual Rust responses", async () => {
    const output = vi.spyOn(console, "log").mockImplementation(() => {});
    vi.spyOn(console, "error").mockImplementation(() => {});
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ sessions: [] }), { status: 200 })));
    expect(await main(["sessions", "--runtime", "rust", "--url", "http://127.0.0.1:14567", "--token", "x"])).toBe(0);
    expect(output.mock.calls.flat().join(" ")).toContain("No sessions yet.");
    output.mockClear();
    const sessions = Array.from({ length: 25 }, (_, index) => ({ sessionId: `s${index}`, title: `Session ${index}` }));
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ sessions }), { status: 200 })));
    expect(await main(["sessions", "--runtime", "rust", "--url", "http://127.0.0.1:14567", "--token", "x"])).toBe(0);
    expect(output).toHaveBeenCalledTimes(20);
  });

  it("shows health but fails status when identity fails without inventing an auth source", async () => {
    vi.stubGlobal("fetch", vi.fn(async (url: string) => url.endsWith("/health")
      ? new Response(JSON.stringify({ status: "ok", serverKind: "lingxi-service", serverVersion: "0.1" }))
      : new Response(JSON.stringify({ message: "unauthorized" }), { status: 401 })));
    const output = vi.spyOn(console, "log").mockImplementation(() => {});
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(await main(["status", "--runtime", "rust", "--url", "http://127.0.0.1:14567", "--token", "bad"])).toBe(1);
    const rendered = output.mock.calls.flat().join("\n");
    expect(rendered).toContain("Version   0.1");
    expect(rendered).toContain("Auth      unavailable (identity check failed)");
    expect(rendered).not.toContain("Auth      explicit");
    expect(errors.mock.calls.flat().join(" ")).toContain("Rust status is incomplete");
  });

  it("refuses a green status when health lacks a service version", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ status: "ok", serverKind: "lingxi-service" }))));
    const output = vi.spyOn(console, "log").mockImplementation(() => {});
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(await main(["status", "--runtime", "rust", "--url", "http://127.0.0.1:14567", "--token", "x"])).toBe(1);
    expect(output).not.toHaveBeenCalled();
    expect(errors.mock.calls.flat().join(" ")).toContain("not a healthy Rust Lingxi service");
  });

  it("prints authenticated health but leaves status failed until Agent and model exist", async () => {
    vi.stubGlobal("fetch", vi.fn(async (url: string) => url.endsWith("/health")
      ? new Response(JSON.stringify({ status: "ok", serverKind: "lingxi-service", serverVersion: "0.1" }))
      : new Response(JSON.stringify({ principalId: "owner", credentialKind: "loopback_token", kind: "local_user", studioId: "studio-1" }))));
    const output = vi.spyOn(console, "log").mockImplementation(() => {});
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(await main(["status", "--runtime", "rust", "--url", "http://127.0.0.1:14567", "--token", "x"])).toBe(1);
    expect(output.mock.calls.flat().join(" ")).toContain("Studio    studio-1");
    expect(errors.mock.calls.flat().join(" ")).toContain("Agent and model are not available yet");
  });

  it("returns a diagnostic failure when Rust service cannot be reached", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => { throw new Error("connection refused"); }));
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(await main(["status", "--runtime", "rust", "--url", "http://127.0.0.1:14567"])).toBe(1);
    expect(error.mock.calls.flat().join(" ")).toContain("unreachable");
  });

  it("refuses to fake a model reply after identifying a real selected session", async () => {
    const routes: string[] = [];
    vi.stubGlobal("fetch", vi.fn(async (url: string) => {
      routes.push(new URL(url).pathname);
      if (url.endsWith("/health")) return new Response(JSON.stringify({ status: "ok", serverKind: "lingxi-service", serverVersion: "0.1" }));
      if (url.endsWith("/me")) return new Response(JSON.stringify({ principalId: "owner", credentialKind: "loopback_token" }));
      if (url.endsWith("/sessions")) return new Response(JSON.stringify({ sessions: [{ sessionId: "s1", title: "A" }] }));
      return new Response(JSON.stringify({ sessionId: "s1", title: "A" }));
    }));
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(await main(["continue", "s1", "--runtime", "rust", "--url", "http://127.0.0.1:14567", "--token", "x"])).toBe(1);
    expect(routes).toContain("/lingxi/v1/sessions/s1");
    expect(routes).not.toContain("/lingxi/v1/sessions/s1/execute");
    expect(error.mock.calls.flat().join(" ")).toContain("model/tool reply streaming are unavailable");
  });
});
