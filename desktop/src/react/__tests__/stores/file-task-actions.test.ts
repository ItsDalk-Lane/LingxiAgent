import { describe, expect, it, vi } from 'vitest';
import {
  openFileTaskEntry,
  registerFileTaskHandler,
} from '../../stores/file-task-actions';

describe('file-task-actions', () => {
  it('routes the fixed entry to the registered input-area handler', async () => {
    const handler = vi.fn(async () => {});
    const unregister = registerFileTaskHandler(handler);
    try {
      await expect(openFileTaskEntry()).resolves.toBe(true);
      expect(handler).toHaveBeenCalledTimes(1);
    } finally {
      unregister();
    }
  });

  it('returns false and does not fake success when no handler is mounted', async () => {
    await expect(openFileTaskEntry()).resolves.toBe(false);
  });

  it('unregister only clears its own handler', async () => {
    const first = vi.fn(async () => {});
    const second = vi.fn(async () => {});
    const unregisterFirst = registerFileTaskHandler(first);
    const unregisterSecond = registerFileTaskHandler(second);
    unregisterFirst();
    await openFileTaskEntry();
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
    unregisterSecond();
  });
});
