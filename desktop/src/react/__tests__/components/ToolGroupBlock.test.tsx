// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import fs from 'node:fs';
import path from 'node:path';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ToolGroupBlock } from '../../components/chat/ToolGroupBlock';
import { useStore } from '../../stores';

/** 载入真实语言包当 window.t：桩函数返回键名时，"标签是不是文案词"根本测不出来。 */
function useRealLocale() {
  const zh = JSON.parse(fs.readFileSync(path.join(process.cwd(), 'desktop/src/locales/zh.json'), 'utf8'));
  window.t = ((key: string, vars: Record<string, string> = {}) => {
    const value = key.split('.').reduce((node, name) => node?.[name], zh);
    return (typeof value === 'string' ? value : key).replace(/\{(\w+)\}/g, (match, name) => vars[name] ?? match);
  }) as typeof window.t;
}

/** 行主标签那一格（渲染在 label span 的 data-label 上）。 */
function labelSpan(toolName: string): HTMLElement {
  const row = document.querySelector(`[data-tool="${toolName}"]`);
  expect(row, `没渲染出 ${toolName} 的工具行`).toBeInTheDocument();
  const span = row!.querySelector('[data-label]');
  expect(span, `${toolName} 的行没有主标签格`).toBeTruthy();
  return span as HTMLElement;
}

/** 行上那格摘要（标签右侧的正文）。 */
function summarySpan(toolName: string): HTMLElement {
  const row = document.querySelector(`[data-tool="${toolName}"]`);
  expect(row, `没渲染出 ${toolName} 的工具行`).toBeInTheDocument();
  const span = row!.querySelector('[class*="summary"]');
  expect(span, `${toolName} 的行没有摘要格`).toBeTruthy();
  return span as HTMLElement;
}

/** 行上的悬停提示：短标签顶掉工具本名后，本名降级到这里。 */
function rowTitle(toolName: string): string | null {
  const row = document.querySelector(`[data-tool="${toolName}"]`);
  expect(row, `没渲染出 ${toolName} 的工具行`).toBeInTheDocument();
  return row!.querySelector('[role="button"]')?.getAttribute('title') ?? null;
}

