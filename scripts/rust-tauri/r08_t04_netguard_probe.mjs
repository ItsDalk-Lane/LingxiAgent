// T04 开工前必查：network-guard-negative 真红只读定性探针。
// 零真实外发：只用 127.0.0.1 / 属性身份断言 / 标记函数，不发任何外部包。
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const http = require('node:http');
const net = require('node:net');
const dns = require('node:dns');

console.log('node:', process.version);

// ① http.Agent.prototype.createConnection 是否在 bootstrap 期绑定了原始 net.createConnection
const identityBefore = http.Agent.prototype.createConnection === net.createConnection;
const origCreate = net.createConnection;
net.createConnection = function patched() { throw new Error('PROBE_PATCH_MARKER'); };
const identityAfter = http.Agent.prototype.createConnection === net.createConnection;
net.createConnection = origCreate;
console.log('agent.prototype.createConnection === net.createConnection (before patch):', identityBefore);
console.log('agent.prototype.createConnection sees patch after patch:', identityAfter);

// ② http.Agent 路径实测（纯本地）：补丁抛标记错误；若 agent 走补丁导出→error 事件带标记；
//    若走 bootstrap 捕获的原始函数→真实连 127.0.0.1:1 → ECONNREFUSED。
net.createConnection = function patched() { throw new Error('PROBE_PATCH_MARKER'); };
const verdict = await new Promise((resolve) => {
  const req = http.request({ host: '127.0.0.1', port: 1, path: '/', timeout: 3000 });
  req.on('error', (e) => resolve({ event: 'error', message: e.message }));
  req.on('timeout', () => { req.destroy(); resolve({ event: 'timeout' }); });
  req.end();
  setTimeout(() => resolve({ event: 'none-8s' }), 8000).unref();
});
net.createConnection = origCreate;
console.log('http.request(127.0.0.1:1) with patched net.createConnection →', JSON.stringify(verdict));

// ③ dns.lookup 是否被 lib/net 动态解析（同样零外发：补丁抛标记，net.connect 本地地址但 host 字符串走 lookup 分支前先判定）
//    用本地主机名不行（守卫语义外），直接测：补丁 dns.lookup 抛标记，net.connect('localhost',1)
const origLookup = dns.lookup;
dns.lookup = function patchedLookup() { throw new Error('PROBE_DNS_MARKER'); };
const dnsVerdict = await new Promise((resolve) => {
  const s = net.connect({ host: 'localhost', port: 1 });
  s.on('error', (e) => resolve({ event: 'error', message: e.message }));
  setTimeout(() => { s.destroy(); resolve({ event: 'none-5s' }); }, 5000).unref();
});
dns.lookup = origLookup;
console.log('net.connect(localhost:1) with patched dns.lookup →', JSON.stringify(dnsVerdict));
