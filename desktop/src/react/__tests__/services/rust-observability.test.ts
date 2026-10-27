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

  it('R07 收口新实现段同样映射到原生路径', () => {
    expect(rustObservabilityRoute(rust, '/api/model-observability/storage')).toBe('/lingxi/v1/observability/storage');
    expect(rustObservabilityRoute(rust, '/api/model-observability/settings')).toBe('/lingxi/v1/observability/settings');
    expect(rustObservabilityRoute(rust, '/api/model-observability/payloads/9')).toBe('/lingxi/v1/observability/payloads/9');
    expect(rustObservabilityRoute(rust, '/api/model-observability/blobs/mb_abc123')).toBe('/lingxi/v1/observability/blobs/mb_abc123');
    expect(rustObservabilityRoute(rust, '/api/model-observability/maintenance/delete')).toBe('/lingxi/v1/observability/maintenance/delete');
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
