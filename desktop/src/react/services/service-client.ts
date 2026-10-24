/**
 * service-client.ts — 业务服务调用唯一入口（R08-T02）
 *
 * 业务文件（services/** 之外）的服务访问一律从这里导入：
 * - HTTP 核心：lingxiFetch / lingxiUrl / translateLegacySessionCall
 * - 连接解析：server-connection 全量
 * - 服务端适配：rust-* ×10
 * - 会话请求：chatFetch / captureChatUploadScope
 *
 * 纯收拢 re-export，无行为变化。services/** 内部互相直接导入属实现层细节；
 * settings/api.ts 是设置窗口既有独立传输通道，属白名单（见 T02_DESIGN.md §6.3）。
 */

export { lingxiFetch, lingxiUrl, translateLegacySessionCall } from '../hooks/use-hana-fetch';
export * from './server-connection';
export * from './rust-agent-activities';
export * from './rust-automation';
export * from './rust-bridge';
export * from './rust-browser';
export * from './rust-chat-history';
export * from './rust-knowledge';
export * from './rust-media';
export * from './rust-observability';
export * from './rust-settings';
export * from './rust-workbench';
export { chatFetch, captureChatUploadScope } from './chat-client';
