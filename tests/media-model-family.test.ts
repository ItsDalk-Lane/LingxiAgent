import { describe, it, expect } from "vitest";
import { matchMediaFamilyDeclaration } from "../core/media/media-family.ts";

/** 与 lib/providers/agnes.ts、volcengine.ts 的声明形状一致的迷你锚点 */
function declaration(id: string, extra: Record<string, unknown> = {}): {
  id: string;
  displayName: string;
  modes: { id: string }[];
  [key: string]: unknown;
} {
  return { id, displayName: id, modes: [{ id: "text2image" }], ...extra };
}

describe("matchMediaFamilyDeclaration", () => {
  const agnesImage = [
    declaration("agnes-image-2.1-flash", { protocolId: "agnes-images", ratios: ["1:1", "3:2"] }),
  ];

  it("同系列新款继承旧款参数页（用户报告的 agnes 场景）", () => {
    const match = matchMediaFamilyDeclaration(agnesImage, "agnes-image-2.5-flash");
    expect(match?.inheritedFrom).toBe("agnes-image-2.1-flash");
    expect(match?.declaration.ratios).toEqual(["1:1", "3:2"]);
  });

  it("候选比锚点多词元（v2.0 → 2.5-flash）仍算同族", () => {
    const agnesVideo = [declaration("agnes-video-v2.0", { protocolId: "agnes-videos" })];
    expect(matchMediaFamilyDeclaration(agnesVideo, "agnes-video-2.5-flash")?.inheritedFrom)
      .toBe("agnes-video-v2.0");
  });

  it("火山场景：版本+日期新款继承同版本声明，跳过不同版本的锚点", () => {
    const volcengine = [
      declaration("doubao-seedream-3-0-t2i"),
      declaration("doubao-seedream-4-0-250828", { tier: "4.0" }),
      declaration("doubao-seedream-4-5-251128", { tier: "4.5" }),
      declaration("doubao-seedream-5-0-lite-260128"),
      declaration("doubao-seedream-5-0-260128", { tier: "5.0" }),
    ];
    const match = matchMediaFamilyDeclaration(volcengine, "doubao-seedream-4-0-20260415");
    expect(match?.inheritedFrom).toBe("doubao-seedream-4-0-250828");
    expect(match?.declaration.tier).toBe("4.0");
  });

  it("日期段不参与距离：4-0 新日期宁可继承 4-0 也不继承数值更近的 4-5", () => {
    const volcengine = [
      declaration("doubao-seedream-4-0-250828", { tier: "4.0" }),
      declaration("doubao-seedream-4-5-251128", { tier: "4.5" }),
    ];
    // 若把日期数值差计入距离，4-5-251128 会因日期更接近而胜出（错误锚点）。
    expect(matchMediaFamilyDeclaration(volcengine, "doubao-seedream-4-0-20260415")?.declaration.tier)
      .toBe("4.0");
  });

  it("带限定词的新款继承无限定词的同版本声明", () => {
    const volcengine = [
      declaration("doubao-seedream-4-0-250828", { tier: "4.0" }),
      declaration("doubao-seedream-5-0-lite-260128", { tier: "5.0-lite" }),
      declaration("doubao-seedream-5-0-260128", { tier: "5.0" }),
    ];
    const match = matchMediaFamilyDeclaration(volcengine, "doubao-seedream-5-0-pro-260628");
    expect(match?.inheritedFrom).toBe("doubao-seedream-5-0-260128");
    expect(match?.declaration.tier).toBe("5.0");
  });

  it("结构不同的家族分支不匹配（词元不同≠词元更多）", () => {
    const dashscope = [
      declaration("qwen-image-2.0-pro"),
      declaration("qwen-image-2.0"),
      declaration("qwen-image-edit-max"),
    ];
    // edit-ultra 与 edit-max 词数相同、词元不同（ultra≠max）：保守拒绝，不跨分支继承。
    expect(matchMediaFamilyDeclaration(dashscope, "qwen-image-edit-ultra")).toBeNull();
    expect(matchMediaFamilyDeclaration(dashscope, "qwen-image-3.0-flash")?.inheritedFrom)
      .toBe("qwen-image-2.0");
    expect(matchMediaFamilyDeclaration(dashscope, "qwen-image-vision-hd")).toBeNull();
  });

  it("锚点词元缺失时拒绝（品牌词不齐不算同族）", () => {
    const volcengine = [declaration("doubao-seedream-3-0-t2i")];
    expect(matchMediaFamilyDeclaration(volcengine, "doubao-seedream-4-0-20260415")).toBeNull();
    expect(matchMediaFamilyDeclaration(volcengine, "doubao-seedance-3-0-260101")).toBeNull();
  });

  it("候选是精确声明时直接返回 null（继承不走同名）", () => {
    const declarations = [declaration("agnes-image-2.1-flash"), declaration("agnes-image")];
    expect(matchMediaFamilyDeclaration(declarations, "AGNES-IMAGE-2.1-FLASH")).toBeNull();
    // 只多限定词、变量槽完全一致也算同族新款（与 5.0-pro 继承 5.0 同理）。
    expect(matchMediaFamilyDeclaration(declarations, "agnes-image-2.1-flash-mini")?.inheritedFrom)
      .toBe("agnes-image-2.1-flash");
  });

  it("锚点变量槽多于候选时拒绝（信息不足不继承）", () => {
    const volcengine = [declaration("doubao-seedream-4-0-250828")];
    expect(matchMediaFamilyDeclaration(volcengine, "doubao-seedream-4")).toBeNull();
  });

  it("无锚点、空声明、非法输入一律返回 null", () => {
    expect(matchMediaFamilyDeclaration([], "agnes-image-2.5-flash")).toBeNull();
    expect(matchMediaFamilyDeclaration(null, "agnes-image-2.5-flash")).toBeNull();
    expect(matchMediaFamilyDeclaration(agnesImage, "")).toBeNull();
    expect(matchMediaFamilyDeclaration([{} as any], "agnes-image-2.5-flash")).toBeNull();
  });
});
