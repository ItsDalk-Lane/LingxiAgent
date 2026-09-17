/**
 * ObservabilityTracesPanel.tsx — 「调用轨迹」子标签页。
 *
 * 筛选条（无 Group By）+ 轨迹列表（轨迹详情弹层由 TraceExplorer 内部挂载）。
 * 轨迹里点开单次调用 → section 级 Inspector 抽屉（浮层，不切页）；
 * 反向「从调用看轨迹」由 section 统一调度（切到本页并选中轨迹）。
 */
import React from 'react';
import styles from '../../Settings.module.css';
import type { ObservabilityQueryStateApi } from './use-observability-query-state';
import { ObservabilityFilterBar } from './ObservabilityFilterBar';
import { ObservabilityTraceExplorer } from './ObservabilityTraceExplorer';

type Props = {
  state: ObservabilityQueryStateApi;
  refreshToken: number;
  /** 轨迹行/详情里点开单次调用 → section 级 Inspector 抽屉。 */
  onSelectCall: (callId: string) => void;
};

export function ObservabilityTracesPanel({
  state,
  refreshToken,
  onSelectCall,
}: Props) {
  return (
    <div className={styles['observability-sub-panel']}>
      <ObservabilityFilterBar state={state} showGroupBy={false} />

      <section className={styles['observability-panel']}>
        <ObservabilityTraceExplorer
          appliedFilter={state.appliedFilter}
          selectedTraceId={state.selectedTraceId}
          onSelectTrace={state.selectTrace}
          onSelectCall={onSelectCall}
          refreshToken={refreshToken}
        />
      </section>
    </div>
  );
}
