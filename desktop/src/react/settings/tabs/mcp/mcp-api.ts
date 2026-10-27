import { lingxiFetch } from '../../api';
import { useSettingsStore } from '../../store';
import { captureSettingsConnection, ownsSettingsConnection } from '../../connection-owner';
import { resolveServerConnection } from '../../../services/service-client';
import type { McpSettingsAction, McpSettingsRequest, McpSettingsResult, McpSettingsState, McpConnectorBulkResult, McpOAuthStartResponse, McpOAuthPollResponse, McpOAuthCancelResponse, McpOAuthLogoutResponse } from '../../../../../../contracts/generated/ts/lingxi-protocol';
import type { McpBulkResult, McpConnectorInput, McpState, McpToolPermission } from './types';

/** Mirrors the manager's own default, used only until the first state load lands. */
const DEFAULT_DEFER_THRESHOLD = 10;

export const EMPTY_MCP_STATE: McpState = {
  enabled: false,
  deferEnabled: true,
  deferThreshold: DEFAULT_DEFER_THRESHOLD,
  builtinDeferEnabled: false,
  connectors: [],
  agentConfig: { connectors: {} },
};

async function jsonOrError<T>(res: Response): Promise<T> {
  const data = await res.json();
  if (data?.error) throw new Error(data.error);
  return data;
}

function usesRustMcp(): boolean {
  return resolveServerConnection(useSettingsStore.getState())?.serverNodeKind === 'lingxi-service';
}

function object(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error(`MCP ${label}格式无效`);
  return value as Record<string, unknown>;
}

async function rustAction(action: McpSettingsAction, payload: Record<string, unknown>, agentId?: string): Promise<McpSettingsResult> {
  const owner = captureSettingsConnection();
  const request: McpSettingsRequest = { action, agentId: agentId || null, payload };
  const response = await lingxiFetch('/lingxi/v1/mcp/settings-action', {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(request),
  });
  const data = object(await response.json(), '设置回执');
  if (!ownsSettingsConnection(owner)) throw new Error('连接已改变，旧MCP回执不能用于当前设置');
  const update = object(data.settingsUpdate, '设置回执');
  if (update.action !== action || !['applied', 'failed', 'cancelled'].includes(String(update.status))
    || typeof update.saved !== 'boolean' || typeof update.runtimeApplied !== 'boolean') throw new Error('MCP设置回执不匹配');
  if (update.status !== 'applied' && !(action === 'mcp.connector.add_bulk' && Array.isArray(data.results))) {
    const detail = update.error && typeof update.error === 'object' ? object(update.error, '错误').message : update.summary;
    throw Object.assign(new Error(`${update.saved ? '设置已保存；' : ''}${update.runtimeApplied ? '运行时变更已发生；' : ''}${typeof detail === 'string' ? detail : 'MCP设置未完成'}`), { settingsUpdate: update });
  }
  return data as unknown as McpSettingsResult;
}

async function rustOAuth(path: string, method = 'GET'): Promise<Record<string, unknown>> {
  const owner = captureSettingsConnection();
  const res = await lingxiFetch(`/lingxi/v1/mcp/${path}`, { method });
  const data = object(await res.json(), '登录回执');
  if (!ownsSettingsConnection(owner)) throw new Error('连接已改变，旧MCP登录回执已丢弃');
  if (!res.ok || data.error) throw new Error(typeof data.message === 'string' ? data.message : String(data.error || 'MCP登录请求未完成'));
  return data;
}

export async function loadMcpState(agentId: string | null): Promise<McpState> {
  if (usesRustMcp()) {
    const owner = captureSettingsConnection();
    const response = await lingxiFetch(`/lingxi/v1/mcp/state${agentId ? `?agentId=${encodeURIComponent(agentId)}` : ''}`);
    const wire = object(await response.json(), '状态') as unknown as McpSettingsState;
    if (!ownsSettingsConnection(owner)) throw new Error('连接已改变，旧MCP状态已丢弃');
    const config = object(wire.config, '公开配置');
    const agent = object(wire.agentConfig, '助手权限');
    if (wire.agentId !== agentId || typeof config.enabled !== 'boolean' || typeof config.deferEnabled !== 'boolean'
      || typeof config.builtinDeferEnabled !== 'boolean' || !Number.isSafeInteger(config.deferThreshold)
      || Number(config.deferThreshold) <= 0 || !Array.isArray(config.connectors)) throw new Error('MCP状态与目标助手不匹配');
    object(agent.connectors, '助手连接器权限');
    for (const entry of config.connectors) {
      const connector = object(entry, '连接器');
      if (typeof connector.id !== 'string' || typeof connector.name !== 'string' || typeof connector.transport !== 'string'
        || !['running', 'stopped', 'failed', 'connecting'].includes(String(connector.status)) || !Array.isArray(connector.tools)) throw new Error('MCP连接器真实状态缺失');
    }
    return { enabled: config.enabled, deferEnabled: config.deferEnabled, deferThreshold: Number(config.deferThreshold),
      builtinDeferEnabled: config.builtinDeferEnabled, connectors: config.connectors as McpState['connectors'], agentConfig: agent as McpState['agentConfig'] };
  }

  const res = await lingxiFetch(`/api/mcp/state${agentId ? `?agentId=${encodeURIComponent(agentId)}` : ''}`);
  const data = await jsonOrError<McpState>(res);
  return {
    enabled: data.enabled === true,
    // Only an explicit false turns defer off, matching the server's own
    // read-time default for configs written before defer existed.
    deferEnabled: data.deferEnabled !== false,
    deferThreshold: Number.isSafeInteger(data.deferThreshold) && data.deferThreshold > 0
      ? data.deferThreshold
      : DEFAULT_DEFER_THRESHOLD,
    builtinDeferEnabled: data.builtinDeferEnabled === true,
    connectors: Array.isArray(data.connectors) ? data.connectors : (Array.isArray(data.servers) ? data.servers : []),
    servers: Array.isArray(data.servers) ? data.servers : undefined,
    agentConfig: data.agentConfig || { connectors: {} },
  };
}

