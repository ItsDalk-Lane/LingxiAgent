/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { useSettingsStore } from '../../../store';

const mocks = vi.hoisted(() => ({
  autoSaveGlobalModels: vi.fn(),
}));

vi.mock('../../../helpers', () => ({
  t: (key: string) => key,
  lookupModelMeta: vi.fn(),
  formatContext: (n: number) => String(n),
  autoSaveGlobalModels: mocks.autoSaveGlobalModels,
}));

vi.mock('../../../api', () => ({
  lingxiFetch: vi.fn(),
}));

vi.mock('../../../actions', () => ({
  loadSettingsConfig: vi.fn(),
}));

vi.mock('../../../widgets/ModelWidget', () => ({
  ModelWidget: () => <div data-testid="model-widget">model-widget</div>,
}));

vi.mock('@/ui', () => ({
  Toggle: ({ on, onChange, label }: { on: boolean; onChange: (next: boolean) => void; label?: string }) => (
    <button
      type="button"
      data-testid={`toggle-${on ? 'on' : 'off'}`}
      onClick={() => onChange(!on)}
    >
      {label}
    </button>
  ),
}));

import { AuxiliaryModelsSection } from '../AuxiliaryModelsSection';

describe('AuxiliaryModelsSection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSettingsStore.setState({
      globalModelsConfig: {
        models: {
          title: null,
          summarize: null,
          memory: null,
          knowledge: null,
          embedding: null,
          rerank: { id: 'rerank-model', provider: 'provider-b' },
          vision: { id: 'gpt-4o', provider: 'openai' },
          approval: null,
          guard: null,
          vision_enabled: false,
        },
        search: { provider: '', api_key: '' },
        utility_api: {},
        operation_models: [
          {
            id: 'embed-model',
            provider: 'provider-a',
            displayName: 'Embedding A',
            operations: ['embedding'],
          },
          {
            id: 'rerank-model',
            provider: 'provider-b',
            displayName: 'Rerank B',
            operations: ['rerank'],
          },
        ],
      },
    });
  });

  afterEach(() => {
    cleanup();
  });

  it('renders the auxiliary vision toggle above the vision model picker and saves it as a global model preference', () => {
    render(<AuxiliaryModelsSection providers={{ openai: { models: ['gpt-4o'] } }} />);

    const visionLabel = screen.getByText('settings.api.visionModel');
    const toggle = screen.getByRole('button', { name: 'settings.api.visionAuxiliaryToggle' });
    const visionRow = visionLabel.parentElement?.parentElement;
    expect(visionRow).not.toBeNull();
    const visionModelWidget = within(visionRow as HTMLElement).getByTestId('model-widget');

    expect(visionLabel.compareDocumentPosition(toggle) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(toggle.compareDocumentPosition(visionModelWidget) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    fireEvent.click(toggle);

    expect(mocks.autoSaveGlobalModels).toHaveBeenCalledWith({
      models: { vision_enabled: true },
    });
  });

  it('renders one model widget per auxiliary slot and does not render the search provider selector', () => {
    render(<AuxiliaryModelsSection providers={{ openai: { models: ['gpt-4o'] } }} />);

    // 7 auxiliary slots (title/summarize/memory/knowledge/vision/approval/guard)
    expect(screen.getAllByTestId('model-widget')).toHaveLength(7);
    expect(screen.queryByText('settings.api.searchProviderField')).not.toBeInTheDocument();
  });

  it('renders optional operation pickers, filters their catalogs, and saves only composite model refs', () => {
    render(<AuxiliaryModelsSection providers={{ openai: { models: ['gpt-4o'] } }} />);

    const embedding = screen.getByRole('combobox', { name: 'settings.api.knowledgeEmbeddingModel' });
    const rerank = screen.getByRole('combobox', { name: 'settings.api.knowledgeRerankModel' });
    expect(within(embedding).getByRole('option', { name: 'Embedding A · provider-a' })).toBeInTheDocument();
    expect(within(embedding).queryByRole('option', { name: 'Rerank B · provider-b' })).not.toBeInTheDocument();
    expect(within(rerank).getByRole('option', { name: 'Rerank B · provider-b' })).toBeInTheDocument();
    expect(screen.queryByRole('textbox', { name: /key/i })).not.toBeInTheDocument();

    fireEvent.change(embedding, { target: { value: 'provider-a\u0000embed-model' } });
    expect(mocks.autoSaveGlobalModels).toHaveBeenCalledWith({
      models: { embedding: { id: 'embed-model', provider: 'provider-a' } },
    });

    fireEvent.change(rerank, { target: { value: '' } });
    expect(mocks.autoSaveGlobalModels).toHaveBeenCalledWith({ models: { rerank: null } });
  });
});
