/**
 * @vitest-environment jsdom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, waitFor } from '@testing-library/react';
import { VideoStage } from '../../../../components/shared/MediaViewer/VideoStage';
import type { FileRef } from '../../../../types/file-ref';

describe('VideoStage', () => {
  beforeEach(() => {
    (window as any).platform = {
      getFileUrl: (p: string) => `file://${p}`,
    };
  });
  afterEach(() => {
    cleanup();
    delete (window as any).platform;
  });

  it('解码失败时显示显式错误卡片并提供系统播放器逃生门', async () => {
    (window as any).t = (key: string) => key;
    const openFile = vi.fn();
    (window as any).platform = {
      getFileUrl: (p: string) => `file://${p}`,
      openFile,
    };
    const onError = vi.fn();
    const file: FileRef = { id: 'v', kind: 'video', source: 'desk', name: 'a.mp4', path: '/a.mp4', ext: 'mp4' };
    const { container } = render(
      <VideoStage file={file} viewport={{ width: 800, height: 600 }} onError={onError} />,
    );
    const video = await waitFor(() => {
      const v = container.querySelector('video');
      expect(v).toBeTruthy();
      return v as HTMLVideoElement;
    });
    fireEvent.error(video);
    expect(onError).toHaveBeenCalled();
    const card = container.querySelector('[data-testid="video-stage-error"]');
    expect(card).toBeTruthy();
    expect(card!.textContent).toContain('mediaViewer.videoDecodeError');
    const btn = container.querySelector('button');
    expect(btn).toBeTruthy();
    fireEvent.click(btn!);
    expect(openFile).toHaveBeenCalledWith('/a.mp4');
  });

  it('渲染 <video controls> 带 file:// url', async () => {
    const file: FileRef = { id: 'v', kind: 'video', source: 'desk', name: 'a.mp4', path: '/a.mp4', ext: 'mp4' };
    const { container } = render(<VideoStage file={file} viewport={{ width: 800, height: 600 }} />);
    await waitFor(() => {
      const v = container.querySelector('video');
      expect(v).toBeTruthy();
      expect(v!.hasAttribute('controls')).toBe(true);
      expect(v!.getAttribute('src')).toMatch(/^file:\/\//);
    });
  });
});
