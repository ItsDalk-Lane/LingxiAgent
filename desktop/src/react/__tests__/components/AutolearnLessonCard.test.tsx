// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AutolearnLessonCard } from '../../components/chat/AutolearnLessonCard';
import { useStore } from '../../stores';

const lingxiFetchMock = vi.fn<(path: string, opts?: any) => Promise<Response>>(
  async () => new Response(JSON.stringify({ ok: true }), { status: 200 }),
);

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: (path: string, opts?: any) => lingxiFetchMock(path, opts),
  lingxiUrl: (path: string) => `http://127.0.0.1:3210${path}`,
}));

function lessonBlock(overrides: Record<string, unknown> = {}) {
  return {
    type: 'suggestion_card',
    kind: 'autolearn_lesson',
    confirmId: 'confirm-al-1',
    status: 'pending',
    title: 'retry-empty-reply',
    description: '空回复先重试一次再放弃',
    detail: {
      kind: 'autolearn_lesson',
      name: 'retry-empty-reply',
      description: '空回复先重试一次再放弃',
      lesson: '遇到空回复先换措辞重试一次。\n\n原样重发通常还是空。',
    },
    actions: [],
    ...overrides,
  };
}

describe('AutolearnLessonCard', () => {
  beforeEach(() => {
    // 必须带花括号：mockClear 的返回值会被 vitest 4 当成收尾钩子登记
    lingxiFetchMock.mockClear();
    window.t = ((key: string, params?: Record<string, string>) => {
      if (params) {
        return `${key}:${Object.entries(params).map(([k, v]) => `${k}=${v}`).join(',')}`;
      }
      return key;
    }) as typeof window.t;
    useStore.setState({ addToast: vi.fn() } as any);
  });

  afterEach(() => {
    cleanup();
  });

  it('pending 卡默认展开：教训正文与两个决策按钮可见', () => {
    render(<AutolearnLessonCard block={lessonBlock() as any} />);
    expect(screen.getByText('retry-empty-reply')).toBeInTheDocument();
    expect(screen.getByText(/遇到空回复先换措辞重试一次/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'autolearn.approve' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'autolearn.dismiss' })).toBeInTheDocument();
    expect(screen.getByText('autolearn.suggested')).toBeInTheDocument();
  });

  it('点「学会它」：POST confirmed 后进入已学习态', async () => {
    render(<AutolearnLessonCard block={lessonBlock() as any} />);
    fireEvent.click(screen.getByRole('button', { name: 'autolearn.approve' }));
    await waitFor(() => {
      expect(lingxiFetchMock).toHaveBeenCalledWith('/api/confirm/confirm-al-1', expect.objectContaining({
        method: 'POST',
      }));
    });
    const body = JSON.parse(String(lingxiFetchMock.mock.calls.at(-1)?.[1]?.body));
    expect(body.action).toBe('confirmed');
    expect(await screen.findByText('autolearn.approvedState')).toBeInTheDocument();
  });

  it('点「忽略」：POST rejected 后进入已忽略态', async () => {
    render(<AutolearnLessonCard block={lessonBlock() as any} />);
    fireEvent.click(screen.getByRole('button', { name: 'autolearn.dismiss' }));
    await waitFor(() => {
      expect(lingxiFetchMock).toHaveBeenCalled();
    });
    const body = JSON.parse(String(lingxiFetchMock.mock.calls.at(-1)?.[1]?.body));
    expect(body.action).toBe('rejected');
    expect(await screen.findByText('autolearn.rejectedState')).toBeInTheDocument();
  });

  it('确认接口 404（条目过期）：如实刻画已过期，不假装成功', async () => {
    lingxiFetchMock.mockResolvedValueOnce(new Response('{}', { status: 404 }));
    render(<AutolearnLessonCard block={lessonBlock() as any} />);
    fireEvent.click(screen.getByRole('button', { name: 'autolearn.approve' }));
    expect(await screen.findByText('autolearn.expiredState')).toBeInTheDocument();
  });

  it('ws 把状态推成 timeout：同样刻画已过期', () => {
    render(<AutolearnLessonCard block={lessonBlock({ status: 'timeout' }) as any} />);
    expect(screen.getByText('autolearn.expiredState')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'autolearn.approve' })).not.toBeInTheDocument();
  });

  it('接口报错：toast 告警，卡仍保持 pending', async () => {
    const addToast = vi.fn();
    useStore.setState({ addToast } as any);
    lingxiFetchMock.mockResolvedValueOnce(new Response(JSON.stringify({ error: 'db locked' }), { status: 500 }));
    render(<AutolearnLessonCard block={lessonBlock() as any} />);
    fireEvent.click(screen.getByRole('button', { name: 'autolearn.approve' }));
    await waitFor(() => {
      expect(addToast).toHaveBeenCalledWith(expect.stringContaining('autolearn.decideFailed'), 'error');
    });
    expect(screen.getByRole('button', { name: 'autolearn.approve' })).toBeInTheDocument();
  });
});
