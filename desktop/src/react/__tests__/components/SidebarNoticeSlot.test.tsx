/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PlatformApi, SessionMetaRecoveryStatus, TrainUpdateStatus } from '../../types';
import { useStore } from '../../stores';

// The regression test below asserts the sidebar card ignores the shell
// auto-updater entirely: a downloaded shell update alone must not trigger the card. Stubbing
// the shell hook to report 'downloaded' makes that assertion meaningful
// regardless of whether SidebarNoticeSlot happens to import the hook at
// all — if a future change re-wires it in, this is what would catch it.
vi.mock('../../hooks/use-auto-update-state', () => ({
  useAutoUpdateState: () => ({
    status: 'downloaded',
    version: '2.0.0',
    releaseNotes: null,
    releaseUrl: null,
    downloadUrl: null,
    progress: null,
    error: null,
  }),
}));

import { SidebarNoticeSlot, SidebarUpdateNoticeCard } from '../../components/notices/SidebarNoticeSlot';

const labels: Record<string, string> = {
  'settings.about.trainStickerTitle': '有新版本可用',
  'settings.about.trainStickerDownloading': '下载中 {percent}%',
  'settings.about.trainStickerApplying': '正在应用更新…',
  'settings.about.shellStickerTitleBlocking': '完成此更新后才能继续接收新版本',
  'settings.about.fallbackStickerTitle': '版本 {fromVersion} 连续启动失败，已退回 {toVersion}。出问题的版本已被隔离，不会自动重试。',
  'settings.about.fallbackStickerAckLabel': '知道了',
  'sidebar.metaRecoveryNoticeTitle': '部分会话待恢复',
  'sidebar.metaRecoveryNoticeBody': '检测到会话元数据异常，历史正文仍保存在磁盘上，受影响的会话属性可能暂缺',
  'window.close': '关闭',
};

const META_RECOVERY_DISMISSED_KEY = 'hana-sidebar-meta-recovery-dismissed-key';

function translate(key: string, vars?: Record<string, string | number>): string {
  let value = labels[key] ?? key;
  for (const [name, replacement] of Object.entries(vars ?? {})) {
    value = value.replace(`{${name}}`, String(replacement));
  }
  return value;
}

