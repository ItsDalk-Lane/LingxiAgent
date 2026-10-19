import { useStore } from '../stores';
import { resolveServerConnection } from './server-connection';
import type { LoopStatusSnapshot } from '../../../../contracts/generated/ts/lingxi-protocol';

type RuntimeError = NonNullable<LoopStatusSnapshot['runtimeError']>;

const shownErrors = new Map<string, string>();

export function readLoopRuntimeError(value: unknown): RuntimeError | undefined {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined;
}

/** 运行故障和持久状态分开呈现，同一连接和会话的同一故障只提醒一次。 */
export function warnLoopRuntimeError(sessionId: string, runtimeError: RuntimeError | undefined): void {
  const state = useStore.getState();
  const connection = resolveServerConnection(state);
  if (connection?.serverNodeKind !== 'lingxi-service') return;
  const key = `${JSON.stringify(connection)}\n${sessionId}`;
  if (!runtimeError) { shownErrors.delete(key); return; }
  const path = `native:${sessionId}`;
  if (!(state.currentSessionId === sessionId && state.currentSessionPath === path)
    && !state.sessions.some(session => session.sessionId === sessionId && session.path === path)) return;
  if (shownErrors.get(key) === runtimeError) return;
  shownErrors.set(key, runtimeError);
  if (shownErrors.size > 200) shownErrors.delete(shownErrors.keys().next().value!);
  state.addToast('循环任务暂时无法自动推进，任务记录状态已保留。请查看循环任务状态。', 'warning', 10_000, {
    dedupeKey: `loop-runtime-error:${sessionId}`,
  });
}
