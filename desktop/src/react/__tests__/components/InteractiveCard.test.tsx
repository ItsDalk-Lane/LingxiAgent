/** @vitest-environment jsdom */
import React from 'react';
import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import type { ServerConnection } from '../../services/server-connection';
const mock = vi.hoisted(() => ({ fetch: vi.fn(), capture: vi.fn(), owns: vi.fn() }));
vi.mock('../../services/session-connection-owner', () => ({ sessionOwnerFetch: mock.fetch, captureSessionConnection: mock.capture, ownsSessionConnection: mock.owns }));
import { InteractiveCard } from '../../components/chat/InteractiveCard';
const block = { type: 'interactive_card' as const, cardId: 'c_actual', title: '卡片', code: '<button>按钮</button>' };
function connection(id: string): ServerConnection { return { connectionId:id, kind:'local', baseUrl:`http://127.0.0.1:${id==='A'?3011:3012}`, wsUrl:'ws://127.0.0.1', token:`fixture-${id}`, serverId:id, serverNodeKind:'lingxi-service', studioId:'local', label:id, authState:'paired', trustState:'local', credentialKind:'loopback_token', capabilities:[] }; }
function select(value: ServerConnection) {
  window.hana={cardFrameNavigationGuard:true} as typeof window.hana;
  useStore.setState({ serverConnections:{[value.connectionId]:value}, activeServerConnectionId:value.connectionId, activeServerConnection:value });
  const owner={connection:structuredClone(value),key:value.connectionId,revision:1};mock.capture.mockReturnValue(owner);mock.owns.mockImplementation((candidate:typeof owner)=>candidate?.key===useStore.getState().activeServerConnectionId);
}
const previous=useStore.getState();
const previousHana=window.hana;
const success=()=>new Response(JSON.stringify({ok:true,frameUrl:`/lingxi/v1/cards/c_actual/frame/${'a'.repeat(64)}`}));
afterEach(()=>{cleanup();window.hana=previousHana;document.querySelectorAll('meta[data-card-test]').forEach(meta=>meta.remove());useStore.setState(previous);vi.resetAllMocks();});
describe('互动卡片连接归属',()=>{
  it('使用捕获连接登记并加载sandbox iframe',async()=>{
    select(connection('A'));mock.fetch.mockResolvedValue(success());
    render(<InteractiveCard block={block}/>);
    const frame=await waitFor(()=>{const value=screen.getByTitle('卡片');expect(value.getAttribute('src')).toContain('/lingxi/v1/cards/c_actual');return value;});
    expect(frame.getAttribute('src')).not.toContain('fixture-A');expect(frame.getAttribute('src')).not.toContain('token=');
    expect(frame.getAttribute('sandbox')).toBe('allow-scripts');expect(frame.getAttribute('referrerpolicy')).toBe('no-referrer');
    expect(mock.fetch.mock.calls[0][1]).toBe('/lingxi/v1/cards/c_actual');
  });
  it('切连接当帧隐藏旧卡片，迟到登记不能把旧来源重新显示',async()=>{
    select(connection('A'));let finish!: (value:Response)=>void;
    mock.fetch.mockReturnValueOnce(new Promise<Response>(resolve=>{finish=resolve;}));
    const view=render(<InteractiveCard block={block}/>);
    await waitFor(()=>expect(mock.fetch).toHaveBeenCalledTimes(1));
    mock.fetch.mockResolvedValue(success());act(()=>select(connection('B')));
    view.rerender(<InteractiveCard block={block}/>);
    await waitFor(()=>expect(screen.getByTitle('卡片').getAttribute('src')).toContain('3012'));
    await act(async()=>{finish(success());});
    expect(screen.getByTitle('卡片').getAttribute('src')).toContain('3012');
    expect(mock.fetch.mock.calls[0][2].signal.aborted).toBe(true);
  });
  it('宿主没有导航保护且父页面未限同源时明确失败',async()=>{
    select(connection('A'));window.hana=undefined as unknown as typeof window.hana;mock.fetch.mockResolvedValue(success());const log=vi.spyOn(console,'error').mockImplementation(()=>{});
    render(<InteractiveCard block={block}/>);await waitFor(()=>expect(screen.getByRole('alert')).toBeTruthy());expect(screen.queryByTitle('卡片')).toBeNull();log.mockRestore();
  });
  it('网页同源且父策略只允许self与blob时可显示票据卡片',async()=>{
    const value={...connection('A'),baseUrl:window.location.origin};select(value);window.hana=undefined as unknown as typeof window.hana;
    const meta=document.createElement('meta');meta.dataset.cardTest='1';meta.httpEquiv='Content-Security-Policy';meta.content="default-src 'self'; frame-src 'self' blob:";document.head.append(meta);
    mock.fetch.mockResolvedValue(success());render(<InteractiveCard block={block}/>);
    await waitFor(()=>expect(screen.getByTitle('卡片').getAttribute('src')).toContain(window.location.origin+'/lingxi/v1/cards/'));
  });
  it('旧服务无票据或不受控frameUrl拒绝显示，不回退整机token',async()=>{
    select(connection('A'));mock.fetch.mockResolvedValue(new Response(JSON.stringify({ok:true,frameUrl:'https://outside.invalid/card'})));const log=vi.spyOn(console,'error').mockImplementation(()=>{});
    render(<InteractiveCard block={block}/>);await waitFor(()=>expect(screen.getByRole('alert')).toBeTruthy());expect(screen.queryByTitle('卡片')).toBeNull();log.mockRestore();
  });
  it('登记失败明确显示错误，不伪装成空白成功',async()=>{
    select(connection('A'));mock.fetch.mockRejectedValue(new Error('synthetic failure'));const log=vi.spyOn(console,'error').mockImplementation(()=>{});
    render(<InteractiveCard block={block}/>);await waitFor(()=>expect(screen.getByRole('alert')).toBeTruthy());expect(screen.queryByTitle('卡片')).toBeNull();log.mockRestore();
  });
});
