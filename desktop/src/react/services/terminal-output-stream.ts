import type {
  TerminalOutputMessage,
  TerminalTailMessage,
  TerminalTranscriptChunk,
} from '../../../../shared/terminal-ui-contract.ts';
import { TERMINAL_TAIL_HARD_MAX_CHUNKS } from '../../../../shared/terminal-ui-contract.ts';

export interface TerminalStreamRef {
  terminalId: string;
  sessionId?: string | null;
  sessionPath: string;
}

export interface TerminalChunkDelivery {
  chunks: TerminalTranscriptChunk[];
  reset: boolean;
}

export interface TerminalGap {
  terminalId: string;
  sessionId: string | null;
  sessionPath: string;
  lastSeq: number;
  nextSeq: number;
}

export interface TerminalOutputSubscriber {
  onChunks: (delivery: TerminalChunkDelivery) => void;
  onGap?: (gap: TerminalGap) => void;
}

interface SubscriberState {
  ref: Required<TerminalStreamRef>;
  callbacks: TerminalOutputSubscriber;
  initialized: boolean;
  lastSeq: number;
  pending: Map<number, TerminalTranscriptChunk>;
  gapReported: boolean;
}

function text(value: unknown): string {
  return typeof value === 'string' ? value.trim() : '';
}

function seq(value: unknown): number | null {
  if (value === undefined || value === null || value === '') return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? Math.max(0, Math.floor(parsed)) : null;
}

function normalizeRef(ref: TerminalStreamRef): Required<TerminalStreamRef> | null {
  const terminalId = text(ref?.terminalId);
  const sessionId = text(ref?.sessionId) || '';
  const sessionPath = text(ref?.sessionPath);
  if (!terminalId || !sessionPath) return null;
  return { terminalId, sessionId, sessionPath };
}

function keyOf(ref: TerminalStreamRef): string | null {
  const normalized = normalizeRef(ref);
  return normalized ? `${normalized.sessionId || normalized.sessionPath}\u0000${normalized.terminalId}` : null;
}

function normalizedChunks(value: unknown): TerminalTranscriptChunk[] {
  if (!Array.isArray(value)) return [];
  return value
    .map((chunk) => {
      const normalizedSeq = seq(chunk?.seq);
      if (normalizedSeq === null || typeof chunk?.data !== 'string') return null;
      return {
        seq: normalizedSeq,
        data: chunk.data,
        ...(chunk.truncatedStart === true ? { truncatedStart: true as const } : {}),
      };
    })
    .filter((chunk): chunk is TerminalTranscriptChunk => chunk !== null)
    .sort((a, b) => a.seq - b.seq);
}

export function createTerminalOutputStream() {
  const subscribers = new Map<string, Set<SubscriberState>>();

  function reportGap(state: SubscriberState) {
    const nextSeq = Math.min(...Array.from(state.pending.keys()).filter((value) => value > state.lastSeq));
    if (!Number.isFinite(nextSeq) || nextSeq === state.lastSeq + 1) {
      state.gapReported = false;
      return;
    }
    if (state.gapReported) return;
    state.gapReported = true;
    state.callbacks.onGap?.({
      ...state.ref,
      sessionId: state.ref.sessionId || null,
      lastSeq: state.lastSeq,
      nextSeq,
    });
  }

  function drain(state: SubscriberState, reset = false) {
    const chunks: TerminalTranscriptChunk[] = [];
    while (state.pending.has(state.lastSeq + 1)) {
      const next = state.pending.get(state.lastSeq + 1)!;
      state.pending.delete(next.seq);
      state.lastSeq = next.seq;
      chunks.push(next);
    }
    if (chunks.length) state.callbacks.onChunks({ chunks, reset });
    reportGap(state);
  }

  function addLiveChunks(state: SubscriberState, chunks: TerminalTranscriptChunk[]) {
    for (const chunk of chunks) {
      if (chunk.seq <= state.lastSeq || state.pending.has(chunk.seq)) continue;
      state.pending.set(chunk.seq, chunk);
    }
    if (!state.initialized) {
      // 首个 tail 还没回来时 live 块全部进 pending；tail 请求失败/丢失会让它无界增长。
      // 按服务端 tail 硬上限只保留最新的块——更旧的部分 tail 也补不回来，等价于截断补读。
      while (state.pending.size > TERMINAL_TAIL_HARD_MAX_CHUNKS) {
        state.pending.delete(Math.min(...state.pending.keys()));
      }
      return;
    }
    drain(state);
  }

  function handleChunks(message: TerminalOutputMessage) {
    const key = keyOf(message);
    if (!key) return;
    const targets = subscribers.get(key);
    if (!targets?.size) return;
    const chunks = normalizedChunks(message.chunks);
    if (!chunks.length) return;
    for (const state of targets) addLiveChunks(state, chunks);
  }

  function handleTail(message: TerminalTailMessage) {
    const key = keyOf(message);
    if (!key) return;
    const targets = subscribers.get(key);
    if (!targets?.size) return;
    const chunks = normalizedChunks(message.chunks);
    for (const state of targets) {
      // 实时块优先：它是完整原始块，可覆盖同序号的有界历史裁剪块。
      for (const chunk of chunks) {
        if (chunk.seq <= state.lastSeq || state.pending.has(chunk.seq)) continue;
        state.pending.set(chunk.seq, chunk);
      }

      if (!state.initialized) {
        const firstSeq = Math.min(...state.pending.keys());
        state.lastSeq = Number.isFinite(firstSeq)
          ? Math.max(0, firstSeq - 1)
          : (seq(message.lastSeq) ?? 0);
        state.initialized = true;
        state.gapReported = false;
        drain(state);
        continue;
      }

      const firstPendingSeq = Math.min(...Array.from(state.pending.keys()).filter((value) => value > state.lastSeq));
      if (message.truncated === true && Number.isFinite(firstPendingSeq) && firstPendingSeq > state.lastSeq + 1) {
        const resetChunks = Array.from(state.pending.values())
          .filter((chunk) => chunk.seq >= firstPendingSeq)
          .sort((a, b) => a.seq - b.seq);
        state.pending.clear();
        if (resetChunks.length) {
          state.lastSeq = resetChunks.at(-1)!.seq;
          state.callbacks.onChunks({ chunks: resetChunks, reset: true });
        }
        state.gapReported = false;
        continue;
      }

      state.gapReported = false;
      drain(state);
    }
  }

  function subscribe(ref: TerminalStreamRef, callbacks: TerminalOutputSubscriber): () => void {
    const normalized = normalizeRef(ref);
    const key = normalized ? keyOf(normalized) : null;
    if (!normalized || !key || typeof callbacks?.onChunks !== 'function') return () => {};
    const state: SubscriberState = {
      ref: normalized,
      callbacks,
      initialized: false,
      lastSeq: 0,
      pending: new Map(),
      gapReported: false,
    };
    const set = subscribers.get(key) || new Set<SubscriberState>();
    set.add(state);
    subscribers.set(key, set);
    return () => {
      const current = subscribers.get(key);
      current?.delete(state);
      if (!current?.size) subscribers.delete(key);
      state.pending.clear();
    };
  }

  return { subscribe, handleChunks, handleTail };
}

export const terminalOutputStream = createTerminalOutputStream();
