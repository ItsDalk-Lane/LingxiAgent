// @vitest-environment jsdom

import fs from 'node:fs';
import path from 'node:path';
import React from 'react';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AccessTab } from '../tabs/AccessTab';
import { lingxiFetch } from '../api';
import { useSettingsStore } from '../store';

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
const cases: Array<Record<string, unknown>> = [];

describe('R02 access settings route selected by actual connection identity', () => {
  const fetchMock = vi.fn(async (_url: RequestInfo | URL, _opts?: RequestInit) => new Response(JSON.stringify(summary), {
    status: 200, headers: { 'content-type': 'application/json' },
  }));

  beforeEach(() => {
    fetchMock.mockClear();
    vi.stubGlobal('fetch', fetchMock);
    window.t = ((key: string) => key) as typeof window.t;
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it.each([
    ['rust', 'lingxi-service', '/lingxi/v1'],
    ['legacy', undefined, '/api'],
  ])('%s connection sends access calls and QR image to its own route family', async (kind, serverNodeKind, prefix) => {
    const connection = {
      connectionId: 'local', kind: 'local', credentialKind: 'loopback_token',
      serverNodeKind, label: 'Local', baseUrl: 'http://127.0.0.1:14500',
      token: 'test-only',
    };
    useSettingsStore.setState({
      settingsSnapshot: { data: null }, serverConnections: { local: connection },
      activeServerConnection: connection, activeServerConnectionId: 'local',
      showToast: vi.fn(),
    } as never);
    render(<AccessTab />);
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    expect(new URL(String(fetchMock.mock.calls[0][0])).pathname).toBe(`${prefix}/access/summary`);
    const qr = await screen.findByAltText('settings.access.qrCode');
    expect(new URL(qr.getAttribute('src') || '').pathname).toBe(`${prefix}/access/mobile-qr.svg`);
    await lingxiFetch('/api/access/mobile-credentials', { method: 'POST' });
    await lingxiFetch('/api/devices/example/revoke', { method: 'POST' });
    expect(new URL(String(fetchMock.mock.calls[1][0])).pathname).toBe(`${prefix}/access/mobile-credentials`);
    expect(new URL(String(fetchMock.mock.calls[2][0])).pathname).toBe(`${prefix}/devices/example/revoke`);
    cases.push({
      case: `access-${kind}-route-and-qr`, expect: 1, actual: 1, ok: true,
      observed: {
        connectionIdentity: serverNodeKind || 'legacy',
        summaryPath: `${prefix}/access/summary`, qrPath: `${prefix}/access/mobile-qr.svg`,
        credentialPath: `${prefix}/access/mobile-credentials`,
        revokePath: `${prefix}/devices/example/revoke`,
      },
    });
  });
});

afterAll(() => {
  const target = process.env.R02_ACCESS_ROUTE_CASES_PATH;
  if (!target) return;
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, `${JSON.stringify({ schema: 'lingxi.leaf-case-results.v1', cases }, null, 2)}\n`);
});
