// @vitest-environment jsdom

import fs from 'node:fs';
import path from 'node:path';
import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AccessTab } from '../tabs/AccessTab';
import { useSettingsStore } from '../store';

const api = vi.hoisted(() => ({ lingxiFetch: vi.fn() }));
vi.mock('../api', () => ({
  lingxiFetch: api.lingxiFetch,
  lingxiUrl: (relative: string) => `http://127.0.0.1:14500${relative}`,
}));

const cases: Array<Record<string, unknown>> = [];
const summary = {
  network: {
    mode: 'lan', listenHost: '0.0.0.0', publicBaseUrl: null,
    publicMobileUrl: null, publicDesktopUrl: null,
    configuredPort: 14500, actualPort: 14500,
    runtimeMode: 'lan', runtimeHost: '0.0.0.0', restartRequired: false,
    lanAddresses: ['192.0.2.10'], localServerUrl: 'http://127.0.0.1:14500',
    candidateLanServerUrl: 'http://192.0.2.10:14500', lanServerUrl: 'http://192.0.2.10:14500',
    localMobileUrl: 'http://127.0.0.1:14500/mobile',
    candidateLanMobileUrl: 'http://192.0.2.10:14500/mobile',
    lanMobileUrl: 'http://192.0.2.10:14500/mobile',
    localDesktopUrl: 'http://127.0.0.1:14500/desktop',
    candidateLanDesktopUrl: 'http://192.0.2.10:14500/desktop',
    lanDesktopUrl: 'http://192.0.2.10:14500/desktop',
  },
  account: { userId: 'local', username: 'owner', displayName: 'Owner', passwordSet: false },
  devices: [], credentials: [],
};

function response(data: unknown) {
  return { json: async () => data };
}

function record(name: string, observed: Record<string, unknown>) {
  cases.push({ case: name, expect: 1, actual: 1, ok: true, observed });
}

