import type {
  SubagentStopRequestMessage,
  SubagentStopResultMessage,
  TerminalCloseRequestMessage,
  TerminalCloseResultMessage,
} from '../../../../shared/terminal-ui-contract.ts';

type ProcessControlResult = TerminalCloseResultMessage | SubagentStopResultMessage;
type WebSocketGetter = () => WebSocket | null;

const REQUEST_TIMEOUT_MS = 10_000;
let getWebSocket: WebSocketGetter = () => null;
let requestSequence = 0;
const pending = new Map<string, {
  resolve: (result: ProcessControlResult) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}>();

export function configureBackgroundProcessWebSocketGetter(getter: WebSocketGetter): void {
  getWebSocket = typeof getter === 'function' ? getter : () => null;
}

function send(message: TerminalCloseRequestMessage | SubagentStopRequestMessage): Promise<ProcessControlResult> {
  const socket = getWebSocket();
  if (!socket || socket.readyState !== 1) {
    return Promise.reject(new Error('process_control_disconnected'));
  }
  const requestId = `process-control-${Date.now()}-${++requestSequence}`;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(requestId);
      reject(new Error('process_control_timeout'));
    }, REQUEST_TIMEOUT_MS);
    pending.set(requestId, { resolve, reject, timer });
    try {
      socket.send(JSON.stringify({ ...message, requestId }));
    } catch (error) {
      clearTimeout(timer);
      pending.delete(requestId);
      reject(error instanceof Error ? error : new Error(String(error)));
    }
  });
}

export function stopTerminalProcess(input: Omit<TerminalCloseRequestMessage, 'type'>): Promise<ProcessControlResult> {
  return send({ type: 'terminal_close_request', ...input });
}

export function stopSubagentProcess(input: Omit<SubagentStopRequestMessage, 'type'>): Promise<ProcessControlResult> {
  return send({ type: 'subagent_stop_request', ...input });
}

export function handleBackgroundProcessControlResult(result: ProcessControlResult): void {
  if (!result.requestId) return;
  const waiter = pending.get(result.requestId);
  if (!waiter) return;
  clearTimeout(waiter.timer);
  pending.delete(result.requestId);
  if (result.status === 'rejected') {
    waiter.reject(new Error(result.reason || 'process_control_rejected'));
    return;
  }
  waiter.resolve(result);
}
