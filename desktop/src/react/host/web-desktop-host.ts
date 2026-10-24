/**
 * web-desktop-host.ts — Web 受限宿主实现（R08-T02 / R08-A03）
 *
 * 由 desktop/src/modules/platform.js 原 63 成员 web fallback 移入并升级：
 * - 真实能力（HTTP 读取 / 浏览器 API / dev-web 连接引导）保留；
 * - 4 处 HTTP 非 ok→null 升级：404→null（显式「未找到」），其余非 ok 抛带状态错误；
 * - openExternal 空 catch 升级：弹窗失败/被拦截返回 {ok:false,error}；
 * - 55 个常量/空成员逐一登记受限理由（capabilityReason），显式 no-op 而非静默；
 * - PlatformApi 可选但 Web 不提供的成员保持缺失，理由统一登记，消费侧 ?.() 判空。
 */

import type { PlatformApi } from '../types';
import type { DesktopHost } from './desktop-host';

export interface WebDesktopHostOptions {
  /** 测试注入；默认全局 fetch。 */
  fetchImpl?: typeof fetch;
  serverPort?: string;
  token?: string;
  apiBaseUrl?: string;
}

interface DevWebConfig {
  serverPort: string;
  apiBaseUrl: string;
}

function normalizeDevWebConfig(value: unknown): DevWebConfig {
  if (!value || typeof value !== 'object') return { serverPort: '', apiBaseUrl: '' };
  const record = value as { serverPort?: unknown; apiBaseUrl?: unknown };
  const serverPort = typeof record.serverPort === 'number' || typeof record.serverPort === 'string'
    ? String(record.serverPort).trim()
    : '';
  const apiBaseUrl = typeof record.apiBaseUrl === 'string'
    ? record.apiBaseUrl.replace(/\/+$/, '')
    : '';
  return { serverPort, apiBaseUrl };
}

function safePortFromBaseUrl(value: string): string {
  try {
    return new URL(value).port;
  } catch {
    return '';
  }
}

/** 服务端文件读取（Web / 移动端共用）：404→null 显式未找到；其余非 ok 显式抛错。 */
export function createWebHostFileReaders(
  fetchImpl: typeof fetch,
  baseUrl: string,
  token: string,
): Pick<PlatformApi, 'readFile' | 'readFileBase64' | 'readDocxHtml' | 'readXlsxHtml'> {
  const read = (route: string) => async (path: string): Promise<string | null> => {
    const headers: Record<string, string> = {};
    if (token) headers['Authorization'] = `Bearer ${token}`;
    const response = await fetchImpl(`${baseUrl}${route}?path=${encodeURIComponent(path)}`, { headers });
    if (response.status === 404) return null;
    if (!response.ok) {
      throw new Error(`Web 宿主读取文件失败（HTTP ${response.status}）：${route}`);
    }
    return response.text();
  };
  return {
    readFile: read('/api/fs/read'),
    readFileBase64: read('/api/fs/read-base64'),
    readDocxHtml: read('/api/fs/docx-html'),
    readXlsxHtml: read('/api/fs/xlsx-html'),
  };
}