export async function setMcpEnabled(enabled: boolean): Promise<void> {
  if (usesRustMcp()) { await rustAction('mcp.global.enabled', { enabled }); return; }
  const res = await lingxiFetch('/api/mcp/settings/enabled', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ enabled }),
  });
  const data = await jsonOrError<McpState>(res);
  if (typeof data?.enabled !== 'boolean') {
    throw new Error('MCP enabled endpoint returned an invalid state');
  }
  if (data.enabled !== enabled) {
    throw new Error(`MCP enabled state did not persist: expected ${enabled}, got ${data.enabled}`);
  }
}

export async function addMcpConnector(input: McpConnectorInput): Promise<void> {
  if (usesRustMcp()) { await rustAction('mcp.connector.add', { connector: input, startIfEnabled: true }); return; }
  const res = await lingxiFetch('/api/mcp/connectors', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
  await jsonOrError(res);
}

/**
 * Import several connectors in one request.
 *
 * The server validates the whole batch before writing any of it. On rejection
 * it still answers with one result per submitted row, so the preview list can
 * mark exactly which entry was refused; that array is returned rather than
 * thrown away with the error.
 */
export async function addMcpConnectorsBulk(inputs: McpConnectorInput[]): Promise<McpBulkResult[]> {
  if (usesRustMcp()) {
    const result = await rustAction('mcp.connector.add_bulk', { connectors: inputs }) as unknown as McpConnectorBulkResult;
    if (!Array.isArray(result.results) || result.results.length !== inputs.length || result.results.some(row => typeof row.ok !== 'boolean' || typeof row.saved !== 'boolean' || typeof row.runtimeApplied !== 'boolean')) throw new Error('MCP批次回执缺失');
    return result.results.map(row => ({ ok: row.ok, ...(row.id ? { id: row.id } : {}), ...(row.error ? { error: row.error } : {}),
      saved: row.saved, runtimeApplied: row.runtimeApplied }));
  }
  const res = await lingxiFetch('/api/mcp/connectors/bulk', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ connectors: inputs }),
  });
  const data = await res.json();
  if (data?.error) {
    const error = new Error(data.error) as Error & { results?: McpBulkResult[] };
    if (Array.isArray(data.results)) error.results = data.results;
    throw error;
  }
  return Array.isArray(data?.results) ? data.results : [];
}

export async function setMcpDeferSettings(
  patch: { deferEnabled?: boolean; deferThreshold?: number; builtinDeferEnabled?: boolean },
): Promise<void> {
  if (usesRustMcp()) { await rustAction('mcp.global.defer', patch); return; }
  const res = await lingxiFetch('/api/mcp/settings/defer', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(patch),
  });
  await jsonOrError(res);
}

export async function updateMcpConnector(connectorId: string, input: McpConnectorInput): Promise<void> {
  if (usesRustMcp()) { await rustAction('mcp.connector.update', { connectorId, patch: input }); return; }
  const res = await lingxiFetch(`/api/mcp/connectors/${encodeURIComponent(connectorId)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(input),
  });
  await jsonOrError(res);
}

export async function removeMcpConnector(connectorId: string): Promise<void> {
  if (usesRustMcp()) { await rustAction('mcp.connector.remove', { connectorId }); return; }
  const res = await lingxiFetch(`/api/mcp/connectors/${encodeURIComponent(connectorId)}`, {
    method: 'DELETE',
  });
  await jsonOrError(res);
}

export async function runMcpConnectorAction(
  connectorId: string,
  action: 'start' | 'stop' | 'refresh-tools',
): Promise<void> {
  if (usesRustMcp()) { await rustAction(action === 'refresh-tools' ? 'mcp.connector.refresh_tools' : action === 'start' ? 'mcp.connector.start' : 'mcp.connector.stop', { connectorId }); return; }
  const res = await lingxiFetch(`/api/mcp/connectors/${encodeURIComponent(connectorId)}/${action}`, {
    method: 'POST',
  });
  await jsonOrError(res);
}

export async function setAgentMcpConnector(
  agentId: string,
  connectorId: string,
  enabled: boolean,
): Promise<void> {
  if (usesRustMcp()) { await rustAction('mcp.agent.connector.enable', { connectorId, enabled }, agentId); return; }
  const res = await lingxiFetch(`/api/mcp/agents/${encodeURIComponent(agentId)}/connectors/${encodeURIComponent(connectorId)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ enabled }),
  });
  await jsonOrError(res);
}

