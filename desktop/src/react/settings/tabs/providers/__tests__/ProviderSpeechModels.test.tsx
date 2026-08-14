/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';

vi.mock('../../../helpers', () => ({
  t: (key: string) => key,
}));

import { ProviderSpeechModels } from '../ProviderSpeechModels';

describe('ProviderSpeechModels', () => {
  afterEach(() => {
    cleanup();
  });

  it('lists only runnable speech models and marks the default', () => {
    render(
      <ProviderSpeechModels
        runtimeProviderId="openai"
        provider={{
          providerId: 'openai',
          displayName: 'OpenAI Speech',
          hasCredentials: true,
          models: [
            { id: 'whisper-1', name: 'Whisper 1', adapterAvailable: true },
            { id: 'unrunnable', name: 'Unrunnable', adapterAvailable: false },
          ],
        }}
        config={{ enabled: true, defaultModel: { provider: 'openai', id: 'whisper-1' } }}
      />,
    );

    expect(screen.getByText('whisper-1')).toBeInTheDocument();
    expect(screen.queryByText('unrunnable')).not.toBeInTheDocument();
    expect(screen.getByText('settings.media.default')).toBeInTheDocument();
  });

  it('renders an empty state when no runnable models exist', () => {
    render(
      <ProviderSpeechModels
        runtimeProviderId="openai"
        provider={{
          providerId: 'openai',
          displayName: 'OpenAI Speech',
          hasCredentials: false,
          models: [{ id: 'whisper-1', name: 'Whisper 1', adapterAvailable: true }],
        }}
        config={{ enabled: true }}
      />,
    );

    expect(screen.getByText('settings.media.noProvider')).toBeInTheDocument();
  });
});
