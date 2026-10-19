import { describe, expect, it } from 'vitest';
import { rustObservabilityRoute } from '../../services/rust-observability';

const rust = { serverNodeKind: 'lingxi-service' };
const node = { serverNodeKind: 'node' };

describe('R07-T10 观测面 Rust 路由映射', () => {
  it('已实现段映射到 /lingxi/v1/observability', () => {
    expect(rustObservabilityRoute(rust, '/api/model-observability/health')).toBe('/lingxi/v1/observability/health');
    expect(rustObservabilityRoute(rust, '/api/model-observability/query/calls')).toBe('/lingxi/v1/observability/query/calls');
    expect(rustObservabilityRoute(rust, '/api/model-observability/query/aggregate')).toBe('/lingxi/v1/observability/query/aggregate');
    expect(rustObservabilityRoute(rust, '/api/model-observability/query/traces')).toBe('/lingxi/v1/observability/query/traces');
    expect(rustObservabilityRoute(rust, '/api/model-observability/calls/call-1')).toBe('/lingxi/v1/observability/calls/call-1');
    expect(rustObservabilityRoute(rust, '/api/model-observability/traces/t-1')).toBe('/lingxi/v1/observability/traces/t-1');
    expect(rustObservabilityRoute(rust, '/api/model-observability/export')).toBe('/lingxi/v1/observability/export');
  });

  it('查询串跟随路径', () => {
    expect(rustObservabilityRoute(rust, '/api/model-observability/calls/call-1?full=1')).toBe('/lingxi/v1/observability/calls/call-1?full=1');
  });

  it('未迁移段保持原路径（服务端响亮 404，不静默降级）', () => {
    for (const p of [
      '/api/model-observability/storage',
      '/api/model-observability/settings',
      '/api/model-observability/payloads/9',
      '/api/model-observability/blobs/b-1',
      '/api/model-observability/maintenance/delete',
    ]) {
      expect(rustObservabilityRoute(rust, p)).toBe(p);
    }
  });

  it('非原生连接不改动', () => {
    expect(rustObservabilityRoute(node, '/api/model-observability/query/calls')).toBe('/api/model-observability/query/calls');
    expect(rustObservabilityRoute({}, '/api/model-observability/query/calls')).toBe('/api/model-observability/query/calls');
  });

  it('形似但未命中段不映射', () => {
    expect(rustObservabilityRoute(rust, '/api/model-observability/query/calls/extra')).toBe('/api/model-observability/query/calls/extra');
    expect(rustObservabilityRoute(rust, '/api/model-observabilityx/query/calls')).toBe('/api/model-observabilityx/query/calls');
  });
});
