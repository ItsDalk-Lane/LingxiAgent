/**
 * chat-types.ts — 聊天消息数据模型
 *
 * 历史消息和流式消息共用同一套类型。
 * ContentBlock 按展示顺序排列（thinking → mood → tools → text），
 * 不按流式到达顺序。
 */

import type { FileVersion } from '../types';
import type { ThinkingLevel } from './model-slice';
import type { AttachedFile, QuotedSelection } from './input-slice';
import type { KnowledgeReferenceMode, KnowledgeRetrievalStats, LegacyKnowledgeReferenceMode } from '../../../../shared/knowledge-refs.ts';
import type { EditorAgentMention, EditorFileRef, EditorSessionRef } from '../utils/editor-serializer';

/**
 * 输入框「发送派发」的快照载荷：直发、排队续发、「立即插入」三条路径共用。
 * text 是用户正文；附件清单/文档引用/引用片段在派发时按快照重新拼接。
 */
export interface ComposerSendBundle {
  type: 'prompt' | 'interject';
  sessionRef: { sessionId: string; sessionPath: string; agentId: string | null };
  text: string;
  skills: string[];
  fileRefs: EditorFileRef[];
  sessionRefs: EditorSessionRef[];
  agentMentions: EditorAgentMention[];
  /** 附件快照（attachedFiles + 编辑器文件徽章合并结果；含粘贴图片的 base64Data 缓存）。 */
  inputFiles: AttachedFile[];
  knowledgeRefs: { notebookIds: string[]; notebookNames: Record<string, string>; mode: KnowledgeReferenceModeDisplay } | null;
  docContextAttached: boolean;
  doc: { path: string; name: string } | null;
  quotes: QuotedSelection[];
  uiContext: unknown;
}

/** 流式期间发送的用户输入：入队等待，上一轮回答结束后自动续发。 */
export interface QueuedTurnInput {
  id: string;
  sessionPath: string;
  /** 卡片展示与再编辑的纯文本（= bundle.text 的镜像）。 */
  text: string;
  createdAt: number;
  bundle: ComposerSendBundle;
  /**
   * 快照版本：编辑保存递增（F1）。在途准备按版本对账，迟到的旧版本结果
   * 不得覆盖更新后的队列项。
   */
  snapshotVersion?: number;
  /** 编辑保护由 coordinator 同步设置，不改变进入编辑前的失败状态。 */
  editing?: boolean;
  /**
   * 调度状态：ready = 等待自动/手动派发；blocked / failed = 上次派发被门禁
   * 拦下或提交前失败，队列项原位保留全部字段，等待用户编辑/删除/显式重试
   *（自动调度不会在失败项上无限循环，也不跳过队首）。
   */
  status?: 'ready' | 'blocked' | 'failed';
  /** blocked/failed 时的显式错误码（与 ComposerDispatchResult.code 一致）。 */
  errorCode?: string;
  /** 上次失败是否允许按原快照显式重试（默认 true；门禁类 blocked 为 false）。 */
  retryable?: boolean;
}

/**
 * 消息投影上的知识引用模式：新值 fast/detailed + 存量 qa/assist（旧消息
 * 显示层保留原标签；存量值只在展示语境出现，提交链路已被 normalize 拒绝）。
 */
export type KnowledgeReferenceModeDisplay = KnowledgeReferenceMode | LegacyKnowledgeReferenceMode;

// ── 工具调用 ──

export interface ToolCall {
  id?: string;
  name: string;
  args?: Record<string, unknown>;
  done: boolean;
  success: boolean;
  status?: 'running' | 'succeeded' | 'failed' | 'unknown';
  error?: string;
  details?: { [key: string]: unknown };
  /** 完成态行内结果注记（如「50 个结果」）：合成过程卡（knowledge_*）经 tool_end
   * resultNote 透出；真实工具不携带。 */
  resultNote?: string;
}

export interface DeferredHistoryContent {
  id: string;
  kind: 'assistant_segment' | 'tool_output' | 'tool_search' | 'tool_input' | 'tool_patch' | 'tool_file_content' | 'skill_content' | 'screenshot' | 'artifact' | 'inline_image';
  size: number;
  preview?: string;
  available: true;
}

// ── 用户附件 ──

export interface UserAttachment {
  fileId?: string;
  path: string;
  name: string;
  isDir: boolean;
  base64Data?: string;
  mimeType?: string;
  deferred?: DeferredHistoryContent;
  presentation?: 'attachment' | 'voice-input' | string;
  listed?: boolean;
  status?: 'available' | 'expired' | string;
  missingAt?: number | null;
  visionAuxiliary?: boolean;
  transcription?: VoiceTranscription;
  waveform?: AudioWaveform;
}

