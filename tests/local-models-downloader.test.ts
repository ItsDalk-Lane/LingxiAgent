import { createHash, randomBytes } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ResumableDownloader, type DownloadAsset } from "../lib/local-models/index.ts";

const roots: string[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

function tempRoot(): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-local-model-download-"));
  roots.push(root);
  return root;
}

function assetFor(payload: Buffer, id = "test-model@q4"): DownloadAsset {
  return {
    id,
    uri: `https://models.example.invalid/${id}.zip`,
    bytes: payload.byteLength,
    sha256: createHash("sha256").update(payload).digest("hex"),
  };
}

function rangeResponse(payload: Buffer, init: RequestInit): Response {
  const headers = new Headers(init.headers);
  const range = headers.get("range");
  if (!range) {
    return new Response(Uint8Array.from(payload), {
      status: 200,
      headers: { "content-length": String(payload.length) },
    });
  }
  const match = /^bytes=(\d+)-(\d+)$/.exec(range);
  if (!match) return new Response(null, { status: 416 });
  const start = Number(match[1]);
  const end = Number(match[2]);
  return new Response(Uint8Array.from(payload.subarray(start, end + 1)), {
    status: 206,
    headers: { "content-range": `bytes ${start}-${end}/${payload.length}` },
  });
}

function headResponse(payload: Buffer): Response {
  return new Response(null, {
    status: 200,
    headers: { "accept-ranges": "bytes", "content-length": String(payload.length) },
  });
}

