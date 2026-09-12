import { describe, expect, it } from 'vitest';
import { projectFullHistoryPage } from '../server/history-read/index.ts';
import { createHistoryDeferredContentFor, createLiveToolContentDescriptor, resolveHistoryDeferredContent } from '../server/history-deferred-content.ts';
import { projectLiveToolResultOutcome } from '../shared/tool-outcome.ts';
import { TOOL_PRESENTATION_TEXT_LIMIT } from '../shared/tool-presentation.ts';

const SESSION_PATH = '/tmp/agents/demo/sessions/history.jsonl';

async function page(sourceMessages: any[]) {
  return projectFullHistoryPage({}, {
    sessionPath: null, sourceMessages, beforeId: null, limit: 50, forceAll: true,
    sanitizeVisibleContent: value => value,
  });
}

function call(id: string, name: string, args: Record<string, unknown>) {
  return { type: 'toolCall', id, name, arguments: args };
}

/**
 * 实时 → 保存 → 历史首包 → deferred 的完整链路。
 * 实时引用（v2 locator，按 toolCallId 定位）与历史 locator（v1）都要能解析出
 * 保存记录里的同一份内容。
 */
function liveThenSaved(
  result: any,
  toolName: string,
  args: Record<string, unknown>,
  toolCallId: string,
) {
  let live: any;
  const capture = { sessionPath: SESSION_PATH, toolCallId };
  const outcome = projectLiveToolResultOutcome(result, { toolName, args }, {
    create: (details) => {
      const outputDeferred = createLiveToolContentDescriptor(capture.sessionPath, capture.toolCallId, 'tool_output', details.output?.length ?? 0);
      if (!outputDeferred) return undefined;
      const search = details.search as any;
      const searchDeferred = search && Array.isArray(search.files) && search.files.length
        ? createLiveToolContentDescriptor(capture.sessionPath, capture.toolCallId, 'tool_search', JSON.stringify(search).length)
        : null;
      return { outputDeferred, ...(searchDeferred ? { searchDeferred } : {}) };
    },
  });
  live = outcome.details;
  const sources = [
    { id: 'assistant-live', role: 'assistant', content: [call(toolCallId, toolName, args)] },
    { id: 'result-live', role: 'toolResult', toolCallId, toolName, ...result },
  ];
  return { live, sources };
}

