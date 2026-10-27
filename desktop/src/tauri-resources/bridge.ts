export type SkillPayload = { name: string; baseDir: string; filePath?: string | null; installed: boolean };
export type SkillTree = { name: string; path: string; isDir: boolean; children: SkillTree[] };
export type FileVersion = { mtimeMs: number; size: number; sha256: string };
export type Snapshot = { content: string; version: FileVersion };
export type SaveResult = { ok: boolean; conflict: boolean; version: FileVersion | null };
export type ResourceEvents = { events?: Array<Record<string, unknown>>; latestSequence?: number; stale?: boolean };
export type ResourceBridge = ReturnType<typeof createResourceBridge>;
export type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
export type FileChangedListener = (callback: (path: string) => void) => Promise<() => void>;

export function createResourceBridge(invoke: Invoke, listenForFileChanges: FileChangedListener) {
  return {
    openSkillViewer: (data: SkillPayload) => invoke<string>('open_skill_viewer', { data }),
    skillViewerLoad: () => invoke<SkillPayload | null>('skill_viewer_load'),
    listSkillFiles: (baseDir: string) => invoke<SkillTree[]>('list_skill_files', { baseDir }),
    readSkillFile: (path: string) => invoke<string | null>('read_skill_file', { path }),
    skillViewerClose: () => invoke<void>('skill_viewer_close'),
    selectFolder: () => invoke<string | null>('select_folder'),
    selectFiles: (multiple = true) => invoke<string[]>('select_files', { multiple }),
    readFile: (path: string) => invoke<string | null>('read_file', { path }),
    readFileSnapshot: (path: string) => invoke<Snapshot | null>('read_file_snapshot', { path }),
    readFileBase64: (path: string) => invoke<string | null>('read_file_base64', { path }),
    fileInfo: (path: string) => invoke<{ path: string; size: number; isFile: boolean } | null>('file_info', { path }),
    writeFile: (path: string, content: string) => invoke<boolean>('write_file', { path, content }),
    writeFileBinary: (path: string, data: string) => invoke<boolean>('write_file_binary', { path, data }),
    copyFile: (source: string, destination: string) => invoke<boolean>('copy_file', { source, destination }),
    writeFileIfUnchanged: (path: string, content: string, expectedVersion: FileVersion | null) => invoke<SaveResult>('write_file_if_unchanged', { path, content, expectedVersion }),
    openFile: (path: string) => invoke<boolean>('open_file', { path }),
    trashItem: (path: string) => invoke<boolean>('trash_item', { path }),
    runEditCommand: (command: string) => invoke<boolean>('run_edit_command', { command }),
    // 兼容注册口供宿主消费者使用；当前 viewer 的刷新走 ResourceIO，不用旧监听启停按钮。
    watchFile: (path: string) => invoke<boolean>('watch_file', { path }),
    unwatchFile: (path: string) => invoke<boolean>('unwatch_file', { path }),
    listenFileChanged: (callback: (path: string) => void) => listenForFileChanges(callback),
    resourceSubscribe: (path: string) => invoke<{ subscriptionId: string; resourceKeys: string[] }>('resource_subscribe', { path }),
    resourceEvents: (since: number) => invoke<ResourceEvents>('resource_events', { since }),
    resourceUnsubscribe: (subscriptionId: string) => invoke<unknown>('resource_unsubscribe', { subscriptionId }),
    spawnViewer: (path: string) => invoke<string>('spawn_viewer', { path }),
    viewerRequestLoad: () => invoke<{ filePath: string; title: string; readOnly: true } | null>('viewer_request_load'),
    viewerClose: () => invoke<void>('viewer_close'),
  };
}

export function nativeBridge(): ResourceBridge {
  const native = (window as unknown as {
    __TAURI__?: {
      core?: { invoke: Invoke };
      event?: { listen: (event: string, handler: (event: { payload: { filePath?: string } }) => void) => Promise<() => void> };
    };
  }).__TAURI__;
  if (!native?.core?.invoke) throw new Error('此文件界面需要当前原生桌面宿主');
  const core = native.core;
  return createResourceBridge(
    async <T,>(command: string, args?: Record<string, unknown>) => {
      const result = await core.invoke<T>(command, args);
      // 供窗口内资源消费者观察真实回包；不包含宿主凭证，也不替换原生执行。
      if (command === 'select_files' || command === 'select_folder' || command === 'resource_events') window.dispatchEvent(new CustomEvent('lingxi-resource-response', { detail: { command, result } }));
      return result;
    },
    async callback => {
      if (!native.event?.listen) throw new Error('原生文件事件监听不可用');
      return native.event.listen('file-changed', event => {
        if (typeof event.payload.filePath === 'string') callback(event.payload.filePath);
      });
    },
  );
}
