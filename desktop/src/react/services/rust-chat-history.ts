/** Rust 历史与实时消息共用回合投影；未知来源不推测成相邻 Run。 */
import type { ChatListItem, ChatMessage, ContentBlock, UserAttachment } from '../stores/chat-types';
import type { LiveAssistantSegment } from '../stores/live-turn-store';
import { renderMarkdown } from '../utils/markdown';
import { projectAssistantTurn } from '../utils/turn-projector';
import { appendConnectionAuth, buildConnectionUrl, requireServerConnection, resolveServerConnection, type ServerConnection } from './server-connection';
import { useStore } from '../stores';
import { conversationSuggestionFromDetails } from './conversation-suggestion';
import { rustMediaBlocksFromDetails } from './rust-media';
import { captureSessionConnection, ownsSessionConnection } from './session-connection-owner';

type Row = Record<string, unknown>;
const record = (value: unknown, label: string): Row => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error(`历史格式错误：${label}`);
  return value as Row;
};
const string = (value: unknown, label: string): string => {
  if (typeof value !== 'string') throw new Error(`历史格式错误：${label}`);
  return value;
};
const bool = (value: unknown, label: string): boolean => {
  if (typeof value !== 'boolean') throw new Error(`历史格式错误：${label}`);
  return value;
};
function runId(row: Row): string | null {
  const run = record(row.run, 'run');
  if (run.state === 'unknown') return null;
  if (run.state !== 'known') throw new Error('历史 Run 身份无效');
  return string(run.runId, 'runId');
}
function preserved(id: string, text: string): Extract<ContentBlock, { type: 'interlude' }> {
  return { type: 'interlude', id, variant: 'native_preserved_record', sourceLabel: '历史记录', text, surfaceRole: 'control', lifecycle: 'sealed' };
}
function attachments(row: Row): UserAttachment[] | undefined {
  const legacy = row.legacyFields == null ? {} : record(row.legacyFields, 'legacyFields');
  const files: UserAttachment[] = legacy.attachments == null ? [] : (() => {
    if (!Array.isArray(legacy.attachments)) throw new Error('历史附件格式无效');
    return legacy.attachments.map(value => {
      const file = record(value, 'attachment');
      return {fileId:string(file.fileId, 'fileId'),path:string(file.path, 'path'),name:string(file.name, 'name'),isDir:bool(file.isDir, 'isDir'),...(typeof file.mimeType === 'string' ? {mimeType:file.mimeType} : {}),...(typeof file.status === 'string' ? {status:file.status} : {})};
    });
  })();
  if (legacy.images == null) return files.length ? files : undefined;
  if (!Array.isArray(legacy.images) || legacy.images.length > 10) throw new Error('历史图片格式无效');
  return [...files, ...legacy.images.map((value, index) => {
    const image = record(value, 'image');
    const mime = string(image.mime, 'image.mime');
    if (typeof image.data === 'string') {
      if (!['image/png', 'image/jpeg', 'image/webp', 'image/gif'].includes(mime) || image.data.length > 20 * 1024 * 1024 || image.data.length === 0) throw new Error('历史图片内容无效');
      try { atob(image.data); } catch { throw new Error('历史图片编码无效'); }
      return { path: `inline:${string(row.messageId, 'messageId')}:${index}`, name: `图片 ${index + 1}.${mime === 'image/jpeg' ? 'jpg' : mime.slice('image/'.length)}`, isDir: false, mimeType: mime, base64Data: image.data };
    }
    const bytes = image.bytes;
    if (!['image/png', 'image/jpeg', 'image/webp', 'image/gif'].includes(mime) || !Array.isArray(bytes)
      || bytes.length > 20 * 1024 * 1024 || bytes.some((v) => !Number.isInteger(v) || v < 0 || v > 255)) throw new Error('历史图片内容无效');
    let binary = '';
    for (let offset = 0; offset < bytes.length; offset += 8192) binary += String.fromCharCode(...bytes.slice(offset, offset + 8192));
    return { path: `inline:${string(row.messageId, 'messageId')}:${index}`, name: `图片 ${index + 1}.${mime === 'image/jpeg' ? 'jpg' : mime.slice('image/'.length)}`, isDir: false, mimeType: mime, base64Data: btoa(binary) };
  })];
}
interface Turn {
  key: string;
  input?: string;
  ids: string[];
  segments: LiveAssistantSegment[];
  blocks: ContentBlock[];
  finals: Row[];
  terminal?: 'completed' | 'failed' | 'aborted';
  timestamp?: number;
}
export function projectRustHistoryItems(items: unknown[], sessionId: string): ChatListItem[] {
  if (!Array.isArray(items) || !sessionId) throw new Error('历史输入无效');
  const output: Array<ChatListItem | Turn> = [];
  const turns = new Map<string, Turn>();
  const inputs = new Map<string, string>();
  const turnFor = (key: string): Turn => {
    let turn = turns.get(key);
    if (!turn) { turn = { key, input: inputs.get(key), ids: [], segments: [], blocks: [], finals: [] }; turns.set(key, turn); output.push(turn); }
    return turn;
  };
  items.forEach((value, order) => {
    const row = record(value, 'item');
    const kind = string(row.kind, 'kind');
    switch (kind) {
      case 'user_message': {
        const id = string(row.messageId, 'messageId');
        const run = runId(row);
        if (run) { inputs.set(run, id); const previous = turns.get(run); if (previous) previous.input = id; }
        const legacy = row.legacyFields == null ? {} : record(row.legacyFields, 'legacyFields');
        const quotedText = legacy.quotedText == null ? undefined : string(legacy.quotedText, 'quotedText');
        const sessionRefs = legacy.sessionRefs == null ? undefined : (() => {
          if (!Array.isArray(legacy.sessionRefs)) throw new Error('历史会话引用格式无效');
          return legacy.sessionRefs.map(value => { const reference = record(value, 'sessionRef'); return {sessionId: string(reference.sessionId, 'sessionRef.sessionId'), label: reference.label == null ? 'Session' : string(reference.label, 'sessionRef.label')}; });
        })();
        const timestamp = row.committedAtUnixMs;
        if (typeof timestamp !== 'number' || !Number.isFinite(timestamp)) throw new Error('历史时间无效');
        output.push({ type: 'message', data: { id, sourceEntryId: id, role: 'user', text: string(row.text, 'text'), textHtml: row.text ? renderMarkdown(string(row.text, 'text')) : undefined, timestamp, quotedText, sessionRefs, attachments: attachments(row), ...(typeof (row.legacyFields as Row | undefined)?.clientMessageId === 'string' ? { clientMessageId: (row.legacyFields as Row).clientMessageId as string } : {}) } });
        break;
      }
      case 'assistant_segment': {
        const id = string(row.segmentId, 'segmentId');
        const phase = string(row.phase, 'phase');
        const segmentKind = string(row.segmentKind, 'segmentKind');
        if (!['reasoning', 'commentary', 'final_answer', 'unresolved'].includes(phase) || !['text', 'reasoning'].includes(segmentKind)) throw new Error('历史助手段类型无效');
        const turn = turnFor(runId(row) || `unknown-segment:${id}`);
        turn.segments.push({ id, kind: segmentKind as LiveAssistantSegment['kind'], semanticPhase: phase as LiveAssistantSegment['semanticPhase'], source: string(row.text, 'text'), lifecycle: bool(row.complete, 'complete') ? 'sealed' : 'streaming', processOrder: order });
        break;
      }
      case 'tool_call': {
        const id = string(row.toolCallId, 'toolCallId');
        const status = string(row.status, 'status');
        if (!['started', 'success', 'failed', 'cancelled', 'unknown'].includes(status)) throw new Error('历史工具状态无效');
        const result = row.result == null ? undefined : record(row.result, 'result');
        const error = result?.error == null ? undefined : record(result.error, 'error');
        turnFor(runId(row) || `unknown-tool:${id}`).blocks.push({ type: 'tool_group', id: `native-tool:${id}`, collapsed: true, processOrder: order, tools: [{ id, name: string(row.target, 'target'), done: status !== 'started', success: status === 'success', status: status === 'started' ? 'running' : status === 'success' ? 'succeeded' : status === 'unknown' ? 'unknown' : 'failed', ...(row.argsSummary == null ? {} : { args: { summary: string(row.argsSummary, 'argsSummary') } }), ...(error ? { error: string(error.message, 'error.message') } : {}), ...(result ? { details: result } : {}) }] });
        if (status === 'success') turnFor(runId(row) || `unknown-tool:${id}`).blocks.push(...rustMediaBlocksFromDetails(result?.details, sessionId));
        const suggestion = status === 'success' ? conversationSuggestionFromDetails(result?.details) : null;
        if (suggestion) turnFor(runId(row) || `unknown-tool:${id}`).blocks.push({ ...suggestion, id: `native-suggestion:${id}`, processOrder: order });
        break;
      }
      case 'final_message': {
        const id = string(row.messageId, 'messageId');
        const turn = turnFor(runId(row) || `unknown-final:${id}`);
        turn.ids.push(id); turn.finals.push(row);
        if (typeof row.committedAtUnixMs !== 'number' || !Number.isFinite(row.committedAtUnixMs)) throw new Error('历史时间无效');
        turn.timestamp = row.committedAtUnixMs; turn.terminal = 'completed';
        break;
      }
      case 'run_terminal': {
        const turn = turnFor(string(row.runId, 'runId'));
        if (!['completed', 'failed', 'cancelled'].includes(String(row.status))) throw new Error('历史终态无效');
        turn.terminal = row.status === 'cancelled' ? 'aborted' : row.status as Turn['terminal'];
        break;
      }
      case 'reset_marker': {
        const id = string(row.messageId, 'messageId');
        output.push({ type: 'interlude', id, data: preserved(id, `历史变更：${string(row.reason, 'reason')}`) });
        break;
      }
      case 'legacy_raw': {
        const id = string(row.messageId, 'messageId');
        if (!Object.hasOwn(row, 'raw')) throw new Error('旧历史原文缺失');
        output.push({ type: 'interlude', id, data: preserved(id, `旧格式记录已完整保留，当前界面无法展开：${string(row.reason, 'reason')}`) });
        break;
      }
      default: throw new Error(`历史项尚不支持：${kind}`);
    }
  });
  return output.map((entry): ChatListItem => {
    if ('type' in entry) return entry;
    const turn = entry;
    const segments = [...turn.segments];
    for (const final of turn.finals) {
      if (!Array.isArray(final.content)) throw new Error('历史最终回复格式无效');
      // 最终正文替换同一 Run 的流式最终段；过程段保持原身份及顺序。
      const finalText = final.content.map((b) => record(b, 'content')).filter((b) => b.type === 'text').map((b) => string(b.text, 'content.text')).join('');
      if (finalText) {
        for (let i = segments.length - 1; i >= 0; i--) if (segments[i].semanticPhase === 'final_answer') segments.splice(i, 1);
        segments.push({ id: `final:${String(final.messageId)}`, kind: 'text', semanticPhase: 'final_answer', source: finalText, lifecycle: 'sealed' });
      }
      final.content.forEach((value, index) => {
        const block = record(value, 'content');
        if (block.type === 'text') return;
        if (block.type === 'reasoning') {
          const text = string(block.text, 'reasoning.text');
          if (!segments.some((s) => s.kind === 'reasoning' && s.source === text)) segments.push({ id: `final-reasoning:${String(final.messageId)}:${index}`, kind: 'reasoning', semanticPhase: 'reasoning', source: text, lifecycle: 'sealed' });
        } else if (block.type === 'opaque') {
          string(block.provider, 'opaque.provider');
          if (!Object.hasOwn(block, 'data')) throw new Error('供应商原始状态缺失');
          turn.blocks.push(preserved(`opaque:${String(final.messageId)}:${index}`, '供应商专用状态已原样保留，当前界面不展开。'));
        } else if (block.type === 'resource_ref') {
          const resource = record(block.resource, 'resource');
          const id = string(resource.resourceId, 'resourceId');
          turn.blocks.push(preserved(`resource:${id}`, `回复包含资源：${typeof resource.displayName === 'string' ? resource.displayName : id}。请在会话文件面板打开。`));
        } else throw new Error(`历史内容尚不支持：${String(block.type)}`);
      });
    }
    const id = `native-run:${sessionId}:${turn.key}`;
    const projected = projectAssistantTurn({ idPrefix: id, inputMessageId: turn.input, assistantMessageIds: turn.ids, segments, legacyBlocks: turn.blocks, status: turn.terminal || 'streaming', runTerminal: turn.terminal != null });
    const data: ChatMessage = { id, role: 'assistant', ...(turn.ids[0] ? { sourceEntryId: turn.ids[0] } : {}), ...(turn.input ? { turnInputEntryId: turn.input, turnInputVisible: true } : {}), blocks: projected.blocks, turnProjection: projected.projection, timestamp: turn.timestamp };
    return { type: 'message', data };
  });
}
export type RustHistoryResult = { notModified: true; etag: string | null } | {
  notModified: false; items: ChatListItem[]; revision: string; etag: string | null;
  page: { hasMore: boolean; nextCursor: string | null }; rawItems: unknown[];
  reconciliation?: { sessionId: string; sessionPath: string; snapshotId: string; runRevision: string; complete: boolean; runStatus: 'running' | 'reconciled_idle' | 'unknown' };
};
export async function readRustChatHistory(connection: ServerConnection, sessionId: string, options: { cursor?: string | null; limit?: number; etag?: string | null; signal?: AbortSignal; all?: boolean; reconciliation?: boolean } = {}): Promise<RustHistoryResult> {
  if (connection.serverNodeKind !== 'lingxi-service' || !sessionId) throw new Error('Rust 历史连接或会话无效');
  if (options.limit != null && (!Number.isInteger(options.limit) || options.limit < 1 || options.limit > 200)) throw new Error('历史分页大小无效');
  if (options.all && options.cursor) throw new Error('全量历史不能同时指定分页游标');
  const authority = JSON.stringify(connection);
  const owner = captureSessionConnection();
  const current = () => { if (!ownsSessionConnection(owner) || JSON.stringify(resolveServerConnection(useStore.getState())) !== authority) throw new Error('连接已切换，旧历史结果已丢弃'); };
  current();
  const query = new URLSearchParams();
  if (options.cursor) query.set('before', options.cursor);
  if (options.limit != null) query.set('limit', String(options.limit));
  if (options.all) query.set('all', '1');
  if (options.reconciliation) query.set('reconciliation', '1');
  const headers = appendConnectionAuth(connection, options.etag && !options.cursor && !options.all && !options.reconciliation ? { 'If-None-Match': options.etag } : {});
  const response = await fetch(buildConnectionUrl(connection, `/lingxi/v1/sessions/${encodeURIComponent(sessionId)}/history${query.size ? `?${query}` : ''}`), { headers, signal: options.signal });
  current();
  const etag = response.headers.get('etag');
  if (response.status === 304) {
    if (!options.etag || options.cursor || options.all || options.reconciliation) throw new Error('历史服务返回了无条件的 304');
    return { notModified: true, etag };
  }
  if (!response.ok) throw new Error(`读取会话历史失败：${response.status}`);
  const body = record(await boundedJson(response), 'response'); current();
  if (body.schemaVersion !== 'lingxi.canonical-history.v1' || body.sessionId !== sessionId || !Array.isArray(body.items)) throw new Error('历史响应不属于当前会话或版本');
  const page = record(body.page, 'page');
  const hasMore = bool(page.hasMore, 'hasMore');
  const nextCursor = page.nextBefore == null ? null : string(page.nextBefore, 'nextBefore');
  if (hasMore !== !!nextCursor) throw new Error('历史分页游标与状态不一致');
  const revision = string(body.revision, 'revision');
  if (!revision || etag !== revision) throw new Error('历史修订与 ETag 不一致');
  let reconciliation: Exclude<RustHistoryResult, {notModified:true}>['reconciliation'];
  if (options.reconciliation) {
    const evidence = record(body.reconciliation, 'reconciliation');
    if (evidence.sessionId !== sessionId || evidence.sessionPath !== `native:${sessionId}`
      || typeof evidence.snapshotId !== 'string' || !/^[0-9a-f]{64}$/.test(evidence.snapshotId)
      || typeof evidence.runRevision !== 'string' || !/^(0|[1-9][0-9]{0,19})$/.test(evidence.runRevision)
      || BigInt(evidence.runRevision) > 18446744073709551615n
      || !['running', 'reconciled_idle', 'unknown'].includes(String(evidence.runStatus))) throw new Error('历史协调证明无效');
    reconciliation = {sessionId, sessionPath: evidence.sessionPath as string, snapshotId: evidence.snapshotId, runRevision: evidence.runRevision, complete: bool(evidence.complete, 'reconciliation.complete'), runStatus: evidence.runStatus as NonNullable<typeof reconciliation>['runStatus']};
    if (reconciliation.complete === hasMore || !reconciliation.complete && reconciliation.runStatus !== 'unknown') throw new Error('历史协调完整度不一致');
  }
  return { notModified: false, items: projectRustHistoryItems(body.items, sessionId), rawItems: body.items, revision, etag, page: { hasMore, nextCursor }, ...(reconciliation ? {reconciliation} : {}) };
}

