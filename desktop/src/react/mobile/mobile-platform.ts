import type { PlatformApi } from '../types';
import { installDesktopHostApi } from '../host/desktop-host';

const noopUnsubscribe = () => {};
const OPEN_SETTINGS_EVENT = 'hana-mobile-open-settings';

/**
 * 安装移动端宿主（DesktopHost kind='mobile'）。
 * 成员面与 HEAD 既有契约逐字一致：只暴露 20 个真实/显式无操作成员；
 * 桌面专属文件系统能力（selectFolder/selectFiles/writeFileIfUnchanged/
 * openFolder/showInFinder/trashItem/getFilePath 等）保持「不存在」——
 * __tests__/mobile/mobile-platform.test.ts 明确断言它们为 undefined，
 * 不得以可调用的空实现冒充；openExternal 保持同步 void 旧返回值，
 * 不把 noopener 窗口返回值冒充系统回执。缺失成员经 capabilityReasons
 * 给出可解释理由（getDesktopHost().capabilityReason / explainHostCapability）。
 * 因此保留 Partial→PlatformApi 的窄化强转：运行时「不存在即受限」是契约本体。
 */
export function installMobilePlatform(): void {
  if (typeof window === 'undefined') return;
  document.documentElement.setAttribute('data-platform', 'web');

  const api: Partial<PlatformApi> = {
    getServerPort: async () => window.location.port || '',
    getServerToken: async () => '',
    openSettings: (tab?: string) => {
      window.dispatchEvent(new CustomEvent(OPEN_SETTINGS_EVENT, { detail: { tab } }));
    },
    onOpenSettingsModal: (callback: (tab?: string) => void) => {
      const listener = (event: Event) => {
        callback((event as CustomEvent<{ tab?: string }>).detail?.tab);
      };
      window.addEventListener(OPEN_SETTINGS_EVENT, listener);
      return () => window.removeEventListener(OPEN_SETTINGS_EVENT, listener);
    },
    getFileUrl: (value: string) => browserSafeUrl(value),
    openFile: (value: string) => {
      const url = browserSafeUrl(value);
      if (url) window.open(url, '_blank', 'noopener,noreferrer');
    },
    openExternal: (url: string) => { window.open(url, '_blank', 'noopener,noreferrer'); },
    settingsChanged: () => {},
    syncWindowTheme: () => {},
    onSettingsChanged: () => noopUnsubscribe,
    onSwitchTab: () => noopUnsubscribe,
    onServerRestarted: () => noopUnsubscribe,
    appReady: () => {},
    getPlatform: async () => 'web',
    showNotification: () => {},
    getAppVersion: async () => '',
    getPendingAnnouncement: async () => null,
    ackAnnouncement: async () => {},
  };

  const reasons: Record<string, string> = {
    selectFolder: '移动端宿主无系统目录选择器，能力不存在而非空实现',
    selectFiles: '移动端宿主无系统文件选择器，能力不存在而非空实现',
    selectSkill: '移动端宿主无系统目录选择器，能力不存在而非空实现',
    writeFile: '移动端宿主不写本地文件系统，能力不存在而非空实现',
    writeFileBinary: '移动端宿主不写本地文件系统，能力不存在而非空实现',
    copyFile: '移动端宿主不复制本地文件，能力不存在而非空实现',
    writeFileIfUnchanged: '移动端宿主不写本地文件系统，能力不存在而非空实现',
    watchFile: '移动端宿主无文件系统监听能力',
    unwatchFile: '移动端宿主无文件系统监听能力',
    onFileChanged: '移动端宿主无文件变更事件源',
    openFolder: '移动端宿主不能打开系统文件管理器，能力不存在而非空实现',
    showInFinder: '移动端宿主不能打开系统文件管理器，能力不存在而非空实现',
    trashItem: '移动端宿主无回收站能力，能力不存在而非空实现',
    getFilePath: '移动端宿主无本地路径解析能力，能力不存在而非空实现',
    spawnViewer: '移动端宿主无派生 Viewer 窗口能力，能力不存在而非空实现',
    openBrowserViewer: '移动端宿主无内嵌浏览器查看器，能力不存在而非空实现',
    closeBrowser: '移动端宿主无内嵌浏览器会话，能力不存在而非空实现',
  };

  installDesktopHostApi(api as PlatformApi, 'mobile', reasons);
}

function browserSafeUrl(value: string): string {
  if (!value) return '';
  if (/^(https?:|data:|blob:)/i.test(value)) return value;
  if (value.startsWith('/api/')) return value;
  // 只放行正式资源内容地址，本机绝对路径仍不可用于浏览器。
  if (/^\/lingxi\/v1\/(?:resources\/[^/?#]+\/content|sessions\/[^/?#]+\/files\/[^/?#]+\/content)(?:[?#]|$)/.test(value)) return value;
  return '';
}
