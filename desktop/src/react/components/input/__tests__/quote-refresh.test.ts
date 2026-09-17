import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { QuotedSelection } from '../../../stores/input-slice';

const fetchMock = vi.fn();
vi.mock('../../../hooks/use-hana-fetch', () => ({
  lingxiFetch: (...args: unknown[]) => fetchMock(...args),
}));

import { refreshQuotedSelectionFromDisk } from '../quote-refresh';

function makeSel(overrides: Partial<QuotedSelection> = {}): QuotedSelection {
  return {
    text: 'old line',
    sourceTitle: 'demo.ts',
    sourceKind: 'preview',
    sourceFilePath: '/tmp/demo.ts',
    lineStart: 2,
    lineEnd: 2,
    selectionAnchorKind: 'codemirror',
    charCount: 8,
    updatedAt: Date.now(),
    ...overrides,
  } as QuotedSelection;
}

function okResponse(text: string) {
  const buf = new TextEncoder().encode(text);
  return {
    ok: true,
    arrayBuffer: async () => buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength),
  };
}

describe('refreshQuotedSelectionFromDisk', () => {
  // 必须带花括号：mockReset() 会返回 mock 本身，箭头函数隐式返回会被
  // vitest 当成 beforeEach 收尾钩子在测试后调用，撞上 mockRejectedValue 就会
  // 以「清理钩子返回拒绝 Promise」的名义误杀测试。
  beforeEach(() => {
    fetchMock.mockReset();
  });

  it('文件内容变化时用行区间现读切片替换捕获文本', async () => {
    fetchMock.mockResolvedValue(okResponse('first\nsecond CHANGED\nthird'));
    const sel = makeSel();
    const out = await refreshQuotedSelectionFromDisk(sel);
    expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('/api/fs/read?path='));
    expect(out.text).toBe('second CHANGED');
    expect(out.charCount).toBe('second CHANGED'.length);
  });

  it('文件内容未变时返回原对象（不重写）', async () => {
    fetchMock.mockResolvedValue(okResponse('first\nold line\nthird'));
    const sel = makeSel();
    const out = await refreshQuotedSelectionFromDisk(sel);
    expect(out).toBe(sel);
  });

  it('多行区间按行切片并 trim', async () => {
    fetchMock.mockResolvedValue(okResponse('a\n  b1  \nb2\nc'));
    const out = await refreshQuotedSelectionFromDisk(makeSel({ lineStart: 2, lineEnd: 3, text: 'b1\nb2' }));
    expect(out.text).toBe('b1  \nb2');
  });

  it('读取失败（404/网络错）回落为捕获文本', async () => {
    fetchMock.mockResolvedValue({ ok: false });
    const sel = makeSel();
    expect(await refreshQuotedSelectionFromDisk(sel)).toBe(sel);
    fetchMock.mockRejectedValue(new Error('network down'));
    expect(await refreshQuotedSelectionFromDisk(sel)).toBe(sel);
  });

  it('行区间越界（文件变短）回落为捕获文本', async () => {
    fetchMock.mockResolvedValue(okResponse('only one line'));
    const sel = makeSel({ lineStart: 5, lineEnd: 9 });
    expect(await refreshQuotedSelectionFromDisk(sel)).toBe(sel);
  });

  it('无文件路径或行号的引用（如聊天消息引用）直接放行', async () => {
    const sel = makeSel({ sourceFilePath: undefined, lineStart: undefined, lineEnd: undefined });
    expect(await refreshQuotedSelectionFromDisk(sel)).toBe(sel);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('超大文件跳过重读', async () => {
    const big = new Uint8Array(5 * 1024 * 1024);
    fetchMock.mockResolvedValue({ ok: true, arrayBuffer: async () => big.buffer });
    const sel = makeSel();
    expect(await refreshQuotedSelectionFromDisk(sel)).toBe(sel);
  });
});
