/**
 * TrajectoryToolbar.tsx — 轨迹工具栏：时间线与台账折叠控制。
 *
 * 移植自 dsh-desktop packages/client/ui-trajectory/src/client/TrajectoryToolbar.tsx
 * （MIT）。适配点：图标内联 SVG（灵犀无同名 icon 出口）；文案走灵犀 i18n。
 */

import { t } from '../../../helpers';
import css from './TrajectoryToolbar.module.css';

function SearchIcon() {
  return (
    <svg
      width="11"
      height="11"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      aria-hidden="true"
    >
      <circle cx="7" cy="7" r="4.6" />
      <path d="M10.6 10.6 14 14" />
    </svg>
  );
}

export interface TrajectoryToolbarProps {
  /** 时间线块使用记录耗时还是等宽。 */
  actualDuration: boolean;
  onActualDurationChange: (actualDuration: boolean) => void;
  /** 记录耗时是否保留操作间隙。 */
  actualTime: boolean;
  onActualTimeChange: (actualTime: boolean) => void;
  /** 全部可折叠轮次当前是否收起。 */
  allTurnsCollapsed: boolean;
  onToggleAllTurns: () => void;
  /** 全部可折叠助手的工具调用当前是否收起。 */
  allAssistantsCollapsed: boolean;
  onToggleAllAssistants: () => void;
  searchQuery: string;
  onSearchQueryChange: (query: string) => void;
}

/** 渲染 sticky 工具栏。 */
export function TrajectoryToolbar({
  actualDuration,
  onActualDurationChange,
  actualTime,
  onActualTimeChange,
  allTurnsCollapsed,
  onToggleAllTurns,
  allAssistantsCollapsed,
  onToggleAllAssistants,
  searchQuery,
  onSearchQueryChange,
}: TrajectoryToolbarProps) {
  const key = 'settings.observability.traceDetail.toolbar';
  return (
    <div className={css.root} role="toolbar" aria-label={t(`${key}.aria`)}>
      <div className={css.inner}>
        <div className={css.actions}>
          <button
            type="button"
            className={css.toggle}
            aria-label={t(`${key}.useActualDuration`)}
            aria-pressed={actualDuration}
            title={actualDuration ? t(`${key}.useEqualWidth`) : t(`${key}.useActualDuration`)}
            onClick={() => { onActualDurationChange(!actualDuration) }}
          >
            <svg
              className={css.toggleIcon}
              viewBox="0 0 16 16"
              fill="none"
              aria-hidden="true"
            >
              <circle cx="8" cy="8" r="5.25" />
              <path d="M8 4.75V8l2.25 1.5" />
            </svg>
            {t(`${key}.duration`)}
          </button>
          <button
            type="button"
            className={css.control}
            role="switch"
            aria-checked={actualTime}
            hidden
            onClick={() => { onActualTimeChange(!actualTime) }}
          >
            <span>{t(`${key}.actualTime`)}</span>
            <span className={css.controlTrack} data-on={actualTime || undefined} aria-hidden="true">
              <span className={css.controlThumb} />
            </span>
          </button>
          <button
            type="button"
            className={css.action}
            aria-label={allTurnsCollapsed ? t(`${key}.expandTurns`) : t(`${key}.collapseTurns`)}
            aria-pressed={allTurnsCollapsed}
            title={allTurnsCollapsed ? t(`${key}.expandTurns`) : t(`${key}.collapseTurns`)}
            onClick={onToggleAllTurns}
          >
            <span className={css.actionIcon} aria-hidden="true">
              {allTurnsCollapsed ? '⊞' : '⊟'}
            </span>
            {t(`${key}.turns`)}
          </button>
          <button
            type="button"
            className={css.action}
            aria-label={allAssistantsCollapsed ? t(`${key}.expandCalls`) : t(`${key}.collapseCalls`)}
            aria-pressed={allAssistantsCollapsed}
            title={allAssistantsCollapsed ? t(`${key}.expandCalls`) : t(`${key}.collapseCalls`)}
            onClick={onToggleAllAssistants}
          >
            <span className={css.actionIcon} aria-hidden="true">
              {allAssistantsCollapsed ? '⊞' : '⊟'}
            </span>
            {t(`${key}.calls`)}
          </button>
        </div>
        <div className={css.search}>
          <span className={css.searchIcon}><SearchIcon /></span>
          <input
            type="search"
            className={css.searchInput}
            aria-label={t(`${key}.search`)}
            placeholder={t(`${key}.searchPlaceholder`)}
            value={searchQuery}
            onChange={(event) => { onSearchQueryChange(event.currentTarget.value) }}
          />
        </div>
      </div>
    </div>
  );
}
