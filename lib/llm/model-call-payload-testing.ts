/**
 * model-call-payload-testing.ts — 测试专用 Payload Sink（§一百二十五/§一百二十六）。
 *
 * 无界数组，仅测试 fixture：可以保存已脱敏正文，生产模块不得默认安装；测试
 * 完成后由 test process 回收。提供按 call 的四层查询 + 毒丸扫描断言。
 * 查询到的都是 sanitized detached copy——本 sink 永远不应接触原始对象。
 */

import type { ModelCallPayloadRecord, ProviderRequestProvenance } from "./model-call-payload-types.ts";
import type { ModelCallPayloadSink } from "./model-call-payload-capture.ts";
import { setModelCallPayloadSink } from "./model-call-payload-capture.ts";
import type { ModelSemanticInputProvenance } from "./semantic-input-provenance.ts";

export type TestModelCallPayloadSink = ModelCallPayloadSink & {
  records: ModelCallPayloadRecord[];
  recordsForCall(callId: string): ModelCallPayloadRecord[];
  recordsOfKind(callId: string, kind: ModelCallPayloadRecord["kind"]): ModelCallPayloadRecord[];
  semanticRequestForCall(callId: string): ModelCallPayloadRecord | null;
  providerRequestsForCall(callId: string): ModelCallPayloadRecord[];
  providerResponsesForCall(callId: string): ModelCallPayloadRecord[];
  semanticResponseForCall(callId: string): ModelCallPayloadRecord | null;
  callIds(): string[];
  /** kind 序列（断言四层基数：1/N/N/0..1，§十八）。 */
  sequenceForCall(callId: string): ModelCallPayloadRecord["kind"][];
  provenanceForCall(callId: string): ModelSemanticInputProvenance | null;
  providerProvenanceForCall(callId: string): ProviderRequestProvenance | null;
  /**
   * 毒丸断言（§一百二十八）：全部 record 的 JSON 序列化不得包含任何敏感标记。
   * 失败信息只报毒丸名，不回显 payload（§一百八十）。
   */
  assertNoSensitiveContent(markers: string[]): void;
  reset(): void;
};

export function createTestModelCallPayloadSink(): TestModelCallPayloadSink {
  const records: ModelCallPayloadRecord[] = [];
  return {
    records,
    handleModelCallPayloadRecord(record) {
      records.push(record);
    },
    recordsForCall(callId) {
      return records.filter((record) => record.callId === callId);
    },
    recordsOfKind(callId, kind) {
      return records.filter((record) => record.callId === callId && record.kind === kind);
    },
    semanticRequestForCall(callId) {
      return records.find((record) => record.callId === callId && record.kind === "semantic_request") ?? null;
    },
    providerRequestsForCall(callId) {
      return records.filter((record) => record.callId === callId && record.kind === "provider_request");
    },
    providerResponsesForCall(callId) {
      return records.filter((record) => record.callId === callId && record.kind === "provider_response");
    },
    semanticResponseForCall(callId) {
      return records.find((record) => record.callId === callId && record.kind === "semantic_response") ?? null;
    },
    callIds() {
      return [...new Set(records.map((record) => record.callId))];
    },
    sequenceForCall(callId) {
      return this.recordsForCall(callId).map((record) => record.kind);
    },
    provenanceForCall(callId) {
      return this.semanticRequestForCall(callId)?.semanticInputProvenance ?? null;
    },
    providerProvenanceForCall(callId) {
      return this.providerRequestsForCall(callId)[0]?.providerRequestProvenance ?? null;
    },
    assertNoSensitiveContent(markers) {
      const serialized = JSON.stringify(records);
      const leaked = markers.filter((marker) => serialized.includes(marker));
      if (leaked.length > 0) {
        throw new Error(`payload records leaked sensitive content: ${leaked.join(", ")}`);
      }
    },
    reset() {
      records.length = 0;
    },
  };
}

/** 安装 test sink 并返回（测试 beforeEach 用；afterEach 传 null 还原 noop）。 */
export function installTestPayloadSink(): TestModelCallPayloadSink {
  const sink = createTestModelCallPayloadSink();
  setModelCallPayloadSink(sink);
  return sink;
}

export { createModelCallPayloadCaptureSession } from "./model-call-payload-capture.ts";
