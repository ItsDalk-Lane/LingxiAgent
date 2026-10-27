/**
 * tauri-desktop-host.ts — Tauri 宿主实现（R09-T01，T08 缺口修复）
 *
 * 桥 = `window.__TAURI__`（`tauri.conf.json` 的 `withGlobalTauri: true` 注入），
 * 不引入 `@tauri-apps/api` 依赖：同一份 React 产物同时给 Electron 与 Tauri，
 * 静态 import Tauri API 会把 Tauri 运行时假设打进 Electron 产物（设计 §3.6）。
 *
 * 能力纪律（AGENTS.md 红线「禁止静默降级」，设计 D3）：
 * - 本文件是 T01 **boot 基线**的唯一边界：解析顺序 `hana > __TAURI__ > platform > web`
 *   由 `desktop-host.ts` 承担；
 * - T03/T04 的 `createTauriHostCapabilities` 片段在此**正式合并**（T08/A16 缺口：
 *   片段此前无消费者，PlatformApi 成员在 Tauri 下全部被 Proxy 拦截）；
 * - 未接线成员不返回 undefined、不做静默 no-op：要么登记 capabilityReason 的
 *   显式 no-op（返回可解除的 unlisten），要么调用即抛 `HostCapabilityError` 带
 *   reason（禁止静默）；
 * - `getFileUrl` 走 `__TAURI__.core.convertFileSrc`（asset protocol），缺失时抛
 *   显式错误并登记原因（Rust 侧 scope 需求见 docs/rust-tauri/R09/HOST_V2_PATCHES.md）；
 * - 右键菜单策略与 Electron `main.cjs:490-507` 等价：非可编辑区域拦截默认菜单，
 *   可编辑区域保留系统菜单（右键粘贴可用）；
 * - token 类命令只由宿主允许 main 窗口调用（Rust 侧再次校验 window label）。
 */

import type { PlatformApi } from '../types';
import type { DesktopHost } from './desktop-host';
import { HostCapabilityError } from './host-capability-error';
import {
  createTauriHostCapabilities,
  type TauriHostCapabilityFragment,
} from './tauri-host-capabilities';

export interface TauriListenEvent<T> {
  event: string;
  id: number;
  payload: T;
}

/** `withGlobalTauri` 暴露面的最小声明（只声明实际使用的成员，禁 any 扩散）。 */
export interface TauriGlobal {
  core: {
    invoke<T = unknown>(command: string, args?: Record<string, unknown>): Promise<T>;
    /** Tauri v2 withGlobalTauri 的 asset 协议 URL 转换；缺省时 getFileUrl 显式受限。 */
    convertFileSrc?(path: string): string;
  };
  event: {
    listen<T = unknown>(
      event: string,
      handler: (event: TauriListenEvent<T>) => void,
    ): Promise<() => void>;
  };
  /** Tauri v2 withGlobalTauri 的 webviewWindow 命名空间（drag-drop 原生路径来源）。 */
  webviewWindow?: {
    getCurrentWebviewWindow?(): {
      label?: string;
      onDragDropEvent?(handler: (event: {
        payload?: { type?: string; paths?: string[] };
      }) => void): Promise<() => void>;
    };
  };
}

declare global {
  interface Window {
    __TAURI__?: TauriGlobal;
  }
}

export const TAURI_RESTRICTED_REASON = 'R09-T03 迁移中：Tauri 宿主该能力尚未接线（禁止静默 no-op）';
export const TAURI_FILE_URL_REASON =
  'Tauri getFileUrl 需要 __TAURI__.core.convertFileSrc（withGlobalTauri）与 assetProtocol scope；'
  + 'Rust/配置侧 patch 见 docs/rust-tauri/R09/HOST_V2_PATCHES.md §1';
export const TAURI_ONBOARDING_REASON =
  'Rust 侧 onboarding_complete 当前注册为 NOT_IMPLEMENTED（T06 注册表）；'
  + '调用返回显式错误，T06 v2 实现/注册后自动转为真实完成路径（见 HOST_V2_PATCHES.md §3）';

/** debug 诊断：把受限成员的调用记进页面全局，宿主 DOM 投影据此归因首屏异常。 */
function recordRestrictedCapability(name: string, reason: string = TAURI_RESTRICTED_REASON): void {
  if (typeof window === 'undefined') return;
  const global = window as { __r09HostErrors?: Array<Record<string, unknown>> };
  if (!global.__r09HostErrors) global.__r09HostErrors = [];
  global.__r09HostErrors.push({ capability: name, reason, stack: new Error().stack?.slice(0, 900) ?? null });
}

function bridgeOrThrow(bridge?: TauriGlobal): TauriGlobal {
  const resolved = bridge ?? (typeof window !== 'undefined' ? window.__TAURI__ : undefined);
  if (!resolved || !resolved.core || typeof resolved.core.invoke !== 'function') {
    throw new Error('createTauriDesktopHost：window.__TAURI__ 未注入（当前不在 Tauri 环境）');
  }
  return resolved;
}

