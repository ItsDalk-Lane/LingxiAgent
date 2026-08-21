import fs from "fs";
import path from "path";
import { describe, expect, it } from "vitest";

function source(relativePath: string) {
  return fs.readFileSync(path.join(process.cwd(), relativePath), "utf-8");
}

function between(text: string, start: string, end: string) {
  const startIndex = text.indexOf(start);
  const endIndex = text.indexOf(end, startIndex + start.length);
  expect(startIndex).toBeGreaterThanOrEqual(0);
  expect(endIndex).toBeGreaterThan(startIndex);
  return text.slice(startIndex, endIndex);
}

describe("fresh credential network boundaries", () => {
  it("uses the fresh auxiliary resolver in both server text-generation bus handlers", () => {
    const text = source("server/index.ts");
    const utilityHandler = between(
      text,
      'hub.eventBus.handle("utility:call-text"',
      'hub.eventBus.handle("model:sample-text"',
    );
    const sampleHandler = between(
      text,
      'hub.eventBus.handle("model:sample-text"',
      'hub.eventBus.handle("usage:list"',
    );

    for (const handler of [utilityHandler, sampleHandler]) {
      expect(handler).toContain('await engine.resolveAuxiliaryModelFresh("summarize"');
      expect(handler).not.toMatch(/engine\.resolveUtilityConfig(Fresh)?\s*\(/);
      // callText 可能经 ModelTraceScope 包装（Phase 4）——契约是"真实经
      // callText 发送"而非固定调用文本形状。
      expect(handler).toMatch(/await\s+runWithModelTraceRoot[\s\S]*?=>\s*callText\(|await callText/);
      expect(handler).not.toContain("callTextConfigFromUtilityConfig");
      expect(handler).toContain("resolved.api");
      expect(handler).toContain("resolved.apiKey");
      expect(handler).not.toContain("apiKey: utility.api_key");
      expect(handler).not.toContain("baseUrl: utility.base_url");
    }
  });

  it("keeps the channel memory request behind the fresh auxiliary boundary", () => {
    const text = source("hub/channel-router.ts");
    const handler = between(text, "async _memorySummarize", "_clearPreviousChannelMemoryFacts");
    expect(handler).toContain('await engine.resolveAuxiliaryModelFresh("memory"');
    expect(handler).not.toMatch(/engine\.resolveUtilityConfig(Fresh)?\s*\(/);
    expect(handler).toContain("await (callText as any)");
  });
});
