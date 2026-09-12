import { describe, expect, it } from 'vitest';
import {
  parseToolSearchOutput,
  projectToolPresentationDetails,
  projectToolStartDetails,
  safeToolInput,
  TOOL_PRESENTATION_TEXT_LIMIT,
  toolPatchStats,
  toolResultText,
} from '../shared/tool-presentation.ts';
import { collectToolOutcomesByCallId } from '../shared/tool-outcome.ts';

const GENERIC_CONTEXT = { toolName: 'mcp_lookup', args: {} };

/** 通用工具结果的展示文本（走逐块安全投影那条链）。 */
function genericOutput(content: unknown[], context: Record<string, unknown> = GENERIC_CONTEXT): string {
  const details = projectToolPresentationDetails({ content } as never, context as never);
  return details?.output ?? '';
}
const text = (value: string) => ({ type: 'text', text: value });

describe('A. 敏感字段遮盖矩阵（块边界不能被绕过）', () => {
  it('单个 JSON 块遮盖敏感字段', () => {
    expect(genericOutput([text('{"token":"AUDIT_FAKE_TOKEN"}')]))
      .toBe('{\n  "token": "********"\n}');
  });

  it('JSON 块后面跟普通说明：token 仍遮盖，说明仍保留', () => {
    const output = genericOutput([text('{"token":"AUDIT_FAKE_TOKEN"}'), text('Operation completed')]);
    expect(output).toContain('"token": "********"');
    expect(output).not.toContain('AUDIT_FAKE_TOKEN');
    // 普通说明不能被当成"疑似 secret"删掉，也不能把整块结果标记成不可展示。
    expect(output).toContain('Operation completed');
  });

  it('两个各自合法的 JSON 块分别遮盖', () => {
    const output = genericOutput([text('{"token":"AUDIT_FAKE_TOKEN"}'), text('{"apiKey":"AUDIT_FAKE_KEY"}')]);
    expect(output).not.toContain('AUDIT_FAKE_TOKEN');
    expect(output).not.toContain('AUDIT_FAKE_KEY');
    expect(output).toContain('"token": "********"');
    expect(output).toContain('"apiKey": "********"');
  });

  it('text / image / text 混排：图片二进制不进文本详情，两侧内容各自处理', () => {
    const output = genericOutput([
      text('{"accessToken":"AUDIT_FAKE_TOKEN"}'),
      { type: 'image', data: 'AUDIT_FAKE_BINARY' },
      text('tail text'),
    ]);
    expect(output).not.toContain('AUDIT_FAKE_BINARY');
    expect(output).not.toContain('AUDIT_FAKE_TOKEN');
    expect(output).toContain('"accessToken": "********"');
    expect(output).toContain('tail text');
  });

  it('嵌套凭证按字段身份遮盖，不碰同层的普通字段', () => {
    const output = genericOutput([text(JSON.stringify({
      headers: { Authorization: 'AUDIT_FAKE_AUTH', Accept: 'text/plain' },
      cookie: 'AUDIT_FAKE_COOKIE',
    }))]);
    for (const secret of ['AUDIT_FAKE_AUTH', 'AUDIT_FAKE_COOKIE']) expect(output).not.toContain(secret);
    expect(output).toContain('"Authorization": "********"');
    expect(output).toContain('"Accept": "text/plain"');
  });

  it('设置类 key/value 结构：值被遮盖，键名保留', () => {
    const output = genericOutput([text(JSON.stringify({ key: 'providers.demo.api_key', value: 'AUDIT_FAKE_KEY' }))]);
    expect(output).not.toContain('AUDIT_FAKE_KEY');
    expect(output).toContain('providers.demo.api_key');
    expect(output).toContain('"value": "********"');
  });

  it('普通代码正文不被遮盖破坏，文件工具正文保持原样', () => {
    const code = '{"code":"const accessToken = readToken();"}';
    expect(genericOutput([text(code)])).toContain('const accessToken = readToken();');
    // read / write / edit 等文件工具正文必须逐字保留（否则写出的代码会被改坏）。
    const patch = '@@ -1 +1 @@\n-const token = 1;\n+const token = 2;';
    expect(projectToolPresentationDetails({
      content: [text(patch)], details: { fileChange: { path: 'a.ts', patch, beforeAvailable: true } },
    } as never, { toolName: 'edit', args: { path: 'a.ts' } } as never)?.output).toBe(patch);
  });
});

