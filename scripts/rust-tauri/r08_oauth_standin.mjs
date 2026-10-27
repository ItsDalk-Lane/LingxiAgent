#!/usr/bin/env node
/**
 * R08 收尾｜D03 OAuth 授权服务器替身（本机环回、确定性）。
 *
 * 实现 MCP OAuth 需要的最小授权服务器面（RFC 9728 保护资源元数据 → RFC 8414
 * 授权服务器元数据 → RFC 7591 动态客户端注册 → 令牌端点），双内核
 * （旧 Node 栈 core/mcp/clients/oauth.ts 与 Rust rmcp AuthorizationManager）
 * 对同一实例完成发现、注册与授权 URL 生成。令牌值运行时随机生成（占位，
 * 非可用凭据）。就绪时向 stdout 打一行 `R08_OAUTH_READY <port>`。
 */
import http from 'node:http';
import crypto from 'node:crypto';

const args = process.argv.slice(2);
const port = Number(args[args.indexOf('--port') + 1] ?? 0);
if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error('--port must be an integer in [0, 65535]');

const SCOPES_SUPPORTED = ['mcp:tools', 'offline_access'];

function send(res, status, body, headers = {}) {
  res.writeHead(status, { 'Content-Type': 'application/json', ...headers });
  res.end(JSON.stringify(body));
}

const server = http.createServer((req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1');
  const base = `http://127.0.0.1:${server.address()?.port ?? port}`;
  if (req.method === 'GET' && url.pathname === '/mcp') {
    send(res, 401, { error: 'unauthorized' }, {
      'WWW-Authenticate': `Bearer resource_metadata="${base}/.well-known/oauth-protected-resource"`,
    });
    return;
  }
  if (req.method === 'GET' && url.pathname === '/.well-known/oauth-protected-resource') {
    send(res, 200, {
      resource: `${base}/mcp`,
      authorization_servers: [base],
      scopes_supported: SCOPES_SUPPORTED,
      bearer_methods_supported: ['header'],
    });
    return;
  }
  if (req.method === 'GET' && (url.pathname === '/.well-known/oauth-authorization-server' || url.pathname === '/.well-known/openid-configuration')) {
    send(res, 200, {
      issuer: base,
      authorization_endpoint: `${base}/authorize`,
      token_endpoint: `${base}/token`,
      registration_endpoint: `${base}/register`,
      response_types_supported: ['code'],
      grant_types_supported: ['authorization_code', 'refresh_token'],
      code_challenge_methods_supported: ['S256'],
      scopes_supported: SCOPES_SUPPORTED,
      token_endpoint_auth_methods_supported: ['none', 'client_secret_post'],
    });
    return;
  }
  if (req.method === 'POST' && url.pathname === '/register') {
    let body = '';
    req.on('data', (chunk) => { body += chunk; });
    req.on('end', () => {
      let parsed = {};
      try { parsed = JSON.parse(body || '{}'); } catch { /* 空体按缺省 */ }
      send(res, 201, {
        client_id: `r08-client-${crypto.randomUUID().slice(0, 8)}`,
        client_id_issued_at: Math.floor(Date.now() / 1000),
        redirect_uris: Array.isArray(parsed.redirect_uris) ? parsed.redirect_uris : [],
        grant_types: ['authorization_code', 'refresh_token'],
        response_types: ['code'],
        token_endpoint_auth_method: 'none',
        scope: SCOPES_SUPPORTED.join(' '),
      });
    });
    return;
  }
  if (req.method === 'POST' && url.pathname === '/token') {
    let body = '';
    req.on('data', (chunk) => { body += chunk; });
    req.on('end', () => {
      void body;
      send(res, 200, {
        access_token: `r08-synthetic-${crypto.randomUUID().slice(0, 12)}`,
        token_type: 'Bearer',
        expires_in: 3600,
        refresh_token: `r08-refresh-${crypto.randomUUID().slice(0, 12)}`,
        scope: SCOPES_SUPPORTED.join(' '),
      });
    });
    return;
  }
  if (req.method === 'GET' && url.pathname === '/authorize') {
    // 浏览器面不需要真实页面：双内核只构造该 URL（start 返回给客户端打开）。
    send(res, 200, { status: 'ok', note: 'r08 oauth standin authorize' });
    return;
  }
  if (req.method === 'GET' && url.pathname === '/info') {
    send(res, 200, { schema: 'r08-oauth-standin/v1', base });
    return;
  }
  send(res, 404, { error: 'unknown route' });
});

server.listen(port, '127.0.0.1', () => {
  process.stdout.write(`R08_OAUTH_READY ${server.address().port}\n`);
});
process.on('SIGTERM', () => server.close(() => process.exit(0)));
process.on('SIGINT', () => server.close(() => process.exit(0)));
