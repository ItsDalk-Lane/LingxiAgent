/**
 * use-observability-query-state.ts — 统一查询状态 hook（Phase 9 §十三/十四）。
 *
 * 单一事实源：Filter Bar / Metrics / Groups / Call Ledger / Trace Explorer /
 * Export 全部消费同一份 appliedFilter——不存在「每个面板自己一套 filter」。
 *
 * 纪律：
 *   - 文本输入（custom 日期 / 精确 ID）draft 与 applied 分离：Enter 或
 *     「应用」提交（applyTextDrafts）；多选/tri-state 立即 applied（§十四）。
 *   - appliedFilter 不可变更新（新引用）——下游 effect 以引用为依赖，
 *     filter 一变 ledger cursor 自动作废重查（§四十五 cursor 主动重置）。
 *   - 不在这里做网络请求（网络在 panel 层经 generation/AbortController 管理）。
 */
import { useCallback, useState } from 'react';
import {
  MODEL_OBSERVABILITY_GROUP_BY_MAX_DIMENSIONS,
  type ModelObservabilityGroupByDimension,
} from '../../../../../../shared/model-observability-api-contract.ts';
import {
  clearAllFilterChips,
  DEFAULT_OBSERVABILITY_FILTER,
  DEFAULT_OBSERVABILITY_GROUP_BY,
  isRelativeDatePreset,
  removeFilterChip,
  stampDateAnchorIfMissing,
  type ObservabilityFilterChip,
  type ObservabilityFilterState,
} from './model-observability-filter';

/** 未提交的文本输入（custom 日期 + 精确 ID；§十四 draft vs applied）。 */
export type ObservabilityTextDrafts = {
  customSince: string;
  customUntil: string;
  sessionId: string;
  conversationId: string;
  agentId: string;
  taskId: string;
};

const EMPTY_DRAFTS: ObservabilityTextDrafts = {
  customSince: '',
  customUntil: '',
  sessionId: '',
  conversationId: '',
  agentId: '',
  taskId: '',
};

export type ObservabilityQueryStateApi = {
  /** canonical applied filter（唯一查询事实源，§十四）。 */
  appliedFilter: ObservabilityFilterState;
  drafts: ObservabilityTextDrafts;
  groupBy: ModelObservabilityGroupByDimension[];
  selectedCallId: string | null;
  selectedTraceId: string | null;
  /** 立即生效控件的合并入口（多选/preset/tri-state）。 */
  patchFilter: (patch: Partial<ObservabilityFilterState>) => void;
  setDrafts: (patch: Partial<ObservabilityTextDrafts>) => void;
  /** 提交文本 draft → applied（Enter/应用按钮；§十四）。 */
  applyTextDrafts: () => void;
  setGroupBy: (next: ModelObservabilityGroupByDimension[]) => void;
  selectCall: (callId: string | null) => void;
  selectTrace: (traceId: string | null) => void;
  removeChip: (chip: ObservabilityFilterChip) => void;
  clearAllFilters: () => void;
};

export function useObservabilityQueryState(): ObservabilityQueryStateApi {
  // 挂载即给默认 7d 打锚点：since 不随后续请求时刻滚动（游标指纹契约）。
  const [appliedFilter, setAppliedFilter] = useState<ObservabilityFilterState>(
    () => stampDateAnchorIfMissing({ ...DEFAULT_OBSERVABILITY_FILTER }),
  );
  const [drafts, setDraftsState] = useState<ObservabilityTextDrafts>(EMPTY_DRAFTS);
  const [groupBy, setGroupByState] = useState<ModelObservabilityGroupByDimension[]>([...DEFAULT_OBSERVABILITY_GROUP_BY]);
  const [selectedCallId, setSelectedCallId] = useState<string | null>(null);
  const [selectedTraceId, setSelectedTraceId] = useState<string | null>(null);

  const patchFilter = useCallback((patch: Partial<ObservabilityFilterState>) => {
    setAppliedFilter((prev) => {
      const next = { ...prev, ...patch };
      // 重选相对预设 → 换新锚（新窗口）；同一预设期内锚点保持不变。
      if (patch.datePreset !== undefined && patch.datePreset !== prev.datePreset && isRelativeDatePreset(patch.datePreset)) {
        return stampDateAnchorIfMissing({ ...next, presetAnchorMs: null });
      }
      return stampDateAnchorIfMissing(next);
    });
  }, []);

  const setDrafts = useCallback((patch: Partial<ObservabilityTextDrafts>) => {
    setDraftsState((prev) => ({ ...prev, ...patch }));
  }, []);

  const applyTextDrafts = useCallback(() => {
    setAppliedFilter((prev) => ({
      ...prev,
      customSince: drafts.customSince,
      customUntil: drafts.customUntil,
      sessionId: drafts.sessionId,
      conversationId: drafts.conversationId,
      agentId: drafts.agentId,
      taskId: drafts.taskId,
    }));
  }, [drafts]);

  const setGroupBy = useCallback((next: ModelObservabilityGroupByDimension[]) => {
    const deduped: ModelObservabilityGroupByDimension[] = [];
    for (const dimension of next) {
      if (!deduped.includes(dimension)) deduped.push(dimension);
    }
    setGroupByState(deduped.slice(0, MODEL_OBSERVABILITY_GROUP_BY_MAX_DIMENSIONS));
  }, []);

  const selectCall = useCallback((callId: string | null) => {
    setSelectedCallId(callId);
  }, []);

  const selectTrace = useCallback((traceId: string | null) => {
    setSelectedTraceId(traceId);
  }, []);

  const removeChip = useCallback((chip: ObservabilityFilterChip) => {
    // removeFilterChip 对 date chip 置空锚点 → 这里统一补新锚。
    setAppliedFilter((prev) => stampDateAnchorIfMissing(removeFilterChip(prev, chip)));
    // exact/date chip 移除时同步清 draft，避免「删了 chip 文本还在」。
    if (chip.kind === 'exact') {
      setDraftsState((prev) => ({ ...prev, [chip.field]: '' }));
    } else if (chip.kind === 'date') {
      setDraftsState((prev) => ({ ...prev, customSince: '', customUntil: '' }));
    }
  }, []);

  const clearAllFilters = useCallback(() => {
    setAppliedFilter(stampDateAnchorIfMissing(clearAllFilterChips()));
    setDraftsState(EMPTY_DRAFTS);
  }, []);

  return {
    appliedFilter,
    drafts,
    groupBy,
    selectedCallId,
    selectedTraceId,
    patchFilter,
    setDrafts,
    applyTextDrafts,
    setGroupBy,
    selectCall,
    selectTrace,
    removeChip,
    clearAllFilters,
  };
}
