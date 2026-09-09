/**
 * @vitest-environment jsdom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { loadMediaSource } from '../../../../components/shared/MediaViewer/media-source';
import { useStore } from '../../../../stores';
import type { FileRef } from '../../../../types/file-ref';

describe('loadMediaSource', () => {
  beforeEach(() => {
    useStore.setState({ serverPort: 3210, serverToken: null, serverConnections: {}, activeServerConnectionId: null, activeServerConnection: null } as never);
    (window as any).platform = {
      // 保留 mock，回归测试中如果谁还在走 readFileBase64 会被 expect().not.toHaveBeenCalled() 捕获
      readFileBase64: vi.fn(async (p: string) => `BASE64_OF_${p}`),
      getFileUrl: vi.fn((p: string) => `file:///MOCK${p}`),
    };
  });
  afterEach(() => {
    delete (window as any).platform;
    useStore.setState({ serverPort: null, serverToken: null, serverConnections: {}, activeServerConnectionId: null, activeServerConnection: null } as never);
  });

  it('image: source=desk 走 getFileUrl（不再整文件 base64）', async () => {
    const ref: FileRef = { id: '1', kind: 'image', source: 'desk', name: 'a.png', path: '/a.png', ext: 'png', version: { mtimeMs: 11, size: 22 } };
    const src = await loadMediaSource(ref);
    expect((window as any).platform.getFileUrl).toHaveBeenCalledWith('/a.png');
    expect(src.url).toBe('file:///MOCK/a.png?v=11-22');
    expect((window as any).platform.readFileBase64).not.toHaveBeenCalled();
  });

  it('svg: 走 getFileUrl（不再整文件 base64）', async () => {
    const ref: FileRef = { id: '1', kind: 'svg', source: 'desk', name: 'a.svg', path: '/a.svg', ext: 'svg' };
    const src = await loadMediaSource(ref);
    expect((window as any).platform.getFileUrl).toHaveBeenCalledWith('/a.svg');
    expect(src.url).toBe('file:///MOCK/a.svg');
    expect((window as any).platform.readFileBase64).not.toHaveBeenCalled();
  });

  it('session-block-screenshot: 直接用 inlineData data URL（base64 已在内存中）', async () => {
    const ref: FileRef = {
      id: '1', kind: 'image', source: 'session-block-screenshot',
      name: 's.png', path: '',
      inlineData: { base64: 'ABC', mimeType: 'image/png' },
    };
    const src = await loadMediaSource(ref);
    expect(src.url).toBe('data:image/png;base64,ABC');
    expect((window as any).platform.getFileUrl).not.toHaveBeenCalled();
    expect((window as any).platform.readFileBase64).not.toHaveBeenCalled();
  });

  it('同时存在 path + inlineData 时，优先走 path 而不是 data URL', async () => {
    const ref: FileRef = {
      id: '1', kind: 'image', source: 'session-attachment',
      name: 'p.png', path: '/persisted.png',
      inlineData: { base64: 'ABC', mimeType: 'image/png' },
    };
    const src = await loadMediaSource(ref);
    expect((window as any).platform.getFileUrl).toHaveBeenCalledWith('/persisted.png');
    expect(src.url).toBe('file:///MOCK/persisted.png');
  });

  it('video: 走 platform.getFileUrl（不手拼 file://）', async () => {
    const ref: FileRef = { id: '1', kind: 'video', source: 'desk', name: 'a.mp4', path: '/a.mp4', ext: 'mp4' };
    const src = await loadMediaSource(ref);
    expect((window as any).platform.getFileUrl).toHaveBeenCalledWith('/a.mp4');
    expect(src.url).toBe('file:///MOCK/a.mp4');
    expect((window as any).platform.readFileBase64).not.toHaveBeenCalled();
  });

  it.each(['image', 'video'] as const)('%s 放大查看优先使用受控资源地址', async (kind) => {
    const ref: FileRef = {
      id: 'generated', kind, source: 'session-block-file', name: 'generated', path: '/tmp/generated',
      resource: { resourceId: 'res_generated', studioId: 'studio_local', links: {
        self: '/api/resources/res_generated', content: '/api/resources/res_generated/content',
      } },
    };
    expect((await loadMediaSource(ref)).url).toBe('http://127.0.0.1:3210/api/resources/res_generated/content');
    expect((window as any).platform.getFileUrl).not.toHaveBeenCalled();
  });

  it('platform 缺失 → 抛错', async () => {
    delete (window as any).platform;
    const ref: FileRef = { id: '1', kind: 'image', source: 'desk', name: 'a.png', path: '/a.png', ext: 'png' };
    await expect(loadMediaSource(ref)).rejects.toThrow(/platform/i);
  });

  it('platform.getFileUrl 缺失 → 抛错', async () => {
    (window as any).platform = {}; // 故意缺 getFileUrl
    const ref: FileRef = { id: '1', kind: 'image', source: 'desk', name: 'a.png', path: '/a.png', ext: 'png' };
    await expect(loadMediaSource(ref)).rejects.toThrow(/getFileUrl/i);
  });

  it('带 path 但 kind 不支持 → 抛 unsupported', async () => {
    const ref: FileRef = { id: '1', kind: 'other', source: 'desk', name: 'a.zip', path: '/a.zip' };
    await expect(loadMediaSource(ref)).rejects.toThrow(/unsupported media kind/i);
  });

  it('缺 path 且无 inlineData → 抛错', async () => {
    const ref: FileRef = { id: 'bad', kind: 'image', source: 'desk', name: 'x', path: '' };
    await expect(loadMediaSource(ref)).rejects.toThrow(/path/);
  });
});
