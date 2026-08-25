// @vitest-environment jsdom

/**
 * Composer 布局重构（任务十三~十六）的 InputArea 级结构验收：
 * - 输入卡片（.input-wrapper）内只保留编辑区与 SendButton；
 * - Composer 工具栏在卡片下方、仍在 .input-stack / .input-surface 内；
 * - 新建聊天/附件/Slash 等控件移出卡片但回调不变（由 ComposerToolbar.test.tsx 覆盖）。
 */

import { cleanup, render, screen } from '@testing-library/react';
import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { InputArea } from '../../components/InputArea';
import { useStore } from '../../stores';
import inputStyles from '../../components/input/InputArea.module.css';

const cardSelector = `.${inputStyles['input-wrapper']}`;
const stackSelector = `.${inputStyles['input-stack']}`;
const surfaceSelector = `.${inputStyles['input-surface']}`;

vi.mock('@tiptap/react', () => ({
  useEditor: () => ({
    commands: {
      focus: vi.fn(),
      clearContent: vi.fn(),
      scrollIntoView: vi.fn(),
      setContent: vi.fn(),
      insertContent: vi.fn(),
    },
    chain: () => ({
      clearContent: () => ({
        insertContent: () => ({
          insertContent: () => ({
            focus: () => ({ run: vi.fn() }),
          }),
        }),
      }),
    }),
    getText: () => '',
    getJSON: () => ({ type: 'doc', content: [] }),
    state: { tr: { setMeta: vi.fn(() => ({})) } },
    view: { dispatch: vi.fn() },
    on: vi.fn(),
    off: vi.fn(),
  }),
  EditorContent: () => React.createElement('div', { 'data-testid': 'editor' }),
}));

vi.mock('@tiptap/starter-kit', () => ({
  default: { configure: () => ({}) },
}));

vi.mock('@tiptap/extension-bold', () => ({
  Bold: { extend: () => ({}) },
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

vi.mock('../../hooks/use-i18n', () => ({
  useI18n: () => ({ t: (key: string) => key, locale: 'zh-CN' }),
}));

vi.mock('../../hooks/use-config', () => ({
  fetchConfig: vi.fn(async () => ({})),
}));

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: vi.fn(async () => new Response('{}', { status: 200 })),
  lingxiUrl: (path: string) => `http://127.0.0.1:3210${path}`,
}));

vi.mock('../../stores/session-actions', () => ({
  ensureSession: vi.fn(async () => true),
  loadSessions: vi.fn(),
  createNewSession: vi.fn(async () => undefined),
  continueDeletedAgentSession: vi.fn(async () => true),
}));

vi.mock('../../stores/desk-actions', () => ({
  loadDeskFiles: vi.fn(),
  revealDeskDirectory: vi.fn(async () => true),
  searchDeskFiles: vi.fn(async () => []),
  toggleJianSidebar: vi.fn(),
}));

vi.mock('../../services/websocket', () => ({
  getWebSocket: vi.fn(() => null),
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

vi.mock('../../components/input/ComposerToolbar', () => ({
  ComposerToolbar: () => React.createElement('div', { 'data-testid': 'composer-toolbar' }),
}));

vi.mock('../../components/input/SendButton', () => ({
  SendButton: () => React.createElement('button', { type: 'button', 'data-testid': 'send-button' }, 'send'),
}));

vi.mock('../../components/input/SessionConfirmationPrompt', () => ({
  SessionConfirmationPrompt: () => null,
}));

vi.mock('../../hooks/use-slash-items', () => ({
  useSkillSlashItems: () => [],
  useServerSlashCommandItems: () => [],
}));

vi.mock('../../utils/paste-upload-feedback', () => ({
  notifyPasteUploadFailure: vi.fn(),
}));

vi.mock('../../services/stream-resume', () => ({
  replayStreamResume: vi.fn(),
  isStreamResumeRebuilding: () => null,
  isStreamScopedMessage: () => false,
  updateSessionStreamMeta: vi.fn(),
}));

function seedInputState() {
  useStore.setState({
    currentSessionPath: '/session/composer-layout.jsonl',
    connected: true,
    pendingNewSession: false,
    pendingSessionSwitchPath: null,
    streamingSessions: [],
    compactingSessions: [],
    inlineErrors: {},
    screenshotTaskCount: 0,
    screenshotProgress: null,
    attachedFiles: [],
    docContextAttached: false,
    quoteCandidate: null,
    quotedSelections: [],
    quotedSelection: null,
    models: [{
      id: 'deepseek-chat',
      provider: 'deepseek',
      name: 'DeepSeek Chat',
      input: ['text'],
      isCurrent: true,
    }],
    sessionModelsByPath: {},
    previewItems: [],
    previewOpen: false,
    activeTabId: null,
    chatSessions: {},
    serverPort: 3210,
    serverToken: null,
    modelSwitching: false,
    welcomeVisible: false,
    agentYuan: 'lingxi',
  } as never);
}

describe('InputArea composer layout', () => {
  beforeEach(() => {
    seedInputState();
    window.platform = {} as typeof window.platform;
    delete (window as unknown as { hana?: unknown }).hana;
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('keeps only the editor and the send control inside the input card', () => {
    const { container } = render(React.createElement(InputArea));

    const card = container.querySelector(cardSelector);
    expect(card).not.toBeNull();
    expect(card!.contains(screen.getByTestId('editor'))).toBe(true);
    expect(card!.contains(screen.getByTestId('send-button'))).toBe(true);
    // 工具栏已移出输入卡片
    expect(card!.contains(screen.getByTestId('composer-toolbar'))).toBe(false);
  });

  it('places the composer toolbar below the card but still inside the input stack/surface', () => {
    const { container } = render(React.createElement(InputArea));

    const surface = container.querySelector(surfaceSelector);
    const stack = container.querySelector(stackSelector);
    const card = container.querySelector(cardSelector);
    const toolbar = screen.getByTestId('composer-toolbar');

    expect(surface).not.toBeNull();
    expect(stack).not.toBeNull();
    expect(card).not.toBeNull();
    expect(stack!.contains(toolbar)).toBe(true);
    expect(surface!.contains(toolbar)).toBe(true);
    // 工具栏在卡片之后（卡片下方）
    expect(card!.compareDocumentPosition(toolbar) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});
