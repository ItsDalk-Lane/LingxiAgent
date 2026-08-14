/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';

const mocks = vi.hoisted(() => ({
  lingxiFetch: vi.fn(),
  invalidateConfigCache: vi.fn(),
  showToast: vi.fn(),
}));

vi.mock('../../../api', () => ({
  lingxiFetch: (...args: unknown[]) => mocks.lingxiFetch(...args),
}));

vi.mock('../../../../hooks/use-config', () => ({
  invalidateConfigCache: () => mocks.invalidateConfigCache(),
}));

vi.mock('../../../store', () => ({
  useSettingsStore: (selector: (state: { showToast: typeof mocks.showToast }) => unknown) =>
    selector({ showToast: mocks.showToast }),
}));

vi.mock('../../../helpers', () => ({
  t: (key: string) => key,
}));

vi.mock('../../../hooks/useAnchoredDropdown', () => ({
  useAnchoredDropdown: () => ({ position: 'fixed', left: 0, top: 0, width: 280 }),
}));

import { ProviderMediaModels } from '../ProviderMediaModels';

describe('ProviderMediaModels', () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('allows adding a custom image model id when provider discovery has no candidate list', async () => {
    mocks.lingxiFetch.mockResolvedValue({ json: async () => ({ ok: true }) });
    const onRefresh = vi.fn(async () => {});

    render(
      <ProviderMediaModels
        capability="imageGeneration"
        runtimeProviderId="dashscope"
        provider={{
          providerId: 'dashscope',
          displayName: 'DashScope',
          hasCredentials: true,
          models: [],
          availableModels: [],
        }}
        onRefresh={onRefresh}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: /settings\.media\.addModel/ }));
    fireEvent.change(screen.getByPlaceholderText('settings.api.searchModel'), {
      target: { value: 'qwen-image-2.0-pro' },
    });
    fireEvent.click(screen.getByRole('button', { name: /qwen-image-2\.0-pro/ }));

    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith('/api/media/image/providers/dashscope/models', expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ model: { id: 'qwen-image-2.0-pro' } }),
      }));
    });
    expect(onRefresh).toHaveBeenCalled();
  });

  it('does not offer add or remove controls for models discovered from a CLI', () => {
    render(
      <ProviderMediaModels
        capability="imageGeneration"
        runtimeProviderId="jimeng-cli"
        provider={{
          providerId: 'jimeng-cli',
          displayName: '即梦 CLI',
          hasCredentials: true,
          runtimeCapability: { status: 'ready' },
          models: [{ id: 'jimeng-image-5.0', name: '即梦图片 5.0' }],
          availableModels: [],
        }}
        onRefresh={vi.fn(async () => {})}
      />,
    );

    expect(screen.getByText('jimeng-image-5.0')).toBeInTheDocument();
    expect(screen.queryByTitle('settings.api.removeModel')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /settings\.media\.addModel/ })).not.toBeInTheDocument();
  });

  it('removes a non-runtime model through the media provider route', async () => {
    mocks.lingxiFetch.mockResolvedValue({ json: async () => ({ ok: true }) });
    const onRefresh = vi.fn(async () => {});

    render(
      <ProviderMediaModels
        capability="videoGeneration"
        runtimeProviderId="agnes"
        provider={{
          providerId: 'agnes',
          displayName: 'Agnes',
          hasCredentials: true,
          models: [{ id: 'agnes-video-v2.0', name: 'Agnes Video V2.0' }],
          availableModels: [],
        }}
        onRefresh={onRefresh}
      />,
    );

    fireEvent.click(screen.getByTitle('settings.api.removeModel'));

    await waitFor(() => {
      expect(mocks.lingxiFetch).toHaveBeenCalledWith(
        '/api/media/video/providers/agnes/models/agnes-video-v2.0',
        expect.objectContaining({ method: 'DELETE' }),
      );
    });
    expect(onRefresh).toHaveBeenCalled();
  });
});