describe('SidebarUpdateNoticeCard', () => {
  beforeEach(() => {
    window.t = translate as typeof window.t;
    window.localStorage.clear();
    useStore.setState({ locale: 'zh-CN' });
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    window.localStorage.clear();
    useStore.setState({ locale: 'zh-CN' });
  });

  it('stays silent when nothing is available and the shell is not blocked', () => {
    const { container, rerender } = render(
      <SidebarUpdateNoticeCard available={null} minShellBlocked={false} phase="idle" progress={null} />,
    );
    expect(container).toBeEmptyDOMElement();

    rerender(<SidebarUpdateNoticeCard available={null} minShellBlocked={false} phase="checking" progress={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('shows the default train form when a train is available, with the content version as subtitle', () => {
    const onApplyTrain = vi.fn();
    render(
      <SidebarUpdateNoticeCard
        available={{ version: '0.400.0' }}
        minShellBlocked={false}
        phase="idle"
        progress={null}
        onApplyTrain={onApplyTrain}
      />,
    );

    expect(screen.getByText('有新版本可用')).toBeInTheDocument();
    expect(screen.getByText('v0.400.0')).toBeInTheDocument();
    const action = screen.getByRole('button', { name: /有新版本可用/ });
    expect(action.firstElementChild?.querySelector('svg')).toBeInTheDocument();

    fireEvent.click(screen.getByText('有新版本可用'));
    expect(onApplyTrain).toHaveBeenCalledTimes(1);
  });

  it('retranslates an already-visible update when i18n finishes loading after the card mounts', () => {
    window.t = ((key: string) => key) as typeof window.t;
    useStore.setState({ locale: '' });
    render(
      <SidebarUpdateNoticeCard
        available={{ version: '0.400.0' }}
        minShellBlocked={false}
        phase="idle"
        progress={null}
      />,
    );

    expect(screen.getByText('settings.about.trainStickerTitle')).toBeInTheDocument();

    window.t = translate as typeof window.t;
    act(() => useStore.setState({ locale: 'zh-CN' }));

    expect(screen.getByText('有新版本可用')).toBeInTheDocument();
    expect(screen.queryByText('settings.about.trainStickerTitle')).not.toBeInTheDocument();
  });

  it('retranslates the card when the user switches locale at runtime', () => {
    let activeLabels: Record<string, string> = {
      ...labels,
      'settings.about.trainStickerTitle': 'Update available',
      'window.close': 'Close',
    };
    window.t = ((key: string) => activeLabels[key] ?? key) as typeof window.t;
    useStore.setState({ locale: 'en' });
    render(
      <SidebarUpdateNoticeCard
        available={{ version: '0.400.0' }}
        minShellBlocked={false}
        phase="idle"
        progress={null}
      />,
    );

    expect(screen.getByText('Update available')).toBeInTheDocument();

    activeLabels = labels;
    act(() => useStore.setState({ locale: 'zh-CN' }));

    expect(screen.getByText('有新版本可用')).toBeInTheDocument();
    expect(screen.queryByText('Update available')).not.toBeInTheDocument();
  });

  it('renders download progress and applying phase text on the card while an apply is in flight', () => {
    const { rerender } = render(
      <SidebarUpdateNoticeCard
        available={{ version: '0.400.0' }}
        minShellBlocked={false}
        phase="downloading"
        progress={{ receivedBytes: 50, totalBytes: 200 }}
      />,
    );
    expect(screen.getByText('下载中 25%')).toBeInTheDocument();
    expect(screen.getByText('v0.400.0')).toBeInTheDocument();

    rerender(
      <SidebarUpdateNoticeCard
        available={{ version: '0.400.0' }}
        minShellBlocked={false}
        phase="applying"
        progress={{ receivedBytes: 200, totalBytes: 200 }}
      />,
    );
    expect(screen.getByText('正在应用更新…')).toBeInTheDocument();
  });

  it('switches to the blocked (shell-required) form when minShellBlocked is true, even if a train is also available', () => {
    const onInstallShell = vi.fn();
    const onApplyTrain = vi.fn();
    render(
      <SidebarUpdateNoticeCard
        available={{ version: '0.400.0' }}
        minShellBlocked
        phase="idle"
        progress={null}
        onInstallShell={onInstallShell}
        onApplyTrain={onApplyTrain}
      />,
    );

    expect(screen.getByText('完成此更新后才能继续接收新版本')).toBeInTheDocument();
    expect(screen.queryByText('有新版本可用')).not.toBeInTheDocument();

    fireEvent.click(screen.getByText('完成此更新后才能继续接收新版本'));
    expect(onInstallShell).toHaveBeenCalledTimes(1);
    expect(onApplyTrain).not.toHaveBeenCalled();
  });

  it('dismissing the train sticker hides it for that version only; a newer available version reappears', () => {
    const { container, rerender } = render(
      <SidebarUpdateNoticeCard available={{ version: '0.400.0' }} minShellBlocked={false} phase="idle" progress={null} />,
    );
    expect(screen.getByText('有新版本可用')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: '关闭' }));
    expect(container).toBeEmptyDOMElement();

    // Re-rendering with the SAME available version stays dismissed (persisted via storage).
    rerender(<SidebarUpdateNoticeCard available={{ version: '0.400.0' }} minShellBlocked={false} phase="idle" progress={null} />);
    expect(container).toBeEmptyDOMElement();

    // A newer available version reappears.
    rerender(<SidebarUpdateNoticeCard available={{ version: '0.401.0' }} minShellBlocked={false} phase="idle" progress={null} />);
    expect(screen.getByText('有新版本可用')).toBeInTheDocument();
  });

  it('dismissing the blocked sticker hides it for this mount only, without touching localStorage (session-only)', () => {
    const { container } = render(
      <SidebarUpdateNoticeCard available={null} minShellBlocked phase="idle" progress={null} />,
    );
    expect(screen.getByText('完成此更新后才能继续接收新版本')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: '关闭' }));
    expect(container).toBeEmptyDOMElement();
    expect(window.localStorage.length).toBe(0);
  });

  it('a fresh mount after a blocked dismissal (simulating next launch) shows the sticker again', () => {
    const first = render(<SidebarUpdateNoticeCard available={null} minShellBlocked phase="idle" progress={null} />);
    fireEvent.click(first.getByRole('button', { name: '关闭' }));
    expect(first.container).toBeEmptyDOMElement();
    first.unmount();

    render(<SidebarUpdateNoticeCard available={null} minShellBlocked phase="idle" progress={null} />);
    expect(screen.getByText('完成此更新后才能继续接收新版本')).toBeInTheDocument();
  });

  it('dismissing the meta-recovery card writes a signature key and survives remount', () => {
    const metaRecovery = { degraded: true, reasons: [{ kind: 'quarantined', detail: 'a.jsonl' }] };
    const first = render(
      <SidebarUpdateNoticeCard
        available={null}
        minShellBlocked={false}
        phase="idle"
        progress={null}
        metaRecovery={metaRecovery}
      />,
    );
    expect(screen.getByText('部分会话待恢复')).toBeInTheDocument();

    fireEvent.click(first.getByRole('button', { name: '关闭' }));
    expect(first.container).toBeEmptyDOMElement();
    expect(window.localStorage.getItem(META_RECOVERY_DISMISSED_KEY)).toBe('quarantined:a.jsonl');
    first.unmount();

    // A fresh mount is what "next launch" looks like: the ledger-driven
    // degradation is still reported, and the card must stay silent for it.
    render(
      <SidebarUpdateNoticeCard
        available={null}
        minShellBlocked={false}
        phase="idle"
        progress={null}
        metaRecovery={metaRecovery}
      />,
    );
    expect(screen.queryByText('部分会话待恢复')).not.toBeInTheDocument();
  });

  it('a changed degradation set reappears once', () => {
    const first = render(
      <SidebarUpdateNoticeCard
        available={null}
        minShellBlocked={false}
        phase="idle"
        progress={null}
        metaRecovery={{ degraded: true, reasons: [{ kind: 'quarantined', detail: 'a.jsonl' }] }}
      />,
    );
    fireEvent.click(first.getByRole('button', { name: '关闭' }));
    expect(first.container).toBeEmptyDOMElement();
    first.unmount();

    // One more damaged file = a new event worth interrupting for, even though
    // the old reason is still in the set.
    render(
      <SidebarUpdateNoticeCard
        available={null}
        minShellBlocked={false}
        phase="idle"
        progress={null}
        metaRecovery={{
          degraded: true,
          reasons: [{ kind: 'quarantined', detail: 'a.jsonl' }, { kind: 'quarantined', detail: 'b.jsonl' }],
        }}
      />,
    );
    expect(screen.getByText('部分会话待恢复')).toBeInTheDocument();
  });

  it('the empty degraded state still produces a stable signature', () => {
    const metaRecovery = { degraded: true, reasons: [] };
    const first = render(
      <SidebarUpdateNoticeCard
        available={null}
        minShellBlocked={false}
        phase="idle"
        progress={null}
        metaRecovery={metaRecovery}
      />,
    );
    expect(screen.getByText('部分会话待恢复')).toBeInTheDocument();

    fireEvent.click(first.getByRole('button', { name: '关闭' }));
    // An empty reason list must not collapse into an empty key — otherwise the
    // stored value would read as "never dismissed" and the card would return.
    expect(window.localStorage.getItem(META_RECOVERY_DISMISSED_KEY)).toBe('degraded');
    first.unmount();

    render(
      <SidebarUpdateNoticeCard
        available={null}
        minShellBlocked={false}
        phase="idle"
        progress={null}
        metaRecovery={metaRecovery}
      />,
    );
    expect(screen.queryByText('部分会话待恢复')).not.toBeInTheDocument();
  });

  it('a dismissed meta-recovery yields the slot to an available train update', () => {
    const metaRecovery = { degraded: true, reasons: [{ kind: 'quarantined', detail: 'a.jsonl' }] };
    const first = render(
      <SidebarUpdateNoticeCard
        available={{ version: '0.400.0' }}
        minShellBlocked={false}
        phase="idle"
        progress={null}
        metaRecovery={metaRecovery}
      />,
    );
    fireEvent.click(first.getByRole('button', { name: '关闭' }));
    first.unmount();

    // The notice slot holds one card. Dismissing the recovery notice must
    // remove it from the running for that slot, not blank the slot itself —
    // the update sticker below it is still worth showing.
    render(
      <SidebarUpdateNoticeCard
        available={{ version: '0.400.0' }}
        minShellBlocked={false}
        phase="idle"
        progress={null}
        metaRecovery={metaRecovery}
      />,
    );
    expect(screen.queryByText('部分会话待恢复')).not.toBeInTheDocument();
    expect(screen.getByText('有新版本可用')).toBeInTheDocument();
  });

  it('a dismissed meta-recovery yields the slot to the blocked (shell-required) card', () => {
    const metaRecovery = { degraded: true, reasons: [{ kind: 'quarantined', detail: 'a.jsonl' }] };
    const first = render(
      <SidebarUpdateNoticeCard
        available={null}
        minShellBlocked
        phase="idle"
        progress={null}
        metaRecovery={metaRecovery}
      />,
    );
    fireEvent.click(first.getByRole('button', { name: '关闭' }));
    first.unmount();

    render(
      <SidebarUpdateNoticeCard
        available={null}
        minShellBlocked
        phase="idle"
        progress={null}
        metaRecovery={metaRecovery}
      />,
    );
    expect(screen.queryByText('部分会话待恢复')).not.toBeInTheDocument();
    expect(screen.getByText('完成此更新后才能继续接收新版本')).toBeInTheDocument();
  });

  it('the meta-recovery signature ignores the order the reasons arrive in', () => {
    const first = render(
      <SidebarUpdateNoticeCard
        available={null}
        minShellBlocked={false}
        phase="idle"
        progress={null}
        metaRecovery={{
          degraded: true,
          reasons: [{ kind: 'quarantined', detail: 'a.jsonl' }, { kind: 'unreadable', detail: 'b.jsonl' }],
        }}
      />,
    );
    fireEvent.click(first.getByRole('button', { name: '关闭' }));
    first.unmount();

    // Same set, server returned it the other way around: still dismissed.
    render(
      <SidebarUpdateNoticeCard
        available={null}
        minShellBlocked={false}
        phase="idle"
        progress={null}
        metaRecovery={{
          degraded: true,
          reasons: [{ kind: 'unreadable', detail: 'b.jsonl' }, { kind: 'quarantined', detail: 'a.jsonl' }],
        }}
      />,
    );
    expect(screen.queryByText('部分会话待恢复')).not.toBeInTheDocument();
  });

  it('malformed reasons from the health payload render the card instead of throwing', () => {
    // /api/health's sessionStore block is forwarded verbatim, so the declared
    // shape is a claim, not a guarantee. None of these may take down the
    // sidebar; each must still yield a dismissible card.
    const malformed = [
      { degraded: true, reasons: 'nope' },
      { degraded: true, reasons: [null] },
      { degraded: true, reasons: ['oops'] },
    ] as unknown as SessionMetaRecoveryStatus[];

    for (const metaRecovery of malformed) {
      const view = render(
        <SidebarUpdateNoticeCard
          available={null}
          minShellBlocked={false}
          phase="idle"
          progress={null}
          metaRecovery={metaRecovery}
        />,
      );
      expect(screen.getByText('部分会话待恢复')).toBeInTheDocument();
      fireEvent.click(view.getByRole('button', { name: '关闭' }));
      expect(view.container).toBeEmptyDOMElement();
      view.unmount();
      window.localStorage.clear();
    }
  });

  it('shows the fallback (crash-recovery) form with both versions in the message when fallbackNotice is present', () => {
    const onAckFallback = vi.fn();
    render(
      <SidebarUpdateNoticeCard
        available={null}
        minShellBlocked={false}
        phase="idle"
        progress={null}
        fallbackNotice={{ kind: 'server', fromVersion: '0.390.0', toVersion: '0.389.0', quarantinedTrain: 7 }}
        onAckFallback={onAckFallback}
      />,
    );

    const message = screen.getByText('版本 0.390.0 连续启动失败，已退回 0.389.0。出问题的版本已被隔离，不会自动重试。');
    expect(message).toBeInTheDocument();

    // Clicking the card body itself must not trigger any action (no
    // onInstallShell/onApplyTrain call) — the fallback form has nothing to
    // "apply", only to acknowledge.
    fireEvent.click(message);
    expect(onAckFallback).not.toHaveBeenCalled();
  });

  it('fallbackNotice outranks minShellBlocked and available: the crash-recovery card wins the single display slot', () => {
    render(
      <SidebarUpdateNoticeCard
        available={{ version: '0.400.0' }}
        minShellBlocked
        phase="idle"
        progress={null}
        fallbackNotice={{ kind: 'renderer', fromVersion: '0.390.0', toVersion: '0.389.0', quarantinedTrain: null }}
      />,
    );

    expect(screen.getByText('版本 0.390.0 连续启动失败，已退回 0.389.0。出问题的版本已被隔离，不会自动重试。')).toBeInTheDocument();
    expect(screen.queryByText('完成此更新后才能继续接收新版本')).not.toBeInTheDocument();
    expect(screen.queryByText('有新版本可用')).not.toBeInTheDocument();
  });

  it('acknowledging the fallback card calls onAckFallback (not a local dismissed-state toggle)', () => {
    const onAckFallback = vi.fn();
    const { container } = render(
      <SidebarUpdateNoticeCard
        available={null}
        minShellBlocked={false}
        phase="idle"
        progress={null}
        fallbackNotice={{ kind: 'server', fromVersion: '0.390.0', toVersion: '0.389.0', quarantinedTrain: null }}
        onAckFallback={onAckFallback}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: '知道了' }));
    expect(onAckFallback).toHaveBeenCalledTimes(1);
    // The parent (hook) owns clearing fallbackNotice; this component doesn't
    // hide itself on its own — re-rendering without fallbackNotice is what
    // makes it disappear, mirrored by the wired-hook test below.
    expect(container).not.toBeEmptyDOMElement();
  });
});

