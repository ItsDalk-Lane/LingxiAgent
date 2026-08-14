// @vitest-environment jsdom

import { cleanup, render } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { ProviderIcon } from '../../ui/ProviderIcon';

describe('ProviderIcon', () => {
  afterEach(() => {
    cleanup();
  });

  it('renders the Fireworks provider logo through the shared icon map', () => {
    const { container } = render(<ProviderIcon provider="fireworks" />);
    const svg = container.querySelector('svg');
    const path = container.querySelector('path');

    expect(svg?.getAttribute('viewBox')).toBe('0 0 24 24');
    expect(path?.getAttribute('d')).toContain('M14.8 5l-2.801 6.795');
    expect(path?.getAttribute('fill')).toBeNull();
    expect(container.querySelector('rect')).toBeNull();
  });

  it('renders a real logo (not the placeholder dot) for every provider that previously lacked one', () => {
    const providers = [
      'agnes', 'anthropic', 'ant-ling', 'baichuan', 'baidu-cloud', 'hunyuan',
      'infini', 'modelscope', 'opencode', 'opencode-go', 'perplexity', 'stepfun',
      'together', 'xai', 'xai-oauth', 'volcengine-speech',
    ];

    for (const provider of providers) {
      const { container } = render(<ProviderIcon provider={provider} />);
      expect(container.querySelector('rect'), `${provider} should not fall back to the placeholder`).toBeNull();
      expect(container.querySelector('path'), `${provider} should render a real path`).not.toBeNull();
      cleanup();
    }
  });
});
