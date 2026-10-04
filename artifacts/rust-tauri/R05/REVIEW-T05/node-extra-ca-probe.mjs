// REVIEW-T05 C12 leg: does the INCUMBENT model-path transport (Node 24
// built-in fetch) honor a private CA via NODE_EXTRA_CA_CERTS? A TLS server
// presents a cert chained to a private CA; fetch runs with
// NODE_EXTRA_CA_CERTS pointing at that CA (set in the environment BEFORE
// process start by the shell wrapper).
import https from "node:https";
import fs from "node:fs";

const server = https.createServer(
  {
    cert: fs.readFileSync("/tmp/rt05_srv.pem"),
    key: fs.readFileSync("/tmp/rt05_srv_key.pem"),
  },
  (_req, res) => {
    res.writeHead(200, { "content-type": "application/json" }).end('{"ok":true}');
  },
);
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const port = server.address().port;

let status = null;
let error = null;
try {
  const res = await fetch(`https://127.0.0.1:${port}/v1/messages`, { method: "POST", body: "{}" });
  status = res.status;
} catch (err) {
  error = String(err && (err.cause ? err.cause : err));
}
console.log(JSON.stringify({
  node: process.version,
  extraCa: process.env.NODE_EXTRA_CA_CERTS ?? null,
  fetchStatus: status,
  fetchError: error,
  verdict: status === 200
    ? "builtin fetch TRUSTS the private CA (NODE_EXTRA_CA_CERTS honored)"
    : "builtin fetch REJECTS the private CA despite NODE_EXTRA_CA_CERTS",
}, null, 2));
server.close();
