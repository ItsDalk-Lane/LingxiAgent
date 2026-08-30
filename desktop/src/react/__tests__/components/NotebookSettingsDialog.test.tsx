// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { NotebookSettingsDialog } from '../../components/knowledge/NotebookSettingsDialog';
import * as knowledgeApi from '../../components/knowledge/knowledge-api';
import { lingxiFetch } from '../../hooks/use-hana-fetch';

vi.mock('../../components/knowledge/knowledge-api', () => ({
  updateKnowledgeNotebookSettings: vi.fn(),
}));

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: vi.fn(),
  lingxiUrl: (path: string) => path,
}));

const notebook = {
  id: 'notebook-a',
  studioId: 'studio-a',
  name: '产品资料',
  createdAt: '2026-08-25T00:00:00.000Z',
  updatedAt: '2026-08-25T00:00:00.000Z',
  deletedAt: null,
  config: {
    embeddingModelRef: null,
    rerankModelRef: null,
    chunkTargetChars: null,
    retrievalTopK: null,
    vectorRetentionDays: null,
  },
  chunkTargetCharsEffective: 6553,
  sourceCount: 2,
  ingestion: { done: 2, pendingEmbedding: 0, processing: 0, failed: 0, untracked: 0 },
};

const preferences = {
  operation_models: [
    { id: 'embed-0', provider: 'openai', displayName: 'OpenAI Embed', operations: ['embedding'] },
    { id: 'rerank-1', provider: 'volc', displayName: 'Volc Rerank', operations: ['rerank'] },
  ],
};

describe('NotebookSettingsDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.t = ((key: string, vars?: Record<string, string | number>) => {
      const labels: Record<string, string> = {
        'knowledge.notebookSettings': '笔记本设置',
        'knowledge.save': '保存',
        'knowledge.cancel': '取消',
        'knowledge.settingsEmbeddingModel': '嵌入模型',
        'knowledge.settingsRerankModel': '重排模型',
        'knowledge.settingsChunkTargetChars': '分块大小（字符）',
        'knowledge.settingsChunkAutoFallback': '自动（约 6553）',
        'knowledge.settingsRetrievalTopK': '检索数量',
        'knowledge.settingsTopKUnlimited': '无上限（默认）',
        'knowledge.settingsTopKMaxRecall': '最大召回数',
        'knowledge.settingsVectorRetention': '向量保留策略',
        'knowledge.settingsRetentionKeepForever': '永久保留（默认）',
        'knowledge.settingsRetentionDaysMode': '天数后清理',
        'knowledge.settingsVectorRetentionHint': '旧版向量超期未使用自动清理',
        'knowledge.settingsGlobalNotConfigured': '未配置',
        'knowledge.settingsInvalidNumber': '请输入 {min}–{max} 之间的整数',
      };
      let text = labels[key] || key;
      for (const [name, value] of Object.entries(vars || {})) {
        text = text.replace(`{${name}}`, String(value));
      }
      return text;
    }) as typeof window.t;
    vi.mocked(lingxiFetch).mockResolvedValue({
      json: async () => preferences,
    } as Response);
    vi.mocked(knowledgeApi.updateKnowledgeNotebookSettings).mockResolvedValue(notebook.config);
  });

  afterEach(() => {
    cleanup();
  });

  it('模型下拉无全局继承选项；分块尺寸只读展示自动计算值', async () => {
    render(<NotebookSettingsDialog notebook={notebook} onClose={() => {}} onSaved={() => {}} />);

    // 空选项是"未配置"（不再有全局继承文案）
    // 嵌入/重排两个下拉各有一个空选项"未配置"
    expect((await screen.findAllByRole('option', { name: '未配置' })).length).toBeGreaterThanOrEqual(2);
    expect(screen.queryByRole('option', { name: /继承全局/ })).not.toBeInTheDocument();

    // 分块尺寸只读展示生效值（server 的 chunkTargetCharsEffective）
    expect(screen.getByText('6553')).toBeInTheDocument();
    // 不存在分块数字输入框
    expect(screen.queryByRole('spinbutton', { name: '分块大小（字符）' })).not.toBeInTheDocument();
  });

  it('检索数量控件已移除：界面无该行，保存原样回传库内 retrievalTopK', async () => {
    const onSaved = vi.fn();
    render(<NotebookSettingsDialog notebook={notebook} onClose={() => {}} onSaved={onSaved} />);

    await screen.findAllByRole('option', { name: '未配置' });
    // 控件整体移除：标签/单选/数字输入都不存在（mock t 仍映射旧键，防回归）。
    expect(screen.queryByText('检索数量')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('无上限（默认）')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('最大召回数')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: '保存' }));
    await waitFor(() => expect(knowledgeApi.updateKnowledgeNotebookSettings).toHaveBeenCalledWith(
      'notebook-a',
      {
        embeddingModelRef: null,
        rerankModelRef: null,
        retrievalTopK: null,
        vectorRetentionDays: null,
      },
    ));
    expect(onSaved).toHaveBeenCalledTimes(1);
  });

  it('存量配置了召回上限的笔记本：控件移除后保存按原值回传（不清不改）', async () => {
    const limited = {
      ...notebook,
      config: { ...notebook.config, retrievalTopK: 20 },
    };
    render(<NotebookSettingsDialog notebook={limited} onClose={() => {}} onSaved={() => {}} />);
    await screen.findAllByRole('option', { name: '未配置' });

    expect(screen.queryByText('检索数量')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '保存' }));
    await waitFor(() => expect(knowledgeApi.updateKnowledgeNotebookSettings).toHaveBeenCalledWith(
      'notebook-a',
      expect.objectContaining({ retrievalTopK: 20 }),
    ));
  });

  it('保存失败内联展示并保持弹窗打开', async () => {
    const onSaved = vi.fn();
    vi.mocked(knowledgeApi.updateKnowledgeNotebookSettings).mockRejectedValue(new Error('boom'));
    render(<NotebookSettingsDialog notebook={notebook} onClose={() => {}} onSaved={onSaved} />);
    await screen.findAllByRole('option', { name: '未配置' });

    fireEvent.click(screen.getByRole('button', { name: '保存' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('boom');
    expect(onSaved).not.toHaveBeenCalled();
    expect(screen.getByRole('dialog')).toBeInTheDocument();
  });
});
