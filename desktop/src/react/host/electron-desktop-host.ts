/**
 * electron-desktop-host.ts — Electron 宿主实现（R08-T02）
 *
 * 这是唯一直读 window.hana 的 TS 文件（边界白名单成员）。
 * api 即 preload 注入的桥本体，全能力可用，无受限理由。
 */

import type { PlatformApi } from '../types';
import type { DesktopHost } from './desktop-host';

export function createElectronDesktopHost(bridge?: PlatformApi): DesktopHost {
  const api = bridge ?? (typeof window !== 'undefined' ? window.hana : undefined);
  if (!api) {
    throw new Error('createElectronDesktopHost：window.hana 未注入（当前不在 Electron 环境）');
  }
  return {
    kind: 'electron',
    api,
    capabilityReason: () => null,
  };
}