/** 大载荷必须明确失败，不能把读取截断后的半页当作成功。 */
async function boundedJson(response: Response): Promise<unknown> {
  const maxBytes = 32 * 1024 * 1024;
  if (Number(response.headers.get('content-length')) > maxBytes) throw new Error('历史响应过大，请缩小分页范围');
  const reader = response.body?.getReader();
  if (!reader) throw new Error('历史响应正文缺失');
  const decoder = new TextDecoder('utf-8', {fatal:true}); let text = ''; let bytes = 0;
  try {
    for (;;) { const chunk = await reader.read(); if (chunk.done) break; bytes += chunk.value.byteLength; if (bytes > maxBytes) throw new Error('历史响应过大，请缩小分页范围'); text += decoder.decode(chunk.value, {stream:true}); }
    text += decoder.decode(); return JSON.parse(text) as unknown;
  } finally { await reader.cancel(); reader.releaseLock(); }
}

/* ── R08-T01：TraceDetailOverlay 会话叙事通道（canonical → 旧最小形状） ──
 * 旧 GET /api/sessions/messages 在 Rust 拓扑下不存在；canonical 历史
 * （/lingxi/v1/sessions/:id/history）形状不同。本段把 canonical 项投影为
 * trace-conversation-model.parseSessionMessages 读取的旧子集，保住轨迹
 * 详情的会话 join。端口决策（如实标注损失，不静默降级）：
 * - 工具 args 只取 argsSummary：canonical 不存完整参数串（完整载荷属观测
 *   调用侧，轨迹视图本就从观测载荷取）；
 * - 工具 startedAt/endedAt 省略：canonical 只有 firstSeq；解析器允许缺省，
 *   时间 join 回落到观测调用自身时间戳；
 * - modelCallRef 只填 modelCallId（traceId/parentCallId 置 null——
 *   modelCallId 是精确 join 键，观测侧据此反查）；
 * - turnStatus 走现役外部词表 completed/failed/aborted：canonical
 *   cancelled→aborted、interrupted_needs_attention→failed（需要注意的
 *   终态如实标错，绝不显示为正常）；
 * - 无 final 的 run：正文取 final_answer/commentary/unresolved 段、推理
 *   取 reasoning 段；段无时间戳——诚实置 null（缺时间戳消息按相邻消息
 *   归属轮次，解析器既有语义）；
 * - Node 连接沿旧 URL 原样：旧字面量留在本 native 双分支文件。 */

