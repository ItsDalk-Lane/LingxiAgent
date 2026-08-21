/**
 * model-call-observer-ext.ts — Provider 请求/响应观测扩展。
 *
 * 与 compaction-guard-ext 同走官方 ExtensionAPI，零 pi SDK 改动。
 *
 * 关联机制（§二十一/§二十七/§二十八）：
 *   Pi streamFn wrapper（lib/pi-sdk/model-call-stream-observer.ts）在调用
 *   original streamFn 时用 ALS 建立 ModelCallScope；pi-ai 内部触发
 *   before_provider_request / after_provider_response 时 promise continuation
 *   继承同一 store，因此 hook 内 currentModelCallScope() 拿到的就是这次调用
 *   的 callId/attemptId。scope 缺失（未标记的后台调用）直接跳过，不猜不伪造。
 *
 * 红线：
 *   - before_provider_request handler 永远 return undefined——绝不修改 payload。
 *   - 只记录 summarizeProviderRequestPayload 的结构 metadata，绝不读取/保存
 *     消息正文、system 文本、工具 schema 细节、base64、headers 全量（§八）。
 *   - handler 抛错被 runner 吞掉，但本扩展仍全 try/catch 自保（§九）。
 */
import {
  extractProviderRequestId,
  getModelCallObserver,
  summarizeProviderRequestPayload,
} from "../llm/model-call-observer.ts";
import { createModelCallRecorder } from "../llm/model-call-recorder.ts";
import { currentModelCallScope } from "../llm/model-call-scope.ts";

export function createModelCallObserverExtension() {
  return function (pi: any) {
    pi.on("before_provider_request", (event: any) => {
      try {
        const scope = currentModelCallScope();
        if (!scope?.callId) return undefined;
        createModelCallRecorder({
          observer: getModelCallObserver(),
          context: scope,
        }).providerRequestPrepared({
          details: summarizeProviderRequestPayload(event?.payload),
        });
      } catch {
        // Observability must never break the model request path.
      }
      // 永远不改 payload：undefined = 保持原样。
      return undefined;
    });

    pi.on("after_provider_response", (event: any) => {
      try {
        const scope = currentModelCallScope();
        if (!scope?.callId) return;
        createModelCallRecorder({
          observer: getModelCallObserver(),
          context: scope,
        }).providerResponseReceived({
          providerRequestId: extractProviderRequestId(event?.headers),
          details: {
            httpStatus: typeof event?.status === "number" ? event.status : null,
          },
        });
      } catch {
        // Observability must never break the model request path.
      }
    });
  };
}