function warn(command: string, error: unknown): void {
  console.warn(`[tauri-desktop-host] ${command} failed:`, error);
}

type ServerConnectionInfoPayload = {
  port: number | null;
  token: string | null;
  serverNodeKind: string | null;
  serverNodeTransport: string;
  host?: string | null;
};

type ServerRestartedPayload = {
  port: number;
  token?: string | null;
  serverNodeKind?: string | null;
  serverNodeTransport?: string | null;
  host?: string | null;
};

/** 同步平台提示（仅用于片段内部的 win/linux 分支；权威平台以 getPlatform 命令为准）。 */
function detectPlatformHint(): string {
  const userAgent = typeof navigator !== 'undefined' ? navigator.userAgent : '';
  if (/Windows/i.test(userAgent)) return 'win32';
  if (/Linux/i.test(userAgent) && !/Android/i.test(userAgent)) return 'linux';
  return 'darwin';
}

let contextMenuPolicyInstalled = false;

/** 与 Electron main.cjs:490-507 等价的 renderer 层右键策略（T08 缺口修复）。 */
export function installTauriContextMenuPolicy(): void {
  if (contextMenuPolicyInstalled || typeof document === 'undefined') return;
  contextMenuPolicyInstalled = true;
  document.addEventListener(
    'contextmenu',
    (event: MouseEvent) => {
      const target = event.target as HTMLElement | null;
      const tag = target?.tagName ?? '';
      const editable =
        target?.isContentEditable === true
        || tag === 'INPUT'
        || tag === 'TEXTAREA'
        || tag === 'SELECT';
      if (!editable) event.preventDefault();
    },
    true,
  );
}

/** 测试隔离：移除右键策略安装标记（document 监听随 jsdom 环境重建）。 */
export function resetTauriContextMenuPolicyForTests(): void {
  contextMenuPolicyInstalled = false;
}

