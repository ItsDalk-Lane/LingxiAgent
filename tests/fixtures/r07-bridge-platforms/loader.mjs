// 仅替换外部 SDK、HTTP 与 WebSocket；所有产品适配器及管道均从正式入口加载。
import { registerHooks } from 'node:module';
import { platformFetch } from './platform-http.mjs';
globalThis.fetch = platformFetch;
registerHooks({ resolve(specifier, context, nextResolve) {
  if (specifier === 'r07-undici-real') return nextResolve('undici', context);
  const external = { 'node-telegram-bot-api': 'external-telegram.mjs', '@larksuiteoapi/node-sdk': 'external-feishu.mjs', ws: 'external-ws.mjs', undici: 'undici-facade.mjs' }[specifier];
  return external ? { url: new URL(external, import.meta.url).href, shortCircuit: true } : nextResolve(specifier, context);
} });
