import { Hono } from "hono";
import fs from "fs/promises";
import os from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createConversationMapRoute } from "../server/routes/conversation-map.ts";

function makeApp(engine) {
  const app = new Hono();
  app.route("/api", createConversationMapRoute(engine));
  return app;
}

function messageEntry(id, parentId, timestamp, message) {
  return JSON.stringify({ type: "message", id, parentId, timestamp, message });
}

describe("conversation-map route", () => {
  let tmpDir;
  let agentsDir;
  let lingxiHome;

  beforeEach(async () => {
    tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), "conversation-map-test-"));
    agentsDir = path.join(tmpDir, "agents");
    lingxiHome = path.join(tmpDir, "lingxi-home");
    await fs.mkdir(path.join(agentsDir, "agent1", "sessions"), { recursive: true });
    await fs.mkdir(lingxiHome, { recursive: true });
  });

  afterEach(async () => {
    await fs.rm(tmpDir, { recursive: true, force: true });
  });

  it("folds session history into turns", async () => {
    const sessionFile = path.join(agentsDir, "agent1", "sessions", "s1.jsonl");
    const lines = [
      JSON.stringify({ type: "session", id: "11111111-2222-3333-4444-555555555555", version: 3, timestamp: "2026-09-19T00:00:00.000Z" }),
      messageEntry("e1", null, "2026-09-19T00:00:01.000Z", { role: "user", content: "第一问" }),
      messageEntry("e2", "e1", "2026-09-19T00:00:02.000Z", {
        role: "assistant",
        content: [
          { type: "text", text: "第一答" },
          { type: "tool_use", id: "tool-1", name: "bash", input: { command: "ls" } },
        ],
      }),
      messageEntry("e3", "e2", "2026-09-19T00:00:03.000Z", { role: "toolResult", content: "ok" }),
      messageEntry("e4", "e3", "2026-09-19T00:00:04.000Z", { role: "user", content: "第二问" }),
      messageEntry("e5", "e4", "2026-09-19T00:00:05.000Z", { role: "assistant", content: "中间答" }),
      messageEntry("e6", "e5", "2026-09-19T00:00:06.000Z", { role: "assistant", content: "最终答" }),
    ];
    await fs.writeFile(sessionFile, lines.join("\n") + "\n", "utf-8");

    const engine = {
      agentsDir,
      lingxiHome,
      getSessionIdForPath: () => null,
      getSessionManifest: () => null,
    };
    const app = makeApp(engine);

    const res = await app.request(`/api/conversation-map/turns?path=${encodeURIComponent(sessionFile)}`);
    const body = await res.json();

    expect(res.status).toBe(200);
    expect(body.sessionId).toBe(null);
    expect(body.turns).toHaveLength(2);

    const [first, second] = body.turns;
    expect(first.turnIndex).toBe(0);
    expect(first.questionEntryId).toBe("e1");
    expect(first.question).toBe("第一问");
    expect(first.questionAt).toBe("2026-09-19T00:00:01.000Z");
    expect(first.answerEntryId).toBe("e2");
    expect(first.answer).toBe("第一答");
    expect(first.answerAt).toBe("2026-09-19T00:00:02.000Z");
    // tool_use 块 + toolResult 各计一次
    expect(first.processCount).toBe(2);
    expect(first.entryIds).toEqual(["e1", "e2", "e3"]);
    expect(first.truncated).toBe(false);

    expect(second.turnIndex).toBe(1);
    expect(second.question).toBe("第二问");
    // 同一回合内后一条非空 assistant 文本覆盖前一条
    expect(second.answer).toBe("最终答");
    expect(second.answerEntryId).toBe("e6");
    expect(second.processCount).toBe(0);
    expect(second.entryIds).toEqual(["e4", "e5", "e6"]);
  });

  it("rejects a session path outside agentsDir", async () => {
    const outsideFile = path.join(tmpDir, "outside.jsonl");
    await fs.writeFile(outsideFile, "{}\n", "utf-8");
    const engine = {
      agentsDir,
      lingxiHome,
      getSessionIdForPath: () => null,
      getSessionManifest: () => null,
    };
    const app = makeApp(engine);

    const res = await app.request(`/api/conversation-map/turns?path=${encodeURIComponent(outsideFile)}`);
    const body = await res.json();

    expect(res.status).toBe(403);
    expect(body.error).toBe("Invalid session path");
  });

  it("returns the default layout when the file is missing and round-trips PUT/GET", async () => {
    const engine = {
      agentsDir,
      lingxiHome,
      getSessionIdForPath: () => null,
      getSessionManifest: () => null,
    };
    const app = makeApp(engine);

    const missingRes = await app.request("/api/conversation-map/layout");
    expect(missingRes.status).toBe(200);
    expect(await missingRes.json()).toEqual({
      version: 1,
      positions: {},
      collapsed: [],
      updatedAt: null,
    });

    const putRes = await app.request("/api/conversation-map/layout", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        positions: { "session-a": { x: 12.4, y: -7.6 } },
        collapsed: ["session-b"],
      }),
    });
    const putBody = await putRes.json();
    expect(putRes.status).toBe(200);
    expect(putBody.ok).toBe(true);
    expect(typeof putBody.updatedAt).toBe("string");

    const getRes = await app.request("/api/conversation-map/layout");
    const getBody = await getRes.json();
    expect(getRes.status).toBe(200);
    // 坐标写入时取整
    expect(getBody.positions).toEqual({ "session-a": { x: 12, y: -8 } });
    expect(getBody.collapsed).toEqual(["session-b"]);
    expect(getBody.version).toBe(1);
    expect(getBody.updatedAt).toBe(putBody.updatedAt);

    // 只传 collapsed 时 positions 保持合并前的值
    const mergeRes = await app.request("/api/conversation-map/layout", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ collapsed: ["session-c"] }),
    });
    expect(mergeRes.status).toBe(200);
    const afterMerge = await (await app.request("/api/conversation-map/layout")).json();
    expect(afterMerge.positions).toEqual({ "session-a": { x: 12, y: -8 } });
    expect(afterMerge.collapsed).toEqual(["session-c"]);
  });

  it("replaces positions wholesale when replacePositions is true", async () => {
    const engine = {
      agentsDir,
      lingxiHome,
      getSessionIdForPath: () => null,
      getSessionManifest: () => null,
    };
    const app = makeApp(engine);

    const seedRes = await app.request("/api/conversation-map/layout", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        positions: { "session-a": { x: 10, y: 20 }, "session-b": { x: 30, y: 40 } },
      }),
    });
    expect(seedRes.status).toBe(200);

    // 默认合并：session-a 保留
    const mergeRes = await app.request("/api/conversation-map/layout", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ positions: { "session-b": { x: 50, y: 60 } } }),
    });
    expect(mergeRes.status).toBe(200);
    const afterMerge = await (await app.request("/api/conversation-map/layout")).json();
    expect(afterMerge.positions).toEqual({
      "session-a": { x: 10, y: 20 },
      "session-b": { x: 50, y: 60 },
    });

    // replacePositions=true：整体替换，旧 key 被丢弃
    const replaceRes = await app.request("/api/conversation-map/layout", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        positions: {},
        replacePositions: true,
        collapsed: ["keep-me"],
      }),
    });
    expect(replaceRes.status).toBe(200);
    const afterReplace = await (await app.request("/api/conversation-map/layout")).json();
    expect(afterReplace.positions).toEqual({});
    expect(afterReplace.collapsed).toEqual(["keep-me"]);
  });

  it("rejects invalid PUT bodies", async () => {
    const engine = {
      agentsDir,
      lingxiHome,
      getSessionIdForPath: () => null,
      getSessionManifest: () => null,
    };
    const app = makeApp(engine);

    const notJsonRes = await app.request("/api/conversation-map/layout", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: "not-json",
    });
    expect(notJsonRes.status).toBe(400);
    expect((await notJsonRes.json()).error).toBe("invalid_body");

    const emptyRes = await app.request("/api/conversation-map/layout", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ unrelated: true }),
    });
    expect(emptyRes.status).toBe(400);
    expect((await emptyRes.json()).error).toBe("invalid_body");
  });

  it("clamps coordinates and drops non-numeric values on PUT", async () => {
    const engine = {
      agentsDir,
      lingxiHome,
      getSessionIdForPath: () => null,
      getSessionManifest: () => null,
    };
    const app = makeApp(engine);

    const putRes = await app.request("/api/conversation-map/layout", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        positions: {
          "too-big": { x: 999999999, y: -999999999 },
          "not-numeric": { x: "1", y: 2 },
          "not-object": 42,
          ok: { x: 0.5, y: 1.5 },
        },
      }),
    });
    expect(putRes.status).toBe(200);

    const body = await (await app.request("/api/conversation-map/layout")).json();
    expect(body.positions).toEqual({
      "too-big": { x: 100000, y: -100000 },
      ok: { x: 1, y: 2 },
    });
  });
});