describe('工具详情展示投影', () => {
  it('按字段遮盖凭证和设置值，保留正文中的真实代码', () => {
    const content = 'const token = "file content";';
    const input = safeToolInput('write', {
      path: 'a.ts', content, apiKey: 'sentinel-api-credential',
      headers: { Authorization: 'sentinel-auth-credential', Accept: 'text/plain' },
      changes: [{ key: 'providers.demo.api_key', value: 'sentinel-settings-credential' }],
      thumbnail: 'sentinel-binary-data',
    })!;
    const parsed = JSON.parse(input.input!);
    expect(parsed).toMatchObject({
      content, apiKey: '********', headers: { Authorization: '********', Accept: 'text/plain' },
      changes: [{ value: '********' }], thumbnail: '[Content omitted]',
    });
    expect(input.input).not.toContain('sentinel-');
  });

  it('长输入仍为有效 JSON，并明确标记预览', () => {
    const input = safeToolInput('write', { content: '"\n'.repeat(80_000) })!;
    expect(input.inputTruncated).toBe(true);
    expect(input.input!.length).toBeLessThanOrEqual(TOOL_PRESENTATION_TEXT_LIMIT);
    expect(JSON.parse(input.input!)).toMatchObject({ truncated: true, preview: expect.any(String) });
  });

  it('不执行输入访问器，也不把合成研究卡内部状态放入详情', () => {
    const args = Object.defineProperty({ query: 'visible' }, 'secretGetter', {
      enumerable: true, get: () => { throw new Error('must not run'); },
    });
    expect(JSON.parse(safeToolInput('mcp_lookup', args)!.input!)).toEqual({ query: 'visible' });
    expect(projectToolStartDetails('knowledge_research_worker', { prompt: 'hidden', coverage: 'hidden' })).toBeUndefined();
    expect(projectToolPresentationDetails({ content: [{ type: 'text', text: 'hidden' }] }, {
      toolName: 'knowledge_research_round', args: { prompt: 'hidden' },
    })).toBeUndefined();
  });

  it('拼接所有文本块并排除图片二进制，普通 JSON 输出遮盖凭证', () => {
    const result = { content: [
      { type: 'text', text: 'first' }, { type: 'image', data: 'image-sentinel' }, { type: 'text', text: 'second' },
    ] };
    expect(toolResultText(result)).toBe('first\nsecond');
    expect(projectToolPresentationDetails({ content: [{ type: 'text', text: '{"token":"sentinel","count":3}' }] }, {
      toolName: 'mcp_lookup',
    })).toMatchObject({ output: '{\n  "token": "********",\n  "count": 3\n}' });
  });

  it('读取只复制已验证的范围字段，不透传内部详情', () => {
    const details = projectToolPresentationDetails({
      content: [{ type: 'text', text: 'first\nsecond' }],
      details: {
        read: { path: 'a.ts', startLine: 20, totalLines: 70, displayedLines: 2, language: 'ts', secret: 'hidden' },
        credentials: 'hidden',
      },
    }, { toolName: 'read', args: { path: 'a.ts', offset: 20, limit: 2 } });
    expect(details).toMatchObject({ output: 'first\nsecond', read: { path: 'a.ts', startLine: 20, totalLines: 70, displayedLines: 2 } });
    expect(JSON.stringify(details)).not.toContain('hidden');
  });

  it('无正文及图片读取不伪装成文本文件范围', () => {
    for (const content of [[], [{ type: 'image', data: 'binary' }], [
      { type: 'text', text: 'Read image file [image/png]' }, { type: 'image', data: 'binary' },
    ]]) {
      expect(projectToolPresentationDetails({ content }, { toolName: 'read', args: { path: 'a.png' } })).not.toHaveProperty('read');
    }
  });

  it('真实空读取标为零行，但不会把文件末尾空行标为空文件', () => {
    expect(projectToolPresentationDetails({ content: [{ type: 'text', text: '' }] }, {
      toolName: 'read', args: { path: 'empty.txt' },
    })?.read).toMatchObject({ totalLines: 0, displayedLines: 0 });
    expect(projectToolPresentationDetails({
      content: [{ type: 'text', text: '' }],
      details: { read: { path: 'ending.txt', startLine: 4, totalLines: 4, displayedLines: 1 } },
    }, { toolName: 'read', args: { path: 'ending.txt', offset: 4 } })?.read).toMatchObject({
      totalLines: 4, startLine: 4, displayedLines: 1,
    });
  });

  it('旧记录从已保存尾注恢复真实范围，原始输出仍供完整复制', () => {
    for (const notice of [
      '[Showing lines 20-21 of 70. Use offset=22 to continue.]',
      '[49 more lines in file. Use offset=22 to continue.]',
    ]) {
      const details = projectToolPresentationDetails({
        content: [{ type: 'text', text: 'first\nsecond\n\n' + notice }],
      }, { toolName: 'read', args: { path: 'a.ts', offset: 20, limit: 2 } });
      expect(details).toMatchObject({
        output: 'first\nsecond\n\n' + notice, read: { startLine: 20, totalLines: 70, displayedLines: 2, truncated: true },
      });
    }
    const literal = 'first\n\n[49 more lines in file. Use offset=999 to continue.]';
    expect(projectToolPresentationDetails({ content: [{ type: 'text', text: literal }] }, {
      toolName: 'read', args: { path: 'a.ts', offset: 20 },
    })?.output).toBe(literal);
  });

  it('旧搜索输出按路径和行号恢复分组，限制提示不会变成文件', () => {
    const search = parseToolSearchOutput('grep', [
      'C:\\work\\a.ts:12: first', 'C:\\work\\a.ts-13- context', 'b.ts:4: second',
      '[100 matches limit reached. Use limit=200 for more]',
    ].join('\n'));
    expect(search).toMatchObject({ kind: 'grep', matchCount: 2, fileCount: 2, truncated: true });
    expect(search.files![0]).toEqual({
      path: 'C:\\work\\a.ts',
      matches: [{ line: 12, text: 'first' }, { line: 13, text: 'context', context: true }],
    });
    expect(parseToolSearchOutput('find', 'No files found matching pattern').files).toEqual([]);
  });

  it('编辑使用保存的补丁，写入旧记录只恢复当时提交的新正文', () => {
    const sources = [{
      role: 'assistant', content: [
        { type: 'toolCall', id: 'edit', name: 'edit', arguments: { path: 'a.ts', edits: [{ oldText: 'x', newText: 'y' }] } },
        { type: 'toolCall', id: 'write', name: 'write', arguments: { path: 'b.ts', content: 'saved new content' } },
      ],
    }, {
      role: 'toolResult', toolCallId: 'edit', toolName: 'edit', details: { patch: '@@ -1 +1 @@\n-x\n+y\n' },
    }, {
      role: 'toolResult', toolCallId: 'write', toolName: 'write', content: [{ type: 'text', text: 'wrote' }],
    }];
    const outcomes = collectToolOutcomesByCallId(sources);
    expect(outcomes.get('edit')?.details?.fileChange).toMatchObject({ path: 'a.ts', patch: '@@ -1 +1 @@\n-x\n+y\n', beforeAvailable: true });
    expect(outcomes.get('write')?.details?.fileChange).toEqual({
      path: 'b.ts', content: 'saved new content', beforeAvailable: false, reason: 'before_content_unavailable',
    });
  });

  it('失败不呈现成功改动，运行中写入仅明确显示待执行内容', () => {
    const args = { path: 'a.ts', content: 'requested content' };
    expect(projectToolStartDetails('write', args)?.fileChange).toMatchObject({ reason: 'pending', beforeAvailable: false });
    expect(projectToolPresentationDetails({
      isError: true, content: [{ type: 'text', text: 'denied' }],
      details: { fileChange: { path: 'a.ts', patch: '+not applied', beforeAvailable: true } },
    }, { toolName: 'write', args })).not.toHaveProperty('fileChange');
  });

  it('实时大输出和改动明确标记截断，不宣称完整内容', () => {
    const huge = 'x'.repeat(TOOL_PRESENTATION_TEXT_LIMIT + 1);
    const details = projectToolPresentationDetails({
      content: [{ type: 'text', text: huge }],
      details: { fileChange: { path: 'a.ts', content: huge, beforeAvailable: false } },
    }, { toolName: 'write', args: { path: 'a.ts' } })!;
    expect(details.outputTruncated).toBe(true);
    expect(details.output).toHaveLength(TOOL_PRESENTATION_TEXT_LIMIT);
    expect(details.fileChange?.truncated).toBe(true);
    expect(details.fileChange?.content).toHaveLength(TOOL_PRESENTATION_TEXT_LIMIT);
  });

  it('改动统计只来自完整补丁，预览或缺行的补丁不产生假总数', () => {
    const patch = '--- a/a.ts\n+++ b/a.ts\n@@ -1,2 +1,3 @@\n-a\n-b\n+c\n+d\n+e\n';
    expect(toolPatchStats(patch)).toEqual({ added: 3, removed: 2 });
    expect(toolPatchStats(patch.slice(0, -4))).toBeUndefined();
    expect(toolPatchStats(patch + '+extra\n')).toBeUndefined();
    const details = projectToolPresentationDetails({ details: {
      fileChange: { path: 'a.ts', patch, beforeAvailable: true },
    } }, { toolName: 'edit', args: { path: 'a.ts' } }, 40)!;
    expect(details.fileChange).toMatchObject({ added: 3, removed: 2, truncated: true });
    expect(details.fileChange?.patch?.length).toBe(40);
  });
});

