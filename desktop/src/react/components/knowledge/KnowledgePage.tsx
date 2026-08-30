import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from '../../stores';
import {
  createKnowledgeNotebook,
  deleteKnowledgeNotebook,
  importKnowledgeFileSource,
  importKnowledgePastedText,
  importKnowledgeWebSnapshot,
  knowledgeSnapshotContentUrl,
  listKnowledgeBlocks,
  listKnowledgeChunks,
  listKnowledgeIngestion,
  listKnowledgeNotebooks,
  listKnowledgeSources,
  reingestKnowledgeSource,
  removeKnowledgeSource,
  refreshKnowledgeSource,
  renameKnowledgeNotebook,
  type KnowledgeChunkDto,
  type KnowledgeBlockDto,
  type KnowledgeIngestionJobDto,
  type KnowledgeNotebookDto,
  type KnowledgeSourceEntryDto,
} from './knowledge-api';
import { NotebookSettingsDialog } from './NotebookSettingsDialog';
import { Overlay } from '../../ui';
import styles from './KnowledgePage.module.css';

const tr = (key: string, vars?: Record<string, string | number>) => window.t?.(key, vars) ?? key;

/** 源行唯一徽章的展示信息：label + 配色（复用解析状态配色语义）。 */
interface SourceBadgeInfo {
  label: string;
  className: string;
  title?: string;
}

/**
 * 摄入徽章文案：由最新 ingestion job 驱动 —— done=已就绪；running 按 phase 细分
 * （embed 带进度 done/total）；queued/pending_embedding/failed 是显式状态；
 * 无 job 且该笔记本摄入数据已加载 = untracked 源 → 「待摄入」（muted）。
 */
function sourceBadgeInfo(job: KnowledgeIngestionJobDto | null, ingestionLoaded: boolean): SourceBadgeInfo | null {
  if (!job) {
    return ingestionLoaded ? { label: tr('knowledge.statusPendingIngestion'), className: '' } : null;
  }
  if (job.status === 'done') return { label: tr('knowledge.statusReady'), className: 'ready' };
  if (job.status === 'pending_embedding') {
    return { label: tr('knowledge.ingestionPendingEmbedding'), className: '', title: tr('knowledge.pendingEmbeddingHint') };
  }
  if (job.status === 'failed') {
    return { label: tr('knowledge.ingestionFailed'), className: 'failed', title: job.error || undefined };
  }
  if (job.status === 'queued') return { label: tr('knowledge.ingestionQueued'), className: 'parsing' };
  if (job.status === 'running') {
    if (job.phase === 'embed') {
      const label = typeof job.progressTotal === 'number' && job.progressTotal > 0
        ? tr('knowledge.ingestionEmbedProgress', { done: job.progressDone ?? 0, total: job.progressTotal })
        : tr('knowledge.ingestionEmbed');
      return { label, className: 'parsing' };
    }
    switch (job.phase) {
      case 'parse': return { label: tr('knowledge.ingestionProcessingParse'), className: 'parsing' };
      case 'chunk': return { label: tr('knowledge.ingestionProcessingChunk'), className: 'parsing' };
      case 'fts_index': return { label: tr('knowledge.ingestionProcessingIndex'), className: 'parsing' };
      default: return { label: tr('knowledge.ingestionQueued'), className: 'parsing' };
    }
  }
  return null;
}

/** embed 阶段细进度条的比例（0–1）；非 embed 运行态或无 total 时为 null。 */
function embedProgressRatio(job: KnowledgeIngestionJobDto | null): number | null {
  if (!job || job.status !== 'running' || job.phase !== 'embed') return null;
  if (typeof job.progressTotal !== 'number' || job.progressTotal <= 0) return null;
  return Math.min(1, Math.max(0, (job.progressDone ?? 0) / job.progressTotal));
}

