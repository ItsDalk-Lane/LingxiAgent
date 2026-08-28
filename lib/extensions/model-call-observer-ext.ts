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
import { resolveOutputBudgetFact } from "../../core/provider-compat/output-budget.ts";

export function createModelCallObserverExtension() {
  return function (pi: any) {
    pi.on("before_provider_request", (event: any) => {
      try {
        const scope = currentModelCallScope();
        if (!scope?.callId) return undefined;
        // Output Budget Fact（借鉴 deepseek-harness materialized defaults）：
        // 在最终 body 上解析「输出预算是谁定的」，物化进 attempt 的持久
        // safe_details_json。模型侧由 wrapper 摘取的 modelBudgetMeta 提供
        // 能力切片（composition/声明上限），身份切片提供线协议。
        const budgetModel = {
          ...(scope.model ?? {}),
          ...(scope.modelBudgetMeta ?? {}),
        };
        const outputBudgetFact = resolveOutputBudgetFact(event?.payload, budgetModel);
        const preparedDetails = summarizeProviderRequestPayload(event?.payload);
        createModelCallRecorder({
          observer: getModelCallObserver(),
          context: scope,
        }).providerRequestPrepared({
          details: outputBudgetFact
            ? { ...preparedDetails, outputBudget: outputBudgetFact }
            : preparedDetails,
        });
        // Phase 6（§七十四/§七十五）：event.payload 是 compat 转换后、序列化前的
        // 最终 provider body 活引用（pi-ai 0.84.1 实证，audit §1.1）→
        // fidelity=runtime_exact；hook 不暴露 headers/endpoint（诚实 null）；
        // 凭证不在 payload（vendor SDK fetch 层拼装）。capture 前经统一 Redactor。
        scope.payloadCapture?.captureProviderRequest({
          attemptId: scope.attemptId ?? null,
          protocol: scope.model?.api ?? null,
          transport: { body: event?.payload ?? null },
          fidelity: "runtime_exact",
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
        // Phase 6（§七十八）：hook 只有 status+headers、无 body（audit §1.2）→
        // metadata_only，不推测 body。headers 进 sink 前经统一 Redactor
        // （set-cookie 等凭证键替换）。
        scope.payloadCapture?.captureProviderResponse({
          attemptId: scope.attemptId ?? null,
          status: typeof event?.status === "number" ? event.status : null,
          headers: event?.headers ?? null,
          body: null,
          fidelity: "metadata_only",
        });
      } catch {
        // Observability must never break the model request path.
      }
    });
  };
}
