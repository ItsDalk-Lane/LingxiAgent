/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { useSettingsStore } from '../../settings/store';

const mocks = vi.hoisted(() => ({
  loadLlmUsageEntries: vi.fn(),
}));

vi.mock('../../settings/tabs/providers/usage-ledger-actions', () => ({
  loadLlmUsageEntries: (...args: unknown[]) => mocks.loadLlmUsageEntries(...args),
}));

vi.mock('../../settings/actions', () => ({
  loadAgents: vi.fn(async () => {}),
  loadAvatars: vi.fn(async () => {}),
  loadSettingsConfig: vi.fn(async () => {}),
  loadSettingsSnapshot: vi.fn(async () => {}),
  loadSettingsModels: vi.fn(async () => {}),
  loadPluginSettings: vi.fn(async () => {}),
}));

vi.mock('../../settings/api', () => ({
  lingxiFetch: vi.fn(async (url: string) => new Response(JSON.stringify(
    url === '/api/config' ? { locale: 'zh-CN' } : {},
  ))),
  lingxiFetchJson: vi.fn(async () => ({})),
}));

import { SettingsNav } from '../../settings/SettingsNav';
import { SettingsContent } from '../../settings/SettingsContent';
import { UsageTab } from '../../settings/tabs/UsageTab';
import { formatNumber } from '../../settings/tabs/providers/usage-ledger-model';

describe('UsageTab settings page registration', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.loadLlmUsageEntries.mockResolvedValue([]);
    window.t = ((key: string) => key) as typeof window.t;
    window.i18n = {
      locale: 'zh-CN',
      defaultName: 'Hana',
      _data: {},
      _agentOverrides: {},
      load: vi.fn(async () => {}),
      setAgentOverrides: vi.fn(),
      t: ((key: string) => key) as typeof window.t,
    };
    window.platform = {
      getServerPort: vi.fn(async () => 3000),
      getServerToken: vi.fn(async () => null),
      getPlatform: vi.fn(async () => 'darwin'),
      onSwitchTab: vi.fn(),
      onSettingsChanged: vi.fn(() => vi.fn()),
      onServerRestarted: vi.fn(),
    } as unknown as typeof window.platform;
    useSettingsStore.setState({
      activeTab: 'usage',
      platformName: 'darwin',
      ready: true,
    } as never);
  });

  afterEach(() => {
    cleanup();
  });

  it('places the usage nav item strictly between providers and media', () => {
    render(<SettingsNav />);

    const tabIds = [...document.querySelectorAll('button[data-tab]')].map(
      el => el.getAttribute('data-tab'),
    );
    const providersIndex = tabIds.indexOf('providers');
    const usageIndex = tabIds.indexOf('usage');
    const mediaIndex = tabIds.indexOf('media');

    expect(providersIndex).toBeGreaterThanOrEqual(0);
    expect(mediaIndex).toBeGreaterThan(providersIndex);
    expect(usageIndex).toBe(providersIndex + 1);
    expect(mediaIndex).toBe(usageIndex + 1);

    const usageButton = document.querySelector('button[data-tab="usage"]') as HTMLButtonElement;
    expect(usageButton.textContent).toContain('settings.tabs.usage');
    expect(usageButton.querySelector('svg')).not.toBeNull();
  });

  it('renders the usage tab content through SettingsContent when usage is active', async () => {
    const { container } = render(<SettingsContent variant="window" />);

    await waitFor(() => {
      expect(container.querySelector('div[data-tab="usage"]')).not.toBeNull();
    });
    // 页标题复用既有 settings.usage.title
    expect(screen.getAllByText('settings.usage.title').length).toBeGreaterThan(0);
    // 导航中 usage 依旧夹在 providers 与 media 之间（TAB_COMPONENTS 注册成功才会渲染 UsageTab）
    const tabIds = [...container.querySelectorAll('button[data-tab]')].map(
      el => el.getAttribute('data-tab'),
    );
    expect(tabIds.indexOf('usage')).toBe(tabIds.indexOf('providers') + 1);
    expect(tabIds.indexOf('media')).toBe(tabIds.indexOf('usage') + 1);
  });
});