export type TraceSessionMessage = Record<string, unknown>;

/** canonical run 终态 → 旧外部词表（transport 兼容映射，见 RunStatus 文档）。 */
const LEGACY_TURN_STATUS: Readonly<Record<string, string>> = {
  completed: 'completed',
  failed: 'failed',
  cancelled: 'aborted',
  interrupted_needs_attention: 'failed',
};

/** canonical HistoryItem[] → 旧 /api/sessions/messages messages[] 最小形状（纯函数，供测试）。 */
export function projectRustHistoryForTrace(items: unknown[]): TraceSessionMessage[] {
  if (!Array.isArray(items)) throw new Error('历史输入无效');
  interface TurnDraft {
    key: string;
    anchorIndex: number;
    final?: Row;
    segments: Row[];
    toolCalls: TraceSessionMessage[];
    turnStatus?: string;
    hasTerminal: boolean;
  }
  const turns = new Map<string, TurnDraft>();
  const turnFor = (key: string, index: number): TurnDraft => {
    let draft = turns.get(key);
    if (!draft) {
      draft = { key, anchorIndex: index, segments: [], toolCalls: [], hasTerminal: false };
      turns.set(key, draft);
    }
    return draft;
  };
  // 输出槽：user 直接落位；assistant 按 turn 聚合后落在锚点（final > 终态 > 首项）。
  const userSlots: Array<{ index: number; entry: TraceSessionMessage }> = [];
  items.forEach((value, index) => {
    const row = record(value, 'item');
    const kind = string(row.kind, 'kind');
    switch (kind) {
      case 'user_message': {
        const legacy = row.legacyFields == null ? {} : record(row.legacyFields, 'legacyFields');
        const entry: TraceSessionMessage = {
          role: 'user',
          entryId: string(row.messageId, 'messageId'),
          content: string(row.text, 'text'),
          timestamp: row.committedAtUnixMs,
        };
        if (typeof legacy.displayText === 'string') entry.displayText = legacy.displayText;
        if (Array.isArray(legacy.images)) entry.images = legacy.images;
        userSlots.push({ index, entry });
        break;
      }
      case 'assistant_segment': {
        const draft = turnFor(runId(row) ?? `unknown-segment:${string(row.segmentId, 'segmentId')}`, index);
        draft.segments.push(row);
        break;
      }
      case 'tool_call': {
        const draft = turnFor(runId(row) ?? `unknown-tool:${string(row.toolCallId, 'toolCallId')}`, index);
        const status = string(row.status, 'status');
        const toolCall: TraceSessionMessage = {
          id: string(row.toolCallId, 'toolCallId'),
          name: string(row.target, 'target'),
          status,
          success: status === 'success',
        };
        if (typeof row.argsSummary === 'string') toolCall.args = row.argsSummary;
        if (row.result != null) {
          const result = record(row.result, 'tool result');
          if (result.error != null) {
            const error = record(result.error, 'tool error');
            if (typeof error.message === 'string') toolCall.error = error.message;
          }
          if (result.details != null) toolCall.details = record(result.details, 'tool details');
        }
        draft.toolCalls.push(toolCall);
        break;
      }
      case 'final_message': {
        const draft = turnFor(runId(row) ?? `unknown-final:${string(row.messageId, 'messageId')}`, index);
        draft.final = row;
        draft.anchorIndex = index;
        break;
      }
      case 'run_terminal': {
        const status = string(row.status, 'status');
        const draft = turnFor(string(row.runId, 'runId'), index);
        draft.turnStatus = LEGACY_TURN_STATUS[status] ?? status;
        draft.hasTerminal = true;
        if (draft.final === undefined) draft.anchorIndex = index;
        break;
      }
      // reset_marker / legacy_raw / 未来新增 kind：对轨迹叙事无对应物，
      // 跳过（旧投影同样不产生 user/assistant 之外的条目）。
      default: break;
    }
  });
  const output: Array<{ index: number; entry: TraceSessionMessage }> = [...userSlots];
  for (const draft of turns.values()) {
    const entry: TraceSessionMessage = { role: 'assistant', entryId: '', content: '', timestamp: null };
    let content = '';
    let thinking = '';
    if (draft.final !== undefined) {
      entry.entryId = string(draft.final.messageId, 'messageId');
      entry.timestamp = draft.final.committedAtUnixMs;
      if (typeof draft.final.modelCallId === 'string' && draft.final.modelCallId !== '') {
        entry.modelCallRef = { modelCallId: draft.final.modelCallId, traceId: null, parentCallId: null };
      }
      // final content 已过规范化 scanner：text/reasoning 块拼接；
      // resource_ref/opaque 块不编造文本。
      const blocks = Array.isArray(draft.final.content) ? draft.final.content : [];
      for (const value of blocks) {
        const block = record(value, 'content block');
        const blockType = string(block.type, 'block.type');
        if (blockType === 'text') content += string(block.text, 'block.text');
        else if (blockType === 'reasoning') thinking += string(block.text, 'block.text');
      }
    } else {
      entry.entryId = `${draft.key}-partial`;
      for (const segment of draft.segments) {
        const phase = string(segment.phase, 'phase');
        const segmentKind = string(segment.segmentKind, 'segmentKind');
        const text = string(segment.text, 'text');
        if (segmentKind === 'reasoning' || phase === 'reasoning') thinking += text;
        else content += text;
      }
    }
    entry.content = content;
    if (thinking !== '') entry.thinking = thinking;
    if (draft.turnStatus !== undefined) entry.turnStatus = draft.turnStatus;
    if (draft.toolCalls.length > 0) entry.toolCalls = draft.toolCalls;
    output.push({ index: draft.anchorIndex, entry });
  }
  output.sort((a, b) => a.index - b.index);
  return output.map(slot => slot.entry);
}

