import React, { useCallback, useEffect, useState } from 'react';
import { useSettingsStore } from '../store';
import { lingxiFetch } from '../api';
import { t } from '../helpers';
import { SettingsSection } from '../components/SettingsSection';
import { SettingsRow } from '../components/SettingsRow';
import styles from '../Settings.module.css';

interface EnvDepStatus {
  id: string;
  label: string;
  kind: 'binary' | 'runtime' | 'managed';
  status: 'installed' | 'missing';
  managed?: boolean;
  version?: string;
  path?: string;
  requiredBy: string[];
  neededByProject: boolean;
  installHint?: string;
}

interface EnvDepsReport {
  checkedAt: string;
  deps: EnvDepStatus[];
  summary: { total: number; installed: number; missing: number; projectMissing: string[] };
}

function requiredByLabels(dep: EnvDepStatus): string {
  return dep.requiredBy.map(id => t(`envDeps.requiredBy.${id}`)).join('、');
}

function StatusPill({ dep }: { dep: EnvDepStatus }) {
  const ok = dep.status === 'installed';
  const text = ok
    ? (dep.managed ? t('envDeps.status.managedInstalled') : t('envDeps.status.installed'))
    : t('envDeps.status.missing');
  return (
    <span className={`${styles['envdeps-pill']} ${ok ? styles['envdeps-pill-ok'] : styles['envdeps-pill-missing']}`}>
      {text}
    </span>
  );
}

function DepRow({ dep, warn }: { dep: EnvDepStatus; warn?: boolean }) {
  const hintParts: string[] = [];
  if (dep.status === 'installed') {
    hintParts.push([dep.version, dep.path].filter(Boolean).join(' · '));
  } else if (dep.installHint) {
    hintParts.push(t('envDeps.installHint', { hint: dep.installHint }));
  }
  const req = requiredByLabels(dep);
  if (req) hintParts.push(t('envDeps.requiredByLabel', { features: req }));
  return (
    <SettingsRow
      label={<span>{dep.label}{dep.neededByProject && dep.status === 'missing' ? <span className={styles['envdeps-needed']}>{t('envDeps.neededByProject')}</span> : null}</span>}
      hint={hintParts.filter(Boolean).join(' · ') || undefined}
      hintVariant={warn ? 'warn' : 'default'}
      control={<StatusPill dep={dep} />}
    />
  );
}

export function EnvDepsTab() {
  const settingsAgentId = useSettingsStore(s => s.settingsAgentId);
  const [report, setReport] = useState<EnvDepsReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async (refresh: boolean) => {
    setLoading(true);
    setError(null);
    try {
      const qs = new URLSearchParams();
      if (settingsAgentId) qs.set('agentId', settingsAgentId);
      const url = refresh
        ? `/api/system/env-deps/refresh?${qs.toString()}`
        : `/api/system/env-deps?${qs.toString()}`;
      const res = refresh
        ? await lingxiFetch(url, { method: 'POST' })
        : await lingxiFetch(url);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      setReport(await res.json());
    } catch (err: any) {
      setError(String(err?.message || err));
    } finally {
      setLoading(false);
    }
  }, [settingsAgentId]);

  useEffect(() => { void load(false); }, [load]);

  const projectMissing = (report?.deps ?? []).filter(d => d.neededByProject && d.status === 'missing');
  const installed = (report?.deps ?? []).filter(d => d.status === 'installed');
  const missingOptional = (report?.deps ?? []).filter(d => d.status === 'missing' && !d.neededByProject);

  return (
    <div className={`${styles['settings-tab-content']} ${styles['active']}`} data-tab="envdeps">
      <SettingsSection
        title={t('envDeps.title')}
        description={report ? t('envDeps.lastChecked', { time: new Date(report.checkedAt).toLocaleString() }) : undefined}
        context={
          <button type="button" className={styles['envdeps-refresh']} disabled={loading} onClick={() => void load(true)}>
            {loading ? t('envDeps.checking') : t('envDeps.refresh')}
          </button>
        }
      >
        {error ? <SettingsRow label={t('envDeps.loadFailed')} hint={error} hintVariant="warn" control={<span />} /> : null}
        {!report && !error ? <SettingsRow label={t('envDeps.checking')} control={<span />} /> : null}

        {projectMissing.length > 0 ? (
          <>
            <SettingsRow label={t('envDeps.sectionProjectMissing')} hint={t('envDeps.sectionProjectMissingHint')} hintVariant="warn" control={<span />} />
            {projectMissing.map(dep => <DepRow key={dep.id} dep={dep} warn />)}
          </>
        ) : null}

        {installed.length > 0 ? (
          <>
            <SettingsRow label={t('envDeps.sectionInstalled')} control={<span />} />
            {installed.map(dep => <DepRow key={dep.id} dep={dep} />)}
          </>
        ) : null}

        {missingOptional.length > 0 ? (
          <>
            <SettingsRow label={t('envDeps.sectionMissing')} control={<span />} />
            {missingOptional.map(dep => <DepRow key={dep.id} dep={dep} />)}
          </>
        ) : null}
      </SettingsSection>
    </div>
  );
}
