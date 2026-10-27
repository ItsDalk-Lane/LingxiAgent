// 探针 ④（决定性，零外发）：createConnection 与 dns.lookup 双补丁+双日志，定位绕过层
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const http = require('node:http');
const net = require('node:net');
const dns = require('node:dns');

net.createConnection = function (...args) {
  console.log('[L1] patched net.createConnection INVOKED');
  throw new Error('MARKER_CREATE_CONNECTION');
};
dns.lookup = function (...args) {
  console.log('[L2] patched dns.lookup INVOKED host=', JSON.stringify(args[0]));
  throw new Error('MARKER_DNS_LOOKUP');
};
try {
  const req = http.request({ host: 'example.com', port: 80, path: '/' });
  console.log('[R] no sync throw');
  req.on('error', (e) => { console.log('[R] async error:', e.message); process.exit(0); });
  req.on('timeout', () => { console.log('[R] timeout event'); process.exit(0); });
  req.end();
  setTimeout(() => { console.log('[R] silent 10s — 无任何事件'); process.exit(2); }, 10000);
} catch (e) {
  console.log('[R] sync throw:', e.message);
  process.exit(0);
}
