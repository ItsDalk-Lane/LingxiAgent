// 探针 ② 补全：http.request 同步抛被捕获后的行为 + ③ dns.lookup 捕获形态（零外发）
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const http = require('node:http');
const net = require('node:net');
const dns = require('node:dns');

// ②b：captureSync 形态复现（守卫语义：补丁按 host 判定，example.com 视为外部）
net.createConnection = function patched(...args) {
  const t = args.find((a) => typeof a === 'object' && a !== null) || {};
  const host = t.host ?? args.find((a) => typeof a === 'string');
  if (typeof host === 'string' && !['localhost','127.0.0.1','::1'].includes(host) && !host.includes('/')) {
    throw new Error(`PROBE blocked: ${host}`);
  }
  return Reflect.apply(Object.getPrototypeOf(net).createConnection ?? (()=>{}), null, args);
};
let r;
try { http.request({ host: 'example.com', port: 80, path: '/' }); r = { threw: false }; }
catch (e) { r = { threw: true, message: e.message }; }
console.log('http.request(example.com) with guard-style patch →', JSON.stringify(r));

// ③ dns.lookup：补丁后 net.connect('localhost',1) 是否触达补丁（lib/net 动态解析 or bootstrap 捕获）
const origLookup = dns.lookup;
let lookupTouched = false;
dns.lookup = function patchedLookup(...args) { lookupTouched = true; return origLookup(...args); };
await new Promise((resolve) => {
  const s = net.connect({ host: 'localhost', port: 1 });
  s.on('error', () => resolve());
  setTimeout(() => { s.destroy(); resolve(); }, 5000).unref();
});
dns.lookup = origLookup;
console.log('lib/net reaches patched dns.lookup:', lookupTouched);