const USAGE_ENTRIES = [
  {
    requestId: 'req-1',
    startedAt: '2026-05-25T00:00:00.000Z',
    endedAt: '2026-05-25T00:00:01.000Z',
    durationMs: 1000,
    status: 'ok',
    source: { subsystem: 'session', operation: 'reply' },
    attribution: { kind: 'session', agentId: 'hana', sessionPath: '/s/a.jsonl' },
    model: { provider: 'openai', modelId: 'gpt-5', api: 'openai-responses' },
    usage: {
      input: { totalTokens: 100, uncachedTokens: 40 },
      output: { totalTokens: 25 },
      cache: { readTokens: 60, hit: true },
      totalTokens: 1250,
      costTotal: 0.001,
    },
    error: null,
  },
];

describe('usage charts cursor tip', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.loadLlmUsageEntries.mockResolvedValue(USAGE_ENTRIES);
    window.t = ((key: string) => key) as typeof window.t;
    window.i18n = {
      locale: 'zh-CN',
      defaultName: 'Hana',
      _data: {},
      _agentOverrides: {},
      load: vi.fn(async () => {}),
      setAgentOverrides: vi.fn(),
      t: ((key: string) => key) as typeof window.t,
    };
  });

  afterEach(() => {
    cleanup();
  });

  it('shows a cursor-following tip with core metrics on ring segments and hides it on leave', async () => {
    const { container } = render(<UsageTab />);
    // overall 视图默认渲染 ModelOrbit 环段
    await waitFor(() => {
      expect(container.querySelector('g[data-usage-tip]')).not.toBeNull();
    });

    const segment = container.querySelector('g[data-usage-tip]') as SVGGElement;
    expect(document.querySelector('[data-testid="usage-cursor-tip"]')).toBeNull();

    fireEvent.pointerEnter(segment, { clientX: 40, clientY: 40 });
    fireEvent.pointerMove(segment, { clientX: 48, clientY: 52 });

    const tips = document.querySelectorAll('[data-testid="usage-cursor-tip"]');
    expect(tips).toHaveLength(1);
    const tip = tips[0];
    expect(tip.textContent).toContain('settings.usage.totalTokens');
    expect(tip.textContent).toContain(formatNumber(1250));
    expect(tip.textContent).toContain('settings.usage.cacheRead');
    expect(tip.textContent).toContain(formatNumber(60));
    expect(tip.textContent).toContain('settings.usage.uncached');
    expect(tip.textContent).toContain('settings.usage.requests');
    expect(tip.textContent).toContain('settings.usage.cacheHitRate');
    expect(tip.textContent).toContain('100%');

    fireEvent.pointerLeave(segment);
    expect(document.querySelector('[data-testid="usage-cursor-tip"]')).toBeNull();
  });

  it('shows a cursor-following tip on daily bars, with aria-label instead of native title', async () => {
    const { container } = render(<UsageTab />);
    fireEvent.click(await screen.findByRole('tab', { name: 'settings.usage.view.daily' }));

    await waitFor(() => {
      expect(container.querySelector('div[data-usage-tip]')).not.toBeNull();
    });
    const days = [...container.querySelectorAll('div[data-usage-tip]')] as HTMLDivElement[];
    expect(days.length).toBe(7);

    // ③ 每根柱（.usage-day，以 data-usage-tip 标记）无 title、有等价 aria-label
    for (const day of days) {
      expect(day).not.toHaveAttribute('title');
      expect(day.getAttribute('aria-label')).toMatch(/·/);
    }

    const activeDay = days.find(day => day.getAttribute('aria-label')?.includes(formatNumber(1250)));
    expect(activeDay).toBeDefined();

    fireEvent.pointerEnter(activeDay as HTMLDivElement, { clientX: 10, clientY: 10 });
    fireEvent.pointerMove(activeDay as HTMLDivElement, { clientX: 20, clientY: 24 });

    const tips = document.querySelectorAll('[data-testid="usage-cursor-tip"]');
    expect(tips).toHaveLength(1);
    expect(tips[0].textContent).toContain(formatNumber(1250));
    expect(tips[0].textContent).toContain('100%');

    fireEvent.pointerLeave(activeDay as HTMLDivElement);
    expect(document.querySelector('[data-testid="usage-cursor-tip"]')).toBeNull();
  });
});
