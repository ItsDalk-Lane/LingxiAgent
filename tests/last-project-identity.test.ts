import { describe, it, expect, beforeEach } from 'vitest';
import {
  persistLastProjectIdentity,
  readLastProjectIdentity,
} from '../desktop/src/react/stores/last-project-identity';

const g = globalThis as unknown as { window?: { localStorage?: unknown } };

function installFakeLocalStorage(impl: unknown): void {
  g.window = { localStorage: impl };
}

describe('last-project-identity', () => {
  let store: Map<string, string>;

  beforeEach(() => {
    store = new Map();
    installFakeLocalStorage({
      getItem: (key: string) => (store.has(key) ? store.get(key)! : null),
      setItem: (key: string, value: string) => { store.set(key, value); },
      removeItem: (key: string) => { store.delete(key); },
    });
  });

  it('persist 后能读回相同身份', () => {
    persistLastProjectIdentity({ workspaceMountId: 'local_fs_abc', cwd: '/tmp/proj', workspaceLabel: 'proj' });
    expect(readLastProjectIdentity()).toEqual({
      workspaceMountId: 'local_fs_abc',
      cwd: '/tmp/proj',
      workspaceLabel: 'proj',
    });
  });

  it('缺 label 的旧缓存读取为 null', () => {
    store.set('hana-last-project-identity', JSON.stringify({ workspaceMountId: 'm1', cwd: null }));
    expect(readLastProjectIdentity()).toEqual({ workspaceMountId: 'm1', cwd: null, workspaceLabel: null });
  });

  it('读取时 trim 两端空白', () => {
    store.set('hana-last-project-identity', JSON.stringify({ workspaceMountId: '  m1  ', cwd: ' /tmp/x ', workspaceLabel: null }));
    expect(readLastProjectIdentity()).toEqual({ workspaceMountId: 'm1', cwd: '/tmp/x', workspaceLabel: null });
  });

  it('身份为空时不覆盖旧值', () => {
    persistLastProjectIdentity({ workspaceMountId: 'keep-me', cwd: null });
    persistLastProjectIdentity({ workspaceMountId: null, cwd: null });
    persistLastProjectIdentity(null);
    persistLastProjectIdentity(undefined);
    persistLastProjectIdentity({ workspaceMountId: '   ', cwd: '' });
    expect(readLastProjectIdentity()).toEqual({ workspaceMountId: 'keep-me', cwd: null, workspaceLabel: null });
  });

  it('损坏的 JSON 返回 null 不抛错', () => {
    store.set('hana-last-project-identity', '{not json');
    expect(readLastProjectIdentity()).toBeNull();
  });

  it('非对象或字段类型错误的缓存返回 null / 忽略坏字段', () => {
    store.set('hana-last-project-identity', JSON.stringify([1, 2]));
    expect(readLastProjectIdentity()).toBeNull();
    store.set('hana-last-project-identity', JSON.stringify({ workspaceMountId: 42, cwd: '/ok' }));
    expect(readLastProjectIdentity()).toEqual({ workspaceMountId: null, cwd: '/ok', workspaceLabel: null });
  });

  it('localStorage 不可用时写入与读取都静默', () => {
    installFakeLocalStorage(undefined);
    expect(() => persistLastProjectIdentity({ workspaceMountId: 'm', cwd: null })).not.toThrow();
    expect(readLastProjectIdentity()).toBeNull();
  });

  it('无任何记录时读取返回 null', () => {
    expect(readLastProjectIdentity()).toBeNull();
  });
});