describe('ToolGroupBlock', () => {
  beforeEach(() => {
    window.t = ((key: string) => key) as typeof window.t;
    useStore.setState({
      currentSessionId: 'sess-a',
      currentSessionPath: '/session/a.jsonl',
      terminalsBySession: {},
    } as never);
  });

  afterEach(() => {
    vi.useRealTimers();
    cleanup();
  });

  it('renders failed and unknown outcomes without presenting either as success', () => {
    render(
      <ToolGroupBlock
        collapsed={false}
        tools={[
          { id: 'failed', name: 'read', done: true, success: false, status: 'failed', error: 'file not found' },
          { id: 'unknown', name: 'read', done: true, success: false, status: 'unknown' },
        ]}
      />,
    );

    expect(screen.getByText('file not found')).toBeInTheDocument();
    expect(document.querySelector('[data-status="failed"]')).toBeInTheDocument();
    expect(document.querySelector('[data-status="unknown"]')).toBeInTheDocument();
    expect(screen.queryByText('✓')).not.toBeInTheDocument();
  });

  it('清单工具行显示统一标签与进度摘要（A21）', () => {
    useRealLocale();
    render(
      <ToolGroupBlock
        collapsed={false}
        tools={[
          {
            id: 'todo-1',
            name: 'todo_write',
            done: true,
            success: true,
            status: 'succeeded',
            details: {
              todoVersion: 2,
              todos: [
                { content: '读 spec', activeForm: '正在读 spec', status: 'completed' },
                { content: '改代码', activeForm: '正在改代码', status: 'completed' },
                { content: '检查兼容性', activeForm: '正在检查兼容性', status: 'in_progress' },
                { content: '补测试', activeForm: '正在补测试', status: 'pending' },
                { content: '写文档', activeForm: '正在写文档', status: 'pending' },
                { content: '清理', activeForm: '正在清理', status: 'pending' },
              ],
            },
          },
        ]}
      />,
    );

    // 统一消息行：标签「任务」+ 完成数量 + 当前步骤（A21）
    const row = document.querySelector('[data-tool="todo_write"]');
    expect(row).toBeInTheDocument();
    expect(row).toHaveTextContent('任务');
    expect(row).toHaveTextContent('已完成 2/6 · 正在检查兼容性');
  });

  it('本地检索完成后只显示一份证据数量与耗时', () => {
    render(<ToolGroupBlock collapsed={false} tools={[{ id: 'local', name: 'knowledge_local_search',
      done: true, success: true, status: 'succeeded', resultNote: '已找到 3 条证据 · 28ms' }]} />);
    expect(screen.getAllByText('已找到 3 条证据 · 28ms')).toHaveLength(1);
    expect(screen.queryByText('tool.knowledge_local_search.done')).not.toBeInTheDocument();
  });

  it('调查聚合卡显示本地化进度和任务短标签，不展示内部参数或隐藏推理', () => {
    useRealLocale();
    const { container } = render(<ToolGroupBlock collapsed={false} tools={[
      { id: 'progress', name: 'knowledge_research_progress', done: false, success: false,
        args: { completed: 2, total: 3, hiddenReasoning: '秘密推理正文', runId: 'internal-run' } },
      { id: 'worker', name: 'knowledge_research_worker', done: false, success: false,
        args: { count: 1, label: '核对两份预算', rawToolResult: { text: '内部工具正文' } } },
    ]} />);
    // 短标签只占主标签格，进度措辞落在摘要格——两处不重复同一句话
    expect(labelSpan('knowledge_research_progress').textContent).toBe('调查');
    expect(summarySpan('knowledge_research_progress')).toHaveTextContent('已完成 2/3 个证据问题');
    expect(summarySpan('knowledge_research_worker')).toHaveTextContent('已派出 1 个调查 Agent');
    expect(summarySpan('knowledge_research_worker')).toHaveTextContent('核对两份预算');
    expect(container.textContent).not.toContain('秘密推理正文');
    expect(container.textContent).not.toContain('内部工具正文');
    expect(container.textContent).not.toContain('internal-run');
  });

  it('shows the full bash command in the hover title when the visible detail is truncated', () => {
    const command = 'rm -rf /Users/jason/.claude/plugins/marketplaces/temp_*';

    render(
      <ToolGroupBlock
        collapsed={false}
        tools={[{
          name: 'bash',
          args: { command },
          done: true,
          success: true,
        }]}
      />,
    );

    const detail = screen.getByTitle(command);

    expect(detail.textContent).toBe(command);
  });

  it('renders exec_command with the legacy bash user-facing copy', () => {
    window.t = ((key: string, vars?: Record<string, unknown>) => {
      if (key === 'tool.bash.done') return `💻 ${vars?.name} 用完电脑了`;
      return key;
    }) as typeof window.t;

    render(
      <ToolGroupBlock
        collapsed={false}
        agentName="Hanako"
        tools={[{
          name: 'exec_command',
          args: { cmd: 'npm test' },
          done: true,
          success: true,
        }]}
      />,
    );

    expect(screen.getByText('npm test')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /npm test/ })).toHaveAttribute('aria-expanded', 'false');
  });

  it('renders every exec_command as an expandable embedded command and output card', () => {
    render(
      <ToolGroupBlock
        collapsed={false}
        tools={[{
          id: 'call-exec',
          name: 'exec_command',
          args: { cmd: 'npm test', workdir: '/workspace' },
          done: true,
          success: true,
          details: {
            output: '11172 tests passed',
            execCommand: {
              renderedCommand: 'cd /workspace && npm test',
              workdir: '/workspace',
              tty: false,
              exitCode: 0,
            },
          },
        }]}
      />,
    );

    expect(screen.queryByText('11172 tests passed')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: /npm test/i }));
    expect(screen.getByText(/cd \/workspace && npm test/)).toBeInTheDocument();
    expect(screen.getByText('11172 tests passed')).toBeInTheDocument();
  });

  it('只在展开历史命令卡时读取完整输出', async () => {
    useStore.setState({ serverPort: '30141' } as never);
    const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(JSON.stringify({
      id: 'deferred-output-1',
      kind: 'tool_output',
      content: '完整输出末尾',
    }), { status: 200, headers: { 'Content-Type': 'application/json' } }));

    render(
      <ToolGroupBlock
        collapsed={false}
        sessionPath="/session/heavy.jsonl"
        tools={[{
          id: 'call-heavy',
          name: 'exec_command',
          args: { cmd: 'npm test' },
          done: true,
          success: true,
          details: {
            output: '输出预览',
            outputDeferred: {
              id: 'deferred-output-1',
              kind: 'tool_output',
              size: 9_000,
              available: true,
            },
            execCommand: { tty: false, exitCode: 0 },
          },
        }]}
      />,
    );

    expect(fetchMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: /npm test/ }));

    await waitFor(() => {
      expect(screen.getByText('完整输出末尾')).toBeInTheDocument();
    });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(String(fetchMock.mock.calls[0][0])).toContain('/api/sessions/content/deferred-output-1');
  });

  it('renders a model read of SKILL.md as a unified expandable skill row', () => {
    window.t = ((key: string, vars?: Record<string, unknown>) => {
      const name = String(vars?.name || '');
      if (key === 'toolGroup.skill.running') return `正在运行技能 ${name}`;
      if (key === 'toolGroup.skill.completed') return `已运行技能 ${name}`;
      if (key === 'toolGroup.skill.failed') return `技能调用失败 ${name}`;
      if (key === 'toolGroup.skill.skillLabel') return '技能';
      if (key === 'toolGroup.skill.paramsLabel') return '参数';
      return key;
    }) as typeof window.t;

    const { container } = render(
      <ToolGroupBlock
        collapsed={false}
        skillPrompt="把模型的用量统计页面从供应商页面独立出来到设置主界面中。"
        tools={[{
          id: 'call-skill',
          name: 'read',
          args: { path: '/workspace/.agents/skills/leader/SKILL.md' },
          done: true,
          success: true,
          details: {
            skillInvocation: {
              content: '# Skill: leader\n\nLead the work carefully.',
            },
          },
        }]}
      />,
    );

    const button = screen.getByRole('button', { name: /leader/ });
    expect(button).toHaveAttribute('aria-expanded', 'false');
    expect(container.querySelector('[data-skill-name="leader"]')).toBeInTheDocument();
    expect(screen.queryByText('Lead the work carefully.', { exact: false })).toBeNull();

    fireEvent.click(button);
    expect(button).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByText('messageActivity.labels.skill')).toBeInTheDocument();
    expect(screen.getAllByText('leader')).toHaveLength(2);
    expect(screen.getByText('参数')).toBeInTheDocument();
    expect(screen.getByText('把模型的用量统计页面从供应商页面独立出来到设置主界面中。')).toBeInTheDocument();
    expect(screen.queryByText('/workspace/.agents/skills/leader/SKILL.md', { exact: false })).not.toBeInTheDocument();
    expect(screen.getByText(/<skill_content name="leader">/)).toBeInTheDocument();
    expect(screen.getByText(/Lead the work carefully\./)).toBeInTheDocument();
  });

  it('keeps skill and ordinary rows in the group regardless of its legacy collapsed flag', () => {
    window.t = ((key: string, vars?: Record<string, unknown>) => (
      key === 'toolGroup.skill.completed' ? `已运行技能 ${vars?.name}` : key
    )) as typeof window.t;

    render(
      <ToolGroupBlock
        collapsed
        tools={[
          {
            id: 'call-skill',
            name: 'read',
            args: { path: '/skills/leader/SKILL.md' },
            done: true,
            success: true,
            details: { skillInvocation: { content: '# Skill: leader' } },
          },
          {
            id: 'call-read',
            name: 'read',
            args: { path: '/tmp/report.md' },
            done: true,
            success: true,
          },
        ]}
      />,
    );

    expect(screen.getByRole('button', { name: /leader/ })).toBeInTheDocument();
    expect(screen.getByText('/tmp/report.md')).toBeInTheDocument();
  });

  it('keeps the chat command card running while its background terminal is still running', () => {
    useStore.setState({
      terminalsBySession: {
        'sess-a': [{
          terminalId: 'term-running',
          toolCallId: 'call-running',
          sessionId: 'sess-a',
          sessionPath: '/session/a.jsonl',
          agentId: 'hana',
          cwd: '/workspace',
          command: 'npm run dev',
          label: 'npm run dev',
          status: 'running',
          seq: 0,
          createdAt: 1,
          lastActivityAt: 1,
          exitedAt: null,
          exitCode: null,
          signal: null,
          transcriptPath: '/state/term-running.jsonl',
        }],
      },
    } as never);

    render(
      <ToolGroupBlock
        collapsed={false}
        tools={[{
          id: 'call-running',
          name: 'exec_command',
          args: { cmd: 'npm run dev' },
          done: true,
          success: true,
          status: 'succeeded',
          details: { execCommand: { terminalId: 'term-running' } },
        }]}
      />,
    );

    expect(document.querySelector('[data-status="running"]')).toBeInTheDocument();
    expect(screen.queryByText('✓')).toBeNull();
  });

  function execTerminal(overrides: Record<string, unknown> = {}) {
    return {
      terminalId: 'term-1',
      toolCallId: 'call-exec',
      sessionId: 'sess-sub',
      sessionPath: '/session/sub.jsonl',
      agentId: 'hana',
      cwd: '/workspace',
      command: 'npm run dev',
      label: 'npm run dev',
      status: 'running',
      seq: 0,
      createdAt: 1,
      lastActivityAt: 1,
      exitedAt: null,
      exitCode: null,
      signal: null,
      transcriptPath: '/state/term-1.jsonl',
      ...overrides,
    };
  }

  function renderExecCard(toolOverrides: Record<string, unknown> = {}) {
    return render(
      <ToolGroupBlock
        collapsed={false}
        tools={[{
          id: 'call-exec',
          name: 'exec_command',
          args: { cmd: 'npm run dev' },
          done: true,
          success: true,
          status: 'succeeded',
          details: { execCommand: { terminalId: 'term-1' } },
          ...toolOverrides,
        }]}
      />,
    );
  }

  it('finds the terminal registered under a subagent session key (subagent preview context)', () => {
    // 子助手预览用子会话路径渲染；tty 终端注册在子会话 key 下，卡片不能因此提前打勾。
    useStore.setState({
      currentSessionPath: '/session/parent.jsonl',
      terminalsBySession: { 'sess-sub': [execTerminal()] },
    } as never);

    renderExecCard();

    expect(document.querySelector('[data-status="running"]')).toBeInTheDocument();
    expect(screen.queryByText('✓')).toBeNull();
  });

  it('renders a stale terminal as neutral lost-contact, never as success or failure', () => {
    useStore.setState({
      terminalsBySession: { 'sess-sub': [execTerminal({ status: 'stale' })] },
    } as never);

    renderExecCard();

    expect(screen.getByText('rightWorkspace.terminal.stale')).toBeInTheDocument();
    expect(screen.queryByText('✓')).toBeNull();
    expect(screen.queryByText('✗')).toBeNull();
  });

  it('falls back to the tool result when an exited terminal has no usable exit code', () => {
    useStore.setState({
      terminalsBySession: { 'sess-sub': [execTerminal({ status: 'exited', exitCode: null })] },
    } as never);

    renderExecCard({ done: true, success: true, status: 'succeeded' });
    expect(document.querySelector('[data-status="succeeded"]')).toBeInTheDocument();
    expect(screen.queryByText('✓')).toBeNull();
    expect(screen.queryByText('✗')).toBeNull();

    cleanup();
    renderExecCard({ done: true, success: false, status: 'failed' });
    expect(document.querySelector('[data-status="failed"]')).toBeInTheDocument();
  });

  it('maps a non-zero exit code to failure and zero to success', () => {
    useStore.setState({
      terminalsBySession: { 'sess-sub': [execTerminal({ status: 'exited', exitCode: 2 })] },
    } as never);

    renderExecCard();
    expect(document.querySelector('[data-status="failed"]')).toBeInTheDocument();

    cleanup();
    useStore.setState({
      terminalsBySession: { 'sess-sub': [execTerminal({ status: 'exited', exitCode: 0 })] },
    } as never);
    renderExecCard();
    expect(document.querySelector('[data-status="succeeded"]')).toBeInTheDocument();
    expect(screen.queryByText('✓')).toBeNull();
  });

  it('keeps terminal and ordinary rows visible within the outer process container', () => {
    render(
      <ToolGroupBlock
        collapsed
        tools={[
          {
            id: 'call-exec',
            name: 'exec_command',
            args: { cmd: 'npm run build' },
            done: true,
            success: true,
          },
          {
            id: 'call-read',
            name: 'read',
            args: { file_path: '/tmp/report.md' },
            done: true,
            success: true,
          },
        ]}
      />,
    );

    expect(screen.getByRole('button', { name: /npm run build/ })).toBeTruthy();
    expect(screen.getByText('/tmp/report.md')).toBeInTheDocument();
  });

  it('renders write_stdin with the unified terminal label', () => {
    window.t = ((key: string, vars?: Record<string, unknown>) => {
      if (key === 'tool.terminal.done') return `💻 ${vars?.name} 敲完了`;
      return key;
    }) as typeof window.t;

    render(
      <ToolGroupBlock
        collapsed={false}
        agentName="Hanako"
        tools={[{
          name: 'write_stdin',
          args: { process_id: 'term_1', chars: 'q\n' },
          done: true,
          success: true,
        }]}
      />,
    );

    expect(screen.getByText('messageActivity.labels.terminal')).toBeInTheDocument();
    // 悬停仍能看到送进终端的完整字符。短标签落地后行上多了一层 title（工具本名），
    // 所以这里指名摘要格，不再靠"第一个带 title 的元素"这种位置约定。
    expect(summarySpan('write_stdin')).toHaveAttribute('title', 'q\n');
  });

  it('leaves all rows visible when the outer completed block collects the group', async () => {
    const { rerender } = render(
      <ToolGroupBlock
        collapsed={false}
        tools={[
          { name: 'bash', args: { command: 'npm test' }, done: true, success: true },
          { name: 'read', args: { file_path: '/tmp/report.md' }, done: false, success: false },
        ]}
      />,
    );

    // 展开时工具内容可见
    expect(screen.getByText('npm test')).toBeInTheDocument();

    rerender(
      <ToolGroupBlock
        collapsed={true}
        tools={[
          { name: 'bash', args: { command: 'npm test' }, done: true, success: true },
          { name: 'read', args: { file_path: '/tmp/report.md' }, done: true, success: true },
        ]}
      />,
    );

    // 内部不再重复收纳；完成后是否可见由外层 ProcessFold 决定。
    await waitFor(() => {
      expect(screen.getByText('npm test')).toBeInTheDocument();
    });
  });

  it('keeps a single tool as an expandable row without a group summary', () => {
    render(
      <ToolGroupBlock
        collapsed={true}
        tools={[{
          name: 'bash',
          args: { command: 'npm test' },
          done: true,
          success: true,
        }]}
      />,
    );

    expect(screen.queryByText('toolGroup.count')).toBeNull();
    expect(screen.getByText('npm test')).toBeTruthy();
  });

  it('hides automation create/update tools because the suggestion card is the UI', () => {
    const { container } = render(
      <ToolGroupBlock
        collapsed={false}
        tools={[
          {
            name: 'automation',
            args: { action: 'create', label: 'Tea' },
            done: true,
            success: true,
          },
          {
            name: 'automation',
            args: { action: 'update', id: 'job_1' },
            done: true,
            success: true,
          },
        ]}
      />,
    );

    expect(container.firstChild).toBeNull();
  });

  it('hides media generation tools because media blocks and output cards are the UI', () => {
    const { container } = render(
      <ToolGroupBlock
        collapsed={false}
        tools={[
          {
            name: 'media_generate-image',
            args: {
              prompt: 'Japanese anime doodle style illustration',
              resolution: '2K',
            },
            done: true,
            success: true,
          },
          {
            name: 'media_generate-video',
            args: {
              prompt: 'A short product reveal clip',
              duration: 5,
            },
            done: true,
            success: true,
          },
        ]}
      />,
    );

    expect(container.firstChild).toBeNull();
  });

  it('hides interactive card guide and render tools because the card is the UI', () => {
    const { container } = render(
      <ToolGroupBlock
        collapsed={false}
        tools={[
          {
            name: 'hana_card_guide',
            args: {},
            done: true,
            success: true,
          },
          {
            name: 'show_card',
            args: {
              title: 'dorm_comparison',
            },
            done: true,
            success: true,
          },
        ]}
      />,
    );

    expect(container.firstChild).toBeNull();
  });

  it('hides current card-backed tools while keeping visible browser and compatibility tools', () => {
    render(
      <ToolGroupBlock
        collapsed={false}
        tools={[
          {
            name: 'workflow',
            args: { taskId: 'workflow-1', workflow: 'Morning brief' },
            done: true,
            success: true,
          },
          {
            name: 'install_skill',
            args: { skill_name: 'daily-review' },
            done: true,
            success: true,
          },
          {
            name: 'update_settings',
            args: { key: 'locale' },
            done: true,
            success: true,
          },
          {
            name: 'automation',
            args: { action: 'pending_add', label: 'Tea' },
            done: true,
            success: true,
          },
          {
            name: 'browser',
            args: { action: 'screenshot' },
            done: true,
            success: true,
          },
          {
            name: 'browser',
            args: { action: 'navigate', url: 'https://example.com' },
            done: true,
            success: true,
          },
          {
            name: 'present_files',
            args: { path: 'legacy.txt' },
            done: true,
            success: true,
          },
        ]}
      />,
    );

    expect(screen.getByText('example.com')).toBeInTheDocument();
    expect(screen.getByText('legacy.txt')).toBeInTheDocument();
    expect(screen.queryByText('Morning brief')).not.toBeInTheDocument();
    expect(screen.queryByText('daily-review')).not.toBeInTheDocument();
    expect(screen.queryByText('locale')).not.toBeInTheDocument();
    expect(screen.queryByText('Tea')).not.toBeInTheDocument();
  });

  it('keeps the tool layout box aligned to the task-block width', () => {
    const css = fs.readFileSync(
      path.join(process.cwd(), 'desktop/src/react/components/chat/Chat.module.css'),
      'utf8',
    );
    const rootCss = fs.readFileSync(
      path.join(process.cwd(), 'desktop/src/styles.css'),
      'utf8',
    );
    const toolGroupRule = css.match(/\.toolGroup\s*\{(?<body>[^}]*)\}/)?.groups?.body || '';

    expect(rootCss).toMatch(/--chat-task-block-width:\s*100%/);
    expect(toolGroupRule).toContain('width: var(--chat-task-block-width)');
    expect(toolGroupRule).toContain('max-width: 100%');
    expect(toolGroupRule).toContain('box-sizing: border-box');
  });

  it('uses fixed icons and a shared row and limits terminal detail height', () => {
    const css = fs.readFileSync(path.join(process.cwd(), 'desktop/src/react/components/chat/MessageActivity.module.css'), 'utf8');
    expect(css).toMatch(/\.row\s*\{[^}]*font-size: 13px/);
    expect(css).toMatch(/\.row\s*\{[^}]*line-height: 24px/);
    expect(css).toMatch(/\.icon\s*\{[^}]*width: 14px/);
    expect(css).toMatch(/\.terminal\s*\{[^}]*max-height: 224px/);
    expect(css).not.toMatch(/arrow|chevron|::before|::after/);
  });

  it('fuses consecutive subagent cards into one rounded block', () => {
    const css = fs.readFileSync(
      path.join(process.cwd(), 'desktop/src/react/components/chat/Chat.module.css'),
      'utf8',
    );
    const leading = css.match(/\.subagentResourceCard\[data-chat-resource-card\]:has\(\+ \.subagentResourceCard\[data-chat-resource-card\]\)\s*\{(?<body>[^}]*)\}/)?.groups?.body || '';
    const trailing = css.match(/\.subagentResourceCard\[data-chat-resource-card\]\s*\+\s*\.subagentResourceCard\[data-chat-resource-card\]\s*\{(?<body>[^}]*)\}/)?.groups?.body || '';

    expect(leading).toContain('margin-bottom: 0');
    expect(leading).toContain('border-bottom-left-radius: 0');
    expect(leading).toContain('border-bottom-right-radius: 0');
    expect(trailing).toContain('margin-top: 0');
    expect(trailing).toContain('border-top-left-radius: 0');
    expect(trailing).toContain('border-top-right-radius: 0');
    expect(trailing).toContain('border-top: 1px solid var(--overlay-light');
  });

  it('renders the task-family container as a bare text row without a card shell', () => {
    const css = fs.readFileSync(
      path.join(process.cwd(), 'desktop/src/react/components/chat/Chat.module.css'),
      'utf8',
    );
    const toolGroupRule = css.match(/\.toolGroup\s*\{(?<body>[^}]*)\}/)?.groups?.body || '';
    expect(toolGroupRule).not.toContain('background:');
    expect(toolGroupRule).not.toContain('border-radius');
    expect(toolGroupRule).not.toContain('padding-left');
    expect(css).not.toMatch(/\.toolGroup::before/);
    expect(css).not.toContain('hana-tool-bar-in');

    const toolDotsRule = css.match(/\.toolDots\s*\{(?<body>[^}]*)\}/)?.groups?.body || '';
    expect(toolDotsRule).toContain('color: var(--tool-text)');
  });

  it('冷门内置工具的行主标签是中文短标签，不是裸英文工具名', () => {
    useRealLocale();
    render(
      <ToolGroupBlock
        collapsed={false}
        tools={[
          { id: 'm1', name: 'search_memory', args: { query: '预算' }, done: true, success: true, status: 'succeeded' },
          { id: 'm2', name: 'pin_memory', args: { content: '记住了' }, done: true, success: true, status: 'succeeded' },
          { id: 'k1', name: 'knowledge_search', args: { query: '报销制度' }, done: true, success: true, status: 'succeeded' },
          { id: 'k2', name: 'knowledge_manage', args: { action: 'add' }, done: true, success: true, status: 'succeeded' },
          { id: 'c1', name: 'channel_reply', args: { text: '收到' }, done: true, success: true, status: 'succeeded' },
          { id: 'n1', name: 'notify', args: { title: '提醒' }, done: true, success: true, status: 'succeeded' },
          { id: 'b1', name: 'browser', args: { action: 'navigate', url: 'https://example.com' }, done: true, success: true, status: 'succeeded' },
          { id: 'p1', name: 'computer', args: { action: 'click' }, done: true, success: true, status: 'succeeded' },
          { id: 'f1', name: 'file', args: { action: 'stat', path: '/tmp/a.txt' }, done: true, success: true, status: 'succeeded' },
          { id: 'd1', name: 'materialize', args: { path: '/tmp/b.txt' }, done: true, success: true, status: 'succeeded' },
          { id: 's1', name: 'stop_task', args: { reason: '用户叫停' }, done: true, success: true, status: 'succeeded' },
          { id: 'sf1', name: 'session_folders', args: { action: 'list' }, done: true, success: true, status: 'succeeded' },
          { id: 'cp1', name: 'check_pending_tasks', args: {}, done: true, success: true, status: 'succeeded' },
          { id: 'lc1', name: 'loop_control', args: { action: 'schedule' }, done: true, success: true, status: 'succeeded' },
          { id: 'cs1', name: 'current_status', args: {}, done: true, success: true, status: 'succeeded' },
          { id: 'a1', name: 'automation', args: { action: 'list' }, done: true, success: true, status: 'succeeded' },
          { id: 'ca1', name: 'create_artifact', args: {}, done: true, success: true, status: 'succeeded' },
          { id: 'dm1', name: 'dm', args: { to: 'hanako' }, done: true, success: true, status: 'succeeded' },
        ]}
      />,
    );

    const expected: Array<[string, string]> = [
      ['search_memory', '回想'],
      ['pin_memory', '钉住'],
      ['knowledge_search', '查资料'],
      ['knowledge_manage', '管资料'],
      ['channel_reply', '回频道'],
      ['notify', '发通知'],
      ['browser', '开网页'],
      ['computer', '操控电脑'],
      ['file', '文件操作'],
      ['materialize', '落盘'],
      ['stop_task', '叫停'],
      ['session_folders', '会话文件夹'],
      ['check_pending_tasks', '看待办'],
      ['loop_control', '循环控制'],
      ['current_status', '报状态'],
      ['automation', '自动化'],
      ['create_artifact', '做卡片'],
      ['dm', '私信'],
    ];
    for (const [name, label] of expected) {
      const span = labelSpan(name);
      expect(span.textContent, `${name} 的行主标签`).toBe(label);
      expect(span.textContent, `${name} 的行主标签不许是工具本名`).not.toBe(name);
      // 原工具名降级到悬停提示，仍可查
      expect(rowTitle(name)).toBe(name);
    }
    // 工具本名不再出现在可见文字里（只在 data-tool / title 这类属性上）
    for (const [name] of expected) expect(screen.queryByText(name)).toBeNull();
  });

  it('子代理转达的行标签走短标签，原工具名降级到悬停提示与完整调用弹窗', () => {
    useRealLocale();
    render(
      <ToolGroupBlock
        collapsed={false}
        tools={[{ id: 'sr1', name: 'subagent_reply', args: { text: '结果已收到' }, done: true, success: true, status: 'succeeded' }]}
      />,
    );

    expect(labelSpan('subagent_reply').textContent).toBe('转达子代理');
    expect(rowTitle('subagent_reply')).toBe('subagent_reply');
    expect(screen.queryByText('subagent_reply')).toBeNull();
    // 完整调用弹窗仍带原工具名，排障时查得到
    fireEvent.click(screen.getAllByRole('button')[0]);
    expect(screen.getByText(/subagent_reply/)).toBeInTheDocument();
  });

  it('MCP / 第三方插件工具行用统一家族词，裸工具名不作主标签', () => {
    useRealLocale();
    render(
      <ToolGroupBlock
        collapsed={false}
        tools={[
          { id: 'x1', name: 'mcp_deep-search', args: { query: '周报' }, done: true, success: true, status: 'succeeded' },
          { id: 'x2', name: 'acme_read', args: { path: '/tmp/a.md' }, done: true, success: true, status: 'succeeded' },
          { id: 'x3', name: 'mcp_search_issues', args: {}, done: true, success: true, status: 'succeeded' },
        ]}
      />,
    );

    for (const name of ['mcp_deep-search', 'acme_read', 'mcp_search_issues']) {
      const span = labelSpan(name);
      expect(span.textContent, `${name} 的主标签应是家族词「扩展」`).toBe('扩展');
      expect(span.textContent).not.toBe(name);
      expect(rowTitle(name)).toBe(name);
      expect(screen.queryByText(name)).toBeNull();
    }
  });

  it('别名与面板标题维持现状：exec_command / write_stdin 叫 Bash，清单工具叫任务', () => {
    useRealLocale();
    render(
      <ToolGroupBlock
        collapsed={false}
        tools={[
          { id: 'e1', name: 'exec_command', args: { cmd: 'npm test' }, done: true, success: true, status: 'succeeded' },
          { id: 'w1', name: 'write_stdin', args: { chars: 'q\n' }, done: true, success: true, status: 'succeeded' },
          { id: 't1', name: 'todo_write', done: true, success: true, status: 'succeeded',
            details: { todos: [{ content: '读 spec', status: 'in_progress' }] } },
        ]}
      />,
    );

    expect(labelSpan('exec_command').textContent).toBe('Bash');
    expect(labelSpan('write_stdin').textContent).toBe('Bash');
    // todo_write 不走 labels，取面板标题「任务」
    const todoRow = document.querySelector('[data-tool="todo_write"]');
    expect(todoRow).toHaveTextContent('任务');
  });

  it('技能行标签仍是「技能」，不被工具短标签覆盖', () => {
    useRealLocale();
    render(
      <ToolGroupBlock
        collapsed={false}
        skillPrompt="按 leader 技能执行"
        tools={[{
          id: 'sk1',
          // 与既有技能用例同一形态：模型读的是技能入口文件
          name: 'read',
          args: { path: '/workspace/.agents/skills/leader/SKILL.md' },
          done: true,
          success: true,
          status: 'succeeded',
          details: { skillInvocation: { content: '# Skill: leader' } },
        }]}
      />,
    );

    expect(labelSpan('read').textContent).toBe('技能');
    expect(labelSpan('read').textContent).not.toBe('读取');
  });
});

