/**
 * model-observability-payload-store.ts — Sanitized Payload 持久化（Phase 7）。
 *
 * 唯一入口是 Phase 6 的 ModelCallPayloadSink contract：本 store 永远只收到
 * **已经过统一 Redaction 的 sanitized detached copy**（§四/§十七）。这里不做
 * 第二次业务 redaction——只做 schema validation / serialization safety /
 * size verification（§十八）。
 *
 * fail closed（§十八）：record 无法 JSON serialize / kind 非法 / 超过 store
 * hard limit → drop record + 安全失败计数；绝不 fallback 保存 raw object、
 * 绝不 util.inspect(raw) 写盘。
 *
 * 排序（§五十/五十一）：id INTEGER PRIMARY KEY 自增 sequence 是稳定 tie-break，
 * captured_at 同毫秒不破坏顺序；读回按 id 升序。
 */

import {
  MODEL_CALL_PAYLOAD_KINDS,
  MODEL_CALL_PAYLOAD_CAPTURE_LIMITS,
  sanitizeStatusOf,
  type ModelCallPayloadRecord,
  type ModelCallPayloadSanitization,
} from "./model-call-payload-types.ts";
import type { ModelObservabilityTraceStore } from "./model-observability-trace-store.ts";

const KINDS = new Set<string>(MODEL_CALL_PAYLOAD_KINDS);
/** store 层 hard limit：单条 record 序列化后的字符上限（与 capture 层一致量级）。 */
const STORE_RECORD_MAX_CHARS = MODEL_CALL_PAYLOAD_CAPTURE_LIMITS.maxRecordChars;

export type StoredPayloadRecord = {
  id: number;
  call_id: string;
  kind: string;
  attempt_id: string | null;
  provider_request_ordinal: number | null;
  captured_at: string;
  visibility: string;
  fidelity: string;
  sanitization_status: string;
  redacted: number;
  truncated: number;
  degraded: number;
  payload_json: string | null;
  semantic_input_provenance_json: string | null;
  provider_request_provenance_json: string | null;
  record_char_count: number | null;
};

/** staged blob descriptor 的存储态归一结果（flush 期机械变换，无内容语义）。 */
export type BlobDescriptorNormalization = {
  /** payload 副本中出现的 staged blobId 集合（供 refs 登记使用）。 */
  stagedBlobIds: string[];
  /** 因 blob 写入失败需要降级为 store_failed 的 blobId 集合。 */
  failedBlobIds: Set<string>;
};

const BLOB_ID_MAX = 128;

function isStagedBlobDescriptor(value: unknown): value is Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const record = value as Record<string, unknown>;
  return record.kind === "external_blob" && record.captureStatus === "staged"
    && typeof record.blobId === "string" && record.blobId.length <= BLOB_ID_MAX;
}

/**
 * 在 sanitized payload 副本上做**存储态**归一（§七十一/七十二）：
 *   - blob 写入成功且 metadata row 已 durable → captureStatus: staged → stored
 *   - blob 写入失败（本批失败，或跨批仍无 metadata row）→ captureStatus:
 *     store_failed，并移除 blobId（绝不留下指向不存在文件的 dangling ref）
 * 这是存储可用性记账，不是第二次业务 redaction——不触碰正文与秘密规则。
 */
export function normalizeStagedBlobDescriptors(
  payload: unknown,
  failedBlobIds: Set<string>,
  stagedBlobIds: string[] = [],
  isBlobDurable: (blobId: string) => boolean = () => true,
): unknown {
  if (Array.isArray(payload)) {
    return payload.map((item) => normalizeStagedBlobDescriptors(item, failedBlobIds, stagedBlobIds, isBlobDurable));
  }
  if (payload && typeof payload === "object") {
    if (isStagedBlobDescriptor(payload)) {
      const blobId = payload.blobId as string;
      stagedBlobIds.push(blobId);
      if (failedBlobIds.has(blobId) || !isBlobDurable(blobId)) {
        const degraded: Record<string, unknown> = { ...payload };
        delete degraded.blobId;
        degraded.captureStatus = "store_failed";
        return degraded;
      }
      return { ...payload, captureStatus: "stored" };
    }
    const out: Record<string, unknown> = {};
    for (const [key, child] of Object.entries(payload as Record<string, unknown>)) {
      out[key] = normalizeStagedBlobDescriptors(child, failedBlobIds, stagedBlobIds, isBlobDurable);
    }
    return out;
  }
  return payload;
}

