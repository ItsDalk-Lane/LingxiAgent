import React from 'react';
import { UsageLedgerSection } from './providers/UsageLedgerSection';
import styles from '../Settings.module.css';

export function UsageTab() {
  return (
    <div className={`${styles['settings-tab-content']} ${styles['active']}`} data-tab="usage">
      <UsageLedgerSection />
    </div>
  );
}
