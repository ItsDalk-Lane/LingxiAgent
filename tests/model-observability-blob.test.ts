/**
 * Phase 7 Blob Store 测试（任务书 §一百一十五/一百一十六）：
 * binary write / atomic publish（无 .tmp 残留）/ payload ref / read roundtrip /
 * ref-count GC / orphan cleanup（grace）/ missing blob 不 crash / size cap /
 * queue overflow / Blob 与 base64 保持 externalized（诚实 PARTIAL，§七十四）。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import fs from "fs";
import path from "path";
import { createModelObservabilityTestHarness } from "../lib/llm/model-observability-testing.ts";
import { createModelCallPayloadCaptureSession } from "../lib/llm/model-call-payload-capture.ts";
import { MODEL_OBSERVABILITY_BLOB_MAX_BYTES } from "../lib/llm/model-observability-blob-store.ts";

describe("Model Observability Blob Store", () => {
  let harness: ReturnType<typeof createModelObservabilityTestHarness>;

  beforeEach(() => {
    harness = createModelObservabilityTestHarness({ policy: { enabled: true, persistPayloads: true, persistBlobs: true } });
  });
  afterEach(async () => {
    await harness.close();
    harness.cleanup();
  });

  function captureWithBinary(callId: string, parameters: Record<string, unknown>): void {
    const session = createModelCallPayloadCaptureSession({ callId, traceId: `mt_${callId}` });
    expect(session).not.toBeNull();
    session!.captureSemanticRequest({ inputShape: "speech_transcribe", parameters });
  }

  it("Buffer 经 externalizer → 文件 + metadata + payload ref + descriptor=stored，字节可 roundtrip", async () => {
    const audio = Buffer.from("FAKE_MP3_BYTES_0123456789ABCDEF", "utf-8");
    captureWithBinary("mc_blob1", { audio, language: "zh" });
    harness.flush();
    await harness.close();

    const reader = harness.openReader();
    try {
      const rows = reader.payloadStore.getPayloadRecords("mc_blob1");
      expect(rows).toHaveLength(1);
      const body = JSON.parse(rows[0].payload_json!);
      expect(body.parameters.audio).toMatchObject({
        kind: "external_blob",
        captureStatus: "stored",
        byteLength: audio.byteLength,
      });
      expect(typeof body.parameters.audio.blobId).toBe("string");
      const blobId = body.parameters.audio.blobId as string;
      const meta = reader.blobStore.getBlobMetadata(blobId);
      expect(meta).toMatchObject({ state: "ready", byte_length: audio.byteLength, media_type: "application/octet-stream" });
      // 文件名 = blobId.bin（绝不使用原文件名，§六十七）；相对路径在 blobs/ 下。
      expect(meta!.relative_path).toMatch(/^blobs\/[^/]+\/mb_[^/]+\.bin$/);
      const bytes = reader.blobStore.readBlob(blobId);
      expect(bytes?.toString("utf-8")).toBe("FAKE_MP3_BYTES_0123456789ABCDEF");
      // payload ↔ blob ref 已登记。
      expect(reader.payloadStore.getBlobIdsForRecord(rows[0].id)).toEqual([blobId]);
      // 磁盘上无 staging 残留（atomic publish，§七十）。
      const leftovers: string[] = [];
      const walk = (dir: string) => {
        for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
          const abs = path.join(dir, entry.name);
          if (entry.isDirectory()) walk(abs);
          else if (entry.name.includes(".tmp-")) leftovers.push(abs);
        }
      };
      walk(harness.blobsRoot);
      expect(leftovers).toEqual([]);
    } finally {
      reader.close();
    }
  });

  it("Blob 实例与 base64 字符串保持 externalized（无法同步取字节，诚实 PARTIAL）", () => {
    captureWithBinary("mc_blob_partial", {
      blob: new Blob(["opaque-bytes"], { type: "application/octet-stream" }),
      b64: Buffer.from("x".repeat(2048)).toString("base64"),
    });
    harness.flush();
    const reader = harness.openReader();
    try {
      const rows = reader.payloadStore.getPayloadRecords("mc_blob_partial");
      const body = JSON.parse(rows[0].payload_json!);
      expect(body.parameters.blob.captureStatus).toBe("externalized");
      expect(body.parameters.blob.blobId).toBeUndefined();
      expect(body.parameters.b64.captureStatus).toBe("externalized");
      expect(body.parameters.b64.blobId).toBeUndefined();
    } finally {
      reader.close();
    }
  });

  it("ref-count GC：删除 payload 后 refless blob 被清（不看 mtime，§九十一）", async () => {
    captureWithBinary("mc_gc", { audio: Buffer.from("GC_BYTES") });
    harness.flush();
    await harness.close();
    const reader = harness.openReader();
    try {
      const rows = reader.payloadStore.getPayloadRecords("mc_gc");
      const blobId = reader.payloadStore.getBlobIdsForRecord(rows[0].id)[0];
      expect(reader.blobStore.getBlobMetadata(blobId)).not.toBeNull();
      // 删除 payload record + refs（模拟 payload retention）。
      reader.db.prepare(`DELETE FROM payload_blob_refs WHERE payload_record_id = ?`).run(rows[0].id);
      reader.db.prepare(`DELETE FROM payload_records WHERE id = ?`).run(rows[0].id);
      const removed = reader.blobStore.collectGarbageBlobs();
      expect(removed).toEqual([blobId]);
      expect(reader.blobStore.getBlobMetadata(blobId)).toBeNull();
      // 文件已 unlink。
      const abs = path.join(harness.lingxiHome, "model-observability", "blobs", blobId.slice(0, 2), `${blobId}.bin`);
      expect(fs.existsSync(abs)).toBe(false);
    } finally {
      reader.close();
    }
  });

  it("orphan recovery：磁盘有、DB 无 row、超过 grace 的文件被清理；新文件不删（§九十二）", async () => {
    await harness.close();
    const orphanOld = path.join(harness.blobsRoot, "ab", "mb_abold111111111111.bin");
    const orphanFresh = path.join(harness.blobsRoot, "cd", "mb_cdfresh1111111111.bin");
    fs.mkdirSync(path.dirname(orphanOld), { recursive: true });
    fs.mkdirSync(path.dirname(orphanFresh), { recursive: true });
    fs.writeFileSync(orphanOld, "old-orphan");
    fs.writeFileSync(orphanFresh, "fresh-orphan");
    const oldTime = new Date(Date.now() - 48 * 3600 * 1000);
    fs.utimesSync(orphanOld, oldTime, oldTime);

    const reader = harness.openReader();
    try {
      const removed = reader.blobStore.recoverOrphanBlobFiles({ graceMs: 24 * 3600 * 1000 });
      expect(removed).toBe(1);
      expect(fs.existsSync(orphanOld)).toBe(false);
      expect(fs.existsSync(orphanFresh)).toBe(true);
    } finally {
      reader.close();
    }
  });

  it("missing blob：row 存在但文件被外部删除 → state=missing，readBlob 返回 null 不 crash（§九十三）", async () => {
    captureWithBinary("mc_missing", { audio: Buffer.from("WILL_BE_DELETED") });
    harness.flush();
    await harness.close();
    const reader = harness.openReader();
    try {
      const rows = reader.payloadStore.getPayloadRecords("mc_missing");
      const blobId = reader.payloadStore.getBlobIdsForRecord(rows[0].id)[0];
      const abs = path.join(harness.lingxiHome, "model-observability", "blobs", blobId.slice(0, 2), `${blobId}.bin`);
      fs.rmSync(abs);
      expect(reader.blobStore.readBlob(blobId)).toBeNull();
      expect(reader.blobStore.getBlobMetadata(blobId)).toMatchObject({ state: "missing" });
    } finally {
      reader.close();
    }
  });

  it("size cap：超过 MODEL_OBSERVABILITY_BLOB_MAX_BYTES 的 binary 不保存（descriptor 退回 externalized，§七十三）", () => {
    const oversized = new Uint8Array(MODEL_OBSERVABILITY_BLOB_MAX_BYTES + 1);
    captureWithBinary("mc_toobig", { audio: oversized });
    harness.flush();
    const health = harness.handle.getHealth();
    expect(health.droppedBlobs).toBe(1);
    const reader = harness.openReader();
    try {
      const rows = reader.payloadStore.getPayloadRecords("mc_toobig");
      const body = JSON.parse(rows[0].payload_json!);
      expect(body.parameters.audio.captureStatus).toBe("externalized");
      expect(body.parameters.audio.blobId).toBeUndefined();
    } finally {
      reader.close();
    }
  });

  it("queue overflow：超过 maxQueuedBlobs 后 stageBinary 返回 null（降级 externalized）+ droppedBlobs 计数（§七十三/一百一十五）", async () => {
    await harness.close();
    const overflow = await (async () => {
      const { installModelObservabilityPersistence } = await import("../lib/llm/model-observability-persistence.ts");
      return installModelObservabilityPersistence({
        lingxiHome: harness.lingxiHome,
        policy: {
          enabled: true,
          persistPayloads: true,
          persistBlobs: true,
          limits: { maxQueuedBlobs: 2 },
        },
      });
    })();
    try {
      const session = createModelCallPayloadCaptureSession({ callId: "mc_overflow", traceId: "mt_of" });
      session!.captureSemanticRequest({
        inputShape: "speech_transcribe",
        parameters: { a: Buffer.from("AAAA"), b: Buffer.from("BBBB"), c: Buffer.from("CCCC") },
      });
      const health = overflow.getHealth();
      expect(health.droppedBlobs).toBe(1);
      overflow.flushSync();
      const reader = harness.openReader();
      try {
        const rows = reader.payloadStore.getPayloadRecords("mc_overflow");
        const body = JSON.parse(rows[0].payload_json!);
        expect(body.parameters.a.captureStatus).toBe("stored");
        expect(body.parameters.b.captureStatus).toBe("stored");
        expect(body.parameters.c.captureStatus).toBe("externalized");
        expect(body.parameters.c.blobId).toBeUndefined();
      } finally {
        reader.close();
      }
    } finally {
      await overflow.close();
    }
  });
});
