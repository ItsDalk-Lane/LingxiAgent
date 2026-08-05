import { describe, expect, it } from "vitest";
import { getYuanVisual, moodLabelForYuan, normalizeYuan } from "../shared/yuan-visuals.ts";

describe("yuan visuals", () => {
  it("keeps the desktop and CLI yuan symbolism in one place", () => {
    expect(getYuanVisual("lingxi")).toMatchObject({
      symbol: "✿",
      moodLabel: "MOOD",
      accent: "#537D96",
      avatar: "Lingxi.png",
    });
    expect(getYuanVisual("butter")).toMatchObject({
      symbol: "❊",
      moodLabel: "PULSE",
      accent: "#5BA88C",
      avatar: "Butter.png",
    });
    expect(getYuanVisual("ming")).toMatchObject({
      symbol: "◈",
      moodLabel: "REFLECT",
      accent: "#8BA4B4",
      avatar: "Ming.png",
    });
  });

  it("falls back to lingxi for unknown yuan values", () => {
    expect(normalizeYuan("unknown")).toBe("lingxi");
    expect(moodLabelForYuan("unknown")).toBe("✿ MOOD");
  });
});
