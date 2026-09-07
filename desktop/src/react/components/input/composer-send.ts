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

function createClientUserMessageId(): string {
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

/**
 * 派发一条已准备好的消息：预检 → base64 读取 → finalText 拼装 → 乐观消息 → ws 发送。
 * 抛错时乐观消息已标记失败；排队续发方负责 toast 兜底。
 */
export async function dispatchComposerSend(
  bundle: ComposerSendBundle,
  options: ComposerSendOptions,
): Promise<void> {
  const { loadVisionAuxiliaryConfig, t } = options;
  const sessionRef = bundle.sessionRef;

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
    return;
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
    } else {
      notifyVideoSendBlockedByModel({
        t,
        addToast: useStore.getState().addToast,
        openSettings: () => openProviderModelSettings(currentModelInfo?.provider),
      });
    }
    return;
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
      return;
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
      return;
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

  const clientMessageId = createClientUserMessageId();
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

  useStore.getState().appendOptimisticUserMessage(sessionPathForSend, {
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
  });

  const ws = getWebSocket();
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
  if (!ws) {
    useStore.getState().markOptimisticUserMessageFailed(
      sessionPathForSend,
      clientMessageId,
      'websocket_unavailable',
    );
    return;
  }
  try {
    ws.send(JSON.stringify(wsMsg));
    // 发送即进入「等待助手」态：服务器在知识检索/排队期间不置 isStreaming，
    // 本地先亮 typing 指示器，首个该 session 的后续事件（status / 流事件 /
    // error）到达即清（见 ws-message-handler 顶部保守清除）。仅限 prompt——
    // interject 发生在流式态中，指示器已由 isStreaming 覆盖。
    if (bundle.type === 'prompt') {
      useStore.getState().beginTurnPending?.(sessionPathForSend);
      // 携带知识库引用的提问：发送瞬间本地点亮「知识库检索中」——服务器在
      // 检索/蒸馏期间不发任何流事件，此前这段时间只剩裸三点指示器（用户
      // 完全看不到动作）。本地置位与服务器 knowledge_retrieval_started 幂等
      // 合流，清除沿用顶部保守清除。
      if (wsMsg.knowledgeRefs) {
        useStore.getState().beginKnowledgeRetrieval?.(sessionPathForSend);
      }
    }
    upsertOptimisticSessionFirstMessage(sessionPathForSend, text, new Date().toISOString());
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    useStore.getState().markOptimisticUserMessageFailed(sessionPathForSend, clientMessageId, message);
    throw err;
  }
}
