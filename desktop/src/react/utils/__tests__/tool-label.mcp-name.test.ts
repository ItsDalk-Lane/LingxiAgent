import { describe, expect, it } from 'vitest';
import { mcpActivityName } from '../tool-label';

describe('mcpActivityName', () => {
  // 第一方内置目标经 mcp_call 调用时，server 恒为 first-party（或缺省）：
  // 这个前缀没有信息量，身份行直接显示工具名。外部来源维持「服务 / 工具」。
  it('shows the bare tool name for first-party bridge calls', () => {
    expect(mcpActivityName('mcp_call', { server: 'first-party', tool: 'knowledge_search' }))
      .toBe('knowledge_search');
    expect(mcpActivityName('mcp_call', { tool: 'knowledge_search' }))
      .toBe('knowledge_search');
  });

  it('keeps the server / tool pair for external sources', () => {
    expect(mcpActivityName('mcp_call', { server: 'zread', tool: 'get_repo_structure' }))
      .toBe('zread / get_repo_structure');
  });

  it('describes bridge lookups by their name argument', () => {
    expect(mcpActivityName('mcp_describe_tool', { server: 'first-party', name: 'notify' }))
      .toBe('notify');
    expect(mcpActivityName('mcp_describe_tool', { server: 'github', name: 't_0' }))
      .toBe('github / t_0');
  });

  it('ignores non-bridge mcp tool names beyond the prefix strip', () => {
    expect(mcpActivityName('mcp_github_t_0')).toBe('github_t_0');
    expect(mcpActivityName('read')).toBeNull();
  });
});
