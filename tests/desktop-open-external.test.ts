import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { describe, expect, it, vi } from 'vitest';

function actualHandler(openExternal: (url: string) => Promise<void>) {
  const source = readFileSync(new URL('../desktop/main.cjs', import.meta.url), 'utf8');
  const start = source.indexOf('wrapIpcBestEffortHandler("open-external"');
  const strict = source.indexOf('wrapIpcHandler("open-external"');
  const from = start >= 0 ? start : strict;
  expect(from).toBeGreaterThanOrEqual(0);
  let handler!: (event: unknown, url: string) => Promise<{ ok: boolean; error?: string }>;
  const register = (_channel: string, callback: typeof handler) => { handler = callback; };
  vm.runInNewContext(source.slice(from, source.indexOf('// 读取文件内容', from)), {
    wrapIpcHandler: register, wrapIpcBestEffortHandler: register, URL, Error, shell: { openExternal }, console,
  });
  return handler;
}

describe('现役打开外部浏览器的系统回执', () => {
  it('真实处理函数等待系统 Promise 完成才返回成功', async () => {
    let finish!: () => void;
    const waiting = new Promise<void>(resolve => { finish = resolve; });
    const open = vi.fn(() => waiting);
    let settled = false;
    const result = Promise.resolve(actualHandler(open)({}, 'https://example.invalid/owned')).then(value => { settled = true; return value; });
    await Promise.resolve();
    expect(open).toHaveBeenCalledOnce(); expect(settled).toBe(false);
    finish(); expect(await result).toEqual({ ok: true });
  });
  it('系统拒绝时返回明确失败而不假报已打开', async () => {
    const rejected = Promise.reject(new Error('自有系统打开失败'));
    void rejected.catch(() => {});
    const handler = actualHandler(() => rejected);
    expect(await handler({}, 'http://127.0.0.1/owned')).toEqual({ ok: false, error: '自有系统打开失败' });
  });
  it.each(['', 'broken url', 'javascript:alert(1)', 'file:///private/owned'])('损坏或不支持的地址不交系统：%s', async url => {
    const open = vi.fn(async () => {});
    expect((await actualHandler(open)({}, url)).ok).toBe(false);
    expect(open).not.toHaveBeenCalled();
  });
});
