/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useSettingsStore } from '../../settings/store';
import { ModelWidget } from '../../settings/widgets/ModelWidget';

const mocks = vi.hoisted(() => ({
  lingxiFetch: vi.fn(async () => ({
    json: async () => ({ models: [] }),
  })),
}));

vi.mock('../../settings/api', () => ({
  lingxiFetch: mocks.lingxiFetch,
}));

describe('ModelWidget', () => {
  beforeEach(() => {
    useSettingsStore.setState({ runtimeModels: [] } as never);
  });

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

  it('uses the shared runtime model catalog without a private fetch', () => {
    useSettingsStore.setState({
      runtimeModels: [{ id: 'model-x', name: 'Model X', provider: 'provider-p' }],
    } as never);

    render(<ModelWidget value={null} onSelect={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: /selectModel/ }));

    expect(screen.getByRole('button', { name: 'Model X' })).toBeTruthy();
    expect(mocks.lingxiFetch).not.toHaveBeenCalled();
  });

  it('reacts to shared catalog additions and removals without remounting', () => {
    useSettingsStore.setState({
      runtimeModels: [
        { id: 'model-x', name: 'Model X', provider: 'provider-p' },
        { id: 'model-y', name: 'Model Y', provider: 'provider-p' },
      ],
    } as never);

    render(<ModelWidget value={null} onSelect={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: /selectModel/ }));
    expect(screen.getByRole('button', { name: 'Model Y' })).toBeTruthy();

    act(() => {
      useSettingsStore.setState({
        runtimeModels: [{ id: 'model-x', name: 'Model X', provider: 'provider-p' }],
      } as never);
    });

    expect(screen.queryByRole('button', { name: 'Model Y' })).toBeNull();

    act(() => {
      useSettingsStore.setState({
        runtimeModels: [
          { id: 'model-x', name: 'Model X', provider: 'provider-p' },
          { id: 'model-z', name: 'Model Z', provider: 'provider-p' },
        ],
      } as never);
    });

    expect(screen.getByRole('button', { name: 'Model Z' })).toBeTruthy();
  });

  it('preserves the vision capability filter', () => {
    useSettingsStore.setState({
      runtimeModels: [
        { id: 'text-only', name: 'Text only', provider: 'provider-p', input: ['text'] },
        { id: 'vision-model', name: 'Vision model', provider: 'provider-p', input: ['text', 'image'] },
      ],
    } as never);

    render(
      <ModelWidget
        value={null}
        onSelect={vi.fn()}
        filterModel={(model) => Array.isArray(model.input) && model.input.includes('image')}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: /selectModel/ }));

    expect(screen.getByRole('button', { name: 'Vision model' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Text only' })).toBeNull();
  });

  it('keeps duplicate model ids distinct by provider and returns a full reference', () => {
    const onSelect = vi.fn();
    useSettingsStore.setState({
      runtimeModels: [
        { id: 'same-model', name: 'Same model', provider: 'provider-a' },
        { id: 'same-model', name: 'Same model', provider: 'provider-b' },
      ],
    } as never);

    render(<ModelWidget value={null} onSelect={onSelect} />);
    fireEvent.click(screen.getByRole('button', { name: /selectModel/ }));
    const choices = screen.getAllByRole('button', { name: 'Same model' });
    expect(choices).toHaveLength(2);
    fireEvent.click(choices[1]);

    expect(onSelect).toHaveBeenCalledWith({ id: 'same-model', provider: 'provider-b' });
  });
});
