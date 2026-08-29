// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { KnowledgePage } from '../../components/knowledge/KnowledgePage';
import * as knowledgeApi from '../../components/knowledge/knowledge-api';
import { useStore } from '../../stores';

vi.mock('../../components/knowledge/knowledge-api', () => ({
  listKnowledgeNotebooks: vi.fn(),
  createKnowledgeNotebook: vi.fn(),
  renameKnowledgeNotebook: vi.fn(),
  deleteKnowledgeNotebook: vi.fn(),
  updateKnowledgeNotebookSettings: vi.fn(),
  listKnowledgeSources: vi.fn(),
  importKnowledgeFileSource: vi.fn(),
  importKnowledgePastedText: vi.fn(),
  importKnowledgeWebSnapshot: vi.fn(),
  removeKnowledgeSource: vi.fn(),
  refreshKnowledgeSource: vi.fn(),
  reingestKnowledgeSource: vi.fn(),
  listKnowledgeIngestion: vi.fn(),
  listKnowledgeBlocks: vi.fn(),
  listKnowledgeChunks: vi.fn(),
  resolveKnowledgeCitation: vi.fn(),
  knowledgeSnapshotContentUrl: vi.fn((id: string) => `http://127.0.0.1/content/${id}`),
}));

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: vi.fn(async () => ({
    json: async () => ({ models: {}, operation_models: [] }),
  })),
  lingxiUrl: (path: string) => path,
}));

const emptyIngestionCounts = { queued: 0, running: 0, pending_embedding: 0, failed: 0, done: 0 };

const notebookA = {
  id: 'notebook-a',
  studioId: 'studio-a',
  name: '产品资料',
  createdAt: '2026-08-25T00:00:00.000Z',
  updatedAt: '2026-08-25T00:00:00.000Z',
  deletedAt: null,
  config: {
    embeddingModelRef: null,
    rerankModelRef: null,
    chunkTargetChars: 1200,
    retrievalTopK: 12,
  },
  sourceCount: 1,
  ingestion: { done: 1, pendingEmbedding: 0, processing: 0, failed: 0, untracked: 0 },
};

const notebookB = {
  ...notebookA,
  id: 'notebook-b',
  name: '市场资料',
};

const sourceEntry = {
  source: {
    id: 'source-a',
    studioId: 'studio-a',
    sourceType: 'file' as const,
    displayName: 'roadmap.md',
    originMetadata: { kind: 'file', fileName: 'roadmap.md' },
    createdAt: '2026-08-25T00:00:00.000Z',
    deletedAt: null,
  },
  snapshot: {
    id: 'snapshot-a',
    sourceId: 'source-a',
    sha256: 'a'.repeat(64),
    mimeType: 'text/markdown',
    byteSize: 128,
    capturedAt: '2026-08-25T00:00:00.000Z',
  },
  membership: {
    notebookId: 'notebook-a',
    sourceId: 'source-a',
    addedAt: '2026-08-25T00:00:00.000Z',
    removedAt: null,
  },
  parseArtifact: {
    id: 'artifact-a',
    contentSnapshotId: 'snapshot-a',
    parserId: 'markdown',
    parserVersion: '1',
    parserConfigHash: 'config',
    status: 'ready' as const,
    warnings: [],
    createdAt: '2026-08-25T00:00:00.000Z',
    completedAt: '2026-08-25T00:00:00.000Z',
  },
};

const block = {
  id: 'block-a',
  parseArtifactId: 'artifact-a',
  ordinal: 0,
  text: '引用级原文内容',
  textSha256: 'b'.repeat(64),
  locatorType: 'markdown' as const,
  locator: { lineStart: 3, lineEnd: 3, headingPath: ['路线图'] },
};

const chunkText = '这是分块的完整正文，用于验证详情层全文展示。'.repeat(11);

const chunk = {
  id: 'chunk-a',
  ordinal: 1,
  text: chunkText,
  tokenCount: 420,
  charCount: chunkText.length,
  headingPath: ['路线图', '里程碑'],
};

