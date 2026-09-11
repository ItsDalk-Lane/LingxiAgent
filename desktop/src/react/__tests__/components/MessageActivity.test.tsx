// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ToolGroupBlock } from '../../components/chat/ToolGroupBlock';
import { ThinkingBlock } from '../../components/chat/ThinkingBlock';
import type { ToolCall } from '../../stores/chat-types';
import { useStore } from '../../stores';
import { clearDeferredHistoryContentCacheForTests } from '../../hooks/use-deferred-history-content';
import { openInternalLink } from '../../utils/link-open';
vi.mock('../../utils/link-open', async importOriginal => ({ ...await importOriginal<typeof import('../../utils/link-open')>(), openInternalLink: vi.fn() }));

const tool = (name: string, details: ToolCall['details'] = {}, args: ToolCall['args'] = {}): ToolCall => ({
  id: name, name, args, details, done: true, success: true, status: 'succeeded',
});
const renderTool = (value: ToolCall, sessionPath = '') => render(<ToolGroupBlock tools={[value]} collapsed={false} sessionPath={sessionPath} />);
const openTool = () => fireEvent.click(screen.getAllByRole('button')[0]);

describe('统一消息行与工具详情', () => {
  beforeEach(() => {
    window.t = ((key: string, vars?: Record<string, unknown>) => `${key}${vars ? ` ${JSON.stringify(vars)}` : ''}`) as typeof window.t;
    useStore.setState({ serverPort: '30141', terminalsBySession: {} } as never);
    clearDeferredHistoryContentCacheForTests();
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: vi.fn().mockResolvedValue(undefined) } });
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); });

  it('保留发生顺序和固定图标，鼠标与键盘均直接展开，外部收纳变化不重置详情', () => {
    const values = [tool('read', { output: 'read output' }), tool('exec_command', { output: 'terminal output' }, { cmd: 'echo hello' }), tool('edit', { output: 'edit output' })];
    const { container, rerender } = render(<ToolGroupBlock tools={values} collapsed={false} />);
    expect([...container.querySelectorAll('[data-tool]')].map(node => node.getAttribute('data-tool'))).toEqual(['read', 'exec_command', 'edit']);
    const row = screen.getAllByRole('button')[0];
    const icon = row.querySelector('svg');
    fireEvent.mouseEnter(row); fireEvent.focus(row);
    expect(row.querySelector('svg')).toBe(icon);
    fireEvent.keyDown(row, { key: 'Enter' });
    expect(row).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByText('read output')).toBeInTheDocument();
    rerender(<ToolGroupBlock tools={values.map(item => ({ ...item }))} collapsed />);
    expect(row).toHaveAttribute('aria-expanded', 'true');
    expect(container.textContent).not.toMatch(/[✓›‹]/);
    expect(row.querySelector('svg')).toBe(icon);
  });

  it('思考运行时取最新非空行，完成后取首个非空行，展开状态保留', () => {
    const { rerender } = render(<ThinkingBlock content={'\n第一行\n\n最后一行\n'} sealed={false} />);
    const row = screen.getByRole('button');
    expect(row).toHaveTextContent('最后一行');
    expect(row).not.toHaveTextContent('第一行');
    fireEvent.click(row);
    rerender(<ThinkingBlock content={'\n第一行\n\n最后一行\n'} sealed />);
    expect(row).toHaveTextContent('第一行');
    expect(row).toHaveAttribute('aria-expanded', 'true');
    expect(row.querySelector('[data-activity-icon="thinking"]')).toBeInTheDocument();
  });

  it('读取显示实际行号与前四后四，复制包含省略部分，文件链接不切换详情', async () => {
    const output = Array.from({ length: 12 }, (_, i) => `line_${i + 1}`).join('\n');
    renderTool(tool('read', { output, read: { path: '/file.txt', startLine: 41, totalLines: 80, displayedLines: 12 } }, { path: '/file.txt' }));
    fireEvent.click(screen.getByRole('link'));
    expect(openInternalLink).toHaveBeenCalledWith('/file.txt', { origin: 'session' });
    expect(screen.getByRole('button')).toHaveAttribute('aria-expanded', 'false');
    openTool();
    expect(screen.getByText('41')).toBeInTheDocument();
    expect(screen.getByText('52')).toBeInTheDocument();
    expect(screen.queryByText('line_5')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'messageActivity.copy' }));
    await waitFor(() => expect(navigator.clipboard.writeText).toHaveBeenCalledWith(output));
    fireEvent.click(screen.getByRole('button', { name: /messageActivity.remaining/ }));
    expect(screen.getByText('line_5')).toBeInTheDocument();
  });

  it('读取尾部范围通知不当成代码编号，复制仍保留原始完整记录', async () => {
    const output = 'one\ntwo\n\n[Showing lines 41-42 of 90. Use offset=43 to continue.]';
    renderTool(tool('read', { output, read: { path: '/file', startLine: 41, displayedLines: 2, totalLines: 90, truncated: true } }));
    openTool();
    expect(screen.getByText('41')).toBeInTheDocument();
    expect(screen.getByText('42')).toBeInTheDocument();
    expect(screen.queryByText('43')).toBeNull();
    expect(screen.getByText(/Showing lines/).parentElement?.className).not.toContain('line_');
    fireEvent.click(screen.getByRole('button', { name: 'messageActivity.copy' }));
    await waitFor(() => expect(navigator.clipboard.writeText).toHaveBeenCalledWith(output));
  });

  it('完整补丁准确统计正文以双加减号开头的行，截断预览不冒充总量', () => {
    const value = tool('edit', { fileChange: { path: 'signs.txt', patch: '--- signs.txt\n+++ signs.txt\n@@ -1 +1 @@\n---old\n+++new', beforeAvailable: true } });
    const { rerender } = renderTool(value);
    expect(screen.getByText('+1 −1')).toBeInTheDocument();
    openTool();
    expect(screen.getByText('---old').parentElement?.className).toContain('removed');
    expect(screen.getByText('+++new').parentElement?.className).toContain('added');
    expect(screen.queryByText('--- signs.txt')).toBeNull();
    rerender(<ToolGroupBlock tools={[{ ...value, details: { fileChange: { path: 'signs.txt', patch: '+preview', beforeAvailable: true, truncated: true, added: 200, removed: 100 } } }]} collapsed={false} />);
    expect(screen.getByText('+200 −100')).toBeInTheDocument();
  });

  it('搜索按文件显示匹配统计与行号', () => {
    renderTool(tool('grep', { output: 'a.ts:2:hello', search: { kind: 'grep', matchCount: 1, fileCount: 1, files: [{ path: 'a.ts', matches: [{ line: 2, text: 'hello' }] }] } }, { pattern: 'hello' }));
    openTool();
    expect(screen.getByText('a.ts')).toBeInTheDocument();
    expect(screen.getByText('2: hello')).toBeInTheDocument();
    expect(screen.getByText(/messageActivity.searchCount/)).toHaveTextContent('"matches":1,"files":1');
  });

  it('多文件搜索全局只展示前四后四行，文件链接使用记录的搜索目录', () => {
    const files = Array.from({ length: 12 }, (_, index) => ({ path: `src/file-${index + 1}.ts`, matches: [{ line: 5, text: `match-${index + 1}` }] }));
    renderTool(tool('grep', { search: { kind: 'grep', basePath: '/workspace', files, matchCount: 12, fileCount: 12 } }));
    openTool();
    expect(screen.getByText('src/file-1.ts')).toBeInTheDocument();
    expect(screen.getByText('src/file-12.ts')).toBeInTheDocument();
    expect(screen.queryByText('src/file-5.ts')).toBeNull();
    fireEvent.click(screen.getByRole('link', { name: 'src/file-1.ts' }));
    expect(openInternalLink).toHaveBeenCalledWith('src/file-1.ts', { origin: 'session', baseFilePath: '/workspace/.tool-search' });
    fireEvent.click(screen.getByRole('button', { name: /messageActivity.remaining/ }));
    expect(screen.getByText('src/file-5.ts')).toBeInTheDocument();
  });

  it('目录和查找结果逐行收纳，不为没有匹配内容的文件创建空白组', () => {
    renderTool(tool('find', { search: { kind: 'find', files: Array.from({ length: 12 }, (_, index) => ({ path: `file-${index + 1}.txt` })), fileCount: 12 } }));
    openTool();
    expect(screen.queryByText('file-5.txt')).toBeNull();
    expect(screen.queryAllByRole('link')).toHaveLength(0);
    fireEvent.click(screen.getByRole('button', { name: /messageActivity.remaining/ }));
    expect(screen.getByText('file-5.txt')).toBeInTheDocument();
  });

  it('文件行摘要使用简短输入路径，预览链接使用本次执行确认的绝对路径', () => {
    renderTool(tool('read', { output: 'hello', read: { path: '/workspace/src/a.ts', startLine: 1 } }, { path: 'src/a.ts' }));
    fireEvent.click(screen.getByRole('link', { name: 'src/a.ts' }));
    expect(openInternalLink).toHaveBeenCalledWith('/workspace/src/a.ts', { origin: 'session' });
  });

  it('编辑展示真实差异和写后正文，只读详情记录当时的输入输出', () => {
    renderTool(tool('edit', { input: '{"path":"a.txt","oldText":"old","newText":"new"}', output: 'saved', fileChange: { path: 'a.txt', patch: '@@ -1 +1 @@\n-old\n+new', content: 'new', beforeAvailable: true } }, { path: 'a.txt' }));
    expect(screen.getByText('+1 −1')).toBeInTheDocument();
    openTool();
    expect(screen.getByText('-old').parentElement?.className).toContain('removed');
    expect(screen.getByText('+new').parentElement?.className).toContain('added');
    expect(screen.getByText('new')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'messageActivity.view' }));
    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByText('saved')).toBeInTheDocument();
    expect(within(dialog).getByText(/"oldText":"old"/)).toBeInTheDocument();
  });

  it('待执行和未知修改不宣称已应用，失败不展示成功差异', () => {
    const value = tool('write', { fileChange: { path: 'empty.txt', patch: '+hello', content: '', beforeAvailable: false } }, { path: 'empty.txt' });
    const { rerender } = renderTool({ ...value, status: 'running', done: false, success: false });
    expect(screen.queryByText('+1 −0')).toBeNull(); openTool();
    expect(screen.getByText('messageActivity.proposed')).toBeInTheDocument();
    expect(screen.getByText('messageActivity.emptyFile')).toBeInTheDocument();
    rerender(<ToolGroupBlock tools={[{ ...value, status: 'unknown', success: false }]} collapsed={false} />);
    expect(screen.queryByText('messageActivity.applied')).toBeNull();
    rerender(<ToolGroupBlock tools={[{ ...value, status: 'failed', success: false, error: 'Permission denied' }]} collapsed={false} />);
    expect(screen.queryByText('+hello')).toBeNull();
    expect(screen.queryByText('messageActivity.applied')).toBeNull();
  });

  it('空读取保留真实零行，历史缺失与截断明确提示', () => {
    renderTool(tool('read', { output: '', outputTruncated: true, read: { path: '/empty', startLine: 1, totalLines: 0, displayedLines: 0 } }));
    openTool();
    expect(screen.getByText(/messageActivity.readLines/)).toHaveTextContent('"shown":0,"total":0');
    expect(screen.getByText('messageActivity.truncated')).toBeInTheDocument();
  });

  it('普通工具按需加载完整输入输出，加载失败禁用复制并允许重试', async () => {
    const fetchMock = vi.spyOn(globalThis, 'fetch').mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce(new Response(JSON.stringify({ id: 'full-generic', kind: 'tool_output', content: 'full result' }), { status: 200 }));
    const value = tool('custom_tool', { input: '{"token":"********"}', output: 'preview', outputDeferred: { id: 'full-generic', kind: 'tool_output', size: 10000, available: true } });
    renderTool(value, '/session/a');
    expect(fetchMock).not.toHaveBeenCalled(); openTool();
    await waitFor(() => expect(screen.getByText('messageActivity.loadFailed')).toBeInTheDocument());
    expect(screen.getByRole('button', { name: 'messageActivity.copy' })).toBeDisabled();
    fireEvent.click(screen.getAllByRole('button')[0]); openTool();
    await waitFor(() => expect(screen.getByText('full result')).toBeInTheDocument());
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('调查详情不泄露隐藏推理、内部输出或身份', () => {
    renderTool(tool('knowledge_research_worker', { input: '{"workerId":"private-worker"}', output: 'private-output' }, { hiddenReasoning: 'private-thought', label: '检查资料' }));
    openTool();
    expect(document.body.textContent).not.toMatch(/private-(worker|output|thought)/);
    expect(screen.queryByRole('button', { name: 'messageActivity.view' })).toBeNull();
    expect(screen.getByText('messageActivity.researchPrivate')).toBeInTheDocument();
  });

  it('终端使用说明作摘要且展开保留完整命令与工作目录', () => {
    renderTool(tool('exec_command', { input: JSON.stringify({ cmd: 'npm test\nnpm run build' }), output: 'ok', execCommand: { workdir: '/workspace' } }, { description: '运行检查', cmd: 'npm test…' }));
    expect(screen.getByRole('button')).toHaveTextContent('运行检查');
    expect(screen.getByRole('button')).not.toHaveTextContent('npm test');
    openTool();
    expect(screen.getByText('/workspace')).toBeInTheDocument();
    expect(screen.getByText(/\$ npm test/)).toHaveTextContent('npm run build');
  });
});
