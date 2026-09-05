import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";
import { isCorsOriginAllowed } from "../server/http/cors-policy.ts";

describe("CORS policy", () => {
  it("keeps PATCH in the allow-methods list for cross-origin renderer preflights", () => {
    // file://（打包/dev 渲染页）与 loopback web 前端发 PATCH 前必过 CORS 预检；
    // 方法列表漏 PATCH 会让 /api/agents/:id/skills/:name 等单项启停路由整体失效
    const source = fs.readFileSync(
      path.join(__dirname, "../server/index.ts"),
      "utf-8",
    );
    const match = source.match(/"Access-Control-Allow-Methods",\s*"([^"]+)"/);
    expect(match).toBeTruthy();
    const methods = (match?.[1] || "").split(",").map(item => item.trim());
    for (const verb of ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"]) {
      expect(methods).toContain(verb);
    }
  });

  it("allows production Electron file-origin frontends to pair with a LAN server", () => {
    expect(isCorsOriginAllowed({ origin: "null" })).toBe(true);
    expect(isCorsOriginAllowed({ origin: "file://" })).toBe(true);
  });

  it("keeps the default browser allowance limited to loopback web frontends", () => {
    expect(isCorsOriginAllowed({ origin: "http://localhost:5173" })).toBe(true);
    expect(isCorsOriginAllowed({ origin: "http://127.0.0.1:14500" })).toBe(true);
    expect(isCorsOriginAllowed({ origin: "http://192.168.31.75:5173" })).toBe(false);
  });

  it("honors an explicit CORS origin as a strict override", () => {
    expect(isCorsOriginAllowed({
      origin: "https://studio.example.com",
      configuredOrigin: "https://studio.example.com",
    })).toBe(true);
    expect(isCorsOriginAllowed({
      origin: "null",
      configuredOrigin: "https://studio.example.com",
    })).toBe(false);
  });
});
