import { useEffect, useMemo, useRef } from 'react';
import { useStore } from '../../stores';
import { resourceWatchKey, retainResourceWatch, type ResourceRef } from '../../services/resource-events';

function normalizeSubdir(value: string): string {
  return value.replace(/\\/g, '/').replace(/^\/+|\/+$/g, '');
}

function joinWorkspaceRoot(basePath: string, subdir: string): string {
  const normalizedSubdir = normalizeSubdir(subdir);
  if (!normalizedSubdir) return basePath;
  const separator = /^[A-Za-z]:\\/.test(basePath) || (basePath.includes('\\') && !basePath.includes('/')) ? '\\' : '/';
  return `${basePath.replace(/[\\/]+$/, '')}${separator}${normalizedSubdir.replace(/\//g, separator)}`;
}

function workspaceWatchRefs(basePath: string, mountId: string, nativeRoot: string, expandedPaths: string[]): ResourceRef[] {
  const roots = new Map<string, ResourceRef>();
  const add = (ref: ResourceRef) => roots.set(resourceWatchKey(ref), ref);

  if (mountId) {
    // mount 形态工作台：服务端的 mount ref 订阅不可用——「default」mount 由
    // MountAwareFileService 合成、不在挂载注册表里（resource_not_found），已创建挂载的
    // capabilities 只有 list/read/write（capabilityDenied: watch）。因此 local_fs 挂载
    // 对 local owner 披露的 native 根（deskWorkspaceNativeRoot）是唯一可行的 watch 入口：
    // 统一改用 local-file 订阅。这与 markDeskTreeDirtyForResourceChange 的口径一致——
    // mount 工作台本来就按 deskWorkspaceNativeRoot 把事件路径归位成 subdir。
    // 无 native 根（远端/虚拟挂载）当前没有可用的服务端 watch，直接不订阅。
    const root = nativeRoot.trim();
    if (!root) return [];
    add({ kind: 'local-file', path: root });
    for (const subdir of expandedPaths) {
      const normalized = normalizeSubdir(subdir);
      if (normalized) add({ kind: 'local-file', path: joinWorkspaceRoot(root, normalized) });
    }
    return [...roots.values()];
  }

  if (!basePath) return [];
  add({ kind: 'local-file', path: basePath });
  for (const subdir of expandedPaths) {
    const normalized = normalizeSubdir(subdir);
    if (normalized) add({ kind: 'local-file', path: joinWorkspaceRoot(basePath, normalized) });
  }
  return [...roots.values()];
}

export function WorkspaceFileChangeBridge() {
  const deskBasePath = useStore(s => s.deskBasePath);
  const deskWorkspaceMountId = useStore(s => s.deskWorkspaceMountId);
  const deskWorkspaceNativeRoot = useStore(s => s.deskWorkspaceNativeRoot);
  const deskExpandedPaths = useStore(s => s.deskExpandedPaths);
  const subscriptionsRef = useRef<Map<string, () => void>>(new Map());
  const watchedRefs = useMemo(
    () => workspaceWatchRefs(deskBasePath, deskWorkspaceMountId || '', deskWorkspaceNativeRoot || '', deskExpandedPaths),
    [deskBasePath, deskExpandedPaths, deskWorkspaceMountId, deskWorkspaceNativeRoot],
  );
  const watchedRefsKey = watchedRefs.map(resourceWatchKey).join('\n');

  useEffect(() => {
    const nextKeys = new Set(watchedRefs.map(resourceWatchKey));
    for (const [key, unsubscribe] of subscriptionsRef.current) {
      if (nextKeys.has(key)) continue;
      unsubscribe();
      subscriptionsRef.current.delete(key);
    }
    for (const ref of watchedRefs) {
      const key = resourceWatchKey(ref);
      if (subscriptionsRef.current.has(key)) continue;
      subscriptionsRef.current.set(key, retainResourceWatch(ref));
    }
  }, [watchedRefsKey]); // eslint-disable-line react-hooks/exhaustive-deps -- watchedRefsKey is the reconciled subscription identity.

  useEffect(() => () => {
    for (const unsubscribe of subscriptionsRef.current.values()) unsubscribe();
    subscriptionsRef.current.clear();
  }, []);

  return null;
}

export const WorkspaceFileWatchBridge = WorkspaceFileChangeBridge;