/**
 * R08-T01：TraceDetailOverlay 的会话叙事读取（Node/Rust 双分支）。
 * Rust：canonical 全量历史 → projectRustHistoryForTrace；Node：旧 URL 原样。
 * 失败抛出——调用方既有语义是 catch → null → calls-only 降级并显式标注。
 */
export async function readTraceSessionMessages(
  sessionId: string,
  signal?: AbortSignal,
): Promise<TraceSessionMessage[] | null> {
  const connection = requireServerConnection(
    useStore.getState(),
    'trace session messages: server connection not ready',
  );
  if (connection.serverNodeKind === 'lingxi-service') {
    const history = await readRustChatHistory(connection, sessionId, { all: true, signal });
    // all:true 下服务禁止 304（readRustChatHistory 亦对此响亮失败）；运行期
    // 不可达。tsconfig.test.json 关 strictNullChecks，判别联合收窄失效，
    // 用显式 Exclude 取全量分支。
    if (history.notModified) throw new Error('全量历史读取不得命中 304');
    const full = history as Exclude<RustHistoryResult, { notModified: true }>;
    return projectRustHistoryForTrace(full.rawItems);
  }
  const res = await fetch(
    buildConnectionUrl(connection, `/api/sessions/messages?sessionId=${encodeURIComponent(sessionId)}&all=1`),
    { headers: appendConnectionAuth(connection, undefined), signal },
  );
  if (!res.ok) throw new Error(`读取会话消息失败：${res.status}`);
  const data: unknown = await res.json();
  const messages = (data as { messages?: unknown } | null)?.messages;
  return Array.isArray(messages) ? messages as TraceSessionMessage[] : null;
}
