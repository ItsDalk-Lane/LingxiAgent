import { describe, expect, it } from "vitest";

import {
  description,
  execute,
  name,
  parameters,
  promptGuidelines,
  sessionPermission,
} from "../plugins/media/tools/get-guide.ts";
import {
  MEDIA_GENERATION_GUIDE,
  MEDIA_GENERATION_GUIDE_VERSION,
} from "../plugins/media/lib/generation-guide.ts";

describe("media_get-guide", () => {
  it("exposes a read-only tool that takes no arguments", () => {
    expect(name).toBe("get-guide");
    expect(description).toBeTruthy();
    expect(sessionPermission).toEqual({ readOnly: true });
    expect(parameters.properties).toEqual({});
  });

  it("returns the guide text with its version", async () => {
    const result = await execute();
    expect(result.details).toEqual({ version: MEDIA_GENERATION_GUIDE_VERSION });
    expect(result.content).toEqual([{ type: "text", text: MEDIA_GENERATION_GUIDE }]);
  });

  it("keeps the routing and async contract the generation tools depend on", async () => {
    const { content } = await execute();
    const text = content[0].text;
    for (const marker of [
      "media_generate-image",
      "media_generate-video",
      "media_describe-options",
      "Generation is asynchronous",
      "do not call stage_files",
    ]) {
      expect(text).toContain(marker);
    }
  });

  // 这份指南以前是 skill 文件，靠 system prompt 里的绝对路径让模型读盘。内置插件装在带版本号的
  // 服务端运行时目录里，那条路径被冻结进会话快照后会在下次更新时失效。指南正文和它的调用提示
  // 都必须保持零路径，否则同一个坑会以另一种形式回来。
  it("never leaks a filesystem path into the model context", async () => {
    const { content } = await execute();
    for (const text of [content[0].text, promptGuidelines, description]) {
      expect(text).not.toMatch(/SKILL\.md/);
      expect(text).not.toMatch(/(^|[\s"'`(])[~/][\w.@-]+\//m);
    }
  });

  it("tells the model to load the guide before the generation tools", () => {
    expect(promptGuidelines).toContain("media_get-guide");
    for (const tool of ["media_generate-image", "media_generate-video", "media_describe-options"]) {
      expect(promptGuidelines).toContain(tool);
    }
  });
});
