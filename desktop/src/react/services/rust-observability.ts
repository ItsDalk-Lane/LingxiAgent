/**
 * R07-T10：模型观测面在原生服务上的路径映射。R07 收口后观测面
 * settings/storage/maintenance/payloads/blobs 已由 lingxi-service 实现；
 * 全段映射到原生路径，不存在的面由服务端响亮 404/409，不在壳层静默降级。
 */
const API_OBSERVABILITY_PREFIX = '/api/model-observability';
const OBSERVABILITY_MAPPED_PREFIX = '/lingxi/v1/observability';
const OBSERVABILITY_STATIC_SEGMENTS = new Set([
  '/health', '/settings', '/storage', '/maintenance/delete', '/export',
  '/query/calls', '/query/aggregate', '/query/traces',
]);
const OBSERVABILITY_ID_PREFIXES = [
  '/calls/', '/traces/', '/payloads/', '/blobs/',
];

export function rustObservabilityRoute(connection: { serverNodeKind?: string }, path: string): string {
  if (connection.serverNodeKind !== 'lingxi-service') return path;
  const q = path.indexOf('?');
  const base = q === -1 ? path : path.slice(0, q);
  if (!base.startsWith(API_OBSERVABILITY_PREFIX)) return path;
  const suffix = base.slice(API_OBSERVABILITY_PREFIX.length);
  const mapped =
    OBSERVABILITY_STATIC_SEGMENTS.has(suffix)
    || OBSERVABILITY_ID_PREFIXES.some((prefix) =>
      suffix.startsWith(prefix) && suffix.length > prefix.length && !suffix.slice(prefix.length).includes('/'));
  if (!mapped) return path;
  return OBSERVABILITY_MAPPED_PREFIX + suffix + (q === -1 ? '' : path.slice(q));
}
