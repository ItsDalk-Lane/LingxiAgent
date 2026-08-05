import {
  PLUGIN_SURFACE_SESSION_HEADER,
  PLUGIN_SURFACE_SESSION_QUERY,
  PLUGIN_UI_CAPABILITY,
  PLUGIN_UI_PROTOCOL,
  PLUGIN_UI_PROTOCOL_VERSION,
  parsePluginUiMessage,
  type PluginResourceOpenInput,
  type PluginResourceOpenResult,
  type PluginResourcePickInput,
  type PluginResourcePickResult,
  type PluginResourceRequestAccessInput,
  type PluginResourceRequestAccessResult,
  type PluginUiError,
  type PluginUiMessage,
} from '@lingxi/plugin-protocol';

export interface LingxiPluginSize {
  width?: number;
  height?: number;
}

export interface LingxiPluginThemeSnapshot {
  theme?: string;
  cssUrl?: string;
}

export interface LingxiPluginRequestOptions {
  timeoutMs?: number;
}

export type LingxiToastType = 'success' | 'error' | 'info' | 'warning';

export interface LingxiToastShowInput {
  message: string;
  type?: LingxiToastType;
  duration?: number;
}

export interface LingxiToastShowResult {
  shown: boolean;
}

export type LingxiExternalOpenInput = string | { url: string };

export interface LingxiExternalOpenResult {
  opened: boolean;
}

export type LingxiClipboardWriteTextInput = string | { text: string };

export interface LingxiClipboardWriteTextResult {
  written: boolean;
}

export interface LingxiPluginSdkOptions {
  parentWindow?: Window;
  targetWindow?: Window;
  targetOrigin?: string;
  requestTimeoutMs?: number;
  idFactory?: () => string;
}

export interface LingxiPluginSdk {
  ready(payload?: unknown): void;
  assets: {
    url(path: string): string;
  };
  api: {
    url(path: string): string;
    fetch(path: string, init?: RequestInit): Promise<Response>;
  };
  ui: {
    resize(size: LingxiPluginSize): void;
  };
  theme: {
    getSnapshot(): LingxiPluginThemeSnapshot;
    subscribe(callback: (theme: LingxiPluginThemeSnapshot) => void): () => void;
  };
  host: {
    request<T = unknown>(
      type: string,
      payload?: unknown,
      options?: LingxiPluginRequestOptions,
    ): Promise<T>;
  };
  toast: {
    show(input: LingxiToastShowInput, options?: LingxiPluginRequestOptions): Promise<LingxiToastShowResult>;
  };
  external: {
    open(input: LingxiExternalOpenInput, options?: LingxiPluginRequestOptions): Promise<LingxiExternalOpenResult>;
  };
  clipboard: {
    writeText(
      input: LingxiClipboardWriteTextInput,
      options?: LingxiPluginRequestOptions,
    ): Promise<LingxiClipboardWriteTextResult>;
  };
  resources: {
    open(
      input: PluginResourceOpenInput,
      options?: LingxiPluginRequestOptions,
    ): Promise<PluginResourceOpenResult>;
    pick(
      input?: PluginResourcePickInput,
      options?: LingxiPluginRequestOptions,
    ): Promise<PluginResourcePickResult>;
    requestAccess(
      input: PluginResourceRequestAccessInput,
      options?: LingxiPluginRequestOptions,
    ): Promise<PluginResourceRequestAccessResult>;
  };
}

export class LingxiPluginError extends Error {
  override name = 'LingxiPluginError';
  readonly code: string;
  readonly details?: unknown;

  constructor(error: PluginUiError) {
    super(error.message);
    this.code = error.code;
    this.details = error.details;
  }
}

let fallbackIdSeq = 0;

function defaultIdFactory(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  fallbackIdSeq += 1;
  return `hana-plugin-${Date.now()}-${fallbackIdSeq}`;
}

function getBrowserWindow(): Window {
  if (typeof window === 'undefined') {
    throw new Error('@lingxi/plugin-sdk requires a browser iframe window.');
  }
  return window;
}

function safeOriginFromUrl(value: string | null | undefined): string | null {
  if (!value) return null;
  try {
    return new URL(value).origin;
  } catch {
    return null;
  }
}

function resolveTargetOrigin(targetWindow: Window, explicit?: string): string {
  if (explicit) return explicit;

  const hostOrigin = new URLSearchParams(targetWindow.location.search).get('hana-host-origin');
  if (hostOrigin) return hostOrigin;

  return safeOriginFromUrl(targetWindow.document.referrer) ?? '*';
}

