import { useCallback, useEffect, useMemo, useState } from 'react';
import { Button, Overlay } from '@/ui';
import { t } from '../../helpers';
import { diffLines, type DiffLine } from '../../../utils/line-diff';
import {
  dreamSectionsEqual,
  loadDreamRevision,
  loadDreamRevisions,
  restoreDream,
  type DreamRevisionDetailPayload,
  type DreamRevisionSummary,
  type DreamSectionsSnapshot,
} from './agent-memory-dream-actions';
import styles from './DreamRevisionBrowser.module.css';
import { dreamErrorText } from './dream-error-presenter';

function formatTime(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

type SectionDiff = {
  changed: boolean;
  /** null 且 changed：超过 line-diff 输入上限，显式降级为不展示差异（不做假 diff）。 */
  lines: DiffLine[] | null;
};

/**
 * diff 方向固定为 current → revision：added 行是"恢复后会出现"的内容，
 * removed 行是"恢复后会被移除"的当前内容。用户看到的就是恢复后的变化。
 */
function diffSectionBodies(currentBody: string, revisionBody: string): SectionDiff {
  if (currentBody === revisionBody) return { changed: false, lines: null };
  return { changed: true, lines: diffLines(currentBody, revisionBody) };
}

function diffWeekDays(
  current: DreamSectionsSnapshot,
  revision: DreamSectionsSnapshot,
): Array<SectionDiff & { date: string }> {
  const currentByDate = new Map(current.weekDays.map((day) => [day.date, day.body]));
  const revisionByDate = new Map(revision.weekDays.map((day) => [day.date, day.body]));
  const dates = [...new Set([...revisionByDate.keys(), ...currentByDate.keys()])].sort();
  return dates.map((date) => ({
    date,
    ...diffSectionBodies(currentByDate.get(date) ?? '', revisionByDate.get(date) ?? ''),
  }));
}

function SectionDiffView({ diff }: { diff: SectionDiff }) {
  if (!diff.changed) {
    return <div className={styles.unchanged}>{t('settings.memory.dream.revisions.sectionUnchanged')}</div>;
  }
  if (diff.lines === null) {
    return <div className={styles.unchanged}>{t('settings.memory.dream.revisions.diffUnavailable')}</div>;
  }
  return (
    <pre className={`${styles.sectionBody} ${styles.diffBody}`}>
      {diff.lines.map((line, index) => (
        <span
          key={index}
          className={line.kind === 'added'
            ? styles.lineAdded
            : line.kind === 'removed'
              ? styles.lineRemoved
              : styles.lineSame}
        >
          {line.kind === 'added' ? '+ ' : line.kind === 'removed' ? '- ' : '  '}
          {line.text}
          {'\n'}
        </span>
      ))}
    </pre>
  );
}

export function DreamRevisionBrowser({
  agentId,
  open,
  onClose,
}: {
  agentId: string;
  open: boolean;
  onClose: () => void;
}) {
  const [revisions, setRevisions] = useState<DreamRevisionSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<DreamRevisionDetailPayload | null>(null);
  // detail effect 的显式刷新信号：恢复成功后 current 已变，但选中项没变，
  // 靠 nonce 强制重取，保证 diff 永远反映真实当前状态。
  const [detailNonce, setDetailNonce] = useState(0);
  const [loading, setLoading] = useState(false);
  const [detailLoading, setDetailLoading] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async (signal?: AbortSignal) => {
    setLoading(true);
    try {
      const next = await loadDreamRevisions(agentId, signal);
      setRevisions(next);
      setSelectedId((current) => current && next.some((item) => item.revisionId === current)
        ? current
        : next[0]?.revisionId || null);
      setError(null);
    } catch (err: unknown) {
      if (signal?.aborted) return;
      setError(dreamErrorText(err, 'settings.memory.dream.errors.revisionsLoadFailed'));
    } finally {
      if (!signal?.aborted) setLoading(false);
    }
  }, [agentId]);

  useEffect(() => {
    if (!open) return undefined;
    const controller = new AbortController();
    setMessage(null);
    setConfirming(false);
    void refresh(controller.signal);
    return () => controller.abort();
  }, [open, refresh]);

  useEffect(() => {
    if (!open || !selectedId) {
      setDetail(null);
      return undefined;
    }
    const controller = new AbortController();
    setDetail(null);
    setDetailLoading(true);
    setConfirming(false);
    void loadDreamRevision(agentId, selectedId, controller.signal)
      .then((next) => {
        if (!controller.signal.aborted) {
          setDetail(next);
          setError(null);
        }
      })
      .catch((err: unknown) => {
        if (!controller.signal.aborted) {
          setDetail(null);
          setError(dreamErrorText(err, 'settings.memory.dream.errors.revisionLoadFailed'));
        }
      })
      .finally(() => {
        if (!controller.signal.aborted) setDetailLoading(false);
      });
    return () => controller.abort();
  }, [agentId, open, selectedId, detailNonce]);

  const enterConfirm = async () => {
    if (!selectedId) return;
    setDetailLoading(true);
    try {
      // 进入确认前现取一次：浏览器开着期间记忆可能已被改写，
      // 确认时看到的 current-vs-revision 差异必须是最新的。
      const fresh = await loadDreamRevision(agentId, selectedId);
      setDetail(fresh);
      setError(null);
      setConfirming(true);
    } catch (err: unknown) {
      setError(dreamErrorText(err, 'settings.memory.dream.errors.revisionLoadFailed'));
    } finally {
      setDetailLoading(false);
    }
  };

  const restoreSelected = async () => {
    if (!selectedId) return;
    setRestoring(true);
    try {
      await restoreDream(agentId, selectedId);
      setMessage(t('settings.memory.dream.revisions.restored'));
      setConfirming(false);
      setError(null);
      await refresh();
      setDetailNonce((nonce) => nonce + 1);
    } catch (err: unknown) {
      setError(dreamErrorText(err, 'settings.memory.dream.errors.restoreFailed'));
    } finally {
      setRestoring(false);
    }
  };

  const sectionDiffs = useMemo(() => {
    if (!detail) return null;
    const { current, revision } = detail;
    return {
      facts: diffSectionBodies(current.facts, revision.before.facts),
      today: diffSectionBodies(current.today, revision.before.today),
      longterm: diffSectionBodies(current.longterm, revision.before.longterm),
      week: diffWeekDays(current, revision.before),
    };
  }, [detail]);

  const identical = detail ? dreamSectionsEqual(detail.current, detail.revision.before) : false;

  return (
    <Overlay
      scope="inline"
      open={open}
      onClose={onClose}
      backdrop="blur"
      zIndex={110}
      className={styles.dialog}
      contained
      contentProps={{ role: 'dialog', 'aria-modal': true, 'aria-labelledby': 'dream-revisions-title' }}
    >
      <header className={styles.header}>
        <h3 className={styles.title} id="dream-revisions-title">
          {t('settings.memory.dream.revisions.title')}
        </h3>
        <Button variant="ghost" size="sm" onClick={onClose}>
          {t('settings.memory.dream.revisions.close')}
        </Button>
      </header>

      <div className={styles.body}>
        <div className={styles.list} aria-label={t('settings.memory.dream.revisions.listLabel')}>
          {loading ? (
            <div className={styles.empty}>{t('settings.memory.dream.revisions.loading')}</div>
          ) : revisions.length === 0 ? (
            <div className={styles.empty}>{t('settings.memory.dream.revisions.empty')}</div>
          ) : revisions.map((revision) => (
            <button
              key={revision.revisionId}
              type="button"
              className={`${styles.revisionButton} ${selectedId === revision.revisionId ? styles.revisionButtonActive : ''}`}
              onClick={() => setSelectedId(revision.revisionId)}
              aria-current={selectedId === revision.revisionId ? 'true' : undefined}
            >
              <span className={styles.revisionTime}>{formatTime(revision.createdAt)}</span>
              <span className={styles.revisionMeta}>
                {t(revision.kind === 'pre_restore'
                  ? 'settings.memory.dream.revisions.preRestore'
                  : revision.trigger === 'automatic'
                    ? 'settings.memory.dream.revisions.automatic'
                    : 'settings.memory.dream.revisions.manual')}
                {' · '}{revision.bodyChars} {t('settings.memory.dream.revisions.characters')}
              </span>
            </button>
          ))}
        </div>

        <div className={styles.detail}>
          {detailLoading ? (
            <div className={styles.empty}>{t('settings.memory.dream.revisions.loading')}</div>
          ) : detail && sectionDiffs ? (
            <>
              <div className={styles.diffLegend}>
                <span>{t('settings.memory.dream.revisions.diffLegendAdded')}</span>
                <span>{t('settings.memory.dream.revisions.diffLegendRemoved')}</span>
              </div>
              {identical && (
                <div className={styles.noDifference} role="status">
                  {t('settings.memory.dream.revisions.noDifference')}
                </div>
              )}
              {([
                { key: 'facts', title: t('settings.memory.editableFactsLabel'), diff: sectionDiffs.facts },
                { key: 'today', title: t('settings.memory.sections.today'), diff: sectionDiffs.today },
                { key: 'longterm', title: t('settings.memory.sections.longterm'), diff: sectionDiffs.longterm },
              ] as const).map((section) => (
                <section className={styles.section} key={section.key}>
                  <h4 className={styles.sectionTitle}>{section.title}</h4>
                  <SectionDiffView diff={section.diff} />
                </section>
              ))}
              <section className={styles.section}>
                <h4 className={styles.sectionTitle}>{t('settings.memory.sections.week')}</h4>
                {sectionDiffs.week.length === 0 ? (
                  <pre className={styles.sectionBody}>{t('settings.memory.dream.revisions.noContent')}</pre>
                ) : sectionDiffs.week.map((day) => (
                  <div className={styles.weekDay} key={day.date}>
                    <span className={styles.weekDate}>{day.date}</span>
                    <SectionDiffView diff={day} />
                  </div>
                ))}
              </section>
            </>
          ) : null}
        </div>
      </div>

      {error && <div className={styles.error} role="alert">{error}</div>}
      {message && <div className={styles.message} role="status">{message}</div>}

      <footer className={styles.footer}>
        <span className={styles.confirmText}>
          {confirming ? t('settings.memory.dream.revisions.confirmHint') : ''}
        </span>
        {confirming ? (
          <div className={styles.confirm}>
            <Button size="sm" onClick={() => setConfirming(false)} disabled={restoring}>
              {t('settings.memory.dream.revisions.cancel')}
            </Button>
            <Button variant="primary" size="sm" loading={restoring} onClick={() => { void restoreSelected(); }}>
              {t('settings.memory.dream.revisions.confirmRestore')}
            </Button>
          </div>
        ) : (
          <Button
            variant="primary"
            size="sm"
            disabled={!detail || detail.revision.revisionId !== selectedId || detailLoading || identical}
            onClick={() => { void enterConfirm(); }}
            title={identical ? t('settings.memory.dream.revisions.noDifference') : undefined}
          >
            {t('settings.memory.dream.revisions.restoreThis')}
          </Button>
        )}
      </footer>
    </Overlay>
  );
}
