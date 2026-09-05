/**
 * GitHistoryModal — 提交历史弹窗（环境信息卡·提交记录行入口）
 *
 * VS Code 源代码管理图表风格：每行 = 泳道图形（竖线/节点/合并曲线，SVG）
 * + 提交信息与 refs 徽标 + 作者·相对时间 + 右端短哈希徽标。
 * 泳道布局由 utils/git-graph 纯函数计算；图形宽度按全图泳道数封顶。
 */
import { useEffect, useMemo, useState } from 'react';
import { Overlay, Tooltip } from '../../ui';
import { useStore } from '../../stores';
import { fetchGitLog, type GitCommit } from '../../utils/git-env-api';
import { computeGraphRows, graphLaneCount } from '../../utils/git-graph';
import styles from './GitHistoryModal.module.css';

const LANE_WIDTH = 14;
const LANE_PAD = 9;
const MAX_LANES = 8;
const ROW_HEIGHT = 46;

function laneX(lane: number): number {
  return LANE_PAD + Math.min(lane, MAX_LANES - 1) * LANE_WIDTH;
}

function relativeTimeLabel(
  t: (key: string, vars?: Record<string, string | number>) => string,
  committedAt: number,
  nowMs: number,
): string {
  const diffSec = Math.max(0, Math.round((nowMs - committedAt * 1000) / 1000));
  if (diffSec < 60) return t('gitEnv.timeJustNow');
  const minutes = Math.floor(diffSec / 60);
  if (minutes < 60) return t('gitEnv.timeMinutesAgo', { n: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t('gitEnv.timeHoursAgo', { n: hours });
  const days = Math.floor(hours / 24);
  if (days < 14) return t('gitEnv.timeDaysAgo', { n: days });
  return new Date(committedAt * 1000).toLocaleString();
}

function RefChip({ kind, name }: { kind: string; name: string }) {
  const label = kind === 'head' ? `HEAD · ${name}` : name;
  return (
    <span
      className={
        kind === 'head' ? styles.chipHead
          : kind === 'branch' ? styles.chipBranch
            : kind === 'tag' ? styles.chipTag
              : styles.chipRemote
      }
    >
      {label}
    </span>
  );
}

function RowGraph({
  nodeLane,
  activeLanes,
  mergeLanes,
  width,
  isHead,
}: {
  nodeLane: number;
  activeLanes: number[];
  mergeLanes: number[];
  width: number;
  isHead: boolean;
}) {
  const nodeY = ROW_HEIGHT / 2;
  const clamp = (lane: number) => Math.min(lane, MAX_LANES - 1);
  return (
    <svg className={styles.graph} width={width} height={ROW_HEIGHT} aria-hidden="true">
      {activeLanes.filter(lane => lane < MAX_LANES).map(lane => (
        <line
          key={`v${lane}`}
          className={styles.laneLine}
          x1={laneX(lane)}
          y1={0}
          x2={laneX(lane)}
          y2={ROW_HEIGHT}
        />
      ))}
      {mergeLanes.filter(lane => lane < MAX_LANES).map(lane => (
        <path
          key={`m${lane}`}
          className={styles.laneLine}
          d={`M ${laneX(clamp(nodeLane))} ${nodeY} C ${laneX(clamp(nodeLane))} ${nodeY + 16}, ${laneX(lane)} ${ROW_HEIGHT - 16}, ${laneX(lane)} ${ROW_HEIGHT}`}
        />
      ))}
      {isHead && (
        <circle className={styles.headRing} cx={laneX(clamp(nodeLane))} cy={nodeY} r={7} />
      )}
      <circle
        className={isHead ? styles.nodeHead : styles.node}
        cx={laneX(clamp(nodeLane))}
        cy={nodeY}
        r={isHead ? 4.5 : 3.5}
      />
    </svg>
  );
}

interface GitHistoryModalProps {
  open: boolean;
  onClose: () => void;
  dir: string;
  agentId?: string | null;
}

export function GitHistoryModal({ open, onClose, dir, agentId }: GitHistoryModalProps) {
  const t = window.t ?? ((p: string) => p);
  const addToast = useStore(s => s.addToast);
  const [commits, setCommits] = useState<GitCommit[] | null>(null);
  const [loadState, setLoadState] = useState<'idle' | 'loading' | 'error'>('idle');
  const [copiedHash, setCopiedHash] = useState<string | null>(null);

  const copyHash = async (hash: string) => {
    try {
      await navigator.clipboard?.writeText?.(hash);
      setCopiedHash(hash);
      window.setTimeout(() => setCopiedHash(cur => (cur === hash ? null : cur)), 1200);
      addToast?.(t('gitEnv.copied'), 'success');
    } catch {
      addToast?.(t('gitEnv.operationFailed'), 'error');
    }
  };

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setLoadState('loading');
    setCommits(null);
    fetchGitLog(dir, agentId, 300)
      .then(res => {
        if (cancelled) return;
        setCommits(res.commits);
        setLoadState('idle');
      })
      .catch(() => {
        if (!cancelled) setLoadState('error');
      });
    return () => { cancelled = true; };
  }, [open, dir, agentId]);

  const graphRows = useMemo(() => computeGraphRows(commits ?? []), [commits]);
  const graphWidth = useMemo(
    () => Math.min(graphLaneCount(graphRows), MAX_LANES) * LANE_WIDTH + LANE_PAD * 2,
    [graphRows],
  );
  const nowMs = useMemo(() => Date.now(), [open, commits]);

  return (
    <Overlay scope="inline" open={open} onClose={onClose} backdrop="blur" className={styles.modal} disableContainerAnimation>
      <div className={styles.header}>
        <h2 className={styles.title}>{t('gitEnv.history')}</h2>
        <button className={styles.closeBtn} onClick={onClose} aria-label="Close">×</button>
      </div>
      <div className={styles.body} data-testid="git-history-list">
        {loadState === 'loading' && <div className={styles.note}>…</div>}
        {loadState === 'error' && <div className={styles.note}>{t('gitEnv.loadFailed')}</div>}
        {loadState === 'idle' && commits != null && commits.length === 0 && (
          <div className={styles.note}>{t('gitEnv.noCommits')}</div>
        )}
        {loadState === 'idle' && commits != null && commits.map((commit, index) => {
          const row = graphRows[index];
          const isHead = commit.refs.some(ref => ref.kind === 'head');
          return (
            <div key={commit.hash} className={styles.commitRow} data-testid={`git-commit-${commit.shortHash}`}>
              <RowGraph
                nodeLane={row.nodeLane}
                activeLanes={row.activeLanes}
                mergeLanes={row.mergeLanes}
                width={graphWidth}
                isHead={isHead}
              />
              <div className={styles.commitMain}>
                <div className={styles.subjectLine}>
                  <Tooltip
                    content={commit.message || commit.subject}
                    variant="panel"
                    placement="top"
                    align="start"
                  >
                    {({ ref, ...tooltipProps }) => (
                      <span
                        ref={(node) => ref(node)}
                        className={styles.subject}
                        {...tooltipProps}
                      >
                        {commit.subject}
                      </span>
                    )}
                  </Tooltip>
                  {commit.refs.map(ref => (
                    <RefChip key={`${ref.kind}:${ref.name}`} kind={ref.kind} name={ref.name} />
                  ))}
                </div>
                <div className={styles.meta}>
                  <span>{commit.authorName}</span>
                  <span className={styles.metaDot}>·</span>
                  <span>{relativeTimeLabel(t, commit.committedAt, nowMs)}</span>
                </div>
              </div>
              <Tooltip content={commit.hash} placement="top">
                {({ ref, ...tooltipProps }) => (
                  <button
                    type="button"
                    ref={(node) => ref(node)}
                    className={`${styles.hashChip}${copiedHash === commit.hash ? ` ${styles.hashChipCopied}` : ''}`}
                    data-testid={`git-hash-${commit.shortHash}`}
                    aria-label={t('gitEnv.copyHash')}
                    onClick={e => {
                      e.stopPropagation();
                      void copyHash(commit.hash);
                    }}
                    {...tooltipProps}
                  >
                    {copiedHash === commit.hash ? '✓' : commit.shortHash}
                  </button>
                )}
              </Tooltip>
            </div>
          );
        })}
      </div>
    </Overlay>
  );
}
