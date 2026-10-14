import { useEffect, useState } from 'react';
import { useStore } from '../../stores';
import {
  appendConnectionAuth, buildConnectionUrl, buildConnectionWsUrl,
  requestConnectionWsTicket, resolveServerConnection, type ServerConnection,
} from '../../services/server-connection';
import { isRustWireServerHello } from '../../services/websocket';
import { version } from '../../../../../package.json';

interface MoodRecord {
  key: string;
  segmentId: string;
  runId: string;
  text: string;
  complete: boolean;
  saved: boolean;
}
interface MoodEvent {
  schemaVersion?: number;
  sessionId?: string;
  streamId?: string;
  runId?: string;
  seq?: string;
  eventType?: string;
  payload?: { type?: string; stage?: string; segmentId?: string; delta?: string };
}

// 和真正发送请求的入口使用同一连接解析，切换服务不能串入别人的轨迹。
function connectionKey(connection: ServerConnection | null): string {
  return connection ? JSON.stringify(connection) : '';
}

export function RustMoodPanel() {
  const scope = useStore(state => {
    const connection = resolveServerConnection(state);
    return connection?.serverNodeKind === 'lingxi-service' && state.currentSessionId
      ? JSON.stringify([state.currentSessionId, connectionKey(connection)]) : '';
  });
  return scope ? <MoodSession key={scope} scope={scope} /> : null;
}

