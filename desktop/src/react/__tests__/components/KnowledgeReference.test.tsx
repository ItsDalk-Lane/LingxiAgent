// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { KnowledgeReferenceBar } from '../../components/input/KnowledgeReferenceBar';
import { KnowledgeReferenceButton } from '../../components/input/KnowledgeReferenceButton';
import { listKnowledgeNotebooks, type KnowledgeNotebookDto } from '../../components/knowledge/knowledge-api';
import { useStore } from '../../stores';
import { selectKnowledgeRefsForSession } from '../../stores/knowledge-reference-slice';

vi.mock('../../components/knowledge/knowledge-api', () => ({
  listKnowledgeNotebooks: vi.fn(),
}));

function notebookDto(partial: Partial<KnowledgeNotebookDto> & Pick<KnowledgeNotebookDto, 'id' | 'name'>): KnowledgeNotebookDto {
  return {
    studioId: 'studio-1',
    createdAt: '2026-08-01T00:00:00Z',
    updatedAt: '2026-08-01T00:00:00Z',
    deletedAt: null,
    config: { embeddingModelRef: null, rerankModelRef: null, chunkTargetChars: null, retrievalTopK: null, vectorRetentionDays: null },
    sourceCount: 2,
    ingestion: { done: 2, pendingEmbedding: 0, processing: 0, failed: 0, untracked: 0 },
    ...partial,
  };
}

const NOTEBOOKS = [
  notebookDto({ id: 'nb-1', name: '产品笔记' }),
  notebookDto({ id: 'nb-2', name: '小说资料', sourceCount: 3, ingestion: { done: 2, pendingEmbedding: 1, processing: 0, failed: 0, untracked: 0 } }),
];

const SESSION = '/sessions/knowledge-ref.jsonl';

