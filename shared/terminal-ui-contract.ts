/**
 * 桌面端终端输出查看与本机进程停止的共享传输契约。
 *
 * 这里保留终端领域的原生状态；“已完成/失败/已终止”等用户文案只在界面层推导。
 */

export const TERMINAL_TAIL_DEFAULT_MAX_BYTES = 64 * 1024;
export const TERMINAL_TAIL_DEFAULT_MAX_CHUNKS = 500;
export const TERMINAL_TAIL_HARD_MAX_BYTES = 256 * 1024;
export const TERMINAL_TAIL_HARD_MAX_CHUNKS = 2_000;

export const TERMINAL_CLIENT_MESSAGE_TYPES = new Set([
  'terminal_snapshot_request',
  'terminal_tail_request',
  'terminal_close_request',
  'subagent_stop_request',
]);

export const TERMINAL_SERVER_MESSAGE_TYPES = new Set([
  'terminal_snapshot',
  'terminal_tail',
  'terminal_state',
  'terminal_output',
  'terminal_close_result',
  'subagent_stop_result',
]);

export type TerminalNativeStatus = 'running' | 'exited' | 'killed' | 'stale';

export interface TerminalPublicEntry {
  terminalId: string;
  /** 关联对话里的 exec_command 工具卡；旧记录可能没有。 */
  toolCallId?: string | null;
  sessionId: string | null;
  sessionPath: string;
  agentId: string;
  cwd: string;
  command: string;
  label: string;
  status: TerminalNativeStatus;
  seq: number;
  createdAt: number;
  lastActivityAt: number;
  exitedAt: number | null;
  exitCode: number | null;
  signal: string | null;
  transcriptPath: string;
}

export interface TerminalTranscriptChunk {
  seq: number;
  data: string;
  /** 单个历史块超过读取字节上限时，只返回其尾部并明确标记。 */
  truncatedStart?: true;
}

export interface TerminalSnapshotRequestMessage {
  type: 'terminal_snapshot_request';
  sessionId?: string | null;
  sessionPath: string;
}

export interface TerminalSnapshotMessage {
  type: 'terminal_snapshot';
  sessionId: string | null;
  sessionPath: string;
  terminals: TerminalPublicEntry[];
}

export interface TerminalTailRequestMessage {
  type: 'terminal_tail_request';
  sessionId?: string | null;
  sessionPath: string;
  terminalId: string;
  sinceSeq?: number;
}

export interface TerminalCloseRequestMessage {
  type: 'terminal_close_request';
  requestId?: string;
  sessionId?: string | null;
  sessionPath: string;
  terminalId: string;
}

export interface SubagentStopRequestMessage {
  type: 'subagent_stop_request';
  requestId?: string;
  sessionId?: string | null;
  sessionPath: string;
  taskId: string;
}

export interface TerminalTailMessage {
  type: 'terminal_tail';
  sessionId: string | null;
  sessionPath: string;
  terminalId: string;
  terminal: TerminalPublicEntry;
  chunks: TerminalTranscriptChunk[];
  sinceSeq: number | null;
  lastSeq: number;
  truncated: boolean;
}

export interface TerminalStateMessage {
  type: 'terminal_state';
  sessionId: string | null;
  sessionPath: string;
  terminal: TerminalPublicEntry;
}

export interface TerminalOutputMessage {
  type: 'terminal_output';
  sessionId: string | null;
  sessionPath: string;
  terminalId: string;
  chunks: TerminalTranscriptChunk[];
}

export interface TerminalCloseResultMessage {
  type: 'terminal_close_result';
  requestId?: string;
  sessionId: string | null;
  sessionPath: string;
  terminalId: string;
  status: 'killed' | 'already_stopped' | 'rejected';
  reason?: string;
}

export interface SubagentStopResultMessage {
  type: 'subagent_stop_result';
  requestId?: string;
  sessionId: string | null;
  sessionPath: string;
  taskId: string;
  status: 'aborted' | 'already_stopped' | 'rejected';
  reason?: string;
}

export type TerminalClientMessage =
  | TerminalSnapshotRequestMessage
  | TerminalTailRequestMessage
  | TerminalCloseRequestMessage
  | SubagentStopRequestMessage;
export type TerminalServerMessage =
  | TerminalSnapshotMessage
  | TerminalTailMessage
  | TerminalStateMessage
  | TerminalOutputMessage
  | TerminalCloseResultMessage
  | SubagentStopResultMessage;
