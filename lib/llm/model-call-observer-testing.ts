/**
 * TestModelCallObserver — 测试/调试用的事件收集器。
 *
 * 无界数组，仅供测试与临时调试接线；生产默认 observer 是 noop（§四十二：
 * 测试 collector 单独实现，不让它偷偷成为生产常驻内存日志）。
 *
 * Phase 4 起附带 Trace Explorer 测试辅助（任务书 §八十二/§八十三）：
 * eventsForTrace / callsForTrace / childrenOf / rootsForTrace +
 * assertTraceGraphValid（图不变量机器校验）——仅供测试，不是生产 Query
 * Service。
 */

import type {
  ModelCallEvent,
  ModelCallEventType,
  ModelCallObserver,
} from "./model-call-observer.ts";
import {
  MODEL_CALL_SEMANTIC_PROVENANCE,
  type ModelSemanticInputProvenance,
} from "./semantic-input-provenance.ts";

export type TestModelCallObserver = ModelCallObserver & {
  events: ModelCallEvent[];
  /** 按事件类型过滤，保持投递顺序。 */
  eventsOfType(type: ModelCallEventType): ModelCallEvent[];
  /** 按 callId 过滤，保持投递顺序。 */
  eventsForCall(callId: string): ModelCallEvent[];
  /** 按 traceId 过滤，保持投递顺序。 */
  eventsForTrace(traceId: string): ModelCallEvent[];
  /** 收集到的全部不同 callId（按首次出现顺序）。 */
  callIds(): string[];
  /** 某 trace 内的全部 callId（按首次出现顺序）。 */
  callsForTrace(traceId: string): string[];
  /** parentCallId === callId 的下游 callId 列表。 */
  childrenOf(callId: string): string[];
  /** trace 内 parentCallId 为 null/undefined 的根 callId 列表。 */
  rootsForTrace(traceId: string): string[];
  /** callId → {traceId, parentCallId}（首个事件的身份快照）。 */
  callIdentity(callId: string): { traceId: string | null; parentCallId: string | null } | null;
  /** 事件序列压缩成类型名数组，便于断言生命周期顺序。 */
  sequence(): ModelCallEventType[];
  /** 收集到的全部不同 attemptId（按首次出现顺序）。 */
  attemptIds(): string[];
  /** 同一 call 的全部 attemptId（按首次出现顺序）。 */
  attemptsForCall(callId: string): string[];
  /**
   * Phase 5（§八十七）：callId → 事件 symbol 引用携带的完整 Semantic Input
   * Provenance。仅测试路径——生产不建内存 Trace Store。
   */
  provenanceForCall(callId: string): ModelSemanticInputProvenance | null;
  /** callId → 去重 category 列表（section 顺序）。 */
  categoriesForCall(callId: string): string[];
  /**
   * 毒丸断言：全部事件的 JSON 序列化不得包含任何敏感标记（§八/§五十七）。
   * 违规时直接 fail（message 指明命中的毒丸）。
   */
  assertNoSensitiveContent(markers: string[]): void;
  /** 断言某 call 的生命周期序列（事件类型逐项相等）。 */
  assertLifecycle(callId: string, expected: ModelCallEventType[]): void;
  /**
   * Trace 图不变量（§五十六～§六十）：
   *   - 每个 call 都有非空 traceId；
   *   - 同一 callId 的全部事件 traceId/parentCallId 稳定不变；
   *   - callId != parentCallId（无自环）；
   *   - parent 若在本 observer 可见集合内，必须与 child 同 trace（无跨 trace
   *     parent）；parent 不可见时放行（运行时子集是合法事实，不猜）；
   *   - parent 链无环。
   */
  assertTraceGraphValid(): void;
  reset(): void;
};

