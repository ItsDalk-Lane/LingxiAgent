import type {
  TerminalSnapshotRequestMessage,
  TerminalTailRequestMessage,
} from '../../../../shared/terminal-ui-contract.ts';

type WebSocketGetter = () => WebSocket | null;

let getWebSocket: WebSocketGetter = () => null;

function nonEmpty(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value.trim() : null;
}

function send(message: TerminalSnapshotRequestMessage | TerminalTailRequestMessage): boolean {
  const socket = getWebSocket();
  if (!socket || socket.readyState !== 1) return false;
  socket.send(JSON.stringify(message));
  return true;
}

export function configureTerminalClientWebSocketGetter(getter: WebSocketGetter): void {
  getWebSocket = typeof getter === 'function' ? getter : () => null;
}

export function requestTerminalSnapshot({ sessionId = null, sessionPath }: {
  sessionId?: string | null;
  sessionPath: string;
}): boolean {
  const path = nonEmpty(sessionPath);
  if (!path) return false;
  const id = nonEmpty(sessionId);
  return send({
    type: 'terminal_snapshot_request',
    sessionPath: path,
    ...(id ? { sessionId: id } : {}),
  });
}

export function requestTerminalTail({
  sessionId = null,
  sessionPath,
  terminalId,
  sinceSeq,
}: {
  sessionId?: string | null;
  sessionPath: string;
  terminalId: string;
  sinceSeq?: number | null;
}): boolean {
  const path = nonEmpty(sessionPath);
  const id = nonEmpty(terminalId);
  if (!path || !id) return false;
  const stableSessionId = nonEmpty(sessionId);
  const normalizedSince = sinceSeq !== undefined && sinceSeq !== null && Number.isFinite(Number(sinceSeq))
    ? Math.max(0, Math.floor(Number(sinceSeq)))
    : null;
  return send({
    type: 'terminal_tail_request',
    sessionPath: path,
    terminalId: id,
    ...(stableSessionId ? { sessionId: stableSessionId } : {}),
    ...(normalizedSince !== null ? { sinceSeq: normalizedSince } : {}),
  });
}