describe('工具详情历史恢复', () => {
  it('实时终态与历史短详情一致，保留多文本结果和真实修改', async () => {
    const args = { path: '/not-a-current-file.ts', edits: [{ oldText: 'old', newText: 'new' }] };
    const result = {
      content: [{ type: 'text', text: 'changed' }, { type: 'text', text: 'registered' }],
      details: { fileChange: { path: args.path, patch: '@@ -1 +1 @@\n-old\n+new', beforeAvailable: true, changeType: 'modified' } },
    };
    const sources = [
      { id: 'assistant', role: 'assistant', content: [call('edit-1', 'edit', args)] },
      { id: 'result', role: 'toolResult', toolCallId: 'edit-1', toolName: 'edit', ...result },
    ];
    const history = await page(sources);
    expect(history.messages[0].toolCalls[0].details).toEqual(projectLiveToolResultOutcome(result, { toolName: 'edit', args }).details);
  });

  it('大输入、补丁和写入内容首包仅预览，详情按保存记录完整恢复', async () => {
    const patch = '@@ -1 +1 @@\n-old\n+' + 'p'.repeat(90_000) + ':patch-end';
    const content = 'c'.repeat(90_000) + ':content-end';
    const args = { path: '/historical.ts', content, apiKey: 'credential-sentinel' };
    const sources = [
      { id: 'assistant', role: 'assistant', content: [call('write-1', 'write', args)] },
      { id: 'result', role: 'toolResult', toolCallId: 'write-1', toolName: 'write', content: [{ type: 'text', text: 'written' }],
        details: { fileChange: { path: args.path, patch, content, beforeAvailable: true, changeType: 'modified' } } },
    ];
    const history = await page(sources);
    const details = history.messages[0].toolCalls[0].details;
    expect(JSON.stringify(history)).not.toContain(':patch-end');
    expect(JSON.stringify(history)).not.toContain(':content-end');
    expect(JSON.stringify(history)).not.toContain('credential-sentinel');
    expect(() => JSON.parse(details.input)).not.toThrow();
    expect(details.inputDeferred.kind).toBe('tool_input');
    expect(details.fileChange.patchDeferred.kind).toBe('tool_patch');
    expect(details.fileChange.contentDeferred.kind).toBe('tool_file_content');
    expect(details.fileChange.truncated).not.toBe(true);
    expect(details.fileChange).toMatchObject({ added: 1, removed: 1 });
    const input = resolveHistoryDeferredContent(sources, details.inputDeferred.id)!;
    expect(JSON.parse(input.content)).toEqual({ ...args, apiKey: '********' });
    expect(resolveHistoryDeferredContent(sources, details.fileChange.patchDeferred.id)?.content).toBe(patch);
    expect(resolveHistoryDeferredContent(sources, details.fileChange.contentDeferred.id)?.content).toBe(content);
  });

  it('旧写入从调用参数恢复新正文，不捏造覆盖前内容', async () => {
    const content = 'old saved text\n'.repeat(1000);
    const sources = [
      { id: 'assistant-old', role: 'assistant', content: [call('write-old', 'write', { path: '/missing-now.txt', content })] },
      { id: 'result-old', role: 'toolResult', toolCallId: 'write-old', toolName: 'write', content: [{ type: 'text', text: 'written' }] },
    ];
    const details = (await page(sources)).messages[0].toolCalls[0].details;
    expect(details.fileChange).toMatchObject({ beforeAvailable: false, reason: 'before_content_unavailable' });
    expect(details.fileChange).not.toHaveProperty('patch');
    expect(resolveHistoryDeferredContent(sources, details.fileChange.contentDeferred.id)?.content).toBe(content);
  });

  it('大搜索只传计数预览，展开恢复所有文本块而不包含图片数据', async () => {
    const lines = Array.from({ length: 700 }, (_, index) => 'a.ts:' + (index + 1) + ': match ' + index);
    const output = lines.join('\n');
    const sources = [
      { id: 'assistant-search', role: 'assistant', content: [call('grep-1', 'grep', { pattern: 'match' })] },
      { id: 'result-search', role: 'toolResult', toolCallId: 'grep-1', toolName: 'grep',
        content: [{ type: 'text', text: lines.slice(0, 350).join('\n') }, { type: 'image', data: 'binary-sentinel' }, { type: 'text', text: lines.slice(350).join('\n') }],
        details: { search: { kind: 'grep', basePath: '/saved-search-root' } } },
    ];
    const details = (await page(sources)).messages[0].toolCalls[0].details;
    expect(details.search).toMatchObject({ kind: 'grep', basePath: '/saved-search-root', fileCount: 1, matchCount: 700 });
    expect(details.search.files).toBeUndefined();
    expect(details.output).toHaveLength(240);
    expect(resolveHistoryDeferredContent(sources, details.outputDeferred.id)?.content).toBe(output);
    expect(JSON.stringify(details)).not.toContain('binary-sentinel');
  });

  it('伪造条目身份、错误工具类型和失败改动均不能恢复详情', () => {
    const assistant = { id: 'assistant', role: 'assistant', content: [call('secret', 'knowledge_research_worker', { prompt: 'hidden' })] };
    const synthetic = createHistoryDeferredContentFor(assistant, 0, 'tool_input', 0, 'hidden');
    expect(resolveHistoryDeferredContent([assistant], synthetic.id)).toBeNull();
    const result = { id: 'result', role: 'toolResult', toolName: 'edit', isError: true,
      details: { fileChange: { path: 'a', patch: '+never written', beforeAvailable: true } } };
    const descriptor = createHistoryDeferredContentFor(result, 0, 'tool_patch', 0, '+never written');
    expect(resolveHistoryDeferredContent([result], descriptor.id)).toBeNull();
    expect(resolveHistoryDeferredContent([{ ...result, id: 'different' }], descriptor.id)).toBeNull();
  });

  it('更换 locator kind 不能读取普通工具的原始凭证或合成研究输出', () => {
    for (const toolName of ['mcp_lookup', 'knowledge_research_worker']) {
      const assistant = { id: 'assistant', role: 'assistant', content: [call('call', toolName, { prompt: 'private' })] };
      const result = { id: 'result', role: 'toolResult', toolCallId: 'call', toolName,
        content: [{ type: 'text', text: '{"token":"credential-sentinel"}' }] };
      const sources = [assistant, result];
      for (const kind of ['skill_content', 'assistant_segment'] as const) {
        const rawKind = createHistoryDeferredContentFor(result, 1, kind, 0, 'ignored');
        expect(resolveHistoryDeferredContent(sources, rawKind.id)).toBeNull();
      }
      const outputKind = createHistoryDeferredContentFor(result, 1, 'tool_output', 0, 'ignored');
      const resolved = resolveHistoryDeferredContent(sources, outputKind.id);
      if (toolName === 'mcp_lookup') expect(JSON.parse(resolved!.content)).toEqual({ token: '********' });
      else expect(resolved).toBeNull();
    }
  });

  it('真实配对的技能读取仍能恢复完整内容', () => {
    const content = '# Skill\n' + 'x'.repeat(9000);
    const sources = [
      { id: 'assistant', role: 'assistant', content: [call('skill', 'read', { path: '/skills/example/SKILL.md' })] },
      { id: 'result', role: 'toolResult', toolCallId: 'skill', toolName: 'read', content: [{ type: 'text', text: content }] },
    ];
    const descriptor = createHistoryDeferredContentFor(sources[1], 1, 'skill_content', 0, content);
    expect(resolveHistoryDeferredContent(sources, descriptor.id)?.content).toBe(content);
  });
});

