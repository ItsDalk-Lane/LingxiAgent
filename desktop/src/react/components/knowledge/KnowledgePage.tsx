import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from '../../stores';
import {
  createKnowledgeNotebook,
  cancelKnowledgeResearch,
  deleteKnowledgeNotebook,
  getKnowledgeResearchReport,
  getKnowledgeResearchRun,
  importKnowledgeFileSource,
  importKnowledgePastedText,
  importKnowledgeWebSnapshot,
  knowledgeSnapshotContentUrl,
  listActiveKnowledgeResearchRuns,
  listKnowledgeBlocks,
  listKnowledgeNotebooks,
  listKnowledgeSources,
  removeKnowledgeSource,
  refreshKnowledgeSource,
  renameKnowledgeNotebook,
  resolveKnowledgeCitation,
  runKnowledgeQuickAnswer,
  runKnowledgeResearch,
  type KnowledgeBlockDto,
  type KnowledgeNotebookDto,
  type KnowledgeParseStatusDto,
  type KnowledgeQueryResultDto,
  type KnowledgeResearchReportResultDto,
  type KnowledgeResearchRunDto,
  type KnowledgeResearchRunResultDto,
  type KnowledgeResolvedCitationDto,
  type KnowledgeSourceEntryDto,
} from './knowledge-api';
import styles from './KnowledgePage.module.css';

const tr = (key: string, vars?: Record<string, string | number>) => window.t?.(key, vars) ?? key;

function parseStatusLabel(status: KnowledgeParseStatusDto | undefined): string {
  switch (status) {
    case 'parsing': return tr('knowledge.statusParsing');
    case 'ready': return tr('knowledge.statusReady');
    case 'needs_ocr': return tr('knowledge.statusNeedsOcr');
    case 'failed': return tr('knowledge.statusFailed');
    default: return tr('knowledge.statusParsing');
  }
}

function researchStateLabel(state: KnowledgeResearchRunDto['state']): string {
  const suffix = state.split('_').map(part => part.charAt(0).toUpperCase() + part.slice(1)).join('');
  return tr(`knowledge.researchState${suffix}`);
}

function researchIsTerminal(state: KnowledgeResearchRunDto['state']): boolean {
  return ['completed', 'partial', 'failed', 'canceled'].includes(state);
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

interface CitationHighlight {
  blockId: string;
  startOffset: number;
  endOffset: number;
}

interface SourceViewerProps {
  entry: Pick<KnowledgeSourceEntryDto, 'source' | 'snapshot' | 'parseArtifact'>;
  blocks: KnowledgeBlockDto[];
  loading: boolean;
  error: string | null;
  highlight?: CitationHighlight | null;
  onClose: () => void;
}

function BlockText({ block, highlight }: { block: KnowledgeBlockDto; highlight?: CitationHighlight | null }) {
  if (
    !highlight
    || highlight.blockId !== block.id
    || highlight.startOffset < 0
    || highlight.endOffset <= highlight.startOffset
    || highlight.endOffset > block.text.length
  ) {
    return <>{block.text}</>;
  }
  return (
    <>
      {block.text.slice(0, highlight.startOffset)}
      <mark className={styles.citationHighlight}>{block.text.slice(highlight.startOffset, highlight.endOffset)}</mark>
      {block.text.slice(highlight.endOffset)}
    </>
  );
}

function SourceViewer({ entry, blocks, loading, error, highlight, onClose }: SourceViewerProps) {
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
        <span className={`${styles.status} ${styles[entry.parseArtifact?.status || 'parsing']}`}>
          {parseStatusLabel(entry.parseArtifact?.status)}
        </span>
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
              <span className={styles.blockText}><BlockText block={block} highlight={highlight} /></span>
            </button>
          ))}
        </div>
      </div>
    </section>
  );
}