function MoodSession({ scope }: { scope: string }) {
  const [records, setRecords] = useState<MoodRecord[]>([]);
  const [notice, setNotice] = useState('正在读取情绪轨迹');
  useEffect(() => {
    const [sessionId, selectedConnection] = JSON.parse(scope) as [string, string];
    const connection = resolveServerConnection(useStore.getState());
    if (!connection || connectionKey(connection) !== selectedConnection) return;
    let stopped = false;
    let socket: WebSocket | null = null;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let handshakeTimer: ReturnType<typeof setTimeout> | undefined;
    const abort = new AbortController();
    const visible = new Map<string, MoodRecord>();
    const valid = () => !stopped && useStore.getState().currentSessionId === sessionId
      && connectionKey(resolveServerConnection(useStore.getState())) === selectedConnection;
    const publish = () => { if (valid()) setRecords([...visible.values()]); };
    const request = async (path: string) => {
      const response = await fetch(buildConnectionUrl(connection, path), {
        headers: appendConnectionAuth(connection), credentials: 'include', signal: abort.signal,
      });
      if (!response.ok) throw new Error(`读取失败（${response.status}）`);
      return response.json();
    };
    let generation = 0;
    const connect = async () => {
      const thisGeneration = ++generation;
      const currentValid = () => valid() && generation === thisGeneration;
      let releaseSnapshot: () => void = () => {};
      const snapshotReady = new Promise<void>(resolve => { releaseSnapshot = resolve; });
      let failureNotice = '';
      let handshake = false;
      let hydrated = false;
      let cut = 0n;
      let moodStream = '';
      const events = new Map<string, MoodEvent>();
      const aggregate = new Map<string, MoodRecord>();
      const savedKeys = new Set<string>();
      const apply = (event: MoodEvent) => {
        if (event.schemaVersion !== 1 || event.sessionId !== sessionId || event.streamId !== moodStream
          || typeof event.runId !== 'string' || typeof event.seq !== 'string' || !/^\d+$/.test(event.seq)) return;
        const payload = event.payload;
        if (!payload || typeof payload.segmentId !== 'string' || payload.type !== 'assistant_mood' || event.eventType !== 'assistant_mood') return;
        const key = `${event.runId}:${payload.segmentId}`;
        const live = BigInt(event.seq) > cut;
        if (payload.stage === 'start') {
          aggregate.set(key, { key, segmentId: payload.segmentId, runId: event.runId, text: '', complete: false, saved: false });
        } else {
          const record = aggregate.get(key);
          if (!record) return;
          if (payload.stage === 'delta' && typeof payload.delta === 'string') record.text += payload.delta;
          else if (payload.stage === 'end') record.complete = true;
          else return;
        }
        const record = aggregate.get(key)!;
        // 快照事件只恢复当前分支实际已有的段；新事件才可以增加实时段。
        if (!live && !savedKeys.has(key)) return;
        const existing = visible.get(key);
        if (existing?.saved && existing.text.startsWith(record.text)
          && (existing.text.length > record.text.length || existing.complete || !record.complete)) return;
        visible.set(key, { ...record, saved: !live && savedKeys.has(key) });
      };
      const hydrate = async (nextCursor?: string) => {
        const history = await request(`/lingxi/v1/sessions/${encodeURIComponent(sessionId)}/mood-history`);
        if (!currentValid()) return;
        if (history.sessionId !== sessionId || !Array.isArray(history.items)) throw new Error('情绪轨迹返回的会话不一致');
        visible.clear();
        for (const item of history.items) {
          if (item.kind !== 'assistant_segment' || item.segmentKind !== 'mood') continue;
          if (typeof item.segmentId !== 'string' || typeof item.text !== 'string'
            || item.run?.state !== 'known' || typeof item.run.runId !== 'string' || typeof item.complete !== 'boolean') throw new Error('情绪轨迹格式不完整');
          const key = `${item.run.runId}:${item.segmentId}`;
          savedKeys.add(key);
          visible.set(key, { key, segmentId: item.segmentId, runId: item.run.runId, text: item.text, complete: item.complete, saved: true });
        }
        // 服务端给出续页游标时补齐快照，不自己猜测或构造游标。
        while (nextCursor && currentValid()) {
          const page = await request(`/lingxi/v1/sessions/${encodeURIComponent(sessionId)}/mood-events?cursor=${encodeURIComponent(nextCursor)}`);
          if (page.streamId !== moodStream || !Array.isArray(page.items)) throw new Error('情绪事件返回的会话不一致');
          for (const event of page.items) events.set(event.seq, event);
          nextCursor = page.nextCursor || undefined;
        }
        if (!currentValid()) return;
        // 等初始快照最后一帧到达后再按序合流，不能让续页先于起始帧。
        await snapshotReady;
        if (!currentValid()) return;
        hydrated = true;
        for (const event of [...events.values()].sort((a, b) => BigInt(a.seq!) < BigInt(b.seq!) ? -1 : 1)) apply(event);
        events.clear();
        publish();
        setNotice('');
      };
      try {
        const ticket = await requestConnectionWsTicket(connection);
        if (!currentValid()) return;
        socket = new WebSocket(buildConnectionWsUrl(connection, '/lingxi/v1/ws', { wsTicket: ticket }));
        const current = socket;
        current.onopen = () => {
          if (!currentValid()) return;
          current.send(JSON.stringify({ protocol: 'lingxi.wire', clientKind: 'desktop', clientVersion: version, protocolMin: 1, protocolMax: 1, caps: [] }));
          handshakeTimer = setTimeout(() => current.close(), 10_000);
        };
        current.onmessage = event => {
          if (!currentValid() || socket !== current) return;
          try {
            const message = JSON.parse(event.data);
            if (!handshake) {
              if (!isRustWireServerHello(message)) throw new Error('情绪连接版本不兼容');
              handshake = true;
              clearTimeout(handshakeTimer);
              current.send(JSON.stringify({ type: 'subscribe_mood', sessionId }));
              return;
            }
            if (typeof message.code === 'string') throw new Error(`情绪订阅失败：${message.message || message.code}`);
            if (message.frameKind === 'control') {
              if (message.type === 'subscribed') {
                if (typeof message.streamId !== 'string' || !message.streamId.startsWith('lingxi.mood/') || typeof message.snapshotSeq !== 'string' || !/^\d+$/.test(message.snapshotSeq)) throw new Error('情绪订阅返回的会话不一致');
                moodStream = message.streamId;
                cut = BigInt(message.snapshotSeq);
                if (message.snapshotSeq === message.fromSeq) releaseSnapshot();
                void hydrate(message.nextCursor).catch(error => {
                  failureNotice = `情绪历史未恢复：${error.message}`;
                  if (currentValid()) setNotice(failureNotice);
                  current.close();
                });
              } else if (message.type === 'snapshot_required') current.close();
              return;
            }
            if (!hydrated) {
              if (typeof message.seq === 'string' && /^\d+$/.test(message.seq)) {
                events.set(message.seq, message);
                if (BigInt(message.seq) >= cut) releaseSnapshot();
              }
            } else { apply(message); publish(); }
          } catch (error) {
            failureNotice = error instanceof Error ? error.message : '情绪事件无法读取';
            if (currentValid()) setNotice(failureNotice);
            current.close();
          }
        };
        current.onclose = () => {
          releaseSnapshot();
          if (!valid() || socket !== current) return;
          clearTimeout(handshakeTimer);
          generation++;
          // 断连只改变提示；已显示的内容不变，也不改成已保存状态。
          setNotice(failureNotice || '情绪连接已断开，保留当前展示');
          retry = setTimeout(() => { void connect(); }, 5_000);
        };
        current.onerror = () => current.close();
      } catch (error) {
        if (!currentValid()) return;
        setNotice(error instanceof Error ? error.message : '情绪连接不可用');
        retry = setTimeout(() => { void connect(); }, 5_000);
      }
    };
    void connect();
    return () => {
      stopped = true;
      abort.abort();
      clearTimeout(retry);
      clearTimeout(handshakeTimer);
      socket?.close();
    };
  }, [scope]);
  return <aside aria-label="情绪与人格状态">
    {notice && <p role="status">{notice}</p>}
    {records.length === 0 ? <p>暂无情绪轨迹</p> : records.map(record => <details key={record.key}>
      <summary>情绪与人格 · {record.saved ? '已保存的轨迹' : '本次实时变化'}{record.complete ? '' : ' · 未完成'}</summary>
      <pre>{record.text}</pre>
    </details>)}
  </aside>;
}
