// @vitest-environment jsdom

import fs from 'node:fs';
import path from 'node:path';
import React from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SharingTab } from '../tabs/SharingTab';
import { SettingsContent } from '../SettingsContent';
import { useSettingsStore } from '../store';
import { READING_FONT_PRESETS, SCREENSHOT_FONT_STORAGE_KEY } from '../../utils/font-presets';
import {
  SCREENSHOT_SEGMENT_VISIBLE_CHAR_LIMIT,
  SCREENSHOT_SEGMENT_VISIBLE_CHAR_LIMIT_STORAGE_KEY,
} from '../../utils/screenshot-segments';

const actionMocks = vi.hoisted(() => ({
  loadAgents: vi.fn(async () => {}),
  loadAvatars: vi.fn(async () => {}),
  loadSettingsSnapshot: vi.fn(async () => {}),
  loadSettingsModels: vi.fn(async () => {}),
  loadProvidersSummary: vi.fn(async () => {}),
}));

vi.mock('../actions', () => ({
  ...actionMocks,
  loadSettingsConfig: vi.fn(async () => {}),
  loadPluginSettings: vi.fn(async () => {}),
  updateSettingsSnapshot: vi.fn(),
}));
vi.mock('../api', () => ({
  lingxiFetch: vi.fn(async () => new Response(JSON.stringify({ locale: 'zh-CN' }))),
}));

type CaseRecord = {
  case: string;
  expect: number;
  actual: number;
  ok: boolean;
  observed: Record<string, unknown>;
};
const recorded: CaseRecord[] = [];
const containerBranches: Record<string, unknown> = {};

function saveCase(name: string, observed: Record<string, unknown>) {
  recorded.push({ case: name, expect: 1, actual: 1, ok: true, observed });
}

const storage = new Map<string, string>();
let denyWrite = false;

function setStorage() {
  vi.stubGlobal('localStorage', {
    getItem: vi.fn((key: string) => storage.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => {
      if (denyWrite) throw new Error('storage denied');
      storage.set(key, value);
    }),
    removeItem: vi.fn((key: string) => {
      if (denyWrite) throw new Error('storage denied');
      storage.delete(key);
    }),
    clear: vi.fn(() => storage.clear()),
  });
}

function buttonContaining(text: string) {
  const button = screen.getByText(text).closest('button');
  expect(button, `button containing ${text}`).toBeTruthy();
  return button as HTMLButtonElement;
}

function selected(button: HTMLElement) {
  return button.className.includes('active');
}

function expectStorageDenial(action: () => void) {
  // React 19 + jsdom 把事件处理器里的同步异常送到 window.error，
  // fireEvent 自身不会抛出；捕获真实错误并阻止它污染 Vitest 全局队列。
  const errors: string[] = [];
  const onError = (event: ErrorEvent) => {
    if (event.error?.message !== 'storage denied') return;
    errors.push(event.error.message);
    event.preventDefault();
  };
  window.addEventListener('error', onError);
  try {
    action();
  } finally {
    window.removeEventListener('error', onError);
  }
  expect(errors).toEqual(['storage denied']);
}