export function createModelObservabilityPayloadStore({ db, traceStore }: {
  db: any;
  traceStore: ModelObservabilityTraceStore;
}) {
  const insertPayloadRecord = db.prepare(`
    INSERT INTO payload_records (
      call_id, kind, attempt_id, provider_request_ordinal, captured_at,
      visibility, fidelity, sanitization_status, redacted, truncated, degraded,
      payload_json, semantic_input_provenance_json, provider_request_provenance_json,
      record_char_count
    ) VALUES (
      @call_id, @kind, @attempt_id, @provider_request_ordinal, @captured_at,
      @visibility, @fidelity, @sanitization_status, @redacted, @truncated, @degraded,
      @payload_json, @semantic_input_provenance_json, @provider_request_provenance_json,
      @record_char_count
    )
  `);
  const insertBlobRef = db.prepare(`
    INSERT OR IGNORE INTO payload_blob_refs (payload_record_id, blob_id, created_at)
    VALUES (?, ?, ?)
  `);
  const selectBlobIdsForRecord = db.prepare(
    `SELECT blob_id FROM payload_blob_refs WHERE payload_record_id = ?`,
  );

  function serializeField(value: unknown): { json: string | null; dropped: boolean } {
    if (value === null || value === undefined) return { json: null, dropped: false };
    try {
      const json = JSON.stringify(value);
      if (json.length > STORE_RECORD_MAX_CHARS) return { json: null, dropped: true };
      return { json, dropped: false };
    } catch {
      return { json: null, dropped: true };
    }
  }

  return {
    /**
     * 插入一条 sanitized payload record（须在 coordinator transaction 内调用）。
     * 返回插入的 payload_record id + staged blob refs（blob 已由 flush 先行写盘，
     * refs 在同一事务登记——§七十二 blob durable 先于 committed payload ref）。
     * fail closed：形状/序列化非法 → 返回 null（调用方计 droppedPayloadRecords）。
     */
    insertRecord(
      record: ModelCallPayloadRecord,
      options: { failedBlobIds?: Set<string>; isBlobDurable?: (blobId: string) => boolean; now?: () => string } = {},
    ): { id: number; blobIds: string[] } | null {
      if (!record || typeof record !== "object") return null;
      if (typeof record.callId !== "string" || !record.callId.trim()) return null;
      if (!KINDS.has(record.kind)) return null;
      if (typeof record.capturedAt !== "string" || !record.capturedAt) return null;

      // Payload 先到而 call row 尚未出现（§二十三）：partial shell，不虚构时间。
      traceStore.callShellFromIdentity({
        callId: record.callId,
        traceId: record.traceId ?? null,
        parentCallId: record.parentCallId ?? null,
        model: (record.model ?? null) as Record<string, unknown> | null,
        source: (record.source ?? null) as Record<string, unknown> | null,
        attribution: (record.attribution ?? null) as Record<string, unknown> | null,
      });

      const stagedBlobIds: string[] = [];
      const payloadValue = record.payload === null || record.payload === undefined
        ? null
        : normalizeStagedBlobDescriptors(
          record.payload,
          options.failedBlobIds ?? new Set(),
          stagedBlobIds,
          options.isBlobDurable,
        );
      const payloadField = serializeField(payloadValue);
      const provenanceField = serializeField(record.semanticInputProvenance ?? null);
      const providerProvenanceField = serializeField(record.providerRequestProvenance ?? null);
      const sanitization = record.sanitization ?? { redacted: false, truncated: false, degraded: false };

      const recordCharCount = payloadField.json?.length ?? 0;
      if (payloadField.dropped && record.payload != null) {
        // 超过 store hard limit：drop record（不是保存残缺 JSON）——§十八。
        return null;
      }

      const info = insertPayloadRecord.run({
        call_id: record.callId,
        kind: record.kind,
        attempt_id: typeof record.attemptId === "string" && record.attemptId ? record.attemptId : null,
        provider_request_ordinal: typeof record.providerRequestOrdinal === "number"
          && Number.isFinite(record.providerRequestOrdinal)
          ? record.providerRequestOrdinal
          : null,
        captured_at: record.capturedAt,
        visibility: record.visibility,
        fidelity: record.fidelity,
        sanitization_status: sanitizeStatusOf(sanitization as ModelCallPayloadSanitization),
        redacted: sanitization.redacted ? 1 : 0,
        truncated: sanitization.truncated ? 1 : 0,
        degraded: sanitization.degraded ? 1 : 0,
        payload_json: payloadField.json,
        semantic_input_provenance_json: provenanceField.json,
        provider_request_provenance_json: providerProvenanceField.json,
        record_char_count: recordCharCount,
      });
      const payloadRecordId = Number(info.lastInsertRowid);
      const now = options.now?.() ?? record.capturedAt;
      const blobIds: string[] = [];
      for (const blobId of stagedBlobIds) {
        insertBlobRef.run(payloadRecordId, blobId, now);
        blobIds.push(blobId);
      }
      return { id: payloadRecordId, blobIds };
    },

    getPayloadRecords(callId: string): StoredPayloadRecord[] {
      return db.prepare(`SELECT * FROM payload_records WHERE call_id = ? ORDER BY id`).all(callId);
    },

    getBlobIdsForRecord(payloadRecordId: number): string[] {
      return selectBlobIdsForRecord.all(payloadRecordId).map((row: any) => row.blob_id);
    },
  };
}

export type ModelObservabilityPayloadStore = ReturnType<typeof createModelObservabilityPayloadStore>;
