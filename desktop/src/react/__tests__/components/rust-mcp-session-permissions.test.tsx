// @vitest-environment jsdom
import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import RustSessionReviewPanel from '../../components/session/RustSessionReviewPanel';
import { useStore } from '../../stores';
import { createLocalServerConnection } from '../../services/server-connection';
import { captureSessionConnection } from '../../services/session-connection-owner';

const target = 'tool:mcp:fixture:alpha';
let capability: unknown = 'mcp.alpha.invoke';
let grantReply: (() => Promise<Response>) | null = null;
let answerFails = false;
const requests: { path: string; body: unknown }[] = [];
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status, headers: { 'content-type': 'application/json' } });
function connect(port = 3210) {
  const connection = createLocalServerConnection({ serverPort: port, serverToken: `synthetic-${port}`, serverNodeKind: 'lingxi-service' })!;
  useStore.setState({ activeServerConnection: connection, activeServerConnectionId: connection.connectionId, serverConnections: { [connection.connectionId]: connection } });
  captureSessionConnection();
  useStore.setState({ currentSessionId: 'session-a', currentAgentId: 'a' });
}
function grantRequests() { return requests.filter(row => row.path.endsWith('/mcp/session-permissions')); }
function answerRequests() { return requests.filter(row => row.path.endsWith('/approvals/approval-a')); }

beforeEach(() => {
  capability = 'mcp.alpha.invoke'; grantReply = null; answerFails = false; requests.length = 0;
  connect();
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = new URL(String(input)).pathname;
    const body = init?.body ? JSON.parse(String(init.body)) : null;
    requests.push({ path, body });
    if (path.endsWith('/mcp/session-permissions')) return grantReply ? grantReply() : json({ ok: true, ...body, persisted: false });
    if (path.endsWith('/approvals/approval-a')) return answerFails ? json({ error: '真实确认失败' }, 409) : json({ ok: true, decision: 'confirmed' });
    if (path.endsWith('/approvals')) return json({ approvals: [{ approvalId: 'approval-a', runId: 'run-a', target, capability, summary: '调用自有MCP工具' }] });
    if (path.endsWith('/deferred-rewind')) return json({ deferredRewind: null });
    throw new Error(`未预期的请求 ${path}`);
  }));
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

describe('Rust当前会话MCP能力授权', () => {
  it('实际按钮提交真实目录能力，再确认当前待批动作；不发送旧Node地址', async () => {
    render(<RustSessionReviewPanel />);
    fireEvent.click(await screen.findByTestId('rust-mcp-approve-session'));
    await waitFor(() => expect(answerRequests()).toHaveLength(1));
    expect(grantRequests()[0].body).toEqual({ sessionId: 'session-a', targetId: target, capability: 'mcp.alpha.invoke' });
    expect(requests.indexOf(grantRequests()[0])).toBeLessThan(requests.indexOf(answerRequests()[0]));
    expect(requests.every(row => row.path.startsWith('/lingxi/v1/'))).toBe(true);
  });
  it('缺少服务器真实能力时不猜测授权值，也不显示记住选项', async () => {
    capability = null;
    render(<RustSessionReviewPanel />);
    await screen.findByText('调用自有MCP工具');
    expect(screen.queryByTestId('rust-mcp-approve-session')).toBeNull();
    expect(grantRequests()).toHaveLength(0);
  });
  it('授权被拒绝时保留待批动作，不发确认执行', async () => {
    grantReply = async () => json({ error: '缺少 sessions.write' }, 403);
    render(<RustSessionReviewPanel />);
    fireEvent.click(await screen.findByTestId('rust-mcp-approve-session'));
    await screen.findByRole('alert');
    expect(answerRequests()).toHaveLength(0);
    expect(screen.getByTestId('rust-mcp-approve-session')).toBeTruthy();
  });
  it('回执目标不匹配时明确失败，不把授权算成功', async () => {
    grantReply = async () => json({ ok: true, sessionId: 'session-b', targetId: target, capability: 'mcp.alpha.invoke', persisted: false });
    render(<RustSessionReviewPanel />);
    fireEvent.click(await screen.findByTestId('rust-mcp-approve-session'));
    expect((await screen.findByRole('alert')).textContent).toContain('回执不匹配');
    expect(answerRequests()).toHaveLength(0);
  });
  it('授权已保存但本次确认失败时如实说明部分结果', async () => {
    answerFails = true;
    render(<RustSessionReviewPanel />);
    fireEvent.click(await screen.findByTestId('rust-mcp-approve-session'));
    expect((await screen.findByRole('alert')).textContent).toContain('本会话授权已生效');
    expect(grantRequests()).toHaveLength(1); expect(answerRequests()).toHaveLength(1);
  });
  it('授权等待中连接切走再返回也不能批准旧动作', async () => {
    let resolve!: (response: Response) => void;
    grantReply = () => new Promise<Response>(done => { resolve = done; });
    render(<RustSessionReviewPanel />);
    fireEvent.click(await screen.findByTestId('rust-mcp-approve-session'));
    await waitFor(() => expect(grantRequests()).toHaveLength(1));
    connect(3211); connect(3210);
    resolve(json({ ok: true, sessionId: 'session-a', targetId: target, capability: 'mcp.alpha.invoke', persisted: false }));
    await waitFor(() => expect(screen.getByTestId('rust-session-review')).toBeTruthy());
    expect(answerRequests()).toHaveLength(0);
  });
});
