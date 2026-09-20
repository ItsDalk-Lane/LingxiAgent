/**
 * TurnEditedFilesCard 纯函数测试：本轮编辑文件收集与 Git 路径对齐。
 * 组件渲染依赖 store / 网络层，这里只测最核心、最容易回归的两个纯函数。
 */
import { describe, expect, it } from 'vitest';
import type { ContentBlock, ToolCall } from '../../../stores/chat-types';
import {
  collectTurnEditedFiles,
  matchGitFileForTest,
} from '../TurnEditedFilesCard';

function toolGroup(tools: ToolCall[]): ContentBlock {
  return { type: 'tool_group', tools, collapsed: false };
}

function editTool(path: string, extra?: Partial<ToolCall>): ToolCall {
  return {
    name: 'edit',
    done: true,
    success: true,
    details: { fileChange: { path } },
    ...extra,
  };
}

describe('collectTurnEditedFiles', () => {
  it('无工具块或无 fileChange 时返回空数组', () => {
    expect(collectTurnEditedFiles([])).toEqual([]);
    expect(collectTurnEditedFiles([
      { type: 'text', source: 'hello' },
      toolGroup([{ name: 'grep', done: true, success: true, details: {} }]),
    ] as unknown as ContentBlock[])).toEqual([]);
  });

  it('收集多个文件的路径；失败与未完成的调用不计入', () => {
    const blocks = [
      toolGroup([
        editTool('core/agent.ts'),
        { ...editTool('core/broken.ts'), success: false },
        { ...editTool('core/running.ts'), done: false },
      ]),
      { type: 'text', source: 'done' },
      toolGroup([editTool('lib/util.ts')]),
    ] as unknown as ContentBlock[];
    expect(collectTurnEditedFiles(blocks).map(f => f.path)).toEqual([
      'core/agent.ts',
      'lib/util.ts',
    ]);
  });

  it('同一路径多次编辑只留最后一次（最后一次是累计口径）', () => {
    const blocks = [
      toolGroup([
        editTool('core/agent.ts', { details: { fileChange: { path: 'core/agent.ts', added: 3, removed: 1 } } }),
        editTool('core/agent.ts', { details: { fileChange: { path: 'core/agent.ts', added: 78, removed: 78 } } }),
      ]),
    ] as unknown as ContentBlock[];
    expect(collectTurnEditedFiles(blocks)).toEqual([
      { path: 'core/agent.ts', added: 78, removed: 78 },
    ]);
  });

  it('无 added/removed 数字时从补丁文本统计', () => {
    const patch = [
      '--- a/core/agent.ts',
      '+++ b/core/agent.ts',
      '@@ -1,2 +1,3 @@',
      ' context',
      '-removed',
      '+added-1',
      '+added-2',
    ].join('\n');
    const blocks = [
      toolGroup([editTool('core/agent.ts', { details: { fileChange: { path: 'core/agent.ts', patch } } })]),
    ] as unknown as ContentBlock[];
    expect(collectTurnEditedFiles(blocks)).toEqual([
      { path: 'core/agent.ts', added: 2, removed: 1 },
    ]);
  });

  it('补丁不可用时 added/removed 保持 null（交给 Git 匹配兜底）', () => {
    const blocks = [toolGroup([editTool('core/agent.ts')])] as unknown as ContentBlock[];
    expect(collectTurnEditedFiles(blocks)).toEqual([
      { path: 'core/agent.ts', added: null, removed: null },
    ]);
  });
});

describe('matchGitFileForTest（编辑路径 ↔ Git 仓库相对路径对齐）', () => {
  const files = [
    { path: 'core/agent.ts', additions: 78, deletions: 78, state: 'modified', staged: false },
    { path: 'docs/new.md', additions: 0, deletions: 0, state: 'untracked', staged: false },
  ] as Parameters<typeof matchGitFileForTest>[1];

  it('相对路径相等命中', () => {
    expect(matchGitFileForTest('core/agent.ts', files)?.path).toBe('core/agent.ts');
  });

  it('绝对路径按仓库相对后缀命中', () => {
    expect(matchGitFileForTest('/Users/me/proj/core/agent.ts', files)?.path).toBe('core/agent.ts');
  });

  it('容忍 Windows 反斜杠与 ./ 前缀', () => {
    expect(matchGitFileForTest('.\\core\\agent.ts', files)?.path).toBe('core/agent.ts');
  });

  it('对不上返回 null', () => {
    expect(matchGitFileForTest('other/file.ts', files)).toBeNull();
    expect(matchGitFileForTest('core', files)).toBeNull();
  });
});
