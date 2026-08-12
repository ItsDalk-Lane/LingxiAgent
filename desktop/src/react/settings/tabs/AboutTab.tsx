import React, { useState, useCallback, useEffect } from 'react';
import { useSettingsStore } from '../store';
import { autoSaveConfig, t } from '../helpers';
import { Toggle } from '@/ui';
import { loadSettingsConfig } from '../actions';
import { loadUpdateDigestHistory } from '../update-history-actions';
import { readConfigBoolean } from '../resource-state';
import { SettingsSection } from '../components/SettingsSection';
import { SettingsRow } from '../components/SettingsRow';
import { SettingsStack } from '../components/SettingsPrimitives';
import { ExpandableRow } from '../components/ExpandableRow';
import { digestLocale, digestText, kindLabel } from '../../components/shared/release-digest-text';
import { useAutoUpdateState } from '../../hooks/use-auto-update-state';
import { useReleaseCheck } from '../../hooks/use-release-check';
import { ConfirmDialog, Overlay } from '../../ui';
import type { AutoUpdateState, InviteChannelStatus, UpdateDigestHistoryResult } from '../../types';
import appIconUrl from '../../../icon.png';
import styles from '../Settings.module.css';
import updateStyles from '../../components/AutoUpdateStatus.module.css';

const EMPTY_HISTORY: UpdateDigestHistoryResult = { entries: [], source: 'none', complete: false };

// 上游项目：本仓库（LingxiAgent）由 openhanako 改名/重构而来，版本线同步跟踪上游。
// UPSTREAM_VERSION 是构建期从 package.json 的 lingxi.upstreamVersion 注入的（见
// vite.config.ts 的 define），单一真相源在 package.json——同步上游代码时改那一处即可，
// 这里自动跟随。一致性由 tests/upstream-version-consistency.test.ts 钉死。
const UPSTREAM_REPO_URL = 'https://github.com/liliMozi/openhanako';
const UPSTREAM_VERSION = String(import.meta.env.LINGXI_UPSTREAM_VERSION ?? '0.0.0-unknown');
// 「下载最新版本」的回退目标：release API 没带回 html_url 时（理论上不会），
// 至少把用户带到 releases/latest 总入口，自己挑平台安装包。owner/repo 与
// package.json electron-builder publish 配置、github-release-check.cjs 一致。
const RELEASES_LATEST_PAGE_URL = 'https://github.com/ItsDalk-Lane/LingxiAgent/releases/latest';

function UpdateHistoryDialog({
  open,
  loading,
  history,
  onClose,
}: {
  open: boolean;
  loading: boolean;
  history: UpdateDigestHistoryResult;
  onClose: () => void;
}) {
  const locale = digestLocale();
  const showNotice = !loading
    && history.entries.length > 0
    && (history.source !== 'online' || !history.complete);
  const noticeKey = history.source === 'bundled'
    ? 'settings.about.updateHistoryOffline'
    : history.source === 'online'
      ? 'settings.about.updateHistoryPartial'
      : 'settings.about.updateHistoryUnavailable';

  return (
    <Overlay
      scope="inline"
      open={open}
      onClose={onClose}
      backdrop="blur"
      zIndex={100}
      className={`${styles['memory-viewer']} ${styles['update-history-viewer']}`}
      backdropClassName={styles['memory-viewer-backdrop']}
      disableContainerAnimation
      contentProps={{
        role: 'dialog',
        'aria-modal': 'true',
        'aria-labelledby': 'update-history-dialog-title',
      }}
    >
      <div className={styles['memory-viewer-header']}>
        <div>
          <h3 id="update-history-dialog-title" className={styles['memory-viewer-title']}>
            {t('settings.about.updateHistoryTitle')}
          </h3>
          <div className={styles['update-history-subtitle']}>
            {t('settings.about.updateHistorySubtitle')}
          </div>
        </div>
        <button
          type="button"
          className={styles['memory-viewer-close']}
          aria-label={t('settings.about.updateDigestClose')}
          onClick={onClose}
        >
          ✕
        </button>
      </div>
      <div className={`${styles['memory-viewer-body']} ${styles['update-history-body']}`}>
        {loading && (
          <div className={styles['update-history-state']}>{t('settings.about.updateHistoryLoading')}</div>
        )}
        {showNotice && (
          <div className={styles['update-history-notice']}>{t(noticeKey)}</div>
        )}
        {!loading && history.entries.length === 0 && (
          <div className={styles['update-history-state']}>{t('settings.about.updateHistoryUnavailable')}</div>
        )}
        {!loading && history.entries.map((digest) => (
          <article key={digest.version} className={styles['update-history-release']}>
            <header className={styles['update-history-release-header']}>
              <h4 className={styles['update-history-version']}>v{digest.version}</h4>
            </header>
            <p className={styles['update-history-summary']}>{digestText(digest.summary, locale)}</p>
            {digest.items.length > 0 && (
              <div className={styles['update-history-items']}>
                {digest.items.map((item) => (
                  <section
                    key={`${digest.version}-${item.id || item.kind}-${item.title.en}`}
                    className={styles['update-history-item']}
                  >
                    <div className={styles['update-history-item-heading']}>
                      <span className={styles['update-history-kind']}>{kindLabel(item.kind)}</span>
                      <h5 className={styles['update-history-item-title']}>{digestText(item.title, locale)}</h5>
                    </div>
                    <p className={styles['update-history-item-summary']}>{digestText(item.summary, locale)}</p>
                  </section>
                ))}
              </div>
            )}
          </article>
        ))}
      </div>
    </Overlay>
  );
}

