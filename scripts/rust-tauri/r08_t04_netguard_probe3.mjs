// 探针 ③：agent 路径实际传给 net.createConnection 的参数形态（打印后立即抛标记，零外发）
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const http = require('node:http');
const net = require('node:net');

net.createConnection = function probe(...args) {
  const summarize = (v) => {
    if (v === null || typeof v !== 'object') return JSON.stringify(v);
    const keys = Object.keys(v);
    const pick = {};
    for (const k of ['host','hostname','servername','port','path','localAddress','_agentKey']) if (k in v) pick[k] = v[k];
    return `{keys:${keys.length} ${JSON.stringify(pick)}}`;
  };
  console.log('argc=', args.length, 'args=', args.map(summarize).join(' | '));
  throw new Error('PROBE_MARKER_AFTER_LOG');
};
try { http.request({ host: 'example.com', port: 80, path: '/' }); console.log('no sync throw'); }
catch (e) { console.log('sync throw:', e.message); }
process.exit(0);
