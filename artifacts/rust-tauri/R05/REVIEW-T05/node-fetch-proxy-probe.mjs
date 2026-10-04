// REVIEW-T05 item 6: does Node 24 built-in fetch (undici) honor ambient
// HTTP_PROXY/HTTPS_PROXY env vars by default? A counting loopback "proxy"
// records every request line it receives; a plain HTTP stub is the target.
// If fetch routes via the env proxy, the proxy's hit counter moves.
import http from "node:http";

const proxyHits = [];
const proxy = http.createServer((req, res) => {
  proxyHits.push(`${req.method} ${req.url}`);
  res.writeHead(502).end("proxy intercepted");
});
await new Promise((r) => proxy.listen(0, "127.0.0.1", r));
const proxyPort = proxy.address().port;

const target = http.createServer((req, res) => {
  res.writeHead(200, { "content-type": "application/json" }).end('{"ok":true}');
});
await new Promise((r) => target.listen(0, "127.0.0.1", r));
const targetPort = target.address().port;

process.env.HTTP_PROXY = `http://127.0.0.1:${proxyPort}`;
process.env.HTTPS_PROXY = `http://127.0.0.1:${proxyPort}`;
process.env.http_proxy = `http://127.0.0.1:${proxyPort}`;
process.env.https_proxy = `http://127.0.0.1:${proxyPort}`;

let status = null;
let error = null;
try {
  const res = await fetch(`http://127.0.0.1:${targetPort}/v1/messages`, { method: "POST", body: "{}" });
  status = res.status;
} catch (err) {
  error = String(err);
}
console.log(JSON.stringify({
  node: process.version,
  fetchStatus: status,
  fetchError: error,
  proxyHits,
  verdict: proxyHits.length === 0 && status === 200
    ? "builtin fetch IGNORED ambient HTTP(S)_PROXY (direct answer from the target)"
    : "builtin fetch ROUTED via the ambient proxy",
}, null, 2));
proxy.close();
target.close();
