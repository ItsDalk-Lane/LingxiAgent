import { Hono } from "hono";
import { describe, expect, it, beforeEach, afterEach, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";

/**
 * 消息级用户反馈沉淀：POST /api/agents/:id/experience/feedback
 * 关键契约：
 *   - 赞/踩分别落到固定分类，内容行带日期/标记/摘录/会话短 id
 *   - 用户显式动作不受 experience.enabled 门控（不静默丢弃），但状态要带回
 *   - 重复反馈按 recordEntry 的内容查重返回 duplicate
 */
describe("agents route: /agents/:id/experience/feedback", () => {
  let tmpDir;
  let app;
  let engine;

  beforeEach(async () => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-feedback-"));
    fs.mkdirSync(path.join(tmpDir, "agent-1"), { recursive: true });
    fs.writeFileSync(path.join(tmpDir, "agent-1", "config.yaml"), "agent: {}\n", "utf-8");
    const { createAgentsRoute } = await import("../server/routes/agents.ts");
    app = new Hono();
    engine = {
      agentsDir: tmpDir,
      getAgent: vi.fn(() => ({ experienceEnabled: false })),
    };
    app.route("/api", createAgentsRoute(engine));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  async function post(body: any) {
    const res = await app.request("/api/agents/agent-1/experience/feedback", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    return { res, data: await res.json().catch(() => null) };
  }

  it("赞：落到「用户认可的做法」，内容行含 ✓/日期/摘录/会话短 id，不受 experience.enabled 门控", async () => {
    const { res, data } = await post({
      rating: "up",
      excerpt: "帮我修好了检索超时的问题",
      sessionId: "sess-abcd1234-5678",
    });
    expect(res.status).toBe(200);
    expect(data.ok).toBe(true);
    expect(data.added).toBe(true);
    expect(data.experienceEnabled).toBe(false);
    expect(data.category).toBe("用户认可的做法");

    const files = fs.readdirSync(path.join(tmpDir, "agent-1", "experience"));
    expect(files.length).toBe(1);
    const doc = fs.readFileSync(path.join(tmpDir, "agent-1", "experience", files[0]), "utf-8");
    expect(doc).toContain("✓ 帮我修好了检索超时的问题");
    expect(doc).toContain("（会话 sess-abc）");
    // 明文分类标题在索引文件里（文档正文只存 base64url 标题注释）
    const index = fs.readFileSync(path.join(tmpDir, "agent-1", "experience.md"), "utf-8");
    expect(index).toContain("用户认可的做法");
  });

  it("踩：落到「用户不认可的做法」，标记为 ✗", async () => {
    const { res, data } = await post({
      rating: "down",
      excerpt: "把简单问题答得很啰嗦",
      sessionId: "sess-abcd1234",
    });
    expect(res.status).toBe(200);
    expect(data.category).toBe("用户不认可的做法");
    const files = fs.readdirSync(path.join(tmpDir, "agent-1", "experience"));
    const doc = fs.readFileSync(path.join(tmpDir, "agent-1", "experience", files[0]), "utf-8");
    expect(doc).toContain("✗ 把简单问题答得很啰嗦");
    const index = fs.readFileSync(path.join(tmpDir, "agent-1", "experience.md"), "utf-8");
    expect(index).toContain("用户不认可的做法");
  });

  it("相同内容重复反馈返回 duplicate 且不重复写库", async () => {
    await post({ rating: "up", excerpt: "同一条反馈", sessionId: "sess-1" });
    const { data } = await post({ rating: "up", excerpt: "同一条反馈", sessionId: "sess-1" });
    expect(data.duplicate).toBe(true);
    expect(data.added).toBe(false);
    const files = fs.readdirSync(path.join(tmpDir, "agent-1", "experience"));
    const doc = fs.readFileSync(path.join(tmpDir, "agent-1", "experience", files[0]), "utf-8");
    expect(doc.match(/同一条反馈/g)?.length).toBe(1);
  });

  it("非法 rating / 空 excerpt → 400", async () => {
    const bad1 = await post({ rating: "meh", excerpt: "x" });
    expect(bad1.res.status).toBe(400);
    const bad2 = await post({ rating: "up", excerpt: "   " });
    expect(bad2.res.status).toBe(400);
  });

  it("不存在的 agent → 404", async () => {
    const res = await app.request("/api/agents/nope/experience/feedback", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ rating: "up", excerpt: "x" }),
    });
    expect(res.status).toBe(404);
  });
});
