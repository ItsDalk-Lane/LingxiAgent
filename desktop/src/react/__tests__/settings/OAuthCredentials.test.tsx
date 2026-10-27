/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import type { ProviderSummary } from '../../settings/store';

const mocks = vi.hoisted(() => ({
  lingxiFetch: vi.fn(),
  openExternal: vi.fn(),
}));
const originalPlatform = Object.getOwnPropertyDescriptor(window, 'platform');

vi.mock('../../settings/api', () => ({
  lingxiFetch: (...args: unknown[]) => mocks.lingxiFetch(...args),
  // 现役授权服务通过JSON入口读取，仍记录同一底层请求以核对浏览器方式和本地回调。
  lingxiFetchJson: async (...args: unknown[]) => {
    const response = await mocks.lingxiFetch(...args);
    const data = await response.json();
    if (!response.ok || data?.error) throw new Error(data?.error || `HTTP ${response.status}`);
    return data;
  },
}));

vi.mock('../../settings/helpers', () => ({
  t: (key: string) => key,
}));

import { OAuthCredentials } from '../../settings/tabs/providers/OAuthCredentials';

function jsonResponse(body: unknown): Response {
  return {
    ok: true,
    status: 200,
    json: async () => body,
  } as Response;
}

const summary: ProviderSummary = {
  type: 'oauth',
  auth_type: 'oauth',
  display_name: 'ChatGPT Plus/Pro',
  base_url: '',
  api: 'openai-responses',
  api_key: '',
  models: [],
  custom_models: [],
  has_credentials: false,
  logged_in: false,
  supports_oauth: true,
  can_delete: false,
};

describe('OAuthCredentials', () => {
  beforeEach(() => {
    mocks.lingxiFetch.mockReset();
    mocks.openExternal.mockReset().mockResolvedValue(undefined);
    Object.defineProperty(window, 'platform', { configurable: true, value: { openExternal: mocks.openExternal } });
    mocks.lingxiFetch.mockResolvedValue(jsonResponse({
      sessionId: 'oauth-session-1',
      url: 'https://auth.example/start',
    }));
  });

  afterEach(() => {
    cleanup();
    if (originalPlatform) Object.defineProperty(window, 'platform', originalPlatform);
    else Reflect.deleteProperty(window, 'platform');
  });

  it('requests the explicit browser login method and keeps the callback session local', async () => {
    const onRefresh = vi.fn(async () => {});
    render(
      <OAuthCredentials
        providerId="openai-codex-oauth"
        summary={summary}
        onRefresh={onRefresh}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'settings.oauth.login' }));

    await waitFor(() => expect(mocks.lingxiFetch).toHaveBeenCalledTimes(1));
    const [startPath, startOptions] = mocks.lingxiFetch.mock.calls[0];
    expect(startPath).toBe('/api/auth/oauth/start');
    expect(JSON.parse(String((startOptions as RequestInit).body))).toEqual({
      provider: 'openai-codex-oauth',
      loginMethod: 'browser',
    });
    await waitFor(() => expect(mocks.openExternal).toHaveBeenCalledWith('https://auth.example/start'));

    mocks.lingxiFetch.mockResolvedValueOnce(jsonResponse({ ok: true }));
    fireEvent.change(await screen.findByRole('textbox'), { target: { value: 'manual-code' } });
    fireEvent.click(screen.getByRole('button', { name: 'settings.oauth.submit' }));

    await waitFor(() => expect(mocks.lingxiFetch).toHaveBeenCalledTimes(2));
    const [callbackPath, callbackOptions] = mocks.lingxiFetch.mock.calls[1];
    expect(callbackPath).toBe('/api/auth/oauth/callback');
    expect(JSON.parse(String((callbackOptions as RequestInit).body))).toEqual({
      sessionId: 'oauth-session-1',
      code: 'manual-code',
    });
  });
});
