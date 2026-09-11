import { describe, expect, it } from 'vitest';
import { projectFullHistoryPage } from '../server/history-read/index.ts';
import { createHistoryDeferredContentFor, resolveHistoryDeferredContent } from '../server/history-deferred-content.ts';
import { projectLiveToolResultOutcome } from '../shared/tool-outcome.ts';

async function page(sourceMessages: any[]) {
  return projectFullHistoryPage({}, {
    sessionPath: null, sourceMessages, beforeId: null, limit: 50, forceAll: true,
    sanitizeVisibleContent: value => value,
  });
}

function call(id: string, name: string, args: Record<string, unknown>) {
  return { type: 'toolCall', id, name, arguments: args };
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
