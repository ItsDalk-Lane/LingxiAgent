export type ChatPerformanceEventName =
  | 'stream_flush'
  | 'markdown_parse'
  | 'structural_message_update'
  | 'transcript_render'
  | 'transcript_projection'
  | 'turn_state_projection'
  | 'assistant_message_render'
  | 'history_projection';

export interface ChatPerformanceEvent {
  name: ChatPerformanceEventName;
  at: number;
  durationMs?: number;
  sessionPath?: string;
  messageId?: string;
  sourceLength?: number;
  itemCount?: number;
  blockCount?: number;
}

type ChatPerformanceListener = (event: ChatPerformanceEvent) => void;
type EventDetails = Omit<ChatPerformanceEvent, 'name' | 'at' | 'durationMs'>;

let activeListener: ChatPerformanceListener | null = null;

function now(): number {
  return globalThis.performance?.now?.() ?? Date.now();
}

/**
 * 性能观测默认关闭；测试或本地基准显式启用后才分配事件对象。
 * 返回值用于恢复上一个监听器，避免测试之间串数据。
 */
export function observeChatPerformance(listener: ChatPerformanceListener): () => void {
  const previous = activeListener;
  activeListener = listener;
  return () => {
    if (activeListener === listener) activeListener = previous;
  };
}

export function recordChatPerformance(
  name: ChatPerformanceEventName,
  details: EventDetails = {},
): void {
  const listener = activeListener;
  if (!listener) return;
  listener({ name, at: now(), ...details });
}

export function measureChatPerformance<T>(
  name: ChatPerformanceEventName,
  details: EventDetails,
  work: () => T,
): T {
  const listener = activeListener;
  if (!listener) return work();
  const startedAt = now();
  try {
    return work();
  } finally {
    listener({
      name,
      at: startedAt,
      durationMs: Math.max(0, now() - startedAt),
      ...details,
    });
  }
}
