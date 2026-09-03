import { createCanvas, loadImage } from "@napi-rs/canvas";
import { describe, expect, it } from "vitest";
import { trimRenderedPage } from "../lib/document-extract/trim-page.ts";

describe("扫描页纯白边缘裁剪", () => {
  it("保留所有非白像素和浅色内容，不改变字号与颜色", async () => {
    const canvas = createCanvas(400, 600), context = canvas.getContext("2d");
    context.fillStyle = "white"; context.fillRect(0, 0, 400, 600);
    context.fillStyle = "black"; context.fillRect(100, 200, 50, 40);
    context.fillStyle = "rgb(254, 255, 255)"; context.fillRect(200, 300, 1, 1);
    const image = await loadImage(Buffer.from(await trimRenderedPage(canvas.toBuffer("image/png"))));
    expect(image.width).toBe(133); expect(image.height).toBe(133);
    const output = createCanvas(image.width, image.height), actual = output.getContext("2d");
    actual.drawImage(image, 0, 0);
    expect([...actual.getImageData(16, 16, 1, 1).data]).toEqual([0, 0, 0, 255]);
    expect([...actual.getImageData(116, 116, 1, 1).data]).toEqual([254, 255, 255, 255]);
  });

  it.each(["white", "black"])("全%s页面不裁切", async (color) => {
    const canvas = createCanvas(400, 600), context = canvas.getContext("2d");
    context.fillStyle = color; context.fillRect(0, 0, 400, 600);
    const input = canvas.toBuffer("image/png");
    expect(await trimRenderedPage(input)).toBe(input);
  });

  it("保留页边内容，不裁掉四角标记", async () => {
    const canvas = createCanvas(400, 600), context = canvas.getContext("2d");
    context.fillStyle = "white"; context.fillRect(0, 0, 400, 600);
    context.fillStyle = "black"; context.fillRect(0, 0, 1, 1); context.fillRect(399, 599, 1, 1);
    const input = canvas.toBuffer("image/png");
    expect(await trimRenderedPage(input)).toBe(input);
  });

  it("取消后不继续解码", async () => {
    const controller = new AbortController(); controller.abort();
    await expect(trimRenderedPage(new Uint8Array(), controller.signal)).rejects.toMatchObject({ name: "AbortError" });
  });
});
