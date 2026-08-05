import { describe, expect, it, vi } from "vitest";
import { buildDesktopSlashSessionRef } from "../server/routes/chat.ts";

describe("desktop slash session identity", () => {
  it("adds the stable session id resolved from the locator", () => {
    const engine = {
      getSessionIdForPath: vi.fn(() => "sess_a"),
    };

    expect(buildDesktopSlashSessionRef(engine, "agent-a", "/sessions/a.jsonl")).toEqual({
      kind: "desktop",
      agentId: "agent-a",
      sessionId: "sess_a",
      sessionPath: "/sessions/a.jsonl",
    });
  });

  it("keeps the locator for compatibility but does not synthesize identity", () => {
    const engine = {
      getSessionIdForPath: vi.fn(() => null),
    };

    expect(buildDesktopSlashSessionRef(engine, "agent-a", "/sessions/legacy.jsonl")).toEqual({
      kind: "desktop",
      agentId: "agent-a",
      sessionPath: "/sessions/legacy.jsonl",
    });
  });
});
