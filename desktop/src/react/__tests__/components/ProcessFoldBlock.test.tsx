// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import fs from 'node:fs';
import path from 'node:path';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ChatTranscript } from '../../components/chat/ChatTranscript';
import { useStore } from '../../stores';
import type { ChatListItem, ContentBlock, ToolCall } from '../../stores/chat-types';
import { navigateToChatCard } from '../../services/chat-card-navigation';

const sessionPath = '/session/process-fold.jsonl';
const retryMock = vi.fn(async (..._args: unknown[]) => true);

vi.mock('../../stores/message-turn-actions', () => ({
  retrySessionTurn: (...args: unknown[]) => retryMock(...args),
  forkSessionTurn: vi.fn(async () => null),
  activateForkedSession: vi.fn(async () => undefined),
}));

function t(key: string, vars?: Record<string, string | number>): string {
  const table: Record<string, string> = {
    'thinking.done': '思考完成',
    'thinking.active': '思考中',
    'toolGroup.count': '{n} 个工具',
    'toolGroup.countWithFail': '{total} 个工具（{fail} 个失败）',
    'toolGroup.running': '{n} 个工具运行中',
    'tool._fallback.done': '小花 忙完了',
    'tool._fallback.running': '小花 忙着',
    'processFold.summary': '✨ {name}忙活了一阵子',
    'processFold.tools': '{n} 个工具',
    'processFold.thinking': '{n} 次思考',
    'processFold.unsuccessful': '{n} 次尝试未成功',
    'common.regenerate': '重新生成',
    'common.forkSession': '分支为新会话',
  };
  return (table[key] || key).replace(/\{(\w+)\}/g, (_, name) => String(vars?.[name] ?? ''));
}

function user(id: string): ChatListItem {
  return { type: 'message', data: { id, sourceEntryId: `entry-${id}`, role: 'user', text: '做一下' } };
}

function assistant(id: string, blocks: ContentBlock[]): ChatListItem {
  return { type: 'message', data: { id, sourceEntryId: `entry-${id}`, role: 'assistant', blocks } };
}

function thinking(content = '过程思考'): ContentBlock {
  return { type: 'thinking', content, sealed: true };
}

function tool(name: string, success = true): ToolCall {
  return { name, args: { command: name }, done: true, success };
}

function toolGroup(tools: ToolCall[]): ContentBlock {
  return { type: 'tool_group', tools, collapsed: false };
}

function textBlock(html: string, source: string): ContentBlock {
  return { type: 'text', html, source };
}

function processTextBlock(html: string, source: string): ContentBlock {
  return {
    ...textBlock(html, source),
    semanticPhase: 'commentary',
    surfaceRole: 'process',
    lifecycle: 'sealed',
  };
}

