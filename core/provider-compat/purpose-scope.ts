/**
 * purpose-scope.ts — provider compat 请求用途（purpose）作用域
 *
 * purpose 决定模型级 capability 开关（model.web / model.structuredOutput）
 * 是否参与 payload 注入：
 *   - "chat"      普通用户聊天（唯一默认继承模型开关的用途）
 *   - "utility"   非流式内部调用（title / summary / memory / approval / health…）
 *   - "compaction" 上下文压缩
 *
 * 难点：Pi 原生 compaction 复用 session 的 onPayload 扩展链（与普通聊天同一
 * before_provider_request 入口），扩展事件本身不带用途标记，也不能靠猜 payload
 * 判断调用目的。因此用 AsyncLocalStorage 把「当前请求用途」沿异步调用链显式
 * 传递：compaction 调用点用 runWithProviderCompatPurpose("compaction", ...) 包住
 * onPayload 调用，engine 扩展在该作用域内读到的 purpose 即为 compaction。
 */

import { AsyncLocalStorage } from "node:async_hooks";

export type ProviderCompatPurpose = "chat" | "utility" | "compaction" | "knowledge_rollup";

const PURPOSE_STORAGE = new AsyncLocalStorage<{ purpose: ProviderCompatPurpose }>();

export function normalizeProviderCompatPurpose(value): ProviderCompatPurpose {
  return value === "utility" || value === "compaction" || value === "knowledge_rollup"
    ? value
    : "chat";
}

export function runWithProviderCompatPurpose<T>(purpose: ProviderCompatPurpose, fn: () => T): T {
  return PURPOSE_STORAGE.run({ purpose: normalizeProviderCompatPurpose(purpose) }, fn);
}

/**
 * 当前异步调用链的请求用途。未显式标记时默认 "chat"（普通聊天会话路径）。
 * utility 路径（llm-client）与 compaction 路径都会显式标记。
 */
export function currentProviderCompatPurpose(): ProviderCompatPurpose {
  return PURPOSE_STORAGE.getStore()?.purpose ?? "chat";
}
