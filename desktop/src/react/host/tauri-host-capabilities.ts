/**
 * tauri-host-capabilities.ts — R09-T03/T04 DesktopHost-Tauri 能力片段
 *
 * 只导出**能力实现片段**，不创建 DesktopHost、不改注册表：由 host-core 的
 * `tauri-desktop-host.ts` 把 `api` 合并进它的 `implemented`，把
 * `capabilityReasons` 合并进它的理由表（未列出成员继续走它的 Proxy →
 * HostCapabilityError，禁止静默 no-op）。
 *
 * 桥约定（deps 注入，便于 jsdom 测试与不引入 `@tauri-apps/api` 依赖）：
 * - `invoke(command, args?)`：`window.__TAURI__.core.invoke`；
 * - `listen(event, handler)`：`window.__TAURI__.event.listen` 的 payload 投影，
 *   返回同步 unlisten（host-core 已实现同形帮助函数）；
 * - `convertFileSrc(path)`：asset 协议 URL 转换（可选；缺失时 getFileUrl 明确受限）；
 * - `platform`：`get_platform` 的 `darwin|win32|linux` 结果（可选）。
 *
 * Rust 侧命令签名与参数形状见 `docs/rust-tauri/R09/T03_T04_PROGRESS.md`
 * §注册清单（host-core 合并时以此为唯一契约）。
 */

import type {
  AutoLaunchStatus,
  DesktopNotificationReceipt,
  KeepAwakeStatus,
  PlatformApi,
  QuickChatOpenSessionRequest,
  QuickChatOpenSessionResult,
  QuickChatSessionTarget,
  ReleaseCheckResult,
  ReleaseDigest,
  SpeechPermissionState,
  UpdateDigestHistoryResult,
  WorkspaceChangePayload,
} from '../types';

export interface TauriHostCapabilityDeps {
  invoke<T = unknown>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T = unknown>(event: string, handler: (payload: T) => void): () => void;
  convertFileSrc?(path: string): string;
  platform?: string;
  /** 当前 webview label（viewer/skill-viewer 窗口用于派生 windowId）。 */
  currentWindowLabel?(): string | undefined;
  /** 浏览器连接不透明 key 的宿主内存储（setBrowserConnection 返回后由打开/请求命令复用）。 */
  browserConnectionKey?(): string | null;
  setBrowserConnectionKey?(key: string | null): void;
  /** Tauri 原生 drag-drop 路径映射（File 对象无路径；宿主在 drop 事件里排队绝对路径）。 */
  getFilePath?(file: File): string | null;
}

export interface TauriHostCapabilityFragment {
  api: Partial<PlatformApi>;
  /** 未接线成员的可解释理由（host-core 合并进 capabilityReason）。 */
  capabilityReasons: Readonly<Record<string, string>>;
}