function formatCheckedAt(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString();
}

/**
 * 更新区状态机：GitHub Release 检测 + electron-updater 自动下载安装。
 *
 * 两层优先级：
 *   1. shellUpdate 活跃时（checking/downloading/downloaded/error）优先展示
 *      自动下载安装进度，用户在应用内完成更新，无需打开浏览器。
 *   2. shellUpdate 不活跃时回退到 release check 结果：available 给「立即更新」
 *      按钮触发 autoUpdateCheck；latest/error 各自展示对应文案。
 *
 * Fallback：auto-updater 出错（如网络异常、签名不匹配）或 macOS 从 DMG 运行
 * 时，自动下载不可用，回退为浏览器手动下载。
 */
function ReleaseUpdateArea({
  status,
  latestVersion,
  error,
  lastCheckedAt,
  shellUpdate,
  onStartUpdate,
  onInstallShell,
  onFallbackDownload,
  onRetry,
}: {
  status: 'idle' | 'checking' | 'latest' | 'available' | 'error';
  latestVersion?: string;
  error?: string;
  lastCheckedAt: string | null;
  shellUpdate: AutoUpdateState | null;
  onStartUpdate: () => void;
  onInstallShell: () => void;
  onFallbackDownload: () => void;
  onRetry: () => void;
}) {
  // ── shell update 活跃态优先 ──

  // 下载中：进度条
  if (shellUpdate?.status === 'downloading') {
    const percent = shellUpdate.progress?.percent ?? 0;
    return (
      <div className={updateStyles.root}>
        <div className={updateStyles.column}>
          <div className={updateStyles.downloadHeader}>
            <span className={updateStyles.message}>
              {t('settings.about.updateDownloadingVersion', { version: shellUpdate.version || latestVersion })}
            </span>
            <span className={updateStyles.progressValue}>{percent}%</span>
          </div>
          <progress className={updateStyles.nativeProgress} value={percent} max={100} />
        </div>
      </div>
    );
  }

  // 已下载就绪：重启安装
  if (shellUpdate?.status === 'downloaded') {
    return (
      <div className={updateStyles.root}>
        <div className={updateStyles.row}>
          <span className={updateStyles.message}>
            {t('settings.about.updateReady', { version: shellUpdate.version || latestVersion })}
          </span>
          <button type="button" className={updateStyles.action} onClick={onInstallShell}>
            {t('settings.about.updateInstall')}
          </button>
        </div>
      </div>
    );
  }

  // auto-updater 正在检查或准备下载
  if (shellUpdate?.status === 'checking' || shellUpdate?.status === 'available') {
    return (
      <div className={updateStyles.root}>
        <div className={updateStyles.row}>
          <span className={updateStyles.message}>{t('settings.about.updatePreparing')}</span>
        </div>
      </div>
    );
  }

  // auto-updater 出错（非 DMG 场景）：提供浏览器手动下载 fallback
  if (shellUpdate?.status === 'error' && shellUpdate.error !== 'running_from_dmg') {
    return (
      <div className={updateStyles.root}>
        <div className={updateStyles.row}>
          <span className={`${updateStyles.message} ${updateStyles.error}`}>
            {t('settings.about.updateAutoFailed')}
          </span>
          <button type="button" className={updateStyles.action} onClick={onFallbackDownload}>
            {t('settings.about.updateDownloadManual')}
          </button>
        </div>
      </div>
    );
  }

  // ── release check 状态（shell update 未活跃时）──

  if (status === 'checking') {
    return (
      <div className={updateStyles.root}>
        <div className={updateStyles.row}>
          <span className={updateStyles.message}>{t('settings.about.updateChecking')}</span>
        </div>
      </div>
    );
  }

  if (status === 'available') {
    // macOS 从 DMG 直接运行时 auto-updater 不可用，直接给浏览器下载
    if (shellUpdate?.error === 'running_from_dmg') {
      return (
        <div className={updateStyles.root}>
          <div className={updateStyles.row}>
            <span className={updateStyles.message}>
              {t('settings.about.updateAvailableGithub', { version: latestVersion })}
            </span>
            <button type="button" className={updateStyles.action} onClick={onFallbackDownload}>
              {t('settings.about.updateDownloadManual')}
            </button>
          </div>
          <div className={updateStyles.row}>
            <span className={updateStyles.message}>{t('settings.about.updateDmgHint')}</span>
          </div>
        </div>
      );
    }
    return (
      <div className={updateStyles.root}>
        <div className={updateStyles.row}>
          <span className={updateStyles.message}>
            {t('settings.about.updateAvailableGithub', { version: latestVersion })}
          </span>
          <button type="button" className={updateStyles.action} onClick={onStartUpdate}>
            {t('settings.about.updateNow')}
          </button>
        </div>
        <div className={updateStyles.row}>
          <span className={updateStyles.message}>{t('settings.about.updateAutoHint')}</span>
        </div>
      </div>
    );
  }

  if (status === 'error') {
    return (
      <div className={updateStyles.root}>
        <div className={updateStyles.row}>
          <span className={`${updateStyles.message} ${updateStyles.error}`}>{t('settings.about.updateError')}</span>
          {error && <span className={updateStyles.errorDetail} title={error}>{error}</span>}
          <button type="button" className={updateStyles.action} onClick={onRetry}>
            {t('settings.about.updateRetryBtn')}
          </button>
        </div>
      </div>
    );
  }

  if (status === 'latest' && lastCheckedAt) {
    return (
      <div className={updateStyles.root}>
        <div className={updateStyles.row}>
          <span className={updateStyles.message}>
            {t('settings.about.updateLatestCheckedAt', { time: formatCheckedAt(lastCheckedAt) })}
          </span>
        </div>
      </div>
    );
  }

  // idle（尚未检查过）或 latest 但没时间戳：不渲染结论文案，只留"检查更新"按钮。
  return null;
}

