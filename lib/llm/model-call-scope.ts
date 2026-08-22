/**
 * model-call-scope.ts — 模型调用上下文的 AsyncLocalStorage 作用域。
 *
 * 解决的问题与 provider-compat/purpose-scope.ts 相同：Pi 的
 * `before_provider_request` / `after_provider_response` 扩展 hook 在 SDK 内部
 * 触发，事件本身不带 Lingxi 的调用身份，也不能靠 payload 内容反推。用 ALS 把
 * 「当前 in-flight 的 model call」沿异步调用链显式传递：
 *
 *   Pi streamFn wrapper 建立 scope（callId/attemptId/model/source/attribution）
 *     → streamSimple → provider adapter → onPayload → before_provider_request hook
 *       → hook 内 currentModelCallScope() 读到的就是这次调用
 *
 * streamFn 到 onPayload 的调用链（sdk.js 闭包 → ModelRuntime.streamSimple →
 * lazyStream setup → provider stream IIFE）全部在同一 ALS 上下文内创建，
 * promise continuation 继承 store，因此关联是精确的；scope 缺失时 hook 直接
 * 跳过，不猜、不伪造（§四十）。
 */

import { AsyncLocalStorage } from "node:async_hooks";
import type {
  ModelCallAttribution,
  ModelCallModelIdentity,
  ModelCallSource,
} from "./model-call-observer.ts";
import type { ModelSemanticInputProvenance } from "./semantic-input-provenance.ts";
import type { ModelCallPayloadCaptureSession } from "./model-call-payload-capture.ts";

export type ModelCallScope = {
  callId: string;
  /** 当前 attempt。hook 触发时属于这个 attempt（Pi SDK 内部 retry 见 §三十一）。 */
  attemptId?: string | null;
  traceId?: string | null;
  parentCallId?: string | null;
  model?: ModelCallModelIdentity | null;
  source?: ModelCallSource | null;
  attribution?: ModelCallAttribution | null;
  /** 上游（如 MC-02 runner）附带的小型安全 metadata，merge 进 logical_call_start details。 */
  details?: Record<string, unknown> | null;
  /**
   * Phase 5：上游构造的 Semantic Input Provenance（MC-02 runner 拥有
   * instruction/liveMessages 的来源知识）。stream observer 读取并 attach 到
   * recorder；未提供时 observer 走 MC-01 自动分类。仅安全 locator/source
   * metadata，不含内容（§八十）。
   */
  semanticInputProvenance?: ModelSemanticInputProvenance | null;
  /**
   * Phase 6（§七十六～§七十八）：capture session 的**能力引用**——只含身份/
   * 计数器/sink 引用，绝不含正文；使 provider hooks（在 SDK 内部触发、只能
   * 靠 ALS 关联）能与 streamFn 边界的 recorder 共享同一 capture 通道（§一二三：
   * hook 里的临时 recorder 看不到原 recorder 实例，共享的是 session handle）。
   */
  payloadCapture?: ModelCallPayloadCaptureSession | null;
};

const MODEL_CALL_STORAGE = new AsyncLocalStorage<ModelCallScope>();

export function runWithModelCallScope<T>(scope: ModelCallScope, fn: () => T): T {
  return MODEL_CALL_STORAGE.run(scope, fn);
}

/** 当前异步调用链上的 in-flight model call；未标记时为 null。 */
export function currentModelCallScope(): ModelCallScope | null {
  return MODEL_CALL_STORAGE.getStore() ?? null;
}