describe("ResumableDownloader", () => {
  it("downloads concurrent ranges through the project dispatcher and reuses a verified completed artifact", async () => {
    const payload = randomBytes(300 * 1024);
    const asset = assetFor(payload);
    const getRanges: string[] = [];
    const dispatcher = { name: "proxy" };
    const fetchImpl = vi.fn(async (_url: string, init: RequestInit & { dispatcher?: unknown }) => {
      expect(init.dispatcher).toBe(dispatcher);
      if (init.method === "HEAD") return headResponse(payload);
      getRanges.push(new Headers(init.headers).get("range") ?? "");
      return rangeResponse(payload, init);
    });
    const downloader = new ResumableDownloader({
      rootDir: tempRoot(),
      concurrency: 4,
      minPartBytes: 64 * 1024,
      getFreeBytes: () => payload.length * 3,
      dispatcherForUrl: () => ({ dispatcher }),
      fetchImpl,
    });

    const first = await downloader.download(asset, { signal: new AbortController().signal });
    expect(fs.readFileSync(first.filePath)).toEqual(payload);
    expect(getRanges).toHaveLength(4);
    expect(new Set(getRanges).size).toBe(4);

    const second = await downloader.download(asset, { signal: new AbortController().signal });
    expect(second.filePath).toBe(first.filePath);
    expect(getRanges).toHaveLength(4);
  });

  it("keeps a shared task alive when only one waiter aborts", async () => {
    const payload = randomBytes(96 * 1024);
    const asset = assetFor(payload, "shared-model@q4");
    let releaseBody!: () => void;
    const bodyReady = new Promise<void>((resolve) => { releaseBody = resolve; });
    let getCalls = 0;
    const downloader = new ResumableDownloader({
      rootDir: tempRoot(),
      concurrency: 1,
      minPartBytes: 64 * 1024,
      getFreeBytes: () => payload.length * 3,
      dispatcherForUrl: () => ({ dispatcher: null }),
      fetchImpl: async (_url, init) => {
        if (init.method === "HEAD") return headResponse(payload);
        getCalls += 1;
        await bodyReady;
        return rangeResponse(payload, init);
      },
    });
    const firstController = new AbortController();
    const first = downloader.download(asset, { signal: firstController.signal });
    const second = downloader.download(asset, { signal: new AbortController().signal });
    firstController.abort();
    await expect(first).rejects.toMatchObject({ code: "LOCAL_MODEL_ABORTED" });
    releaseBody();
    const result = await second;
    expect(fs.readFileSync(result.filePath)).toEqual(payload);
    expect(getCalls).toBe(1);
  });

  it("persists a partial file on pause and resumes from the next byte", async () => {
    const payload = randomBytes(128 * 1024);
    const asset = assetFor(payload, "resume-model@q4");
    const ranges: string[] = [];
    let pauseOnce = true;
    let downloader!: ResumableDownloader;
    const fetchImpl = vi.fn(async (_url: string, init: RequestInit) => {
      if (init.method === "HEAD") return headResponse(payload);
      const range = new Headers(init.headers).get("range") ?? "";
      ranges.push(range);
      if (pauseOnce) {
        return new Response(new ReadableStream<Uint8Array>({
          start(controller) {
            controller.enqueue(payload.subarray(0, 32 * 1024));
          },
        }), { status: 206 });
      }
      return rangeResponse(payload, init);
    });
    downloader = new ResumableDownloader({
      rootDir: tempRoot(),
      concurrency: 1,
      minPartBytes: 64 * 1024,
      getFreeBytes: () => payload.length * 3,
      dispatcherForUrl: () => ({ dispatcher: null }),
      fetchImpl,
      onProgress: ({ taskId, downloadedBytes }) => {
        if (pauseOnce && downloadedBytes >= 32 * 1024) {
          pauseOnce = false;
          downloader.pause(taskId);
        }
      },
    });

    await expect(downloader.download(asset, { signal: new AbortController().signal }))
      .rejects.toMatchObject({ code: "LOCAL_MODEL_ABORTED" });
    expect((await downloader.listTasks())[0]).toMatchObject({ status: "paused" });

    const result = await downloader.download(asset, { signal: new AbortController().signal });
    expect(fs.readFileSync(result.filePath)).toEqual(payload);
    expect(ranges).toEqual([`bytes=0-${payload.length - 1}`, `bytes=${32 * 1024}-${payload.length - 1}`]);
  });

  it("retries one integrity mismatch and then succeeds", async () => {
    const payload = randomBytes(80 * 1024);
    const corrupt = Buffer.from(payload);
    corrupt[0] ^= 0xff;
    const asset = assetFor(payload, "retry-model@q4");
    let getCalls = 0;
    const downloader = new ResumableDownloader({
      rootDir: tempRoot(),
      concurrency: 1,
      minPartBytes: 64 * 1024,
      getFreeBytes: () => payload.length * 3,
      dispatcherForUrl: () => ({ dispatcher: null }),
      fetchImpl: async (_url, init) => {
        if (init.method === "HEAD") return headResponse(payload);
        getCalls += 1;
        return rangeResponse(getCalls === 1 ? corrupt : payload, init);
      },
    });

    const result = await downloader.download(asset, { signal: new AbortController().signal });
    expect(fs.readFileSync(result.filePath)).toEqual(payload);
    expect(getCalls).toBe(2);
  });

  it("fails closed after two integrity mismatches and removes partial data", async () => {
    const payload = randomBytes(80 * 1024);
    const corrupt = Buffer.from(payload);
    corrupt[0] ^= 0xff;
    const asset = assetFor(payload, "bad-model@q4");
    const root = tempRoot();
    const downloader = new ResumableDownloader({
      rootDir: root,
      concurrency: 1,
      minPartBytes: 64 * 1024,
      getFreeBytes: () => payload.length * 3,
      dispatcherForUrl: () => ({ dispatcher: null }),
      fetchImpl: async (_url, init) => init.method === "HEAD"
        ? headResponse(payload)
        : rangeResponse(corrupt, init),
    });

    await expect(downloader.download(asset, { signal: new AbortController().signal }))
      .rejects.toMatchObject({ code: "LOCAL_MODEL_DOWNLOAD_INTEGRITY" });
    const taskDir = path.join(root, (await downloader.listTasks())[0].taskId);
    expect(fs.readdirSync(taskDir).filter((name) => name.startsWith("part-"))).toEqual([]);
    expect(fs.existsSync(path.join(taskDir, "artifact.bin"))).toBe(false);
  });

  it("rejects non-HTTPS assets and insufficient disk space before network access", async () => {
    const payload = randomBytes(1024);
    const valid = assetFor(payload);
    const fetchImpl = vi.fn(async () => headResponse(payload));
    const downloader = new ResumableDownloader({
      rootDir: tempRoot(),
      getFreeBytes: () => payload.length,
      dispatcherForUrl: () => ({ dispatcher: null }),
      fetchImpl,
    });

    await expect(downloader.download({ ...valid, uri: "http://example.invalid/model.zip" }, {
      signal: new AbortController().signal,
    })).rejects.toMatchObject({ code: "LOCAL_MODEL_DOWNLOAD_NETWORK" });
    await expect(downloader.download(valid, { signal: new AbortController().signal }))
      .rejects.toMatchObject({ code: "LOCAL_MODEL_DISK_SPACE" });
    expect(fetchImpl).not.toHaveBeenCalled();
  });
});
