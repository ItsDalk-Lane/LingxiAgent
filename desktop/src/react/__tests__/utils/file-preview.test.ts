/**
 * @vitest-environment jsdom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  openPreview: vi.fn(),
  showError: vi.fn(),
  openMediaViewerFromContext: vi.fn(),
}));

vi.mock('../../stores/preview-actions', () => ({
  openPreview: mocks.openPreview,
}));

vi.mock('../../utils/ui-helpers', () => ({
  showError: mocks.showError,
}));

vi.mock('../../utils/open-media-viewer', () => ({
  openMediaViewerFromContext: mocks.openMediaViewerFromContext,
}));

import { openFilePreview, openSkillPreview } from '../../utils/file-preview';

describe('file-preview IPC error handling', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    (window as any).platform = {
      readFile: vi.fn(),
      readDocxHtml: vi.fn(),
      readXlsxHtml: vi.fn(),
      readFileBase64: vi.fn(),
      getFileUrl: vi.fn((filePath: string) => `file://${filePath}`),
      openSkillViewer: vi.fn(),
    };
  });

  afterEach(() => {
    delete (window as any).platform;
  });

  it('预览读取异常时向用户报错，并且不再把 Promise 泄漏到全局', async () => {
    (window as any).platform.readFile.mockRejectedValue(new Error('preview exploded'));

    await expect(openFilePreview('/tmp/demo.md', 'demo.md', 'md', { origin: 'desk' })).resolves.toBeUndefined();

    expect(mocks.showError).toHaveBeenCalledWith('preview exploded');
    expect(mocks.openPreview).not.toHaveBeenCalled();
    expect(mocks.openMediaViewerFromContext).not.toHaveBeenCalled();
  });

  it('技能预览使用既有 Skill Viewer overlay，而不是 markdown Preview 面板', async () => {
    (window as any).platform.readFile.mockResolvedValue('---\nname: demo-skill\n---\n# Demo');

    await expect(openSkillPreview('demo-skill', '/tmp/demo-skill/SKILL.md')).resolves.toBeUndefined();

    expect((window as any).platform.openSkillViewer).toHaveBeenCalledWith({
      name: 'demo-skill',
      baseDir: '/tmp/demo-skill',
      filePath: '/tmp/demo-skill/SKILL.md',
      installed: true,
    });
    expect((window as any).platform.readFile).not.toHaveBeenCalled();
    expect(mocks.openPreview).not.toHaveBeenCalled();
  });

  it('技能预览优先使用已登记的 installedSkillSource.baseDir', async () => {
    const openSkillPreviewWithSource = openSkillPreview as unknown as (
      skillName: string,
      skillFilePath: string,
      source: { skillName: string; baseDir: string; filePath: string },
    ) => Promise<void>;

    await expect(openSkillPreviewWithSource('demo-skill', '/stale/path/SKILL.md', {
      skillName: 'source-skill',
      baseDir: '/installed/source-skill',
      filePath: '/installed/source-skill/SKILL.md',
    })).resolves.toBeUndefined();

    expect((window as any).platform.openSkillViewer).toHaveBeenCalledWith({
      name: 'source-skill',
      baseDir: '/installed/source-skill',
      filePath: '/installed/source-skill/SKILL.md',
      installed: true,
    });
    expect(mocks.openPreview).not.toHaveBeenCalled();
  });

  it('技能预览缺少可用路径时显式报错', async () => {
    await expect(openSkillPreview('demo-skill', '')).resolves.toBeUndefined();

    expect(mocks.showError).toHaveBeenCalledWith('skill preview path missing');
    expect(mocks.openPreview).not.toHaveBeenCalled();
    expect((window as any).platform.openSkillViewer).not.toHaveBeenCalled();
  });

  it('PDF 预览使用本地 file URL，不因 base64 读取失败回退成文件信息卡', async () => {
    (window as any).platform.readFileBase64.mockResolvedValue(null);

    await expect(openFilePreview('/tmp/Report.PDF', 'Report.PDF', 'PDF', { origin: 'desk' })).resolves.toBeUndefined();

    expect((window as any).platform.getFileUrl).toHaveBeenCalledWith('/tmp/Report.PDF');
    expect((window as any).platform.readFileBase64).not.toHaveBeenCalled();
    expect(mocks.openPreview).toHaveBeenCalledWith(expect.objectContaining({
      id: 'file-/tmp/Report.PDF',
      type: 'pdf',
      title: 'Report.PDF',
      content: '',
      filePath: '/tmp/Report.PDF',
      ext: 'pdf',
      sourceUrl: 'file:///tmp/Report.PDF',
    }));
  });

  it('映射表外的未知扩展名：文本内容按可编辑 code 打开（VS Code 式全量打开）', async () => {
    (window as any).platform.readFile.mockResolvedValue('plain text body\n');

    await expect(openFilePreview('/tmp/data.bak', 'data.bak', 'BAK', { origin: 'desk' })).resolves.toBeUndefined();

    expect(mocks.openPreview).toHaveBeenCalledWith(expect.objectContaining({
      id: 'file-/tmp/data.bak',
      type: 'code',
      title: 'data.bak',
      content: 'plain text body\n',
      filePath: '/tmp/data.bak',
      ext: 'bak',
      language: 'bak',
    }));
  });

  it('无扩展名文件（如 Makefile）可读文本时同样按 code 打开', async () => {
    (window as any).platform.readFile.mockResolvedValue('all:\n\techo ok\n');

    await expect(openFilePreview('/tmp/Makefile', 'Makefile', '', { origin: 'desk' })).resolves.toBeUndefined();

    expect(mocks.openPreview).toHaveBeenCalledWith(expect.objectContaining({
      type: 'code',
      title: 'Makefile',
      ext: '',
      language: undefined,
    }));
  });

  it('未知扩展名且读不出文本（二进制嗅探拒绝）→ 回退文件信息卡', async () => {
    (window as any).platform.readFileSnapshot = vi.fn().mockResolvedValue(null);
    (window as any).platform.readFile.mockResolvedValue(null);

    await expect(openFilePreview('/tmp/blob.bin', 'blob.bin', 'bin', { origin: 'desk' })).resolves.toBeUndefined();

    expect(mocks.openPreview).toHaveBeenCalledWith(expect.objectContaining({
      id: 'file-/tmp/blob.bin',
      type: 'file-info',
      ext: 'bin',
    }));
  });

  it('新纳入映射表的常见文本格式（如 toml）按 code 打开', async () => {
    (window as any).platform.readFile.mockResolvedValue('[package]\nname = "demo"\n');

    await expect(openFilePreview('/tmp/Cargo.toml', 'Cargo.toml', 'toml', { origin: 'desk' })).resolves.toBeUndefined();

    expect(mocks.openPreview).toHaveBeenCalledWith(expect.objectContaining({
      type: 'code',
      title: 'Cargo.toml',
      ext: 'toml',
      language: 'toml',
    }));
  });

  it('HTML 预览保留调用方提供的安全资源根', async () => {
    (window as any).platform.readFile.mockResolvedValue('<img src="../assets/pic.png">');

    await expect(openFilePreview(
      '/workspace/pages/demo.html',
      'demo.html',
      'HTML',
      { origin: 'desk', sourceRootPath: '/workspace' } as Parameters<typeof openFilePreview>[3] & { sourceRootPath: string },
    )).resolves.toBeUndefined();

    expect(mocks.openPreview).toHaveBeenCalledWith(expect.objectContaining({
      id: 'file-/workspace/pages/demo.html',
      type: 'html',
      title: 'demo.html',
      content: '<img src="../assets/pic.png">',
      filePath: '/workspace/pages/demo.html',
      ext: 'html',
      sourceRootPath: '/workspace',
    }));
  });
});
