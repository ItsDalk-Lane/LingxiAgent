/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';

import type { AutoUpdateState } from '../../../types';

const IDLE_SHELL_STATE: AutoUpdateState = {
  status: 'idle',
  version: null,
  releaseNotes: null,
  releaseUrl: null,
  downloadUrl: null,
  progress: null,
  error: null,
  digest: null,
  digestUrl: null,
  digestError: null,
  updateSource: null,
};

let shellUpdateStateOverride: AutoUpdateState | null = null;

vi.mock('../../../hooks/use-auto-update-state', () => ({
  useAutoUpdateState: (): AutoUpdateState => shellUpdateStateOverride ?? IDLE_SHELL_STATE,
}));

const checkReleaseNow = vi.fn();

interface ReleaseOverride {
  status: 'idle' | 'checking' | 'latest' | 'available' | 'error';
  latestVersion?: string;
  releaseUrl?: string | null;
  error?: string;
  lastCheckedAt: string | null;
}

const DEFAULT_RELEASE_OVERRIDE: ReleaseOverride = {
  status: 'latest',
  latestVersion: '0.1.2',
  releaseUrl: 'https://github.com/ItsDalk-Lane/LingxiAgent/releases/tag/v0.1.2',
  error: undefined,
  lastCheckedAt: '2026-08-06T08:00:00.000Z',
};

let releaseOverride: ReleaseOverride = { ...DEFAULT_RELEASE_OVERRIDE };

vi.mock('../../../hooks/use-release-check', () => ({
  useReleaseCheck: () => ({
    ...releaseOverride,
    checkNow: checkReleaseNow,
  }),
}));

vi.mock('@/ui', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/ui')>(),
  Toggle: ({
    on,
    onChange,
    label,
    ariaLabel,
  }: {
    on: boolean | undefined;
    onChange: (next: boolean) => void;
    label?: string;
    ariaLabel?: string;
  }) => (
    <button
      type="button"
      aria-label={ariaLabel || label}
      aria-busy={on === undefined ? 'true' : undefined}
      aria-checked={on === undefined ? 'mixed' : on ? 'true' : 'false'}
      data-testid={`${ariaLabel || label}-${on === undefined ? 'loading' : on ? 'on' : 'off'}`}
      disabled={on === undefined}
      onClick={() => {
        if (on !== undefined) onChange(!on);
      }}
    >
      toggle
    </button>
  ),
}));

const autoSaveConfig = vi.fn();
const loadSettingsConfig = vi.fn();

vi.mock('../../helpers', () => ({
  t: (key: string) => key,
  autoSaveConfig: (...args: unknown[]) => autoSaveConfig(...args),
}));

vi.mock('../../actions', () => ({
  loadSettingsConfig: (...args: unknown[]) => loadSettingsConfig(...args),
}));

import { AboutTab } from '../AboutTab';
import { useSettingsStore } from '../../store';

afterEach(() => {
  cleanup();
  autoSaveConfig.mockReset();
  loadSettingsConfig.mockReset();
  checkReleaseNow.mockReset();
  releaseOverride = { ...DEFAULT_RELEASE_OVERRIDE };
  shellUpdateStateOverride = null;
  useSettingsStore.setState({ settingsConfig: null });
  vi.unstubAllGlobals();
});

function installHana(overrides: Record<string, unknown> = {}) {
  vi.stubGlobal('window', Object.assign(window, {
    hana: {
      getAppVersion: vi.fn().mockResolvedValue('0.1.2'),
      autoUpdateCheck: vi.fn(),
      autoUpdateInstall: vi.fn(),
      autoUpdateSetChannel: vi.fn(),
      openExternal: vi.fn(),
      getUpdateDigestHistory: vi.fn().mockResolvedValue({ entries: [], source: 'none', complete: false }),
      ...overrides,
    },
  }));
}

const digest = (version: string) => ({
  schemaVersion: 1 as const,
  tag: `v${version}`,
  version,
  previousTag: '',
  generatedAt: '2026-07-01T00:00:00.000Z',
  noUserFacingChanges: false,
  summary: { zh: `${version} 摘要`, en: `${version} summary` },
  counts: { feature: 0, fix: 1, improvement: 0, migration: 0 },
  items: [{
    id: `${version}-fix`,
    kind: 'fix' as const,
    importance: 'high' as const,
    title: { zh: `${version} 修复`, en: `${version} fix` },
    summary: { zh: `${version} 修复说明`, en: `${version} fix detail` },
    details: [],
    sources: [],
  }],
});