describe('R02/R00 AccessTab credential UI branches', () => {
  const toast = vi.fn();

  beforeEach(() => {
    api.lingxiFetch.mockReset();
    toast.mockReset();
    api.lingxiFetch.mockImplementation(async (url: string) => {
      if (url === '/api/access/summary') return response(summary);
      throw new Error(`unexpected route ${url}`);
    });
    window.t = ((key: string) => key) as typeof window.t;
    useSettingsStore.setState({
      settingsSnapshot: { data: null }, showToast: toast,
      serverConnections: {
        local: {
          connectionId: 'local', kind: 'local', credentialKind: 'loopback_token',
          label: 'Local', baseUrl: 'http://127.0.0.1:14500', token: 'test-only',
        },
      },
      activeServerConnectionId: 'local',
      activeServerConnection: {
        connectionId: 'local', kind: 'local', credentialKind: 'loopback_token',
        label: 'Local', baseUrl: 'http://127.0.0.1:14500', token: 'test-only',
      },
    } as never);
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    delete (navigator as { clipboard?: unknown }).clipboard;
  });

  it('keeps the original copied projection when the clipboard API is absent', async () => {
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: undefined });
    render(<AccessTab />);
    await waitFor(() => expect(screen.getAllByTitle('settings.access.copy')[0].hasAttribute('disabled')).toBe(false));
    fireEvent.click(screen.getAllByTitle('settings.access.copy')[0]);
    await waitFor(() => expect(toast).toHaveBeenCalledWith('settings.access.copied', 'success'));
    record('access-copy-no-clipboard-original-projection', {
      clipboardApiAbsent: true, copiedToastShown: true,
    });
  });

  it('does not turn a failed access-summary request into an empty device list', async () => {
    api.lingxiFetch.mockRejectedValue(new Error('summary unavailable'));
    render(<AccessTab />);
    await waitFor(() => expect(toast).toHaveBeenCalledWith(expect.stringContaining('summary unavailable'), 'error'));
    expect(screen.queryByText('settings.access.noDevices')).toBeNull();
    record('access-summary-error-is-not-empty', {
      failureToast: true, emptyDeviceProjection: false,
    });
  });

  it('shows an empty device list only after a successful empty summary', async () => {
    let finish: ((value: ReturnType<typeof response>) => void) | undefined;
    api.lingxiFetch.mockImplementation(() => new Promise(resolve => { finish = resolve; }));
    render(<AccessTab />);
    expect(screen.queryByText('settings.access.noDevices')).toBeNull();
    expect(screen.queryByRole('alert')).toBeNull();
    finish?.(response(summary));
    await waitFor(() => expect(screen.getByText('settings.access.noDevices')).toBeTruthy());
    expect(screen.queryByRole('alert')).toBeNull();
    record('access-summary-success-empty-only-after-load', {
      loadingDidNotPretendEmpty: true, successfulEmptyShown: true,
    });
  });

  it.each([
    ['device', { ...summary, devices: [{ deviceId: 'device-1', displayName: 'Phone' }] }],
    ['credential', { ...summary, credentials: [{ credentialId: 'credential-1', deviceId: 'device-1', status: 'active' }] }],
    ['network', { ...summary, network: { ...summary.network, lanAddresses: [null] } }],
  ])('rejects a malformed %s summary instead of showing an empty state', async (kind, malformed) => {
    api.lingxiFetch.mockResolvedValue(response(malformed));
    render(<AccessTab />);
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('settings.access.loadFailed'));
    expect(screen.queryByText('settings.access.noDevices')).toBeNull();
    expect(toast).toHaveBeenCalledWith(expect.stringContaining('incomplete'), 'error');
    record(`access-summary-malformed-${kind}-is-error`, {
      responseRejected: true, errorVisible: true, emptyDeviceProjection: false,
    });
  });

  it.each([
    ['mobile', 'settings.access.generateMobileKey', '/api/access/mobile-credentials', 'settings.access.mobileKey'],
    ['desktop', 'settings.access.generateDesktopKey', '/api/access/desktop-credentials', 'settings.access.desktopKey'],
  ])('%s credential: successful one-time secret survives a failed summary refresh', async (kind, button, route, field) => {
    let summaryReads = 0;
    api.lingxiFetch.mockImplementation(async (url: string) => {
      if (url === '/api/access/summary') {
        summaryReads += 1;
        if (summaryReads > 1) throw new Error('summary refresh denied');
        return response(summary);
      }
      if (url === route) return response({ secret: `test-${kind}-secret` });
      throw new Error(`unexpected route ${url}`);
    });
    render(<AccessTab />);
    await waitFor(() => expect(api.lingxiFetch).toHaveBeenCalledWith('/api/access/summary'));
    await waitFor(() => expect(screen.getByRole('button', { name: button }).hasAttribute('disabled')).toBe(false));
    expect(screen.getByAltText('settings.access.qrCode').getAttribute('src')).toContain('mobile-qr.svg');
    fireEvent.click(screen.getByRole('button', { name: button }));
    await waitFor(() => expect(screen.getByDisplayValue(`test-${kind}-secret`)).toBeTruthy());
    expect(screen.getByText(field)).toBeTruthy();
    await waitFor(() => expect(toast).toHaveBeenCalledWith(expect.stringContaining('summary refresh denied'), 'error'));
    expect(toast.mock.calls.some(call => call[1] === 'success')).toBe(false);
    record(`access-${kind}-secret-refresh-failure`, {
      initialSummaryReads: 1, summaryReads, secretVisible: true,
      refreshFailureShown: true, falseSuccess: false, qrShown: true,
    });
  });

  it.each([
    ['mobile', 'settings.access.generateMobileKey', '/api/access/mobile-credentials'],
    ['desktop', 'settings.access.generateDesktopKey', '/api/access/desktop-credentials'],
  ])('%s credential: malformed response and rejected generation do not report success', async (kind, button, route) => {
    let generation = 0;
    api.lingxiFetch.mockImplementation(async (url: string) => {
      if (url === '/api/access/summary') return response(summary);
      if (url === route) {
        generation += 1;
        if (generation === 1) return response({ secret: `old-${kind}-secret` });
        if (generation === 2) return response({ credentialId: 'missing-secret' });
        throw new Error('generation denied');
      }
      throw new Error(`unexpected route ${url}`);
    });
    render(<AccessTab />);
    await waitFor(() => expect(screen.getByRole('button', { name: button }).hasAttribute('disabled')).toBe(false));
    const generate = screen.getByRole('button', { name: button });
    fireEvent.click(generate);
    await waitFor(() => expect(screen.getByDisplayValue(`old-${kind}-secret`)).toBeTruthy());
    toast.mockClear();
    fireEvent.click(generate);
    await waitFor(() => expect(toast).toHaveBeenCalledWith(expect.stringContaining('missing secret'), 'error'));
    expect(screen.getByDisplayValue(`old-${kind}-secret`)).toBeTruthy();
    expect(toast.mock.calls.some(call => call[1] === 'success')).toBe(false);
    toast.mockClear();
    fireEvent.click(generate);
    await waitFor(() => expect(toast).toHaveBeenCalledWith(expect.stringContaining('generation denied'), 'error'));
    expect(screen.getByDisplayValue(`old-${kind}-secret`)).toBeTruthy();
    expect(toast.mock.calls.some(call => call[1] === 'success')).toBe(false);
    record(`access-${kind}-generation-failure-keeps-old-secret`, {
      malformedResponseRejected: true, rejectedRequestReported: true,
      oldSecretKept: true, falseSuccess: false,
    });
  });
});

afterAll(() => {
  const target = process.env.R02_ACCESS_CASES_PATH;
  if (!target) return;
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, `${JSON.stringify({ schema: 'lingxi.leaf-case-results.v1', cases }, null, 2)}\n`);
});