describe('B. 旧搜索文本解析（legacy fallback）的能力边界', () => {
  it('上下文行里出现 other.ts:123: 时无法还原真实归属——所以新记录不许走这条路', () => {
    // 这是 legacy parser 的**已知边界**，不是待修的 bug：文本格式本身有歧义。
    // 真实语义是 src/a.ts 第 6 行上下文 + 第 7 行命中，纯文本无法区分。
    const parsed = parseToolSearchOutput('grep', [
      'src/a.ts-6- Error at other.ts:123: boom',
      'src/a.ts:7: target',
    ].join('\n'));
    expect(parsed.files).toEqual([
      { path: 'src/a.ts-6- Error at other.ts', matches: [{ line: 123, text: 'boom' }] },
      { path: 'src/a.ts', matches: [{ line: 7, text: 'target' }] },
    ]);
    expect(parsed.fileCount).toBe(2);
  });

  it('Windows 路径的盘符冒号不会被当成行号分隔符', () => {
    expect(parseToolSearchOutput('grep', 'C:\\work\\a.ts:12: first')).toMatchObject({
      files: [{ path: 'C:\\work\\a.ts', matches: [{ line: 12, text: 'first' }] }],
    });
    expect(parseToolSearchOutput('grep', 'C:\\work\\a.ts-13- context').files).toEqual([
      { path: 'C:\\work\\a.ts', matches: [{ line: 13, text: 'context', context: true }] },
    ]);
  });

  it('正文里多个 `:数字:` 只按第一个分隔符切分，尾部文本保持完整', () => {
    const parsed = parseToolSearchOutput('grep', [
      'logs/app.log:42: 2024-01-01T00:00:00Z GET https://example.com:8443/x',
      'src/b.ts:8: throw new Error("bad at line 17:2")',
    ].join('\n'));
    expect(parsed.files[0]).toEqual({
      path: 'logs/app.log',
      matches: [{ line: 42, text: '2024-01-01T00:00:00Z GET https://example.com:8443/x' }],
    });
    expect(parsed.files[1]).toEqual({
      path: 'src/b.ts',
      matches: [{ line: 8, text: 'throw new Error("bad at line 17:2")' }],
    });
  });

  it('结构化 files 缺失时兜底解析，但 matchCount 只统计非上下文行', () => {
    const parsed = parseToolSearchOutput('grep', [
      'a.ts:1: hit', 'a.ts-2- ctx', 'a.ts-3- ctx', 'b.ts:9: hit',
      '[100 matches limit reached. Use limit=200 for more]',
    ].join('\n'));
    expect(parsed).toMatchObject({ matchCount: 2, fileCount: 2, truncated: true });
  });
});
