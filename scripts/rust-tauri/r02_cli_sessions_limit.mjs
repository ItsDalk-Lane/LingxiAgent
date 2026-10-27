// 真服务的合成根仅含两条会话，无法触发 CLI 的 20 条显示上限。
// 此处仅用模拟的 25 条成功响应检查真实 CLI 渲染入口；身份和主体隔离
// 另由同一叶生产者运行的真服务 owner/foreign/无凭据分支证明。
import { main } from '../../cli/entry.ts';

const messages = [];
const errors = [];
const originalFetch = globalThis.fetch;
const originalLog = console.log;
const originalError = console.error;
try {
  globalThis.fetch = async () => new Response(JSON.stringify({
    sessions: Array.from({ length: 25 }, (_, index) => ({
      sessionId: `synthetic-${index + 1}`,
      title: `Synthetic bounded session ${index + 1}`,
      agentId: 'lingxi',
    })),
  }), { status: 200, headers: { 'content-type': 'application/json' } });
  console.log = (...args) => { messages.push(args.map(String).join(' ')); };
  console.error = (...args) => { errors.push(args.map(String).join(' ')); };
  const exitCode = await main([
    'sessions', '--runtime', 'rust', '--url', 'http://127.0.0.1:14567', '--token', 'synthetic-only',
  ]);
  const actual = Number(exitCode === 0 && messages.length === 20 && errors.length === 0
    && messages[0]?.includes('Synthetic bounded session 1')
    && messages[19]?.includes('Synthetic bounded session 20')
    && !messages.join('\n').includes('Synthetic bounded session 21'));
  process.stdout.write(`${JSON.stringify({
    case: 'a05-cli-sessions-limit-20', expect: 1, actual, ok: actual === 1,
    observed: { exitCode, renderedLines: messages.length, errors: errors.length, suppliedSessions: 25 },
  })}\n`);
  if (actual !== 1) process.exitCode = 1;
} finally {
  globalThis.fetch = originalFetch;
  console.log = originalLog;
  console.error = originalError;
}