describe('SidebarNoticeSlot (wired to the real hook)', () => {
  beforeEach(() => {
    window.t = translate as typeof window.t;
    window.localStorage.clear();
    useStore.setState({ locale: 'zh-CN' });
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    window.localStorage.clear();
    useStore.setState({ locale: 'zh-CN' });
  });

  function installHana(status: Partial<TrainUpdateStatus>) {
    window.hana = {
      trainUpdateStatus: vi.fn().mockResolvedValue({
        staged: false,
        train: null,
        version: null,
        minShellBlocked: false,
        available: null,
        lastError: null,
        lastCheckedAt: null,
        currentVersion: '0.388.0',
        fallbackNotice: null,
        ...status,
      }),
      trainUpdateCheck: vi.fn(),
      trainUpdateApply: vi.fn().mockResolvedValue({ ok: true }),
      onTrainUpdateAvailable: vi.fn(() => () => {}),
      onTrainUpdateProgress: vi.fn(() => () => {}),
      onTrainFallbackNotice: vi.fn(() => () => {}),
      ackTrainFallbackNotice: vi.fn().mockResolvedValue({ ok: true }),
      autoUpdateInstall: vi.fn(),
    } as unknown as PlatformApi;
  }

  it('regression: a shell auto-updater "downloaded" state alone must never surface the card — only `available`/`minShellBlocked` from the train status can', async () => {
    // The module-level mock above makes useAutoUpdateState() report
    // 'downloaded' for every test in this describe block — the old bug was
    // exactly "shell downloaded -> card shows". The train status here
    // deliberately supplies nothing shell-flavored (no available, not
    // blocked), so if the card is still driven by shell state in any form,
    // this is where it would show up.
    installHana({});

    const { container } = render(<SidebarNoticeSlot />);
    await waitFor(() => expect(window.hana?.trainUpdateStatus).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });

  it('shows the train card once the real hook resolves an available update', async () => {
    installHana({ available: { train: 9, version: '0.400.0', serverSha256: 'a'.repeat(64), rendererSha256: 'b'.repeat(64), sizes: { server: 1, renderer: 1 }, recordedAt: '2026-07-11T00:00:00.000Z' } });

    render(<SidebarNoticeSlot />);

    expect(await screen.findByText('有新版本可用')).toBeInTheDocument();
    expect(screen.getByText('v0.400.0')).toBeInTheDocument();
  });

  it('applying the update calls trainUpdateApply via the hook', async () => {
    const trainUpdateApply = vi.fn().mockResolvedValue({ ok: true });
    installHana({ available: { train: 9, version: '0.400.0', serverSha256: 'a'.repeat(64), rendererSha256: 'b'.repeat(64), sizes: { server: 1, renderer: 1 }, recordedAt: '2026-07-11T00:00:00.000Z' } });
    (window.hana as unknown as { trainUpdateApply: typeof trainUpdateApply }).trainUpdateApply = trainUpdateApply;

    render(<SidebarNoticeSlot />);
    const card = await screen.findByText('有新版本可用');
    fireEvent.click(card);

    expect(trainUpdateApply).toHaveBeenCalledTimes(1);
  });

  it('shows the blocked form and wires it to autoUpdateInstall when minShellBlocked is true', async () => {
    const autoUpdateInstall = vi.fn();
    installHana({ minShellBlocked: true });
    (window.hana as unknown as { autoUpdateInstall: typeof autoUpdateInstall }).autoUpdateInstall = autoUpdateInstall;

    render(<SidebarNoticeSlot />);
    const card = await screen.findByText('完成此更新后才能继续接收新版本');
    fireEvent.click(card);

    expect(autoUpdateInstall).toHaveBeenCalledTimes(1);
  });

  it('reacts to a background onTrainUpdateAvailable broadcast without a remount', async () => {
    type AvailablePayload = { version: string; minShellBlocked: boolean };
    const box: { deliver: ((payload: AvailablePayload) => void) | null } = { deliver: null };
    installHana({});
    (window.hana as unknown as { onTrainUpdateAvailable: (cb: (p: AvailablePayload) => void) => () => void }).onTrainUpdateAvailable = (cb) => {
      box.deliver = cb;
      return () => { box.deliver = null; };
    };

    render(<SidebarNoticeSlot />);
    await waitFor(() => expect(window.hana?.trainUpdateStatus).toHaveBeenCalled());
    expect(screen.queryByText('有新版本可用')).not.toBeInTheDocument();
    await waitFor(() => expect(box.deliver).not.toBeNull());

    box.deliver?.({ version: '0.402.0', minShellBlocked: false });

    expect(await screen.findByText('有新版本可用')).toBeInTheDocument();
    expect(screen.getByText('v0.402.0')).toBeInTheDocument();
  });

  it('shows the fallback card from a cold-start status pull and outranks an available train update', async () => {
    installHana({
      available: { train: 9, version: '0.400.0', serverSha256: 'a'.repeat(64), rendererSha256: 'b'.repeat(64), sizes: { server: 1, renderer: 1 }, recordedAt: '2026-07-11T00:00:00.000Z' },
      fallbackNotice: { kind: 'server', fromVersion: '0.390.0', toVersion: '0.389.0', quarantinedTrain: 7 },
    });

    render(<SidebarNoticeSlot />);

    expect(await screen.findByText('版本 0.390.0 连续启动失败，已退回 0.389.0。出问题的版本已被隔离，不会自动重试。')).toBeInTheDocument();
    expect(screen.queryByText('有新版本可用')).not.toBeInTheDocument();
  });

  it('acknowledging the fallback card via the real hook calls ackTrainFallbackNotice and the card disappears', async () => {
    const ackTrainFallbackNotice = vi.fn().mockResolvedValue({ ok: true });
    installHana({ fallbackNotice: { kind: 'renderer', fromVersion: '0.390.0', toVersion: '0.389.0', quarantinedTrain: null } });
    (window.hana as unknown as { ackTrainFallbackNotice: typeof ackTrainFallbackNotice }).ackTrainFallbackNotice = ackTrainFallbackNotice;

    const { container } = render(<SidebarNoticeSlot />);
    const ackButton = await screen.findByRole('button', { name: '知道了' });
    fireEvent.click(ackButton);

    expect(ackTrainFallbackNotice).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(container).toBeEmptyDOMElement());
  });

  it('reacts to a background onTrainFallbackNotice broadcast without a remount (runtime renderer crash path)', async () => {
    type FallbackPayload = { kind: 'server' | 'renderer'; fromVersion: string | null; toVersion: string | null; quarantinedTrain: number | null };
    const box: { deliver: ((payload: FallbackPayload) => void) | null } = { deliver: null };
    installHana({});
    (window.hana as unknown as { onTrainFallbackNotice: (cb: (p: FallbackPayload) => void) => () => void }).onTrainFallbackNotice = (cb) => {
      box.deliver = cb;
      return () => { box.deliver = null; };
    };

    render(<SidebarNoticeSlot />);
    await waitFor(() => expect(window.hana?.trainUpdateStatus).toHaveBeenCalled());
    expect(screen.queryByText(/连续启动失败/)).not.toBeInTheDocument();
    await waitFor(() => expect(box.deliver).not.toBeNull());

    box.deliver?.({ kind: 'renderer', fromVersion: '0.391.0', toVersion: '0.390.0', quarantinedTrain: 3 });

    expect(await screen.findByText('版本 0.391.0 连续启动失败，已退回 0.390.0。出问题的版本已被隔离，不会自动重试。')).toBeInTheDocument();
  });
});
