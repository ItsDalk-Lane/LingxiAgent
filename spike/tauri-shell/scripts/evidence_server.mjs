#!/usr/bin/env node
// R01-T06 loopback 证据服务器（唯一网络监听：127.0.0.1，默认端口 19274）。
// 职责：
//   - 提供"远端源"页面 /remote.html 与 /probe.js（证明远端 URL 不获原生权限）
//   - 收取页面 POST /report（页面侧探针结果）
//   - 提供 /updates/latest.json 与 /updates/<file>（updater 环回探测，内容由 runner 生成）
//   - GET /__status 供 runner 轮询
// 所有内容合成、无任何真实数据；不代理、不外发。
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';

const PORT = Number(process.env.T06_EVIDENCE_PORT || 19274);
const OUT_DIR = process.env.T06_EVIDENCE_DIR; // 必传：报告落盘目录
const WWW_DIR = process.env.T06_WWW_DIR;      // remote.html 所在目录
const PROBE_JS = process.env.T06_PROBE_JS;    // frontend/probe.js 路径
const UPDATES_DIR = process.env.T06_UPDATES_DIR || ''; // runner 生成的 latest.json / 伪更新包

if (!OUT_DIR || !WWW_DIR || !PROBE_JS) {
  console.error('T06_EVIDENCE_DIR / T06_WWW_DIR / T06_PROBE_JS required');
  process.exit(2);
}
fs.mkdirSync(OUT_DIR, { recursive: true });
const reportPath = path.join(OUT_DIR, 'page-reports.jsonl');

const server = http.createServer((req, res) => {
  const url = new URL(req.url || '/', `http://127.0.0.1:${PORT}`);
  // CORS：页面从 tauri://localhost 跨源 POST 需要预检放行（loopback 证据通道）。
  if (req.method === 'OPTIONS') {
    res.writeHead(204, {
      'access-control-allow-origin': '*',
      'access-control-allow-methods': 'POST, GET, OPTIONS',
      'access-control-allow-headers': 'content-type',
    });
    res.end();
    return;
  }
  if (req.method === 'GET' && url.pathname === '/remote.html') {
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    res.end(fs.readFileSync(path.join(WWW_DIR, 'remote.html')));
    return;
  }
  if (req.method === 'GET' && url.pathname === '/probe.js') {
    res.writeHead(200, { 'content-type': 'application/javascript; charset=utf-8' });
    res.end(fs.readFileSync(PROBE_JS));
    return;
  }
  if (req.method === 'POST' && url.pathname === '/report') {
    let body = '';
    req.on('data', (c) => { body += c; if (body.length > 1_000_000) req.destroy(); });
    req.on('end', () => {
      fs.appendFileSync(reportPath, body + '\n');
      res.writeHead(200, { 'content-type': 'application/json', 'access-control-allow-origin': '*' });
      res.end('{"ok":true}');
    });
    return;
  }
  if (req.method === 'GET' && url.pathname.startsWith('/updates/')) {
    const name = path.basename(url.pathname);
    const file = path.join(UPDATES_DIR, name);
    if (UPDATES_DIR && fs.existsSync(file)) {
      res.writeHead(200, { 'content-type': 'application/octet-stream' });
      res.end(fs.readFileSync(file));
    } else {
      res.writeHead(404); res.end('not found');
    }
    return;
  }
  if (req.method === 'GET' && url.pathname === '/__status') {
    const reports = fs.existsSync(reportPath)
      ? fs.readFileSync(reportPath, 'utf8').trim().split('\n').filter(Boolean).map((l) => {
          try { return JSON.parse(l).label; } catch { return 'unparseable'; }
        })
      : [];
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ reports }));
    return;
  }
  res.writeHead(404); res.end('not found');
});
server.listen(PORT, '127.0.0.1', () => {
  console.log(`[t06-evidence] listening on http://127.0.0.1:${PORT}`);
});
process.on('SIGTERM', () => process.exit(0));