describe('ProcessFoldBlock', () => {
  beforeEach(() => {
    window.t = t as typeof window.t;
    useStore.setState({
      agents: [],
      agentName: '小花',
      agentYuan: 'lingxi',
      streamingSessions: [],
      selectedIdsBySession: {},
      currentSessionId: null,
      sessions: [],
      sessionLocatorsById: {},
      chatSessions: {
        [sessionPath]: {
          hasMore: false,
          loadingMore: false,
          items: [],
        },
      },
      currentSessionPath: sessionPath,
      terminalsBySession: {},
    } as never);
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', {
      configurable: true,
      value: vi.fn(),
    });
    window.requestAnimationFrame = ((callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    }) as typeof window.requestAnimationFrame;
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('exposes retry and fork on a collapsed process-only turn completion', () => {
    render(
      <ChatTranscript
        items={[
          user('u1'),
          assistant('a1', [thinking('第一步'), toolGroup([tool('read')])]),
          assistant('a2', [thinking('第二步'), toolGroup([tool('write')])]),
          assistant('a3', [thinking('第三步'), toolGroup([tool('verify')])]),
        ]}
        sessionPath={sessionPath}
        enableProcessFold
      />,
    );

    const footer = screen.getByTestId('process-fold-completion-actions');
    expect(within(footer).getByTitle('重新生成')).toBeInTheDocument();
    expect(within(footer).getByTitle('分支为新会话')).toBeInTheDocument();
    fireEvent.click(within(footer).getByTitle('重新生成'));

    expect(retryMock).toHaveBeenCalledWith(
      sessionPath,
      { role: 'assistant', entryId: 'entry-a3' },
      { message: expect.objectContaining({ id: 'u1', sourceEntryId: 'entry-u1', text: '做一下' }) },
    );
  });

  it('keeps turn completion actions on the visible answer when one message is split', () => {
    const processBlock: ContentBlock = {
      id: 'entry-a1:thinking',
      type: 'thinking',
      content: '先检查一下',
      sealed: true,
      semanticPhase: 'reasoning',
      surfaceRole: 'process',
      lifecycle: 'sealed',
    };
    const answerBlock: ContentBlock = {
      id: 'entry-a1:answer',
      type: 'text',
      html: '<p>已经完成。</p>',
      source: '已经完成。',
      semanticPhase: 'final_answer',
      surfaceRole: 'answer',
      lifecycle: 'sealed',
    };
    const turn = assistant('a1', [processBlock, answerBlock]);
    if (turn.type !== 'message') throw new Error('expected assistant');
    turn.data.turnInputEntryId = 'entry-u1';
    turn.data.turnProjection = {
      id: 'entry-a1:turn',
      inputMessageId: 'entry-u1',
      assistantMessageIds: ['entry-a1'],
      processBlockIds: ['entry-a1:thinking'],
      answerBlockIds: ['entry-a1:answer'],
      resultBlockIds: [],
      controlBlockIds: [],
      status: 'completed',
    };

    render(
      <ChatTranscript
        items={[user('u1'), turn]}
        sessionPath={sessionPath}
        enableProcessFold
      />,
    );

    const processButton = screen.getByRole('button', { name: /小花忙活了一阵子/ });
    const processGroup = processButton.closest('[data-process-group-id]');
    if (!(processGroup instanceof HTMLElement)) throw new Error('expected process group');
    expect(within(processGroup).queryByTitle('重新生成')).not.toBeInTheDocument();
    expect(screen.getAllByTestId('assistant-completion-actions')).toHaveLength(1);
    fireEvent.click(processButton);
    expect(within(processGroup).queryByTitle('重新生成')).not.toBeInTheDocument();
    expect(screen.getAllByTestId('assistant-completion-actions')).toHaveLength(1);
    expect(screen.getByText('已经完成。')).toBeInTheDocument();
  });

  it('collapses process-only assistant runs and expands original blocks in place', () => {
    const items: ChatListItem[] = [
      user('u1'),
      assistant('a1', [
        thinking('第一段思考'),
        processTextBlock('<p>现在开始执行。</p>', '现在开始执行。'),
        toolGroup([tool('npm test')]),
      ]),
      assistant('a2', [
        thinking('第二段思考'),
        processTextBlock('<p>第二步：继续读文件。</p>', '第二步：继续读文件。'),
        toolGroup([tool('read'), tool('write', false)]),
      ]),
      assistant('a3', [
        thinking('第三段思考'),
        processTextBlock('<p>第三步：核对结果。</p>', '第三步：核对结果。'),
        toolGroup([tool('verify')]),
      ]),
      assistant('a4', [
        thinking('正文前思考'),
        { type: 'mood', yuan: 'butter', text: 'PULSE' },
        { type: 'text', html: '<p>正文来了</p>' },
      ]),
    ];

    render(
      <ChatTranscript
        items={items}
        sessionPath={sessionPath}
        enableProcessFold
      />,
    );

    const summary = screen.getByRole('button', {
      name: '✨ 小花忙活了一阵子 · 4 个工具 · 4 次思考 · 1 次尝试未成功',
    });
    expect(summary).toBeInTheDocument();
    expect(summary).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByText('第一段思考')).not.toBeInTheDocument();
    expect(screen.queryByText('现在开始执行。')).not.toBeInTheDocument();
    expect(screen.queryByText('npm test')).not.toBeInTheDocument();
    expect(screen.getByText('正文来了')).toBeInTheDocument();
    expect(screen.queryByText('思考完成')).not.toBeInTheDocument();
    expect(screen.queryByText(/PULSE/)).not.toBeInTheDocument();

    fireEvent.click(summary);

    expect(summary).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByText('npm test')).toBeInTheDocument();
    expect(screen.getByText('现在开始执行。')).toBeInTheDocument();
    expect(screen.getAllByText('思考完成')).toHaveLength(4);
    expect(screen.getByText(/PULSE/)).toBeInTheDocument();
  });

  it('终端定位先展开所属过程组，再由挂载后的命令卡完成精确定位', async () => {
    const execBlock: ContentBlock = {
      id: 'entry-a1:exec',
      type: 'tool_group',
      tools: [{
        id: 'call-exec',
        name: 'exec_command',
        args: { cmd: 'npm run dev' },
        done: true,
        success: true,
        status: 'succeeded',
        details: { execCommand: { terminalId: 'term-1' } },
      }],
      collapsed: false,
      semanticPhase: 'tool',
      surfaceRole: 'process',
      lifecycle: 'sealed',
    };
    const answer: ContentBlock = {
      id: 'entry-a1:answer',
      type: 'text',
      html: '<p>服务已启动。</p>',
      source: '服务已启动。',
      semanticPhase: 'final_answer',
      surfaceRole: 'answer',
      lifecycle: 'sealed',
    };
    const turn = assistant('a1', [execBlock, answer]);
    if (turn.type !== 'message') throw new Error('expected assistant');
    turn.data.turnInputEntryId = 'entry-u1';
    turn.data.turnProjection = {
      id: 'entry-a1:turn',
      inputMessageId: 'entry-u1',
      assistantMessageIds: ['entry-a1'],
      processBlockIds: ['entry-a1:exec'],
      answerBlockIds: ['entry-a1:answer'],
      resultBlockIds: [],
      controlBlockIds: [],
      status: 'completed',
    };
    useStore.setState({
      terminalsBySession: {
        [sessionPath]: [{
          terminalId: 'term-1',
          toolCallId: 'call-exec',
          sessionId: null,
          sessionPath,
          agentId: 'hana',
          cwd: '/workspace',
          command: 'npm run dev',
          label: 'npm run dev',
          status: 'running',
          seq: 2,
          createdAt: 1,
          lastActivityAt: 2,
          exitedAt: null,
          exitCode: null,
          signal: null,
          transcriptPath: '/state/term-1.jsonl',
        }],
      },
    } as never);

    render(
      <ChatTranscript
        items={[user('u1'), turn]}
        sessionPath={sessionPath}
        enableProcessFold
      />,
    );

    const processButton = screen.getByRole('button', { name: /小花忙活了一阵子/ });
    expect(processButton).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('button', { name: 'npm run dev' })).toBeNull();
    expect(useStore.getState().terminalsBySession[sessionPath]?.[0]?.status).toBe('running');

    act(() => {
      navigateToChatCard({ kind: 'terminal', ids: ['call-exec', 'term-1'], sessionPath });
    });

    await waitFor(() => expect(processButton).toHaveAttribute('aria-expanded', 'true'));
    const execButton = await screen.findByRole('button', { name: 'npm run dev' });
    await waitFor(() => expect(execButton).toHaveAttribute('aria-expanded', 'true'));
    expect(HTMLElement.prototype.scrollIntoView).toHaveBeenCalled();
    expect(useStore.getState().terminalsBySession[sessionPath]?.[0]?.status).toBe('running');
  });

  it('keeps the process-fold Collapse shell full width inside the assistant flex column', () => {
    const source = fs.readFileSync(
      path.join(process.cwd(), 'desktop/src/react/components/chat/ProcessFoldBlock.tsx'),
      'utf8',
    );
    const css = fs.readFileSync(
      path.join(process.cwd(), 'desktop/src/react/components/chat/Chat.module.css'),
      'utf8',
    );
    const processFoldCollapseRule = css.match(/\.processFoldCollapse\s*\{(?<body>[^}]*)\}/)?.groups?.body || '';
    const processFoldMessageRule = css.match(/\.processFoldPanel \.message\s*\{(?<body>[^}]*)\}/)?.groups?.body || '';

    expect(source).toContain('className={styles.processFoldCollapse}');
    expect(processFoldCollapseRule).toContain('width: 100%');
    expect(processFoldCollapseRule).toContain('box-sizing: border-box');
    expect(processFoldMessageRule).toContain('width: 100%');
    expect(processFoldMessageRule).toContain('max-width: 100%');
  });
});
