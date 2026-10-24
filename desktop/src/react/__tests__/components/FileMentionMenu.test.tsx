// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { FileMentionMenu } from '../../components/input/FileMentionMenu';
import { resetDesktopHostForTests } from '../../host/desktop-host';

describe('FileMentionMenu', () => {
  afterEach(() => {
    cleanup();
    resetDesktopHostForTests();
  });

  it('renders file candidates without a leading at sign and uses audio file icons', () => {
    const { container } = render(
      <FileMentionMenu
        items={[{
          id: 'session:sf_audio',
          source: 'session',
          fileId: 'sf_audio',
          path: '/tmp/recording.wav',
          name: 'recording.wav',
          mimeType: 'audio/wav',
          detail: '/tmp/recording.wav',
        }]}
        selected={0}
        busy={false}
        onSelect={vi.fn()}
        onHover={vi.fn()}
      />,
    );

    expect(screen.getByText('recording.wav')).toBeInTheDocument();
    expect(screen.queryByText('@recording.wav')).toBeNull();
    expect(container.querySelector('svg[data-file-kind="audio"]')).not.toBeNull();
  });
});
