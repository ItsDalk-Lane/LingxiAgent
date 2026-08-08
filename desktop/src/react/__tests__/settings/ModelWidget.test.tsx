/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ModelWidget } from '../../settings/widgets/ModelWidget';

vi.mock('../../settings/api', () => ({
  lingxiFetch: vi.fn(async () => ({
    json: async () => ({ models: [] }),
  })),
}));

describe('ModelWidget', () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('shows the provider icon in the closed selected trigger', () => {
    render(
      <ModelWidget
        value={{ id: 'glm-5.2', provider: 'zhipu-coding' }}
        onSelect={vi.fn()}
        placeholder="select"
      />,
    );

    const trigger = screen.getByRole('button', { name: /zhipu-coding\/glm-5.2/ });
    expect(trigger.querySelector('svg')).toBeTruthy();
  });

  it('shows the followLabel in the trigger when value is null', () => {
    render(
      <ModelWidget
        value={null}
        followLabel="跟随主模型"
        onSelect={vi.fn()}
      />,
    );

    const trigger = screen.getByRole('button', { name: /跟随主模型/ });
    expect(trigger).toBeTruthy();
  });

  it('renders the follow option and calls onSelect(null) when clicked', () => {
    const onSelect = vi.fn();
    render(
      <ModelWidget
        value={null}
        followLabel="跟随主模型"
        onSelect={onSelect}
      />,
    );

    // 打开下拉
    fireEvent.click(screen.getByRole('button', { name: /跟随主模型/ }));
    // 点击 follow 选项
    const followOption = screen.getByRole('button', { name: /^跟随主模型$/ });
    fireEvent.click(followOption);
    expect(onSelect).toHaveBeenCalledWith(null);
  });
});