describe('knowledge reference UI', () => {
  beforeEach(() => {
    window.t = ((key: string, params?: Record<string, unknown>) => {
      const messages: Record<string, string> = {
        'input.knowledgeButton': '知识库',
        'input.knowledgeModeLabel': '知识库引用模式',
        'input.knowledgeModeFast': '快速',
        'input.knowledgeModeDetailed': '详细',
        'input.knowledgeModeQa': '问答',
        'input.knowledgeModeAssist': '辅助',
        'input.knowledgeModeFastHint': '纯本地快速检索；不等待远程嵌入、重排或多轮调查。',
        'input.knowledgeModeDetailedHint': '进行多轮检索、阅读和证据核对后生成详细回答。',
        'input.knowledgeModeQaHint': '严格基于检索内容回答，超出范围会明说',
        'input.knowledgeModeAssistHint': '检索内容作为参考，回答可结合对话与常识',
        'input.knowledgeRemoveNotebook': `移除知识库引用 ${params?.name ?? ''}`,
        'knowledge.loading': '正在读取…',
        'knowledge.loadFailed': '知识库读取失败',
        'knowledge.emptyNotebooks': '还没有笔记本。先建一个知识域，再添加来源。',
        'knowledge.sourceCount': `${params?.count ?? 0} 个来源`,
        'knowledge.statusReady': '已就绪',
        'knowledge.statusPendingIngestion': '待摄入',
        'knowledge.readinessPendingEmbedding': `${params?.count ?? 0} 待嵌入`,
        'knowledge.readinessProcessing': `${params?.count ?? 0} 摄入中`,
        'knowledge.readinessFailed': `${params?.count ?? 0} 失败`,
      };
      return messages[key] ?? key;
    }) as typeof window.t;
    vi.mocked(listKnowledgeNotebooks).mockReset();
    vi.mocked(listKnowledgeNotebooks).mockResolvedValue(NOTEBOOKS);
    useStore.setState({ locale: 'zh', knowledgeRefsBySession: {} } as never);
  });

  afterEach(() => {
    cleanup();
    useStore.setState({ knowledgeRefsBySession: {} } as never);
  });

  it('按钮打开菜单列出笔记本（名称 + 源数 + 就绪徽章），点击整体引用并保持菜单打开', async () => {
    render(<KnowledgeReferenceButton sessionKey={SESSION} />);

    fireEvent.click(screen.getByRole('button', { name: '知识库' }));
    expect(listKnowledgeNotebooks).toHaveBeenCalledTimes(1);

    const first = await screen.findByText('产品笔记');
    expect(screen.getByText('小说资料')).toBeInTheDocument();
    expect(screen.getByText('2 个来源')).toBeInTheDocument();
    expect(screen.getByText('已就绪')).toBeInTheDocument();
    expect(screen.getByText('1 待嵌入')).toBeInTheDocument();

    fireEvent.click(first);
    await waitFor(() => {
      expect(selectKnowledgeRefsForSession(useStore.getState(), SESSION)?.notebookIds).toEqual(['nb-1']);
    });
    // 多选场景：选择后菜单不关闭，可继续点第二个笔记本
    expect(screen.getByText('小说资料')).toBeInTheDocument();
    fireEvent.click(screen.getByText('小说资料'));
    await waitFor(() => {
      expect(selectKnowledgeRefsForSession(useStore.getState(), SESSION)?.notebookIds).toEqual(['nb-1', 'nb-2']);
    });
    expect(screen.getByRole('menuitemcheckbox', { name: /产品笔记/ })).toHaveAttribute('aria-checked', 'true');

    // 再点已引用项 = 取消引用
    fireEvent.click(screen.getByRole('menuitemcheckbox', { name: /产品笔记/ }));
    await waitFor(() => {
      expect(selectKnowledgeRefsForSession(useStore.getState(), SESSION)?.notebookIds).toEqual(['nb-2']);
    });
  });

  it('列表读取失败时显示错误而非静默空白', async () => {
    vi.mocked(listKnowledgeNotebooks).mockRejectedValue(new Error('boom'));
    render(<KnowledgeReferenceButton sessionKey={SESSION} />);
    fireEvent.click(screen.getByRole('button', { name: '知识库' }));
    expect(await screen.findByText('知识库读取失败')).toBeInTheDocument();
  });

  it('存在 untracked 源的笔记本显示「待摄入」而非就绪', async () => {
    vi.mocked(listKnowledgeNotebooks).mockResolvedValue([
      notebookDto({
        id: 'nb-untracked',
        name: '未摄入资料',
        ingestion: { done: 0, pendingEmbedding: 0, processing: 0, failed: 0, untracked: 2 },
      }),
    ]);
    render(<KnowledgeReferenceButton sessionKey={SESSION} />);
    fireEvent.click(screen.getByRole('button', { name: '知识库' }));
    expect(await screen.findByText('待摄入')).toBeInTheDocument();
    expect(screen.queryByText('已就绪')).not.toBeInTheDocument();
  });

  it('无会话键时按钮禁用', () => {
    render(<KnowledgeReferenceButton sessionKey={null} />);
    expect(screen.getByRole('button', { name: '知识库' })).toBeDisabled();
  });

  it('引用条渲染已引用笔记本 chip 与模式切换，× 移除单个引用', async () => {
    useStore.setState({
      knowledgeRefsBySession: {
        [SESSION]: { notebookIds: ['nb-1', 'nb-2'], notebookNames: { 'nb-1': '产品笔记' }, mode: 'fast' },
      },
    } as never);
    render(<KnowledgeReferenceBar sessionKey={SESSION} />);

    // 名称解析：nb-1 先走名称缓存，列表到达后 nb-2 也显示名称
    expect(await screen.findByText('小说资料')).toBeInTheDocument();
    expect(screen.getByText('产品笔记')).toBeInTheDocument();

    const fastBtn = screen.getByRole('button', { name: '快速' });
    const detailedBtn = screen.getByRole('button', { name: '详细' });
    expect(fastBtn).toHaveAttribute('title', '纯本地快速检索；不等待远程嵌入、重排或多轮调查。');
    expect(detailedBtn).toHaveAttribute('aria-description', '进行多轮检索、阅读和证据核对后生成详细回答。');
    expect(fastBtn).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(detailedBtn);
    expect(selectKnowledgeRefsForSession(useStore.getState(), SESSION)?.mode).toBe('detailed');
    expect(detailedBtn).toHaveAttribute('aria-pressed', 'true');

    fireEvent.click(screen.getByLabelText('移除知识库引用 产品笔记'));
    expect(selectKnowledgeRefsForSession(useStore.getState(), SESSION)?.notebookIds).toEqual(['nb-2']);
  });

  it('列表未加载时 chip 退回名称缓存；脏 id 兜底显示原始 id 且可移除', async () => {
    vi.mocked(listKnowledgeNotebooks).mockRejectedValue(new Error('offline'));
    useStore.setState({
      knowledgeRefsBySession: {
        [SESSION]: { notebookIds: ['nb-1', 'nb-deleted'], notebookNames: { 'nb-1': '产品笔记' }, mode: 'detailed' },
      },
    } as never);
    render(<KnowledgeReferenceBar sessionKey={SESSION} />);

    expect(screen.getByText('产品笔记')).toBeInTheDocument();
    // 脏 id：列表里已不存在的笔记本保留显示原始 id，可手动移除
    expect(screen.getByText('nb-deleted')).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('移除知识库引用 nb-deleted'));
    expect(selectKnowledgeRefsForSession(useStore.getState(), SESSION)?.notebookIds).toEqual(['nb-1']);
    // 模式沿用会话内既有设置
    expect(selectKnowledgeRefsForSession(useStore.getState(), SESSION)?.mode).toBe('detailed');
  });

  it('无引用时引用条不渲染', () => {
    const { container } = render(<KnowledgeReferenceBar sessionKey={SESSION} />);
    expect(container).toBeEmptyDOMElement();
  });
});