export interface AudioWaveform {
  version: 1;
  peaks: number[];
  durationMs?: number;
  source?: 'computed' | 'fallback';
}

export interface VoiceTranscription {
  status: 'pending' | 'ready' | 'failed';
  text?: string;
  providerId?: string;
  modelId?: string;
  protocolId?: string;
  language?: string;
  durationMs?: number;
  error?: string;
  createdAt?: number;
  updatedAt?: number;
}

export interface AgentReviewContext {
  requestId?: string | null;
  status: 'running' | 'completed' | 'failed' | 'cancelled';
  reviewedSessionId?: string | null;
  reviewerSessionId?: string | null;
  reviewerAgentId: string;
  reviewerAgentName: string;
  text?: string | null;
  error?: string | null;
  completedAt?: string | null;
}

export interface AgentReviewRequestContext {
  requestId?: string | null;
  reviewedSessionId: string;
  reviewerAgentId: string;
  reviewerAgentName: string;
}

export interface DeskContext {
  dir: string;
  fileCount: number;
}

export interface SessionRegistryFile {
  id?: string;
  fileId?: string;
  sessionPath?: string;
  filePath?: string;
  realPath?: string;
  legacyFileIds?: string[];
  legacyFilePaths?: string[];
  label?: string;
  displayName?: string;
  filename?: string;
  ext?: string;
  mime?: string;
  kind?: string;
  storageKind?: string;
  presentation?: 'attachment' | 'voice-input' | string;
  listed?: boolean;
  status?: 'available' | 'expired' | string;
  missingAt?: number | null;
  origin?: string;
  operations?: string[];
  createdAt?: number;
  mtimeMs?: number;
  size?: number | null;
  version?: FileVersion | null;
  isDirectory?: boolean;
  resource?: ResourceEnvelope;
  transcription?: VoiceTranscription;
  waveform?: AudioWaveform;
}

export interface ResourceEnvelope {
  schemaVersion: 1;
  resourceId: string;
  name: string;
  studioId: string;
  type: 'file' | string;
  source: 'session_file' | string;
  sourceId?: string;
  fileId?: string;
  displayName?: string;
  filename?: string;
  ext?: string | null;
  mime?: string;
  size?: number | null;
  kind?: string;
  isDirectory?: boolean;
  origin?: string;
  operations?: string[];
  createdAt?: number | string;
  mtimeMs?: number;
  lifecycle: {
    status: 'available' | 'expired' | string;
    missingAt: number | string | null;
  };
  storage: {
    provider: 'session_file' | string;
    storageKind?: string;
    localOnly?: boolean;
  };
  links: {
    self: string;
    content?: string;
  };
}

// ── 内容块 ──

export type AssistantSemanticPhase = 'reasoning' | 'commentary' | 'mood' | 'tool' | 'final_answer' | 'unresolved';
export type ContentSurfaceRole = 'process' | 'provisional' | 'answer' | 'result' | 'control';
export type ContentLifecycle = 'streaming' | 'sealed';
export type AssistantTurnStatus = 'streaming' | 'completed' | 'failed' | 'aborted';
/** 回合结局的唯一裁决来源（turn-outcome.ts）；渲染层不得自行猜测。 */
export type AssistantTurnOutcome =
  | 'streaming'
  | 'completed_with_answer'
  | 'completed_with_result'
  | 'completed_with_control'
  | 'completed_without_user_output'
  | 'failed'
  | 'aborted';
export type MissingFinalAnswerReason = 'no_final_answer_segment' | 'only_process_blocks' | 'empty_final_answer';

export interface AssistantTurnDiagnostics {
  code: 'unresolved_phase_fallback' | 'no_final_answer_segment' | 'only_process_blocks' | 'empty_final_answer';
  segmentId?: string;
  fallbackPhase?: 'final_answer';
}

export interface AssistantTurnProjection {
  id: string;
  inputMessageId: string | null;
  assistantMessageIds: string[];
  processBlockIds: string[];
  /** 流式期身份未判明的临时文字（unresolved 段），只存在于 live 投影 */
  provisionalBlockIds?: string[];
  answerBlockIds: string[];
  resultBlockIds: string[];
  controlBlockIds: string[];
  status: AssistantTurnStatus;
  outcome?: AssistantTurnOutcome;
  missingFinalAnswerReason?: MissingFinalAnswerReason;
  diagnostics?: AssistantTurnDiagnostics[];
  startedAt?: number;
  completedAt?: number;
}

