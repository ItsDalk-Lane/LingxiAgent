// @vitest-environment jsdom
import '@testing-library/jest-dom/vitest';
import { createRequire } from 'node:module';
const NetworkSocket=createRequire(import.meta.url)('ws').WebSocket as typeof WebSocket;
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it } from 'vitest';
import { useStore } from '../../../stores';
import { createLocalServerConnection } from '../../../services/server-connection';
import { RustMoodPanel } from '../RustMoodPanel';
afterEach(cleanup);
it.skipIf(!process.env.R06_MOOD_UI_BASE)('reviewer_mood_paginated_history_actual_network',async()=>{
  const base=process.env.R06_MOOD_UI_BASE!;const token=process.env.R06_MOOD_UI_TOKEN!;
  expect(base).toBeTruthy();
  const connection=createLocalServerConnection({serverPort:new URL(base).port,serverToken:token,serverNodeKind:'lingxi-service'})!;
  // 只设置登记表连接，验证实际组件沿生产解析器选择同源连接。
  useStore.setState({serverConnections:{[connection.connectionId]:connection},activeServerConnectionId:connection.connectionId,activeServerConnection:null,currentSessionId:'sess_local_alpha',currentSessionPath:'review-pagination'});
  const sockets:WebSocket[]=[];const Original=globalThis.WebSocket;
  class RecordedSocket extends NetworkSocket {constructor(url:string|URL,protocols?:string|string[]){super(url,protocols);sockets.push(this);}}
  globalThis.WebSocket=RecordedSocket as unknown as typeof WebSocket;
  try{
    const executed=await fetch(`${base}/lingxi/v1/sessions/sess_local_alpha/execute`,{method:'POST',headers:{Authorization:`Bearer ${token}`,'Content-Type':'application/json'},body:JSON.stringify({input:'REVIEW_MANY_MOOD'})});
    expect(executed.status).toBe(200);
    const history=await fetch(`${base}/lingxi/v1/sessions/sess_local_alpha/mood-history`,{headers:{Authorization:`Bearer ${token}`}}).then(r=>r.json());
    expect(history.items.filter((i:any)=>i.segmentKind==='mood')).toHaveLength(300);
    render(<RustMoodPanel/>);
    try {await waitFor(()=>expect(screen.getAllByText('情绪与人格 · 已保存的轨迹')).toHaveLength(300),{timeout:7000});}
    finally {console.log('review pagination actual UI=',document.body.textContent);}
  }finally{globalThis.WebSocket=Original;for(const socket of sockets)socket.close();}
},30_000);
