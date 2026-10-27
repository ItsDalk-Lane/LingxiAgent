#!/usr/bin/env node
/**
 * r01-t04-proxy.mjs — R01-T04 代理能力实测用的本地正向代理（CONNECT + 绝对 URI HTTP）。
 * 仅绑 127.0.0.1；每个请求/隧道写一行日志到 stderr（由驱动脚本采集到证据目录）。
 * 用法：node r01-t04-proxy.mjs [port]   默认 18282
 */
import http from "node:http";
import net from "node:net";

const PORT = Number(process.argv[2] || 18282);
const HOST = "127.0.0.1";

const server = http.createServer((req, res) => {
  // 绝对 URI 形式的普通 HTTP 代理转发
  console.error(`[t04-proxy] HTTP ${req.method} ${req.url}`);
  let target;
  try {
    target = new URL(req.url);
  } catch {
    res.writeHead(400).end("bad proxy request");
    return;
  }
  // 红线：本测试代理只转发 loopback 目标，拒绝一切真实外发。
  if (target.hostname !== "127.0.0.1" && target.hostname !== "localhost") {
    console.error(`[t04-proxy] REFUSED non-loopback ${req.url}`);
    res.writeHead(403).end("loopback only");
    return;
  }
  const upstream = http.request(
    {
      host: target.hostname,
      port: target.port || 80,
      path: target.pathname + target.search,
      method: req.method,
      headers: { ...req.headers, host: target.host },
    },
    (up) => {
      res.writeHead(up.statusCode || 502, up.headers);
      up.pipe(res);
    },
  );
  upstream.on("error", (e) => {
    res.writeHead(502).end(`proxy upstream error: ${e.message}`);
  });
  req.pipe(upstream);
});

server.on("connect", (req, clientSocket, head) => {
  const [host, port] = String(req.url).split(":");
  if (host !== "127.0.0.1" && host !== "localhost") {
    console.error(`[t04-proxy] REFUSED CONNECT ${req.url}`);
    clientSocket.write("HTTP/1.1 403 Forbidden\r\n\r\n");
    clientSocket.destroy();
    return;
  }
  console.error(`[t04-proxy] CONNECT ${req.url}`);
  const upstream = net.connect(Number(port) || 443, host, () => {
    clientSocket.write("HTTP/1.1 200 Connection Established\r\n\r\n");
    if (head && head.length) upstream.write(head);
    upstream.pipe(clientSocket);
    clientSocket.pipe(upstream);
  });
  upstream.on("error", () => clientSocket.destroy());
});

server.listen(PORT, HOST, () => {
  console.log(`[t04-proxy] listening http://${HOST}:${PORT}`);
});
