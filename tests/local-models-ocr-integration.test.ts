import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

import { extractDocument } from "../lib/document-extract/index.ts";
import { readOfficeDocument } from "../plugins/office/lib/read-document.ts";

const roots: string[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

describe("本地 OCR 文档接线", () => {
  it("图片跳过普通文档解析并直接调用 OCR", async () => {
    const png = Buffer.concat([
      Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
      Buffer.from("fixture"),
    ]);
    const loadApi = vi.fn();
    const ocr = vi.fn(async () => ({
      modelId: "paddleocr-vl-0.9b",
      output: { markdown: "识别结果", text: "识别结果", warnings: [] },
    }));

    await expect(extractDocument({ buffer: png, filename: "shot.png" }, { loadApi, ocr }))
      .resolves.toEqual({
        ok: true,
        markdown: "识别结果",
        format: "ocr",
        warnings: ["ocr:paddleocr-vl-0.9b"],
      });
    expect(loadApi).not.toHaveBeenCalled();
    expect(ocr).toHaveBeenCalledWith(expect.objectContaining({ mime: "image/png" }));
  });

  it("扫描 PDF 逐页光栅化后识别，并保留页码边界", async () => {
    const ocr = vi.fn(async ({ image }) => ({
      modelId: "glm-ocr",
      output: { markdown: `文字-${image[0]}`, warnings: [] },
    }));
    const renderPdfPages = vi.fn(async () => [
      { pageNumber: 1, image: new Uint8Array([11]), mime: "image/png" },
      { pageNumber: 2, image: new Uint8Array([22]), mime: "image/png" },
    ]);
    const loadApi = async () => ({
      formatFromBytes: () => "pdf",
      formatFromExtension: () => "pdf",
      toMarkdownBytes: async () => { throw new Error("image-only PDF needs OCR"); },
    } as any);

    const result = await extractDocument({
      buffer: Buffer.from("%PDF-test"),
      filename: "scan.pdf",
      ocrMaxPages: 2,
      ocrMaxPixelsPerPage: 4_000_000,
    }, { loadApi, ocr, renderPdfPages });
    expect(result).toMatchObject({
      ok: true,
      format: "ocr",
      warnings: ["ocr:glm-ocr"],
    });
    if (result.ok) expect(result.markdown).toBe("## Page 1\n\n文字-11\n\n## Page 2\n\n文字-22");
    expect(renderPdfPages).toHaveBeenCalledWith(expect.any(Uint8Array), expect.objectContaining({
      maxPages: 2,
      maxPixelsPerPage: 4_000_000,
    }));
  });

  it("真实 PDF 页面渲染器能把扫描夹具转成有界 PNG", async () => {
    const filePath = path.join(process.cwd(), "tests", "fixtures", "document-extract", "sample-scanned.pdf");
    const ocr = vi.fn(async ({ image, mime }) => {
      expect(mime).toBe("image/png");
      expect(image.byteLength).toBeGreaterThan(100);
      return { modelId: "fake-ocr", output: { markdown: "渲染成功", warnings: [] } };
    });
    await expect(extractDocument({ filePath, ocrMaxPages: 1 }, { ocr })).resolves.toMatchObject({
      ok: true,
      markdown: "渲染成功",
      warnings: ["ocr:fake-ocr"],
    });
    expect(ocr).toHaveBeenCalledOnce();
  });

  it("Office 图片阅读透传 OCR 覆盖参数", async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-office-ocr-"));
    roots.push(root);
    const filePath = path.join(root, "screen.png");
    fs.writeFileSync(filePath, Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]));
    const documentExtract = vi.fn(async () => ({
      ok: true,
      markdown: "Office OCR",
      format: "ocr",
      warnings: ["ocr:glm-ocr"],
    }));

    await expect(readOfficeDocument({
      filePath,
      ocrModelId: "local:glm-ocr@q4@1",
      ocrLanguage: "zh",
    }, { documentExtract })).resolves.toMatchObject({
      kind: "ocr",
      format: "markdown",
      content: "Office OCR",
      warnings: ["ocr:glm-ocr"],
    });
    expect(documentExtract).toHaveBeenCalledWith(expect.objectContaining({
      ocrModelId: "local:glm-ocr@q4@1",
      ocrLanguage: "zh",
    }));
  });
});
