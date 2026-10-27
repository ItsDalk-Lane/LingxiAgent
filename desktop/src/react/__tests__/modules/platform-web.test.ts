/**
 * @vitest-environment jsdom
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import fs from 'fs';
import path from 'path';
import { getDesktopHost, resetDesktopHostForTests } from '../../host/desktop-host';

function runPlatformScript(): void {
  const source = fs.readFileSync(path.join(process.cwd(), 'desktop/src/modules/platform.js'), 'utf-8');
  new Function(source)();
}

// R08-T02：web fallback 的 63 成员契约已从 platform.js 移入 host/web-desktop-host.ts，
// platform.js 只保留可视引导；宿主由 getDesktopHost() 惰性安装并写 window.platform 别名。
// 以下断言逐字保留原契约（trashItem 缺失、dev-web 注入的连接信息、readFile 的
// HTTP 路由与空 headers），仅把「装载 fallback」一步改为走 DesktopHost 惰性解析。
describe('web platform fallback capability contract', () => {
  beforeEach(() => {
    delete (window as any).hana;
    resetDesktopHostForTests();
    delete (window as any).__LINGXI_DEV_WEB__;
    (globalThis as any).localStorage = {
      getItem: () => '',
      setItem: () => {},
      removeItem: () => {},
      clear: () => {},
    };
  });

  it('does not expose system trash when the browser environment cannot provide it', () => {
    runPlatformScript();
    getDesktopHost();

    expect((getDesktopHost().api as any).trashItem).toBeUndefined();
  });

  it('uses injected dev-web server info for browser preview API access', async () => {
    (window as any).__LINGXI_DEV_WEB__ = {
      serverPort: 5173,
      apiBaseUrl: 'http://127.0.0.1:5173',
    };
    const fetchMock = vi.fn(async () => new Response('file contents'));
    vi.stubGlobal('fetch', fetchMock);

    runPlatformScript();
    getDesktopHost();

    expect(await (getDesktopHost().api as any).getServerPort()).toBe('5173');
    expect(await (getDesktopHost().api as any).getServerToken()).toBe('');

    const content = await (getDesktopHost().api as any).readFile('/tmp/demo.md');
    expect(content).toBe('file contents');
    expect(fetchMock).toHaveBeenCalledWith(
      'http://127.0.0.1:5173/api/fs/read?path=%2Ftmp%2Fdemo.md',
      expect.objectContaining({
        headers: {},
      }),
    );
  });
});