function readInitialTheme(targetWindow: Window): LingxiPluginThemeSnapshot {
  const params = new URLSearchParams(targetWindow.location.search);
  return {
    theme: params.get('hana-theme') ?? undefined,
    cssUrl: params.get('hana-css') ?? undefined,
  };
}

function isTrustedHostEvent(event: MessageEvent, parentWindow: Window, targetOrigin: string): boolean {
  if (event.source !== parentWindow) return false;
  if (targetOrigin !== '*' && event.origin !== targetOrigin) return false;
  return true;
}

function externalOpenPayload(input: LingxiExternalOpenInput): { url: string } {
  return typeof input === 'string' ? { url: input } : input;
}

function clipboardWriteTextPayload(input: LingxiClipboardWriteTextInput): { text: string } {
  return typeof input === 'string' ? { text: input } : input;
}

function readPluginIdFromIframeRoute(targetWindow: Window): string {
  const match = /^\/api\/plugins\/([^/]+)(?:\/|$)/.exec(targetWindow.location.pathname || '');
  if (!match) {
    throw new Error('Plugin asset URL helper requires an iframe route under /api/plugins/:pluginId/.');
  }
  try {
    return decodeURIComponent(match[1]);
  } catch {
    throw new Error('Plugin asset URL helper could not decode the current plugin id.');
  }
}

function normalizeAssetPath(input: string): string {
  if (typeof input !== 'string' || input.length === 0) {
    throw new Error('Invalid plugin asset path.');
  }
  if (input.includes('\\') || input.includes('\0') || /^[a-z][a-z0-9+.-]*:/i.test(input)) {
    throw new Error('Invalid plugin asset path.');
  }
  const stripped = input.replace(/^\/+/, '');
  if (!stripped || stripped.startsWith('./')) {
    throw new Error('Invalid plugin asset path.');
  }
  const segments = stripped.split('/');
  if (segments.some((segment) => !segment || segment === '.' || segment === '..' || segment.startsWith('.'))) {
    throw new Error('Invalid plugin asset path.');
  }
  return segments.map((segment) => encodeURIComponent(segment)).join('/');
}

function pluginAssetUrl(targetWindow: Window, input: string): string {
  const pluginId = readPluginIdFromIframeRoute(targetWindow);
  const assetPath = normalizeAssetPath(input);
  return `${targetWindow.location.origin}/api/plugins/${encodeURIComponent(pluginId)}/assets/${assetPath}`;
}

function readSurfaceSession(targetWindow: Window): string | null {
  return new URLSearchParams(targetWindow.location.search).get(PLUGIN_SURFACE_SESSION_QUERY) || null;
}

function normalizePluginApiPath(input: string): string {
  if (typeof input !== 'string' || input.length === 0) {
    throw new Error('Invalid plugin API path.');
  }
  const trimmed = input.trim();
  if (
    !trimmed
    || trimmed.includes('\\')
    || trimmed.includes('\0')
    || trimmed.includes('#')
    || trimmed.startsWith('//')
    || /^[a-z][a-z0-9+.-]*:/i.test(trimmed)
  ) {
    throw new Error('Invalid plugin API path.');
  }

  const stripped = trimmed.replace(/^\/+/, '');
  if (!stripped || stripped.startsWith('./') || stripped === 'api/plugins' || stripped.startsWith('api/plugins/')) {
    throw new Error('Invalid plugin API path. Use a route path relative to the current plugin.');
  }

  const queryIndex = stripped.indexOf('?');
  const rawPath = queryIndex >= 0 ? stripped.slice(0, queryIndex) : stripped;
  if (!rawPath) {
    throw new Error('Invalid plugin API path.');
  }
  const segments = rawPath.split('/');
  for (const segment of segments) {
    if (!segment) throw new Error('Invalid plugin API path.');
    let decoded: string;
    try {
      decoded = decodeURIComponent(segment);
    } catch {
      throw new Error('Invalid plugin API path.');
    }
    if (decoded === '.' || decoded === '..' || decoded.includes('/') || decoded.includes('\\')) {
      throw new Error('Invalid plugin API path.');
    }
  }

  const parsed = new URL(`http://hana.local/${stripped}`);
  const safePath = segments.map(segment => encodeURIComponent(decodeURIComponent(segment))).join('/');
  return `${safePath}${parsed.search}`;
}

function pluginApiUrl(targetWindow: Window, input: string): string {
  const pluginId = readPluginIdFromIframeRoute(targetWindow);
  const apiPath = normalizePluginApiPath(input);
  return `${targetWindow.location.origin}/api/plugins/${encodeURIComponent(pluginId)}/${apiPath}`;
}