/**
 * 新聊天投影使用的统一语义字段。
 * 字段在兼容期保持可选，旧会话仍可按原形状读取；所有新建内容块会在构建边界补齐。
 */
export interface ContentBlockSemantics {
  id?: string;
  semanticPhase?: AssistantSemanticPhase;
  surfaceRole?: ContentSurfaceRole;
  lifecycle?: ContentLifecycle;
  /**
   * 过程区内的到达序号（越小越早）：事件入口在块首次物化时盖戳，
   * 投影层按它把思考段与工具块交错回真实时间线；旧数据没有此字段时保持原顺序。
   */
  processOrder?: number;
}

export interface SessionConfirmationBlock {
  type: 'session_confirmation';
  confirmId: string;
  kind: string;
  surface: 'input' | 'message';
  status: 'pending' | 'confirmed' | 'rejected' | 'timeout' | 'aborted';
  title: string;
  body?: string;
  subject?: {
    label: string;
    detail?: string;
  };
  severity?: 'normal' | 'elevated' | 'danger';
  actions?: {
    confirmLabel?: string;
    rejectLabel?: string;
  };
  payload?: Record<string, unknown>;
}

export interface SettingsUpdateChange {
  key: string;
  label: string;
  before: string;
  after: string;
  sensitive?: boolean;
}

export interface SettingsUpdatePayload {
  status: 'applied' | 'failed' | 'skipped' | 'needs_action' | string;
  action: string;
  key: string;
  title: string;
  summary: string;
  target?: {
    type?: string;
    id?: string | null;
    label?: string | null;
  };
  changes?: SettingsUpdateChange[];
}

export interface SuggestionCardBlock {
  type: 'suggestion_card';
  kind: 'automation_draft' | string;
  confirmId?: string;
  suggestionId?: string;
  suggestionShortCode?: string;
  operation?: 'create' | 'update' | string;
  status: 'pending' | 'approved' | 'rejected' | string;
  title: string;
  description?: string;
  target?: {
    type?: string;
    id?: string | null;
    label?: string | null;
  };
  detail?: {
    kind?: string;
    operation?: 'create' | 'update' | string;
    jobData?: Record<string, unknown>;
    [key: string]: unknown;
  };
  actions?: Array<{
    id?: string;
    kind?: string;
    label?: string;
  }>;
}

// 物种 A：文本装饰器（流式组装，upsert 到 blocks 数组）
export type TextDecorator = ContentBlockSemantics & (
  | { type: 'thinking'; content: string; sealed: boolean; deferred?: DeferredHistoryContent }
  | { type: 'mood'; yuan: string; text: string }
  | { type: 'tool_group'; tools: ToolCall[]; collapsed: boolean }
  // COMPAT(v0.128, remove no earlier than v0.133): 旧会话可能只有 html；新块必须写 source。
  | (
    | { type: 'text'; source: string; html?: string; deferred?: DeferredHistoryContent }
    | { type: 'text'; source?: undefined; html: string; deferred?: DeferredHistoryContent }
  )
);