/**
 * 邀请制测试通道。三条纪律：
 *  1. 核销服务没配置（configured=false）就整块不渲染——正式构建在服务上线前
 *     看不到任何入口，而不是给出一个点了会报错的按钮。
 *  2. 核销成功只是拿到一个地址，绝不顺手落盘；写通道状态必须先过确认对话框。
 *  3. 失败文案只分两类：码本身不认（无效/用完）与够不着服务（网络/服务端），
 *     服务端原话原样附在下面，不美化、不重试。
 */
function InviteChannelSection() {
  const hana = window.hana;
  const [status, setStatus] = useState<InviteChannelStatus | null>(null);
  const [code, setCode] = useState('');
  const [redeeming, setRedeeming] = useState(false);
  const [errorKey, setErrorKey] = useState<string | null>(null);
  const [errorDetail, setErrorDetail] = useState<string | null>(null);
  const [pending, setPending] = useState<{ feedUrl: string; inviteCodes: string[] } | null>(null);
  const [activating, setActivating] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const next = await hana?.inviteStatus?.();
        if (!cancelled && next) setStatus(next);
      } catch (err) {
        // 状态问不出来就不提供入口：宁可不露出一个行为不明的按钮，也不猜。
        console.error('[invite] failed to read the update channel status', err);
      }
    })();
    return () => { cancelled = true; };
  }, [hana]);

  const copyInviteCode = useCallback(async (value: string) => {
    if (!value) return;
    await navigator.clipboard?.writeText(value);
  }, []);

  const handleRedeem = useCallback(async () => {
    const trimmed = code.trim();
    if (!trimmed || redeeming) return;
    setRedeeming(true);
    setErrorKey(null);
    setErrorDetail(null);
    try {
      const result = await hana?.inviteRedeem?.(trimmed);
      if (!result) {
        setErrorKey('settings.about.inviteErrorNetwork');
        return;
      }
      if (result.ok) {
        setPending({ feedUrl: result.feedUrl, inviteCodes: result.childCodes });
        return;
      }
      // 只有"码本身不认"才归到邀请码文案；够不着服务的一律说是连接问题。
      setErrorKey(result.reason === 'invalid'
        ? 'settings.about.inviteErrorInvalid'
        : 'settings.about.inviteErrorNetwork');
      setErrorDetail(result.message || null);
    } catch (err) {
      setErrorKey('settings.about.inviteErrorNetwork');
      setErrorDetail(err instanceof Error ? err.message : String(err));
    } finally {
      setRedeeming(false);
    }
  }, [code, hana, redeeming]);

  const handleConfirmActivate = useCallback(async () => {
    if (!pending || activating) return;
    setActivating(true);
    try {
      const next = await hana?.inviteActivate?.(pending);
      if (next) setStatus(next);
      setPending(null);
      setCode('');
      setErrorKey(null);
      setErrorDetail(null);
    } catch (err) {
      setPending(null);
      setErrorKey('settings.about.inviteErrorActivate');
      setErrorDetail(err instanceof Error ? err.message : String(err));
    } finally {
      setActivating(false);
    }
  }, [activating, hana, pending]);

  if (!status?.configured) return null;

  return (
    <SettingsSection title={t('settings.about.inviteSectionTitle')}>
      {status.active ? (
        <>
          <SettingsRow
            label={t('settings.about.inviteChannelActive')}
            hint={t('settings.about.inviteChannelActiveHint')}
            control={<span />}
          />
          {status.inviteCodes.length > 0 && (
            <SettingsRow
              label={t('settings.about.inviteCodesLabel')}
              hint={t('settings.about.inviteCodesHint')}
              layout="stacked"
              control={
                <SettingsStack gap="sm">
                  {status.inviteCodes.map((inviteCode) => (
                    <div key={inviteCode} className={styles['access-url-row']}>
                      <input className={styles['settings-input']} value={inviteCode} readOnly />
                      <button
                        type="button"
                        className={styles['settings-btn-secondary']}
                        onClick={() => { void copyInviteCode(inviteCode); }}
                      >
                        {t('settings.about.inviteCopy')}
                      </button>
                    </div>
                  ))}
                </SettingsStack>
              }
            />
          )}
        </>
      ) : (
        <SettingsRow
          label={t('settings.about.inviteCodeLabel')}
          hint={errorKey ? (
            <>
              <span>{t(errorKey)}</span>
              {errorDetail && <span title={errorDetail}> {errorDetail}</span>}
            </>
          ) : t('settings.about.inviteCodeHint')}
          hintVariant={errorKey ? 'warn' : 'default'}
          layout="stacked"
          control={
            <div className={styles['access-url-row']}>
              <input
                className={styles['settings-input']}
                aria-label={t('settings.about.inviteCodeLabel')}
                placeholder={t('settings.about.inviteCodePlaceholder')}
                value={code}
                disabled={redeeming}
                onChange={(event) => setCode(event.target.value)}
              />
              <button
                type="button"
                className={styles['settings-btn-primary']}
                onClick={() => { void handleRedeem(); }}
                disabled={redeeming || !code.trim()}
              >
                {redeeming ? t('settings.about.inviteRedeeming') : t('settings.about.inviteRedeemBtn')}
              </button>
            </div>
          }
        />
      )}

      <ConfirmDialog
        open={Boolean(pending)}
        scope="inline"
        title={t('settings.about.inviteConfirmTitle')}
        confirmLabel={t('settings.about.inviteConfirmOk')}
        cancelLabel={t('settings.about.inviteConfirmCancel')}
        confirmTone="danger"
        busy={activating}
        onConfirm={() => { void handleConfirmActivate(); }}
        onCancel={() => setPending(null)}
      >
        {t('settings.about.inviteConfirmBody')}
      </ConfirmDialog>
    </SettingsSection>
  );
}

