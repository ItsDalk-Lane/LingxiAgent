import type { PlatformApi } from '../types';
import { getDesktopHost } from '../host/desktop-host';

declare global {
  interface Window {
    platform: PlatformApi;
  }
}

/**
 * 返回当前宿主的能力 API（Electron IPC / Web 受限宿主 / 移动端宿主）。
 * 永不为 undefined：宿主未显式安装时惰性创建 Web 受限宿主，
 * 受限能力经 explainHostCapability 得到明确解释而非 TypeError。
 */
export function usePlatform(): PlatformApi {
  return getDesktopHost().api;
}
