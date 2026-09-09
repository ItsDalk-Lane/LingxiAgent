// @vitest-environment jsdom

import React from 'react';
import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Toast } from '../Toast';
import { useSettingsStore } from '../store';

describe('设置操作反馈', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useSettingsStore.setState({ toastMessage: '', toastType: '', toastVisible: false });
  });

  afterEach(() => {
    cleanup();
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it('播报保存成功，并在提示结束后清除已过期的文字', () => {
    render(<Toast />);
    act(() => useSettingsStore.getState().showToast('已保存', 'success'));

    const status = screen.getByRole('status');
    expect(status.textContent).toBe('已保存');
    expect(status.getAttribute('aria-live')).toBe('polite');
    expect(status.getAttribute('aria-atomic')).toBe('true');
    expect(status.querySelector('svg')?.getAttribute('aria-hidden')).toBe('true');

    act(() => vi.advanceTimersByTime(2999));
    expect(status.textContent).toBe('已保存');
    act(() => vi.advanceTimersByTime(1));
    expect(status.textContent).toBe('');
  });

  it('把失败原因作为紧急提示播报，并替换之前的成功提示', () => {
    render(<Toast />);
    act(() => useSettingsStore.getState().showToast('已保存', 'success'));
    act(() => vi.advanceTimersByTime(2000));
    act(() => useSettingsStore.getState().showToast('保存失败：连接中断', 'error'));

    const alert = screen.getByRole('alert');
    expect(alert.textContent).toBe('保存失败：连接中断');
    expect(alert.getAttribute('aria-live')).toBe('assertive');
    expect(screen.queryByText('已保存')).toBeNull();
    act(() => vi.advanceTimersByTime(5999));
    expect(alert.textContent).toBe('保存失败：连接中断');
    act(() => vi.advanceTimersByTime(1));
    expect(alert.textContent).toBe('');
  });
});
