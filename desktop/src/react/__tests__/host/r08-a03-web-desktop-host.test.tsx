/**
 * @vitest-environment jsdom
 *
 * R08-A03 验收：纯浏览器环境（不注入任何 Electron API）跑通
 * 会话 / 设置 / 文件预览三条链，受限能力给出显式、可解释的消息，
 * 而不是 TypeError / 空对象爆雷。
 *
 * 纪律：本文件全程不设置 window.hana；window.platform 只允许由
 * DesktopHost 惰性安装产生（getDesktopHost 写别名），测试不手挂。
 */
import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';

const storeMock = vi.hoisted(() => ({
  state: {} as Record<string, unknown>,
}));

vi.mock('../../stores', () => ({
  useStore: {
    getState: () => storeMock.state,
  },
}));

vi.mock('../../settings/api', () => ({
  lingxiFetch: vi.fn(async () => new Response(JSON.stringify({}), { status: 200 })),
  lingxiFetchJson: vi.fn(async () => ({})),
}));

vi.mock('../../settings/actions', () => ({
  loadAgents: vi.fn(async () => {}),
  loadAvatars: vi.fn(async () => {}),
  loadSettingsConfig: vi.fn(async () => {}),
  loadSettingsSnapshot: vi.fn(async () => {}),
  loadSettingsModels: vi.fn(async () => {}),
  loadPluginSettings: vi.fn(async () => {}),
  updateSettingsSnapshot: vi.fn(async () => {}),
}));

import {
  explainHostCapability,
  getDesktopHost,
  resetDesktopHostForTests,
} from '../../host/desktop-host';
import { usePlatform } from '../../hooks/use-platform';
import { lingxiFetch } from '../../services/service-client';
import { takeScreenshot } from '../../utils/screenshot';
import { GeneralTab } from '../../settings/tabs/GeneralTab';

function seedBaseStore(): void {
  storeMock.state = {
    homeFolder: '/tmp/hana-home',
    chatSessions: {},
    selectedIdsBySession: {},
    currentAgentId: null,
    agentName: 'Hana',
    userName: '我',
    beginScreenshotTask: vi.fn(),
    updateScreenshotProgress: vi.fn(),
    endScreenshotTask: vi.fn(),
  };
}

function expectNoElectronBridge(): void {
  expect((window as { hana?: unknown }).hana).toBeUndefined();
}

