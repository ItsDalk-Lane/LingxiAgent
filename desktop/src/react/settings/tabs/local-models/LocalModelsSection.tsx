import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { lingxiFetch } from '../../api';
import { t } from '../../helpers';
import { SettingsRow } from '../../components/SettingsRow';
import { Toggle } from '@/ui';
import sharedStyles from '../../Settings.module.css';
import styles from './LocalModelsSection.module.css';

type Category = 'embedding' | 'ocr' | 'stt' | 'tts';
type Variant = { quant: string; tier: 'small' | 'large'; estimatedPeakRssMb: number | null; default?: boolean };
type CatalogEntry = { id: string; category: Category; displayName: string; runtimeId: string; license: string; distributionStatus: string; variants: Variant[] };
type Installed = { id: string; category: Category; quant: string; version: string; tier: string; bytes: number; integrity: string; licenseAvailable?: boolean };
type Download = {
  taskId: string;
  assetId: string;
  status: string;
  downloadedBytes: number;
  totalBytes: number;
  bytesPerSecond: number;
  remainingMs: number | null;
  error?: string | null;
};
type ManualCandidate = { id: string; category: Category; displayName: string; quant: string; tier: string; runtimeReady: boolean };
type LocalState = {
  config: any;
  manifest: { configured: boolean; warning?: string | null; version?: string | null };
  catalog: CatalogEntry[];
  installed: Installed[];
  downloads: Download[];
  instances: Array<{ key: string; phase: string; tier: string; refs: number; backend?: string; protocolId?: string }>;
  rejected?: Array<{ name: string; reason: string }>;
  resources?: {
    memoryBudgetSmallMb: number;
    reservations: Array<{ key: string; tier: string; reservedMb: number }>;
    largeSlot: { activeKey: string | null; queue: Array<{ key: string }> };
  };
};

const CATEGORY_KEYS: Record<Category, string> = {
  embedding: 'settings.localModels.category.embedding',
  ocr: 'settings.localModels.category.ocr',
  stt: 'settings.localModels.category.stt',
  tts: 'settings.localModels.category.tts',
};

const selectClass = [sharedStyles['settings-input'], styles.select].join(' ');
const inputClass = [sharedStyles['settings-input'], styles.input].join(' ');

function SelectControl({ wrapClassName, children, ...rest }: React.SelectHTMLAttributes<HTMLSelectElement> & { wrapClassName?: string }) {
  return (
    <span className={[styles.selectWrap, wrapClassName].filter(Boolean).join(' ')}>
      <select className={selectClass} {...rest}>{children}</select>
      <svg className={styles.selectChevron} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M4 6l4 4 4-4" />
      </svg>
    </span>
  );
}