export function createTestModelCallObserver(): TestModelCallObserver {
  const events: ModelCallEvent[] = [];
  const provenanceByCall = new Map<string, ModelSemanticInputProvenance>();
  const eventsForCall = (source: ModelCallEvent[], callId: string): ModelCallEvent[] =>
    source.filter((event) => event.callId === callId);
  const eventsForTrace = (traceId: string): ModelCallEvent[] =>
    events.filter((event) => event.traceId === traceId);
  const callIdentityMap = (): Map<string, { traceId: string | null; parentCallId: string | null }> => {
    const map = new Map<string, { traceId: string | null; parentCallId: string | null }>();
    for (const event of events) {
      if (!map.has(event.callId)) {
        map.set(event.callId, { traceId: event.traceId ?? null, parentCallId: event.parentCallId ?? null });
      }
    }
    return map;
  };
  return {
    events,
    handleModelCallEvent(event) {
      events.push(event);
      const provenance = (event as any)[MODEL_CALL_SEMANTIC_PROVENANCE] as
        | ModelSemanticInputProvenance
        | undefined;
      if (provenance && !provenanceByCall.has(event.callId)) {
        provenanceByCall.set(event.callId, provenance);
      }
    },
    eventsOfType(type: ModelCallEventType) {
      return events.filter((event) => event.eventType === type);
    },
    eventsForCall(callId: string) {
      return eventsForCall(events, callId);
    },
    eventsForTrace(traceId: string) {
      return eventsForTrace(traceId);
    },
    sequence() {
      return events.map((event) => event.eventType);
    },
    callIds() {
      return [...new Set(events.map((event) => event.callId))];
    },
    callsForTrace(traceId: string) {
      return [...new Set(eventsForTrace(traceId).map((event) => event.callId))];
    },
    childrenOf(callId: string) {
      const identities = callIdentityMap();
      return [...new Set(events
        .map((event) => event.callId)
        .filter((child) => identities.get(child)?.parentCallId === callId))];
    },
    rootsForTrace(traceId: string) {
      const identities = callIdentityMap();
      return [...new Set(eventsForTrace(traceId)
        .map((event) => event.callId)
        .filter((callId) => {
          const parent = identities.get(callId)?.parentCallId;
          return parent === null || parent === undefined;
        }))];
    },
    callIdentity(callId: string) {
      return callIdentityMap().get(callId) ?? null;
    },
    attemptIds() {
      return [...new Set(
        events
          .map((event) => event.attemptId)
          .filter((id): id is string => typeof id === "string" && id.length > 0),
      )];
    },
    attemptsForCall(callId: string): string[] {
      const scoped = eventsForCall(events, callId);
      return [...new Set(
        scoped
          .map((event) => event.attemptId)
          .filter((id): id is string => typeof id === "string" && id.length > 0),
      )];
    },
    provenanceForCall(callId: string): ModelSemanticInputProvenance | null {
      return provenanceByCall.get(callId) ?? null;
    },
    categoriesForCall(callId: string): string[] {
      const provenance = provenanceByCall.get(callId);
      if (!provenance) return [];
      const categories: string[] = [];
      for (const section of provenance.sections) {
        if (!categories.includes(section.category)) categories.push(section.category);
      }
      return categories;
    },
    assertNoSensitiveContent(markers: string[]): void {
      const serialized = JSON.stringify(events);
      const leaked = markers.filter((marker) => serialized.includes(marker));
      if (leaked.length > 0) {
        throw new Error(
          `observer events leaked sensitive content: ${leaked.join(", ")}\n${serialized}`,
        );
      }
    },
    assertLifecycle(callId: string, expected: ModelCallEventType[]): void {
      const actual = eventsForCall(events, callId).map((event) => event.eventType);
      if (JSON.stringify(actual) !== JSON.stringify(expected)) {
        throw new Error(
          `lifecycle mismatch for ${callId}:\n  actual:   ${JSON.stringify(actual)}\n  expected: ${JSON.stringify(expected)}`,
        );
      }
    },
    assertTraceGraphValid(): void {
      const problems: string[] = [];
      const identities = callIdentityMap();
      for (const [callId, identity] of identities) {
        if (!identity.traceId) {
          problems.push(`call ${callId} has empty traceId`);
        }
        if (identity.parentCallId && identity.parentCallId === callId) {
          problems.push(`call ${callId} parents itself`);
        }
      }
      // 同 call 生命周期内 trace/parent 不得漂移（§五十六/§五十七）。
      for (const callId of identities.keys()) {
        const scoped = eventsForCall(events, callId);
        for (const event of scoped) {
          if ((event.traceId ?? null) !== identities.get(callId)!.traceId) {
            problems.push(`call ${callId} traceId drifted across events`);
            break;
          }
          if ((event.parentCallId ?? null) !== identities.get(callId)!.parentCallId) {
            problems.push(`call ${callId} parentCallId drifted across events`);
            break;
          }
        }
      }
      // parent 可见时必须同 trace（§六十）；不可见时放行（不猜）。
      for (const [callId, identity] of identities) {
        const parent = identity.parentCallId;
        if (!parent) continue;
        const parentIdentity = identities.get(parent);
        if (parentIdentity && parentIdentity.traceId !== identity.traceId) {
          problems.push(
            `call ${callId} (trace ${identity.traceId}) parents ${parent} (trace ${parentIdentity.traceId})`,
          );
        }
      }
      // parent 链无环。
      for (const callId of identities.keys()) {
        const seen = new Set<string>([callId]);
        let cursor = identities.get(callId)?.parentCallId ?? null;
        while (cursor && identities.has(cursor)) {
          if (seen.has(cursor)) {
            problems.push(`parent cycle detected at ${callId} -> ${cursor}`);
            break;
          }
          seen.add(cursor);
          cursor = identities.get(cursor)?.parentCallId ?? null;
        }
      }
      if (problems.length > 0) {
        throw new Error(`trace graph invariants violated:\n  - ${problems.join("\n  - ")}`);
      }
    },
    reset() {
      events.length = 0;
      provenanceByCall.clear();
    },
  };
}
