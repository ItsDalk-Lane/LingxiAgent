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
          knowledgeDistill: null,
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

  it('renders one model widget per auxiliary slot (8 slots incl. knowledgeDistill) and no operation pickers', () => {
    render(<AuxiliaryModelsSection providers={{ openai: { models: ['gpt-4o'] } }} />);

    // 8 auxiliary slots (title/summarize/memory/knowledge/knowledgeDistill/vision/approval/guard)
    expect(screen.getAllByTestId('model-widget')).toHaveLength(8);
    expect(screen.getByText('settings.api.auxKnowledgeDistillModel')).toBeInTheDocument();
    expect(screen.queryByText('settings.api.searchProviderField')).not.toBeInTheDocument();
    // 知识库嵌入/重排全局配置已退役（迁移至笔记本级）：不出现对应下拉。
    expect(screen.queryByRole('combobox', { name: 'settings.api.knowledgeEmbeddingModel' })).not.toBeInTheDocument();
    expect(screen.queryByRole('combobox', { name: 'settings.api.knowledgeRerankModel' })).not.toBeInTheDocument();
    expect(screen.queryByRole('textbox', { name: /key/i })).not.toBeInTheDocument();
  });
});
