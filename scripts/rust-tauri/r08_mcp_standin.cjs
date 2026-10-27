#!/usr/bin/env node
/**
 * R08 收尾｜D03 确定性 MCP stdio 替身（协议允许的外部确定性替身，见任务书 D 节）。
 *
 * 极简 MCP stdio 服务器：JSON-RPC over stdin/stdout（LSP 风格 Content-Length 帧，
 * 兼容 newline-delimited JSON），支持 initialize / tools/list / resources/list /
 * resources/read / ping。零依赖、零网络、零随机：同输入同输出。
 * 用途：A/B 双内核（旧 Node 栈与 Rust）真实启动同一个连接器进程，验证连接器
 * CRUD/启停/刷新工具/资源读取等路由行为。
 */
const readline = require('node:readline');

const SERVER_INFO = { name: 'r08-mcp-standin', version: '1.0.0' };
const TOOLS = [
  { name: 'r08_echo', description: '原样返回输入文本', inputSchema: { type: 'object', properties: { text: { type: 'string' } }, required: ['text'] } },
  // MCP Apps：_meta.ui.resourceUri 声明的工具即「应用」，launch 面经 resourceUri 绑定。
  {
    name: 'r08_app', description: 'R08 确定性应用替身', inputSchema: { type: 'object', properties: { launchInput: { type: 'string' } } },
    _meta: { ui: { resourceUri: 'ui://r08/app.html' } },
  },
];
const RESOURCES = [
  { uri: 'r08://standin/hello', name: 'hello', mimeType: 'text/plain', text: 'R08-MCP-STANDIN-RESOURCE' },
  { uri: 'ui://r08/app.html', name: 'r08-app', mimeType: 'text/html', text: '<!doctype html><title>R08-APP-STANDIN</title>R08 app resource' },
];

function reply(id, result) { return { jsonrpc: '2.0', id, result }; }
function handle(msg) {
  const { id, method, params } = msg;
  switch (method) {
    case 'initialize':
      return reply(id, { protocolVersion: (params && params.protocolVersion) || '2024-11-05', capabilities: { tools: {}, resources: {} }, serverInfo: SERVER_INFO });
    case 'notifications/initialized':
    case 'initialized':
      return null; // 通知不回
    case 'ping':
      return reply(id, {});
    case 'tools/list':
      return reply(id, { tools: TOOLS });
    case 'resources/list':
      return reply(id, { resources: RESOURCES });
    case 'resources/read': {
      if (!params || typeof params.uri !== 'string') {
        return { jsonrpc: '2.0', id, error: { code: -32002, message: 'unknown or missing resource uri' } };
      }
      const found = RESOURCES.find((r) => r.uri === params.uri);
      if (!found) {
        return { jsonrpc: '2.0', id, error: { code: -32002, message: 'unknown resource uri' } };
      }
      return reply(id, { contents: [found] });
    }
    case 'tools/call': {
      if (params && params.name === 'r08_app') {
        return reply(id, { content: [{ type: 'text', text: `R08-APP-RUN:${JSON.stringify((params.arguments || {}).launchInput ?? null)}` }] });
      }
      return reply(id, { content: [{ type: 'text', text: `ECHO:${(params && params.arguments && params.arguments.text) || ''}` }] });
    }
    default:
      return { jsonrpc: '2.0', id, error: { code: -32601, message: `method not found: ${method}` } };
  }
}

// newline-delimited JSON（旧栈 SDK 与 Rust rmcp 客户端都以行分隔启动握手即可用）
const rl = readline.createInterface({ input: process.stdin });
rl.on('line', (line) => {
  const text = line.trim();
  if (!text) return;
  let msg;
  try { msg = JSON.parse(text); } catch { return; }
  const out = handle(msg);
  if (out) process.stdout.write(`${JSON.stringify(out)}\n`);
});
rl.on('close', () => process.exit(0));