export function createWebDesktopHost(options: WebDesktopHostOptions = {}): DesktopHost {
  const fetchImpl: typeof fetch = options.fetchImpl ?? ((...args: Parameters<typeof fetch>) => fetch(...args));
  const hasWindow = typeof window !== 'undefined';
  // 测试可能用 vi.stubGlobal('window', {}) 挂出裸 window：location/localStorage 均可缺席。
  const location = hasWindow ? window.location : undefined;
  const params = location?.search ? new URLSearchParams(location.search) : new URLSearchParams();
  const devWeb = normalizeDevWebConfig(hasWindow ? window.__LINGXI_DEV_WEB__ : undefined);
  const token = options.token
    ?? params.get('token')
    ?? (hasWindow && window.localStorage ? window.localStorage.getItem('hana-token') : null)
    ?? '';
  const baseUrl = options.apiBaseUrl
    ?? devWeb.apiBaseUrl
    ?? (location ? `${location.protocol}//${location.host}` : 'http://127.0.0.1:3000');
  // 端口逐段回退：显式注入 > dev-web 注入 > baseUrl 解析 > location.port > 3000；空串视为未提供。
  const serverPort = options.serverPort
    || devWeb.serverPort
    || safePortFromBaseUrl(baseUrl)
    || location?.port
    || '3000';

  // 受限理由登记：显式 no-op / 缺失成员的可解释原因（A03「受限能力可解释」）。
  const reasons: Record<string, string> = {
    appReady: 'Web 宿主无主进程 ready 握手，显式无操作',
    syncWindowTheme: 'Web 主题是页面内 data-theme，无原生窗口主题同步',
    runEditCommand: 'Web 宿主不接管编辑命令，返回 false 由浏览器默认行为处理',
    readFileSnapshot: 'Web 宿主无本地文件版本快照，消费侧回退 readFile',
    writeFile: '浏览器不能写本地文件系统，显式返回未写入',
    writeFileBinary: '浏览器不能写本地文件系统，显式返回未写入',
    copyFile: '浏览器不能复制本地文件，显式返回未复制',
    writeFileIfUnchanged: '浏览器不能写本地文件系统，显式返回未写入',
    watchFile: '浏览器无文件系统监听能力',
    unwatchFile: '浏览器无文件系统监听能力',
    onFileChanged: '浏览器无文件变更事件源，注册为无操作',
    watchWorkspace: '浏览器无工作区监听能力',
    unwatchWorkspace: '浏览器无工作区监听能力',
    onWorkspaceChanged: '浏览器无工作区变更事件源，注册为无操作',
    spawnViewer: 'Web 宿主无派生 Viewer 窗口能力，显式返回未派生',
    viewerRequestLoad: 'Web 宿主无 Viewer 窗口载荷，显式返回空',
    viewerClose: 'Web 宿主无 Viewer 窗口，显式无操作',
    onViewerClosed: 'Web 宿主无 Viewer 关闭事件源，注册为无操作',
    getFilePath: '浏览器 File 对象不暴露本地路径，显式返回空',
    getAvatarPath: 'Web 宿主无本地头像资源目录，显式返回空',
    getSplashInfo: 'Web 宿主无 splash 个性化元信息，显式返回空对象',
    selectFolder: '浏览器无系统目录选择器，显式返回空',
    selectFiles: '浏览器无系统多文件选择器（组件自管 input[type=file]），显式返回空数组',
    selectSkill: '浏览器无系统目录选择器，显式返回空',
    selectPlugin: '浏览器无系统文件选择器，显式返回空',
    openFolder: '浏览器不能打开系统文件管理器，显式无操作',
    openFile: '浏览器不能打开本地路径，显式无操作',
    showInFinder: '浏览器不能打开系统文件管理器，显式无操作',
    startDrag: '浏览器不能发起系统级拖拽，显式无操作',
    openSettings: 'Web 宿主的设置入口由应用内路由承担，宿主层显式无操作',
    settingsChanged: 'Web 单窗口无跨窗设置广播，显式无操作',
    onSettingsChanged: 'Web 单窗口无跨窗设置变更事件源，注册为无操作',
    onOpenSettingsModal: 'Web 单窗口无打开设置弹窗事件源，注册为无操作',
    openBrowserViewer: 'Web 宿主无内嵌浏览器查看器，显式无操作',
    closeBrowserViewer: 'Web 宿主无内嵌浏览器查看器，显式无操作',
    onBrowserUpdate: 'Web 宿主无浏览器查看器事件源，注册为无操作',
    browserGoBack: 'Web 宿主无内嵌浏览器查看器，显式无操作',
    browserGoForward: 'Web 宿主无内嵌浏览器查看器，显式无操作',
    browserReload: 'Web 宿主无内嵌浏览器查看器，显式无操作',
    browserNewTab: 'Web 宿主无内嵌浏览器查看器，显式无操作',
    browserSwitchTab: 'Web 宿主无内嵌浏览器查看器，显式无操作',
    browserCloseTab: 'Web 宿主无内嵌浏览器查看器，显式无操作',
    browserEmergencyStop: 'Web 宿主无内嵌浏览器会话，显式无操作',
    closeBrowser: 'Web 宿主无内嵌浏览器会话，显式无操作',
    openSkillViewer: 'Web 宿主无技能查看器窗口，显式无操作',
    listSkillFiles: '浏览器不能列本地技能目录，显式返回空数组',
    readSkillFile: '浏览器不能读本地技能文件，显式返回空',
    onboardingComplete: 'Web 宿主无 onboarding 窗口生命周期，显式无操作',
    debugOpenOnboarding: '调试入口仅 Electron 桌面提供，显式无操作',
    debugOpenOnboardingPreview: '调试入口仅 Electron 桌面提供，显式无操作',
    windowMinimize: '浏览器无窗口最小化控制，显式无操作',
    windowMaximize: '浏览器无窗口最大化控制，显式无操作',
    windowClose: '浏览器无窗口关闭控制，显式无操作',
    windowIsMaximized: '浏览器无窗口最大化状态，显式返回 false',
    onMaximizeChange: '浏览器无窗口最大化事件源，注册为无操作',
  };

  const fileReaders = createWebHostFileReaders(fetchImpl, baseUrl, token);

  const api: PlatformApi = {
    // 服务器连接引导（真实能力）
    getServerPort: async () => serverPort,
    getServerToken: async () => token,
    getPlatform: async () => 'web',
    reloadMainWindow: async () => { if (hasWindow) window.location?.reload(); },

    // 文件读取 → server HTTP（真实能力，404→null / 其余非 ok 显式抛错）
    ...fileReaders,
    readFileSnapshot: async () => null,

    // 文件写入 / 监听 / viewer（显式 no-op）
    writeFile: async () => false,
    writeFileBinary: async () => false,
    copyFile: async () => false,
    writeFileIfUnchanged: async () => ({ ok: false }),
    watchFile: async () => false,
    unwatchFile: async () => false,
    onFileChanged: () => {},
    watchWorkspace: async () => false,
    unwatchWorkspace: async () => false,
    onWorkspaceChanged: () => {},
    spawnViewer: async () => null,
    viewerRequestLoad: async () => null,
    viewerClose: () => {},
    onViewerClosed: () => {},

    // 文件路径 / 资源（显式空值）
    getFilePath: () => null,
    getAvatarPath: async () => null,
    getSplashInfo: async () => ({}),

    // 系统对话框（显式空值）
    selectFolder: async () => null,
    selectFiles: async () => [],
    selectSkill: async () => null,
    selectPlugin: async () => null,

    // OS 集成
    openFolder: () => {},
    openFile: () => {},
    // 升级：原空 catch → 弹窗失败/被拦截显式返回失败对象
    openExternal: async (url: string) => {
      if (!hasWindow) return { ok: false, error: 'web_host_open_external_unavailable' };
      try {
        const opened = window.open(url, '_blank');
        if (!opened) return { ok: false, error: 'web_host_open_external_blocked' };
        return undefined;
      } catch {
        return { ok: false, error: 'web_host_open_external_unavailable' };
      }
    },
    showInFinder: () => {},
    startDrag: () => {},

    // 窗口 / 设置通信（显式 no-op）
    openSettings: () => {},
    settingsChanged: () => {},
    onSettingsChanged: () => {},
    onOpenSettingsModal: () => {},

    // 浏览器查看器 / 技能查看器（显式 no-op）
    openBrowserViewer: () => {},
    closeBrowserViewer: () => {},
    onBrowserUpdate: () => {},
    browserGoBack: () => {},
    browserGoForward: () => {},
    browserReload: () => {},
    browserNewTab: () => {},
    browserSwitchTab: () => {},
    browserCloseTab: () => {},
    browserEmergencyStop: () => {},
    closeBrowser: () => {},
    openSkillViewer: () => {},
    listSkillFiles: async () => [],
    readSkillFile: async () => null,

    // Onboarding / 调试入口（显式 no-op）
    onboardingComplete: async () => {},
    debugOpenOnboarding: async () => {},
    debugOpenOnboardingPreview: async () => {},

    // 窗口控制（显式 no-op）
    appReady: async () => {},
    syncWindowTheme: () => {},
    runEditCommand: async () => false,
    windowMinimize: () => {},
    windowMaximize: () => {},
    windowClose: () => {},
    windowIsMaximized: async () => false,
    onMaximizeChange: () => {},
  };

  return {
    kind: 'web',
    api,
    capabilityReason: (name: string) => reasons[name] ?? null,
  };
}
