// @vitest-environment jsdom
import React from 'react';
import { describe, it, expect, vi, afterEach } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { DeferSettings } from '../DeferSettings';

vi.mock('../../../helpers', () => ({
  t: (key: string) => key,
}));

afterEach(cleanup);

describe('DeferSettings', () => {
  // 内置/插件按需加载与 MCP 连接器按需加载各管各的来源，任一开关都不锁另一个；
  // 只有提交中的 busy 状态会暂时禁用全部控件。
  it('keeps the built-in switch usable while the MCP master defer switch is off', () => {
    const onChange = vi.fn();
    render(
      <DeferSettings deferEnabled={false} deferThreshold={4} builtinDeferEnabled={false} busy={false} onChange={onChange} />,
    );
    fireEvent.click(screen.getByText('settings.mcp.deferTitle'));

    const builtin = screen.getByRole('switch', { name: 'settings.mcp.deferBuiltin' });
    expect(builtin.hasAttribute('disabled') || builtin.getAttribute('aria-disabled') === 'true').toBe(false);
    fireEvent.click(builtin);
    expect(onChange).toHaveBeenCalledWith({ builtinDeferEnabled: true });
  });

  it('disables every control while busy', () => {
    const onChange = vi.fn();
    render(
      <DeferSettings deferEnabled={true} deferThreshold={4} builtinDeferEnabled={true} busy={true} onChange={onChange} />,
    );
    fireEvent.click(screen.getByText('settings.mcp.deferTitle'));

    const builtin = screen.getByRole('switch', { name: 'settings.mcp.deferBuiltin' });
    expect(builtin.hasAttribute('disabled') || builtin.getAttribute('aria-disabled') === 'true').toBe(true);
  });

  it('lets the built-in tier through once the master switch is on', () => {
    const onChange = vi.fn();
    render(
      <DeferSettings deferEnabled={true} deferThreshold={4} builtinDeferEnabled={false} busy={false} onChange={onChange} />,
    );
    fireEvent.click(screen.getByText('settings.mcp.deferTitle'));

    fireEvent.click(screen.getByRole('switch', { name: 'settings.mcp.deferBuiltin' }));
    expect(onChange).toHaveBeenCalledWith({ builtinDeferEnabled: true });
  });
});
