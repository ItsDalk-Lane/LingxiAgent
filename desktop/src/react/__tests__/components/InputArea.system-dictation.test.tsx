// @vitest-environment jsdom

/**
 * F7 前端入口移除守卫（用户 2026-09-08 指示：输入框不提供话筒/听写入口）。
 *
 * 后端能力链全部保留（TTS / ASR / 授权桥 / 识别核心，供其他界面与调用方使用），
 * 本文件只守卫两件事：
 *  1. Composer 工具栏不渲染任何话筒按钮，也不做后端听写探测；
 *  2. 语音消息老路径（快捷键 Cmd+Shift+M，仅原生音频模型）保持原样——即使
 *     localStorage 残留旧听写开关，录音也只会走语音附件发送，绝不进转写分支。
 */

import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { InputArea } from '../../components/InputArea';
import { useStore } from '../../stores';
import { resetComposerSendCoordinatorForTests } from '../../services/composer-send-coordinator';

const mocks = vi.hoisted(() => ({
  clearContent: vi.fn(),
  insertContent: vi.fn(),
  editorFocus: vi.fn(),
  ensureSession: vi.fn(),
  lingxiFetch: vi.fn(),
  upsertOptimisticSessionFirstMessage: vi.fn(),
  wsSend: vi.fn(),
}));

vi.mock('@tiptap/react', () => ({
  useEditor: () => {
    const chain: Record<string, unknown> = {};
    chain.clearContent = vi.fn(() => chain);
    chain.deleteRange = vi.fn(() => chain);
    chain.insertContent = vi.fn(() => chain);
    chain.focus = vi.fn(() => chain);
    chain.run = vi.fn();
    return {
      commands: {
        focus: mocks.editorFocus,
        clearContent: mocks.clearContent,
        scrollIntoView: vi.fn(),
        setContent: vi.fn(),
        insertContent: mocks.insertContent,
      },
      chain: () => chain,
      getText: () => '',
      getJSON: () => ({ type: 'doc', content: [] }),
      state: { tr: { setMeta: vi.fn(() => ({})) } },
      view: { dispatch: vi.fn() },
      on: vi.fn(),
      off: vi.fn(),
      isDestroyed: false,
    };
  },
  EditorContent: () => React.createElement('div', { 'data-testid': 'editor' }),
}));

vi.mock('@tiptap/starter-kit', () => ({
  default: { configure: () => ({}) },
}));

vi.mock('@tiptap/extension-placeholder', () => ({
  default: { configure: () => ({}) },
}));

vi.mock('../../components/input/extensions/skill-badge', () => ({
  SkillBadge: {},
}));

vi.mock('../../components/input/extensions/file-badge', () => ({
  FileBadge: {},
}));

import { createTestTranslator } from '../helpers/i18n-test-strings';

const testT = createTestTranslator();

vi.mock('../../hooks/use-i18n', () => ({
  useI18n: () => ({ t: testT }),
}));

vi.mock('../../hooks/use-config', () => ({
  fetchConfig: vi.fn(async () => ({})),
}));

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: (path: string, opts?: RequestInit) => mocks.lingxiFetch(path, opts),
  lingxiUrl: (path: string) => `http://127.0.0.1:3210${path}`,
}));

vi.mock('../../stores/session-actions', () => ({
  ensureSession: mocks.ensureSession,
  loadSessions: vi.fn(),
  upsertOptimisticSessionFirstMessage: mocks.upsertOptimisticSessionFirstMessage,
}));

vi.mock('../../stores/desk-actions', () => ({
  loadDeskFiles: vi.fn(),
  searchDeskFiles: vi.fn(async () => []),
  toggleJianSidebar: vi.fn(),
}));

const wsInstance = { send: mocks.wsSend } as unknown as WebSocket;

vi.mock('../../services/websocket', () => ({
  getWebSocket: () => wsInstance,
}));

vi.mock('../../MainContent', () => ({
  attachFilesFromPaths: vi.fn(),
}));

vi.mock('../../components/input/SlashCommandMenu', () => ({
  SlashCommandMenu: () => null,
}));

vi.mock('../../components/input/FileMentionMenu', () => ({
  FileMentionMenu: () => null,
}));

vi.mock('../../components/input/InputStatusBars', () => ({
  InputStatusBars: () => null,
}));

