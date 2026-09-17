/**
 * ObservabilityLedgerPanel.tsx — 「调用台账」子标签页。
 *
 * 筛选条（无 Group By，分组维度只服务用量聚合）+ 逐次调用列表。
 * 调用详情抽屉挂在 section 顶层（浮层不属于任何子页），本页只负责
 * 把行点击/「按此 ID 筛选」回传。「按此 ID 筛选」落在本页自己的筛选上。
 */
import React, { useCallback } from 'react';
import styles from '../../Settings.module.css';
import type { ObservabilityQueryStateApi } from './use-observability-query-state';
import { ObservabilityFilterBar } from './ObservabilityFilterBar';
import { ObservabilityCallLedger } from './ObservabilityCallLedger';

type Props = {
  state: ObservabilityQueryStateApi;
  refreshToken: number;
  /** section 级 Inspector 抽屉的当前选中（跨子页共享的浮层状态）。 */
  selectedCallId: string | null;
  onSelectCall: (callId: string) => void;
};

export function ObservabilityLedgerPanel({
  state,
  refreshToken,
  selectedCallId,
  onSelectCall,
}: Props) {
  const handleFilterExact = useCallback((field: 'sessionId' | 'conversationId' | 'agentId' | 'taskId', value: string) => {
    state.setDrafts({ [field]: value });
    state.patchFilter({ [field]: value });
  }, [state]);

  return (
    <div className={styles['observability-sub-panel']}>
      <ObservabilityFilterBar state={state} showGroupBy={false} />

      <section className={styles['observability-panel']}>
        <ObservabilityCallLedger
          appliedFilter={state.appliedFilter}
          selectedCallId={selectedCallId}
          onSelectCall={onSelectCall}
          onFilterExact={handleFilterExact}
          refreshToken={refreshToken}
        />
      </section>
    </div>
  );
}