describe('搜索结构化事实与延迟引用的展示语义', () => {
  beforeEach(() => {
    useRealLocale();
    useStore.setState({ serverPort: '30141', terminalsBySession: {} } as never);
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); });

  const structuredFiles = [
    { path: 'src/a.ts', matches: [
      { line: 6, text: 'Error at other.ts:123: boom', context: true },
      { line: 7, text: 'target' },
    ] },
  ];
  const ambiguousOutput = [
    'src/a.ts-6- Error at other.ts:123: boom',
    'src/a.ts:7: target',
  ].join('\n');

  function grepTool(details: Record<string, unknown>) {
    return {
      id: 'grep-1', name: 'grep', args: { pattern: 'target' }, done: true, success: true, status: 'succeeded' as const,
      details,
    };
  }
  /**
   * grep 详情：文件标题渲染在 h4 里，行号与正文渲染在 `.line` 行里。
   * `.lines` 容器也会被 [class*=] 命中，所以按 class 词边界排除它。
   */
  const searchPaths = () => [...document.querySelectorAll('h4')].map(node => node.textContent ?? '');
  const searchLines = () => [...document.querySelectorAll('[class*="line"]')]
    .filter(node => !/\bline/.test(node.className))
    .map(node => node.textContent ?? '');

  it('首包省略 files 时用搜索结构引用恢复，不从正文重猜路径与行号', async () => {
    // 输出与搜索结构是两条引用、两次请求；同一个 Response 对象的 body 只能读一次，
    // 所以必须按 URL 分别构造（共享响应体会让第二条请求读空、显示加载失败）。
    const fetchMock = vi.spyOn(globalThis, 'fetch').mockImplementation(async (input: RequestInfo | URL) => {
      const isSearch = String(input).includes('search-ref-1');
      return new Response(JSON.stringify(isSearch
        ? { id: 'search-ref-1', kind: 'tool_search',
          content: JSON.stringify({ kind: 'grep', basePath: '/root', files: structuredFiles, matchCount: 1, fileCount: 1 }) }
        : { id: 'out-ref-1', kind: 'tool_output', content: ambiguousOutput }),
      { status: 200, headers: { 'Content-Type': 'application/json' } });
    });

    render(
      <ToolGroupBlock
        collapsed={false}
        sessionPath="/session/search.jsonl"
        tools={[grepTool({
          output: 'preview',
          outputDeferred: { id: 'out-ref-1', kind: 'tool_output', size: 20_000, available: true },
          search: { kind: 'grep', basePath: '/root', matchCount: 1, fileCount: 1,
            searchDeferred: { id: 'search-ref-1', kind: 'tool_search', size: 200, available: true } },
        })]}
      />,
    );

    fireEvent.click(screen.getAllByRole('button')[0]);
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    await waitFor(() => expect(searchPaths()).toContain('src/a.ts'));
    // 恢复的是结构化真相：src/a.ts + line 6 context + line 7 match。
    expect(searchPaths()).not.toContain('src/a.ts-6- Error at other.ts');
    const lines = searchLines();
    expect(lines).toContain('6: Error at other.ts:123: boom');
    expect(lines).toContain('7: target');
  });

  it('真正没有结构化 metadata 的旧记录才走文本解析兜底', async () => {
    render(
      <ToolGroupBlock
        collapsed={false}
        sessionPath="/session/legacy.jsonl"
        tools={[grepTool({ output: ambiguousOutput, search: { kind: 'grep', fileCount: 1, matchCount: 1 } })]}
      />,
    );
    fireEvent.click(screen.getAllByRole('button')[0]);
    // legacy 路径的能力边界：文本歧义无法消除，这里明确记录当前行为——
    // 上下文行被读成 `src/a.ts-6- Error at other.ts` / 第 123 行。
    await waitFor(() => expect(searchPaths().length).toBeGreaterThan(0));
    // legacy 只能还原"看起来像"的东西：命中行是对的，上下文行被读成另一个文件
    // （`src/a.ts-6- Error at other.ts` / 第 123 行 / boom）。
    expect(searchPaths()).toContain('src/a.ts');
    expect(searchPaths()).toContain('src/a.ts-6- Error at other.ts');
    expect(document.body.textContent).toContain('123: boom');
    // 这就是结构化记录必须走引用的原因：结构化路径两条都落回同一个文件。
  });

  it('延迟加载失败时显示失败，不用截断预览冒充完整正文', async () => {
    vi.spyOn(globalThis, 'fetch').mockRejectedValue(new Error('offline'));
    render(
      <ToolGroupBlock
        collapsed={false}
        sessionPath="/session/fail.jsonl"
        tools={[grepTool({
          output: 'preview',
          outputDeferred: { id: 'out-ref-fail', kind: 'tool_output', size: 20_000, available: true },
          search: { kind: 'grep', fileCount: 1, matchCount: 1,
            searchDeferred: { id: 'search-ref-fail', kind: 'tool_search', size: 200, available: true } },
        })]}
      />,
    );
    fireEvent.click(screen.getAllByRole('button')[0]);
    await waitFor(() => expect(screen.getByText(/加载失败/)).toBeInTheDocument());
  });

  it('历史兼容工具 present_files 显示专属短标签而不是泛化的“工具”', () => {
    render(
      <ToolGroupBlock
        collapsed={false}
        tools={[{ id: 'pf', name: 'present_files', args: { path: 'legacy.txt' }, done: true, success: true, status: 'succeeded' }]}
      />,
    );
    expect(labelSpan('present_files').textContent).toBe('交付');
    expect(labelSpan('present_files').textContent).not.toBe('工具');
  });

  it('office 工具按运行时长名命中专属短标签', () => {
    render(
      <ToolGroupBlock
        collapsed={false}
        tools={[{
          id: 'o1', name: 'office_html-to-pdf', args: { path: 'a.html' },
          done: true, success: true, status: 'succeeded' as const,
        }]}
      />,
    );
    expect(labelSpan('office_html-to-pdf').textContent).toBe('转 PDF');
  });
});
