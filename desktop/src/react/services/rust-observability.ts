/**
 * R07-T10：模型观测面在原生服务上的路径映射。仅映射已实现段；
 * 未迁移段（storage/settings/payloads/blobs/maintenance）保持原路径，
 * 由服务端响亮 404，不在壳层静默降级。
 */
export function rustObservabilityRoute(connection: { serverNodeKind?: string }, path: string): string {
  if (connection.serverNodeKind !== 'lingxi-service') return path;
  const q = path.indexOf('?');
  const base = q === -1 ? path : path.slice(0, q);
  const m = /^\/api\/model-observability(\/(?:health|query\/(?:calls|aggregate|traces)|calls\/[^/?]+|traces\/[^/?]+|export))$/.exec(base);
  if (!m) return path;
  return `/lingxi/v1/observability${m[1]}${q === -1 ? '' : path.slice(q)}`;
}