describe('B. 搜索结构化事实跨实时 / 首包 / deferred 保持一致', () => {
  const STRUCTURED_FILES = [
    { path: 'src/a.ts', matches: [
      { line: 6, text: 'Error at other.ts:123: boom', context: true },
      { line: 7, text: 'target' },
    ] },
    { path: 'C:\\work\\b.ts', matches: [{ line: 12, text: 'win hit' }] },
  ];
  const AMBIGUOUS_OUTPUT = [
    'src/a.ts-6- Error at other.ts:123: boom',
    'src/a.ts:7: target',
    'C:\\work\\b.ts:12: win hit',
  ].join('\n');

  function grepSources(pad: number) {
    const padding = pad > 0 ? '\n' + 'x'.repeat(pad) : '';
    return [
      { id: 'assistant-grep', role: 'assistant', content: [call('grep-1', 'grep', { pattern: 'target' })] },
      { id: 'result-grep', role: 'toolResult', toolCallId: 'grep-1', toolName: 'grep',
        content: [{ type: 'text', text: AMBIGUOUS_OUTPUT + padding }],
        details: { search: { kind: 'grep', basePath: '/root', files: STRUCTURED_FILES, matchCount: 2, fileCount: 2 } } },
    ];
  }

  it('首包低于/超过 deferred 阈值时，路径 / 行号 / context / 统计完全一致', async () => {
    const small = (await page(grepSources(0))).messages[0].toolCalls[0].details;
    const large = (await page(grepSources(20_000))).messages[0].toolCalls[0].details;

    // 小结果：结构随首包直达，不产生引用。
    expect(small.search.files).toEqual(STRUCTURED_FILES);
    expect(small.search.searchDeferred).toBeUndefined();
    expect(small).toMatchObject({ search: { matchCount: 2, fileCount: 2 } });

    // 大结果：首包省掉 files（有界），但统计与真相不变，并给出可加载引用。
    expect(large.outputDeferred).toMatchObject({ kind: 'tool_output' });
    expect(large.search.files).toBeUndefined();
    expect(large.search).toMatchObject({ kind: 'grep', basePath: '/root', matchCount: 2, fileCount: 2 });
    expect(large.search.searchDeferred).toMatchObject({ kind: 'tool_search', available: true });

    const restored = JSON.parse(resolveHistoryDeferredContent(grepSources(20_000), large.search.searchDeferred.id)!.content);
    // 恢复出来的结构与小结果逐字一致：path / line / context / 统计都不是猜的。
    expect(restored.files).toEqual(STRUCTURED_FILES);
    expect(restored).toMatchObject({ kind: 'grep', basePath: '/root', matchCount: 2, fileCount: 2 });
    expect(restored).toEqual(small.search);
  });

  it('上下文行里的 other.ts:123: 不会被误判成路径与行号', async () => {
    const details = (await page(grepSources(20_000))).messages[0].toolCalls[0].details;
    const restored = JSON.parse(resolveHistoryDeferredContent(grepSources(20_000), details.search.searchDeferred.id)!.content);
    const [first] = restored.files;
    expect(first.path).toBe('src/a.ts');
    expect(first.matches).toEqual([
      { line: 6, text: 'Error at other.ts:123: boom', context: true },
      { line: 7, text: 'target' },
    ]);
    // 对照 legacy parser：同样输入会被解析成 src/a.ts-6- Error at other.ts / line 123。
    expect(restored.files).not.toEqual([
      { path: 'src/a.ts-6- Error at other.ts', matches: [{ text: 'boom', line: 123 }] },
    ]);
  });

  it('首包不把刚刚省掉的搜索结构用 preview 原样塞回来（仍然有界）', async () => {
    const sources = grepSources(20_000);
    const details = (await page(sources)).messages[0].toolCalls[0].details;
    expect(details.search.searchDeferred.preview).toBeUndefined();
    expect(JSON.stringify(details.search)).not.toContain('other.ts:123');
  });

  it('搜索结构引用不能跨会话解析（locator 不是授权凭证）', async () => {
    const details = (await page(grepSources(20_000))).messages[0].toolCalls[0].details;
    const sources = grepSources(20_000);
    // v1 locator 仍按 entryId 校验
    expect(resolveHistoryDeferredContent([{ ...sources[1], id: 'other' }], details.search.searchDeferred.id)).toBeNull();
    // 实时 v2 locator 绑定的会话必须与本次读取的会话一致
    const live = createLiveToolContentDescriptor(SESSION_PATH, 'grep-1', 'tool_search', 10) as { id: string };
    expect(resolveHistoryDeferredContent(sources, live.id, SESSION_PATH)).toMatchObject({ kind: 'tool_search' });
    expect(resolveHistoryDeferredContent(sources, live.id, '/tmp/agents/other/sessions/x.jsonl')).toBeNull();
    expect(resolveHistoryDeferredContent(sources, live.id, null)).toBeNull();
  });

  it('实时引用不接受未签发的 kind，也不能用 locator 绕过普通工具的字段遮盖', () => {
    const sources = [
      { id: 'assistant-x', role: 'assistant', content: [call('call-x', 'mcp_lookup', { prompt: 'private' })] },
      { id: 'result-x', role: 'toolResult', toolCallId: 'call-x', toolName: 'mcp_lookup',
        content: [{ type: 'text', text: '{"token":"AUDIT_FAKE_TOKEN"}' }] },
    ];
    // 只签发 tool_output / tool_search；补丁、写入正文、技能正文都解不出来。
    for (const kind of ['tool_patch', 'tool_file_content', 'skill_content', 'artifact'] as const) {
      const forged = Buffer.from(JSON.stringify({
        version: 2, sessionPath: SESSION_PATH, toolCallId: 'call-x', kind,
      }), 'utf8').toString('base64url');
      expect(resolveHistoryDeferredContent(sources, forged, SESSION_PATH), kind).toBeNull();
    }
    // 合法 kind 也仍然走普通工具的字段遮盖，locator 不是绕过遮盖的后门。
    const output = createLiveToolContentDescriptor(SESSION_PATH, 'call-x', 'tool_output', 1) as { id: string };
    const resolved = resolveHistoryDeferredContent(sources, output.id, SESSION_PATH)!;
    expect(JSON.parse(resolved.content)).toEqual({ token: '********' });
    expect(resolved.content).not.toContain('AUDIT_FAKE_TOKEN');
  });

  it('真正没有结构化 search metadata 的旧记录仍走 legacy parser', async () => {
    const sources = [
      { id: 'assistant-old', role: 'assistant', content: [call('grep-old', 'grep', { pattern: 'target' })] },
      { id: 'result-old', role: 'toolResult', toolCallId: 'grep-old', toolName: 'grep',
        content: [{ type: 'text', text: 'src/a.ts:7: target' }] },
    ];
    const details = (await page(sources)).messages[0].toolCalls[0].details;
    // 没有结构化 metadata 时投影退回文本解析（这是唯一允许猜测的入口）。
    expect(details.search).toMatchObject({ kind: 'grep', fileCount: 1, matchCount: 1 });
    expect(details.search.files).toEqual([{ path: 'src/a.ts', matches: [{ line: 7, text: 'target' }] }]);
    expect(details.search.searchDeferred).toBeUndefined();
  });
});

