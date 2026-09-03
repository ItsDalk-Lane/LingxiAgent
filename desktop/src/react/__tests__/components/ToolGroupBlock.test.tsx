// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import fs from 'node:fs';
import path from 'node:path';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ToolGroupBlock } from '../../components/chat/ToolGroupBlock';
import { useStore } from '../../stores';

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
    expect(screen.getByText('✗')).toBeInTheDocument();
    expect(screen.getByText('?')).toBeInTheDocument();
    expect(screen.queryByText('✓')).not.toBeInTheDocument();
  });

  it('本地检索完成后只显示一份证据数量与耗时', () => {
    render(<ToolGroupBlock collapsed={false} tools={[{ id: 'local', name: 'knowledge_local_search',
      done: true, success: true, status: 'succeeded', resultNote: '已找到 3 条证据 · 28ms' }]} />);
    expect(screen.getAllByText('已找到 3 条证据 · 28ms')).toHaveLength(1);
    expect(screen.queryByText('tool.knowledge_local_search.done')).not.toBeInTheDocument();
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

    expect(detail.textContent).toBe('rm -rf /Users/jason/.claude/plugins/mar…');
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
    expect(screen.getByRole('button', { name: 'npm test' })).toHaveAttribute('aria-expanded', 'false');
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
    fireEvent.click(screen.getByRole('button', { name: 'npm test' }));

    await waitFor(() => {
      expect(screen.getByText('完整输出末尾')).toBeInTheDocument();
    });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(String(fetchMock.mock.calls[0][0])).toContain('/api/sessions/content/deferred-output-1');
  });

  it('renders a model read of SKILL.md as a full-width expandable skill card', () => {
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

    const button = screen.getByRole('button', { name: '已运行技能 leader' });
    expect(button).toHaveAttribute('aria-expanded', 'false');
    expect(container.querySelector('[data-skill-name="leader"]')).toBeInTheDocument();
    expect(screen.queryByText('Lead the work carefully.', { exact: false })).toBeNull();

    fireEvent.click(button);
    expect(button).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByText('技能')).toBeInTheDocument();
    expect(screen.getByText('leader')).toBeInTheDocument();
    expect(screen.getByText('参数')).toBeInTheDocument();
    expect(screen.getByText('把模型的用量统计页面从供应商页面独立出来到设置主界面中。')).toBeInTheDocument();
    expect(screen.queryByText('/workspace/.agents/skills/leader/SKILL.md', { exact: false })).not.toBeInTheDocument();
    expect(screen.getByText(/<skill_content name="leader">/)).toBeInTheDocument();
    expect(screen.getByText(/Lead the work carefully\./)).toBeInTheDocument();
  });

  it('keeps skill cards visible when the surrounding multi-tool group is collapsed', () => {
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

    expect(screen.getByRole('button', { name: '已运行技能 leader' })).toBeInTheDocument();
    expect(screen.queryByText('/tmp/report.md')).toBeNull();
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

    expect(screen.getByText('…')).toBeInTheDocument();
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

    expect(screen.getByText('…')).toBeInTheDocument();
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
    expect(screen.getByText('✓')).toBeInTheDocument();
    expect(screen.queryByText('✗')).toBeNull();

    cleanup();
    renderExecCard({ done: true, success: false, status: 'failed' });
    expect(screen.getByText('✗')).toBeInTheDocument();
  });

  it('maps a non-zero exit code to failure and zero to success', () => {
    useStore.setState({
      terminalsBySession: { 'sess-sub': [execTerminal({ status: 'exited', exitCode: 2 })] },
    } as never);

    renderExecCard();
    expect(screen.getByText('✗')).toBeInTheDocument();

    cleanup();
    useStore.setState({
      terminalsBySession: { 'sess-sub': [execTerminal({ status: 'exited', exitCode: 0 })] },
    } as never);
    renderExecCard();
    expect(screen.getByText('✓')).toBeInTheDocument();
  });

  it('keeps exec_command cards visible even when the surrounding multi-tool group is collapsed', () => {
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

    expect(screen.getByRole('button', { name: 'npm run build' })).toBeTruthy();
    expect(screen.queryByText('/tmp/report.md')).toBeNull();
  });

  it('renders write_stdin with the legacy terminal user-facing copy', () => {
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

    expect(screen.getByText('💻 Hanako 敲完了')).toBeInTheDocument();
    expect(document.querySelector('[data-tool="write_stdin"] [title]')).toHaveAttribute('title', 'q\n');
  });

  it('syncs a multi-tool group to collapsed when the completed block updates', async () => {
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

    // 折叠后，Collapse 组件通过 AnimatePresence 退场动画后移除内容。
    // jsdom 下 requestAnimationFrame 可能延迟执行退场，用 waitFor 等待。
    await waitFor(() => {
      expect(screen.queryByText('npm test')).not.toBeInTheDocument();
    });
  });

  it('keeps a single tool as a plain indicator without a fold summary', () => {
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

  it('lets terminal and subagent chat cards use the full message width', () => {
    const css = fs.readFileSync(
      path.join(process.cwd(), 'desktop/src/react/components/chat/Chat.module.css'),
      'utf8',
    );
    const execContentRule = css.match(/\.toolGroupExecContent\s*\{(?<body>[^}]*)\}/)?.groups?.body || '';

    expect(execContentRule).toContain('padding: 0');
    expect(css).not.toContain('.toolGroupWithExec');
  });

  it('keeps embedded terminal and subagent details at one fixed scrollable height', () => {
    const css = fs.readFileSync(
      path.join(process.cwd(), 'desktop/src/react/components/chat/Chat.module.css'),
      'utf8',
    );
    const subagentPreviewRule = css.match(/\.subagentEmbeddedPreview\s*\{(?<body>[^}]*)\}/)?.groups?.body || '';
    const execDetailsRule = css.match(/\.execCommandDetails\s*\{(?<body>[^}]*)\}/)?.groups?.body || '';
    const execOutputRules = [...css.matchAll(/\.execCommandOutput\s*\{(?<body>[^}]*)\}/g)];
    const execOutputRule = execOutputRules.at(-1)?.groups?.body || '';
    const nestedTerminalRule = css.match(/\.execCommandDetails\s+:global\(\[data-testid\^="terminal-preview-"\]\)\s*\{(?<body>[^}]*)\}/)?.groups?.body || '';

    expect(subagentPreviewRule).toContain('height: var(--chat-embedded-detail-height)');
    expect(subagentPreviewRule).toContain('overflow-y: auto');
    expect(execDetailsRule).toContain('height: var(--chat-embedded-detail-height)');
    expect(execDetailsRule).toContain('display: flex');
    expect(execOutputRule).toContain('overflow: auto');
    expect(nestedTerminalRule).toContain('max-height: none');
    expect(nestedTerminalRule).toContain('overflow: auto');
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
});
