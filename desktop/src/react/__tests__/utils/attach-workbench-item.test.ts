/**
 * @vitest-environment jsdom
 *
 * 工作台条目附加到聊天输入框的入口契约：上限、页签守卫与空路径。
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { attachWorkbenchItemToInput } from '../../utils/attach-workbench-item';

describe('attachWorkbenchItemToInput', () => {
  beforeEach(() => {
    window.t = ((key: string) => key) as typeof window.t;
    useStore.setState({
      currentTab: 'chat',
      attachedFiles: [],
      attachedFilesBySession: {},
      toasts: [],
    } as never);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('attaches a workspace file by its absolute path without uploading a copy', () => {
    const added = attachWorkbenchItemToInput({ path: '/Users/me/project/report.md', name: 'report.md' });

    expect(added).toBe(true);
    expect(useStore.getState().attachedFiles).toEqual([
      { path: '/Users/me/project/report.md', name: 'report.md', isDirectory: false },
    ]);
  });

  it('keeps the directory flag for folders', () => {
    attachWorkbenchItemToInput({ path: '/Users/me/project/notes', name: 'notes', isDirectory: true });

    expect(useStore.getState().attachedFiles[0]).toMatchObject({ isDirectory: true });
  });

  it('refuses to attach outside the chat tab and tells the user why', () => {
    useStore.setState({ currentTab: 'knowledge' } as never);

    expect(attachWorkbenchItemToInput({ path: '/Users/me/project/report.md', name: 'report.md' })).toBe(false);
    expect(useStore.getState().attachedFiles).toEqual([]);
    expect(useStore.getState().toasts.map(toast => toast.text)).toContain('knowledge.useImportButton');
  });

  it('stops at the nine attachment limit', () => {
    useStore.setState({
      attachedFiles: Array.from({ length: 9 }, (_, i) => ({ path: `/tmp/f${i}.txt`, name: `f${i}.txt` })),
    } as never);

    expect(attachWorkbenchItemToInput({ path: '/Users/me/project/report.md', name: 'report.md' })).toBe(false);
    expect(useStore.getState().attachedFiles).toHaveLength(9);
  });

  it('ignores entries without a path', () => {
    expect(attachWorkbenchItemToInput({ path: '', name: 'report.md' })).toBe(false);
    expect(useStore.getState().attachedFiles).toEqual([]);
  });
});