vi.mock('../../components/input/InputContextRow', () => ({
  InputContextRow: () => null,
}));

vi.mock('../../components/input/ModelSelector', () => ({
  ModelSelector: () => null,
}));

vi.mock('../../components/input/ThinkingLevelButton', () => ({
  ThinkingLevelButton: () => null,
}));

vi.mock('../../components/input/ContextRing', () => ({
  ContextRing: () => null,
}));

vi.mock('../../components/input/PlanModeButton', () => ({
  PlanModeButton: () => null,
}));

vi.mock('../../components/input/KnowledgeReferenceButton', () => ({
  KnowledgeReferenceButton: () => null,
}));

vi.mock('../../services/stream-resume', () => ({
  replayStreamResume: vi.fn(),
  isStreamResumeRebuilding: () => null,
  isStreamScopedMessage: () => false,
  updateSessionStreamMeta: vi.fn(),
}));

vi.mock('../../utils/paste-upload-feedback', () => ({
  notifyPasteUploadFailure: vi.fn(),
}));

type AudioProcessorHandle = {
  onaudioprocess: ((event: { inputBuffer: { getChannelData: () => Float32Array } }) => void) | null;
  connect?: unknown;
  disconnect?: unknown;
};

function installAudioCaptureMocks() {
  let processor: AudioProcessorHandle | null = null;
  const stopTrack = vi.fn();
  const stream = {
    getTracks: vi.fn(() => [{ stop: stopTrack }]),
  };
  class AudioContextMock {
    sampleRate = 24000;
    state = 'running';
    destination = {};
    createMediaStreamSource = vi.fn(() => ({ connect: vi.fn(), disconnect: vi.fn() }));
    createScriptProcessor = vi.fn(() => {
      processor = {
        onaudioprocess: null,
        connect: vi.fn(),
        disconnect: vi.fn(),
      };
      return processor;
    });
    createGain = vi.fn(() => ({ gain: { value: 0 }, connect: vi.fn(), disconnect: vi.fn() }));
    close = vi.fn(async () => {});
  }
  Object.defineProperty(navigator, 'mediaDevices', {
    configurable: true,
    value: { getUserMedia: vi.fn(async () => stream) },
  });
  vi.stubGlobal('AudioContext', AudioContextMock);
  return {
    get processor() {
      return processor;
    },
    stopTrack,
  };
}

function pushAudioChunk(capture: ReturnType<typeof installAudioCaptureMocks>) {
  expect(capture.processor).toBeTruthy();
  act(() => {
    capture.processor!.onaudioprocess?.({
      inputBuffer: { getChannelData: () => new Float32Array([0.4, -0.4, 0.25, -0.25]) },
    });
  });
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status });
}

const TEXT_ONLY_MODEL = {
  id: 'deepseek-chat',
  provider: 'deepseek',
  name: 'DeepSeek Chat',
  input: ['text'],
  isCurrent: true,
};

const NATIVE_AUDIO_MODEL = {
  id: 'mimo-v2.5',
  provider: 'mimo',
  name: 'MiMo V2.5',
  api: 'openai-completions',
  baseUrl: 'https://api.xiaomimimo.com/v1',
  audio: true,
  audioTransport: 'mimo-input-audio',
  audioTransportSupported: true,
  input: ['text'],
  isCurrent: true,
};

const fetchCalls: string[] = [];

