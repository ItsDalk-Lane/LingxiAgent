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
  listKnowledgeSources: vi.fn(),
  importKnowledgeFileSource: vi.fn(),
  importKnowledgePastedText: vi.fn(),
  importKnowledgeWebSnapshot: vi.fn(),
  removeKnowledgeSource: vi.fn(),
  refreshKnowledgeSource: vi.fn(),
  listKnowledgeBlocks: vi.fn(),
  runKnowledgeQuickAnswer: vi.fn(),
  runKnowledgeResearch: vi.fn(),
  listActiveKnowledgeResearchRuns: vi.fn(),
  getKnowledgeResearchRun: vi.fn(),
  getKnowledgeResearchReport: vi.fn(),
  cancelKnowledgeResearch: vi.fn(),
  resolveKnowledgeCitation: vi.fn(),
  knowledgeSnapshotContentUrl: vi.fn((id: string) => `http://127.0.0.1/content/${id}`),
}));

const notebookA = {
  id: 'notebook-a',
  studioId: 'studio-a',
  name: '产品资料',
  createdAt: '2026-08-25T00:00:00.000Z',
  updatedAt: '2026-08-25T00:00:00.000Z',
  deletedAt: null,
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
        'knowledge.analysis': '分析',
        'knowledge.askYourKnowledge': '向你的知识库提问',
        'knowledge.scope': '本次来源',
        'knowledge.addNotebook': '添加笔记本',
        'knowledge.sourceViewer': '来源查看器',
        'knowledge.closeViewer': '返回分析区',
        'knowledge.statusReady': '准备完成',
        'knowledge.lineNumber': '第 {number} 行',
        'knowledge.removeSource': '从笔记本移除 {name}',
        'knowledge.quickAnswer': '标准回答',
        'knowledge.fullResearch': '全文研究',
        'knowledge.questionPlaceholder': '输入想了解的问题…',
        'knowledge.send': '发送',
        'knowledge.quickRetrievalHint': '基于相关内容检索，不代表已阅读全部来源',
        'knowledge.researching': '正在研究…',
        'knowledge.researchResult': '全文研究结果',
        'knowledge.cancelResearch': '取消研究',
        'knowledge.coverage': '研究覆盖',
        'knowledge.coverageSourceReadiness': '来源准备',
        'knowledge.coverageExtraction': '内容提取',
        'knowledge.coveragePrimaryScan': '全文扫描',
        'knowledge.coverageContradiction': '矛盾检查',
        'knowledge.coverageCitationValidation': '证据验证',
        'knowledge.researchConclusions': '研究结论',
        'knowledge.researchMajorFindings': '主要发现',
        'knowledge.researchConflicts': '冲突与反例',
        'knowledge.researchUncertainties': '不确定点',
        'knowledge.researchLimitations': '研究限制',
        'knowledge.researchStateScanning': '全文扫描',
        'knowledge.researchStateRecovering': '正在恢复',
        'knowledge.researchStateCompleted': '研究完成',
        'knowledge.answerResult': '回答结果',
        'knowledge.relatedContentBasis': '基于相关内容检索',
        'knowledge.citations': '引用',
        'knowledge.openCitation': '打开引用 {number}',
        'knowledge.sourceSnapshots': '来源快照',
        'knowledge.snapshotShortId': '快照 {id}',
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
    vi.mocked(knowledgeApi.listActiveKnowledgeResearchRuns).mockResolvedValue([]);
  });

  afterEach(() => {
    cleanup();
  });

  it('shows the three native areas and opens citation-grade source blocks', async () => {
    render(<KnowledgePage />);

    expect(await screen.findByRole('heading', { name: '知识库' })).toBeInTheDocument();
    expect(await screen.findByText('roadmap.md')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: '向你的知识库提问' })).toBeInTheDocument();

    fireEvent.click(screen.getByText('roadmap.md'));

    expect(await screen.findByText('引用级原文内容')).toBeInTheDocument();
    expect(screen.getByText('第 3 行')).toBeInTheDocument();
    expect(knowledgeApi.listKnowledgeBlocks).toHaveBeenCalledWith('artifact-a');
  });

  it('keeps source viewing separate from notebook-only query scope', async () => {
    render(<KnowledgePage />);
    await screen.findByText('roadmap.md');

    const scopeArea = screen.getByText('本次来源').parentElement;
    expect(scopeArea).not.toBeNull();
    expect(within(scopeArea as HTMLElement).getByRole('button', { name: '产品资料' })).toBeInTheDocument();
    const sourcePane = screen.getByText('来源').closest('section');
    expect(sourcePane).not.toBeNull();
    expect(within(sourcePane as HTMLElement).queryByRole('checkbox')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /添加笔记本/ }));
    const marketCheckbox = screen.getByRole('checkbox', { name: '市场资料' });
    fireEvent.click(marketCheckbox);
    expect(within(scopeArea as HTMLElement).getByRole('button', { name: '市场资料 ×' })).toBeInTheDocument();

    fireEvent.click(screen.getByText('roadmap.md'));
    await screen.findByText('引用级原文内容');
    fireEvent.click(screen.getByRole('button', { name: '返回分析区' }));

    expect(screen.getByRole('button', { name: '产品资料 ×' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '市场资料 ×' })).toBeInTheDocument();
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

  it('sends only notebook scope, labels the retrieval basis, and opens a historical citation', async () => {
    const citation = {
      id: 'citation-a',
      parseArtifactId: 'artifact-a',
      blockId: 'block-a',
      startOffset: 5,
      endOffset: 10,
      canonicalText: '九月十五日',
      canonicalTextSha256: 'c'.repeat(64),
      createdAt: '2026-08-25T00:00:00.000Z',
    };
    const citationBlock = {
      ...block,
      text: '交付日期是九月十五日。',
      locator: { lineStart: 8, lineEnd: 8 },
    };
    vi.mocked(knowledgeApi.runKnowledgeQuickAnswer).mockResolvedValue({
      retrievalBasis: 'related_content',
      run: {
        id: 'run-a',
        studioId: 'studio-a',
        mode: 'quick',
        question: '什么时候交付？',
        scopeSnapshotId: 'scope-a',
        status: 'completed',
        retrievalMode: 'fts',
        answerText: '交付日期是九月十五日。 [1]',
        errorCode: null,
        createdAt: '2026-08-25T00:00:00.000Z',
        completedAt: '2026-08-25T00:00:01.000Z',
        citations: [{
          runId: 'run-a',
          ordinal: 0,
          marker: 1,
          citationId: 'citation-a',
          candidateRef: 'K1',
        }],
        retrievals: [],
      },
      scope: {
        id: 'scope-a',
        studioId: 'studio-a',
        mode: 'quick',
        createdAt: '2026-08-25T00:00:00.000Z',
        notebooks: [{
          scopeSnapshotId: 'scope-a',
          notebookId: 'notebook-a',
          notebookName: '产品资料',
          ordinal: 0,
        }],
        sources: [],
      },
      citations: [{
        marker: 1,
        citation,
        source: { ...sourceEntry.source, displayName: '历史版本.md' },
        snapshot: sourceEntry.snapshot,
        parseArtifact: sourceEntry.parseArtifact,
        locator: citationBlock.locator,
        viewer: { contentUrl: '/content/snapshot-a', locator: citationBlock.locator },
      }],
    });
    vi.mocked(knowledgeApi.resolveKnowledgeCitation).mockResolvedValue({
      citation,
      block: citationBlock,
      source: { ...sourceEntry.source, displayName: '历史版本.md' },
      snapshot: sourceEntry.snapshot,
      parseArtifact: sourceEntry.parseArtifact,
      viewer: { contentUrl: '/content/snapshot-a', locator: citationBlock.locator },
    });

    render(<KnowledgePage />);
    await screen.findByText('roadmap.md');
    const question = screen.getByPlaceholderText('输入想了解的问题…');
    const send = screen.getByRole('button', { name: '发送' });
    expect(screen.getByText('基于相关内容检索，不代表已阅读全部来源')).toBeInTheDocument();
    expect(send).toBeDisabled();

    fireEvent.change(question, { target: { value: '什么时候交付？' } });
    expect(send).toBeEnabled();
    fireEvent.click(send);

    await waitFor(() => expect(knowledgeApi.runKnowledgeQuickAnswer).toHaveBeenCalledWith({
      question: '什么时候交付？',
      notebookIds: ['notebook-a'],
    }));
    expect(await screen.findByText('基于相关内容检索')).toBeInTheDocument();
    expect(screen.getByText('历史版本.md')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: '打开引用 1' }));
    await waitFor(() => expect(knowledgeApi.resolveKnowledgeCitation).toHaveBeenCalledWith('citation-a'));
    expect(await screen.findByRole('heading', { name: '历史版本.md' })).toBeInTheDocument();
    expect(screen.getByText('九月十五日', { selector: 'mark' })).toBeInTheDocument();
    expect(screen.getByText('第 8 行')).toBeInTheDocument();
  });

  it('keeps Send disabled when no Notebook exists even after a question is entered', async () => {
    vi.mocked(knowledgeApi.listKnowledgeNotebooks).mockResolvedValue([]);
    vi.mocked(knowledgeApi.listKnowledgeSources).mockResolvedValue([]);

    render(<KnowledgePage />);
    await screen.findByRole('heading', { name: '知识库' });
    fireEvent.change(screen.getByPlaceholderText('输入想了解的问题…'), {
      target: { value: '没有范围时能发送吗？' },
    });

    expect(screen.getByRole('button', { name: '发送' })).toBeDisabled();
    expect(knowledgeApi.runKnowledgeQuickAnswer).not.toHaveBeenCalled();
    expect(knowledgeApi.runKnowledgeResearch).not.toHaveBeenCalled();
  });

  it('runs Full Research, shows separate coverage ledgers, and opens report citations', async () => {
    const coverage = {
      sourceReadiness: { completed: 1, total: 1 },
      extraction: { completed: 1, total: 1 },
      primaryScan: { completed: 1, total: 1 },
      contradiction: { completed: 1, total: 1 },
      citationValidation: { completed: 1, total: 1, valid: 1, invalid: 0 },
    };
    const run = {
      id: 'research-a',
      studioId: 'studio-a',
      mode: 'research' as const,
      question: '完整结论是什么？',
      scopeSnapshotId: 'scope-r',
      status: 'running' as const,
      retrievalMode: 'fts' as const,
      answerText: null,
      errorCode: null,
      createdAt: '2026-08-25T00:00:00.000Z',
      completedAt: null,
      citations: [],
      retrievals: [],
    };
    const scope = {
      id: 'scope-r',
      studioId: 'studio-a',
      mode: 'research' as const,
      createdAt: '2026-08-25T00:00:00.000Z',
      notebooks: [{
        scopeSnapshotId: 'scope-r',
        notebookId: 'notebook-a',
        notebookName: '产品资料',
        ordinal: 0,
      }],
      sources: [{
        scopeSnapshotId: 'scope-r',
        notebookId: 'notebook-a',
        sourceId: 'source-a',
        sourceDisplayName: 'roadmap.md',
        contentSnapshotId: 'snapshot-a',
        parseArtifactId: 'artifact-a',
        ordinal: 0,
      }],
    };
    const research = {
      runId: run.id,
      hostTaskId: `knowledge-research:${run.id}`,
      state: 'scanning' as const,
      spec: {
        originalQuestion: run.question,
        scopeSnapshotId: scope.id,
        notebookIds: ['notebook-a'],
        goal: run.question,
        dimensions: [],
        outputRequirements: [],
        definitions: [],
        assumptions: [],
      },
      manifest: {
        runId: run.id,
        sourceCount: 1,
        parseArtifactCount: 1,
        blockCount: 1,
        unitCount: 1,
        primaryCharCount: 10,
        createdAt: run.createdAt,
      },
      coverage: { ...coverage, primaryScan: { completed: 0, total: 1 }, contradiction: { completed: 0, total: 1 } },
      reportAvailable: false,
      errorCode: null,
      createdAt: run.createdAt,
      updatedAt: run.createdAt,
      completedAt: null,
    };
    vi.mocked(knowledgeApi.runKnowledgeResearch).mockResolvedValue({ run, scope, research, citations: [] });
    vi.mocked(knowledgeApi.getKnowledgeResearchRun).mockResolvedValue({
      run: { ...run, status: 'completed', answerText: '完整报告', completedAt: '2026-08-25T00:00:01.000Z' },
      scope,
      research: {
        ...research,
        state: 'completed',
        coverage,
        reportAvailable: true,
        completedAt: '2026-08-25T00:00:01.000Z',
      },
      citations: [],
    });
    const citation = {
      id: 'research-citation',
      parseArtifactId: 'artifact-a',
      blockId: 'block-a',
      startOffset: 0,
      endOffset: 6,
      canonicalText: '引用级原文',
      canonicalTextSha256: 'd'.repeat(64),
      createdAt: run.createdAt,
    };
    vi.mocked(knowledgeApi.getKnowledgeResearchReport).mockResolvedValue({
      report: {
        runId: run.id,
        title: '完整研究报告',
        summary: '所有冻结来源均已扫描。',
        conclusions: [{ text: '结论有证据。', claimIds: ['claim-a'], citationMarkers: [1] }],
        majorFindings: [],
        conflicts: [],
        uncertainties: ['仍需长期观察。'],
        limitations: ['仅限所选笔记本。'],
        coverage,
        citations: [{ marker: 1, evidenceId: 'evidence-a', citationId: citation.id }],
        createdAt: '2026-08-25T00:00:01.000Z',
      },
      citations: [{
        marker: 1,
        evidenceId: 'evidence-a',
        citation,
        source: sourceEntry.source,
        snapshot: sourceEntry.snapshot,
        parseArtifact: sourceEntry.parseArtifact,
        locator: block.locator,
        viewer: { contentUrl: '/content/snapshot-a', locator: block.locator },
      }],
    });
    vi.mocked(knowledgeApi.resolveKnowledgeCitation).mockResolvedValue({
      citation,
      block,
      source: sourceEntry.source,
      snapshot: sourceEntry.snapshot,
      parseArtifact: sourceEntry.parseArtifact,
      viewer: { contentUrl: '/content/snapshot-a', locator: block.locator },
    });

    render(<KnowledgePage />);
    await screen.findByText('roadmap.md');
    fireEvent.click(screen.getByRole('button', { name: '全文研究' }));
    const question = screen.getByPlaceholderText('输入想了解的问题…');
    expect(question).toBeEnabled();
    fireEvent.change(question, { target: { value: run.question } });
    fireEvent.click(screen.getByRole('button', { name: '发送' }));

    await waitFor(() => expect(knowledgeApi.runKnowledgeResearch).toHaveBeenCalledWith({
      question: run.question,
      notebookIds: ['notebook-a'],
    }));
    expect(await screen.findByRole('heading', { name: '完整研究报告' })).toBeInTheDocument();
    expect(screen.getByText('所有冻结来源均已扫描。')).toBeInTheDocument();
    expect(screen.getByText('来源准备')).toBeInTheDocument();
    expect(screen.getByText('矛盾检查')).toBeInTheDocument();
    expect(screen.getByText('证据验证')).toBeInTheDocument();
    expect(screen.getByText('快照 snapshot-a')).toBeInTheDocument();
    expect(screen.getByText('仍需长期观察。')).toBeInTheDocument();
    expect(screen.getByText('仅限所选笔记本。')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: '打开引用 1' }));
    await waitFor(() => expect(knowledgeApi.resolveKnowledgeCitation).toHaveBeenCalledWith(citation.id));
    expect(await screen.findByRole('heading', { name: 'roadmap.md' })).toBeInTheDocument();
  });

  it('offers Cancel while Full Research is active', async () => {
    const run = {
      id: 'research-cancel',
      studioId: 'studio-a',
      mode: 'research' as const,
      question: '取消',
      scopeSnapshotId: 'scope-cancel',
      status: 'running' as const,
      retrievalMode: 'fts' as const,
      answerText: null,
      errorCode: null,
      createdAt: '2026-08-25T00:00:00.000Z',
      completedAt: null,
      citations: [],
      retrievals: [],
    };
    const coverage = {
      sourceReadiness: { completed: 1, total: 1 },
      extraction: { completed: 1, total: 1 },
      primaryScan: { completed: 0, total: 1 },
      contradiction: { completed: 0, total: 0 },
      citationValidation: { completed: 0, total: 0, valid: 0, invalid: 0 },
    };
    const scope = {
      id: run.scopeSnapshotId,
      studioId: 'studio-a',
      mode: 'research' as const,
      createdAt: run.createdAt,
      notebooks: [],
      sources: [],
    };
    const research = {
      runId: run.id,
      hostTaskId: `knowledge-research:${run.id}`,
      state: 'scanning' as const,
      spec: {
        originalQuestion: run.question,
        scopeSnapshotId: scope.id,
        notebookIds: ['notebook-a'],
        goal: run.question,
        dimensions: [],
        outputRequirements: [],
        definitions: [],
        assumptions: [],
      },
      manifest: null,
      coverage,
      reportAvailable: false,
      errorCode: null,
      createdAt: run.createdAt,
      updatedAt: run.createdAt,
      completedAt: null,
    };
    vi.mocked(knowledgeApi.runKnowledgeResearch).mockResolvedValue({ run, scope, research, citations: [] });
    vi.mocked(knowledgeApi.getKnowledgeResearchRun).mockImplementation(() => new Promise(() => {}));
    vi.mocked(knowledgeApi.cancelKnowledgeResearch).mockResolvedValue({
      run: { ...run, status: 'cancelled', completedAt: '2026-08-25T00:00:01.000Z' },
      research: { ...research, state: 'canceled', completedAt: '2026-08-25T00:00:01.000Z' },
    });

    render(<KnowledgePage />);
    await screen.findByText('roadmap.md');
    fireEvent.click(screen.getByRole('button', { name: '全文研究' }));
    fireEvent.change(screen.getByPlaceholderText('输入想了解的问题…'), { target: { value: '取消' } });
    fireEvent.click(screen.getByRole('button', { name: '发送' }));
    fireEvent.click(await screen.findByRole('button', { name: '取消研究' }));

    await waitFor(() => expect(knowledgeApi.cancelKnowledgeResearch).toHaveBeenCalledWith(run.id));
    expect(screen.queryByRole('button', { name: '取消研究' })).not.toBeInTheDocument();
  });

  it('reconnects the page to a recovering research run after reload', async () => {
    const run = {
      id: 'research-recovering',
      studioId: 'studio-a',
      mode: 'research' as const,
      question: '继续上次研究',
      scopeSnapshotId: 'scope-recovering',
      status: 'running' as const,
      retrievalMode: 'fts' as const,
      answerText: null,
      errorCode: null,
      createdAt: '2026-08-25T00:00:00.000Z',
      completedAt: null,
      citations: [],
      retrievals: [],
    };
    const scope = {
      id: 'scope-recovering',
      studioId: 'studio-a',
      mode: 'research' as const,
      createdAt: run.createdAt,
      notebooks: [{
        scopeSnapshotId: 'scope-recovering',
        notebookId: 'notebook-a',
        notebookName: '产品资料',
        ordinal: 0,
      }],
      sources: [],
    };
    const coverage = {
      sourceReadiness: { completed: 1, total: 1 },
      extraction: { completed: 1, total: 1 },
      primaryScan: { completed: 2, total: 4 },
      contradiction: { completed: 0, total: 0 },
      citationValidation: { completed: 1, total: 1, valid: 1, invalid: 0 },
    };
    const recovered = {
      run,
      scope,
      research: {
        runId: run.id,
        hostTaskId: `knowledge-research:${run.id}`,
        state: 'recovering' as const,
        spec: {
          originalQuestion: run.question,
          scopeSnapshotId: scope.id,
          notebookIds: ['notebook-a'],
          goal: run.question,
          dimensions: [],
          outputRequirements: [],
          definitions: [],
          assumptions: [],
        },
        manifest: null,
        coverage,
        reportAvailable: false,
        errorCode: null,
        createdAt: run.createdAt,
        updatedAt: run.createdAt,
        completedAt: null,
      },
      citations: [] as [],
    };
    vi.mocked(knowledgeApi.listActiveKnowledgeResearchRuns).mockResolvedValue([recovered]);
    vi.mocked(knowledgeApi.getKnowledgeResearchRun).mockImplementation(() => new Promise(() => {}));

    render(<KnowledgePage />);

    expect(await screen.findByDisplayValue('继续上次研究')).toBeDisabled();
    expect(await screen.findByText('正在恢复')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '取消研究' })).toBeInTheDocument();
    expect(knowledgeApi.runKnowledgeResearch).not.toHaveBeenCalled();
  });
});
