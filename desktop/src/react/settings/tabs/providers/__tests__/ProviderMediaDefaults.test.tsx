/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';

vi.mock('../../../helpers', () => ({
  t: (key: string) => key,
}));

vi.mock('@/ui', () => ({
  SelectWidget: ({ value, onChange, options }: {
    value: string;
    onChange: (value: string) => void;
    options: Array<{ value: string; label: string }>;
  }) => (
    <select value={value} onChange={(event) => onChange(event.target.value)}>
      {options.map(option => (
        <option key={option.value} value={option.value}>{option.label}</option>
      ))}
    </select>
  ),
}));

import { ProviderMediaDefaults } from '../ProviderMediaDefaults';

function invokeUpdater(onSaveConfig: ReturnType<typeof vi.fn>) {
  const updater = onSaveConfig.mock.calls[0][0];
  return typeof updater === 'function' ? updater({}) : updater;
}

describe('ProviderMediaDefaults', () => {
  afterEach(() => {
    cleanup();
  });

  it('saves provider mode defaults under provider/model/mode keyed by runtimeProviderId', () => {
    const onSaveConfig = vi.fn();

    render(
      <ProviderMediaDefaults
        capability="videoGeneration"
        runtimeProviderId="jimeng-cli"
        provider={{
          providerId: 'jimeng-cli',
          displayName: '即梦 CLI',
          hasCredentials: true,
          availableModels: [],
          models: [{
            id: 'seedance2.0_vip',
            name: 'Seedance 2.0 VIP',
            protocolId: 'jimeng-cli-videos',
            modes: [{
              id: 'text2video',
              label: '文生视频',
              parameterSchema: {
                type: 'object',
                properties: {
                  video_resolution: {
                    type: 'string',
                    enum: ['720p', '1080p'],
                    default: '720p',
                  },
                },
              },
            }],
          }],
        }}
        config={{}}
        onSaveConfig={onSaveConfig}
      />,
    );

    expect(screen.getByText('video_resolution')).toBeInTheDocument();
    const selects = screen.getAllByRole('combobox');
    fireEvent.change(selects[1], { target: { value: '1080p' } });

    expect(invokeUpdater(onSaveConfig)).toEqual({
      providerDefaults: {
        'jimeng-cli': {
          models: {
            'seedance2.0_vip': {
              modes: {
                text2video: {
                  video_resolution: '1080p',
                },
              },
            },
          },
        },
      },
    });
  });

  it('uses ratio/resolution fallback when there is no schema', () => {
    const onSaveConfig = vi.fn();

    render(
      <ProviderMediaDefaults
        capability="imageGeneration"
        runtimeProviderId="volcengine"
        provider={{
          providerId: 'volcengine',
          displayName: 'Volcengine',
          hasCredentials: true,
          availableModels: [],
          models: [{
            id: 'seedream-5',
            name: 'Seedream 5.0',
            ratios: ['1:1', '3:2'],
            resolutions: ['1K', '2K'],
          }],
        }}
        config={{}}
        onSaveConfig={onSaveConfig}
      />,
    );

    // 非 schema 回退：尺寸 + 长宽比两个选择器。
    const selects = screen.getAllByRole('combobox');
    expect(selects.length).toBe(2);

    fireEvent.change(selects[1], { target: { value: '3:2' } });
    expect(invokeUpdater(onSaveConfig)).toEqual({
      providerDefaults: {
        volcengine: { aspect_ratio: '3:2' },
      },
    });
  });

  it('renders an explicit empty state when the added model declares no parameter schema', () => {
    // 自定义添加的媒体模型（不在声明目录中，无 modes/ratios/resolutions）：
    // 不能渲染成只剩标题的空白表单
    render(
      <ProviderMediaDefaults
        capability="imageGeneration"
        runtimeProviderId="openai"
        provider={{
          providerId: 'openai',
          displayName: 'OpenAI',
          hasCredentials: true,
          availableModels: [],
          models: [{ id: 'my-custom-image-model', name: 'my-custom-image-model' }],
        }}
        config={{}}
        onSaveConfig={vi.fn()}
      />,
    );

    expect(screen.queryAllByRole('combobox')).toHaveLength(0);
    expect(screen.getByText('settings.media.noParameterDefaults')).toBeInTheDocument();
    expect(document.querySelector('[data-media-defaults-empty="true"]')).not.toBeNull();
  });

  it('renders the same empty state when the runtime catalog has no added models at all', () => {
    // chat 槽认领的媒体模型不进运行时目录：按钮亮但表单数据源为空，
    // 同样必须给出显式空态而不是空白
    render(
      <ProviderMediaDefaults
        capability="imageGeneration"
        runtimeProviderId="dashscope"
        provider={{
          providerId: 'dashscope',
          displayName: 'DashScope',
          hasCredentials: true,
          availableModels: [{ id: 'qwen-image', name: 'Qwen Image' }],
          models: [],
        }}
        config={{}}
        onSaveConfig={vi.fn()}
      />,
    );

    expect(screen.queryAllByRole('combobox')).toHaveLength(0);
    expect(screen.getByText('settings.media.noParameterDefaults')).toBeInTheDocument();
  });
});
