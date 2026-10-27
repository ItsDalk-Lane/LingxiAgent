#!/usr/bin/env node
// R01-T06 WebDriver 探针（W3C 协议，loopback 直连，无外部 driver 进程）。
// 用法：
//   node probe_webdriver.mjs session-test <port> <out.json>   # 期望嵌入式驱动可用
//   node probe_webdriver.mjs closed-check <port>              # 期望端口关闭（release 产物）
//   node probe_webdriver.mjs finish <port>                    # 在 main 窗口执行 finish_e2e
// 退出码：0 = 探测结果符合该子命令的"可用/关闭"语义；1 = 不符合或协议错误。
import net from 'node:net';
import fs from 'node:fs';

const [cmd, portStr, outPath] = process.argv.slice(2);
const port = Number(portStr);
const base = `http://127.0.0.1:${port}`;

async function wd(method, path, body) {
  const res = await fetch(base + path, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  let json = null;
  try { json = JSON.parse(text); } catch { json = { raw: text }; }
  return { status: res.status, json };
}

function tryConnect(p) {
  return new Promise((resolve) => {
    const sock = net.connect({ host: '127.0.0.1', port: p });
    sock.once('connect', () => { sock.destroy(); resolve('open'); });
    sock.once('error', (e) => resolve('closed:' + e.code));
    setTimeout(() => { sock.destroy(); resolve('timeout'); }, 2000);
  });
}

async function main() {
  if (cmd === 'closed-check') {
    const tcp = await tryConnect(port);
    let http = 'unreachable';
    try { await fetch(base + '/status', { signal: AbortSignal.timeout(2000) }); http = 'responded'; }
    catch (e) { http = 'refused:' + (e.cause?.code || e.message); }
    const closed = tcp.startsWith('closed') && http.startsWith('refused');
    console.log(JSON.stringify({ tcp, http, closed }, null, 2));
    process.exit(closed ? 0 : 1);
  }

  if (cmd === 'session-test') {
    const evidence = { steps: [] };
    const status = await wd('GET', '/status');
    evidence.steps.push({ step: 'status', status: status.status, body: status.json });
    if (status.status !== 200) { fs.writeFileSync(outPath, JSON.stringify(evidence, null, 2)); process.exit(1); }

    const session = await wd('POST', '/session', { capabilities: { alwaysMatch: {} } });
    evidence.steps.push({ step: 'new_session', status: session.status, body: session.json });
    const sessionId = session.json?.value?.sessionId;
    if (!sessionId) { fs.writeFileSync(outPath, JSON.stringify(evidence, null, 2)); process.exit(1); }

    const handles = await wd('GET', `/session/${sessionId}/window/handles`);
    evidence.steps.push({ step: 'handles', status: handles.status, body: handles.json });
    const handleList = handles.json?.value || [];

    const perWindow = {};
    for (const handle of handleList) {
      await wd('POST', `/session/${sessionId}/window`, { handle });
      const id = await wd('POST', `/session/${sessionId}/execute/sync`, {
        script: 'return (document.body && document.body.dataset.label || "") + "|" + document.title + "|" + location.href;',
        args: [],
      });
      const ident = String(id.json?.value ?? '');
      const label = ident.split('|')[0] || ident;
      // 页面自报结果（官方驱动路径读取页面状态）
      const res = await wd('POST', `/session/${sessionId}/execute/sync`, {
        script: 'return window.__T06_RESULT || null;',
        args: [],
      });
      // 驱动主动再试一次敏感命令（execute/async，双向印证页面自报）
      const active = await wd('POST', `/session/${sessionId}/execute/async`, {
        script: 'const done = arguments[arguments.length-1]; try { if (!window.__TAURI__ || !window.__TAURI__.core) { done({ok:false,error:"no __TAURI__"}); } else { window.__TAURI__.core.invoke("shell_sensitive_probe").then(v=>done({ok:true,value:String(v)}),e=>done({ok:false,error:String(e&&e.message||e)})); } } catch(e){ done({ok:false,error:String(e)}); }',
        args: [],
      });
      perWindow[label] = { ident, pageResult: res.json?.value ?? null, driverSensitiveProbe: active.json?.value ?? null };
    }
    evidence.perWindow = perWindow;
    await wd('DELETE', `/session/${sessionId}`);
    fs.writeFileSync(outPath, JSON.stringify(evidence, null, 2));
    console.log(`session-test OK: windows=${Object.keys(perWindow).join(',')}`);
    process.exit(0);
  }

  if (cmd === 'sidecar-recovery') {
    // runner 已在外部 kill -9 sidecar；这里从 main 窗口验证宿主检测死亡并完成一次重启。
    const session = await wd('POST', '/session', { capabilities: { alwaysMatch: {} } });
    const sessionId = session.json?.value?.sessionId;
    if (!sessionId) { console.error('no session'); process.exit(1); }
    const handles = await wd('GET', `/session/${sessionId}/window/handles`);
    let done = false;
    for (const handle of handles.json?.value || []) {
      await wd('POST', `/session/${sessionId}/window`, { handle });
      const id = await wd('POST', `/session/${sessionId}/execute/sync`, {
        script: 'return (document.body && document.body.dataset.label || "");', args: [],
      });
      if (id.json?.value !== 'main') continue;
      const drill = await wd('POST', `/session/${sessionId}/execute/async`, {
        script: `const done = arguments[arguments.length-1]; (async () => {
          const inv = window.__TAURI__.core.invoke;
          const out = {};
          try { out.status_after_kill = await inv('sidecar_status'); } catch(e){ out.status_after_kill = 'ERR:'+String(e); }
          try { out.restart = await inv('sidecar_restart'); } catch(e){ out.restart = 'ERR:'+String(e); }
          try { out.ping_after_restart = await inv('sidecar_ping'); } catch(e){ out.ping_after_restart = 'ERR:'+String(e); }
          try { out.final_status = await inv('sidecar_status'); } catch(e){ out.final_status = 'ERR:'+String(e); }
          done(out);
        })(); setTimeout(()=>done({timeout:true}),15000);`,
        args: [],
      });
      fs.writeFileSync(outPath, JSON.stringify(drill.json?.value ?? { error: drill.json }, null, 2));
      console.log('sidecar-recovery drill captured');
      done = true;
    }
    process.exit(done ? 0 : 1);
  }

  if (cmd === 'finish') {
    const session = await wd('POST', '/session', { capabilities: { alwaysMatch: {} } });
    const sessionId = session.json?.value?.sessionId;
    if (!sessionId) { console.error('no session'); process.exit(1); }
    const handles = await wd('GET', `/session/${sessionId}/window/handles`);
    for (const handle of handles.json?.value || []) {
      await wd('POST', `/session/${sessionId}/window`, { handle });
      const id = await wd('POST', `/session/${sessionId}/execute/sync`, {
        script: 'return (document.body && document.body.dataset.label || "");', args: [],
      });
      if (id.json?.value === 'main') {
        await wd('POST', `/session/${sessionId}/execute/async`, {
          script: 'const done = arguments[arguments.length-1]; window.__TAURI__.core.invoke("finish_e2e").then(()=>done("ok"),e=>done(String(e))); setTimeout(()=>done("timeout"),10000);',
          args: [],
        });
        console.log('finish_e2e invoked in main window');
        process.exit(0);
      }
    }
    console.error('main window not found');
    process.exit(1);
  }

  console.error('unknown subcommand: ' + cmd);
  process.exit(2);
}

main().catch((e) => { console.error(String(e && e.stack || e)); process.exit(1); });
