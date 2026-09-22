import { afterEach, describe, expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { readLocalServerInfo, resolveCliLingxiHome, resolveConnection } from "../cli/local-server.ts";
import { resolveLingxiHome } from "../shared/hana-runtime-paths.ts";

function makeTmpDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "hana-cli-server-"));
}

describe("CLI local server discovery", () => {
  let tmpDir = null;

  afterEach(() => {
    if (tmpDir) fs.rmSync(tmpDir, { recursive: true, force: true });
    tmpDir = null;
  });

  it("reports a missing server-info file explicitly", () => {
    tmpDir = makeTmpDir();
    const result = readLocalServerInfo({ lingxiHome: tmpDir });

    expect(result).toMatchObject({
      ok: false,
      reason: "missing_server_info",
    });
  });

  it("builds a loopback connection from server-info", () => {
    tmpDir = makeTmpDir();
    fs.writeFileSync(path.join(tmpDir, "server-info.json"), JSON.stringify({
      pid: process.pid,
      port: 14500,
      token: "hana_token",
      version: "1.2.3",
    }), "utf8");

    expect(resolveConnection({ lingxiHome: tmpDir })).toMatchObject({
      ok: true,
      baseUrl: "http://127.0.0.1:14500",
      token: "hana_token",
      source: "server-info",
      queryTokenAllowed: true,
    });
  });

  it("uses explicit URL without allowing query-token transport by default", () => {
    expect(resolveConnection({ url: "http://example.com/", token: "device" })).toMatchObject({
      ok: true,
      baseUrl: "http://example.com",
      token: "device",
      source: "explicit",
      queryTokenAllowed: false,
    });
  });

  // ── P01-A09：遗留配置对象经真实 adapter 进入 strict 核心 ──
  // cli/local-server.ts 在 tsconfig.core-contracts.json strict 范围内；
  // server-info.json 是外部 JSON，正常/字段缺失两例都必须显式失败，不隐藏。
  it("A09 normal: a complete server-info.json resolves with explicit fields", () => {
    tmpDir = makeTmpDir();
    fs.writeFileSync(path.join(tmpDir, "server-info.json"), JSON.stringify({
      pid: process.pid,
      port: 14501,
      token: "hana_token",
    }), "utf8");

    const result = readLocalServerInfo({ lingxiHome: tmpDir });
    expect(result.ok).toBe(true);
    if (!result.ok) throw new Error("unreachable");
    expect(result.baseUrl).toBe("http://127.0.0.1:14501");
    expect(result.token).toBe("hana_token");
  });

  it("A09 missing field: server-info.json without token fails explicitly as incomplete", () => {
    tmpDir = makeTmpDir();
    fs.writeFileSync(path.join(tmpDir, "server-info.json"), JSON.stringify({
      pid: process.pid,
      port: 14502,
    }), "utf8");

    expect(readLocalServerInfo({ lingxiHome: tmpDir })).toMatchObject({
      ok: false,
      reason: "incomplete_server_info",
    });
  });

  it("A09 malformed: unparseable server-info.json fails explicitly as invalid", () => {
    tmpDir = makeTmpDir();
    fs.writeFileSync(path.join(tmpDir, "server-info.json"), "{not json", "utf8");

    expect(readLocalServerInfo({ lingxiHome: tmpDir })).toMatchObject({
      ok: false,
      reason: "invalid_server_info",
    });
  });

  it("A09 stale: a dead pid is reported as stale when process checks are on", () => {
    tmpDir = makeTmpDir();
    fs.writeFileSync(path.join(tmpDir, "server-info.json"), JSON.stringify({
      pid: 0xFFFFFF, // 假定不存在的 pid（范围上界，非当前进程）
      port: 14503,
      token: "hana_token",
    }), "utf8");

    expect(readLocalServerInfo({ lingxiHome: tmpDir })).toMatchObject({
      ok: false,
      reason: "stale_server_info",
    });
  });

  // ── P01-A11（LINGXI_HOME 统一的同语义证明）：统一前 resolveCliLingxiHome 的
  // 既有语义（env 缺省→~/.lingxi、~ 展开、空白 trim、相对路径 resolve）在统一
  // 到 shared/hana-runtime-paths 后逐例保持一致。
  it("A11: resolveCliLingxiHome keeps legacy semantics on the shared authority", () => {
    // 未设置 → 默认 ~/.lingxi
    expect(resolveCliLingxiHome({})).toBe(path.join(os.homedir(), ".lingxi"));
    // 空串/空白 → 默认
    expect(resolveCliLingxiHome({ LINGXI_HOME: "" })).toBe(path.join(os.homedir(), ".lingxi"));
    expect(resolveCliLingxiHome({ LINGXI_HOME: "   " })).toBe(path.join(os.homedir(), ".lingxi"));
    // ~ 展开
    expect(resolveCliLingxiHome({ LINGXI_HOME: "~" })).toBe(os.homedir());
    expect(resolveCliLingxiHome({ LINGXI_HOME: "~/x" })).toBe(path.join(os.homedir(), "x"));
    // 相对/绝对路径 resolve + trim
    expect(resolveCliLingxiHome({ LINGXI_HOME: "/tmp/hana-home " })).toBe("/tmp/hana-home");
    expect(resolveCliLingxiHome({ LINGXI_HOME: "rel/home" }))
      .toBe(resolveLingxiHome("rel/home"));
  });
});