describe('AboutTab', () => {
  it('keeps startup and background controls out of the about page', () => {
    installHana();
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });

    render(<AboutTab />);

    expect(screen.getByText('settings.about.autoCheckUpdates')).toBeTruthy();
    expect(screen.getByText('settings.about.betaUpdates')).toBeTruthy();
    expect(screen.queryByText('settings.general.launchAtLogin')).toBeNull();
    expect(screen.queryByText('settings.general.keepAwake')).toBeNull();
  });

  it('keeps update switches in loading state until settings config is ready', () => {
    installHana();
    useSettingsStore.setState({ settingsConfig: null });

    render(<AboutTab />);

    const switches = screen.getAllByRole('button').filter(
      el => el.getAttribute('aria-checked') === 'mixed',
    ) as HTMLButtonElement[];
    expect(switches).toHaveLength(2);
    for (const item of switches) {
      expect(item.disabled).toBe(true);
      fireEvent.click(item);
    }
    expect(autoSaveConfig).not.toHaveBeenCalled();
    expect(loadSettingsConfig).not.toHaveBeenCalled();
  });

  it('shows the app version (app.getVersion) in the hero, not the train content version', async () => {
    const getAppVersion = vi.fn().mockResolvedValue('0.1.2');
    installHana({ getAppVersion });
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });

    await act(async () => {
      render(<AboutTab />);
      await Promise.resolve();
    });

    expect(getAppVersion).toHaveBeenCalledTimes(1);
    expect(screen.getByText('v0.1.2')).toBeTruthy();
  });

  it('does not render the install button when no shell update is pending', () => {
    installHana();
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });
    shellUpdateStateOverride = { ...IDLE_SHELL_STATE };

    render(<AboutTab />);

    expect(screen.queryByText('settings.about.updateInstall')).toBeNull();
    expect(screen.queryByText('settings.about.updateReady')).toBeNull();
  });

  it('shows the install button in ReleaseUpdateArea while a shell update is downloaded, wired to autoUpdateInstall', () => {
    const autoUpdateInstall = vi.fn();
    installHana({ autoUpdateInstall });
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });
    shellUpdateStateOverride = { ...IDLE_SHELL_STATE, status: 'downloaded', version: '2.0.0' };

    render(<AboutTab />);

    expect(screen.getByText('settings.about.updateReady')).toBeTruthy();
    fireEvent.click(screen.getByText('settings.about.updateInstall'));
    expect(autoUpdateInstall).toHaveBeenCalledTimes(1);
  });

  it('available: shows "Update now" button that triggers autoUpdateCheck', () => {
    const autoUpdateCheck = vi.fn();
    installHana({ autoUpdateCheck });
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });
    releaseOverride = {
      ...DEFAULT_RELEASE_OVERRIDE,
      status: 'available',
      latestVersion: '0.2.0',
      releaseUrl: 'https://github.com/ItsDalk-Lane/LingxiAgent/releases/tag/v0.2.0',
    };

    render(<AboutTab />);

    expect(screen.getByText('settings.about.updateAvailableGithub')).toBeTruthy();
    fireEvent.click(screen.getByText('settings.about.updateNow'));

    expect(autoUpdateCheck).toHaveBeenCalledTimes(1);
    // No manual "check for updates" button while the update button is showing.
    expect(screen.queryByText('settings.about.updateCheckBtn')).toBeNull();
  });

  it('shell error: shows manual download fallback that calls openExternal with the release url', () => {
    const openExternal = vi.fn();
    installHana({ openExternal });
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });
    releaseOverride = {
      ...DEFAULT_RELEASE_OVERRIDE,
      status: 'available',
      latestVersion: '0.2.0',
      releaseUrl: 'https://github.com/ItsDalk-Lane/LingxiAgent/releases/tag/v0.2.0',
    };
    shellUpdateStateOverride = { ...IDLE_SHELL_STATE, status: 'error', error: 'network error' };

    render(<AboutTab />);

    expect(screen.getByText('settings.about.updateAutoFailed')).toBeTruthy();
    fireEvent.click(screen.getByText('settings.about.updateDownloadManual'));
    expect(openExternal).toHaveBeenCalledWith('https://github.com/ItsDalk-Lane/LingxiAgent/releases/tag/v0.2.0');
  });

  it('shell error: falls back to releases/latest page when releaseUrl is missing', () => {
    const openExternal = vi.fn();
    installHana({ openExternal });
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });
    releaseOverride = { ...DEFAULT_RELEASE_OVERRIDE, status: 'available', latestVersion: '0.2.0', releaseUrl: null };
    shellUpdateStateOverride = { ...IDLE_SHELL_STATE, status: 'error', error: 'download failed' };

    render(<AboutTab />);

    fireEvent.click(screen.getByText('settings.about.updateDownloadManual'));
    expect(openExternal).toHaveBeenCalledWith('https://github.com/ItsDalk-Lane/LingxiAgent/releases/latest');
  });

  it('dmg: shows manual download when running from DMG, no auto-update button', () => {
    const openExternal = vi.fn();
    installHana({ openExternal });
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });
    releaseOverride = { ...DEFAULT_RELEASE_OVERRIDE, status: 'available', latestVersion: '0.2.0', releaseUrl: null };
    shellUpdateStateOverride = { ...IDLE_SHELL_STATE, status: 'error', error: 'running_from_dmg' };

    render(<AboutTab />);

    expect(screen.getByText('settings.about.updateAvailableGithub')).toBeTruthy();
    expect(screen.getByText('settings.about.updateDmgHint')).toBeTruthy();
    // No auto-update button when running from DMG
    expect(screen.queryByText('settings.about.updateNow')).toBeNull();
    fireEvent.click(screen.getByText('settings.about.updateDownloadManual'));
    expect(openExternal).toHaveBeenCalledWith('https://github.com/ItsDalk-Lane/LingxiAgent/releases/latest');
  });

  it('downloading: shows download progress percentage', () => {
    installHana();
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });
    shellUpdateStateOverride = {
      ...IDLE_SHELL_STATE,
      status: 'downloading',
      version: '0.2.0',
      progress: { percent: 42, bytesPerSecond: 1000, transferred: 500, total: 1000 },
    };

    render(<AboutTab />);

    expect(screen.getByText('42%')).toBeTruthy();
    expect(screen.queryByText('settings.about.updateNow')).toBeNull();
    expect(screen.queryByText('settings.about.updateCheckBtn')).toBeNull();
  });

  it('error: shows the error copy with a retry button that calls checkNow', () => {
    installHana();
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });
    releaseOverride = { ...DEFAULT_RELEASE_OVERRIDE, status: 'error', error: 'network' };

    render(<AboutTab />);

    expect(screen.getByText('settings.about.updateError')).toBeTruthy();

    fireEvent.click(screen.getByText('settings.about.updateRetryBtn'));
    expect(checkReleaseNow).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByText('settings.about.updateRetryBtn'));
    expect(checkReleaseNow).toHaveBeenCalledTimes(2);
    // The generic check button is redundant once the retry button is showing.
    expect(screen.queryByText('settings.about.updateCheckBtn')).toBeNull();
  });

  it('up-to-date: shows the "latest, last checked at" line and keeps the manual check button', () => {
    installHana();
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });
    releaseOverride = { ...DEFAULT_RELEASE_OVERRIDE, status: 'latest', lastCheckedAt: '2026-08-06T08:00:00.000Z' };

    render(<AboutTab />);

    expect(screen.getByText('settings.about.updateLatestCheckedAt')).toBeTruthy();
    // Manual check remains available from the calm "up to date" state.
    expect(screen.getByText('settings.about.updateCheckBtn')).toBeTruthy();
  });

  it('checking: shows the checking message and hides the manual check button', () => {
    installHana();
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });
    releaseOverride = { ...DEFAULT_RELEASE_OVERRIDE, status: 'checking', lastCheckedAt: null };

    render(<AboutTab />);

    expect(screen.getByText('settings.about.updateChecking')).toBeTruthy();
    expect(screen.queryByText('settings.about.updateCheckBtn')).toBeNull();
  });

  it('the beta toggle drives the shell channel IPC', async () => {
    const autoUpdateSetChannel = vi.fn();
    installHana({ autoUpdateSetChannel });
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });

    render(<AboutTab />);

    // Toggle rows render in JSX order: autoCheckUpdates, then betaUpdates —
    // pick the second toggle (mocked Toggle exposes aria-checked).
    const toggles = screen.getAllByRole('button').filter(b => b.hasAttribute('aria-checked'));
    expect(toggles).toHaveLength(2);
    await act(async () => {
      fireEvent.click(toggles[1]);
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(autoUpdateSetChannel).toHaveBeenCalledWith('beta');
    expect(autoSaveConfig).toHaveBeenCalledWith({ update_channel: 'beta' }, { silent: true });
  });

  it('loads the newest five releases only after the update-history dialog is opened', async () => {
    const getUpdateDigestHistory = vi.fn().mockResolvedValue({
      entries: [
        digest('0.400.5'),
        digest('0.400.4'),
        digest('0.400.3'),
        digest('0.400.2'),
        digest('0.400.1'),
      ],
      source: 'online',
      complete: true,
    });
    installHana({
      getUpdateDigestHistory,
    });
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });

    render(<AboutTab />);

    expect(getUpdateDigestHistory).not.toHaveBeenCalled();
    expect(screen.queryByRole('dialog', { name: 'settings.about.updateHistoryTitle' })).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'settings.about.updateHistoryTitle' }));

    expect(await screen.findByRole('dialog', { name: 'settings.about.updateHistoryTitle' })).toBeTruthy();
    expect(await screen.findByText('v0.400.5')).toBeTruthy();
    expect(screen.getByText('v0.400.1')).toBeTruthy();
    expect(getUpdateDigestHistory).toHaveBeenCalledTimes(1);
  });

  // ── 邀请制测试通道 ──

  it('renders no invite entry at all while the redemption service is unconfigured', async () => {
    const inviteStatus = vi.fn().mockResolvedValue({
      configured: false, active: false, inviteCodes: [], channel: 'default',
    });
    installHana({ inviteStatus });
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });

    await act(async () => {
      render(<AboutTab />);
      await Promise.resolve();
    });

    expect(inviteStatus).toHaveBeenCalledTimes(1);
    expect(screen.queryByText('settings.about.inviteSectionTitle')).toBeNull();
    expect(screen.queryByText('settings.about.inviteRedeemBtn')).toBeNull();
  });

  it('offers the invite code field once the redemption service is configured', async () => {
    installHana({
      inviteStatus: vi.fn().mockResolvedValue({
        configured: true, active: false, inviteCodes: [], channel: 'default',
      }),
    });
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });

    await act(async () => {
      render(<AboutTab />);
      await Promise.resolve();
    });

    expect(screen.getByText('settings.about.inviteSectionTitle')).toBeTruthy();
    expect(screen.getByText('settings.about.inviteRedeemBtn')).toBeTruthy();
  });

  it('separates "code is invalid or used up" from a network failure in the redeem error copy', async () => {
    const inviteRedeem = vi.fn().mockResolvedValue({ ok: false, reason: 'invalid', message: 'code not found' });
    installHana({
      inviteStatus: vi.fn().mockResolvedValue({
        configured: true, active: false, inviteCodes: [], channel: 'default',
      }),
      inviteRedeem,
    });
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });

    await act(async () => {
      render(<AboutTab />);
      await Promise.resolve();
    });

    fireEvent.change(screen.getByLabelText('settings.about.inviteCodeLabel'), { target: { value: 'HANA-BAD' } });
    await act(async () => {
      fireEvent.click(screen.getByText('settings.about.inviteRedeemBtn'));
      await Promise.resolve();
    });

    expect(inviteRedeem).toHaveBeenCalledWith('HANA-BAD');
    expect(screen.getByText('settings.about.inviteErrorInvalid')).toBeTruthy();
    expect(screen.queryByText('settings.about.inviteErrorNetwork')).toBeNull();

    inviteRedeem.mockResolvedValue({ ok: false, reason: 'network', message: 'ENOTFOUND' });
    await act(async () => {
      fireEvent.click(screen.getByText('settings.about.inviteRedeemBtn'));
      await Promise.resolve();
    });

    expect(screen.getByText('settings.about.inviteErrorNetwork')).toBeTruthy();
    expect(screen.queryByText('settings.about.inviteErrorInvalid')).toBeNull();
  });

  it('never activates the channel when the one-way-data confirmation is cancelled', async () => {
    const inviteActivate = vi.fn();
    installHana({
      inviteStatus: vi.fn().mockResolvedValue({
        configured: true, active: false, inviteCodes: [], channel: 'default',
      }),
      inviteRedeem: vi.fn().mockResolvedValue({
        ok: true, feedUrl: 'https://updates.example.com/alpha', childCodes: ['CHILD-1', 'CHILD-2'],
      }),
      inviteActivate,
    });
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });

    await act(async () => {
      render(<AboutTab />);
      await Promise.resolve();
    });

    fireEvent.change(screen.getByLabelText('settings.about.inviteCodeLabel'), { target: { value: 'HANA-OK' } });
    await act(async () => {
      fireEvent.click(screen.getByText('settings.about.inviteRedeemBtn'));
      await Promise.resolve();
    });

    expect(screen.getByText('settings.about.inviteConfirmTitle')).toBeTruthy();
    expect(inviteActivate).not.toHaveBeenCalled();

    await act(async () => {
      fireEvent.click(screen.getByText('settings.about.inviteConfirmCancel'));
      await Promise.resolve();
    });

    expect(inviteActivate).not.toHaveBeenCalled();
    expect(screen.queryByText('settings.about.inviteChannelActive')).toBeNull();
  });

  it('activates the channel and shows the two fission codes only after the confirmation is accepted', async () => {
    const inviteActivate = vi.fn().mockResolvedValue({
      configured: true, active: true, inviteCodes: ['CHILD-1', 'CHILD-2'], channel: 'alpha',
    });
    installHana({
      inviteStatus: vi.fn().mockResolvedValue({
        configured: true, active: false, inviteCodes: [], channel: 'default',
      }),
      inviteRedeem: vi.fn().mockResolvedValue({
        ok: true, feedUrl: 'https://updates.example.com/alpha', childCodes: ['CHILD-1', 'CHILD-2'],
      }),
      inviteActivate,
    });
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });

    await act(async () => {
      render(<AboutTab />);
      await Promise.resolve();
    });

    fireEvent.change(screen.getByLabelText('settings.about.inviteCodeLabel'), { target: { value: 'HANA-OK' } });
    await act(async () => {
      fireEvent.click(screen.getByText('settings.about.inviteRedeemBtn'));
      await Promise.resolve();
    });
    await act(async () => {
      fireEvent.click(screen.getByText('settings.about.inviteConfirmOk'));
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(inviteActivate).toHaveBeenCalledWith({
      feedUrl: 'https://updates.example.com/alpha',
      inviteCodes: ['CHILD-1', 'CHILD-2'],
    });
    expect(screen.getByText('settings.about.inviteChannelActive')).toBeTruthy();
    expect((screen.getByDisplayValue('CHILD-1') as HTMLInputElement).readOnly).toBe(true);
    expect(screen.getByDisplayValue('CHILD-2')).toBeTruthy();
    expect(screen.getAllByText('settings.about.inviteCopy')).toHaveLength(2);
  });

  it('shows an explicit bundled-history warning when online history is unavailable', async () => {
    installHana({
      getUpdateDigestHistory: vi.fn().mockResolvedValue({
        entries: [digest('0.400.0')],
        source: 'bundled',
        complete: false,
      }),
    });
    useSettingsStore.setState({ settingsConfig: { auto_check_updates: true, update_channel: 'stable' } });

    render(<AboutTab />);
    fireEvent.click(screen.getByRole('button', { name: 'settings.about.updateHistoryTitle' }));

    expect(await screen.findByText('settings.about.updateHistoryOffline')).toBeTruthy();
    expect(screen.getByText('v0.400.0')).toBeTruthy();
  });
});
