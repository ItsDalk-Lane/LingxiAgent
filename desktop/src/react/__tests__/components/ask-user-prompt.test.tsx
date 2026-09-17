// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import React from 'react';
import { SessionConfirmationPrompt } from '../../components/input/SessionConfirmationPrompt';
import type { SessionConfirmationBlock } from '../../stores/chat-types';

const lingxiFetchMock = vi.fn<(path: string, opts?: RequestInit) => Promise<Response>>(
  async () => new Response('{}', { status: 200 }),
);

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: (path: string, opts?: RequestInit) => lingxiFetchMock(path, opts),
  lingxiUrl: (path: string) => `http://127.0.0.1:3210${path}`,
}));

function askUserBlock(questions: unknown[], status: 'pending' | 'confirmed' | 'timeout' | 'rejected' = 'pending'): SessionConfirmationBlock {
  return {
    type: 'session_confirmation',
    confirmId: 'confirm-1',
    kind: 'ask_user',
    surface: 'input',
    status,
    title: '助手想问你几个问题',
    severity: 'normal',
    actions: { confirmLabel: '提交', rejectLabel: '暂不回答' },
    payload: { questions },
  };
}

function radios(container: HTMLElement): HTMLInputElement[] {
  return Array.from(container.querySelectorAll('input[type="radio"]'));
}

function checkboxes(container: HTMLElement): HTMLInputElement[] {
  return Array.from(container.querySelectorAll('input[type="checkbox"]'));
}

function lastConfirmBody() {
  const call = lingxiFetchMock.mock.calls.at(-1);
  return JSON.parse(String(call?.[1]?.body));
}

describe('ask_user 提问卡', () => {
  beforeEach(() => {
    // 必须带花括号：mockClear 的返回值会被 vitest 4 当成收尾钩子登记
    lingxiFetchMock.mockClear();
  });

  afterEach(() => {
    cleanup();
  });

  it('渲染选项描述与推荐徽标，推荐项预选，改选后提交收集到的值', async () => {
    const { container } = render(
      <SessionConfirmationPrompt
        block={askUserBlock([
          {
            key: 'approach',
            question: '用哪种方案？',
            type: 'single',
            options: [
              { value: 'a', label: '方案 A', description: '稳妥但慢' },
              { value: 'b', label: '方案 B' },
            ],
            recommended: ['a'],
            required: true,
          },
          { key: 'note', question: '补充', type: 'text', options: [], recommended: [], required: false },
        ])}
      />,
    );

    expect(screen.getByText('稳妥但慢')).toBeTruthy();
    expect(screen.getByText('推荐')).toBeTruthy();

    const [radioA, radioB] = radios(container);
    expect(radioA.checked).toBe(true);
    expect(radioB.checked).toBe(false);

    fireEvent.click(radioB);
    fireEvent.click(screen.getByText('提交'));

    await waitFor(() => expect(lingxiFetchMock).toHaveBeenCalled());
    const [path] = lingxiFetchMock.mock.calls.at(-1)!;
    expect(path).toBe('/api/confirm/confirm-1');
    // 空的可选文本题不发送
    expect(lastConfirmBody()).toEqual({ action: 'confirmed', value: { approach: 'b' } });
  });

  it('必填未答拦截提交并提示，补上后才放行', async () => {
    const { container } = render(
      <SessionConfirmationPrompt
        block={askUserBlock([
          {
            key: 'pick',
            question: '选一个',
            type: 'single',
            options: [{ value: 'a', label: 'A' }, { value: 'b', label: 'B' }],
            recommended: [],
            required: true,
          },
        ])}
      />,
    );

    fireEvent.click(screen.getByText('提交'));
    expect(lingxiFetchMock).not.toHaveBeenCalled();
    expect(screen.getByTestId('ask-user-required')).toBeTruthy();

    fireEvent.click(radios(container)[0]);
    fireEvent.click(screen.getByText('提交'));
    await waitFor(() => expect(lingxiFetchMock).toHaveBeenCalled());
    expect(lastConfirmBody()).toEqual({ action: 'confirmed', value: { pick: 'a' } });
  });

  it('多选收集数组，取消勾选从数组移除', async () => {
    const { container } = render(
      <SessionConfirmationPrompt
        block={askUserBlock([
          {
            key: 'extras',
            question: '还要哪些？',
            type: 'multi',
            options: [
              { value: 'x', label: 'X' },
              { value: 'y', label: 'Y' },
            ],
            recommended: ['x'],
            required: false,
          },
        ])}
      />,
    );

    expect(screen.getByText('可多选')).toBeTruthy();
    const [boxX, boxY] = checkboxes(container);
    expect(boxX.checked).toBe(true);

    fireEvent.click(boxY);
    fireEvent.click(boxX); // 取消推荐项
    fireEvent.click(screen.getByText('提交'));

    await waitFor(() => expect(lingxiFetchMock).toHaveBeenCalled());
    expect(lastConfirmBody()).toEqual({ action: 'confirmed', value: { extras: ['y'] } });
  });

  it('已解决的提问卡按「回答」而非「同意」呈现', () => {
    const confirmed = render(<SessionConfirmationPrompt block={askUserBlock([], 'confirmed')} />);
    expect(screen.getByText('已回答')).toBeTruthy();
    confirmed.unmount();

    const timedOut = render(<SessionConfirmationPrompt block={askUserBlock([], 'timeout')} />);
    expect(screen.getByText('超时未答（已按推荐处理）')).toBeTruthy();
    timedOut.unmount();

    render(<SessionConfirmationPrompt block={askUserBlock([], 'rejected')} />);
    expect(screen.getByText('未回答')).toBeTruthy();
  });
});