describe('R02/R00 sharing leaf: real component and storage behavior', () => {
  beforeEach(() => {
    storage.clear();
    denyWrite = false;
    setStorage();
    actionMocks.loadAgents.mockReset().mockResolvedValue(undefined);
    actionMocks.loadAvatars.mockReset().mockResolvedValue(undefined);
    actionMocks.loadSettingsSnapshot.mockReset().mockResolvedValue(undefined);
    actionMocks.loadSettingsModels.mockReset().mockResolvedValue(undefined);
    actionMocks.loadProvidersSummary.mockReset().mockResolvedValue(undefined);
    window.t = ((key: string) => key) as typeof window.t;
    window.i18n = {
      locale: 'zh-CN', defaultName: 'Hana', _data: {}, _agentOverrides: {},
      load: vi.fn(async () => {}), setAgentOverrides: vi.fn(),
      t: ((key: string) => key) as typeof window.t,
    };
    window.platform = {
      getServerPort: vi.fn(async () => 3000),
      getServerToken: vi.fn(async () => null),
      getPlatform: vi.fn(async () => 'darwin'),
    } as unknown as typeof window.platform;
    useSettingsStore.setState({
      activeTab: 'sharing', settingsConfig: null, ready: true,
      serverPort: null, serverToken: null,
      serverConnections: {}, activeServerConnection: null,
      activeServerConnectionId: null,
    } as never);
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('renders the cold mask, then exposes the page after the actual loaders finish', async () => {
    let releaseAgents: (() => void) | undefined;
    actionMocks.loadAgents.mockImplementation(() => new Promise<void>(resolve => {
      releaseAgents = resolve;
    }));
    render(<SettingsContent variant="window" />);
    expect(useSettingsStore.getState().ready).toBe(false);
    expect(document.querySelector('.settings-loading-mask')).toBeTruthy();
    await waitFor(() => expect(releaseAgents).toBeTypeOf('function'));
    releaseAgents?.();
    await waitFor(() => expect(useSettingsStore.getState().ready).toBe(true));
    expect(screen.getByText('settings.screenshot.color')).toBeTruthy();
    containerBranches.cold = { maskBefore: true, readyAfter: true };
  });

  it('keeps cached settings visible during background refresh', async () => {
    useSettingsStore.setState({ settingsConfig: { existing: 'cached' }, ready: true } as never);
    let releaseAgents: (() => void) | undefined;
    actionMocks.loadAgents.mockImplementation(() => new Promise<void>(resolve => {
      releaseAgents = resolve;
    }));
    render(<SettingsContent variant="window" />);
    expect(document.querySelector('.settings-loading-mask')).toBeNull();
    expect(screen.getByText('settings.screenshot.color')).toBeTruthy();
    await waitFor(() => expect(releaseAgents).toBeTypeOf('function'));
    releaseAgents?.();
    await waitFor(() => expect(actionMocks.loadProvidersSummary).toHaveBeenCalled());
    containerBranches.cached = { oldContentVisible: true, backgroundRefresh: true };
  });

  it('uses the 15-second escape hatch for an unresolved connection', () => {
    vi.useFakeTimers();
    window.platform = {
      getServerPort: vi.fn(() => new Promise<number>(() => {})),
      getServerToken: vi.fn(async () => null),
      getPlatform: vi.fn(async () => 'darwin'),
    } as unknown as typeof window.platform;
    render(<SettingsContent variant="window" />);
    expect(useSettingsStore.getState().ready).toBe(false);
    act(() => vi.advanceTimersByTime(15_000));
    expect(useSettingsStore.getState().ready).toBe(true);
    containerBranches.timeout = { waitedMs: 15_000, forcedReady: true };
  });

  it('logs init errors but unblocks the page; provider summary failure does not block ready', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    window.platform = {
      getServerPort: vi.fn(async () => { throw new Error('test port failure'); }),
      getServerToken: vi.fn(async () => null),
      getPlatform: vi.fn(async () => 'darwin'),
    } as unknown as typeof window.platform;
    render(<SettingsContent variant="window" />);
    await waitFor(() => expect(useSettingsStore.getState().ready).toBe(true));
    expect(error).toHaveBeenCalled();
    containerBranches.error = { logged: true, ready: true };
    cleanup();

    useSettingsStore.setState({ ready: false, settingsConfig: null } as never);
    window.platform = {
      getServerPort: vi.fn(async () => 3000),
      getServerToken: vi.fn(async () => null),
      getPlatform: vi.fn(async () => 'darwin'),
    } as unknown as typeof window.platform;
    actionMocks.loadProvidersSummary.mockRejectedValue(new Error('test summary failure'));
    render(<SettingsContent variant="window" />);
    await waitFor(() => expect(actionMocks.loadProvidersSummary).toHaveBeenCalled());
    expect(useSettingsStore.getState().ready).toBe(true);
    containerBranches.providers = { failureDidNotBlockReady: true };
  });

  it('persists color and preview, defaults light, and throws without a false success on denied storage', () => {
    render(<SharingTab />);
    const light = buttonContaining('settings.screenshot.light');
    const dark = buttonContaining('settings.screenshot.dark');
    const sakura = buttonContaining('settings.screenshot.sakura');
    expect(selected(light)).toBe(true);
    const before = screen.getByAltText('settings.screenshot.mobileTitle').getAttribute('src');
    fireEvent.click(dark);
    const after = screen.getByAltText('settings.screenshot.mobileTitle').getAttribute('src');
    expect(storage.get('hana-screenshot-color')).toBe('dark');
    expect(selected(dark)).toBe(true);
    expect(after).not.toBe(before);
    denyWrite = true;
    expectStorageDenial(() => fireEvent.click(sakura));
    expect(storage.get('hana-screenshot-color')).toBe('dark');
    expect(selected(dark)).toBe(true);
    saveCase('sharing-color-success-default-denied', {
      defaultColor: 'light', persistedColor: storage.get('hana-screenshot-color'),
      previewChanged: before !== after, deniedThrows: true, selectedAfterDenial: 'dark',
    });
  });

  it('persists width and preview, defaults mobile, and keeps the old UI on denied storage', () => {
    render(<SharingTab />);
    const mobile = buttonContaining('settings.screenshot.mobileTitle');
    const desktop = buttonContaining('settings.screenshot.desktopTitle');
    expect(selected(mobile)).toBe(true);
    const before = screen.getByAltText('settings.screenshot.mobileTitle').getAttribute('src');
    fireEvent.click(desktop);
    expect(storage.get('hana-screenshot-width')).toBe('desktop');
    expect(selected(desktop)).toBe(true);
    const after = screen.getByAltText('settings.screenshot.desktopTitle').getAttribute('src');
    expect(after).not.toBe(before);
    denyWrite = true;
    expectStorageDenial(() => fireEvent.click(mobile));
    expect(storage.get('hana-screenshot-width')).toBe('desktop');
    expect(selected(desktop)).toBe(true);
    saveCase('sharing-width-success-default-denied', {
      defaultWidth: 'mobile', persistedWidth: storage.get('hana-screenshot-width'),
      previewChanged: before !== after, deniedThrows: true, selectedAfterDenial: 'desktop',
    });
  });

  it('normalizes font and segment limit, persists both, then rejects denied writes without UI drift', () => {
    render(<SharingTab />);
    const preset = READING_FONT_PRESETS[0];
    expect(preset).toBeTruthy();
    const follow = screen.getByTitle('settings.fonts.followReading');
    fireEvent.click(follow);
    fireEvent.click(screen.getByRole('option', { name: preset.labelKey }));
    expect(storage.get(SCREENSHOT_FONT_STORAGE_KEY)).toBe(preset.id);
    expect(screen.getByTitle(preset.labelKey)).toBeTruthy();

    const number = document.querySelector('input[type="number"]') as HTMLInputElement;
    expect(Number(number.value)).toBe(SCREENSHOT_SEGMENT_VISIBLE_CHAR_LIMIT);
    fireEvent.change(number, { target: { value: '1500' } });
    expect(storage.get(SCREENSHOT_SEGMENT_VISIBLE_CHAR_LIMIT_STORAGE_KEY)).toBe('1500');
    expect(number.value).toBe('1500');
    denyWrite = true;
    expectStorageDenial(() => fireEvent.change(number, { target: { value: '200' } }));
    expect(number.value).toBe('1500');
    expect(storage.get(SCREENSHOT_SEGMENT_VISIBLE_CHAR_LIMIT_STORAGE_KEY)).toBe('1500');
    fireEvent.click(screen.getByTitle(preset.labelKey));
    expectStorageDenial(() =>
      fireEvent.click(screen.getByRole('option', { name: 'settings.fonts.followReading' })));
    expect(screen.getByTitle(preset.labelKey)).toBeTruthy();
    expect(storage.get(SCREENSHOT_FONT_STORAGE_KEY)).toBe(preset.id);
    saveCase('sharing-font-limit-success-default-denied', {
      defaultFont: 'settings.fonts.followReading', persistedFont: preset.id,
      defaultLimit: SCREENSHOT_SEGMENT_VISIBLE_CHAR_LIMIT,
      persistedLimit: storage.get(SCREENSHOT_SEGMENT_VISIBLE_CHAR_LIMIT_STORAGE_KEY),
      deniedThrows: true, visibleLimitAfterDenial: number.value,
    });
  });

  afterAll(() => {
    const branches = ['cold', 'cached', 'timeout', 'error', 'providers'];
    const allContainerBranches = branches.every(name => Object.hasOwn(containerBranches, name));
    recorded.push({
      case: 'sharing-container-cold-cache-timeout-error',
      expect: 1,
      actual: allContainerBranches ? 1 : 0,
      ok: allContainerBranches,
      observed: { ...containerBranches },
    });
    const output = process.env.R02_CLIENT_SHARING_EVIDENCE;
    if (!output) return;
    fs.mkdirSync(path.dirname(output), { recursive: true });
    fs.writeFileSync(output, JSON.stringify({
      schema: 'lingxi.leaf-case-results.v1',
      cases: recorded,
    }, null, 2));
  });
});
