import { describe, expect, it } from 'vitest';
import { rustSettingsRoute } from '../../services/rust-settings';

describe('现役设置窗口和主窗口的共同地址', () => {
  const native = { serverNodeKind: 'lingxi-service' };
  it.each([
    ['/api/config', '/lingxi/v1/settings/config'],
    ['/api/config?locale=zh', '/lingxi/v1/settings/config?locale=zh'],
    ['/api/config/workspaces/recent?agentId=a', '/lingxi/v1/settings/config/workspaces/recent?agentId=a'],
    ['/api/config/default-workspace', '/lingxi/v1/settings/config/default-workspace'],
    ['/api/agents', '/lingxi/v1/agents'],
    ['/api/providers/p/api-key', '/lingxi/v1/providers/p/api-key'],
    ['/api/models/health', '/lingxi/v1/models/health'],
    ['/api/preferences/models', '/lingxi/v1/preferences/models'],
    ['/api/settings/export', '/lingxi/v1/settings/export'],
    ['/api/cards/a', '/lingxi/v1/cards/a'],
  ])('%s 到同一服务', (input, expected) => {
    expect(rustSettingsRoute(native, input)).toBe(expected);
    expect(rustSettingsRoute({ serverNodeKind: 'legacy' }, input)).toBe(input);
  });
  it('保留未知路径与已有正式地址', () => {
    for (const path of ['/api/configuration', '/api/preferences/models-other', '/api/other', '/lingxi/v1/agents']) {
      expect(rustSettingsRoute(native, path)).toBe(path);
    }
  });
});
