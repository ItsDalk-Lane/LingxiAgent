import React from 'react';
import { ModelObservabilitySection } from './observability/ModelObservabilitySection';
import styles from '../Settings.module.css';

/**
 * Phase 9：旧 Usage 页（Usage Ledger 浏览器端聚合）已退出主路径，由
 * Model Observatory 取代。内部 tab id 保持 "usage"（§五：不迁移
 * nav/search/测试/UI 状态）；可见名称经 settings.tabs.usage 升级为
 * 「模型观测」。
 */
export function UsageTab() {
  return (
    <div className={`${styles['settings-tab-content']} ${styles['active']}`} data-tab="usage">
      <ModelObservabilitySection />
    </div>
  );
}
