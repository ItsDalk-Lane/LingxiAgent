/**
 * TestModelCallObserver — 测试/调试用的事件收集器。
 *
 * 无界数组，仅供测试与临时调试接线；生产默认 observer 是 noop（§四十二：
 * 测试 collector 单独实现，不让它偷偷成为生产常驻内存日志）。
 */

import type {
  ModelCallEvent,
  ModelCallEventType,
  ModelCallObserver,
} from "./model-call-observer.ts";

export type TestModelCallObserver = ModelCallObserver & {
  events: ModelCallEvent[];
  /** 按事件类型过滤，保持投递顺序。 */
  eventsOfType(type: ModelCallEventType): ModelCallEvent[];
  /** 按 callId 过滤，保持投递顺序。 */
  eventsForCall(callId: string): ModelCallEvent[];
  /** 事件序列压缩成类型名数组，便于断言生命周期顺序。 */
  sequence(): ModelCallEventType[];
  /** 收集到的全部不同 callId（按首次出现顺序）。 */
  callIds(): string[];
  /** 收集到的全部不同 attemptId（按首次出现顺序，跳过 null/undefined）。 */
  attemptIds(): string[];
  /** 同一 call 的全部 attemptId（按首次出现顺序）。 */
  attemptsForCall(callId: string): string[];
  /**
   * 毒丸断言：全部事件的 JSON 序列化不得包含任何敏感标记（§八/§五十七）。
   * 违规时直接 fail（message 指明命中的毒丸）。
   */
  assertNoSensitiveContent(markers: string[]): void;
  /** 断言某 call 的生命周期序列（事件类型逐项相等）。 */
  assertLifecycle(callId: string, expected: ModelCallEventType[]): void;
  reset(): void;
};

export function createTestModelCallObserver(): TestModelCallObserver {
  const events: ModelCallEvent[] = [];
  const eventsForCall = (source: ModelCallEvent[], callId: string): ModelCallEvent[] =>
    source.filter((event) => event.callId === callId);
  return {
    events,
    handleModelCallEvent(event: ModelCallEvent) {
      events.push(event);
    },
    eventsOfType(type: ModelCallEventType) {
      return events.filter((event) => event.eventType === type);
    },
    eventsForCall(callId: string) {
      return eventsForCall(events, callId);
    },
    sequence() {
      return events.map((event) => event.eventType);
    },
    callIds() {
      return [...new Set(events.map((event) => event.callId))];
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
    reset() {
      events.length = 0;
    },
  };
}
