// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ToolGroupBlock } from '../../components/chat/ToolGroupBlock';
import { ActivityIcon, activityIconFamily } from '../../components/chat/MessageActivity';
import { BUILTIN_TOOL_NAMES, BUNDLED_PLUGIN_TOOL_NAMES } from '../../utils/tool-label';
import { ThinkingBlock } from '../../components/chat/ThinkingBlock';
import type { ToolCall } from '../../stores/chat-types';
import { useStore } from '../../stores';
import { clearDeferredHistoryContentCacheForTests } from '../../hooks/use-deferred-history-content';
import { openInternalLink } from '../../utils/link-open';
const activityCss = readFileSync(path.join(process.cwd(), 'desktop/src/react/components/chat/MessageActivity.module.css'), 'utf8');
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

  it('查看弹窗 portal 到 body 后仍自带内边距，不依赖 .activity 子树里的局部 token', () => {
    renderTool(tool('read', { input: '{"path":"a.ts"}', output: 'content' }));
    openTool();
    fireEvent.click(screen.getByRole('button', { name: 'messageActivity.view' }));
    const dialog = screen.getByRole('dialog');
    // 局部 token（--ma-pad-x / --ma-dialog-pad）定义在 .activity 上，portal 后不再继承；
    // 弹窗自身的盒模型声明必须带 var() 回退字面量，否则 padding 整条失效（渲染成 0）。
    const dialogRule = /\.dialog \{([^}]*)\}/.exec(activityCss)?.[1] ?? '';
    expect(dialogRule).toMatch(/padding:\s*var\(--ma-dialog-pad,\s*[^)]+\)/);
    expect(dialogRule).not.toMatch(/padding:\s*var\(--ma-dialog-pad\)\s*;/);
    expect(dialogRule).toMatch(/box-shadow:\s*var\(--shadow-2xl,\s*[^)]+\)/);
    // 关闭按钮与弹窗头部落在 .activity 之外，样式同样不能依赖局部作用域的 class。
    const headerRule = /\.header \{([^}]*)\}/.exec(activityCss)?.[1] ?? '';
    expect(headerRule).toMatch(/padding:\s*var\(--space-8\)\s+var\(--ma-pad-x,\s*[^)]+\)/);
    expect(document.body.contains(dialog)).toBe(true);
    // .activity 不在弹窗祖先链上：内边距只能来自弹窗自身带回退的声明。
    expect(dialog.closest('[class*="activity"]')).toBeNull();
  });

  it('工具行按家族给图标，未匹配的家族才落通用网格', () => {
    // 家族图标写在 data-activity-family 上，data-activity-icon 仍是原工具名（选择器/排障不变）。
    const familyOf = (name: string) => {
      renderTool(tool(name));
      const svg = document.querySelector(`[data-activity-icon="${name}"]`);
      expect(svg, `${name} 的行没有图标`).toBeInTheDocument();
      return svg!.getAttribute('data-activity-family');
    };

    // 记忆族
    for (const name of ['search_memory', 'pin_memory', 'unpin_memory', 'recall_experience', 'record_experience', 'tenet_propose']) {
      expect(familyOf(name), name).toBe('memory');
      cleanup();
    }
    // 回归：search_memory 曾经落通用网格（格子图标），现在必须是记忆族气泡
    renderTool(tool('search_memory'));
    const memorySvg = document.querySelector('[data-activity-icon="search_memory"]');
    expect(memorySvg).toHaveAttribute('data-activity-icon', 'search_memory');
    expect(memorySvg).toHaveAttribute('data-activity-family', 'memory');
    expect(memorySvg!.querySelectorAll('rect')).toHaveLength(0);
    expect(memorySvg!.querySelectorAll('path')).toHaveLength(1);
    cleanup();
    // 知识族：knowledge_read 在旧正则下会被 /read/ 抢走，必须排在前面
    for (const name of ['knowledge_search', 'knowledge_read', 'knowledge_outline', 'knowledge_grep', 'knowledge_manage', 'knowledge_local_search', 'knowledge_research_progress']) {
      expect(familyOf(name), name).toBe('knowledge');
      cleanup();
    }
    // 频道 / 通知 / 浏览器 / 电脑 / 文件 / 自动化 / 子代理 / 停止
    const families: Array<[string, string]> = [
      ['channel_read_context', 'channel'], ['channel_reply', 'channel'], ['channel_pass', 'channel'], ['channel', 'channel'],
      ['notify', 'notify'],
      ['browser', 'browser'],
      ['computer', 'computer'],
      ['file', 'file'], ['materialize', 'file'],
      ['automation', 'automation'],
      ['subagent_reply', 'subagent'], ['subagent_close', 'subagent'],
      ['stop_task', 'stop'],
    ];
    for (const [name, family] of families) {
      expect(familyOf(name), name).toBe(family);
      cleanup();
    }
    // 未匹配家族的（MCP / 第三方插件 / 未知工具名）维持通用网格
    for (const name of ['mcp_deep-search', 'mcp_search_issues', 'acme_read', 'brand_new_builtin_tool']) {
      expect(familyOf(name), name).toBe('grid');
      cleanup();
    }
  });

  it('内置工具名都有图标家族，落网格的必须显式豁免', () => {
    // 对账口径与 tool-label 的 BUILTIN_TOOL_NAMES 共用一份名单：新工具漏配家族图标
    // 会静默变成通用网格，这里把它变成红灯。
    const exempt = new Set([
      'todo_write',       // 清单面板工具，行图标由面板标题语义决定，不做专属家族
      'stage_files',      // 卡片承载，不进进程区
      'subagent', 'show_card', 'hana_card_guide', 'workflow', 'install_skill', 'update_settings',
      'present_files',    // 已下线，历史 JSONL 里的旧调用
      ...BUNDLED_PLUGIN_TOOL_NAMES,
    ]);
    const unmapped = [...BUILTIN_TOOL_NAMES]
      .filter(name => !exempt.has(name) && activityIconFamily(name) === 'grid')
      .sort();
    expect(unmapped, '这些内置工具会落通用网格图标，请补家族或显式豁免').toEqual([]);
    // 反向：豁免名单里的名字必须真的是内置工具，防止名单过期后成为垃圾抽屉
    expect([...exempt].filter(name => !BUILTIN_TOOL_NAMES.has(name))).toEqual([]);
  });

  it('原有核心工具的图标家族不变', () => {
    const families: Array<[string, string]> = [
      ['read', 'read'], ['skill', 'skill'], ['write', 'edit'], ['edit', 'edit'],
      ['grep', 'search'], ['find', 'search'], ['ls', 'ls'],
      ['bash', 'terminal'], ['exec_command', 'terminal'], ['write_stdin', 'terminal'], ['terminal', 'terminal'],
      ['thinking', 'thinking'],
    ];
    for (const [name, family] of families) {
      const { container } = render(<ActivityIcon kind={name} />);
      const svg = container.querySelector('svg');
      expect(svg?.getAttribute('data-activity-family'), name).toBe(family);
      cleanup();
    }
  });
});
