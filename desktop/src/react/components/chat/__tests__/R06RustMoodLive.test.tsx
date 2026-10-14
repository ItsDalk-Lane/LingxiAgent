// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { createRequire } from 'node:module';
const NetworkSocket = createRequire(import.meta.url)('ws').WebSocket as typeof WebSocket;
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it } from 'vitest';
import { useStore } from '../../../stores';
import { createLocalServerConnection } from '../../../services/server-connection';
import { RustMoodPanel } from '../RustMoodPanel';

afterEach(cleanup);
const base = process.env.R06_MOOD_UI_BASE;
it.skipIf(!base)('rust_mood_current_chat_real_service', async () => {
  const token = process.env.R06_MOOD_UI_TOKEN!;
  const release = process.env.R06_MOOD_UI_RELEASE!;
  const connection = createLocalServerConnection({ serverPort: new URL(base!).port, serverToken: token, serverNodeKind: 'lingxi-service' });
  expect(connection).not.toBeNull();
  useStore.setState({serverConnections:{[connection!.connectionId]:connection!},activeServerConnectionId:connection!.connectionId,activeServerConnection:connection,currentSessionId:'sess_local_alpha',currentSessionPath:'mood-ui-session'});
  const sockets: WebSocket[] = [];
  const Original = globalThis.WebSocket;
  // 真实网络套接字仅记录断连入口，避免测试容器的两套 Event 类混用。
  class RecordedSocket extends NetworkSocket { constructor(url: string | URL, protocols?: string | string[]) { super(url, protocols); sockets.push(this); } }
  globalThis.WebSocket = RecordedSocket as unknown as typeof WebSocket;
  const request = async (session: string, input: string) => {
    const response = await fetch(`${base}/lingxi/v1/sessions/${session}/execute`,{method:'POST',headers:{Authorization:`Bearer ${token}`,'Content-Type':'application/json'},body:JSON.stringify({input})});
    expect(response.status).toBe(200);return response.json();
  };
  try {
    const view = render(<RustMoodPanel />);
    await waitFor(() => expect(screen.queryByText('正在读取情绪轨迹')).not.toBeInTheDocument(), { timeout: 10_000 });
    expect(screen.getByText('暂无情绪轨迹')).toBeInTheDocument();
    let finished = false;
    const execute = request('sess_local_alpha','LIVE_UI').then(value=>{finished=true;return value;});
    await screen.findByText('情绪与人格 · 本次实时变化 · 未完成');
    fireEvent.click(screen.getByText('情绪与人格 · 本次实时变化 · 未完成'));
    await screen.findByText('清醒');
    expect(finished).toBe(false);
    expect(screen.queryByText(/已保存的轨迹/)).not.toBeInTheDocument();
    await act(async () => { sockets.at(-1)!.close(); });
    await screen.findByText('情绪连接已断开，保留当前展示');
    expect(screen.getByText('清醒')).toBeInTheDocument();
    expect(screen.queryByText(/已保存的轨迹/)).not.toBeInTheDocument();
    expect((await fetch(release,{method:'POST'})).status).toBe(200);
    await execute;
    expect(screen.getByText('清醒')).toBeInTheDocument();
    expect(screen.queryByText(/已保存的轨迹/)).not.toBeInTheDocument();
    view.unmount();
    const reopened = render(<RustMoodPanel />);
    await screen.findByText('情绪与人格 · 已保存的轨迹');
    fireEvent.click(screen.getByText('情绪与人格 · 已保存的轨迹'));
    await screen.findByText('清醒 Will: 保持克制');
    await request('sess_local_beta','NO_MOOD');
    await act(async()=>{useStore.setState({currentSessionId:'sess_local_beta'});});
    await waitFor(() => expect(screen.queryByText('正在读取情绪轨迹')).not.toBeInTheDocument(), { timeout: 10_000 });
    expect(screen.getByText('暂无情绪轨迹')).toBeInTheDocument();
    expect(screen.queryByText(/清醒/)).not.toBeInTheDocument();
    expect(screen.queryByText(/本次实时变化/)).not.toBeInTheDocument();
    reopened.unmount();
  } finally {globalThis.WebSocket=Original; for(const socket of sockets)socket.close();}
},50_000);