export function createTauriDesktopHost(bridge?: TauriGlobal): DesktopHost {
  const api = bridgeOrThrow(bridge);
  const invoke = <T = unknown>(command: string, args?: Record<string, unknown>): Promise<T> =>
    api.core.invoke<T>(command, args);
  const listen = <T = unknown>(
    event: string,
    handler: (payload: T) => void,
  ): (() => void) => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void api.event
      .listen<T>(event, (message) => handler(message.payload))
      .then((off) => {
        if (disposed) off();
        else unlisten = off;
      })
      .catch((error: unknown) => warn(`listen(${event})`, error));
    return () => {
      disposed = true;
      unlisten?.();
    };
  };

  const convertFileSrc = typeof api.core.convertFileSrc === 'function'
    ? api.core.convertFileSrc.bind(api.core)
    : undefined;

  // Tauri 没有 File.path/webUtils：原生 drag-drop 事件给绝对路径，宿主按 drop 顺序排队，
  // getFilePath 在紧随其后的 DOM drop 回调里按顺序取（无路径时返回 null，不伪造）。
  const currentWebview = api.webviewWindow?.getCurrentWebviewWindow?.();
  const dropPaths: string[] = [];
  let lastDropAt = 0;
  if (currentWebview?.onDragDropEvent) {
    void currentWebview.onDragDropEvent((event) => {
      const payload = event?.payload;
      if (payload?.type === 'drop' && Array.isArray(payload.paths)) {
        dropPaths.length = 0;
        dropPaths.push(...payload.paths.filter((path) => typeof path === 'string'));
        lastDropAt = Date.now();
      }
    }).catch((error: unknown) => warn('onDragDropEvent', error));
  }
  let browserConnectionKey: string | null = null;

  const fragment: TauriHostCapabilityFragment = createTauriHostCapabilities({
    invoke,
    listen,
    convertFileSrc,
    platform: detectPlatformHint(),
    currentWindowLabel: () => currentWebview?.label,
    browserConnectionKey: () => browserConnectionKey,
    setBrowserConnectionKey: (key) => {
      browserConnectionKey = key;
    },
    getFilePath: (_file: File) => {
      if (dropPaths.length === 0 || Date.now() - lastDropAt > 5_000) return null;
      return dropPaths.shift() ?? null;
    },
  });

  installTauriContextMenuPolicy();

  // 只保留片段没有覆盖的显式 no-op：有返回值语义的成员返回不产生副作用的空值，
  // capabilityReason 解释原因（不是静默降级）。
  const noopReasons: Record<string, string> = {
    onTrainUpdateAvailable: 'R09/R10 范围：Tauri OTA 列车更新事件链未接线；显式空监听，UI 保持零通知',
    onTrainUpdateProgress: 'R09/R10 范围：Tauri OTA 列车更新进度事件链未接线；显式空监听',
    trainUpdateStatus: 'R09/R10 范围：Tauri OTA 列车状态未接线；显式返回 null（不伪造更新状态，不静默声称最新）',
    setBrowserConnection: 'R09-T05 迁移中：Tauri 受控浏览器连接尚未接线（显式缺失，不假装已建立浏览器会话）',
    onTrainFallbackNotice: 'R09/R10 范围：Tauri OTA 崩溃回退广播未接线（显式缺失，UI 不伪造回退提示）',
    ackTrainFallbackNotice: 'R09/R10 范围：Tauri OTA 崩溃回退 ack 未接线（显式缺失）',
  };
  // v3：onShowSkillViewer 已由 Rust `show-skill-viewer` 事件真实接线，无显式 no-op 成员。
  const noops: Partial<PlatformApi> = {};
  // 可选成员：保持“缺失 + 可解释理由”（与 Web 宿主同一纪律，消费侧 ?.() 判空），
  // 不能用假函数冒充能力，也不能静默 no-op。
  const absentMembers = new Set([
    'onTrainUpdateAvailable',
    'onTrainUpdateProgress',
    'trainUpdateStatus',
    'setBrowserConnection',
    'onTrainFallbackNotice',
    'ackTrainFallbackNotice',
  ]);
  // Rust 侧尚未实现的成员：真实 invoke 调用 + 失败时显式 HostCapabilityError+理由。
  const pendingRustReasons: Record<string, string> = {
    onboardingComplete: TAURI_ONBOARDING_REASON,
    ...(convertFileSrc ? {} : { getFileUrl: TAURI_FILE_URL_REASON }),
  };

  const onboardingComplete = async (): Promise<void> => {
    try {
      await invoke('onboarding_complete');
    } catch (error) {
      throw new HostCapabilityError(
        'onboardingComplete',
        'tauri',
        `${TAURI_ONBOARDING_REASON}（原始错误：${error instanceof Error ? error.message : String(error)}）`,
      );
    }
  };

  const getFileUrl = (path: string): string => {
    if (!convertFileSrc) {
      recordRestrictedCapability('getFileUrl', TAURI_FILE_URL_REASON);
      throw new HostCapabilityError('getFileUrl', 'tauri', TAURI_FILE_URL_REASON);
    }
    return convertFileSrc(path);
  };

  const implemented: Partial<PlatformApi> = {
    // T03/T04 片段：覆盖成员均为真实实现（invoke/listen/convertFileSrc）。
    ...fragment.api,
    // ── boot 基线（app-init.ts 强依赖；优先于片段同名成员）────────────────
    getServerConnectionInfo: () => invoke<ServerConnectionInfoPayload>('get_server_connection_info'),
    getServerPort: async () => {
      const port = await invoke<number | null>('get_server_port');
      return port == null ? '' : String(port);
    },
    getServerToken: async () => (await invoke<string | null>('get_server_token')) ?? '',
    appReady: () => {
      void invoke('app_ready').catch((error: unknown) => warn('app_ready', error));
    },
    getPlatform: () => invoke<string>('get_platform'),
    getAppVersion: () => invoke<string>('get_app_version'),
    onServerRestarted: (callback: (data: ServerRestartedPayload) => void) =>
      listen<ServerRestartedPayload>('server-restarted', callback),
    openExternal: async (url: string) => {
      const result = await invoke<{ ok: boolean; error?: string }>('open_external', { url });
      return result;
    },
    // T08：asset 协议（或缺失时的显式错误）；onboarding 显式登记 pendingRust。
    getFileUrl,
    onboardingComplete,
    ...noops,
  };

  // 未接线成员：调用即抛显式错误（带 capability 与 reason），不是 undefined / no-op。
  const proxy = new Proxy(implemented as PlatformApi, {
    get(target, property, receiver) {
      if (typeof property === 'symbol' || property === 'then') {
        return Reflect.get(target, property, receiver);
      }
      if (Object.prototype.hasOwnProperty.call(target, property)) {
        return Reflect.get(target, property, receiver);
      }
      const name = String(property);
      if (absentMembers.has(name)) return undefined;
      const reason = fragment.capabilityReasons[name] ?? TAURI_RESTRICTED_REASON;
      return (..._args: unknown[]): never => {
        recordRestrictedCapability(name, reason);
        throw new HostCapabilityError(name, 'tauri', reason);
      };
    },
    has(target, property) {
      return Object.prototype.hasOwnProperty.call(target, property);
    },
  });

  return {
    kind: 'tauri',
    api: proxy,
    capabilityReason(name: string): string | null {
      if (noopReasons[name]) return noopReasons[name];
      if (pendingRustReasons[name]) return pendingRustReasons[name];
      if (Object.prototype.hasOwnProperty.call(implemented, name)) return null;
      return fragment.capabilityReasons[name] ?? TAURI_RESTRICTED_REASON;
    },
  };
}
