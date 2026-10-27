/**
 * desktop-host.ts — DesktopHost 注册表与访问器（R08-T02）
 *
 * DesktopHost 是「宿主能力」的统一接口：Electron（preload 桥全能力）、
 * Web 受限宿主、移动端宿主三选一安装。业务代码一律经 usePlatform() /
 * getDesktopHost() 访问，禁止直连 window.hana / window.platform。
 *
 * 本文件是边界白名单成员：探测 window.hana 的存在性、收编既有
 * window.platform（测试桩兼容），并在安装时写 window.platform 别名。
 * 桥身份（hana/platform 对象身份）变化时自动重解析，测试逐用例重挂
 * window.hana 桩不会被过期缓存遮蔽。
 */

import type { PlatformApi } from '../types';
import { createElectronDesktopHost } from './electron-desktop-host';
import { createTauriDesktopHost } from './tauri-desktop-host';
import { createWebDesktopHost } from './web-desktop-host';

export type DesktopHostKind = 'electron' | 'tauri' | 'web' | 'mobile';

export interface DesktopHost {
  readonly kind: DesktopHostKind;
  readonly api: PlatformApi;
  /** 受限能力的可解释原因；能力可用或未登记时返回 null。 */
  capabilityReason(name: string): string | null;
}

let currentHost: DesktopHost | null = null;
// 桥身份快照：window.hana / window.platform 的身份发生变化（测试逐用例
// 重挂桩、宿主热切换）时，getDesktopHost 按当前身份重新解析宿主，
// 不会返回已过期的缓存宿主。生产环境桥身份稳定，仅解析一次。
let lastHana: PlatformApi | null = null;
let lastPlatform: PlatformApi | null = null;
let lastTauri: unknown = null;

class BasicDesktopHost implements DesktopHost {
  constructor(
    readonly kind: DesktopHostKind,
    readonly api: PlatformApi,
    private readonly reasons: Readonly<Record<string, string>> = {},
  ) {}

  capabilityReason(name: string): string | null {
    return this.reasons[name] ?? null;
  }
}

function snapshotBridgeIdentity(): void {
  if (typeof window === 'undefined') {
    lastHana = null;
    lastPlatform = null;
    lastTauri = null;
    return;
  }
  lastHana = window.hana ?? null;
  lastPlatform = window.platform ?? null;
  lastTauri = window.__TAURI__ ?? null;
}

export function installDesktopHost(host: DesktopHost): void {
  currentHost = host;
  if (typeof window !== 'undefined') {
    // 别名仅为向后兼容（既有测试桩 / 残留读取）；业务代码不得直连。
    window.platform = host.api;
  }
  snapshotBridgeIdentity();
}

export function installDesktopHostApi(
  api: PlatformApi,
  kind: DesktopHostKind,
  reasons: Readonly<Record<string, string>> = {},
): DesktopHost {
  const host = new BasicDesktopHost(kind, api, reasons);
  installDesktopHost(host);
  return host;
}

/**
 * 返回当前宿主，永不为 undefined。按 window.hana / window.__TAURI__ /
 * window.platform 的当前身份解析并缓存；桥身份不变时直接返回缓存宿主：
 * window.hana 存在 → Electron 宿主（优先，preload 桥为唯一全能力来源）；
 * 否则 window.__TAURI__ 存在 → Tauri 宿主（R09-T01，withGlobalTauri 全局桥）；
 * 否则 window.platform 存在 → 收编既有别名（platform.js 的 Electron 抢先
 * 安装 / 显式 installDesktopHost / 旧测试桩）；
 * 否则 → 惰性创建 Web 受限宿主并写 window.platform 别名。
 * 惰性 Web 宿主让纯浏览器 / jsdom 组件测试无需注入任何全局即可运行，
 * 受限能力得到明确解释而非 TypeError。
 */
export function getDesktopHost(): DesktopHost {
  if (typeof window === 'undefined') {
    if (!currentHost) currentHost = createWebDesktopHost();
    return currentHost;
  }
  const hana = window.hana ?? null;
  const tauri = window.__TAURI__ ?? null;
  const platform = window.platform ?? null;
  if (
    currentHost
    && hana === lastHana
    && tauri === lastTauri
    && platform === lastPlatform
  ) {
    return currentHost;
  }
  let host: DesktopHost;
  if (hana) {
    host = createElectronDesktopHost(hana);
  } else if (tauri) {
    host = createTauriDesktopHost(tauri);
    // 兼容旧读取：React 启动前可能还有代码经 window.platform 取值；
    // 与 Electron 分支一样安装别名（业务代码仍必须经 DesktopHost）。
    window.platform = host.api;
  } else if (platform) {
    // 收编既有 platform 对象：显式安装的宿主已被快照短路返回，这里只可能是
    // platform.js 的 Electron 抢先别名（会与 hana 同体，走不到本分支）或外部测试桩。
    host = new BasicDesktopHost('web', platform);
  } else {
    host = createWebDesktopHost();
    window.platform = host.api;
  }
  currentHost = host;
  snapshotBridgeIdentity();
  return host;
}

/**
 * 能力受限的可解释原因。成员不存在时给出宿主级说明；成员存在但登记过
 * 理由（显式 no-op）时返回该理由；Electron 全能力或未登记时返回 null。
 */
export function explainHostCapability(name: string): string | null {
  const host = getDesktopHost();
  const registered = host.capabilityReason(name);
  if (registered) return registered;
  if (typeof host.api[name] === 'function') return null;
  if (host.kind === 'electron') return null;
  return `当前宿主（${host.kind}）未提供该能力`;
}

/** 测试隔离用：清空已安装宿主与桥身份快照（连同 window.platform 别名）。 */
export function resetDesktopHostForTests(): void {
  currentHost = null;
  if (typeof window !== 'undefined') {
    delete (window as { platform?: PlatformApi }).platform;
  }
  snapshotBridgeIdentity();
}
