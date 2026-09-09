/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { useSettingsStore } from '../../../store';
import { lingxiFetch } from '../../../api';

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
    vi.mocked(lingxiFetch).mockReset();
    vi.useFakeTimers();
    useSettingsStore.setState({
      globalModelsConfig: {
        models: {
          title: null,
          summarize: null,
          memory: null,
          knowledge: null,
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
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it('shows a named busy state until the model test succeeds, then restores the action', async () => {
    let finish!: (response: Response) => void;
    vi.mocked(lingxiFetch).mockReturnValueOnce(new Promise(resolve => { finish = resolve; }));
    render(<AuxiliaryModelsSection providers={{}} />);
    fireEvent.click(screen.getByRole('button', { name: 'settings.providers.verifyConnection' }));

    const button = screen.getByRole('button', { name: 'settings.search.verifying' });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute('aria-busy', 'true');
    expect(button).toHaveAttribute('title', 'settings.search.verifying');
    expect(button.querySelector('svg')?.getAttribute('class')).toContain('spinning');

    await act(async () => finish(new Response(JSON.stringify({ ok: true }))));
    expect(screen.getByRole('button', { name: 'settings.providers.verifySuccess' })).not.toBeDisabled();
    expect(button).toHaveAttribute('aria-busy', 'false');
    act(() => vi.advanceTimersByTime(3000));
    expect(screen.getByRole('button', { name: 'settings.providers.verifyConnection' })).toBe(button);
  });

  it.each(['failed-result', 'request-error'] as const)('shows a readable failure for %s', async (failure) => {
    if (failure === 'request-error') vi.mocked(lingxiFetch).mockRejectedValueOnce(new Error('offline'));
    else vi.mocked(lingxiFetch).mockResolvedValueOnce(new Response(JSON.stringify({ ok: false })));
    render(<AuxiliaryModelsSection providers={{}} />);
    await act(async () => fireEvent.click(screen.getByRole('button', { name: 'settings.providers.verifyConnection' })));
    expect(screen.getByRole('button', { name: 'settings.providers.verifyFailed' })).toHaveAttribute('aria-busy', 'false');
  });

  it('ignores the previous model result after a model change and aborts requests on unmount', async () => {
    let finishOld!: (response: Response) => void;
    let finishNew!: (response: Response) => void;
    vi.mocked(lingxiFetch)
      .mockReturnValueOnce(new Promise(resolve => { finishOld = resolve; }))
      .mockReturnValueOnce(new Promise(resolve => { finishNew = resolve; }));
    const { unmount } = render(<AuxiliaryModelsSection providers={{}} />);
    fireEvent.click(screen.getByRole('button', { name: 'settings.providers.verifyConnection' }));
    const oldSignal = vi.mocked(lingxiFetch).mock.calls[0][1]?.signal;
    act(() => {
      const config = useSettingsStore.getState().globalModelsConfig!;
      useSettingsStore.setState({ globalModelsConfig: {
        ...config, models: { ...config.models, vision: { id: 'new-model', provider: 'openai' } },
      } });
    });
    expect(oldSignal?.aborted).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'settings.providers.verifyConnection' }));
    await act(async () => finishOld(new Response(JSON.stringify({ ok: true }))));
    expect(screen.getByRole('button', { name: 'settings.search.verifying' })).toBeDisabled();
    const newSignal = vi.mocked(lingxiFetch).mock.calls[1][1]?.signal;
    unmount();
    expect(newSignal?.aborted).toBe(true);
    await act(async () => finishNew(new Response(JSON.stringify({ ok: true }))));
    expect(vi.getTimerCount()).toBe(0);
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

  it('renders one model widget per auxiliary slot (7 slots; knowledgeDistill removed with the distill path) and no operation pickers', () => {
    render(<AuxiliaryModelsSection providers={{ openai: { models: ['gpt-4o'] } }} />);

    // 7 auxiliary slots (title/summarize/memory/knowledge/vision/approval/guard)
    expect(screen.getAllByTestId('model-widget')).toHaveLength(7);
    expect(screen.queryByText('settings.api.auxKnowledgeDistillModel')).not.toBeInTheDocument();
    expect(screen.queryByText('settings.api.searchProviderField')).not.toBeInTheDocument();
    // 知识库嵌入/重排全局配置已退役（迁移至笔记本级）：不出现对应下拉。
    expect(screen.queryByRole('combobox', { name: 'settings.api.knowledgeEmbeddingModel' })).not.toBeInTheDocument();
    expect(screen.queryByRole('combobox', { name: 'settings.api.knowledgeRerankModel' })).not.toBeInTheDocument();
    expect(screen.queryByRole('textbox', { name: /key/i })).not.toBeInTheDocument();
  });
});