export function LocalModelsSection() {
  const [state, setState] = useState<LocalState | null>(null);
  const [busy, setBusy] = useState('');
  const [error, setError] = useState('');
  const [selectedQuant, setSelectedQuant] = useState<Record<string, string>>({});
  const [dragging, setDragging] = useState(false);
  const [pendingImport, setPendingImport] = useState<{ directory: string; candidates: ManualCandidate[]; selected: string } | null>(null);
  const [license, setLicense] = useState<{ title: string; content: string } | null>(null);

  const load = useCallback(async () => {
    try {
      const response = await lingxiFetch('/api/local-models');
      const next = await response.json() as LocalState;
      if (!Array.isArray(next?.catalog)) throw new Error('invalid local models state');
      setState(next);
      setSelectedQuant(current => Object.fromEntries(next.catalog.map(entry => [
        entry.id,
        current[entry.id] || entry.variants.find(variant => variant.default)?.quant || entry.variants[0]?.quant || '',
      ])));
      setError('');
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const shouldPoll = busy.startsWith('install:') || Boolean(state?.downloads.some(task =>
    task.status === 'queued' || task.status === 'downloading' || task.status === 'verifying'));
  useEffect(() => {
    if (!shouldPoll) return;
    const timer = window.setInterval(() => { void load(); }, 500);
    return () => window.clearInterval(timer);
  }, [load, shouldPoll]);

  const mutate = useCallback(async (key: string, path: string, init: RequestInit = {}) => {
    setBusy(key);
    try {
      await lingxiFetch(path, init);
      await load();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy('');
    }
  }, [load]);

  const saveConfig = useCallback(async (patch: any) => {
    if (!state) return;
    const next = { ...state.config, ...patch };
    await mutate('config', '/api/local-models/config', {
      method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(next),
    });
  }, [mutate, state]);

  const installModel = useCallback((modelId: string, quant: string) => {
    const key = `install:${modelId}`;
    setBusy(key);
    setError('');
    void lingxiFetch('/api/local-models/install', {
      method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ modelId, quant }),
    }).then(() => load()).catch((cause) => {
      if ((cause as { code?: string })?.code !== 'LOCAL_MODEL_ABORTED') {
        setError(cause instanceof Error ? cause.message : String(cause));
      }
    }).finally(() => setBusy(current => current === key ? '' : current));
  }, [load]);

  const installedIdentities = useMemo(() => (state?.installed || []).map(model => ({
    ...model,
    identity: `local:${model.id}@${model.quant}@${model.version}`,
  })), [state?.installed]);

  const importPath = useCallback(async (directory: string) => {
    if (!directory) return;
    setBusy('inspect-import');
    setError('');
    try {
      const response = await lingxiFetch('/api/local-models/import/inspect', {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ directory }),
      });
      const inspected = await response.json() as { hasModelJson: boolean; candidates?: ManualCandidate[] };
      if (inspected.hasModelJson) {
        await mutate('import', '/api/local-models/import', {
          method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ directory }),
        });
        return;
      }
      const candidates = inspected.candidates || [];
      const first = candidates.find(candidate => candidate.runtimeReady) || candidates[0];
      setPendingImport({ directory, candidates, selected: first ? `${first.id}\0${first.quant}` : '' });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(current => current === 'inspect-import' ? '' : current);
    }
  }, [mutate]);

  if (!state) return <div className={styles.root}>{error || t('settings.localModels.loading')}</div>;

  const secondary = sharedStyles['settings-btn-secondary'];
  const primary = sharedStyles['settings-btn-primary'];

  return (
    <div className={styles.root}>
      <div className={styles.toolbar}>
        <button className={secondary} type="button" onClick={() => void mutate('refresh', '/api/local-models/manifest/refresh', { method: 'POST' })} disabled={Boolean(busy)}>
          {t('settings.localModels.refresh')}
        </button>
        <button className={secondary} type="button" onClick={async () => {
          const directory = await window.platform?.selectFolder?.();
          if (!directory) return;
          await importPath(directory);
        }} disabled={Boolean(busy)}>{t('settings.localModels.import')}</button>
        <button className={secondary} type="button" onClick={() => window.platform?.openLocalModelsFolder?.()}>
          {t('settings.localModels.openDirectory')}
        </button>
        <span className={styles.toolbarCount}>{t('settings.localModels.installedCount', { count: state.installed.length })}</span>
      </div>
      {state.manifest.warning && <div className={styles.notice}>{state.manifest.warning}</div>}
      {error && <div className={styles.error}>{error}</div>}
      {pendingImport && <div className={styles.dialog} role="dialog" aria-label={t('settings.localModels.manualImportTitle')}>
        <strong className={styles.dialogTitle}>{t('settings.localModels.manualImportTitle')}</strong>
        <div className={styles.dialogHint}>{t('settings.localModels.manualImportHint')}</div>
        <SelectControl wrapClassName={styles.selectFull} aria-label={t('settings.localModels.manualImportChoice')} value={pendingImport.selected} onChange={event => setPendingImport(current => current ? { ...current, selected: event.target.value } : null)}>
          {pendingImport.candidates.map(candidate => <option key={`${candidate.id}:${candidate.quant}`} value={`${candidate.id}\0${candidate.quant}`} disabled={!candidate.runtimeReady}>
            {candidate.displayName} · {candidate.category} · {candidate.quant}{candidate.runtimeReady ? '' : ` · ${t('settings.localModels.runtimeMissing')}`}
          </option>)}
        </SelectControl>
        <div className={styles.dialogActions}>
          <button className={secondary} type="button" onClick={() => setPendingImport(null)}>{t('settings.localModels.cancelImport')}</button>
          <button className={primary} type="button" disabled={!pendingImport.selected || !pendingImport.candidates.some(candidate => `${candidate.id}\0${candidate.quant}` === pendingImport.selected && candidate.runtimeReady)} onClick={() => {
            const [modelId, quant] = pendingImport.selected.split('\0');
            const directory = pendingImport.directory;
            setPendingImport(null);
            void mutate('import', '/api/local-models/import', {
              method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ directory, modelId, quant }),
            });
          }}>{t('settings.localModels.confirmImport')}</button>
        </div>
      </div>}
      {license && <div className={styles.dialog} role="dialog" aria-label={t('settings.localModels.licenseTitle', { model: license.title })}>
        <strong className={styles.dialogTitle}>{t('settings.localModels.licenseTitle', { model: license.title })}</strong>
        <pre className={styles.licenseText}>{license.content}</pre>
        <div className={styles.dialogActions}>
          <button className={secondary} type="button" onClick={() => setLicense(null)}>{t('settings.localModels.closeLicense')}</button>
        </div>
      </div>}

      <div className={styles.config}>
        <div className={styles.groupTitle}>{t('settings.localModels.groupRuntime')}</div>
        <SettingsRow
          label={t('settings.localModels.backend')}
          control={<SelectControl
            wrapClassName={styles.selectBackend}
            aria-label={t('settings.localModels.backend')}
            value={state.config.backend}
            onChange={event => void saveConfig({ backend: event.target.value })}
            disabled={Boolean(busy)}
          >
            {['auto', 'cpu', 'coreml', 'metal', 'cuda', 'vulkan', 'directml'].map(value => <option key={value} value={value}>{value}</option>)}
          </SelectControl>}
        />
        <SettingsRow
          label={t('settings.localModels.threads')}
          control={<input
            className={inputClass}
            type="text"
            aria-label={t('settings.localModels.threads')}
            value={state.config.threads}
            onChange={event => void saveConfig({ threads: event.target.value === 'auto' ? 'auto' : Number(event.target.value) })}
            disabled={Boolean(busy)}
          />}
        />

        <div className={styles.groupTitle}>{t('settings.localModels.groupMemory')}</div>
        <SettingsRow
          label={t('settings.localModels.smallBudget')}
          control={<input
            className={inputClass}
            type="number"
            min={128}
            aria-label={t('settings.localModels.smallBudget')}
            value={state.config.memoryBudgetSmallMb}
            onChange={event => void saveConfig({ memoryBudgetSmallMb: Number(event.target.value) })}
            disabled={Boolean(busy)}
          />}
        />
        <SettingsRow
          label={t('settings.localModels.smallIdleSeconds')}
          control={<input
            className={inputClass}
            type="number"
            min={0}
            aria-label={t('settings.localModels.smallIdleSeconds')}
            value={Math.round(state.config.idleUnloadMs.small / 1000)}
            onChange={event => void saveConfig({ idleUnloadMs: { ...state.config.idleUnloadMs, small: Number(event.target.value) * 1000 } })}
            disabled={Boolean(busy)}
          />}
        />
        <SettingsRow
          label={t('settings.localModels.largeIdleSeconds')}
          control={<input
            className={inputClass}
            type="number"
            min={0}
            aria-label={t('settings.localModels.largeIdleSeconds')}
            value={Math.round(state.config.idleUnloadMs.large / 1000)}
            onChange={event => void saveConfig({ idleUnloadMs: { ...state.config.idleUnloadMs, large: Number(event.target.value) * 1000 } })}
            disabled={Boolean(busy)}
          />}
        />
        <SettingsRow
          label={t('settings.localModels.useMmap')}
          control={<Toggle ariaLabel={t('settings.localModels.useMmap')} on={state.config.useMmap === true} onChange={checked => void saveConfig({ useMmap: checked })} disabled={Boolean(busy)} />}
        />
        <SettingsRow
          label={t('settings.localModels.mlock')}
          control={<Toggle ariaLabel={t('settings.localModels.mlock')} on={state.config.mlock === true} onChange={checked => void saveConfig({ mlock: checked })} disabled={Boolean(busy)} />}
        />

        <div className={styles.groupTitle}>{t('settings.localModels.groupDefaults')}</div>
        <SettingsRow
          label={t('settings.localModels.defaultOcr')}
          control={<SelectControl
            wrapClassName={styles.selectWide}
            aria-label={t('settings.localModels.defaultOcr')}
            value={state.config.ocr?.defaultModel || ''}
            onChange={event => void saveConfig({ ocr: { ...state.config.ocr, defaultModel: event.target.value } })}
            disabled={Boolean(busy)}
          >
            <option value="">{t('settings.localModels.none')}</option>
            {installedIdentities.filter(model => model.category === 'ocr').map(model => <option key={model.identity} value={model.identity}>{model.id} · {model.quant}</option>)}
          </SelectControl>}
        />
        <SettingsRow
          label={t('settings.localModels.defaultTts')}
          control={<SelectControl
            wrapClassName={styles.selectWide}
            aria-label={t('settings.localModels.defaultTts')}
            value={state.config.tts?.defaultModel || ''}
            onChange={event => void saveConfig({ tts: { ...state.config.tts, defaultModel: event.target.value } })}
            disabled={Boolean(busy)}
          >
            <option value="">{t('settings.localModels.none')}</option>
            {installedIdentities.filter(model => model.category === 'tts').map(model => <option key={model.identity} value={model.identity}>{model.id} · {model.quant}</option>)}
          </SelectControl>}
        />
        <SettingsRow
          label={t('settings.localModels.preloadSmall')}
          control={<Toggle ariaLabel={t('settings.localModels.preloadSmall')} on={state.config.preloadSmall === true} onChange={checked => void saveConfig({ preloadSmall: checked })} disabled={Boolean(busy)} />}
        />

        <div className={styles.groupTitle}>{t('settings.localModels.groupDownload')}</div>
        <SettingsRow
          label={t('settings.localModels.bridgeReply')}
          control={<Toggle ariaLabel={t('settings.localModels.bridgeReply')} on={state.config.tts?.bridgeReply === true} onChange={checked => void saveConfig({ tts: { ...state.config.tts, bridgeReply: checked } })} disabled={Boolean(busy)} />}
        />
        <SettingsRow
          label={t('settings.localModels.mirror')}
          control={<input
            key={state.config.download?.mirrorBaseUrl || 'default'}
            className={[inputClass, styles.inputWide].join(' ')}
            type="url"
            aria-label={t('settings.localModels.mirror')}
            defaultValue={state.config.download?.mirrorBaseUrl || ''}
            placeholder="https://"
            onBlur={event => void saveConfig({ download: { ...state.config.download, mirrorBaseUrl: event.target.value } })}
            disabled={Boolean(busy)}
          />}
        />
      </div>

      <div
        className={`${styles.dropZone} ${dragging ? styles.dropZoneActive : ''}`}
        onDragOver={event => { event.preventDefault(); setDragging(true); }}
        onDragLeave={() => setDragging(false)}
        onDrop={event => {
          event.preventDefault();
          setDragging(false);
          const file = event.dataTransfer.files[0];
          const directory = file && (window.platform?.getFilePath?.(file) || (file as File & { path?: string }).path);
          if (directory) void importPath(directory);
          else setError(t('settings.localModels.dropPathUnavailable'));
        }}
      >{t('settings.localModels.dropImport')}</div>

      <div className={styles.statGrid}>
        <div className={styles.statTile}>{t('settings.localModels.memoryStatus', {
          used: state.resources?.reservations.reduce((sum, entry) => sum + entry.reservedMb, 0) || 0,
          budget: state.resources?.memoryBudgetSmallMb || state.config.memoryBudgetSmallMb,
        })}</div>
        <div className={styles.statTile}>{state.resources?.largeSlot.activeKey
          ? t('settings.localModels.largeSlotActive', { model: state.resources.largeSlot.activeKey, queued: state.resources.largeSlot.queue.length })
          : t('settings.localModels.largeSlotIdle', { queued: state.resources?.largeSlot.queue.length || 0 })}</div>
      </div>
      {state.instances.map(instance => <div className={styles.metaLine} key={instance.key}>
        {instance.key} · {instance.phase} · {instance.backend || state.config.backend} · {instance.refs}
      </div>)}
      {(state.rejected || []).map(item => <div className={styles.errorLine} key={`${item.name}:${item.reason}`}>
        {t('settings.localModels.rejectedImport', { name: item.name, reason: item.reason })}
      </div>)}
      <div className={styles.noteLine}>{t('settings.localModels.vectorRetention')}</div>

      {state.downloads.some(task => task.bytesPerSecond > 0) && <div className={styles.noteLine}>
        {t('settings.localModels.globalSpeed')}: {formatRate(state.downloads.reduce((sum, task) => sum + task.bytesPerSecond, 0))}
      </div>}
      {state.downloads.map(task => (
        <div key={task.taskId} className={styles.download}>
          <div className={styles.downloadHead}>
            <span className={styles.downloadName}>{task.assetId}</span>
            <span className={styles.status}>{task.status}</span>
            <span className={styles.downloadActions}>
              {(task.status === 'queued' || task.status === 'downloading') && <button className={secondary} type="button" disabled={busy.startsWith('pause:') || busy.startsWith('cancel:')} onClick={() => void mutate(`pause:${task.taskId}`, `/api/local-models/downloads/${task.taskId}/pause`, { method: 'POST' })}>{t('settings.localModels.pause')}</button>}
              {task.status !== 'completed' && <button className={secondary} type="button" disabled={busy.startsWith('pause:') || busy.startsWith('cancel:')} onClick={() => void mutate(`cancel:${task.taskId}`, `/api/local-models/downloads/${task.taskId}`, { method: 'DELETE' })}>{t('settings.localModels.cancel')}</button>}
            </span>
          </div>
          <progress className={styles.progress} max={task.totalBytes || 1} value={task.downloadedBytes} />
          <div className={styles.downloadMeta}>
            <span>{formatBytes(task.downloadedBytes)} / {formatBytes(task.totalBytes)}</span>
            {task.bytesPerSecond > 0 && <span>{formatRate(task.bytesPerSecond)}</span>}
            {task.remainingMs !== null && <span>{t('settings.localModels.remaining', { time: formatDuration(task.remainingMs) })}</span>}
          </div>
          {task.error && <div className={styles.error}>{task.error}</div>}
        </div>
      ))}

      <div className={styles.groups}>
        {(Object.keys(CATEGORY_KEYS) as Category[]).map(category => (
          <div className={styles.group} key={category}>
            <h3 className={styles.groupTitle}>{t(CATEGORY_KEYS[category])}</h3>
            {state.catalog.filter(entry => entry.category === category).map(entry => {
              const quant = selectedQuant[entry.id] || '';
              const installed = state.installed.find(model => model.id === entry.id && model.quant === quant);
              const variant = entry.variants.find(item => item.quant === quant);
              const task = state.downloads.find(item => item.assetId === `model-${entry.id}@${quant}`
                || item.assetId.startsWith(`runtime-${entry.runtimeId}@`));
              return <div className={styles.model} key={entry.id}>
                <div className={styles.modelInfo}>
                  <div className={styles.modelName}>{entry.displayName}</div>
                  <div className={styles.modelMeta}>
                    <span>{entry.license}</span>
                    ·
                    <span className={[styles.status, installed ? styles.statusReady : ''].filter(Boolean).join(' ')}>{installed ? t('settings.localModels.ready') : task ? task.status : t('settings.localModels.notInstalled')}</span>
                    {variant ? ` · ${variant.tier}` : ''}
                  </div>
                  {installed && <div className={styles.modelMeta}>{formatBytes(installed.bytes)} · {installed.integrity}</div>}
                </div>
                <div className={styles.modelActions}>
                  <SelectControl
                    wrapClassName={styles.selectQuant}
                    aria-label={`${entry.displayName} ${t('settings.localModels.quant')}`}
                    value={quant}
                    onChange={event => setSelectedQuant(current => ({ ...current, [entry.id]: event.target.value }))}
                  >
                    {entry.variants.map(item => <option key={item.quant} value={item.quant}>{item.quant}{item.estimatedPeakRssMb ? ` · ${item.estimatedPeakRssMb} MB` : ''}</option>)}
                  </SelectControl>
                  {installed ? <button className={secondary} type="button" disabled={Boolean(busy)} onClick={() => {
                    if (!window.confirm(t('settings.localModels.deleteConfirm', { size: formatBytes(installed.bytes) }))) return;
                    void mutate(`remove:${entry.id}`, `/api/local-models/models/${category}/${encodeURIComponent(entry.id)}/${encodeURIComponent(quant)}`, { method: 'DELETE' });
                  }}>{t('settings.localModels.delete')}</button> : <button className={primary} type="button" disabled={Boolean(busy) || entry.distributionStatus !== 'manifest-available' || Boolean(task && ['queued', 'downloading', 'verifying'].includes(task.status))} onClick={() => installModel(entry.id, quant)}>{task && ['paused', 'interrupted', 'failed'].includes(task.status) ? t('settings.localModels.resume') : t('settings.localModels.download')}</button>}
                  {installed && <button className={secondary} type="button" disabled={!installed.licenseAvailable || Boolean(busy)} onClick={async () => {
                    setBusy(`license:${entry.id}`);
                    try {
                      const response = await lingxiFetch(`/api/local-models/models/${category}/${encodeURIComponent(entry.id)}/${encodeURIComponent(quant)}/license`);
                      const body = await response.json() as { content?: string };
                      if (typeof body.content !== 'string') throw new Error('invalid license response');
                      setLicense({ title: entry.displayName, content: body.content });
                    } catch (cause) {
                      setError(cause instanceof Error ? cause.message : String(cause));
                    } finally {
                      setBusy('');
                    }
                  }}>{installed.licenseAvailable ? t('settings.localModels.viewLicense') : t('settings.localModels.licenseUnavailable')}</button>}
                </div>
              </div>;
            })}
          </div>
        ))}
      </div>
    </div>
  );
}

function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 MB';
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  return `${(bytes / (1024 * 1024)).toFixed(0)} MB`;
}

function formatRate(bytesPerSecond: number) {
  if (bytesPerSecond >= 1024 * 1024) return `${(bytesPerSecond / (1024 * 1024)).toFixed(1)} MB/s`;
  return `${Math.max(0, bytesPerSecond / 1024).toFixed(0)} KB/s`;
}

function formatDuration(milliseconds: number) {
  const seconds = Math.max(0, Math.ceil(milliseconds / 1000));
  if (seconds < 60) return `${seconds}s`;
  return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}