// 物种 B：富内容块（通过 content_block 事件 push，不 upsert）
export type RichBlock = ContentBlockSemantics & (
  | { type: 'file'; fileId?: string; filePath: string; label: string; ext: string; mime?: string; kind?: string; storageKind?: string; presentation?: 'attachment' | 'voice-input' | string; listed?: boolean; status?: 'available' | 'expired' | string; missingAt?: number | null; resource?: ResourceEnvelope; mtimeMs?: number; size?: number | null; version?: FileVersion | null; waveform?: AudioWaveform; replacesTaskId?: string }
  | { type: 'media_generation'; taskId: string; kind: 'image' | 'video' | string; status: 'pending' | 'failed' | 'aborted' | string; prompt?: string; batchId?: string; reason?: string }
  // COMPAT(create_artifact, remove no earlier than v0.133 after legacy sessions are migrated)
  | { type: 'artifact'; artifactId: string; artifactType: string; title: string; content: string; deferred?: DeferredHistoryContent; language?: string | null; fileId?: string; filePath?: string; label?: string; ext?: string; mime?: string; kind?: string; storageKind?: string; presentation?: 'attachment' | 'voice-input' | string; listed?: boolean; status?: 'available' | 'expired' | string; missingAt?: number | null; resource?: ResourceEnvelope; mtimeMs?: number; size?: number | null; version?: FileVersion | null }
  | { type: 'screenshot'; base64?: string; mimeType: string; deferred?: DeferredHistoryContent }
  | { type: 'skill'; skillName: string; skillFilePath: string; fileId?: string; installedFile?: Record<string, unknown>; installedSkillSource?: Record<string, unknown> }
  | { type: 'cron_confirm'; confirmId?: string; jobData: Record<string, unknown>; status: 'pending' | 'approved' | 'rejected' }
  | SuggestionCardBlock
  | { type: 'settings_confirm'; confirmId?: string; settingKey: string; cardType: 'toggle' | 'list' | 'text'; currentValue: string; proposedValue: string; options?: string[]; optionLabels?: Record<string, string>; label: string; description?: string; frontend?: boolean; status: 'pending' | 'confirmed' | 'rejected' | 'timeout' }
  | { type: 'settings_update'; update: SettingsUpdatePayload }
  | SessionConfirmationBlock
  | {
    type: 'interlude';
    id: string;
    deliveryId?: string;
    variant: 'deferred_result' | string;
    timelinePlacement?: 'after_anchor_message' | string;
    taskId?: string;
    status?: 'success' | 'failed' | 'aborted' | string;
    sourceKind?: 'subagent' | 'workflow' | 'tool' | string;
    sourceLabel?: string;
    previewSessionId?: string;
    previewSessionPath?: string;
    previewAgentId?: string;
    text: string;
    detailMarkdown?: string;
    turnCount?: number;
    maxTurns?: number;
  }
  | {
    type: 'subagent';
    taskId: string;
    task: string;
    taskTitle: string;
    agentId?: string;
    agentName?: string;
    requestedAgentId?: string;
    requestedAgentName?: string;
    executorAgentId?: string;
    executorAgentNameSnapshot?: string;
    sessionId?: string | null;
    streamKey: string;
    streamStatus: 'running' | 'done' | 'failed' | 'aborted';
    summary?: string;
    label?: string | null;
    reuseInstance?: string | null;
  }
  | {
    // workflow inline 概览块（聊天流工具卡）：只携带「名 + 状态 + 时长」，不展开实时流。
    type: 'workflow';
    taskId: string;
    taskTitle: string;
    streamStatus: 'running' | 'done' | 'failed' | 'aborted';
    summary?: string;
    startedAt?: number | null;
    finishedAt?: number | null;
  }
  | { type: 'interactive_card'; cardId: string; title: string; code: string }
  | {
    type: 'turn_status';
    status: 'missing_final_answer' | 'failed' | 'aborted';
    label?: string;
  }
);

export type ContentBlock = TextDecorator | RichBlock;

// ── 消息 ──

// ── 历史分页 Run 缝合事实 ──
// 分页按原始记录切页，一个 Run 可能跨多页。被截断的 Run 项携带这些原始事实，
// 更早页片段到达时与既有项缝合并重新投影；Run 头部记录加载完成后即丢弃（内存有界）。

/** Run 缝合所需的单条原始助手记录事实（display id = 服务端 display 序号）。 */
export interface HistoryRunRecordFact {
  displayId: string;
  entryId?: string;
  content: string;
  thinking?: string | null;
  toolCalls?: Array<Record<string, unknown>> | null;
  /** 该记录的持久化语义分段（原始形态，未加组内偏移）。 */
  segments: Array<{
    id: string;
    kind: 'text' | 'reasoning';
    semanticPhase: 'reasoning' | 'commentary' | 'final_answer';
    source: string;
    lifecycle: 'sealed';
    deferred?: DeferredHistoryContent;
    processOrder?: number;
  }>;
  turnStatus?: 'failed' | 'aborted';
  /** 该记录锚定的页级 blocks（已在页构建时解析为记录归属，不含 interlude）。 */
  inlineBlocks: Array<Record<string, unknown>>;
}

/** 同一 Run 的跨页缝合事实。records 按旧→新追加；runKey 即 Run 身份键。 */
export interface HistoryRunFacts {
  runKey: string;
  turnStartIndex: number;
  turnEndIndex: number;
  turnInputEntryId?: string;
  turnInputVisible?: boolean;
  firstRecordTimestamp?: number;
  records: HistoryRunRecordFact[];
}