function pluginApiFetch(targetWindow: Window, input: string, init?: RequestInit): Promise<Response> {
  const surfaceSession = readSurfaceSession(targetWindow);
  if (!surfaceSession) {
    throw new Error('hana.api.fetch requires pluginSurfaceSession in the iframe URL.');
  }
  const fetchImpl = targetWindow.fetch?.bind(targetWindow) ?? globalThis.fetch?.bind(globalThis);
  if (!fetchImpl) {
    throw new Error('hana.api.fetch requires window.fetch.');
  }
  const requestInit = init ?? {};
  const headers = new Headers(requestInit.headers);
  headers.set(PLUGIN_SURFACE_SESSION_HEADER, surfaceSession);
  return fetchImpl(pluginApiUrl(targetWindow, input), {
    ...requestInit,
    headers,
  });
}

export function createLingxiPluginSdk(options: LingxiPluginSdkOptions = {}): LingxiPluginSdk {
  const targetWindow = options.targetWindow ?? getBrowserWindow();
  const parentWindow = options.parentWindow ?? targetWindow.parent;
  const targetOrigin = resolveTargetOrigin(targetWindow, options.targetOrigin);
  const requestTimeoutMs = options.requestTimeoutMs ?? 10_000;
  const idFactory = options.idFactory ?? defaultIdFactory;
  let themeSnapshot = readInitialTheme(targetWindow);
  const themeSubscribers = new Set<(theme: LingxiPluginThemeSnapshot) => void>();

  function post(message: PluginUiMessage): void {
    parentWindow.postMessage(message, targetOrigin);
  }

  function postEvent(type: string, payload?: unknown): void {
    const message: PluginUiMessage = {
      protocol: PLUGIN_UI_PROTOCOL,
      version: PLUGIN_UI_PROTOCOL_VERSION,
      kind: 'event',
      type,
    };
    if (payload !== undefined) message.payload = payload;
    post(message);
  }

  function onThemeMessage(event: MessageEvent): void {
    if (!isTrustedHostEvent(event, parentWindow, targetOrigin)) return;
    const parsed = parsePluginUiMessage(event.data);
    if (!parsed.ok) return;

    const message = parsed.value;
    if (message.kind !== 'event' || message.type !== 'hana.theme.changed') return;
    if (typeof message.payload !== 'object' || message.payload === null) return;

    const payload = message.payload as Record<string, unknown>;
    themeSnapshot = {
      theme: typeof payload.theme === 'string' ? payload.theme : themeSnapshot.theme,
      cssUrl: typeof payload.cssUrl === 'string' ? payload.cssUrl : themeSnapshot.cssUrl,
    };
    for (const callback of themeSubscribers) callback(themeSnapshot);
  }

  function request<T = unknown>(
    type: string,
    payload?: unknown,
    requestOptions: LingxiPluginRequestOptions = {},
  ): Promise<T> {
    const id = idFactory();
    const timeoutMs = requestOptions.timeoutMs ?? requestTimeoutMs;

    return new Promise<T>((resolve, reject) => {
      const cleanup = () => {
        targetWindow.removeEventListener('message', onMessage);
        targetWindow.clearTimeout(timeout);
      };

      const onMessage = (event: MessageEvent) => {
        if (!isTrustedHostEvent(event, parentWindow, targetOrigin)) return;
        const parsed = parsePluginUiMessage(event.data);
        if (!parsed.ok) return;

        const message = parsed.value;
        if (message.id !== id || message.type !== type) return;

        if (message.kind === 'response') {
          cleanup();
          resolve(message.payload as T);
        }
        if (message.kind === 'error' && message.error) {
          cleanup();
          reject(new LingxiPluginError(message.error));
        }
      };

      const timeout = targetWindow.setTimeout(() => {
        cleanup();
        reject(new LingxiPluginError({
          code: 'TIMEOUT',
          message: `Plugin host request timed out: ${type}.`,
        }));
      }, timeoutMs);

      targetWindow.addEventListener('message', onMessage);

      const message: PluginUiMessage = {
        protocol: PLUGIN_UI_PROTOCOL,
        version: PLUGIN_UI_PROTOCOL_VERSION,
        id,
        kind: 'request',
        type,
      };
      if (payload !== undefined) message.payload = payload;
      post(message);
    });
  }

  return {
    ready(payload?: unknown) {
      postEvent('hana.ready', payload);
    },
    assets: {
      url(assetPath: string) {
        return pluginAssetUrl(targetWindow, assetPath);
      },
    },
    api: {
      url(apiPath: string) {
        return pluginApiUrl(targetWindow, apiPath);
      },
      fetch(apiPath: string, init?: RequestInit) {
        return pluginApiFetch(targetWindow, apiPath, init);
      },
    },
    ui: {
      resize(size: LingxiPluginSize) {
        postEvent(PLUGIN_UI_CAPABILITY.UI_RESIZE, size);
      },
    },
    theme: {
      getSnapshot() {
        return { ...themeSnapshot };
      },
      subscribe(callback: (theme: LingxiPluginThemeSnapshot) => void) {
        if (themeSubscribers.size === 0) {
          targetWindow.addEventListener('message', onThemeMessage);
        }
        themeSubscribers.add(callback);
        callback({ ...themeSnapshot });
        return () => {
          themeSubscribers.delete(callback);
          if (themeSubscribers.size === 0) {
            targetWindow.removeEventListener('message', onThemeMessage);
          }
        };
      },
    },
    host: {
      request,
    },
    toast: {
      show(input: LingxiToastShowInput, options?: LingxiPluginRequestOptions) {
        return request<LingxiToastShowResult>(PLUGIN_UI_CAPABILITY.TOAST_SHOW, input, options);
      },
    },
    external: {
      open(input: LingxiExternalOpenInput, options?: LingxiPluginRequestOptions) {
        return request<LingxiExternalOpenResult>(PLUGIN_UI_CAPABILITY.EXTERNAL_OPEN, externalOpenPayload(input), options);
      },
    },
    clipboard: {
      writeText(input: LingxiClipboardWriteTextInput, options?: LingxiPluginRequestOptions) {
        return request<LingxiClipboardWriteTextResult>(
          PLUGIN_UI_CAPABILITY.CLIPBOARD_WRITE_TEXT,
          clipboardWriteTextPayload(input),
          options,
        );
      },
    },
    resources: {
      open(input: PluginResourceOpenInput, options?: LingxiPluginRequestOptions) {
        return request<PluginResourceOpenResult>(PLUGIN_UI_CAPABILITY.RESOURCE_OPEN, input, options);
      },
      pick(input: PluginResourcePickInput = {}, options?: LingxiPluginRequestOptions) {
        return request<PluginResourcePickResult>(PLUGIN_UI_CAPABILITY.RESOURCE_PICK, input, options);
      },
      requestAccess(input: PluginResourceRequestAccessInput, options?: LingxiPluginRequestOptions) {
        return request<PluginResourceRequestAccessResult>(
          PLUGIN_UI_CAPABILITY.RESOURCE_REQUEST_ACCESS,
          input,
          options,
        );
      },
    },
  };
}