export function AboutTab() {
  const hana = window.hana;
  const settingsConfig = useSettingsStore(s => s.settingsConfig);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [history, setHistory] = useState<UpdateDigestHistoryResult>(EMPTY_HISTORY);
  const shellUpdate = useAutoUpdateState();
  // Hero 版本号用 app.getVersion()（产品/壳版本，如 0.1.2），不再用列车内容
  // 版本（useTrainUpdateState 的 currentVersion，会显示成上游版本 0.442.0）。
  // getAppVersion IPC 读的就是 app.getVersion()，单一源、不歧义。
  const [appVersion, setAppVersion] = useState('');
  useEffect(() => {
    let alive = true;
    void hana?.getAppVersion?.().then((v) => {
      if (alive && v) setAppVersion(v);
    });
    return () => { alive = false; };
  }, [hana]);
  // 更新检测主源：查 GitHub Releases。OTA 签名通道（useTrainUpdateState）依赖
  // 未配置的 LINGXI_ARTIFACT_CHANNEL_BASE_URL，正式构建里从未生效，且失败会
  // 永久残留在 ota-state.json——这里改走 GitHub，开箱即用、失败不落盘。
  const {
    status: releaseStatus,
    latestVersion,
    releaseUrl,
    error: releaseError,
    lastCheckedAt: releaseLastCheckedAt,
    checkNow: checkReleaseNow,
  } = useReleaseCheck();
  const isBeta = readConfigBoolean(settingsConfig, cfg => cfg.update_channel === 'beta', false);
  // 默认 true：老用户（preferences 里没写这个字段）保持原有"自动检查"行为
  const autoCheck = readConfigBoolean(settingsConfig, cfg => cfg.auto_check_updates, true);

  const handleCheck = useCallback(() => {
    void checkReleaseNow();
  }, [checkReleaseNow]);

  // 「立即更新」：触发 electron-updater 自动下载安装。auto-updater 检测到
  // 新版本后自动下载，下载完成在 ReleaseUpdateArea 展示「重启更新」按钮。
  const handleStartUpdate = useCallback(() => {
    void hana?.autoUpdateCheck?.();
  }, [hana]);

  // Fallback：自动更新不可用时（网络异常 / macOS DMG 运行），在浏览器打开
  // release 页面让用户手动下载安装。
  const handleFallbackDownload = useCallback(() => {
    const url = releaseUrl || RELEASES_LATEST_PAGE_URL;
    void hana?.openExternal?.(url);
  }, [hana, releaseUrl]);

  const handleInstallShell = useCallback(async () => {
    await hana?.autoUpdateInstall?.();
  }, [hana]);

  const handleHistoryOpen = useCallback(async () => {
    setHistoryOpen(true);
    setHistoryLoading(true);
    try {
      setHistory(await loadUpdateDigestHistory());
    } catch {
      setHistory(EMPTY_HISTORY);
    } finally {
      setHistoryLoading(false);
    }
  }, []);

  const handleBetaToggle = useCallback(async (on: boolean) => {
    const channel = on ? 'beta' : 'stable';
    hana?.autoUpdateSetChannel?.(channel);
    await autoSaveConfig({ update_channel: channel }, { silent: true });
    await loadSettingsConfig();
  }, [hana]);

  const handleAutoCheckToggle = useCallback(async (on: boolean) => {
    await autoSaveConfig({ auto_check_updates: on }, { silent: true });
    await loadSettingsConfig();
  }, []);

  // shell update 活跃时（checking/downloading/downloaded/error）隐藏通用
  // 检查按钮——ReleaseUpdateArea 已经在展示对应状态。
  const shellActive = shellUpdate != null
    && shellUpdate.status !== 'idle'
    && shellUpdate.status !== 'latest';
  const showCheckButton = (releaseStatus === 'idle' || releaseStatus === 'latest') && !shellActive;

  return (
    <div className={`${styles['settings-tab-content']} ${styles['active']}`} data-tab="about">
      {/* Hero：版本号用 app.getVersion()（产品/壳版本，如 0.1.2），单一源、
          无歧义；上游版本 0.442.0 作为 Info 区独立信息行展示，不混进 Hero。
          更新检测走 GitHub Releases（ReleaseUpdateArea）：查到新版本给「立即
          更新」按钮触发 auto-updater 自动下载安装，不再跳浏览器。 */}
      <div className={styles['about-hero']}>
        <img className={styles['about-icon']} src={appIconUrl} alt="LingxiAgent" />
        <div className={styles['about-name']}>LingxiAgent</div>
        <div className={styles['about-tagline']}>{t('settings.about.tagline')}</div>
        {appVersion && <div className={styles['about-version']}>v{appVersion}</div>}
        <ReleaseUpdateArea
          status={releaseStatus}
          latestVersion={latestVersion}
          error={releaseError}
          lastCheckedAt={releaseLastCheckedAt}
          shellUpdate={shellUpdate}
          onStartUpdate={handleStartUpdate}
          onInstallShell={handleInstallShell}
          onFallbackDownload={handleFallbackDownload}
          onRetry={handleCheck}
        />
        <div className={styles['about-update-actions']}>
          {showCheckButton && (
            <button type="button" className={styles['about-check-update-btn']} onClick={handleCheck}>
              {t('settings.about.updateCheckBtn')}
            </button>
          )}
          <button type="button" className={styles['about-check-update-btn']} onClick={handleHistoryOpen}>
            {t('settings.about.updateHistoryTitle')}
          </button>
        </div>
      </div>

      {/* Info：标准 row（license / copyright / auto-check / beta toggle） */}
      <SettingsSection>
        <SettingsRow
          label={t('settings.about.license')}
          control={<span>Apache License 2.0</span>}
        />
        <SettingsRow
          label={t('settings.about.copyright')}
          control={<span>© 2026 Lingxi</span>}
        />
        <SettingsRow
          label={t('settings.about.upstreamProject')}
          control={
            <button
              type="button"
              className={styles['settings-btn-secondary']}
              onClick={() => { void hana?.openExternal?.(UPSTREAM_REPO_URL); }}
            >
              openhanako
            </button>
          }
        />
        <SettingsRow
          label={t('settings.about.upstreamVersion')}
          control={<span>v{UPSTREAM_VERSION}</span>}
        />
        <SettingsRow
          label={t('settings.about.autoCheckUpdates')}
          control={<Toggle on={autoCheck} onChange={handleAutoCheckToggle} />}
        />
        <SettingsRow
          label={t('settings.about.betaUpdates')}
          control={<Toggle on={isBeta} onChange={handleBetaToggle} />}
        />
      </SettingsSection>

      <InviteChannelSection />

      {/* License 全文：ExpandableRow 直接作为 tab 末尾元素 */}
      <ExpandableRow label={t('settings.about.licenseToggle')}>
        <pre className={styles['about-license-text']}>{LICENSE_TEXT}</pre>
      </ExpandableRow>

      <UpdateHistoryDialog
        open={historyOpen}
        loading={historyLoading}
        history={history}
        onClose={() => setHistoryOpen(false)}
      />
    </div>
  );
}

const LICENSE_TEXT = `Apache License, Version 2.0

Copyright 2026 Lingxi

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.`;
