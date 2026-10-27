/** @vitest-environment jsdom */
import { createServer } from 'node:http';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { handleAppEvent } from '../../services/app-event-actions';
import { startRustAutomationNotifications } from '../../services/rust-automation';
import type { ServerConnection } from '../../services/server-connection';

function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { promise, resolve }; }
afterEach(() => { vi.useRealTimers(); });

describe('Rust app-events HTTP → 正式消费者 → 真实频道与协作 store', () => {
  it('consumes all five conversation events, refreshes authoritative history, and deduplicates the real cursor', async () => {
    window.t = key => key;
    const paths: string[] = [];
    const first = deferred<void>();
    let reads = 0;
    const message = { sender: 'hana', timestamp: '2026-10-16T00:00:00Z', body: '正式频道消息' };
    const dmMessage = { sender: 'peer', timestamp: '2026-10-16T00:00:01Z', body: '正式私聊消息' };
    const activity = { conversationId: 'c', agentId: 'hana', timestamp: '2026-10-16T00:00:02Z', state: 'replied', summary: '已公开回复' };
    const details = { suggestionId: 'suggestion', kind: 'session_send_draft', target: { type: 'session', sessionId: 'target' }, draft: { message: '原文' }, frozenApply: { message: '确认文字' }, status: 'approved', decisionPersisted: true, result: { deliveryStatus: 'accepted', firstMessageAccepted: true, acceptedRunId: 'accepted-run' } };
    const events = [
      ['channel_created', { channelName: 'new-channel' }],
      ['channel_new_message', { channelName: 'c', message }],
      ['dm_new_message', { from: 'peer', to: 'hana' }],
      ['conversation_agent_activity', { activity }],
      ['session_collab_decision', { sessionId: 'source', sourceSessionId: 'source', suggestionId: 'suggestion', decision: { status: 'approved' }, result: details.result }],
    ].map(([type, payload], index) => ({ seq: index + 1, type, payload, source: 'server', atUnixMs: 1_800_000_000_000 + index }));
    const channels = [{ id: 'c', name: '已知频道', members: ['hana'], messageCount: 1, newMessageCount: 1 }, { id: 'new-channel', name: '新频道', members: ['hana'], messageCount: 0, newMessageCount: 0 }];
    const server = createServer((request, response) => {
      paths.push(request.url || '');
      response.setHeader('Content-Type', 'application/json');
      if (request.headers.authorization !== 'Bearer event-token') { response.statusCode = 401; response.end(JSON.stringify({ error: 'authorization required' })); return; }
      const path = new URL(request.url!, 'http://127.0.0.1').pathname;
      let value: unknown;
      if (path === '/lingxi/v1/app-events') value = ++reads === 1 ? { stale: false, latestSequence: 0, events: [] } : { stale: false, latestSequence: 5, events };
      else if (path === '/lingxi/v1/browser/workspaces') value = { workspaces: [] };
      else if (path === '/lingxi/v1/desk/activities') value = { activities: [] };
      else if (path === '/lingxi/v1/channels') value = { channels };
      else if (path === '/lingxi/v1/dm') value = { ownerAgentId: 'hana', dms: [{ peerId: 'peer', peerName: '同伴', ownerAgentId: 'hana' }] };
      else if (path === '/lingxi/v1/dm/peer') value = { peerId: 'peer', peerName: '同伴', ownerAgentId: 'hana', messages: [dmMessage] };
      else if (path.endsWith('/agent-activities')) value = { activities: [] };
      else if (path.endsWith('/agent-phone-settings')) value = { mode: 'read_only', proactiveEnabled: true };
      else if (path === '/lingxi/v1/sessions/source/media/jobs') value = { jobs: [] };
      else if (path === '/lingxi/v1/sessions/source/history') {
        response.setHeader('ETag', 'history-v1');
        value = { schemaVersion: 'lingxi.canonical-history.v1', sessionId: 'source', revision: 'history-v1', page: { hasMore: false, nextBefore: null }, items: [
          { kind: 'user_message', messageId: 'user', seq: '1', committedAtUnixMs: 1, text: '确认协作', run: { state: 'known', runId: 'source-run' } },
          { kind: 'tool_call', run: { state: 'known', runId: 'source-run' }, toolCallId: 'call', target: 'tool:first-party:session', status: 'success', firstSeq: '2', result: { status: 'success', details, content: [], resourceRefs: [], truncated: false } },
        ] };
      } else { response.statusCode = 404; value = { error: `unexpected test endpoint: ${path}` }; }
      response.end(JSON.stringify(value));
    });
    await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
    const address = server.address(); if (!address || typeof address === 'string') throw new Error('HTTP fixture unavailable');
    const source: ServerConnection = { connectionId: 'test-remote', kind: 'custom_remote', serverId: 'rust', serverNodeKind: 'lingxi-service', studioId: 'studio', label: '合成服务', baseUrl: `http://127.0.0.1:${address.port}`, wsUrl: `ws://127.0.0.1:${address.port}`, token: 'event-token', authState: 'paired', trustState: 'tunnel', credentialKind: 'device_credential', capabilities: [] };
    useStore.setState({ serverConnections: { [source.connectionId]: source }, activeServerConnectionId: source.connectionId, activeServerConnection: source, currentAgentId: 'hana', agents: [{ id: 'hana', name: 'Hana', isPrimary: true }], channelsEnabled: true, channels: [{ ...channels[0], isDM: false }, { id: 'dm:peer', name: '同伴', members: ['peer'], peerId: 'peer', dmOwnerId: 'hana', isDM: true }], currentTab: 'channels', currentChannel: 'dm:peer', channelMessages: [], channelMessageCache: {}, channelMessageCacheDirty: {}, channelAgentActivities: {}, currentSessionPath: 'native:source', currentSessionId: 'source', sessions: [], chatSessions: {}, _loadMessagesVersion: {} } as never);
    const delivered: string[] = [];
    const errors: string[] = [];
    vi.useFakeTimers();
    const stop = startRustAutomationNotifications(source, { replaceActivities: () => first.resolve(), reportError: error => errors.push(error), deliverEvent: (type, payload, origin, isCurrent) => { delivered.push(type); return handleAppEvent(type, payload, { source: origin, isCurrent }); } });
    try {
      await first.promise;
      await vi.advanceTimersByTimeAsync(5_000);
      await vi.waitFor(() => {
        const state = useStore.getState();
        expect(state.channels.some(channel => channel.id === 'new-channel')).toBe(true);
        expect(state.channelMessageCache.c).toEqual([message]);
        expect(state.channelMessages).toEqual([dmMessage]);
        expect(state.channelAgentActivities.c.hana).toEqual([activity]);
        const cards = Object.values(state.chatSessions).flatMap(session => session.items.flatMap(item => item.type === 'message' ? item.data.blocks || [] : []));
        expect(cards).toEqual(expect.arrayContaining([expect.objectContaining({ type: 'suggestion_card', status: 'approved', result: expect.objectContaining({ deliveryStatus: 'accepted', acceptedRunId: 'accepted-run' }) })]));
      });
      await vi.advanceTimersByTimeAsync(0);
      await vi.advanceTimersByTimeAsync(5_000);
      await vi.waitFor(() => expect(paths).toContain('/lingxi/v1/app-events?since=5'));
      await vi.advanceTimersByTimeAsync(0);
      expect(delivered).toEqual(events.map(event => event.type));
      expect(paths.filter(path => path === '/lingxi/v1/sessions/source/history')).toHaveLength(1);
      // 历史刷新后沿同一会话读取正式媒体任务回执（loadMessages → refreshRustMediaJobs）。
      expect(paths.filter(path => path === '/lingxi/v1/sessions/source/media/jobs')).toHaveLength(1);
      expect(paths.some(path => path.includes('/notifications'))).toBe(false);
      expect(errors).toEqual([]);
    } finally { stop(); vi.useRealTimers(); server.closeAllConnections(); await new Promise<void>(resolve => server.close(() => resolve())); }
  }, 10_000);
});
