/**
 * composer-send.ts — 输入框「发送派发」的组件外抽取。
 *
 * 三条路径共用同一套实现（避免行为分叉）：
 *   1. 普通发送（InputArea.submitEditorMessage）
 *   2. 排队消息的自动续发（上一轮回答结束后）
 *   3. 排队消息的「立即插入」（注入进行中的回合）
 *
 * 输入是 prepare 阶段快照好的 ComposerSendBundle：派发时按「当下」的模型/能力
 * 状态重新做图片/视频/音频预检与 base64 读取，而非入队时刻的状态。
 */

import { useStore } from '../../stores';
import { sessionScopedValue } from '../../stores/session-slice';
import type { AttachedFile } from '../../stores/input-slice';
import { upsertOptimisticSessionFirstMessage } from '../../stores/session-actions';
import { getWebSocket } from '../../services/websocket';
import { renderMarkdown } from '../../utils/markdown';
import { isImageFile, isVideoFile } from '../../utils/format';
import { isAudioFileName } from '../../utils/file-kind';
import type { ComposerSendBundle } from '../../stores/chat-types';
import {
  evaluateChatAudioSendPreflight,
  evaluateChatImageSendPreflight,
  evaluateChatVideoSendPreflight,
  notifyChatVideoFormatUnsupported,
  notifyTextModelImageFileOnly,
  notifyVideoSendBlockedByModel,
} from '../../utils/chat-image-send-preflight';
import { openProviderModelSettings } from '../../utils/model-settings-navigation';
import { formatQuotedSelectionForPrompt } from '../../utils/quoted-selection';
import {
  isAllowedChatVideoMime,
  isChatVideoBase64ContentCompatible,
  isChatVideoBase64WithinLimit,
} from '../../../../../shared/video-mime.ts';

export type { ComposerSendBundle };