function seedSession(models: unknown[] = [TEXT_ONLY_MODEL]) {
  useStore.setState({
    currentSessionPath: '/session/media.jsonl',
    currentSessionId: 'sess_media',
    currentAgentId: 'hana',
    sessions: [{
      path: '/session/media.jsonl',
      sessionId: 'sess_media',
      agentId: 'hana',
      agentName: 'Hana',
    }],
    sessionLocatorsById: { sess_media: { path: '/session/media.jsonl' } },
    connected: true,
    pendingNewSession: false,
    streamingSessions: [],
    turnPendingSessions: [],
    knowledgeRetrievingSessions: [],
    inlineErrors: {},
    toasts: [],
    attachedFiles: [],
    attachedFilesBySession: {},
    docContextAttached: false,
    quoteCandidate: null,
    quotedSelections: [],
    quotedSelection: null,
    models,
    sessionModelsByPath: {},
    previewItems: [],
    previewOpen: false,
    chatSessions: {},
    serverPort: 3210,
    serverToken: null,
    modelSwitching: false,
    currentTab: 'chat',
    settingsModal: { open: false, activeTab: 'agent' },
    mediaViewer: null,
    skillViewerData: null,
    channelCreateOverlayVisible: false,
  } as never);
  useStore.getState().clearSession('/session/media.jsonl');
  useStore.getState().initSession('/session/media.jsonl', [], false);
}

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.removeItem('hana-dictate-mode');
  resetComposerSendCoordinatorForTests();
  fetchCalls.length = 0;
  mocks.ensureSession.mockResolvedValue({
    sessionId: 'sess_media',
    sessionPath: '/session/media.jsonl',
    agentId: 'hana',
  });
  mocks.lingxiFetch.mockImplementation(async (path: string) => {
    fetchCalls.push(path);
    if (path === '/api/upload-blob') {
      return jsonResponse({
        uploads: [{
          fileId: 'sf_rec_1',
          dest: '/tmp/hana/session-files/rec1.wav',
          name: '录音 1.wav',
        }],
      });
    }
    if (path === '/api/preferences/models') {
      return jsonResponse({
        models: {
          vision_enabled: false,
          vision: { id: 'qwen-vl', provider: 'dashscope', input: ['text', 'image'] },
        },
      });
    }
    return jsonResponse({});
  });
  window.platform = {
    readFileBase64: vi.fn(async () => 'AUDIO_BASE64'),
  } as unknown as typeof window.platform;
  (window as unknown as { hana?: unknown }).hana = {};
  seedSession();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  delete (window as unknown as { hana?: unknown }).hana;
});

describe('输入框无话筒/听写入口（F7 前端入口移除守卫）', () => {
  it('纯文字模型：不渲染话筒按钮、不做后端探测、快捷键不启动录音', async () => {
    const capture = installAudioCaptureMocks();
    vi.spyOn(document, 'hasFocus').mockReturnValue(true);
    render(React.createElement(InputArea));

    expect(screen.queryByTestId('voice-toggle')).toBeNull();
    expect(screen.queryByTestId('dictate-toggle')).toBeNull();
    // 输入框侧不再探测 ASR 供应商（探测属于其他调用方）
    await act(async () => { await new Promise((r) => setTimeout(r, 20)); });
    expect(fetchCalls.some((p) => p.startsWith('/api/media/providers'))).toBe(false);

    fireEvent.keyDown(window, { key: 'm', metaKey: true, shiftKey: true });
    await act(async () => { await new Promise((r) => setTimeout(r, 20)); });
    expect(navigator.mediaDevices.getUserMedia).not.toHaveBeenCalled();
    expect(capture.processor).toBeNull();
  });

  it('原生音频模型：仍无话筒按钮；快捷键录音只走语音附件发送，残留听写开关也不进转写', async () => {
    const capture = installAudioCaptureMocks();
    vi.spyOn(document, 'hasFocus').mockReturnValue(true);
    // 模拟旧版本残留的听写开关：即使为 on，也不得把录音变成转写
    localStorage.setItem('hana-dictate-mode', '1');
    seedSession([NATIVE_AUDIO_MODEL]);
    render(React.createElement(InputArea));

    expect(screen.queryByTestId('voice-toggle')).toBeNull();
    expect(screen.queryByTestId('dictate-toggle')).toBeNull();

    fireEvent.keyDown(window, { key: 'm', metaKey: true, shiftKey: true });
    await waitFor(() => {
      expect(navigator.mediaDevices.getUserMedia).toHaveBeenCalledTimes(1);
    });
    // 等录音态真正建立（runtime 已登记、卡片进入 recording 标题）再灌音频块
    await screen.findByText('input.audioRecording');
    pushAudioChunk(capture);
    fireEvent.keyDown(window, { key: 'm', metaKey: true, shiftKey: true });

    await waitFor(() => {
      expect(mocks.wsSend).toHaveBeenCalledTimes(1);
    });
    const payload = JSON.parse(String(mocks.wsSend.mock.calls[0][0]));
    expect(payload.audios).toEqual([{
      type: 'audio',
      data: expect.any(String),
      mimeType: 'audio/wav',
    }]);
    // 绝不调用 ASR 转写入口，也不向编辑器插入任何转写文本
    expect(fetchCalls).not.toContain('/api/media/asr/transcribe');
    expect(mocks.insertContent).not.toHaveBeenCalled();
  });
});
