/**
 * GitHistoryModal — 提交历史弹窗（环境信息卡·提交记录行入口）
 *
 * Git Graph 表格风格（深色）：表头 图|描述|日期|作者|提交，每行 =
 * 泳道图形（SVG，线条全连通）+ refs 徽标 + 提交标题 + 增删统计行 +
 * 绝对时间 + 作者 + 可一键复制的短哈希。泳道布局由 utils/git-graph 纯函数
 * 计算；图形宽度按全图泳道数封顶。标题悬停浮出完整提交信息。
 */
import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { Overlay, Tooltip } from '../../ui';
import { useStore } from '../../stores';
import { fetchGitLog, fetchGitLogStats, type GitCommit, type GitCommitRef, type GitLogStatsResponse } from '../../utils/git-env-api';
import { computeGraphRows, graphLaneCount, type GraphLaneRow } from '../../utils/git-graph';
import styles from './GitHistoryModal.module.css';

const LANE_WIDTH = 14;
const LANE_PAD = 9;
const MAX_LANES = 8;
const ROW_HEIGHT = 46;

function laneX(lane: number): number {
  return LANE_PAD + Math.min(lane, MAX_LANES - 1) * LANE_WIDTH;
}

/** 绝对时间（本地时区 24 小时制）：MM/DD HH:mm */
function formatCommitTime(committedAt: number): { short: string; full: string } {
  const d = new Date(committedAt * 1000);
  if (Number.isNaN(d.getTime())) return { short: '—', full: '' };
  const p = (n: number) => String(n).padStart(2, '0');
  return {
    short: `${p(d.getMonth() + 1)}/${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`,
    full: d.toLocaleString(),
  };
}

/** refs 徽标：HEAD 与其指向的分支各一枚（分离头指针只有 HEAD 一枚） */
function RefChips({ refs }: { refs: GitCommitRef[] }): ReactNode {
  const chips: ReactNode[] = [];
  for (const ref of refs) {
    if (ref.kind === 'head') {
      chips.push(
        <span key={`head:${ref.name}`} className={styles.chipHead}>
          <i className={styles.chipGlyph}>⎇</i>HEAD
        </span>,
      );
      if (ref.name !== 'HEAD') {
        chips.push(
          <span key={`branch:${ref.name}`} className={styles.chipBranch}>
            <i className={styles.chipGlyph}>⎇</i>{ref.name}
          </span>,
        );
      }
    } else if (ref.kind === 'tag') {
      chips.push(
        <span key={`tag:${ref.name}`} className={styles.chipTag}>
          <i className={styles.chipGlyph}>◈</i>{ref.name}
        </span>,
      );
    } else {
      chips.push(
        <span key={`${ref.kind}:${ref.name}`} className={styles.chipRemote}>
          <i className={styles.chipGlyph}>⎇</i>{ref.name}
        </span>,
      );
    }
  }
  return <>{chips}</>;
}