describe('R08-A03：无 Electron API 的 Web 宿主链', () => {
  beforeEach(() => {
    resetDesktopHostForTests();
    delete (window as { hana?: unknown }).hana;
    seedBaseStore();
    const storage = new Map<string, string>();
    vi.stubGlobal('localStorage', {
      getItem: vi.fn((key: string) => storage.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => storage.set(key, value)),
      removeItem: vi.fn((key: string) => storage.delete(key)),
      clear: vi.fn(() => storage.clear()),
    });
    window.i18n = { locale: 'zh' } as typeof window.i18n;
  });

  afterEach(() => {
    cleanup();
    resetDesktopHostForTests();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('宿主惰性解析为 web，受限能力逐一可解释，且全程无 Electron 桥', () => {
    expectNoElectronBridge();
    const host = getDesktopHost();
    expect(host.kind).toBe('web');
    // 别名由宿主安装产生，非测试手挂；usePlatform 即宿主 api，永不为 undefined。
    expect(window.platform).toBe(host.api);
    expect(usePlatform()).toBe(host.api);
    // 桥身份未变时重复解析返回同一宿主（生产环境等价只解析一次）。
    expect(getDesktopHost()).toBe(host);

    // 显式 no-op 成员：登记过的理由原样返回。
    const selectReason = explainHostCapability('selectFolder');
    expect(selectReason).toBeTruthy();
    expect(selectReason).toContain('目录选择器');
    // 宿主未提供的可选成员：宿主级可解释说明，而非 undefined 直接爆雷。
    const screenshotReason = explainHostCapability('screenshotRender');
    expect(screenshotReason).toBe('当前宿主（web）未提供该能力');
    const connectionInfoReason = explainHostCapability('getServerConnectionInfo');
    expect(connectionInfoReason).toBe('当前宿主（web）未提供该能力');
    // 真实具备的能力不附理由。
    expect(explainHostCapability('readFile')).toBeNull();
    expectNoElectronBridge();
  });

  it('截图链：无 screenshotRender 能力时给出显式内联提示而非 TypeError', async () => {
    expectNoElectronBridge();
    const notices: Array<{ text: string; type: string }> = [];
    const onNotice = (event: Event) => notices.push((event as CustomEvent).detail);
    window.addEventListener('hana-inline-notice', onNotice);
    try {
      const sessionPath = '/session/a03.jsonl';
      storeMock.state = {
        ...storeMock.state,
        selectedIdsBySession: { [sessionPath]: ['u1'] },
        chatSessions: {
          [sessionPath]: {
            hasMore: false,
            loadingMore: false,
            items: [
              { type: 'message', data: { id: 'u1', role: 'user', text: '在吗' } },
            ],
          },
        },
      };

      // 关键断言：不抛 TypeError，走显式受限提示路径。
      await expect(takeScreenshot('u1', sessionPath)).resolves.toBeUndefined();
      expect(notices).toEqual([{ text: 'common.screenshotFailed', type: 'error' }]);
      expect(storeMock.state.beginScreenshotTask).not.toHaveBeenCalled();
      expectNoElectronBridge();
    } finally {
      window.removeEventListener('hana-inline-notice', onNotice);
    }
  });

  it('会话链：lingxiFetch 经 ServiceClient 路由到 /lingxi/v1 会话读端口，无需 Electron 桥', async () => {
    expectNoElectronBridge();
    storeMock.state = {
      ...storeMock.state,
      serverPort: 3210,
      serverToken: 'a03-token',
      serverNodeKind: 'lingxi-service',
      serverNodeTransport: 'http',
    };
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ sessions: [] }), { status: 200 }));
    vi.stubGlobal('fetch', fetchMock);

    const res = await lingxiFetch('/api/sessions/search');
    expect(res.status).toBe(200);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toContain('/lingxi/v1/sessions/search');
    // lingxi-service 连接走 Authorization 头认证（query token 仅限 loopback_token）。
    expect((init?.headers as Record<string, string>)?.Authorization).toBe('Bearer a03-token');
    expectNoElectronBridge();
  });

  it('设置链：GeneralTab 在无桥环境渲染，getAutoLaunchStatus 缺席显式可解释', async () => {
    expectNoElectronBridge();
    // Web 宿主不提供 getAutoLaunchStatus：渲染不得崩，且理由可解释。
    expect(usePlatform().getAutoLaunchStatus).toBeUndefined();
    expect(explainHostCapability('getAutoLaunchStatus')).toBe('当前宿主（web）未提供该能力');

    const { container } = render(<GeneralTab />);
    await waitFor(() => {
      expect(container.firstElementChild).not.toBeNull();
    });
    // 能力缺席时设置页保持存活（不显式渲染不可用的开关，也不抛错）。
    expectNoElectronBridge();
  });

  it('文件预览链：HTTP 读取 404→null 显式未找到，其余非 ok 抛带状态错误', async () => {
    expectNoElectronBridge();
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes('/api/fs/read?')) {
        return new Response('not found', { status: 404 });
      }
      if (url.includes('/api/fs/docx-html')) {
        return new Response('<p>预览</p>', { status: 200 });
      }
      return new Response('boom', { status: 500 });
    });
    vi.stubGlobal('fetch', fetchMock);
    // 默认 jsdom location：localhost:3000 → Web 宿主同源裁决。
    const api = usePlatform();

    // 404 → null（显式「未找到」），不是抛错也不是 TypeError。
    await expect(api.readFile('/tmp/missing.md')).resolves.toBeNull();
    // 200 → 正常读取（预览 HTML 链）。
    await expect(api.readDocxHtml('/tmp/demo.docx')).resolves.toBe('<p>预览</p>');
    // 500 → 显式带状态错误消息。
    await expect(api.readFileBase64('/tmp/broken.bin')).rejects.toThrow('HTTP 500');
    // 无 Authorization 头（无 token）。
    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect((init.headers as Record<string, string>).Authorization).toBeUndefined();
    expectNoElectronBridge();
  });
});