export async function setAgentMcpTool(
  agentId: string,
  connectorId: string,
  toolName: string,
  enabled: boolean,
): Promise<void> {
  await setAgentMcpTools(agentId, connectorId, { [toolName]: enabled });
}

/**
 * Flip several tools for one agent in a single request.
 *
 * The batch selection controls route through here: forty tools used to mean
 * forty round trips, each racing the others' state reload.
 */
export async function setAgentMcpTools(
  agentId: string,
  connectorId: string,
  tools: Record<string, boolean>,
): Promise<void> {
  if (usesRustMcp()) { await rustAction('mcp.agent.tool.enable', { connectorId, tools }, agentId); return; }
  const res = await lingxiFetch(`/api/mcp/agents/${encodeURIComponent(agentId)}/connectors/${encodeURIComponent(connectorId)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ tools }),
  });
  await jsonOrError(res);
}

/**
 * Write connector-level policy fields.
 *
 * These ride the ordinary connector update endpoint. They are deliberately sent
 * on their own: an update carrying only policy leaves the connection fingerprint
 * untouched, so changing a permission never restarts a live connector.
 */
export async function updateMcpConnectorPolicy(
  connectorId: string,
  patch: {
    permissionMode?: 'review-all' | 'allowlist';
    toolPermissions?: Record<string, McpToolPermission>;
    trustReadOnlyHint?: boolean;
    pinnedTools?: Record<string, boolean>;
  },
): Promise<void> {
  if (usesRustMcp()) { await rustAction('mcp.connector.update', { connectorId, patch }); return; }
  const res = await lingxiFetch(`/api/mcp/connectors/${encodeURIComponent(connectorId)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(patch),
  });
  await jsonOrError(res);
}

export async function startMcpOAuth(connectorId: string): Promise<{ sessionId: string; url: string }> {
  if (usesRustMcp()) {
    const data = await rustOAuth(`connectors/${encodeURIComponent(connectorId)}/oauth/start`, 'POST');
    if (typeof data.sessionId !== 'string' || !data.sessionId || typeof data.url !== 'string') throw new Error('MCP登录回执格式无效');
    const url = new URL(data.url);
    if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) throw new Error('MCP登录地址无效');
    return data as unknown as McpOAuthStartResponse;
  }
  const res = await lingxiFetch(`/api/mcp/connectors/${encodeURIComponent(connectorId)}/oauth/start`, {
    method: 'POST',
  });
  return jsonOrError<{ sessionId: string; url: string }>(res);
}

export async function pollMcpOAuth(sessionId: string): Promise<McpOAuthPollResponse> {
  if (usesRustMcp()) {
    const data = await rustOAuth(`oauth/poll/${encodeURIComponent(sessionId)}`);
    if (!['pending', 'exchanging', 'done', 'error', 'cancelled', 'missing'].includes(String(data.status))
      || (data.error !== undefined && typeof data.error !== 'string')) throw new Error('MCP登录状态格式无效');
    return data as unknown as McpOAuthPollResponse;
  }
  const res = await lingxiFetch(`/api/mcp/oauth/poll/${encodeURIComponent(sessionId)}`);
  return jsonOrError<{ status: string; error?: string }>(res);
}

/** Abandon a browser round trip the user gave up on. Saved credentials are untouched. */
export async function cancelMcpOAuth(connectorId: string): Promise<void> {
  if (usesRustMcp()) {
    const data = await rustOAuth(`connectors/${encodeURIComponent(connectorId)}/oauth/cancel`, 'POST') as unknown as McpOAuthCancelResponse;
    if (!Number.isSafeInteger(data.cancelled) || data.cancelled < 0) throw new Error('MCP取消回执格式无效');
    return;
  }
  const res = await lingxiFetch(`/api/mcp/connectors/${encodeURIComponent(connectorId)}/oauth/cancel`, {
    method: 'POST',
  });
  await jsonOrError(res);
}

export async function logoutMcpOAuth(connectorId: string): Promise<void> {
  if (usesRustMcp()) {
    const data = await rustOAuth(`connectors/${encodeURIComponent(connectorId)}/oauth/logout`, 'POST') as unknown as McpOAuthLogoutResponse;
    object(data.connector, '退出连接器');
    const view = object(data.state, '退出后状态');
    object(view.config, '退出后配置');
    return;
  }
  const res = await lingxiFetch(`/api/mcp/connectors/${encodeURIComponent(connectorId)}/oauth/logout`, {
    method: 'POST',
  });
  await jsonOrError(res);
}
