/** @vitest-environment jsdom */

import React from 'react';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const fixture = vi.hoisted(() => ({
  state: {
    wsState: 'reconnecting',
    wsReconnectAttempt: 2,
    wsFailureReasonKey: 'status.wsTicketFailed' as string | null,
    wsRecoveryNotice: false,
  },
}));

vi.mock('../../stores', () => {
  const useStore = (selector: (state: typeof fixture.state) => unknown) => selector(fixture.state);
  useStore.setState = (patch: Partial<typeof fixture.state>) => Object.assign(fixture.state, patch);
  return { useStore };
});
vi.mock('../../services/websocket', () => ({ manualReconnect: vi.fn() }));

import { StatusBar } from '../../components/StatusBar';
import { manualReconnect } from '../../services/websocket';

describe('R02 connection status visibility', () => {
  beforeEach(() => {
    vi.mocked(manualReconnect).mockClear();
    Object.assign(fixture.state, {
      wsState: 'reconnecting', wsReconnectAttempt: 2,
      wsFailureReasonKey: 'status.wsTicketFailed', wsRecoveryNotice: false,
    });
    vi.stubGlobal('t', (key: string) => key);
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('uses the manual retry path so a user click resets the reconnect delay', () => {
    Object.assign(fixture.state, {
      wsState: 'disconnected', wsReconnectAttempt: 21,
      wsFailureReasonKey: 'status.wsConnectionFailed', wsRecoveryNotice: false,
    });
    render(<StatusBar />);
    fireEvent.click(screen.getByRole('button', { name: 'status.reconnect' }));
    expect(manualReconnect).toHaveBeenCalledTimes(1);
  });

  it('shows the real failure category during retry and the successful recovery result', () => {
    vi.useFakeTimers();
    const view = render(<StatusBar />);
    expect(screen.getByText(/status.reconnecting/)).toBeInTheDocument();
    expect(screen.getByText('status.wsTicketFailed')).toBeInTheDocument();

    Object.assign(fixture.state, {
      wsState: 'connected', wsReconnectAttempt: 0,
      wsFailureReasonKey: null, wsRecoveryNotice: true,
    });
    view.rerender(<StatusBar />);
    expect(screen.getByText('status.reconnected')).toBeInTheDocument();
    expect(screen.queryByText('status.wsTicketFailed')).not.toBeInTheDocument();

    act(() => vi.advanceTimersByTime(5000));
    view.rerender(<StatusBar />);
    expect(screen.queryByText('status.reconnected')).not.toBeInTheDocument();
  });

  it('shows chat unavailability even after a Rust transport handshake', () => {
    Object.assign(fixture.state, {
      wsState: 'connected', wsReconnectAttempt: 0,
      wsFailureReasonKey: 'status.rustCoreUnavailable', wsRecoveryNotice: false,
    });
    render(<StatusBar />);
    expect(screen.getByText('status.rustCoreUnavailable')).toBeInTheDocument();
  });
});
