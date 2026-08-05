import { describe, expect, it } from "vitest";
import { Hub } from "../hub/index.ts";

describe("media generation agent surface", () => {
  it("does not let the Hub shadow native media:generate-image with the old compatibility bridge", () => {
    const engine = {
      setHubCallbacks: () => {},
      setEventBus: () => {},
    };
    const hub = new Hub({ engine } as any);

    expect(hub.eventBus.hasHandler("media:generate-image")).toBe(false);
  });
});