/** 片段不提供、由其它任务/host-core 接管的成员的统一理由。 */
export const TAURI_CAPABILITY_REASONS: Readonly<Record<string, string>> = {
  // T05 浏览器/媒体接线
  closeBrowser: 'R09-T05 迁移中：closeBrowser 无生产消费者（T05 裁决）',
  browserGoBack: 'R09-T05 迁移中：浏览器查看器由 T05 接线',
  browserGoForward: 'R09-T05 迁移中：浏览器查看器由 T05 接线',
  browserReload: 'R09-T05 迁移中：浏览器查看器由 T05 接线',
  browserNewTab: 'R09-T05 迁移中：浏览器查看器由 T05 接线',
  browserSwitchTab: 'R09-T05 迁移中：浏览器查看器由 T05 接线',
  browserCloseTab: 'R09-T05 迁移中：浏览器查看器由 T05 接线',
  browserEmergencyStop: 'R09-T05 迁移中：浏览器急停由 T05 接线',
  // R10 功能补齐：About 更新链的检测/历史/升级公告已实现；安装型自动更新与
  // OTA 列车仍受发布通道与签名裁决约束（OWNER-03：不做正式签名、不声称自动更新）。
  autoUpdateCheck: '发布通道裁决未授权：OWNER-03 不做正式签名/自动更新；本轮不实现',
  autoUpdateInstall: '发布通道裁决未授权：OWNER-03 不做正式签名/自动更新；本轮不实现',
  autoUpdateState: '发布通道裁决未授权：OWNER-03 不做正式签名/自动更新；本轮不实现',
  autoUpdateSetChannel: '发布通道裁决未授权：OWNER-03 不做正式签名/自动更新；本轮不实现',
  onAutoUpdateState: '发布通道裁决未授权：OWNER-03 不做正式签名/自动更新；本轮不实现',
  trainUpdateStatus: 'OTA 发布通道未建立（无 channel manifest/latest.json）；本轮不实现',
  trainUpdateCheck: 'OTA 发布通道未建立（无 channel manifest/latest.json）；本轮不实现',
  trainUpdateApply: 'OTA 发布通道未建立（无 channel manifest/latest.json）；本轮不实现',
  onTrainUpdateAvailable: 'OTA 发布通道未建立（无 channel manifest/latest.json）；本轮不实现',
  onTrainUpdateProgress: 'OTA 发布通道未建立（无 channel manifest/latest.json）；本轮不实现',
  onTrainFallbackNotice: 'OTA 发布通道未建立（无 channel manifest/latest.json）；本轮不实现',
  ackTrainFallbackNotice: 'OTA 发布通道未建立（无 channel manifest/latest.json）；本轮不实现',
  // 平台/契约限制
  cardFrameNavigationGuard: 'Tauri/Wry 无 will-frame-navigate 等价：保持 flag=false，HTTP 卡片降级为仅同源（OQ-07）',
  debugOpenOnboarding: 'debug 构建专属：release 命令不存在（T06 裁决）',
  debugOpenOnboardingPreview: 'debug 构建专属：release 命令不存在（T06 裁决）',
};

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

/** Uint8Array → base64（Tauri command 的二进制参数走字符串，避免 JSON 数字数组膨胀）。 */
function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  const chunkSize = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    const slice = bytes.subarray(offset, offset + chunkSize);
    binary += String.fromCharCode(...slice);
  }
  return globalThis.btoa(binary);
}

/** 通知 options 只接受 desktopFocusPolicy 枚举（Rust 侧同规则校验）。 */
function normalizeNotificationOptions(
  options: { desktopFocusPolicy?: 'when_unfocused' | 'always' } | undefined,
): { desktopFocusPolicy: 'when_unfocused' | 'always' } {
  return {
    desktopFocusPolicy: options?.desktopFocusPolicy === 'when_unfocused'
      ? 'when_unfocused'
      : 'always',
  };
}


/** viewer-<n> / skill-viewer-<n> label → 数字 windowId（Electron 语义）；无法解析返回 null。 */
function windowIdFromLabel(label: string | undefined | null): number | null {
  if (!label) return null;
  const match = /(\d+)\s*$/.exec(label);
  if (!match) return null;
  const value = Number(match[1]);
  return Number.isSafeInteger(value) ? value : null;
}

function extensionOf(path: string | null | undefined): string {
  if (!path) return '';
  const base = path.split(/[\\/]/).pop() ?? '';
  const dot = base.lastIndexOf('.');
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : '';
}

/** 从文件扩展名推导 PreviewItem.type 的最近似值（Rust ViewerPayload 不含 type）。 */
function previewTypeFromPath(path: string | null | undefined): string {
  const ext = extensionOf(path);
  if (['md', 'markdown'].includes(ext)) return 'markdown';
  if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp'].includes(ext)) return 'image';
  if (['mp4', 'mov', 'webm', 'mkv', 'avi'].includes(ext)) return 'video';
  if (['mp3', 'wav', 'm4a', 'flac', 'ogg'].includes(ext)) return 'audio';
  if (['pdf'].includes(ext)) return 'pdf';
  if (['html', 'htm'].includes(ext)) return 'html';
  if (['json', 'yaml', 'yml', 'toml', 'js', 'ts', 'tsx', 'jsx', 'css', 'py', 'rs', 'sh', 'sql'].includes(ext)) return 'code';
  return 'text';
}