function RowGraph({ row, width, isHead }: {
  row: GraphLaneRow;
  width: number;
  isHead: boolean;
}) {
  const nodeY = ROW_HEIGHT / 2;
  const clamp = (lane: number) => Math.min(lane, MAX_LANES - 1);
  // 本行新建的泳道不画竖线（曲线落到行底，由下一行从行顶续上），否则会出现
  // 悬空断线段；节点泳道被释放（会合）时补一段行顶→节点的竖线接住来线。
  const verticals = row.activeLanes.filter(lane => lane < MAX_LANES && !row.newLanes.includes(lane));
  const nodeReleased = !row.activeLanes.includes(row.nodeLane) && row.nodeLane < MAX_LANES;
  const nodeAlt = row.nodeLane !== 0;
  return (
    <svg className={styles.graph} width={width} height={ROW_HEIGHT} aria-hidden="true">
      {verticals.map(lane => (
        <line
          key={`v${lane}`}
          className={lane === 0 ? styles.laneLineMain : styles.laneLineAlt}
          x1={laneX(lane)}
          y1={0}
          x2={laneX(lane)}
          y2={ROW_HEIGHT}
        />
      ))}
      {nodeReleased && (
        <line
          className={nodeAlt ? styles.laneLineAlt : styles.laneLineMain}
          x1={laneX(row.nodeLane)}
          y1={0}
          x2={laneX(row.nodeLane)}
          y2={nodeY}
        />
      )}
      {row.mergeLanes.filter(lane => lane < MAX_LANES).map(lane => (
        <path
          key={`m${lane}`}
          className={lane === 0 ? styles.laneLineMain : styles.laneLineAlt}
          d={`M ${laneX(clamp(row.nodeLane))} ${nodeY} C ${laneX(clamp(row.nodeLane))} ${nodeY + 16}, ${laneX(lane)} ${ROW_HEIGHT - 16}, ${laneX(lane)} ${ROW_HEIGHT}`}
        />
      ))}
      {isHead && (
        <circle className={styles.headRing} cx={laneX(clamp(row.nodeLane))} cy={nodeY} r={7} />
      )}
      <circle
        className={isHead ? styles.nodeHead : nodeAlt ? styles.nodeAlt : styles.nodeMain}
        cx={laneX(clamp(row.nodeLane))}
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
  // 统计两段加载：列表秒出渲染，变更统计按哈希分块并行补齐（服务端有缓存）
  const [statsState, setStatsState] = useState<'idle' | 'pending' | 'done'>('idle');
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
    setStatsState('pending');
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

  useEffect(() => {
    if (!open || loadState !== 'idle' || statsState !== 'pending') return;
    if (commits == null || commits.length === 0) return;
    let cancelled = false;
    // 注意：此处不得再 setState（曾用 'loading' 中间态，自身触发 cleanup 把
    // cancelled 置 true，真实网络返回时结果被丢弃，统计永远合并不进来）。
    // 每块 100 个哈希（请求体约 4KB），并行发出，先到先渲染
    const chunks: string[][] = [];
    for (let i = 0; i < commits.length; i += 100) chunks.push(commits.slice(i, i + 100).map(c => c.hash));
    Promise.all(chunks.map(chunk =>
      fetchGitLogStats(dir, agentId, chunk).catch((err: unknown) => {
        console.warn('[git-history] 变更统计拉取失败（统计行隐藏）', err);
        return { isRepo: false, stats: {} } satisfies GitLogStatsResponse;
      })
    )).then(results => {
      if (cancelled) return;
      const merged = Object.assign({}, ...results.map(r => r.stats));
      setCommits(prev => {
        if (prev == null) return prev;
        return prev.map(c => {
          const s = merged[c.hash];
          return s ? { ...c, additions: s.additions, deletions: s.deletions, changedFiles: s.changedFiles } : c;
        });
      });
      setStatsState('done');
    });
    return () => { cancelled = true; };
  }, [open, dir, agentId, loadState, statsState, commits]);

  const graphRows = useMemo(() => computeGraphRows(commits ?? []), [commits]);
  const graphWidth = useMemo(
    () => Math.min(graphLaneCount(graphRows), MAX_LANES) * LANE_WIDTH + LANE_PAD * 2,
    [graphRows],
  );

  return (
    <Overlay scope="inline" open={open} onClose={onClose} backdrop="blur" className={styles.modal} disableContainerAnimation>
      <div className={styles.header}>
        <h2 className={styles.title}>{t('gitEnv.history')}</h2>
        <button className={styles.closeBtn} onClick={onClose} aria-label="Close">×</button>
      </div>
      <div className={styles.body} data-testid="git-history-list">
        {loadState === 'loading' && <div className={styles.note}>{t('gitEnv.historyLoading')}</div>}
        {loadState === 'error' && <div className={styles.note}>{t('gitEnv.loadFailed')}</div>}
        {loadState === 'idle' && commits != null && commits.length === 0 && (
          <div className={styles.note}>{t('gitEnv.noCommits')}</div>
        )}
        {loadState === 'idle' && commits != null && commits.length > 0 && (
          <>
            <div className={styles.thead}>
              <div className={styles.colGraph}>
                <span className={styles.graphColLabel}>{t('gitEnv.historyColGraph')}</span>
              </div>
              <div className={styles.colDesc}>{t('gitEnv.historyColDescription')}</div>
              <div className={styles.colDate}>{t('gitEnv.historyColDate')}</div>
              <div className={styles.colAuthor}>{t('gitEnv.historyColAuthor')}</div>
              <div className={styles.colHash}>{t('gitEnv.historyColHash')}</div>
            </div>
            {commits.map((commit, index) => {
              const row = graphRows[index];
              const isHead = commit.refs.some(ref => ref.kind === 'head');
              const time = formatCommitTime(commit.committedAt);
              return (
                <div key={commit.hash} className={`${styles.commitRow}${isHead ? ` ${styles.headRow}` : ''}`} data-testid={`git-commit-${commit.shortHash}`}>
                  <div className={styles.colGraph}>
                    <RowGraph row={row} width={graphWidth} isHead={isHead} />
                  </div>
                  <div className={styles.colDesc}>
                    <div className={styles.subjectLine}>
                      <RefChips refs={commit.refs} />
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
                    </div>
                    {commit.changedFiles > 0 && (
                      <div className={styles.stats}>
                        <span className={styles.statAdd}>+{commit.additions.toLocaleString()}</span>
                        <span className={styles.statDel}>−{commit.deletions.toLocaleString()}</span>
                        <span className={styles.statFiles}>{t('gitEnv.commitFiles', { n: commit.changedFiles })}</span>
                      </div>
                    )}
                  </div>
                  <div className={styles.colDate} title={time.full}>{time.short}</div>
                  <div className={styles.colAuthor}>{commit.authorName}</div>
                  <div className={styles.colHash}>
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
                          {copiedHash === commit.hash ? '✓' : commit.shortHash.slice(0, 7)}
                        </button>
                      )}
                    </Tooltip>
                  </div>
                </div>
              );
            })}
          </>
        )}
      </div>
    </Overlay>
  );
}
