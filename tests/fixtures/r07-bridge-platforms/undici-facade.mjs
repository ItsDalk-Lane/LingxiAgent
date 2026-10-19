// 保留真实代理与连接器类型，只替换外部 HTTP 响应。
export * from 'r07-undici-real';
export { platformFetch as fetch } from './platform-http.mjs';
