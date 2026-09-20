/**
 * @vitest-environment jsdom
 */
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const lingxiFetchMock = vi.fn(async (..._args: unknown[]) => ({ json: async () => ({}) }));

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: (...args: unknown[]) => lingxiFetchMock(...args),
  lingxiUrl: (p: string) => p,
}));

import { BrowserCard } from '../../components/BrowserCard';
import { useStore } from '../../stores';
import { setBrowserStateForPath } from '../../stores/browser-slice';
import { handleServerMessage } from '../../services/ws-message-handler';

const SESSION_PATH = '/tmp/agents/hana/sessions/browser-card.jsonl';

const browserEmergencyStopMock = vi.fn();
const openBrowserViewerMock = vi.fn();

function browserStatus(running: boolean) {
  handleServerMessage({
    type: 'browser_status',
    sessionPath: SESSION_PATH,
    running,
    url: running ? 'https://example.com' : null,
  });
}

function card() {
  return document.getElementById('browserFloatingCard');
}

describe('BrowserCard collapse semantics', () => {
  beforeEach(() => {
    globalThis.t = ((key: string) => key) as typeof globalThis.t;
    lingxiFetchMock.mockClear();
    browserEmergencyStopMock.mockClear();
    openBrowserViewerMock.mockClear();
    Object.defineProperty(window, 'platform', {
      configurable: true,
      value: {
        browserEmergencyStop: browserEmergencyStopMock,
        openBrowserViewer: openBrowserViewerMock,
      },
    });
    useStore.setState({
      currentSessionPath: SESSION_PATH,
      currentSessionId: null,
      sessions: [],
      sessionLocatorsById: {},
      browserBySession: {},
    } as never);
  });

  afterEach(() => {
    cleanup();
  });

  it('collapses the card locally without stopping or closing the session browser', () => {
    act(() => {
      setBrowserStateForPath(SESSION_PATH, { running: true, url: 'https://example.com', thumbnail: null });
    });

    render(<BrowserCard />);
    expect(card()).toBeInTheDocument();

    fireEvent.click(screen.getByTitle('browser.collapse'));

    expect(lingxiFetchMock).not.toHaveBeenCalled();
    expect(browserEmergencyStopMock).not.toHaveBeenCalled();
    expect(openBrowserViewerMock).not.toHaveBeenCalled();
    expect(card()).not.toBeInTheDocument();
    expect(useStore.getState().browserBySession[SESSION_PATH].running).toBe(true);
  });

  it('keeps the card collapsed while the browser keeps reporting status', () => {
    act(() => {
      setBrowserStateForPath(SESSION_PATH, { running: true, url: 'https://example.com', thumbnail: null });
    });
    render(<BrowserCard />);
    fireEvent.click(screen.getByTitle('browser.collapse'));
    expect(card()).not.toBeInTheDocument();

    act(() => { browserStatus(true); });

    expect(card()).not.toBeInTheDocument();
  });

  it('brings the card back when the browser restarts for that session', () => {
    act(() => {
      setBrowserStateForPath(SESSION_PATH, { running: true, url: 'https://example.com', thumbnail: null });
    });
    render(<BrowserCard />);
    fireEvent.click(screen.getByTitle('browser.collapse'));
    expect(card()).not.toBeInTheDocument();

    act(() => { browserStatus(false); });
    expect(card()).not.toBeInTheDocument();

    act(() => { browserStatus(true); });

    expect(card()).toBeInTheDocument();
  });
});

describe('BrowserCard cross-session fallback', () => {
  const OTHER_PATH = '/tmp/agents/hana/sessions/other.jsonl';
  const THIRD_PATH = '/tmp/agents/hana/sessions/third.jsonl';

  beforeEach(() => {
    globalThis.t = ((key: string) => key) as typeof globalThis.t;
    lingxiFetchMock.mockClear();
    browserEmergencyStopMock.mockClear();
    openBrowserViewerMock.mockClear();
    Object.defineProperty(window, 'platform', {
      configurable: true,
      value: {
        browserEmergencyStop: browserEmergencyStopMock,
        openBrowserViewer: openBrowserViewerMock,
      },
    });
    useStore.setState({
      currentSessionPath: SESSION_PATH,
      currentSessionId: null,
      sessions: [],
      sessionLocatorsById: {},
      browserBySession: {},
    } as never);
  });

  afterEach(() => {
    cleanup();
  });

  it('shows the most recently active foreign browser when the current session has none', () => {
    act(() => {
      setBrowserStateForPath(OTHER_PATH, { running: true, url: 'https://other.example.com', thumbnail: null, lastActiveAt: 100 });
      setBrowserStateForPath(THIRD_PATH, { running: true, url: 'https://third.example.com', thumbnail: null, lastActiveAt: 200 });
    });

    render(<BrowserCard />);
    expect(card()).toBeInTheDocument();
    expect(card()!.textContent).toContain('third.example.com');
  });

  it('opens the owner session\'s browser viewer when clicking the fallback card', () => {
    act(() => {
      setBrowserStateForPath(OTHER_PATH, { running: true, url: 'https://other.example.com', thumbnail: null, lastActiveAt: 100 });
    });

    render(<BrowserCard />);
    fireEvent.click(card()!);

    expect(openBrowserViewerMock).toHaveBeenCalledWith({ sessionPath: OTHER_PATH });
  });

  it('collapses the foreign card on the owner session, not the current one', () => {
    act(() => {
      setBrowserStateForPath(OTHER_PATH, { running: true, url: 'https://other.example.com', thumbnail: null, lastActiveAt: 100 });
    });

    render(<BrowserCard />);
    fireEvent.click(screen.getByTitle('browser.collapse'));
    expect(card()).not.toBeInTheDocument();
    expect(useStore.getState().browserBySession[OTHER_PATH].collapsed).toBe(true);

    // 回到归属会话，卡片也保持收起
    act(() => {
      useStore.setState({ currentSessionPath: OTHER_PATH } as never);
    });
    expect(card()).not.toBeInTheDocument();
  });

  it('prefers the current session\'s own running browser over foreign ones', () => {
    act(() => {
      setBrowserStateForPath(OTHER_PATH, { running: true, url: 'https://other.example.com', thumbnail: null, lastActiveAt: 200 });
      setBrowserStateForPath(SESSION_PATH, { running: true, url: 'https://own.example.com', thumbnail: null, lastActiveAt: 100 });
    });

    render(<BrowserCard />);
    expect(card()).toBeInTheDocument();
    expect(card()!.textContent).toContain('own.example.com');
  });

  it('hides the fallback card once the foreign browser stops', () => {
    act(() => {
      setBrowserStateForPath(OTHER_PATH, { running: true, url: 'https://other.example.com', thumbnail: null, lastActiveAt: 100 });
    });

    render(<BrowserCard />);
    expect(card()).toBeInTheDocument();

    act(() => {
      handleServerMessage({
        type: 'browser_status',
        sessionPath: OTHER_PATH,
        running: false,
        url: null,
      });
    });
    expect(card()).not.toBeInTheDocument();
  });
});
