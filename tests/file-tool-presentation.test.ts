import path from "path";
import { describe, expect, it, vi } from "vitest";
import { createPresentedLsTool, createPresentedReadTool } from "../lib/sandbox/file-tool-presentation.ts";

describe("file tool presentation metadata", () => {
  it("counts the same enhanced read buffer and keeps the requested range", async () => {
    const readFile = vi.fn(async () => Buffer.from("一\n二\n三\n四"));
    const read = createPresentedReadTool(process.cwd(), { readFile, access: async () => {} });
    const result = await read.execute("range", { path: "notes.md", offset: 2, limit: 2 });

    expect(readFile).toHaveBeenCalledTimes(1);
    expect(result.content[0]).toMatchObject({ text: expect.stringContaining("二\n三") });
    expect(result.details.read).toMatchObject({
      startLine: 2, totalLines: 4, displayedLines: 2, language: "md", truncated: true,
    });
  });

  it("counts extracted document text and isolates simultaneous calls", async () => {
    let release: (() => void) | undefined;
    const waiting = new Promise<void>(resolve => { release = resolve; });
    const read = createPresentedReadTool(process.cwd(), {
      access: async () => {},
      readFile: async filePath => {
        if (filePath.endsWith("slow.docx")) { await waiting; return Buffer.from("段落一\n段落二\n"); }
        return Buffer.from("single");
      },
    });
    const slow = read.execute("document", { path: "slow.docx", limit: 1 });
    const quick = await read.execute("quick", { path: "quick.txt" });
    release!();
    const document = await slow;

    expect(quick.details.read).toMatchObject({ totalLines: 1, displayedLines: 1, truncated: false });
    expect(document.details.read).toMatchObject({ totalLines: 3, displayedLines: 1, language: "text" });
  });

  it("does not count binary image bytes as text lines", async () => {
    const read = createPresentedReadTool(process.cwd(), {
      access: async () => {},
      readFile: async () => Buffer.from("image bytes\nnot source text"),
    });
    const result = await read.execute("image", { path: "picture.bmp" });
    expect(result.details?.read).toBeUndefined();
  });

  it("reports zero displayed lines when the first line exceeds the SDK byte limit", async () => {
    const read = createPresentedReadTool(process.cwd(), {
      access: async () => {},
      readFile: async () => Buffer.from("x".repeat(60 * 1024) + "\nlast"),
    });
    const result = await read.execute("large-line", { path: "large.txt" });
    expect(result.details.read).toMatchObject({ totalLines: 2, displayedLines: 0, truncated: true });
  });

  it("captures only successfully listed entries in their actual order and marks limits", async () => {
    const cwd = process.cwd();
    const stat = vi.fn(async filePath => {
      if (filePath.endsWith("broken")) throw new Error("not available");
      return { isDirectory: () => filePath === cwd || filePath.endsWith("folder") };
    });
    const ls = createPresentedLsTool(cwd, {
      exists: async () => true,
      stat,
      readdir: async () => ["z.txt", "folder", "broken", "a.txt"],
    });
    const result = await ls.execute("list", { path: ".", limit: 2 });

    expect(result.content[0]).toMatchObject({ text: expect.stringContaining("a.txt\nfolder/") });
    expect(result.details.search).toEqual({
      kind: "ls", basePath: cwd, files: [{ path: "a.txt" }, { path: "folder/" }], fileCount: 2, truncated: true,
    });
    expect(stat).not.toHaveBeenCalledWith(path.join(cwd, "z.txt"));
  });
});
