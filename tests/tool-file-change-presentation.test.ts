import path from "path";
import { applyPatch } from "diff";
import { describe, expect, it, vi } from "vitest";
import { createEditTool, createWriteTool } from "../lib/pi-sdk/index.ts";
import { createResourceIoToolOperations } from "../lib/resource-io/pi-tool-operations.ts";
import { FILE_PRESENTATION_MAX_BYTES } from "../lib/resource-io/file-change-presentation.ts";

function fileTools(initial: Record<string, string> = {}) {
  const cwd = process.cwd();
  const files = new Map(Object.entries(initial).map(([name, content]) => [path.resolve(cwd, name), content]));
  const resourceIO = {
    stat: vi.fn(async ref => ({ exists: files.has(ref.path) || ref.path === cwd, isDirectory: ref.path === cwd,
      version: { size: Buffer.byteLength(files.get(ref.path) ?? "") } })),
    read: vi.fn(async ref => {
      if (!files.has(ref.path)) throw Object.assign(new Error("missing"), { code: "ENOENT" });
      return { content: Buffer.from(files.get(ref.path)!) };
    }),
    write: vi.fn(async (ref, content) => {
      const changeType = files.has(ref.path) ? "modified" : "created";
      files.set(ref.path, String(content));
      return { changeType };
    }),
    mkdir: vi.fn(async () => ({})),
  };
  const operations = createResourceIoToolOperations({ cwd, resourceIO: resourceIO as any });
  const write = createWriteTool(cwd, { operations: operations.write });
  const edit = createEditTool(cwd, { operations: operations.edit });
  return {
    files, resourceIO,
    write: (name: string, content: string, id = name) => operations.withFileChangeCapture("write", () => write.execute(id, { path: name, content })),
    edit: (name: string, oldText: string, newText: string) => operations.withFileChangeCapture("edit", () => edit.execute(name, { path: name, edits: [{ oldText, newText }] })),
  };
}

describe("file changes captured inside the SDK mutation queue", () => {
  it("distinguishes new files, overwrites, and empty writes using the actual result", async () => {
    const tools = fileTools({ "existing.txt": "old\nline" });
    const created = await tools.write("new.txt", "new");
    const overwritten = await tools.write("existing.txt", "changed");
    const emptied = await tools.write("existing.txt", "");
    const emptyNew = await tools.write("empty.txt", "");

    expect(created.details.fileChange).toMatchObject({ changeType: "created", beforeAvailable: true, content: "new" });
    expect(applyPatch("", created.details.fileChange.patch)).toBe("new");
    expect(overwritten.details.fileChange.changeType).toBe("modified");
    expect(applyPatch("old\nline", overwritten.details.fileChange.patch)).toBe("changed");
    expect(applyPatch("changed", emptied.details.fileChange.patch)).toBe("");
    expect(emptyNew.details.fileChange).toMatchObject({ changeType: "created", beforeAvailable: true, content: "" });
  });

  it("keeps concurrent same-file snapshots in queue order and separate-file captures isolated", async () => {
    const tools = fileTools({ "queue.txt": "zero", "other.txt": "other-before" });
    const [first, second, other] = await Promise.all([
      tools.write("queue.txt", "one", "first"),
      tools.write("queue.txt", "two", "second"),
      tools.write("other.txt", "other-after"),
    ]);
    expect(applyPatch("zero", first.details.fileChange.patch)).toBe("one");
    expect(applyPatch("one", second.details.fileChange.patch)).toBe("two");
    expect(applyPatch("other-before", other.details.fileChange.patch)).toBe("other-after");
  });

  it("does not block permitted writes or invent empty-before diffs when reading is denied", async () => {
    const tools = fileTools({ "private.txt": "unreadable old" });
    tools.resourceIO.read.mockRejectedValue(Object.assign(new Error("read denied"), { code: "EACCES" }));
    const result = await tools.write("private.txt", "allowed write");

    expect(tools.resourceIO.write).toHaveBeenCalledTimes(1);
    expect(result.details.fileChange).toMatchObject({
      changeType: "modified", beforeAvailable: false, reason: "before_permission_denied", content: "allowed write",
    });
    expect(result.details.fileChange.patch).toBeUndefined();
  });

  it("bounds oversized metadata and avoids reading a known oversized previous file", async () => {
    const tools = fileTools({ "large.txt": "a".repeat(FILE_PRESENTATION_MAX_BYTES + 1) });
    const result = await tools.write("large.txt", "新".repeat(FILE_PRESENTATION_MAX_BYTES));
    expect(tools.resourceIO.read).not.toHaveBeenCalled();
    expect(tools.resourceIO.write).toHaveBeenCalledTimes(1);
    expect(result.details.fileChange).toMatchObject({ beforeAvailable: false, reason: "before_content_too_large", truncated: true });
    expect(Buffer.byteLength(result.details.fileChange.content)).toBeLessThanOrEqual(FILE_PRESENTATION_MAX_BYTES);
    expect(result.details.fileChange.content).not.toContain("�");
    expect(result.details.fileChange.patch).toBeUndefined();
  });

  it("preserves the exact patch returned by the SDK edit result", async () => {
    const tools = fileTools({ "edit.txt": "before\nkeep" });
    const result = await tools.edit("edit.txt", "before", "after");
    expect(result.details.fileChange.patch).toBe(result.details.patch);
    expect(result.details.fileChange.content).toBe("after\nkeep");
    expect(applyPatch("before\nkeep", result.details.fileChange.patch)).toBe("after\nkeep");
  });
});
