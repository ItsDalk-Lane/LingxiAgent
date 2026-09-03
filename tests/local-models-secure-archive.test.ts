import fs from "node:fs";
import { randomBytes } from "node:crypto";
import os from "node:os";
import path from "node:path";
import archiver from "archiver";
import { afterEach, describe, expect, it } from "vitest";
import { extractLocalModelZip } from "../lib/local-models/index.ts";

const roots: string[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

function tempRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-local-model-archive-"));
  roots.push(root);
  return root;
}

function buildZip(
  zipPath: string,
  entries: Array<{ name: string; content?: string | Buffer; symlinkTarget?: string }>,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const output = fs.createWriteStream(zipPath);
    const archive = archiver("zip", { zlib: { level: 9 } });
    output.on("close", resolve);
    output.on("error", reject);
    archive.on("error", reject);
    archive.pipe(output);
    for (const entry of entries) {
      if (entry.symlinkTarget) archive.symlink(entry.name, entry.symlinkTarget);
      else archive.append(entry.content ?? "", { name: entry.name });
    }
    void archive.finalize();
  });
}

describe("local model secure archive", () => {
  it("streams only manifest-declared files and validates CRC/size", async () => {
    const root = tempRoot();
    const zipPath = path.join(root, "model.zip");
    await buildZip(zipPath, [
      { name: "model.onnx", content: randomBytes(1024 * 1024) },
      { name: "assets/tokens.txt", content: "a\nb\n" },
      { name: "LICENSE", content: "Apache-2.0" },
    ]);
    const destination = path.join(root, "stage");
    const result = await extractLocalModelZip(zipPath, destination, {
      signal: new AbortController().signal,
      expectedEntries: ["model.onnx", "assets/tokens.txt", "LICENSE"],
    });
    expect(result.files.map((entry) => entry.path)).toEqual(["model.onnx", "assets/tokens.txt", "LICENSE"]);
    expect(result.totalBytes).toBe(1024 * 1024 + 4 + 10);
    expect(fs.statSync(path.join(destination, "model.onnx")).size).toBe(1024 * 1024);
  });

  it("rejects undeclared files and removes the owned staging directory", async () => {
    const root = tempRoot();
    const zipPath = path.join(root, "extra.zip");
    await buildZip(zipPath, [
      { name: "model.onnx", content: "model" },
      { name: "unexpected.sh", content: "run" },
    ]);
    const destination = path.join(root, "stage");
    await expect(extractLocalModelZip(zipPath, destination, {
      signal: new AbortController().signal,
      expectedEntries: ["model.onnx"],
    })).rejects.toMatchObject({ code: "LOCAL_MODEL_ARCHIVE_UNSAFE" });
    expect(fs.existsSync(destination)).toBe(false);
  });

  it("rejects symlink entries without touching their target", async () => {
    const root = tempRoot();
    const canary = path.join(root, "canary.txt");
    fs.writeFileSync(canary, "original");
    const zipPath = path.join(root, "symlink.zip");
    await buildZip(zipPath, [{ name: "model.onnx", symlinkTarget: canary }]);
    await expect(extractLocalModelZip(zipPath, path.join(root, "stage"), {
      signal: new AbortController().signal,
      expectedEntries: ["model.onnx"],
    })).rejects.toMatchObject({ code: "LOCAL_MODEL_ARCHIVE_UNSAFE" });
    expect(fs.readFileSync(canary, "utf8")).toBe("original");
  });

  it("rejects compression bombs and oversized single entries before extraction", async () => {
    const root = tempRoot();
    const zipPath = path.join(root, "bomb.zip");
    await buildZip(zipPath, [{ name: "model.bin", content: Buffer.alloc(128 * 1024, 0) }]);
    await expect(extractLocalModelZip(zipPath, path.join(root, "stage-ratio"), {
      signal: new AbortController().signal,
      expectedEntries: ["model.bin"],
      limits: { maxCompressionRatio: 2 },
    })).rejects.toMatchObject({ code: "LOCAL_MODEL_ARCHIVE_UNSAFE" });
    await expect(extractLocalModelZip(zipPath, path.join(root, "stage-size"), {
      signal: new AbortController().signal,
      expectedEntries: ["model.bin"],
      limits: { maxFileBytes: 1024 },
    })).rejects.toMatchObject({ code: "LOCAL_MODEL_ARCHIVE_UNSAFE" });
  });

  it("rejects case-insensitive duplicate destinations", async () => {
    const root = tempRoot();
    const zipPath = path.join(root, "duplicate.zip");
    await buildZip(zipPath, [
      { name: "MODEL.bin", content: "one" },
      { name: "model.bin", content: "two" },
    ]);
    await expect(extractLocalModelZip(zipPath, path.join(root, "stage"), {
      signal: new AbortController().signal,
      expectedEntries: ["MODEL.bin", "model.bin"],
    })).rejects.toMatchObject({ code: "LOCAL_MODEL_ARCHIVE_UNSAFE" });
  });

  it("honors AbortSignal and removes partial output", async () => {
    const root = tempRoot();
    const zipPath = path.join(root, "abort.zip");
    await buildZip(zipPath, [
      { name: "first.bin", content: randomBytes(64 * 1024) },
      { name: "second.bin", content: randomBytes(64 * 1024) },
    ]);
    const controller = new AbortController();
    const destination = path.join(root, "stage");
    await expect(extractLocalModelZip(zipPath, destination, {
      signal: controller.signal,
      expectedEntries: ["first.bin", "second.bin"],
      onEntry: ({ path: entryPath }) => {
        if (entryPath === "first.bin") controller.abort();
      },
    })).rejects.toMatchObject({ code: "LOCAL_MODEL_ABORTED" });
    expect(fs.existsSync(destination)).toBe(false);
  });
});