/** 笔记本就绪汇总后缀：只列非零的异常/进行中项（徽章级摘要）。 */
function readinessSuffix(notebook: KnowledgeNotebookDto): string {
  const parts: string[] = [];
  if (notebook.ingestion.processing > 0) {
    parts.push(tr('knowledge.readinessProcessing', { count: notebook.ingestion.processing }));
  }
  if (notebook.ingestion.pendingEmbedding > 0) {
    parts.push(tr('knowledge.readinessPendingEmbedding', { count: notebook.ingestion.pendingEmbedding }));
  }
  if (notebook.ingestion.failed > 0) {
    parts.push(tr('knowledge.readinessFailed', { count: notebook.ingestion.failed }));
  }
  return parts.length > 0 ? ` · ${parts.join(' · ')}` : '';
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function locatorLabel(block: KnowledgeBlockDto): string {
  const locator = block.locator;
  if (block.locatorType === 'pdf' && typeof locator.page === 'number') {
    return tr('knowledge.pageNumber', { number: locator.page });
  }
  if ((block.locatorType === 'text' || block.locatorType === 'markdown') && typeof locator.lineStart === 'number') {
    const end = typeof locator.lineEnd === 'number' ? locator.lineEnd : locator.lineStart;
    return end === locator.lineStart
      ? tr('knowledge.lineNumber', { number: locator.lineStart })
      : tr('knowledge.lineRange', { start: locator.lineStart, end });
  }
  if (block.locatorType === 'html' && typeof locator.selector === 'string') {
    return String(locator.selector);
  }
  return tr('knowledge.paragraphNumber', { number: block.ordinal + 1 });
}

/** 分块卡片定位：heading 面包屑优先，其次页码；都没有时返回 null。 */
function chunkLocatorLabel(chunk: KnowledgeChunkDto): string | null {
  if (chunk.headingPath && chunk.headingPath.length > 0) return chunk.headingPath.join(' / ');
  if (typeof chunk.pageNumber === 'number') return tr('knowledge.pageNumber', { number: chunk.pageNumber });
  return null;
}

interface SourceViewerProps {
  entry: Pick<KnowledgeSourceEntryDto, 'source' | 'snapshot' | 'parseArtifact'>;
  blocks: KnowledgeBlockDto[];
  loading: boolean;
  error: string | null;
  onClose: () => void;
}

function SourceViewer({ entry, blocks, loading, error, onClose }: SourceViewerProps) {
  const [selectedPdfPage, setSelectedPdfPage] = useState(1);
  const isPdf = entry.snapshot.mimeType === 'application/pdf';
  const pdfUrl = useMemo(() => {
    if (!isPdf) return null;
    return `${knowledgeSnapshotContentUrl(entry.snapshot.id)}#page=${selectedPdfPage}`;
  }, [entry.snapshot.id, isPdf, selectedPdfPage]);

  useEffect(() => {
    const firstPage = blocks.find(block => block.locatorType === 'pdf')?.locator.page;
    setSelectedPdfPage(typeof firstPage === 'number' ? firstPage : 1);
  }, [blocks, entry.source.id]);

  return (
    <section className={styles.viewer} aria-label={tr('knowledge.sourceViewer')}>
      <header className={styles.viewerHeader}>
        <div className={styles.viewerTitleWrap}>
          <button className={styles.backButton} onClick={onClose} aria-label={tr('knowledge.closeViewer')}>←</button>
          <div>
            <h2>{entry.source.displayName}</h2>
            <p>{formatBytes(entry.snapshot.byteSize)} · {entry.snapshot.mimeType}</p>
            {entry.source.originMetadata.url && (
              <p className={styles.sourceOrigin} title={entry.source.originMetadata.url}>
                {entry.source.originMetadata.url}
              </p>
            )}
            {entry.source.originMetadata.fetchedAt && (
              <p>{tr('knowledge.fetchedAt', { time: entry.source.originMetadata.fetchedAt })}</p>
            )}
          </div>
        </div>
      </header>

      {entry.parseArtifact?.status === 'needs_ocr' && (
        <div className={styles.notice}>{tr('knowledge.needsOcrHint')}</div>
      )}
      {entry.parseArtifact?.status === 'failed' && (
        <div className={styles.errorNotice}>{tr('knowledge.parseFailedHint')}</div>
      )}
      {error && <div className={styles.errorNotice}>{error}</div>}

      <div className={`${styles.viewerBody} ${isPdf ? styles.viewerBodyPdf : ''}`}>
        {isPdf && pdfUrl && (
          <iframe
            key={selectedPdfPage}
            className={styles.pdfFrame}
            src={pdfUrl}
            title={entry.source.displayName}
          />
        )}
        <div className={styles.blockList} aria-busy={loading}>
          {loading && <div className={styles.centerMessage}>{tr('knowledge.loading')}</div>}
          {!loading && !error && blocks.length === 0 && entry.parseArtifact?.status === 'ready' && (
            <div className={styles.centerMessage}>{tr('knowledge.emptyContent')}</div>
          )}
          {blocks.map(block => (
            <button
              key={block.id}
              className={styles.blockCard}
              onClick={() => {
                const page = block.locatorType === 'pdf' ? block.locator.page : null;
                if (typeof page === 'number') setSelectedPdfPage(page);
              }}
            >
              <span className={styles.blockLocator}>{locatorLabel(block)}</span>
              <span className={styles.blockText}>{block.text}</span>
            </button>
          ))}
        </div>
      </div>
    </section>
  );
}

interface ChunkViewerProps {
  entry: Pick<KnowledgeSourceEntryDto, 'source'>;
  chunks: KnowledgeChunkDto[];
  loading: boolean;
  error: string | null;
  onClose: () => void;
  onOpenChunk: (chunk: KnowledgeChunkDto) => void;
}

/** 分块内容视图：卡片网格（#ordinal + 定位 + 预览），点击卡片弹出全文详情层。 */
function ChunkViewer({ entry, chunks, loading, error, onClose, onOpenChunk }: ChunkViewerProps) {
  return (
    <section className={styles.viewer} aria-label={tr('knowledge.chunkViewer')}>
      <header className={styles.viewerHeader}>
        <div className={styles.viewerTitleWrap}>
          <button className={styles.backButton} onClick={onClose} aria-label={tr('knowledge.closeViewer')}>←</button>
          <div>
            <h2>{entry.source.displayName}</h2>
            <p>{tr('knowledge.chunkCount', { count: chunks.length })}</p>
          </div>
        </div>
      </header>

      {error && <div className={styles.errorNotice}>{error}</div>}

      <div className={styles.chunkGrid} aria-busy={loading}>
        {loading && <div className={styles.centerMessage}>{tr('knowledge.loading')}</div>}
        {!loading && !error && chunks.length === 0 && (
          <div className={styles.centerMessage}>{tr('knowledge.noChunks')}</div>
        )}
        {chunks.map(chunk => (
          <button key={chunk.id} className={styles.chunkCard} onClick={() => onOpenChunk(chunk)}>
            <span className={styles.chunkCardMeta}>
              <span className={styles.chunkOrdinal}>#{chunk.ordinal}</span>
              {chunkLocatorLabel(chunk) && (
                <span className={styles.chunkLocator} title={chunkLocatorLabel(chunk) ?? undefined}>
                  {chunkLocatorLabel(chunk)}
                </span>
              )}
            </span>
            <span className={styles.chunkPreview}>
              {chunk.text.length > 200 ? `${chunk.text.slice(0, 200)}…` : chunk.text}
            </span>
          </button>
        ))}
      </div>
    </section>
  );
}

/** 分块详情弹层：全文 + 定位信息 + token/字符数（仿 NotebookSettingsDialog 的 Overlay 模式）。 */
function ChunkDetailDialog({ chunk, onClose }: { chunk: KnowledgeChunkDto; onClose: () => void }) {
  const closeRef = useRef<HTMLButtonElement>(null);
  const locator = chunkLocatorLabel(chunk);
  return (
    <Overlay
      open
      scope="window"
      onClose={onClose}
      className={styles.chunkDialog}
      initialFocusRef={closeRef}
      contentProps={{ role: 'dialog', 'aria-modal': true }}
    >
      <header className={styles.chunkDialogHeader}>
        <h2 className={styles.chunkDialogTitle}>{tr('knowledge.chunkDetailTitle', { ordinal: chunk.ordinal })}</h2>
        <button ref={closeRef} className={styles.iconButton} onClick={onClose} aria-label={tr('common.close')}>×</button>
      </header>
      <div className={styles.chunkDialogMeta}>
        {locator && <span>{locator}</span>}
        <span>{tr('knowledge.chunkTokenCount', { count: chunk.tokenCount })}</span>
        <span>{tr('knowledge.chunkCharCount', { count: chunk.charCount })}</span>
      </div>
      <div className={styles.chunkDialogBody}>
        <pre className={styles.chunkDialogText}>{chunk.text}</pre>
      </div>
    </Overlay>
  );
}

export function KnowledgePage() {
  const addToast = useStore(state => state.addToast);
  const [notebooks, setNotebooks] = useState<KnowledgeNotebookDto[]>([]);
  const [selectedNotebookId, setSelectedNotebookId] = useState<string | null>(null);
  const [sources, setSources] = useState<KnowledgeSourceEntryDto[]>([]);
  const [selectedSourceId, setSelectedSourceId] = useState<string | null>(null);
  const [blocks, setBlocks] = useState<KnowledgeBlockDto[]>([]);
  const [loadingNotebooks, setLoadingNotebooks] = useState(true);
  const [loadingSources, setLoadingSources] = useState(false);
  const [loadingBlocks, setLoadingBlocks] = useState(false);
  const [pageError, setPageError] = useState<string | null>(null);
  const [viewerError, setViewerError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState('');
  const [renameId, setRenameId] = useState<string | null>(null);
  const [renameName, setRenameName] = useState('');
  const [importing, setImporting] = useState(false);
  const [showImportMenu, setShowImportMenu] = useState(false);
  const [sourceDraftKind, setSourceDraftKind] = useState<'pasted_text' | 'web_snapshot' | null>(null);
  const [sourceDraftName, setSourceDraftName] = useState('');
  const [sourceDraftValue, setSourceDraftValue] = useState('');
  const [refreshingSourceId, setRefreshingSourceId] = useState<string | null>(null);
  const [ingestionJobs, setIngestionJobs] = useState<KnowledgeIngestionJobDto[]>([]);
  const [ingestionRefreshKey, setIngestionRefreshKey] = useState(0);
  /** 摄入 job 列表已成功拉取的笔记本：只有该笔记本的「无 job」才判为 untracked（待摄入）。 */
  const [ingestionLoadedNotebookId, setIngestionLoadedNotebookId] = useState<string | null>(null);
  const [settingsNotebookId, setSettingsNotebookId] = useState<string | null>(null);
  const [sourceMenuFor, setSourceMenuFor] = useState<string | null>(null);
  const [sourceViewMode, setSourceViewMode] = useState<'preview' | 'chunks'>('preview');
  const [chunks, setChunks] = useState<KnowledgeChunkDto[]>([]);
  const [loadingChunks, setLoadingChunks] = useState(false);
  const [chunksError, setChunksError] = useState<string | null>(null);
  const [chunkDetail, setChunkDetail] = useState<KnowledgeChunkDto | null>(null);
  const sourceMenuRowRef = useRef<HTMLDivElement | null>(null);

  const selectedNotebook = notebooks.find(item => item.id === selectedNotebookId) ?? null;
  const selectedSource = sources.find(item => item.source.id === selectedSourceId) ?? null;
  const settingsNotebook = notebooks.find(item => item.id === settingsNotebookId) ?? null;

  // API 按 created_at 倒序返回：每个源的第一条即最新 job。
  const latestJobBySource = useMemo(() => {
    const map = new Map<string, KnowledgeIngestionJobDto>();
    for (const job of ingestionJobs) {
      if (!map.has(job.sourceId)) map.set(job.sourceId, job);
    }
    return map;
  }, [ingestionJobs]);

  const refreshNotebooks = useCallback(async (preferredId?: string | null) => {
    const loaded = await listKnowledgeNotebooks();
    setNotebooks(loaded);
    setSelectedNotebookId(current => {
      const candidate = preferredId ?? current;
      if (candidate && loaded.some(item => item.id === candidate)) return candidate;
      return loaded[0]?.id ?? null;
    });
    return loaded;
  }, []);

  useEffect(() => {
    let active = true;
    setLoadingNotebooks(true);
    listKnowledgeNotebooks()
      .then(loaded => {
        if (!active) return;
        setNotebooks(loaded);
        setSelectedNotebookId(loaded[0]?.id ?? null);
      })
      .catch(error => {
        if (active) setPageError(error instanceof Error ? error.message : tr('knowledge.loadFailed'));
      })
      .finally(() => {
        if (active) setLoadingNotebooks(false);
      });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    if (!selectedNotebookId) {
      setSources([]);
      setSelectedSourceId(null);
      return;
    }
    let active = true;
    setLoadingSources(true);
    setSelectedSourceId(null);
    setBlocks([]);
    listKnowledgeSources(selectedNotebookId)
      .then(loaded => {
        if (active) setSources(loaded);
      })
      .catch(error => {
        if (active) setPageError(error instanceof Error ? error.message : tr('knowledge.loadFailed'));
      })
      .finally(() => {
        if (active) setLoadingSources(false);
      });
    return () => { active = false; };
  }, [selectedNotebookId]);

  useEffect(() => {
    const artifact = selectedSource?.parseArtifact;
    if (!artifact || artifact.status !== 'ready' || sourceViewMode !== 'preview') {
      setBlocks([]);
      return;
    }
    let active = true;
    setLoadingBlocks(true);
    setViewerError(null);
    listKnowledgeBlocks(artifact.id)
      .then(loaded => {
        if (active) setBlocks(loaded);
      })
      .catch(error => {
        if (active) setViewerError(error instanceof Error ? error.message : tr('knowledge.loadFailed'));
      })
      .finally(() => {
        if (active) setLoadingBlocks(false);
      });
    return () => { active = false; };
  }, [selectedSource?.parseArtifact, selectedSource?.source.id, sourceViewMode]);

  // 分块内容视图：仅 chunks 模式且 parse ready 时拉取（未 ready 由菜单入口禁用兜底）。
  useEffect(() => {
    const artifact = selectedSource?.parseArtifact;
    if (sourceViewMode !== 'chunks' || !artifact || artifact.status !== 'ready') {
      setChunks([]);
      return;
    }
    let active = true;
    setLoadingChunks(true);
    setChunksError(null);
    listKnowledgeChunks(artifact.id)
      .then(loaded => {
        if (active) setChunks(loaded.chunks);
      })
      .catch(error => {
        if (active) setChunksError(error instanceof Error ? error.message : tr('knowledge.loadFailed'));
      })
      .finally(() => {
        if (active) setLoadingChunks(false);
      });
    return () => { active = false; };
  }, [selectedSource?.parseArtifact, selectedSource?.source.id, sourceViewMode]);

  // 源行操作菜单：外点关闭 + Escape（仿 KnowledgeReferenceButton 的 rootRef 模式）。
  useEffect(() => {
    if (!sourceMenuFor) return;
    const onMouseDown = (e: MouseEvent) => {
      if (sourceMenuRowRef.current && !sourceMenuRowRef.current.contains(e.target as Node)) setSourceMenuFor(null);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setSourceMenuFor(null);
    };
    document.addEventListener('mousedown', onMouseDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('mousedown', onMouseDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [sourceMenuFor]);

  const openSourceView = useCallback((entry: KnowledgeSourceEntryDto, mode: 'preview' | 'chunks') => {
    setSourceMenuFor(null);
    setSourceViewMode(mode);
    setSelectedSourceId(entry.source.id);
  }, []);

  const handleCreate = async () => {
    const name = newName.trim();
    if (!name) return;
    try {
      const notebook = await createKnowledgeNotebook(name);
      setNewName('');
      setCreating(false);
      await refreshNotebooks(notebook.id);
    } catch (error) {
      addToast(error instanceof Error ? error.message : tr('knowledge.saveFailed'), 'error');
    }
  };

  const handleRename = async (id: string) => {
    const name = renameName.trim();
    if (!name) return;
    try {
      await renameKnowledgeNotebook(id, name);
      setRenameId(null);
      setRenameName('');
      await refreshNotebooks(id);
    } catch (error) {
      addToast(error instanceof Error ? error.message : tr('knowledge.saveFailed'), 'error');
    }
  };

  const handleDelete = async (notebook: KnowledgeNotebookDto) => {
    if (!window.confirm(tr('knowledge.deleteNotebookConfirm', { name: notebook.name }))) return;
    try {
      await deleteKnowledgeNotebook(notebook.id);
      await refreshNotebooks(null);
    } catch (error) {
      addToast(error instanceof Error ? error.message : tr('knowledge.saveFailed'), 'error');
    }
  };

  const refreshSources = useCallback(async () => {
    if (!selectedNotebookId) return;
    const loaded = await listKnowledgeSources(selectedNotebookId);
    setSources(loaded);
  }, [selectedNotebookId]);

  // 摄入状态：进入笔记本/导入/刷新/重试/设置保存后拉一次；有活跃 job（queued/running）
  // 时才持续轮询，全部落终态后刷新来源与就绪汇总并停止轮询。
  useEffect(() => {
    if (!selectedNotebookId) {
      setIngestionJobs([]);
      return;
    }
    let active = true;
    let timer: ReturnType<typeof setInterval> | null = null;
    let hadActive = false;
    const tick = async () => {
      try {
        const state = await listKnowledgeIngestion({ notebookId: selectedNotebookId });
        if (!active) return;
        setIngestionJobs(state.jobs);
        setIngestionLoadedNotebookId(selectedNotebookId);
        const hasActive = state.jobs.some(job => job.status === 'queued' || job.status === 'running');
        if (hasActive && !timer) {
          timer = setInterval(() => { void tick(); }, 2000);
        } else if (!hasActive && timer) {
          clearInterval(timer);
          timer = null;
        }
        // 活跃 job 期间同步刷就绪汇总，页头/引用菜单的 ingestion 摘要不再陈旧。
        if (hasActive) {
          void refreshNotebooks(selectedNotebookId);
        }
        if (hadActive && !hasActive) {
          void refreshSources();
          void refreshNotebooks(selectedNotebookId);
        }
        hadActive = hasActive;
      } catch {
        // 轮询失败不打断页面；若仍有活跃 job，下个周期自动重试。
      }
    };
    void tick();
    return () => {
      active = false;
      if (timer) clearInterval(timer);
    };
  }, [selectedNotebookId, ingestionRefreshKey, refreshSources, refreshNotebooks]);

  const handleImportFiles = async () => {
    if (!selectedNotebookId || !window.platform?.selectFiles) {
      addToast(tr('knowledge.importUnavailable'), 'error');
      return;
    }
    setShowImportMenu(false);
    const filePaths = await window.platform.selectFiles({ multiple: true });
    if (filePaths.length === 0) return;
    setImporting(true);
    let successCount = 0;
    try {
      for (const filePath of filePaths) {
        try {
          await importKnowledgeFileSource(selectedNotebookId, filePath);
          successCount += 1;
        } catch {
          // 单个来源失败不阻断其余来源，最终只汇报数量，避免泄露本机路径。
        }
      }
      // 先触发摄入轮询再刷源列表：导入返回时 job 已入队，避免新源短暂显示「待摄入」。
      setIngestionRefreshKey(key => key + 1);
      await refreshSources();
      if (successCount === filePaths.length) {
        addToast(tr('knowledge.importSuccess', { count: successCount }), 'success');
      } else {
        addToast(tr('knowledge.importPartialFailure', { success: successCount, total: filePaths.length }), 'error');
      }
    } finally {
      setImporting(false);
    }
  };

  const resetSourceDraft = () => {
    setSourceDraftKind(null);
    setSourceDraftName('');
    setSourceDraftValue('');
  };

  const openSourceDraft = (kind: 'pasted_text' | 'web_snapshot') => {
    setShowImportMenu(false);
    setSourceDraftKind(kind);
    setSourceDraftName('');
    setSourceDraftValue('');
  };

  const handleImportManagedSource = async () => {
    if (!selectedNotebookId || !sourceDraftKind || !sourceDraftValue.trim() || importing) return;
    setImporting(true);
    try {
      const displayName = sourceDraftName.trim() || undefined;
      if (sourceDraftKind === 'pasted_text') {
        await importKnowledgePastedText(selectedNotebookId, {
          text: sourceDraftValue,
          displayName,
        });
      } else {
        await importKnowledgeWebSnapshot(selectedNotebookId, {
          url: sourceDraftValue.trim(),
          displayName,
        });
      }
      resetSourceDraft();
      setIngestionRefreshKey(key => key + 1);
      await refreshSources();
      addToast(tr('knowledge.importSuccess', { count: 1 }), 'success');
    } catch (error) {
      addToast(error instanceof Error ? error.message : tr('knowledge.saveFailed'), 'error');
    } finally {
      setImporting(false);
    }
  };

  const handleRemoveSource = async (entry: KnowledgeSourceEntryDto) => {
    if (!selectedNotebookId) return;
    if (!window.confirm(tr('knowledge.removeSourceConfirm', { name: entry.source.displayName }))) return;
    try {
      await removeKnowledgeSource(selectedNotebookId, entry.source.id);
      if (selectedSourceId === entry.source.id) setSelectedSourceId(null);
      await refreshSources();
    } catch (error) {
      addToast(error instanceof Error ? error.message : tr('knowledge.saveFailed'), 'error');
    }
  };

  const handleRefreshSource = async (entry: KnowledgeSourceEntryDto) => {
    if (!selectedNotebookId || refreshingSourceId) return;
    setRefreshingSourceId(entry.source.id);
    try {
      const refreshed = await refreshKnowledgeSource(selectedNotebookId, entry.source.id);
      setIngestionRefreshKey(key => key + 1);
      await refreshSources();
      addToast(
        refreshed.changed ? tr('knowledge.refreshSourceChanged') : tr('knowledge.refreshSourceUnchanged'),
        'success',
      );
    } catch (error) {
      addToast(error instanceof Error ? error.message : tr('knowledge.saveFailed'), 'error');
    } finally {
      setRefreshingSourceId(null);
    }
  };

  const handleReingest = async (entry: KnowledgeSourceEntryDto) => {
    if (!selectedNotebookId) return;
    try {
      await reingestKnowledgeSource(selectedNotebookId, entry.source.id);
      setIngestionRefreshKey(key => key + 1);
    } catch (error) {
      addToast(error instanceof Error ? error.message : tr('knowledge.saveFailed'), 'error');
    }
  };

  const handleSettingsSaved = async () => {
    const notebookId = settingsNotebookId;
    setSettingsNotebookId(null);
    await refreshNotebooks(notebookId);
    // 分块尺寸/嵌入模型变更已触发服务端全量重建：立即拉取新的摄入状态开始轮询。
    setIngestionRefreshKey(key => key + 1);
    addToast(tr('knowledge.settingsSaved'), 'success');
  };

  return (
    <div className={styles.page} data-testid="knowledge-page">
      <aside className={styles.notebookPane}>
        <header className={styles.paneHeader}>
          <div>
            <span className={styles.eyebrow}>{tr('knowledge.tab')}</span>
            <h1>{tr('knowledge.title')}</h1>
          </div>
          <button className={styles.iconButton} onClick={() => setCreating(true)} aria-label={tr('knowledge.newNotebook')}>＋</button>
        </header>

        {creating && (
          <div className={styles.inlineEditor}>
            <input
              autoFocus
              value={newName}
              placeholder={tr('knowledge.notebookName')}
              onChange={event => setNewName(event.target.value)}
              onKeyDown={event => {
                if (event.key === 'Enter') void handleCreate();
                if (event.key === 'Escape') setCreating(false);
              }}
            />
            <button onClick={() => void handleCreate()} disabled={!newName.trim()}>{tr('knowledge.create')}</button>
            <button className={styles.quietButton} onClick={() => setCreating(false)}>{tr('knowledge.cancel')}</button>
          </div>
        )}

        <div className={styles.notebookList} aria-busy={loadingNotebooks}>
          {loadingNotebooks && <div className={styles.centerMessage}>{tr('knowledge.loading')}</div>}
          {!loadingNotebooks && notebooks.length === 0 && (
            <div className={styles.emptyState}>{tr('knowledge.emptyNotebooks')}</div>
          )}
          {notebooks.map(notebook => (
            <div
              key={notebook.id}
              className={`${styles.notebookRow} ${selectedNotebookId === notebook.id ? styles.selected : ''}`}
            >
              {renameId === notebook.id ? (
                <input
                  className={styles.renameInput}
                  autoFocus
                  value={renameName}
                  onChange={event => setRenameName(event.target.value)}
                  onBlur={() => void handleRename(notebook.id)}
                  onKeyDown={event => {
                    if (event.key === 'Enter') void handleRename(notebook.id);
                    if (event.key === 'Escape') setRenameId(null);
                  }}
                />
              ) : (
                <button className={styles.notebookSelect} onClick={() => setSelectedNotebookId(notebook.id)}>
                  <span className={styles.notebookGlyph}>◇</span>
                  <span>{notebook.name}</span>
                </button>
              )}
              <div className={styles.rowActions}>
                <button
                  onClick={() => {
                    setRenameId(notebook.id);
                    setRenameName(notebook.name);
                  }}
                  aria-label={tr('knowledge.renameNotebook', { name: notebook.name })}
                >✎</button>
                <button onClick={() => void handleDelete(notebook)} aria-label={tr('knowledge.deleteNotebook', { name: notebook.name })}>×</button>
              </div>
            </div>
          ))}
        </div>

        {!creating && (
          <button className={styles.newNotebookButton} onClick={() => setCreating(true)}>
            ＋ {tr('knowledge.newNotebook')}
          </button>
        )}
      </aside>

      <section className={styles.sourcePane}>
        <header className={styles.paneHeader}>
          <div>
            <span className={styles.eyebrow}>{tr('knowledge.sources')}</span>
            <h2>{selectedNotebook?.name || tr('knowledge.noNotebookSelected')}</h2>
            {selectedNotebook && (
              <p>
                {tr('knowledge.sourceCount', { count: sources.length })}
                {readinessSuffix(selectedNotebook)}
              </p>
            )}
          </div>
          <div className={styles.headerActions}>
            {selectedNotebook && (
              <button
                className={styles.iconButton}
                onClick={() => setSettingsNotebookId(selectedNotebook.id)}
                aria-label={tr('knowledge.notebookSettings')}
                title={tr('knowledge.notebookSettings')}
              >⚙</button>
            )}
            <button
              className={styles.importButton}
              onClick={() => setShowImportMenu(value => !value)}
              disabled={!selectedNotebookId || importing}
              aria-expanded={showImportMenu}
            >
              {importing ? tr('knowledge.importing') : `＋ ${tr('knowledge.addSource')}`}
            </button>
          </div>
        </header>

        {showImportMenu && selectedNotebookId && (
          <div className={styles.sourceImportMenu} role="menu" aria-label={tr('knowledge.addSource')}>
            <button role="menuitem" onClick={() => void handleImportFiles()}>{tr('knowledge.addLocalFile')}</button>
            <button role="menuitem" onClick={() => openSourceDraft('pasted_text')}>{tr('knowledge.addPastedText')}</button>
            <button role="menuitem" onClick={() => openSourceDraft('web_snapshot')}>{tr('knowledge.addWebSnapshot')}</button>
          </div>
        )}

        {sourceDraftKind && (
          <div className={styles.sourceImportEditor}>
            <strong>{tr(sourceDraftKind === 'pasted_text'
              ? 'knowledge.addPastedText'
              : 'knowledge.addWebSnapshot')}</strong>
            <input
              value={sourceDraftName}
              maxLength={255}
              placeholder={tr('knowledge.sourceDisplayNameOptional')}
              onChange={event => setSourceDraftName(event.target.value)}
            />
            {sourceDraftKind === 'pasted_text' ? (
              <textarea
                autoFocus
                value={sourceDraftValue}
                placeholder={tr('knowledge.pastedTextPlaceholder')}
                onChange={event => setSourceDraftValue(event.target.value)}
              />
            ) : (
              <input
                autoFocus
                type="url"
                value={sourceDraftValue}
                placeholder={tr('knowledge.webUrlPlaceholder')}
                onChange={event => setSourceDraftValue(event.target.value)}
                onKeyDown={event => {
                  if (event.key === 'Enter') void handleImportManagedSource();
                }}
              />
            )}
            <div className={styles.sourceImportActions}>
              <button
                className={styles.importButton}
                disabled={!sourceDraftValue.trim() || importing}
                onClick={() => void handleImportManagedSource()}
              >{tr('knowledge.importSource')}</button>
              <button className={styles.quietButton} onClick={resetSourceDraft}>{tr('knowledge.cancel')}</button>
            </div>
          </div>
        )}

        <div className={styles.sourceList} aria-busy={loadingSources}>
          {loadingSources && <div className={styles.centerMessage}>{tr('knowledge.loading')}</div>}
          {!loadingSources && selectedNotebookId && sources.length === 0 && (
            <div className={styles.emptyState}>{tr('knowledge.noSources')}</div>
          )}
          {!selectedNotebookId && <div className={styles.emptyState}>{tr('knowledge.selectNotebook')}</div>}
          {sources.map(entry => {
            const latestJob = latestJobBySource.get(entry.source.id) ?? null;
            const badge = sourceBadgeInfo(latestJob, ingestionLoadedNotebookId === selectedNotebookId);
            const embedRatio = embedProgressRatio(latestJob);
            const menuOpen = sourceMenuFor === entry.source.id;
            const parseReady = entry.parseArtifact?.status === 'ready';
            return (
            <div
              key={entry.source.id}
              ref={menuOpen ? sourceMenuRowRef : undefined}
              className={`${styles.sourceRow} ${selectedSourceId === entry.source.id ? styles.selected : ''}`}
            >
              <button
                className={`${styles.sourceSelect} ${latestJob?.status === 'failed' ? styles.hasRetry : ''}`}
                aria-haspopup="menu"
                aria-expanded={menuOpen}
                onClick={() => setSourceMenuFor(menuOpen ? null : entry.source.id)}
              >
                <span className={styles.fileGlyph}>
                  {entry.source.sourceType === 'web_snapshot' ? '◎' : entry.source.sourceType === 'pasted_text' ? '≡' : '▤'}
                </span>
                <span className={styles.sourceInfo}>
                  <strong>{entry.source.displayName}</strong>
                  <span>{formatBytes(entry.snapshot.byteSize)}</span>
                </span>
                <span className={styles.sourceBadges}>
                  {badge && (
                    <span
                      className={`${styles.status} ${badge.className ? styles[badge.className] : ''}`}
                      title={badge.title}
                    >
                      {badge.label}
                    </span>
                  )}
                </span>
              </button>
              {embedRatio !== null && (
                <div
                  className={styles.embedProgress}
                  role="progressbar"
                  aria-valuemin={0}
                  aria-valuemax={latestJob?.progressTotal ?? 0}
                  aria-valuenow={latestJob?.progressDone ?? 0}
                >
                  <div className={styles.embedProgressFill} style={{ width: `${Math.round(embedRatio * 100)}%` }} />
                </div>
              )}
              {menuOpen && (
                <div className={styles.sourceActionMenu} role="menu" aria-label={tr('knowledge.sourceActions')}>
                  <button
                    role="menuitem"
                    disabled={!parseReady}
                    title={!parseReady ? tr('knowledge.sourceMenuNotReadyHint') : undefined}
                    onClick={() => openSourceView(entry, 'preview')}
                  >{tr('knowledge.sourceMenuPreview')}</button>
                  <button
                    role="menuitem"
                    disabled={!parseReady}
                    title={!parseReady ? tr('knowledge.sourceMenuNotReadyHint') : undefined}
                    onClick={() => openSourceView(entry, 'chunks')}
                  >{tr('knowledge.sourceMenuChunks')}</button>
                </div>
              )}
              {latestJob?.status === 'failed' && (
                <button
                  className={styles.retryIngestion}
                  onClick={() => void handleReingest(entry)}
                  aria-label={tr('knowledge.retryIngestion', { name: entry.source.displayName })}
                  title={tr('knowledge.retryIngestion', { name: entry.source.displayName })}
                >⟳</button>
              )}
              {entry.source.sourceType === 'file' && (
                <button
                  className={styles.refreshSource}
                  onClick={() => void handleRefreshSource(entry)}
                  disabled={refreshingSourceId === entry.source.id}
                  aria-label={tr('knowledge.refreshSource', { name: entry.source.displayName })}
                >↻</button>
              )}
              <button
                className={styles.removeSource}
                onClick={() => void handleRemoveSource(entry)}
                aria-label={tr('knowledge.removeSource', { name: entry.source.displayName })}
              >×</button>
            </div>
            );
          })}
        </div>
      </section>

      <main className={styles.analysisPane}>
        {selectedSource ? (
          sourceViewMode === 'chunks' ? (
            <ChunkViewer
              entry={selectedSource}
              chunks={chunks}
              loading={loadingChunks}
              error={chunksError}
              onClose={() => {
                setSelectedSourceId(null);
              }}
              onOpenChunk={setChunkDetail}
            />
          ) : (
            <SourceViewer
              entry={selectedSource}
              blocks={blocks}
              loading={loadingBlocks}
              error={viewerError}
              onClose={() => {
                setSelectedSourceId(null);
              }}
            />
          )
        ) : (
          <div className={`${styles.emptyState} ${styles.analysisEmpty}`}>
            {tr('knowledge.selectSource')}
          </div>
        )}
      </main>

      {chunkDetail && (
        <ChunkDetailDialog chunk={chunkDetail} onClose={() => setChunkDetail(null)} />
      )}

      {settingsNotebook && (
        <NotebookSettingsDialog
          notebook={settingsNotebook}
          onClose={() => setSettingsNotebookId(null)}
          onSaved={() => void handleSettingsSaved()}
        />
      )}

      {pageError && (
        <div className={styles.pageError} role="alert">
          <span>{pageError}</span>
          <button onClick={() => setPageError(null)}>×</button>
        </div>
      )}
    </div>
  );
}
