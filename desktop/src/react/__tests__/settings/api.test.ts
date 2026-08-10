/**
 * @vitest-environment jsdom
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createLocalServerConnection } from '../../services/server-connection';
import { lingxiFetchJson } from '../../settings/api';
import { useSettingsStore } from '../../settings/store';

describe('settings JSON request boundary', () => {
  const fetchMock = vi.fn();

  beforeEach(() => {
    const connection = createLocalServerConnection({ serverPort: 3210, serverToken: null });
    if (!connection) throw new Error('expected local test connection');
    useSettingsStore.setState({
      serverPort: 3210,
      serverToken: null,
      serverConnections: { [connection.connectionId]: connection },
      activeServerConnectionId: connection.connectionId,
      activeServerConnection: connection,
    });
    vi.stubGlobal('fetch', fetchMock);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it('rejects a non-2xx mutation response before callers can show success', async () => {
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ error: 'runtime refresh failed' }), {
      status: 500,
      headers: { 'Content-Type': 'application/json' },
    }));

    await expect(lingxiFetchJson('/api/config', { method: 'PUT' }))
      .rejects.toThrow('runtime refresh failed');
  });

  it('also rejects an HTTP 200 response carrying a JSON error', async () => {
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ error: 'catalog rejected mutation' }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    }));

    await expect(lingxiFetchJson('/api/config', { method: 'PUT' }))
      .rejects.toThrow('catalog rejected mutation');
  });
});