export interface ChatMessage {
  id: string;              // UI message id；本地发送的 user message 可先使用 clientMessageId
  clientMessageId?: string; // 可验证的桌面输入关联；分页ID仍保持历史索引
  sourceEntryId?: string;  // Pi SDK session entry id，用于 branch-aware 的重新生成/编辑
  /** 本次 Agent turn 的真实输入 entry；隐藏后台输入也必须保留，不能猜到最近可见 user。 */
  turnInputEntryId?: string;
  /** false 表示真实 turn input 只用于模型上下文，不对应可见用户节点。 */
  turnInputVisible?: boolean;
  role: 'user' | 'assistant';
  // User
  text?: string;
  textHtml?: string;
  quotedText?: string;
  attachments?: UserAttachment[];
  deskContext?: DeskContext | null;
  skills?: string[];
  sessionRefs?: Array<{ sessionId: string; label: string }>;
  agentMentions?: Array<{ agentId: string; label: string }>;
  /** 本条消息引用的知识库笔记本（展示投影；notebooks 为名称缓存，可能滞后于重命名） */
  knowledgeRefs?: {
    notebookIds: string[];
    mode: KnowledgeReferenceModeDisplay;
    notebooks?: Array<{ id: string; name?: string }>;
  };
  /** 本次知识库注入的检索统计（session_user_message 回显 / 历史透出；乐观消息无此字段）。 */
  knowledgeRetrieval?: KnowledgeRetrievalStats;
  agentReview?: AgentReviewContext;
  agentReviewRequest?: AgentReviewRequestContext;
  sendStatus?: 'pending' | 'failed';
  sendError?: string;
  /** 失败后是否允许按原快照显式重试（服务端拒绝回执携带；C01）。 */
  sendRetryable?: boolean;
  /** 非用户本人发出的消息来源（如别的 Agent 经跨 session 协作投递）。老数据无此字段，按普通用户消息渲染。 */
  origin?: { kind: 'agent'; agentId: string | null; agentName: string | null };
  // Assistant
  blocks?: ContentBlock[];
  turnProjection?: AssistantTurnProjection;
  /** 跨页 Run 缝合事实：仅当该 Run 被分页截断（头部记录未加载）时携带。 */
  runFacts?: HistoryRunFacts;
  // 通用
  timestamp?: number;
}

// ── Virtuoso 列表项 ──

export type ChatListItem =
  | { type: 'message'; data: ChatMessage }
  | { type: 'interlude'; id: string; data: Extract<ContentBlock, { type: 'interlude' }> }
  | { type: 'compaction'; id: string; yuan: string };

// ── Per-session 模型快照 ──
// 挂在 chat-slice 的 sessionModelsByPath keyed map 上，
// 与消息缓存 SessionMessages 解耦：模型信息可以在消息还没加载时独立写入，
// 不会因为在 chatSessions 里创建 stub 而骗过"是否已加载"的判据（issue #405）。

export interface SessionModel {
  id: string;
  name: string;
  provider: string;
  /** 输入模态数组（Pi SDK 标准字段），镜像后端 /models, /models/switch 响应；音频走 Hana 兼容能力字段。 */
  input?: ("text" | "image" | "video" | "audio")[];
  video?: boolean;
  videoTransport?: string | null;
  videoTransportSupported?: boolean;
  audio?: boolean;
  audioTransport?: string | null;
  audioTransportSupported?: boolean;
  reasoning?: boolean;
  xhigh?: boolean;
  thinkingLevels?: ThinkingLevel[];
  defaultThinkingLevel?: ThinkingLevel;
  contextWindow?: number;
  /** Historical sessions can remain readable while their exact model is no longer executable. */
  available?: boolean;
  unavailableReason?: 'model_removed' | 'provider_not_configured' | 'temporarily_unavailable' | null;
}

// ── Per-session 消息状态 ──
// entry 存在 ⟺ 消息状态已初始化（initSession 调用过）。
// 不要为了存别的东西（例如模型快照）就写 stub 进来——会把这个语义打破。

export interface SessionMessages {
  items: ChatListItem[];
  hasMore: boolean;
  loadingMore: boolean;
  oldestId?: string;
  /**
   * 服务端下发的下一页游标（display 序号字符串；null=没有更早记录）。
   * 分页边界由服务端原始页面范围决定；oldestId 是显示项身份，禁止再当游标用（F1）。
   * 兼容旧服务端（无 nextBefore）时由原始首条记录 id 回退推导。
   */
  nextBefore?: string | null;
  /**
   * hydrate 时服务端返回的磁盘修订点（stat 签名）。
   * null = 未知（如 WS 端为新会话 initSession 的空状态，或服务端 stat 失败）。
   * reconcileCurrentSessionMessages 用它与 /api/sessions 列表投影的 revision
   * 对比，决定是否补拉离线窗口（/rc 接管等）漏掉的消息（issue #1610）。
   */
  revision?: string | null;
}

// ── 流式缓冲（不入 Zustand） ──

export interface StreamBuffer {
  sessionPath: string;
  textAcc: string;
  thinkingAcc: string;
  moodAcc: string;
  moodYuan: string;
  inThinking: boolean;
  inMood: boolean;
  lastFlushTime: number;
}