export function createClientUserMessageId(): string {
  const uuid = globalThis.crypto?.randomUUID?.();
  if (uuid) return `client-user-${uuid}`;
  return `client-user-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function chatVideoMimeTypeForName(name: string, fallback?: string): string {
  if (fallback?.startsWith('video/')) return fallback;
  const ext = name.toLowerCase().replace(/^.*\./, '');
  const mimeMap: Record<string, string> = {
    mp4: 'video/mp4',
    m4v: 'video/mp4',
    webm: 'video/webm',
    mov: 'video/quicktime',
    mkv: 'video/x-matroska',
  };
  return mimeMap[ext] || 'video/mp4';
}

export function chatImageMimeTypeForName(name: string, fallback?: string): string {
  if (fallback?.startsWith('image/')) return fallback;
  const ext = name.toLowerCase().replace(/^.*\./, '');
  const mimeMap: Record<string, string> = {
    png: 'image/png',
    jpg: 'image/jpeg',
    jpeg: 'image/jpeg',
    gif: 'image/gif',
    webp: 'image/webp',
    bmp: 'image/bmp',
    svg: 'image/svg+xml',
  };
  return mimeMap[ext] || 'image/png';
}

export function chatAudioMimeTypeForName(name: string, fallback?: string): string {
  if (fallback?.startsWith('audio/')) return fallback;
  const ext = name.toLowerCase().replace(/^.*\./, '');
  const mimeMap: Record<string, string> = {
    mp3: 'audio/mpeg',
    wav: 'audio/wav',
    ogg: 'audio/ogg',
    flac: 'audio/flac',
    m4a: 'audio/mp4',
    weba: 'audio/webm',
    webm: 'audio/webm',
  };
  return mimeMap[ext] || 'audio/wav';
}

export interface ComposerSendOptions {
  /** 视觉辅助配置读取器（桌面/移动端面各自实现，沿用 InputArea 的闭包）。 */
  loadVisionAuxiliaryConfig: () => Promise<{ enabled: boolean; model: string | null }>;
  /** 组件侧 i18n 函数（沿用 InputArea 的 useI18n 实例，文案键与调用方一致）。 */
  t: (key: string, params?: Record<string, string | number>) => string;
}

export function modelUnavailableMessageKey(reason: string | null | undefined): string {
  if (reason === 'model_removed') return 'model.unavailableReason.modelRemoved';
  if (reason === 'provider_not_configured') return 'model.unavailableReason.providerNotConfigured';
  return 'model.unavailableReason.temporarilyUnavailable';
}

/**
 * 派发显式结果（F1/P2.2）：不再以 Promise<void> 混淆成功与阻止。
 * - transport_submitted：仅表示 WebSocket 提交动作成功，不代表服务端已接收/落盘。
 * - blocked：门禁/预检拦下，没有发送；输入与快照原位保留，可按语义重试。
 * - failed_before_submit：准备/提交执行失败，乐观消息若已创建则同卡标失败。
 * - delivery_unknown：提交后断线或回执超时，服务端是否收到不可证；禁止自动重发。
 */
export type ComposerDispatchResult =
  | { kind: 'transport_submitted'; clientMessageId: string }
  | { kind: 'blocked'; code: string; retryable: boolean }
  | { kind: 'failed_before_submit'; code: string; retryable: boolean }
  | { kind: 'delivery_unknown'; clientMessageId: string; code: string };

/** prepare 阶段的可能失败：尚未上线路由，只会是 blocked / failed_before_submit。 */
export type ComposerPreSubmitResult = Extract<ComposerDispatchResult, { kind: 'blocked' | 'failed_before_submit' }>;
/** commit 的返回：要么提交成功，要么提交前明确失败/被拦；delivery_unknown 只来自提交后的看门狗/断连。 */
export type ComposerCommitResult = Exclude<ComposerDispatchResult, { kind: 'delivery_unknown' }>;

/** prepare 的产出：commit 所需的全部载荷与上下文（不持有租约语义）。 */
export interface PreparedComposerSend {
  bundle: ComposerSendBundle;
  clientMessageId: string;
  sessionPathForSend: string;
  /** 预检所用模型（provider:id）；commit 重新核验模型未变更。 */
  modelKey: string | null;
  finalText: string;
  displayMessage: Record<string, unknown>;
  wsMsg: Record<string, unknown>;
  optimisticMessage: Record<string, unknown>;
}

export type ComposerPrepareOutcome =
  | { ok: true; prepared: PreparedComposerSend }
  | { ok: false; result: ComposerPreSubmitResult };

export interface ComposerPrepareOptions extends ComposerSendOptions {
  /** 复用既有逻辑消息身份（租约/重试）；缺省生成新 clientMessageId。 */
  clientMessageId?: string;
}

function webSocketOpenOrMissing(): 'open' | 'unavailable' {
  const ws = getWebSocket();
  if (!ws) return 'unavailable';
  // 测试 mock 可能不带 readyState；真实 WebSocket 必带，非 OPEN 一律拦截。
  if (typeof ws.readyState === 'number' && ws.readyState !== WebSocket.OPEN) return 'unavailable';
  return 'open';
}

/**
 * 准备阶段：模型快照 → 预检 → base64 读取 → finalText/载荷拼装。
 * 每个预检分支都返回显式结果；图片「提示后退化为文件引用」策略保留。
 * 不产生任何副作用式清理，也不创建乐观消息。
 */
export async function prepareComposerSend(
  bundle: ComposerSendBundle,
  options: ComposerPrepareOptions,
): Promise<ComposerPrepareOutcome> {
  const { loadVisionAuxiliaryConfig, t } = options;
  const sessionRef = bundle.sessionRef;

  // WebSocket 不存在/非 OPEN：连准备都不必做，明确 blocked（输入保留）。
  if (webSocketOpenOrMissing() !== 'open') {
    return { ok: false, result: { kind: 'blocked', code: 'websocket_unavailable', retryable: true } };
  }

  // 派发时刻的模型快照（排队消息按「实际发出时」的模型能力预检，而非入队时刻）。
  const state = useStore.getState();
  const sessionModel = bundle.sessionRef.sessionPath
    ? sessionScopedValue(state as any, state.sessionModelsByPath, bundle.sessionRef.sessionPath)
    : undefined;
  const models = state.models;
  const globalModelInfo = models.find(m => m.isCurrent);
  const sessionModelInfo = (() => {
    if (!sessionModel) return undefined;
    const full = models.find(m => m.id === sessionModel.id && m.provider === sessionModel.provider);
    return full ? { ...full, ...sessionModel } : sessionModel;
  })();
  const currentModelInfo = sessionModelInfo || globalModelInfo;
  const modelKey = currentModelInfo ? `${currentModelInfo.provider}:${currentModelInfo.id}` : null;
  // 模型被显式标记不可用：明确 blocked（与 InputArea 的点击侧守卫同一语义）。
  if (sessionModelInfo && (sessionModelInfo as { available?: boolean }).available === false) {
    useStore.getState().addToast(
      t(modelUnavailableMessageKey((sessionModelInfo as { unavailableReason?: string }).unavailableReason)),
      'warning',
      6000,
      { dedupeKey: 'session-model-unavailable' },
    );
    return { ok: false, result: { kind: 'blocked', code: 'model_unavailable', retryable: false } };
  }
  // input 数组缺失视为未知；只有显式 text-only 的模型才标记“辅助视觉”。
  const supportsVision = !Array.isArray(currentModelInfo?.input) || currentModelInfo.input.includes('image');

  const text = bundle.text;
  const skills = bundle.skills;
  const sessionRefs = bundle.sessionRefs;
  const agentMentions = bundle.agentMentions;
  const inputFiles = bundle.inputFiles;
  const hasFiles = inputFiles.length > 0;

  // 分离原生媒体和普通附件；后端决定图片视觉桥、视频/音频原生能力或显式报错。
  const imageFiles = hasFiles ? inputFiles.filter(f => !f.isDirectory && isImageFile(f.name)) : [];
  const videoFiles = hasFiles ? inputFiles.filter(f => !f.isDirectory && isVideoFile(f.name)) : [];
  const audioFiles = hasFiles ? inputFiles.filter(f => !f.isDirectory && isAudioFileName(f.name, f.mimeType)) : [];

  const imagePreflight = await evaluateChatImageSendPreflight({
    attachments: inputFiles,
    model: currentModelInfo,
    loadVisionAuxiliaryConfig,
  });
  // #1647：视觉能力不可用不再拦下整条消息。图片始终携带文件身份
  //（displayMessage.attachments → 服务端登记 SessionFile + 注入路径 marker），
  // 这里只决定是否附带像素载荷；降级是显式的（toast 告知 + 不读字节）。
  const imagesAsFileOnly = !imagePreflight.ok;
  if (imagesAsFileOnly) {
    notifyTextModelImageFileOnly({
      t,
      addToast: useStore.getState().addToast,
      openSettings: () => openProviderModelSettings(currentModelInfo?.provider),
    });
  }
  const videoPreflight = await evaluateChatVideoSendPreflight({
    attachments: inputFiles,
    model: currentModelInfo,
  });
  if (videoFiles.length > 3) {
    useStore.getState().addToast(t('error.maxVideos', { max: 3 }), 'error', 6000);
    return { ok: false, result: { kind: 'blocked', code: 'video_count_exceeded', retryable: false } };
  }
  const sendVideosNatively = videoPreflight.ok && videoPreflight.reason === 'native-video';
  if (!videoPreflight.ok) {
    // 两种拦截分流：模型无视频能力 vs 格式不在该端点契约交集内，提示语不同。
    if (videoPreflight.reason === 'video-format-unsupported') {
      notifyChatVideoFormatUnsupported({
        t,
        addToast: useStore.getState().addToast,
        mimeType: videoPreflight.mimeType,
      });
      return { ok: false, result: { kind: 'blocked', code: 'video_format_unsupported', retryable: false } };
    }
    notifyVideoSendBlockedByModel({
      t,
      addToast: useStore.getState().addToast,
      openSettings: () => openProviderModelSettings(currentModelInfo?.provider),
    });
    return { ok: false, result: { kind: 'blocked', code: 'video_blocked_by_model', retryable: false } };
  }
  const audioPreflight = await evaluateChatAudioSendPreflight({
    attachments: inputFiles,
    model: currentModelInfo,
  });
  const sendAudiosNatively = audioPreflight.ok && audioPreflight.reason === 'native-audio';
  const otherFiles = hasFiles ? inputFiles.filter(f =>
    f.isDirectory || (
      !isImageFile(f.name)
      && !(sendVideosNatively && isVideoFile(f.name))
      && !(sendAudiosNatively && isAudioFileName(f.name, f.mimeType))
    )
  ) : [];

  const sessionPathForSend = sessionRef.sessionPath;
  const sessionFileRefs = otherFiles
    .filter(f => f.fileId)
    .map(f => ({
      fileId: f.fileId,
      sessionId: sessionRef.sessionId,
      sessionPath: sessionPathForSend,
      label: f.name || f.path,
      kind: f.isDirectory ? 'directory' : 'attachment',
    }));

  let finalText = text;
  if (otherFiles.length > 0) {
    const fileBlock = otherFiles.map(f => {
      const label = f.fileId ? (f.name || f.path) : f.path;
      return f.isDirectory
        ? t('input.attachmentDirectory', { label })
        : t('input.attachmentFile', { label });
    }).join('\n');
    finalText = text ? `${text}\n\n${fileBlock}` : fileBlock;
  }

  // 图片 / 视频读 base64。统一走 platform 层：Electron 里 platform 代理到 hana，
  // Web/PWA 里 platform 代理到 HTTP fallback。
  const platform = window.platform;
  const images: Array<{ type: 'image'; data: string; mimeType: string }> = [];
  const videos: Array<{ type: 'video'; data: string; mimeType: string }> = [];
  const audios: Array<{ type: 'audio'; data: string; mimeType: string }> = [];
  const imageBase64Map = new Map<string, { base64Data: string; mimeType: string }>();
  const videoBase64Map = new Map<string, { base64Data: string; mimeType: string }>();
  const audioBase64Map = new Map<string, { base64Data: string; mimeType: string }>();
  // 单图读取失败同样不拦整条消息：该图退化为仅文件身份，显式提示（#1647）
  const imageFileOnlyPaths = new Set<string>();
  for (const img of imagesAsFileOnly ? [] : imageFiles) {
    try {
      if (img.base64Data && img.mimeType) {
        images.push({ type: 'image', data: img.base64Data, mimeType: img.mimeType });
      } else {
        const base64 = await platform?.readFileBase64?.(img.path);
        if (base64) {
          const mimeType = chatImageMimeTypeForName(img.name, img.mimeType);
          imageBase64Map.set(img.path, { base64Data: base64, mimeType });
          images.push({ type: 'image', data: base64, mimeType });
        } else {
          throw new Error(`failed to read image attachment: ${img.path}`);
        }
      }
    } catch (err) {
      console.warn('[input] failed to read image attachment', err);
      imageFileOnlyPaths.add(img.path);
      useStore.getState().addToast(t('input.imageReadFailedSentAsFile'), 'warning', 6000, {
        dedupeKey: `image-read-failed:${img.path}`,
      });
    }
  }
  for (const audio of sendAudiosNatively ? audioFiles : []) {
    try {
      if (audio.base64Data) {
        const mimeType = chatAudioMimeTypeForName(audio.name, audio.mimeType);
        audios.push({ type: 'audio', data: audio.base64Data, mimeType: mimeType });
      } else {
        const base64 = await platform?.readFileBase64?.(audio.path);
        if (base64) {
          const mimeType = chatAudioMimeTypeForName(audio.name, audio.mimeType);
          audioBase64Map.set(audio.path, { base64Data: base64, mimeType });
          audios.push({ type: 'audio', data: base64, mimeType });
        } else {
          throw new Error(`failed to read audio attachment: ${audio.path}`);
        }
      }
    } catch (err) {
      console.warn('[input] failed to read audio attachment', err);
      useStore.getState().addToast(t('input.audioReadFailed'), 'error', 6000, {
        dedupeKey: `audio-read-failed:${audio.path}`,
      });
      return { ok: false, result: { kind: 'failed_before_submit', code: 'audio_read_failed', retryable: true } };
    }
  }
  for (const video of sendVideosNatively ? videoFiles : []) {
    try {
      if (video.base64Data && video.mimeType) {
        const mimeType = chatVideoMimeTypeForName(video.name, video.mimeType);
        if (!isAllowedChatVideoMime(mimeType)
          || !isChatVideoBase64WithinLimit(video.base64Data)
          || !isChatVideoBase64ContentCompatible(video.base64Data, mimeType)) {
          throw new Error(`unsupported or oversized video: ${video.name}`);
        }
        videos.push({ type: 'video', data: video.base64Data, mimeType });
      } else {
        const base64 = await platform?.readFileBase64?.(video.path);
        if (base64) {
          const mimeType = chatVideoMimeTypeForName(video.name, video.mimeType);
          if (!isAllowedChatVideoMime(mimeType)
            || !isChatVideoBase64WithinLimit(base64)
            || !isChatVideoBase64ContentCompatible(base64, mimeType)) {
            throw new Error(`unsupported or oversized video: ${video.name}`);
          }
          videoBase64Map.set(video.path, { base64Data: base64, mimeType });
          videos.push({ type: 'video', data: base64, mimeType });
        } else {
          throw new Error(`failed to read video attachment: ${video.path}`);
        }
      }
    } catch (err) {
      console.warn('[input] failed to read video attachment', err);
      useStore.getState().addToast(t('input.videoReadFailed'), 'error', 6000, {
        dedupeKey: `video-read-failed:${video.path}`,
      });
      return { ok: false, result: { kind: 'failed_before_submit', code: 'video_read_failed', retryable: true } };
    }
  }

  // 文档上下文
  let docForRender: { path: string; name: string } | null = null;
  if (bundle.docContextAttached && bundle.doc) {
    finalText = finalText
      ? `${finalText}\n\n${t('input.referenceDocument', { path: bundle.doc.path })}`
      : t('input.referenceDocument', { path: bundle.doc.path });
    docForRender = bundle.doc;
  }

  // 引用片段
  const quotes = bundle.quotes;
  if (quotes.length > 0) {
    const quoteStr = quotes.map(formatQuotedSelectionForPrompt).join('\n\n');
    finalText = finalText ? `${finalText}\n\n${quoteStr}` : quoteStr;
  }

  const allFiles = [...inputFiles];
  if (docForRender) allFiles.push({ path: docForRender.path, name: docForRender.name });

  const clientMessageId = options.clientMessageId || createClientUserMessageId();
  const displayMessage = {
    text,
    skills: skills.length > 0 ? skills : undefined,
    quotedText: quotes.length > 0 ? quotes.map(q => (q as { text: string }).text).join('\n\n') : undefined,
    sessionRefs: sessionRefs.length > 0 ? sessionRefs : undefined,
    agentMentions: agentMentions.length > 0 ? agentMentions : undefined,
    // 消息投影用的知识库引用（含名称缓存，仅展示；功能字段走 wsMsg.knowledgeRefs）
    knowledgeRefs: bundle.knowledgeRefs && bundle.knowledgeRefs.notebookIds.length > 0
      ? {
        notebookIds: bundle.knowledgeRefs.notebookIds,
        mode: bundle.knowledgeRefs.mode,
        notebooks: bundle.knowledgeRefs.notebookIds.map(id => ({
          id,
          name: bundle.knowledgeRefs!.notebookNames[id],
        })),
      }
      : undefined,
    attachments: allFiles.length > 0 ? allFiles.map(f => {
      const cached = imageBase64Map.get(f.path);
      const cachedVideo = videoBase64Map.get(f.path);
      const cachedAudio = audioBase64Map.get(f.path);
      const imageFile = !f.isDirectory && isImageFile(f.name);
      return {
        fileId: f.fileId,
        path: f.path,
        name: f.name,
        isDir: !!f.isDirectory,
        mimeType: f.mimeType || cached?.mimeType || cachedVideo?.mimeType || cachedAudio?.mimeType || undefined,
        visionAuxiliary: imageFile && !supportsVision && !imagesAsFileOnly && !imageFileOnlyPaths.has(f.path),
        ...(f.waveform ? { waveform: f.waveform } : {}),
      };
    }) : undefined,
  };

  const optimisticMessage = {
    id: clientMessageId,
    role: 'user',
    text,
    textHtml: text ? renderMarkdown(text) : undefined,
    timestamp: Date.now(),
    attachments: displayMessage.attachments,
    quotedText: displayMessage.quotedText,
    skills: displayMessage.skills,
    knowledgeRefs: displayMessage.knowledgeRefs,
    sendStatus: 'pending',
    agentReview: agentMentions.length === 1 ? {
      status: 'running',
      reviewerAgentId: agentMentions[0].agentId,
      reviewerAgentName: agentMentions[0].label,
    } : undefined,
  };

  const wsMsg: Record<string, unknown> = {
    type: bundle.type,
    clientMessageId,
    text: finalText,
    sessionId: sessionRef.sessionId,
    sessionPath: sessionPathForSend,
    uiContext: bundle.uiContext,
    displayMessage,
  };
  if (sessionFileRefs.length > 0) wsMsg.sessionFileRefs = sessionFileRefs;
  if (images.length > 0) wsMsg.images = images;
  if (videos.length > 0) wsMsg.videos = videos;
  if (audios.length > 0) wsMsg.audios = audios;
  if (skills.length > 0) wsMsg.skills = skills;
  if (sessionRefs.length > 0) wsMsg.sessionRefs = sessionRefs;
  if (agentMentions.length > 0) wsMsg.agentReviewRequests = agentMentions;
  if (bundle.knowledgeRefs && bundle.knowledgeRefs.notebookIds.length > 0) {
    wsMsg.knowledgeRefs = {
      notebookIds: bundle.knowledgeRefs.notebookIds,
      mode: bundle.knowledgeRefs.mode,
    };
  }

  return {
    ok: true,
    prepared: {
      bundle,
      clientMessageId,
      sessionPathForSend,
      modelKey,
      finalText,
      displayMessage,
      wsMsg,
      optimisticMessage,
    },
  };
}

export interface ComposerCommitRuntime {
  t: ComposerSendOptions['t'];
  /**
   * 提交前的同步复核（租约/连接代次/会话身份/模型/目标 run）：返回非 null
   * 即拒绝提交——此时不得清理输入、不得创建乐观消息。由调用方（coordinator
   * 或薄封装）提供；缺省仅做 ws 可用性检查。
   */
  revalidate?: () => ComposerPreSubmitResult | null;
  /** 复核通过后、乐观消息创建前的同步钩子（InputArea 的输入清理）。 */
  onCommit?: () => void;
}

/**
 * 提交阶段：同步复核 → （可选）输入清理钩子 → 乐观消息 → ws 发送。
 * 复核与发送在同一个同步窗口内完成，期间没有 await，杜绝「准备完成到实际
 * 发送之间」的状态漂移。
 */
export async function commitPreparedComposerSend(
  prepared: PreparedComposerSend,
  runtime: ComposerCommitRuntime,
): Promise<ComposerCommitResult> {
  const sessionPathForSend = prepared.sessionPathForSend;
  const clientMessageId = prepared.clientMessageId;

  const rejection = runtime.revalidate?.() ?? null;
  if (rejection) return rejection;

  const ws = getWebSocket();
  if (!ws || (typeof ws.readyState === 'number' && ws.readyState !== WebSocket.OPEN)) {
    return { kind: 'failed_before_submit', code: 'websocket_unavailable', retryable: true };
  }

  // 复核通过：内容随派发落定，调用方做输入清理（草稿/附件/引用/编辑器）。
  runtime.onCommit?.();

  useStore.getState().appendOptimisticUserMessage(sessionPathForSend, prepared.optimisticMessage as never);
  try {
    ws.send(JSON.stringify(prepared.wsMsg));
    // 发送即进入「等待助手」态：服务器在知识检索/排队期间不置 isStreaming，
    // 本地先亮 typing 指示器，首个该 session 的后续事件（status / 流事件 /
    // error）到达即清（见 ws-message-handler 顶部保守清除）。仅限 prompt——
    // interject 发生在流式态中，指示器已由 isStreaming 覆盖。
    if (prepared.bundle.type === 'prompt') {
      useStore.getState().beginTurnPending?.(sessionPathForSend);
      // 携带知识库引用的提问：发送瞬间本地点亮「知识库检索中」——服务器在
      // 检索/蒸馏期间不发任何流事件，此前这段时间只剩裸三点指示器（用户
      // 完全看不到动作）。本地置位与服务器 knowledge_retrieval_started 幂等
      // 合流，清除沿用顶部保守清除。
      if (prepared.wsMsg.knowledgeRefs) {
        useStore.getState().beginKnowledgeRetrieval?.(sessionPathForSend);
      }
    }
    upsertOptimisticSessionFirstMessage(sessionPathForSend, prepared.bundle.text, new Date().toISOString());
    return { kind: 'transport_submitted', clientMessageId };
  } catch (err) {
    // 提交动作本身失败：乐观消息同卡标失败，记录保留完整快照供显式重试。
    console.warn('[input] websocket send failed', err);
    useStore.getState().markOptimisticUserMessageFailed(
      sessionPathForSend,
      clientMessageId,
      err instanceof Error ? err.message : String(err),
    );
    return { kind: 'failed_before_submit', code: 'ws_send_threw', retryable: true };
  }
}

/**
 * 公开派发入口（判别联合结果）：普通发送/排队续发/立即插入共用
 * prepare → commit 两段式；coordinator 以租约语义包裹同一对阶段。
 */
export async function dispatchComposerSend(
  bundle: ComposerSendBundle,
  options: ComposerSendOptions,
): Promise<ComposerDispatchResult> {
  const prep = await prepareComposerSend(bundle, options);
  if (prep.ok === false) return prep.result;
  return commitPreparedComposerSend(prep.prepared, { t: options.t });
}
