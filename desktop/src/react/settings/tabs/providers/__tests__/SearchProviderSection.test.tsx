/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { useSettingsStore } from '../../../store';

const mocks = vi.hoisted(() => ({
  autoSaveGlobalModels: vi.fn(),
}));

vi.mock('../../../helpers', () => ({
  t: (key: string) => key,
  autoSaveGlobalModels: mocks.autoSaveGlobalModels,
}));

vi.mock('@/ui', () => ({
  SelectWidget: ({ value, onChange }: { value: string; onChange: (value: string) => void }) => (
    <button type="button" data-testid="search-provider-select" onClick={() => onChange(value)}>
      {value}
    </button>
  ),
}));

import { SearchProviderSection } from '../SearchProviderSection';

describe('SearchProviderSection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSettingsStore.setState({
      globalModelsConfig: {
        models: {},
        search: { provider: 'tavily', api_key: 'sk-tavily' },
        utility_api: {},
      },
    });
  });

  afterEach(() => {
    cleanup();
  });

  it('saves the selected search provider through autoSaveGlobalModels', () => {
    render(<SearchProviderSection />);

    const select = screen.getByTestId('search-provider-select');
    expect(select.textContent).toContain('tavily');

    fireEvent.click(select);

    expect(mocks.autoSaveGlobalModels).toHaveBeenCalledWith({
      search: { provider: 'tavily' },
    });
  });

  it('clears the api key for keyless search providers', () => {
    useSettingsStore.setState({
      globalModelsConfig: {
        models: {},
        search: { provider: 'bing_browser', api_key: '' },
        utility_api: {},
      },
    });

    render(<SearchProviderSection />);

    fireEvent.click(screen.getByTestId('search-provider-select'));

    expect(mocks.autoSaveGlobalModels).toHaveBeenCalledWith({
      search: { provider: 'bing_browser', api_key: '' },
    });
  });
});
