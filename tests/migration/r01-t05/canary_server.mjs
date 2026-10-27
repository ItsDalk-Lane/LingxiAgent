#!/usr/bin/env node
/**
 * R01-T05 loopback canary server — S5 危险资源样本的 http 外发探针。
 * 仅绑 127.0.0.1；任何到达的请求都追加一行 JSON 到日志文件。
 * 用法：node canary_server.mjs [port] [logfile]   默认 18291 /tmp/r01t05/canary.log
 */
import http from "node:http";
import fs from "node:fs";

const PORT = Number(process.argv[2] || 18291);
const LOG = process.argv[3] || "/tmp/r01t05/canary.log";

http.createServer((req, res) => {
  fs.appendFileSync(LOG, JSON.stringify({ t: new Date().toISOString(), method: req.method, url: req.url }) + "\n");
  res.writeHead(200, { "content-type": "image/png" });
  // 1x1 透明 PNG
  res.end(Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==", "base64"));
}).listen(PORT, "127.0.0.1", () => console.error(`[t05-canary] listening http://127.0.0.1:${PORT}`));