describe('KnowledgePage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.t = ((key: string, vars?: Record<string, string | number>) => {
      const labels: Record<string, string> = {
        'knowledge.tab': '知识',
        'knowledge.title': '知识库',
        'knowledge.sources': '来源',
        'knowledge.newNotebook': '新建笔记本',
        'knowledge.notebookName': '笔记本名称',
        'knowledge.create': '创建',
        'knowledge.addSource': '添加来源',
        'knowledge.addLocalFile': '选择本地文件',
        'knowledge.addPastedText': '粘贴文本',
        'knowledge.addWebSnapshot': '网页快照',
        'knowledge.sourceDisplayNameOptional': '来源名称（可选）',
        'knowledge.pastedTextPlaceholder': '粘贴需要保存和引用的文字…',
        'knowledge.webUrlPlaceholder': 'https://example.com/page',
        'knowledge.importSource': '导入',
        'knowledge.sourceViewer': '来源查看器',
        'knowledge.chunkViewer': '分块内容查看器',
        'knowledge.closeViewer': '返回分析区',
        'knowledge.statusReady': '已就绪',
        'knowledge.statusPendingIngestion': '待摄入',
        'knowledge.sourceActions': '来源操作',
        'knowledge.sourceMenuPreview': '预览原文',
        'knowledge.sourceMenuChunks': '查看分块内容',
        'knowledge.sourceMenuNotReadyHint': '解析完成后可用',
        'knowledge.chunkCount': '{count} 个分块',
        'knowledge.chunkDetailTitle': '分块 #{ordinal}',
        'knowledge.chunkTokenCount': '{count} tokens',
        'knowledge.chunkCharCount': '{count} 字符',
        'knowledge.noChunks': '分块尚未生成，等待摄入完成。',
        'common.close': '关闭',
        'knowledge.lineNumber': '第 {number} 行',
        'knowledge.removeSource': '从笔记本移除 {name}',
        'knowledge.notebookSettings': '笔记本设置',
        'knowledge.save': '保存',
        'knowledge.cancel': '取消',
        'knowledge.selectSource': '选择一个来源查看解析内容',
        'knowledge.ingestionQueued': '排队中',
        'knowledge.ingestionEmbedProgress': '嵌入中 {done}/{total}',
        'knowledge.ingestionPendingEmbedding': '待嵌入',
        'knowledge.ingestionFailed': '摄入失败',
        'knowledge.retryIngestion': '重试摄入 {name}',
      };
      let text = labels[key] || key;
      for (const [name, value] of Object.entries(vars || {})) {
        text = text.replace(`{${name}}`, String(value));
      }
      return text;
    }) as typeof window.t;
    Object.defineProperty(window, 'platform', {
      configurable: true,
      value: { selectFiles: vi.fn(async () => []) },
    });
    useStore.setState({ addToast: vi.fn() } as never);
    vi.mocked(knowledgeApi.listKnowledgeNotebooks).mockResolvedValue([notebookA, notebookB]);
    vi.mocked(knowledgeApi.listKnowledgeSources).mockResolvedValue([sourceEntry]);
    vi.mocked(knowledgeApi.listKnowledgeBlocks).mockResolvedValue([block]);
    vi.mocked(knowledgeApi.listKnowledgeChunks).mockResolvedValue({ chunkerConfigId: 'cfg-a', chunks: [chunk] });
    vi.mocked(knowledgeApi.listKnowledgeIngestion).mockResolvedValue({
      jobs: [],
      counts: { ...emptyIngestionCounts },
    });
  });

  afterEach(() => {
    cleanup();
  });

  it('opens a source action menu and previews original blocks from it', async () => {
    render(<KnowledgePage />);

    expect(await screen.findByRole('heading', { name: '知识库' })).toBeInTheDocument();
    expect(await screen.findByText('roadmap.md')).toBeInTheDocument();

    // 摄入数据加载完成后，无 job 的源显示「待摄入」而非 parse 状态徽章
    expect(await screen.findByText('待摄入')).toBeInTheDocument();
    expect(screen.queryByText('准备完成')).not.toBeInTheDocument();

    fireEvent.click(screen.getByText('roadmap.md'));
    const menu = await screen.findByRole('menu', { name: '来源操作' });
    expect(within(menu).getByRole('menuitem', { name: '预览原文' })).toBeEnabled();
    expect(within(menu).getByRole('menuitem', { name: '查看分块内容' })).toBeEnabled();

    fireEvent.click(within(menu).getByRole('menuitem', { name: '预览原文' }));

    expect(await screen.findByText('引用级原文内容')).toBeInTheDocument();
    expect(screen.getByText('第 3 行')).toBeInTheDocument();
    expect(knowledgeApi.listKnowledgeBlocks).toHaveBeenCalledWith('artifact-a');
  });

  it('opens the chunk cards view from the menu and a detail dialog per card', async () => {
    render(<KnowledgePage />);
    await screen.findByText('roadmap.md');

    fireEvent.click(screen.getByText('roadmap.md'));
    fireEvent.click(await screen.findByRole('menuitem', { name: '查看分块内容' }));

    expect(await waitFor(() => expect(knowledgeApi.listKnowledgeChunks).toHaveBeenCalledWith('artifact-a')));
    expect(await screen.findByText('1 个分块')).toBeInTheDocument();
    const card = screen.getByRole('button', { name: /#1/ });
    expect(card).toHaveTextContent('路线图 / 里程碑');
    // 卡片只显示前 200 字符预览，不携带全文
    expect(card).toHaveTextContent(`${chunkText.slice(0, 200)}…`);
    expect(card).not.toHaveTextContent(chunkText);

    fireEvent.click(card);
    const dialog = await screen.findByRole('dialog');
    expect(dialog).toHaveTextContent('分块 #1');
    expect(dialog).toHaveTextContent('420 tokens');
    expect(dialog).toHaveTextContent(`${chunkText.length} 字符`);
    expect(dialog).toHaveTextContent(chunkText);

    fireEvent.click(within(dialog).getByRole('button', { name: '关闭' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  });

  it('disables source menu entries until the parse artifact is ready', async () => {
    const parsingEntry = {
      ...sourceEntry,
      parseArtifact: { ...sourceEntry.parseArtifact, status: 'parsing' as const, completedAt: null },
    };
    vi.mocked(knowledgeApi.listKnowledgeSources).mockResolvedValue([parsingEntry]);

    render(<KnowledgePage />);
    await screen.findByText('roadmap.md');

    fireEvent.click(screen.getByText('roadmap.md'));
    const menu = await screen.findByRole('menu', { name: '来源操作' });
    expect(within(menu).getByRole('menuitem', { name: '预览原文' })).toBeDisabled();
    expect(within(menu).getByRole('menuitem', { name: '查看分块内容' })).toBeDisabled();
    expect(knowledgeApi.listKnowledgeBlocks).not.toHaveBeenCalled();
    expect(knowledgeApi.listKnowledgeChunks).not.toHaveBeenCalled();
  });

  it('shows a single job-driven badge per source: done = ready, no job = pending ingestion', async () => {
    const doneJob = {
      id: 'job-done',
      notebookId: 'notebook-a',
      sourceId: 'source-a',
      artifactId: 'artifact-a',
      phase: 'done',
      status: 'done',
      attempt: 1,
      retryAfter: null,
      error: null,
      chunkerConfigId: 'cfg',
      createdAt: '2026-08-25T00:00:00.000Z',
      updatedAt: '2026-08-25T00:00:00.000Z',
    } as const;
    const untrackedEntry = {
      ...sourceEntry,
      source: { ...sourceEntry.source, id: 'source-c', displayName: 'draft.md' },
      snapshot: { ...sourceEntry.snapshot, id: 'snapshot-c', sourceId: 'source-c' },
      membership: { ...sourceEntry.membership, sourceId: 'source-c' },
    };
    vi.mocked(knowledgeApi.listKnowledgeSources).mockResolvedValue([sourceEntry, untrackedEntry]);
    vi.mocked(knowledgeApi.listKnowledgeIngestion).mockResolvedValue({
      jobs: [doneJob],
      counts: { ...emptyIngestionCounts, done: 1 },
    });

    render(<KnowledgePage />);
    expect(await screen.findByText('已就绪')).toBeInTheDocument();
    expect(await screen.findByText('待摄入')).toBeInTheDocument();
  });

  it('shows embed progress counts and a thin progress bar while embedding', async () => {
    const embedJob = {
      id: 'job-embed',
      notebookId: 'notebook-a',
      sourceId: 'source-a',
      artifactId: 'artifact-a',
      phase: 'embed',
      status: 'running',
      attempt: 1,
      retryAfter: null,
      error: null,
      chunkerConfigId: 'cfg',
      createdAt: '2026-08-25T00:00:00.000Z',
      updatedAt: '2026-08-25T00:00:00.000Z',
      progressDone: 320,
      progressTotal: 708,
    } as const;
    vi.mocked(knowledgeApi.listKnowledgeIngestion).mockResolvedValue({
      jobs: [embedJob],
      counts: { ...emptyIngestionCounts, running: 1 },
    });

    render(<KnowledgePage />);
    expect(await screen.findByText('嵌入中 320/708')).toBeInTheDocument();

    const bar = screen.getByRole('progressbar');
    expect(bar).toHaveAttribute('aria-valuenow', '320');
    expect(bar).toHaveAttribute('aria-valuemax', '708');
    const fill = bar.firstElementChild as HTMLElement;
    expect(fill.style.width).toBe('45%');
  });

  it('creates a notebook and imports selected local files through the native picker', async () => {
    const notebookC = { ...notebookA, id: 'notebook-c', name: '新资料' };
    vi.mocked(knowledgeApi.createKnowledgeNotebook).mockResolvedValue(notebookC);
    vi.mocked(knowledgeApi.listKnowledgeNotebooks)
      .mockResolvedValueOnce([notebookA, notebookB])
      .mockResolvedValueOnce([notebookA, notebookB, notebookC]);
    vi.mocked(knowledgeApi.importKnowledgeFileSource).mockResolvedValue(sourceEntry);
    vi.mocked(knowledgeApi.listKnowledgeSources)
      .mockResolvedValueOnce([sourceEntry])
      .mockResolvedValueOnce([sourceEntry]);
    vi.mocked(window.platform.selectFiles).mockResolvedValue(['/tmp/a.md', '/tmp/b.pdf']);

    render(<KnowledgePage />);
    await screen.findByText('roadmap.md');

    fireEvent.click(screen.getAllByRole('button', { name: '新建笔记本' })[0]);
    fireEvent.change(screen.getByPlaceholderText('笔记本名称'), { target: { value: '新资料' } });
    fireEvent.click(screen.getByRole('button', { name: '创建' }));

    await waitFor(() => expect(knowledgeApi.createKnowledgeNotebook).toHaveBeenCalledWith('新资料'));
    await waitFor(() => expect(knowledgeApi.listKnowledgeSources).toHaveBeenCalledWith('notebook-c'));

    fireEvent.click(screen.getByRole('button', { name: /添加来源/ }));
    fireEvent.click(screen.getByRole('menuitem', { name: '选择本地文件' }));
    await waitFor(() => expect(knowledgeApi.importKnowledgeFileSource).toHaveBeenCalledTimes(2));
    expect(knowledgeApi.importKnowledgeFileSource).toHaveBeenNthCalledWith(1, 'notebook-c', '/tmp/a.md');
    expect(knowledgeApi.importKnowledgeFileSource).toHaveBeenNthCalledWith(2, 'notebook-c', '/tmp/b.pdf');
  });

  it('imports pasted text and a frozen web snapshot without a local path picker', async () => {
    vi.mocked(knowledgeApi.importKnowledgePastedText).mockResolvedValue({
      ...sourceEntry,
      source: { ...sourceEntry.source, sourceType: 'pasted_text' },
    });
    vi.mocked(knowledgeApi.importKnowledgeWebSnapshot).mockResolvedValue({
      ...sourceEntry,
      source: {
        ...sourceEntry.source,
        sourceType: 'web_snapshot',
        originMetadata: { kind: 'web_snapshot', url: 'https://example.com/page' },
      },
    });

    render(<KnowledgePage />);
    await screen.findByText('roadmap.md');

    fireEvent.click(screen.getByRole('button', { name: /添加来源/ }));
    fireEvent.click(screen.getByRole('menuitem', { name: '粘贴文本' }));
    fireEvent.change(screen.getByPlaceholderText('来源名称（可选）'), { target: { value: '会议纪要' } });
    fireEvent.change(screen.getByPlaceholderText('粘贴需要保存和引用的文字…'), {
      target: { value: '冻结这段原文。' },
    });
    fireEvent.click(screen.getByRole('button', { name: '导入' }));
    await waitFor(() => expect(knowledgeApi.importKnowledgePastedText).toHaveBeenCalledWith('notebook-a', {
      text: '冻结这段原文。',
      displayName: '会议纪要',
    }));

    fireEvent.click(screen.getByRole('button', { name: /添加来源/ }));
    fireEvent.click(screen.getByRole('menuitem', { name: '网页快照' }));
    fireEvent.change(screen.getByPlaceholderText('https://example.com/page'), {
      target: { value: 'https://example.com/page' },
    });
    fireEvent.click(screen.getByRole('button', { name: '导入' }));
    await waitFor(() => expect(knowledgeApi.importKnowledgeWebSnapshot).toHaveBeenCalledWith('notebook-a', {
      url: 'https://example.com/page',
      displayName: undefined,
    }));
    expect(window.platform.selectFiles).not.toHaveBeenCalled();
  });

  it('shows ingestion badges for pending and failed sources and retries a failed job', async () => {
    const failedJob = {
      id: 'job-1',
      notebookId: 'notebook-a',
      sourceId: 'source-a',
      artifactId: 'artifact-a',
      phase: 'embed',
      status: 'failed',
      attempt: 3,
      retryAfter: null,
      error: 'KNOWLEDGE_RETRIEVAL_UNAVAILABLE: boom',
      chunkerConfigId: 'cfg',
      createdAt: '2026-08-25T00:00:00.000Z',
      updatedAt: '2026-08-25T00:00:00.000Z',
    } as const;
    const pendingJob = {
      ...failedJob,
      id: 'job-2',
      sourceId: 'source-b',
      status: 'pending_embedding',
      attempt: 0,
      error: null,
    } as const;
    const pendingEntry = {
      ...sourceEntry,
      source: { ...sourceEntry.source, id: 'source-b', displayName: 'notes.txt' },
      snapshot: { ...sourceEntry.snapshot, id: 'snapshot-b', sourceId: 'source-b' },
      membership: { ...sourceEntry.membership, sourceId: 'source-b' },
    };
    vi.mocked(knowledgeApi.listKnowledgeSources).mockResolvedValue([sourceEntry, pendingEntry]);
    vi.mocked(knowledgeApi.listKnowledgeIngestion).mockResolvedValue({
      jobs: [failedJob, pendingJob],
      counts: { ...emptyIngestionCounts, failed: 1, pending_embedding: 1 },
    });
    vi.mocked(knowledgeApi.reingestKnowledgeSource).mockResolvedValue({
      job: { ...failedJob, status: 'queued', attempt: 0 },
      retried: true,
    });

    render(<KnowledgePage />);
    expect(await screen.findByText('摄入失败')).toBeInTheDocument();
    expect(await screen.findByText('待嵌入')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: '重试摄入 roadmap.md' }));
    await waitFor(() => expect(knowledgeApi.reingestKnowledgeSource)
      .toHaveBeenCalledWith('notebook-a', 'source-a'));
  });

  it('opens the notebook settings dialog from the source pane header', async () => {
    render(<KnowledgePage />);
    await screen.findByText('roadmap.md');

    fireEvent.click(screen.getByRole('button', { name: '笔记本设置' }));
    const dialog = await screen.findByRole('dialog');
    expect(dialog).toHaveTextContent('笔记本设置 · 产品资料');

    fireEvent.click(screen.getByRole('button', { name: '取消' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  });
});
