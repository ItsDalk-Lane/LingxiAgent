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
  reset(): void;
};

export function createTestModelCallObserver(): TestModelCallObserver {
  const events: ModelCallEvent[] = [];
  return {
    events,
    handleModelCallEvent(event: ModelCallEvent) {
      events.push(event);
    },
    eventsOfType(type: ModelCallEventType) {
      return events.filter((event) => event.eventType === type);
    },
    eventsForCall(callId: string) {
      return events.filter((event) => event.callId === callId);
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
    reset() {
      events.length = 0;
    },
  };
}