let singleton: LingxiPluginSdk | null = null;

function getSingleton(): LingxiPluginSdk {
  singleton ??= createLingxiPluginSdk();
  return singleton;
}

export const hana: LingxiPluginSdk = {
  ready(payload?: unknown) {
    return getSingleton().ready(payload);
  },
  assets: {
    url(assetPath: string) {
      return getSingleton().assets.url(assetPath);
    },
  },
  api: {
    url(apiPath: string) {
      return getSingleton().api.url(apiPath);
    },
    fetch(apiPath: string, init?: RequestInit) {
      return getSingleton().api.fetch(apiPath, init);
    },
  },
  ui: {
    resize(size: LingxiPluginSize) {
      return getSingleton().ui.resize(size);
    },
  },
  theme: {
    getSnapshot() {
      return getSingleton().theme.getSnapshot();
    },
    subscribe(callback: (theme: LingxiPluginThemeSnapshot) => void) {
      return getSingleton().theme.subscribe(callback);
    },
  },
  host: {
    request<T = unknown>(
      type: string,
      payload?: unknown,
      options?: LingxiPluginRequestOptions,
    ) {
      return getSingleton().host.request<T>(type, payload, options);
    },
  },
  toast: {
    show(input: LingxiToastShowInput, options?: LingxiPluginRequestOptions) {
      return getSingleton().toast.show(input, options);
    },
  },
  external: {
    open(input: LingxiExternalOpenInput, options?: LingxiPluginRequestOptions) {
      return getSingleton().external.open(input, options);
    },
  },
  clipboard: {
    writeText(input: LingxiClipboardWriteTextInput, options?: LingxiPluginRequestOptions) {
      return getSingleton().clipboard.writeText(input, options);
    },
  },
  resources: {
    open(input: PluginResourceOpenInput, options?: LingxiPluginRequestOptions) {
      return getSingleton().resources.open(input, options);
    },
    pick(input?: PluginResourcePickInput, options?: LingxiPluginRequestOptions) {
      return getSingleton().resources.pick(input, options);
    },
    requestAccess(input: PluginResourceRequestAccessInput, options?: LingxiPluginRequestOptions) {
      return getSingleton().resources.requestAccess(input, options);
    },
  },
};