function AnswerText({
  text,
  result,
  onOpenCitation,
}: {
  text: string;
  result: KnowledgeQueryResultDto;
  onOpenCitation: (citationId: string) => void;
}) {
  const citationByMarker = new Map(
    result.run.citations.map(citation => [citation.marker, citation.citationId]),
  );
  return (
    <div className={styles.answerText}>
      {text.split(/(\[[1-9][0-9]*\])/gu).map((part, index) => {
        const match = /^\[([1-9][0-9]*)\]$/u.exec(part);
        const citationId = match ? citationByMarker.get(Number(match[1])) : null;
        if (!match || !citationId) return <span key={`${index}-${part}`}>{part}</span>;
        return (
          <button
            key={`${index}-${part}`}
            className={styles.inlineCitation}
            onClick={() => onOpenCitation(citationId)}
            aria-label={tr('knowledge.openCitation', { number: Number(match[1]) })}
          >
            {part}
          </button>
        );
      })}
    </div>
  );
}

export function KnowledgePage() {
  const addToast = useStore(state => state.addToast);
  const [notebooks, setNotebooks] = useState<KnowledgeNotebookDto[]>([]);
  const [selectedNotebookId, setSelectedNotebookId] = useState<string | null>(null);
  const [scopeIds, setScopeIds] = useState<string[]>([]);
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
  const [showScopePicker, setShowScopePicker] = useState(false);
  const [mode, setMode] = useState<'quick' | 'research'>('quick');
  const [question, setQuestion] = useState('');
  const [asking, setAsking] = useState(false);
  const [queryResult, setQueryResult] = useState<KnowledgeQueryResultDto | null>(null);
  const [researchResult, setResearchResult] = useState<KnowledgeResearchRunResultDto | null>(null);
  const [researchReport, setResearchReport] = useState<KnowledgeResearchReportResultDto | null>(null);
  const [resolvedCitation, setResolvedCitation] = useState<KnowledgeResolvedCitationDto | null>(null);
  const userStartedQuery = useRef(false);

  const selectedNotebook = notebooks.find(item => item.id === selectedNotebookId) ?? null;
  const selectedSource = sources.find(item => item.source.id === selectedSourceId) ?? null;
  const viewerEntry = resolvedCitation || selectedSource;
  const viewerBlocks = resolvedCitation ? [resolvedCitation.block] : blocks;
  const citationHighlight = resolvedCitation ? {
    blockId: resolvedCitation.citation.blockId,
    startOffset: resolvedCitation.citation.startOffset,
    endOffset: resolvedCitation.citation.endOffset,
  } : null;

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
    let active = true;
    listActiveKnowledgeResearchRuns()
      .then(runs => {
        if (!active || userStartedQuery.current || runs.length === 0) return;
        const recovered = runs[0];
        setMode('research');
        setQuestion(recovered.run.question);
        setScopeIds(recovered.scope.notebooks.map(notebook => notebook.notebookId));
        setResearchResult(recovered);
        setAsking(true);
      })
      .catch(error => {
        if (active) setPageError(error instanceof Error ? error.message : tr('knowledge.resumeLoadFailed'));
      });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    if (!selectedNotebookId) {
      setSources([]);
      setSelectedSourceId(null);
      return;
    }
    setScopeIds(current => current.includes(selectedNotebookId) ? current : [...current, selectedNotebookId]);
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
    if (!artifact || artifact.status !== 'ready') {
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
  }, [selectedSource?.parseArtifact, selectedSource?.source.id]);

  const activeResearchRunId = researchResult && !researchIsTerminal(researchResult.research.state)
    ? researchResult.run.id
    : null;

  useEffect(() => {
    if (!activeResearchRunId) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const poll = async () => {
      try {
        const result = await getKnowledgeResearchRun(activeResearchRunId);
        if (!active) return;
        if (result.research.state === 'completed') {
          try {
            const report = await getKnowledgeResearchReport(activeResearchRunId);
            if (!active) return;
            setResearchResult(result);
            setResearchReport(report);
            setAsking(false);
          } catch (error) {
            if (!active) return;
            setResearchResult(result);
            setAsking(false);
            setPageError(error instanceof Error ? error.message : tr('knowledge.queryFailed'));
          }
          return;
        }
        setResearchResult(result);
        if (researchIsTerminal(result.research.state)) {
          setAsking(false);
          if (result.research.state === 'partial') setPageError(tr('knowledge.researchPartialHint'));
          if (result.research.state === 'failed') setPageError(tr('knowledge.researchFailedHint'));
          return;
        }
        timer = setTimeout(() => void poll(), 750);
      } catch (error) {
        if (!active) return;
        setAsking(false);
        setPageError(error instanceof Error ? error.message : tr('knowledge.queryFailed'));
      }
    };
    timer = setTimeout(() => void poll(), 100);
    return () => {
      active = false;
      if (timer) clearTimeout(timer);
    };
  }, [activeResearchRunId]);

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
      setScopeIds(current => current.filter(id => id !== notebook.id));
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

  const handleAsk = async () => {
    const normalizedQuestion = question.trim();
    if (!normalizedQuestion || scopeIds.length === 0 || asking) return;
    userStartedQuery.current = true;
    setAsking(true);
    setPageError(null);
    setSelectedSourceId(null);
    setResolvedCitation(null);
    setResearchReport(null);
    try {
      if (mode === 'quick') {
        const result = await runKnowledgeQuickAnswer({
          question: normalizedQuestion,
          notebookIds: scopeIds,
        });
        setResearchResult(null);
        setQueryResult(result);
        setAsking(false);
      } else {
        const result = await runKnowledgeResearch({
          question: normalizedQuestion,
          notebookIds: scopeIds,
        });
        setQueryResult(null);
        setResearchResult(result);
      }
    } catch (error) {
      setPageError(error instanceof Error ? error.message : tr('knowledge.queryFailed'));
      setAsking(false);
    }
  };

  const handleCancelResearch = async () => {
    if (!activeResearchRunId) return;
    try {
      const result = await cancelKnowledgeResearch(activeResearchRunId);
      setResearchResult(current => current ? {
        ...current,
        run: result.run,
        research: result.research,
      } : current);
      setAsking(false);
    } catch (error) {
      setPageError(error instanceof Error ? error.message : tr('knowledge.queryFailed'));
    }
  };

  const handleOpenCitation = async (citationId: string) => {
    setViewerError(null);
    try {
      const resolved = await resolveKnowledgeCitation(citationId);
      setSelectedSourceId(null);
      setResolvedCitation(resolved);
    } catch (error) {
      setPageError(error instanceof Error ? error.message : tr('knowledge.citationLoadFailed'));
    }
  };

  const canSend = question.trim().length > 0 && scopeIds.length > 0 && !asking;
  const researchCoverageItems = researchResult ? [
    [tr('knowledge.coverageSourceReadiness'), researchResult.research.coverage.sourceReadiness],
    [tr('knowledge.coverageExtraction'), researchResult.research.coverage.extraction],
    [tr('knowledge.coveragePrimaryScan'), researchResult.research.coverage.primaryScan],
    [tr('knowledge.coverageContradiction'), researchResult.research.coverage.contradiction],
    [tr('knowledge.coverageCitationValidation'), researchResult.research.coverage.citationValidation],
  ] as const : [];
  const reportCitationByMarker = new Map(
    (researchReport?.citations || []).map(citation => [citation.marker, citation]),
  );

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
            {selectedNotebook && <p>{tr('knowledge.sourceCount', { count: sources.length })}</p>}
          </div>
          <button
            className={styles.importButton}
            onClick={() => setShowImportMenu(value => !value)}
            disabled={!selectedNotebookId || importing}
            aria-expanded={showImportMenu}
          >
            {importing ? tr('knowledge.importing') : `＋ ${tr('knowledge.addSource')}`}
          </button>
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
          {sources.map(entry => (
            <div
              key={entry.source.id}
              className={`${styles.sourceRow} ${selectedSourceId === entry.source.id ? styles.selected : ''}`}
            >
              <button className={styles.sourceSelect} onClick={() => {
                setResolvedCitation(null);
                setSelectedSourceId(entry.source.id);
              }}>
                <span className={styles.fileGlyph}>
                  {entry.source.sourceType === 'web_snapshot' ? '◎' : entry.source.sourceType === 'pasted_text' ? '≡' : '▤'}
                </span>
                <span className={styles.sourceInfo}>
                  <strong>{entry.source.displayName}</strong>
                  <span>{formatBytes(entry.snapshot.byteSize)}</span>
                </span>
                <span className={`${styles.status} ${styles[entry.parseArtifact?.status || 'parsing']}`}>
                  {parseStatusLabel(entry.parseArtifact?.status)}
                </span>
              </button>
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
          ))}
        </div>
      </section>

      <main className={styles.analysisPane}>
        {viewerEntry ? (
          <SourceViewer
            entry={viewerEntry}
            blocks={viewerBlocks}
            loading={resolvedCitation ? false : loadingBlocks}
            error={viewerError}
            highlight={citationHighlight}
            onClose={() => {
              setSelectedSourceId(null);
              setResolvedCitation(null);
            }}
          />
        ) : (
          <section className={styles.composer}>
            <header>
              <span className={styles.eyebrow}>{tr('knowledge.analysis')}</span>
              <h2>{tr('knowledge.askYourKnowledge')}</h2>
              <p>{tr('knowledge.scopeHint')}</p>
            </header>

            <div className={styles.fieldGroup}>
              <span className={styles.fieldLabel}>{tr('knowledge.scope')}</span>
              <div className={styles.scopeRow}>
                {scopeIds.map(id => {
                  const notebook = notebooks.find(item => item.id === id);
                  if (!notebook) return null;
                  return (
                    <button
                      key={id}
                      className={styles.scopeChip}
                      onClick={() => {
                        if (scopeIds.length > 1) setScopeIds(current => current.filter(scopeId => scopeId !== id));
                      }}
                      title={scopeIds.length > 1 ? tr('knowledge.removeFromScope') : tr('knowledge.scopeCannotBeEmpty')}
                    >
                      {notebook.name} {scopeIds.length > 1 && '×'}
                    </button>
                  );
                })}
                <button className={styles.addScopeButton} onClick={() => setShowScopePicker(value => !value)}>
                  ＋ {tr('knowledge.addNotebook')}
                </button>
              </div>
              {showScopePicker && (
                <div className={styles.scopePicker}>
                  {notebooks.map(notebook => (
                    <label key={notebook.id}>
                      <input
                        type="checkbox"
                        checked={scopeIds.includes(notebook.id)}
                        onChange={event => {
                          setScopeIds(current => event.target.checked
                            ? [...new Set([...current, notebook.id])]
                            : current.length > 1
                              ? current.filter(id => id !== notebook.id)
                              : current);
                        }}
                      />
                      <span>{notebook.name}</span>
                    </label>
                  ))}
                </div>
              )}
            </div>

            <div className={styles.fieldGroup}>
              <span className={styles.fieldLabel}>{tr('knowledge.mode')}</span>
              <div className={styles.modeSwitch}>
                <button className={mode === 'quick' ? styles.activeMode : ''} onClick={() => setMode('quick')}>
                  {tr('knowledge.quickAnswer')}
                </button>
                <button className={mode === 'research' ? styles.activeMode : ''} onClick={() => setMode('research')}>
                  {tr('knowledge.fullResearch')}
                </button>
              </div>
            </div>

            <div className={styles.questionCard}>
              <textarea
                value={question}
                placeholder={tr('knowledge.questionPlaceholder')}
                disabled={asking}
                maxLength={4000}
                onChange={event => setQuestion(event.target.value)}
                onKeyDown={event => {
                  if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
                    event.preventDefault();
                    void handleAsk();
                  }
                }}
              />
              <div className={styles.questionFooter}>
                <span>{mode === 'quick'
                  ? tr('knowledge.quickRetrievalHint')
                  : tr('knowledge.researchFullScanHint')}</span>
                <button disabled={!canSend} onClick={() => void handleAsk()}>
                  {asking
                    ? mode === 'research' ? tr('knowledge.researching') : tr('knowledge.answering')
                    : tr('knowledge.send')}
                </button>
              </div>
            </div>

            {queryResult?.run.answerText && (
              <section className={styles.answerCard} aria-label={tr('knowledge.answerResult')}>
                <header className={styles.answerHeader}>
                  <div>
                    <span className={styles.fieldLabel}>{tr('knowledge.quickAnswer')}</span>
                    <h3>{tr('knowledge.answerResult')}</h3>
                  </div>
                  <span className={styles.retrievalBadge}>{tr('knowledge.relatedContentBasis')}</span>
                </header>

                <AnswerText
                  text={queryResult.run.answerText}
                  result={queryResult}
                  onOpenCitation={citationId => void handleOpenCitation(citationId)}
                />

                <div className={styles.answerSection}>
                  <h4>{tr('knowledge.citations')}</h4>
                  <div className={styles.citationList}>
                    {queryResult.citations.map(item => {
                      const ref = queryResult.run.citations.find(entry => entry.marker === item.marker);
                      return (
                        <button
                          key={item.citation.id}
                          onClick={() => {
                            if (ref) void handleOpenCitation(ref.citationId);
                          }}
                        >
                          <strong>[{item.marker}] {item.citation.canonicalText}</strong>
                          <span>{item.source.displayName}</span>
                        </button>
                      );
                    })}
                  </div>
                </div>

                <div className={styles.answerSection}>
                  <h4>{tr('knowledge.scopeSnapshot')}</h4>
                  <div className={styles.scopeSnapshotList}>
                    {queryResult.scope.notebooks.map(notebook => (
                      <span key={notebook.notebookId}>{notebook.notebookName}</span>
                    ))}
                  </div>
                </div>
              </section>
            )}

            {researchResult && (
              <section className={`${styles.answerCard} ${styles.researchCard}`} aria-label={tr('knowledge.researchResult')}>
                <header className={styles.answerHeader}>
                  <div>
                    <span className={styles.fieldLabel}>{tr('knowledge.fullResearch')}</span>
                    <h3>{researchReport?.report.title || tr('knowledge.researchResult')}</h3>
                  </div>
                  <div className={styles.researchHeaderActions}>
                    <span className={styles.retrievalBadge}>
                      {researchStateLabel(researchResult.research.state)}
                    </span>
                    {activeResearchRunId && (
                      <button className={styles.cancelResearchButton} onClick={() => void handleCancelResearch()}>
                        {tr('knowledge.cancelResearch')}
                      </button>
                    )}
                  </div>
                </header>

                <div className={styles.coverageGrid} aria-label={tr('knowledge.coverage')}>
                  {researchCoverageItems.map(([label, metric]) => (
                    <div className={styles.coverageItem} key={label}>
                      <span>{label}</span>
                      <strong>{metric.completed} / {metric.total}</strong>
                      <progress max={Math.max(1, metric.total)} value={metric.completed} />
                    </div>
                  ))}
                </div>

                {!researchIsTerminal(researchResult.research.state) && (
                  <p className={styles.researchProgressHint}>{tr('knowledge.researchProgressHint')}</p>
                )}
                {(researchResult.research.state === 'partial' || researchResult.research.state === 'failed') && (
                  <div className={styles.errorNotice}>
                    {researchResult.research.state === 'partial'
                      ? tr('knowledge.researchPartialHint')
                      : tr('knowledge.researchFailedHint')}
                    {researchResult.research.errorCode && ` (${researchResult.research.errorCode})`}
                  </div>
                )}

                {researchReport && (
                  <>
                    <div className={styles.answerText}>{researchReport.report.summary}</div>
                    {([
                      [tr('knowledge.researchConclusions'), researchReport.report.conclusions],
                      [tr('knowledge.researchMajorFindings'), researchReport.report.majorFindings],
                      [tr('knowledge.researchConflicts'), researchReport.report.conflicts],
                    ] as const).map(([heading, items]) => items.length > 0 && (
                      <div className={styles.answerSection} key={heading}>
                        <h4>{heading}</h4>
                        <div className={styles.researchItemList}>
                          {items.map((item, itemIndex) => (
                            <div key={`${heading}-${itemIndex}`} className={styles.researchItem}>
                              <span>{item.text}</span>
                              <span className={styles.reportMarkers}>
                                {item.citationMarkers.map(marker => {
                                  const citation = reportCitationByMarker.get(marker);
                                  return citation ? (
                                    <button
                                      key={marker}
                                      className={styles.inlineCitation}
                                      onClick={() => void handleOpenCitation(citation.citation.id)}
                                      aria-label={tr('knowledge.openCitation', { number: marker })}
                                    >
                                      [{marker}]
                                    </button>
                                  ) : null;
                                })}
                              </span>
                            </div>
                          ))}
                        </div>
                      </div>
                    ))}

                    {researchReport.report.uncertainties.length > 0 && (
                      <div className={styles.answerSection}>
                        <h4>{tr('knowledge.researchUncertainties')}</h4>
                        <ul className={styles.researchNotes}>
                          {researchReport.report.uncertainties.map(item => <li key={item}>{item}</li>)}
                        </ul>
                      </div>
                    )}
                    {researchReport.report.limitations.length > 0 && (
                      <div className={styles.answerSection}>
                        <h4>{tr('knowledge.researchLimitations')}</h4>
                        <ul className={styles.researchNotes}>
                          {researchReport.report.limitations.map(item => <li key={item}>{item}</li>)}
                        </ul>
                      </div>
                    )}

                    <div className={styles.answerSection}>
                      <h4>{tr('knowledge.citations')}</h4>
                      <div className={styles.citationList}>
                        {researchReport.citations.map(item => (
                          <button
                            key={item.citation.id}
                            onClick={() => void handleOpenCitation(item.citation.id)}
                          >
                            <strong>[{item.marker}] {item.citation.canonicalText}</strong>
                            <span>{item.source.displayName}</span>
                          </button>
                        ))}
                      </div>
                    </div>
                  </>
                )}

                <div className={styles.answerSection}>
                  <h4>{tr('knowledge.scopeSnapshot')}</h4>
                  <div className={styles.scopeSnapshotList}>
                    {researchResult.scope.notebooks.map(notebook => (
                      <span key={notebook.notebookId}>{notebook.notebookName}</span>
                    ))}
                  </div>
                </div>
                <div className={styles.answerSection}>
                  <h4>{tr('knowledge.sourceSnapshots')}</h4>
                  <div className={styles.sourceSnapshotList}>
                    {researchResult.scope.sources.map(source => (
                      <div key={`${source.notebookId}:${source.sourceId}`}>
                        <span>{source.sourceDisplayName}</span>
                        <code>{tr('knowledge.snapshotShortId', {
                          id: source.contentSnapshotId.slice(0, 12),
                        })}</code>
                      </div>
                    ))}
                  </div>
                </div>
              </section>
            )}
          </section>
        )}
      </main>

      {pageError && (
        <div className={styles.pageError} role="alert">
          <span>{pageError}</span>
          <button onClick={() => setPageError(null)}>×</button>
        </div>
      )}
    </div>
  );
}