describe('C. 大内容完整性：实时与历史最终同语义', () => {
  function largeOutput(marker: string) {
    return 'x'.repeat(TOOL_PRESENTATION_TEXT_LIMIT + 1_000) + marker;
  }

  it('64KB 以下：无 deferred，行为保持原样', async () => {
    const small = 'y'.repeat(1_000);
    const { live, sources } = liveThenSaved(
      { content: [{ type: 'text', text: small }] }, 'exec_command', { cmd: 'run' }, 'call-small',
    );
    expect(live.outputDeferred).toBeUndefined();
    expect(live.output).toBe(small);
    const history = (await page(sources)).messages[0].toolCalls[0].details;
    expect(history.output).toBe(small);
    expect(history.outputDeferred).toBeUndefined();
  });

  it('超过 64KB：首屏有界 + 明确标记预览 + 合法引用；展开取得全文；复制用完整正文', async () => {
    const full = largeOutput(':FULL-END');
    const { live, sources } = liveThenSaved(
      { content: [{ type: 'text', text: full }] }, 'exec_command', { cmd: 'run' }, 'call-large',
    );
    // 首屏有界：WS 里只有预览，没有全文，也没有把全文塞进引用。
    expect(live.output).toHaveLength(TOOL_PRESENTATION_TEXT_LIMIT);
    expect(live.outputTruncated).toBe(true);
    expect(JSON.stringify(live)).not.toContain(':FULL-END');
    expect(live.outputDeferred).toMatchObject({ kind: 'tool_output', available: true });
    expect(live.outputDeferred.size).toBe(TOOL_PRESENTATION_TEXT_LIMIT);

    // 引用可解析出保存记录里的全文 —— 实时阶段就能拿到完整已记录内容。
    const liveResolved = resolveHistoryDeferredContent(sources, live.outputDeferred.id, SESSION_PATH);
    expect(liveResolved?.content).toBe(full);

    // 历史重开后是同一份全文。
    const history = (await page(sources)).messages[0].toolCalls[0].details;
    const historyResolved = resolveHistoryDeferredContent(sources, history.outputDeferred.id, SESSION_PATH);
    expect(historyResolved?.content).toBe(liveResolved?.content);
    expect(historyResolved?.content).toBe(full);
  });

  it('延迟加载失败不伪装成全文：拿不到记录就是 null，不退回预览', async () => {
    const full = largeOutput(':FULL-END');
    const { live, sources } = liveThenSaved(
      { content: [{ type: 'text', text: full }] }, 'exec_command', { cmd: 'run' }, 'call-missing',
    );
    // 记录还在：引用能取出全文。
    expect(resolveHistoryDeferredContent(sources, live.outputDeferred.id, SESSION_PATH)?.content).toBe(full);

    // 引用指向的调用不存在（会话被裁掉 / 引用过期）→ 解析失败，返回 null。
    // 调用方（useDeferredHistoryContent → ToolGroupBlock）据此显示 loadFailed 并禁用复制，
    // 绝不能拿 details.output 里的截断预览冒充"完整已记录内容"。
    const withoutResult = sources.filter(source => source.role !== 'toolResult');
    expect(resolveHistoryDeferredContent(withoutResult, live.outputDeferred.id, SESSION_PATH)).toBeNull();
    expect(live.outputTruncated).toBe(true);
    expect(live.output).not.toContain(':FULL-END');
  });

  it('引用绑定的是调用身份：同一次调用被改名后的伪造条目不能顶替', async () => {
    const full = largeOutput(':FULL-END');
    const { live, sources } = liveThenSaved(
      { content: [{ type: 'text', text: full }] }, 'exec_command', { cmd: 'run' }, 'call-real',
    );
    const forged = [
      sources[0],
      { ...sources[1], toolCallId: 'call-other', content: [{ type: 'text', text: 'forged' }] },
    ];
    expect(resolveHistoryDeferredContent(forged, live.outputDeferred.id, SESSION_PATH)).toBeNull();
    expect(resolveHistoryDeferredContent(sources, live.outputDeferred.id, SESSION_PATH)?.content).toBe(full);
  });

  it('超过 256KB 的 write：不把截断的 fileChange.content 当完整结果', async () => {
    // 生产端已经把正文截到 256KB 并标记 truncated；历史恢复不能假装它是全文。
    const applied = 'c'.repeat(300 * 1024);
    const retained = applied.slice(0, 256 * 1024);
    const sources = [
      { id: 'assistant-write', role: 'assistant', content: [call('write-1', 'write', { path: '/big.ts', content: applied })] },
      { id: 'result-write', role: 'toolResult', toolCallId: 'write-1', toolName: 'write',
        content: [{ type: 'text', text: 'written' }],
        details: { fileChange: { path: '/big.ts', content: retained, beforeAvailable: false, truncated: true, reason: 'diff_too_large' } } },
    ];
    const details = (await page(sources)).messages[0].toolCalls[0].details;
    // 预览有界，且明确标记截断 —— 不能声称完整。
    expect(details.fileChange.content.length).toBeLessThanOrEqual(TOOL_PRESENTATION_TEXT_LIMIT);
    expect(details.fileChange.truncated).toBe(true);
    // 引用恢复的是**保存记录里保留的那份有界正文**，不是被截断后又被当成全文的东西。
    const resolved = resolveHistoryDeferredContent(sources, details.fileChange.contentDeferred.id, SESSION_PATH);
    expect(resolved?.content).toBe(retained);
    expect(resolved?.content).not.toBe(applied);
    expect(details.fileChange.truncated).toBe(true);
  });

  it('edit 大 patch：preview 有界；完整 patch 按需加载；增删统计不猜总数', async () => {
    const patch = '--- a/a.ts\n+++ b/a.ts\n@@ -1,1 +1,2 @@\n-old\n+' + 'p'.repeat(90_000) + '\n+new\n';
    const sources = [
      { id: 'assistant-edit', role: 'assistant', content: [call('edit-1', 'edit', { path: '/a.ts', edits: [] })] },
      { id: 'result-edit', role: 'toolResult', toolCallId: 'edit-1', toolName: 'edit',
        content: [{ type: 'text', text: 'applied' }],
        details: { patch, fileChange: { path: '/a.ts', patch, beforeAvailable: true } } },
    ];
    const details = (await page(sources)).messages[0].toolCalls[0].details;
    expect(details.fileChange.patch.length).toBeLessThanOrEqual(240);
    expect(details.fileChange.patchDeferred).toMatchObject({ kind: 'tool_patch' });
    // 统计来自完整补丁（保存记录里那份），不是从截断预览猜的。
    expect(details.fileChange).toMatchObject({ added: 2, removed: 1 });
    expect(details.fileChange.truncated).not.toBe(true);
    expect(resolveHistoryDeferredContent(sources, details.fileChange.patchDeferred.id, SESSION_PATH)?.content).toBe(patch);
  });
});