function basename(path: string | null | undefined): string {
  if (!path) return '';
  return path.split(/[\\/]/).filter(Boolean).pop() ?? '';
}

function browserTarget(
  target: { connectionKey?: string; sessionPath?: string | null } | string | undefined,
  storedKey: string | null,
): { connectionKey: string; sessionPath: string } {
  if (typeof target === 'string') {
    // 调用面只接受 {connectionKey, sessionPath}；字符串形态没有会话路径可安全推导。
    return { connectionKey: storedKey ?? '', sessionPath: target };
  }
  return {
    connectionKey: target?.connectionKey || storedKey || '',
    sessionPath: target?.sessionPath ?? '',
  };
}

export function createTauriHostCapabilities(
  deps: TauriHostCapabilityDeps,
): TauriHostCapabilityFragment {
  const invoke = deps.invoke;
  const listen = deps.listen;
  const platform = deps.platform ?? '';
  const isWindowsLike = platform === 'win32' || platform === 'linux';
  const currentWindowLabel = () => deps.currentWindowLabel?.() ?? '';
  const storedBrowserKey = () => deps.browserConnectionKey?.() ?? null;

  const api: Partial<PlatformApi> = {
    // ── 应用信息（host-core 已有同名实现时以 host-core 为准）──────────
    getPlatform: () => invoke<string>('get_platform'),
    getAppVersion: () => invoke<string>('get_app_version'),

    // ── 窗口控制（只作用于调用者窗口；label 门在 Rust 侧）────────────
    windowMinimize: () => {
      void invoke('window_minimize').catch(() => undefined);
    },
    windowMaximize: () => {
      void invoke('window_maximize').catch(() => undefined);
    },
    windowClose: () => {
      void invoke('window_close').catch(() => undefined);
    },
    windowIsMaximized: () => invoke<boolean>('window_is_maximized'),
    onMaximizeChange: (callback: (maximized: boolean) => void) => {
      // 现役 Electron 事件名 window-maximized / window-unmaximized（mac 无自绘标题栏）
      const offMax = listen<unknown>('window-maximized', () => callback(true));
      const offMin = listen<unknown>('window-unmaximized', () => callback(false));
      return () => {
        offMax();
        offMin();
      };
    },

    // ── 重载 / 重启 / 主题 ────────────────────────────────────────
    reloadMainWindow: async () => {
      await invoke('reload_main_window');
    },
    restartApp: () => invoke<{ ok: boolean }>('restart_app'),
    syncWindowTheme: (theme: string) => {
      // 现役命令名 window_theme_changed（host-core 已注册，保持一致）。
      void invoke('window_theme_changed', { theme }).catch(() => undefined);
    },

    // ── 设置窗口 / 跨窗广播 ──────────────────────────────────────
    openSettings: (tab?: string) => {
      void invoke('open_settings', { tab: tab ?? null }).catch(() => undefined);
    },
    settingsChanged: (event: string, payload?: unknown) => {
      void invoke('settings_changed', { eventType: event, data: payload ?? null }).catch(
        () => undefined,
      );
    },
    onSettingsChanged: (callback: (event: string, payload: unknown) => void) => {
      return listen<{ type?: string; data?: unknown }>('settings-changed', (payload) => {
        if (typeof payload?.type !== 'string') return;
        callback(payload.type, payload.data);
      });
    },
    onOpenSettingsModal: (callback: (tab?: string) => void) =>
      listen<{ tab?: string }>('open-settings-modal', (payload) => callback(payload?.tab)),
    onSwitchTab: (callback: (tab: string) => void) =>
      listen<{ tab?: string }>('settings-switch-tab', (payload) => {
        if (typeof payload?.tab === 'string') callback(payload.tab);
      }),

    // ── 通知 ────────────────────────────────────────────────────
    showNotification: async (
      title: string,
      body: string,
      agentId?: string | null,
      options?: { desktopFocusPolicy?: 'when_unfocused' | 'always' },
    ): Promise<DesktopNotificationReceipt> => {
      try {
        return await invoke<DesktopNotificationReceipt>('show_notification', {
          title,
          body,
          agentId: agentId ?? null,
          options: normalizeNotificationOptions(options),
        });
      } catch (error) {
        // 宿主失败如实回执（不伪装成已展示，也不 throw 打断调用方）。
        console.warn('[tauri-host-capabilities] show_notification failed:', errorMessage(error));
        return { shown: false, reason: 'failed' };
      }
    },

    // ── 系统语音识别授权（T04；命令只允许主窗口调用）──────────────
    speechPermissionStatus: async (): Promise<SpeechPermissionState> => {
      const result = await invoke<{ state: SpeechPermissionState }>('speech_permission_status');
      return result?.state ?? 'bridge_unavailable';
    },
    speechPermissionRequest: async (): Promise<SpeechPermissionState> => {
      const result = await invoke<{ state: SpeechPermissionState }>('speech_permission_request');
      return result?.state ?? 'bridge_unavailable';
    },

    // ── Office 预览（R09-T03）：宿主经本机服务受限 fs 根转换 DOCX/XLSX 为 HTML ──
    readDocxHtml: (path: string) => invoke<string | null>('read_docx_html', { path }),
    readXlsxHtml: (path: string) => invoke<string | null>('read_xlsx_html', { path }),

    // ── 系统对话框 ──────────────────────────────────────────────
    selectFolder: () => invoke<string | null>('select_folder'),
    selectFiles: async (options?: { multiple?: boolean }) =>
      (await invoke<string[] | null>('select_files', {
        multiple: options?.multiple ?? true,
      })) ?? [],
    selectSkill: () => invoke<string | null>('select_skill'),

    // ── 外链 / 路径打开 / 回收站 ─────────────────────────────────
    openExternal: async (url: string) => {
      try {
        return await invoke<{ ok: boolean; error?: string }>('open_external', { url });
      } catch (error) {
        return { ok: false, error: errorMessage(error) };
      }
    },
    openFolder: (path: string) => {
      void invoke('open_folder', { path }).catch(() => undefined);
    },
    openFile: (path: string) => {
      void invoke('open_file', { path }).catch(() => undefined);
    },
    showInFinder: (path: string) => {
      void invoke('show_in_finder', { path }).catch(() => undefined);
    },
    trashItem: (path: string) => invoke<boolean>('trash_item', { path }),

    // ── 拖出（Rust 侧校验绝对路径/形状）────────────────────────────
    startDrag: (filePaths: string | string[]) => {
      const paths = Array.isArray(filePaths) ? filePaths : [filePaths];
      void invoke('start_drag', { paths }).catch(() => undefined);
    },

    // ── 剪贴板编辑命令（4 个允许值）──────────────────────────────
    runEditCommand: (command: 'cut' | 'copy' | 'paste' | 'selectAll') =>
      invoke<boolean>('run_edit_command', { command }),

    // ── 保持唤醒 / 自启动 ────────────────────────────────────────
    getKeepAwakeStatus: () => invoke<KeepAwakeStatus>('get_keep_awake_status'),
    setKeepAwakeEnabled: (enabled: boolean) =>
      invoke<KeepAwakeStatus>('set_keep_awake_enabled', { enabled }),
    getAutoLaunchStatus: () => invoke<AutoLaunchStatus>('get_auto_launch_status'),
    setAutoLaunchEnabled: (enabled: boolean) =>
      invoke<AutoLaunchStatus>('set_auto_launch_enabled', { enabled }),

    // ── 全局快捷键 ──────────────────────────────────────────────
    keybindingsReloadGlobal: () =>
      invoke<{ ok: boolean; shortcut: string; error?: string }>('keybindings_reload_global'),
    quickChatReloadShortcut: () =>
      invoke<{ ok: boolean; shortcut: string; error?: string }>('quick_chat_reload_shortcut'),
    keybindingsTestRegister: (shortcut: string) =>
      invoke<{ ok: boolean; shortcut: string; self?: boolean; error?: string }>(
        'keybindings_test_register',
        { shortcut },
      ),
    quickChatShortcutStatus: () =>
      invoke<{ shortcut: string; registered: boolean; shortcuts?: string[] }>(
        'quick_chat_shortcut_status',
      ),

    // ── Quick Chat 窗口 / 会话交接 ───────────────────────────────
    quickChatShow: () => {
      void invoke('quick_chat_show').catch(() => undefined);
    },
    quickChatHide: () => {
      void invoke('quick_chat_hide').catch(() => undefined);
    },
    quickChatResize: (request: 'compact' | 'chat' | { mode: 'compact' | 'chat'; height?: number }) => {
      const payload = typeof request === 'string'
        ? { mode: request, height: null }
        : { mode: request.mode, height: request.height ?? null };
      void invoke('quick_chat_resize', payload).catch(() => undefined);
    },
    quickChatOpenSession: (target: QuickChatSessionTarget) =>
      invoke<QuickChatOpenSessionResult>('quick_chat_open_session', {
        target: {
          sessionPath: target.sessionPath,
          sessionId: target.sessionId,
          ownerKey: target.ownerKey,
        },
      }),
    onQuickChatOpenSession: (
      callback: (payload: QuickChatOpenSessionRequest) => Promise<QuickChatOpenSessionResult>,
    ) => {
      // 主窗收到打开请求后必须回 ack（host-core 在 listen 回调里回 invoke）
      return listen<QuickChatOpenSessionRequest>('quick-chat-open-session', (payload) => {
        void (async () => {
          let result: QuickChatOpenSessionResult;
          try {
            result = await callback(payload);
          } catch (error) {
            result = { ok: false, code: 'QUICK_CHAT_SWITCH_FAILED', message: errorMessage(error) };
          }
          try {
            await invoke('quick_chat_open_session_ack', {
              result: { ...result, requestId: payload?.requestId ?? null },
            });
          } catch {
            // ack 失败由主进程超时兜底；不重复发送。
          }
        })();
      });
    },
    onQuickChatShown: (callback: () => void) => listen<unknown>('quick-chat-shown', () => callback()),

    // ── Splash / Onboarding ─────────────────────────────────────
    getAvatarPath: (role: string) => invoke<string | null>('get_avatar_path', { role }),
    getSplashInfo: () =>
      invoke<{ agentName?: string; locale?: string; yuan?: string } | null>('get_splash_info'),
    onboardingComplete: async () => {
      await invoke('onboarding_complete');
    },

    // ── 文件读/写（file-text-io：R06 真实命令，参数名以 lib.rs 为准）────────
    readFile: (path: string) => invoke<string | null>('read_file', { path }),
    readFileSnapshot: (path: string) => invoke<unknown>('read_file_snapshot', { path }) as never,
    readFileBase64: (path: string) => invoke<string | null>('read_file_base64', { path }),
    writeFile: (filePath: string, content: string) =>
      invoke<boolean>('write_file', { path: filePath, content }),
    writeFileBinary: (filePath: string, base64Data: string) =>
      invoke<boolean>('write_file_binary', { path: filePath, data: base64Data }),
    writeFileIfUnchanged: (filePath: string, content: string, expectedVersion?: unknown) =>
      invoke<unknown>('write_file_if_unchanged', {
        path: filePath,
        content,
        expectedVersion: expectedVersion ?? null,
      }) as never,
    copyFile: (sourcePath: string, destinationPath: string) =>
      invoke<boolean>('copy_file', { source: sourcePath, destination: destinationPath }),
    watchFile: (filePath: string) => invoke<boolean>('watch_file', { path: filePath }),
    unwatchFile: (filePath: string) => invoke<boolean>('unwatch_file', { path: filePath }),
    onFileChanged: (callback: (filePath: string) => void) =>
      listen<{ filePath?: string }>('file-changed', (payload) => {
        if (typeof payload?.filePath === 'string') callback(payload.filePath);
      }),

    // ── 派生 viewer（detached-viewer：R06 spawn_viewer/viewer_request_load/viewer_close）──
    spawnViewer: async (data: { filePath: string; title: string; type: string; language?: string | null }) => {
      const label = await invoke<string | null>('spawn_viewer', { path: data.filePath });
      return windowIdFromLabel(label);
    },
    viewerRequestLoad: async () => {
      const payload = await invoke<{
        filePath?: string; file_path?: string; title?: string; readOnly?: boolean; read_only?: boolean;
      } | null>('viewer_request_load');
      if (!payload) return null;
      const filePath = payload.filePath ?? payload.file_path ?? '';
      return {
        filePath,
        title: payload.title ?? basename(filePath),
        type: previewTypeFromPath(filePath),
        language: null,
        windowId: windowIdFromLabel(currentWindowLabel()) ?? 0,
      };
    },
    viewerClose: () => {
      void invoke('viewer_close').catch(() => undefined);
    },

    // ── Skill viewer（skill-viewer：open_skill_viewer/list_skill_files/read_skill_file）──
    openSkillViewer: (opts?: { skillPath?: string; name?: string; baseDir?: string; filePath?: string; installed?: boolean }) => {
      const baseDir = opts?.baseDir ?? opts?.skillPath ?? '';
      const data = {
        name: opts?.name ?? basename(baseDir),
        baseDir,
        filePath: opts?.filePath ?? null,
        installed: opts?.installed === true,
      };
      void invoke('open_skill_viewer', { data }).catch(() => undefined);
    },
    listSkillFiles: (baseDir: string) => invoke<unknown[]>('list_skill_files', { baseDir }),
    readSkillFile: (filePath: string) => invoke<string | null>('read_skill_file', { path: filePath }),

    // ── 浏览器查看器（browser-viewer：五个 Rust 命令 + connection-changed 事件）──
    setBrowserConnection: async (connection: import('../services/server-connection').ServerConnection | null) => {
      const key = await invoke<string | null>('set_browser_connection', {
        connection: connection
          ? {
              connectionId: connection.connectionId,
              kind: connection.kind,
              baseUrl: connection.baseUrl,
              token: connection.token,
              serverId: connection.serverId,
              serverNodeKind: connection.serverNodeKind,
              serverNodeId: connection.serverNodeId,
              serverNodeTransport: connection.serverNodeTransport,
              userId: connection.userId,
              studioId: connection.studioId,
              authState: connection.authState,
              trustState: connection.trustState,
              credentialKind: connection.credentialKind,
              label: connection.label,
            }
          : null,
      });
      deps.setBrowserConnectionKey?.(key ?? null);
      return key;
    },
    getBrowserViewerContext: () => invoke<unknown>('get_browser_viewer_context') as never,
    openBrowserViewer: (target?: string | { connectionKey?: string; sessionPath?: string | null }) => {
      void invoke('open_browser_viewer', { target: browserTarget(target, storedBrowserKey()) }).catch(() => undefined);
    },
    rustBrowserRequest: (target: { connectionKey?: string; sessionPath?: string | null }, action: string, options?: { tabId?: string; url?: string }) =>
      invoke<unknown>('rust_browser_request', {
        target: browserTarget(target, storedBrowserKey()),
        action,
        options: options ?? null,
      }),
    closeBrowserViewer: () => {
      void invoke('close_browser_viewer').catch(() => undefined);
    },
    onBrowserUpdate: (callback: (data: { running?: boolean; reason?: string | null }) => void) =>
      listen<{ active?: boolean; reason?: string }>('browser:connection-changed', (payload) =>
        callback({ running: payload?.active, reason: payload?.reason ?? null }),
      ),

    // ── v3 Rust 补齐：workspace 监听 / viewer-closed / show-skill-viewer ──
    watchWorkspace: (rootPath: string) => invoke<boolean>('watch_workspace', { rootPath }),
    unwatchWorkspace: (rootPath: string) => invoke<boolean>('unwatch_workspace', { rootPath }),
    onWorkspaceChanged: (callback: (payload: WorkspaceChangePayload) => void) =>
      listen<WorkspaceChangePayload>('workspace-changed', (payload) => callback(payload)),
    onViewerClosed: (callback: (windowId: number) => void) =>
      listen<number>('viewer-closed', (windowId) => {
        if (typeof windowId === 'number') callback(windowId);
      }),
    onShowSkillViewer: (callback: (data: unknown) => void) =>
      listen<unknown>('show-skill-viewer', (data) => callback(data)),

    // ── 文件拖入路径（Tauri 无 File.path；宿主 drop 事件队列 → getFilePath）──
    ...(deps.getFilePath ? { getFilePath: (file: File) => deps.getFilePath!(file) } : {}),

    // ── About 更新链（R10）：固定 GitHub 端点；超时/尺寸/cache 在 Rust 侧 ──
    releaseCheckLatest: () => invoke<ReleaseCheckResult>('release_check_latest'),
    getUpdateDigestHistory: () =>
      invoke<UpdateDigestHistoryResult>('get_update_digest_history'),
    getPendingAnnouncement: () =>
      invoke<{ version: string; entries: ReleaseDigest[] } | null>('get_pending_announcement'),
    ackAnnouncement: async () => {
      await invoke('ack_announcement');
    },

    // ── 可观测导出（R10）：exportId 是绑定发起窗口的不透明 capability token；
    //    chunk 用 base64 过 IPC（Rust 侧解码并限制单块 ≤4MB）。
    observabilityExportBegin: (payload: { defaultFileName: string }) =>
      invoke<
        { canceled: true } | { canceled: false; exportId: string; filePath: string }
      >('observability_export_begin', { defaultFileName: payload.defaultFileName }),
    observabilityExportWrite: (payload: { exportId: string; chunk: Uint8Array }) =>
      invoke<{ bytesWritten: number }>('observability_export_write', {
        exportId: payload.exportId,
        chunk: bytesToBase64(payload.chunk),
      }),
    observabilityExportEnd: (payload: { exportId: string }) =>
      invoke<{ bytesWritten: number; filePath: string }>('observability_export_end', {
        exportId: payload.exportId,
      }),
    observabilityExportAbort: (payload: { exportId: string }) =>
      invoke<{ aborted: boolean }>('observability_export_abort', { exportId: payload.exportId }),

    // ── 文件 URL（asset 协议；无 convertFileSrc 时显式受限）────────
    getFileUrl: (path: string) => {
      if (!deps.convertFileSrc) {
        throw new Error(
          'Tauri getFileUrl 需要 __TAURI__.core.convertFileSrc（withGlobalTauri）与 assetProtocol scope 配置',
        );
      }
      return deps.convertFileSrc(path);
    },
  };

  // 自绘标题栏只存在于 Windows/Linux（与现役 WindowControls.tsx 使用面一致）；
  // mac 保留方法但调用无意义——不隐藏能力，由调用方按 platform 判定。
  void isWindowsLike;

  return {
    api,
    capabilityReasons: TAURI_CAPABILITY_REASONS,
  };
}
