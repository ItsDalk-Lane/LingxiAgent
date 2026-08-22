/**
 * Pi 0.84.1 transport retry 可见性实证（任务书 Step 6 / Scenario H）。
 *
 * 实证对象：pi-ai `retryProviderRequest`（dist/utils/provider-retry.js）。
 * 它是 streamFn 内部对 SDK 请求的包装：408/409/429/5xx/网络错误按指数退避
 * 重试，**整个循环没有任何 hook/事件/回调暴露给外层**。
 *
 * 本测试要证明的不是「retry 存在」（源码已证），而是：
 *   1. retry 循环确实发出多个真实网络 attempt（request 被调用多次）；
 *   2. 这多个 attempt 对 ModelCallObserver 完全不可见——observer 只看到
 *      1 个 logical call + 1 个 attempt，且该 attempt 明确标记
 *      attemptVisibility: "logical_boundary"；
 *   3. 我们**不伪造** attemptId A1/A2（§三十一/§四十）；
 *   4. retry 策略本身不变：错误类型、耗尽后抛出行为与未包装一致。
 *
 * 备注：`retry-after-ms: 1` 响应头把退避压到 1ms，测试不等真实指数退避。
 */
import { describe, it, expect, afterEach, vi } from "vitest";
// 深路径引入被测实现本身——这就是 runtime 实证，不是复述源码结论。
// pi-ai 的 exports map 不公开该模块，测试按文件路径直引（仅测试用）。
import { retryProviderRequest } from "../node_modules/@earendil-works/pi-ai/dist/utils/provider-retry.js";
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import { installModelCallStreamObserver } from "../lib/pi-sdk/model-call-stream-observer.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import { createTestModelCallObserver } from "../lib/llm/model-call-observer-testing.ts";

function providerError(status: number) {
  return Object.assign(new Error(`HTTP ${status}`), {
    status,
    headers: new Headers({ "retry-after-ms": "1" }),
  });
}

afterEach(() => {
  setModelCallObserver(null);
  vi.restoreAllMocks();
});

describe("pi-ai retryProviderRequest（0.84.1 实证）", () => {
  it("429 → 429 → success：request 被真实调用 3 次后成功", async () => {
    const request = vi.fn()
      .mockRejectedValueOnce(providerError(429))
      .mockRejectedValueOnce(providerError(429))
      .mockResolvedValueOnce("ok");

    const result = await retryProviderRequest(request, { maxRetries: 3 });
    expect(result).toBe("ok");
    expect(request).toHaveBeenCalledTimes(3);
  });

  it("重试耗尽后原错误照常抛出（策略不变）", async () => {
    const error = providerError(500);
    const request = vi.fn().mockRejectedValue(error);
    await expect(retryProviderRequest(request, { maxRetries: 2 })).rejects.toBe(error);
    expect(request).toHaveBeenCalledTimes(3); // 1 + 2 retries
  });

  it("非 retryable 错误（400）不重试", async () => {
    const error = providerError(400);
    const request = vi.fn().mockRejectedValue(error);
    await expect(retryProviderRequest(request, { maxRetries: 3 })).rejects.toBe(error);
    expect(request).toHaveBeenCalledTimes(1);
  });
});

describe("transport retry 对 Observer 的可见性（Scenario H）", () => {
  it("SDK 内部 3 次网络 attempt 被折叠成 1 个 logical call + 1 个标记 attempt", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);

    const networkAttempts = vi.fn()
      .mockRejectedValueOnce(providerError(429))
      .mockRejectedValueOnce(providerError(500))
      .mockResolvedValueOnce("response-body");

    // 模拟 pi-ai streamSimple 内部结构：streamFn 被调用一次，内部经
    // retryProviderRequest 发出多个真实网络 attempt。
    const innerStreamFn = async (..._args: unknown[]) => {
      const body = await retryProviderRequest(networkAttempts, { maxRetries: 3 });
      expect(body).toBe("response-body");
      const stream = createAssistantMessageEventStream();
      queueMicrotask(() => {
        stream.push({
          type: "done",
          reason: "stop",
          message: {
            role: "assistant",
            content: [{ type: "text", text: "ok" }],
            api: "openai-completions",
            provider: "test",
            model: "m",
            stopReason: "stop",
            usage: { input: 1, output: 1, cacheRead: 0, cacheWrite: 0, totalTokens: 2 },
            timestamp: Date.now(),
          },
        } as any);
        stream.end();
      });
      return stream;
    };

    const session = {
      agent: { streamFunction: innerStreamFn },
      sessionManager: { getSessionId: () => "s", getSessionFile: () => "/tmp/s.jsonl" },
      isCompacting: false,
    };
    installModelCallStreamObserver(session);

    const stream = await session.agent.streamFunction(
      { id: "m", provider: "test", api: "openai-completions" },
      { messages: [] },
      {},
    );
    await stream.result();
    await new Promise((resolve) => setImmediate(resolve));
    await new Promise((resolve) => setImmediate(resolve));

    // 底层事实：3 次真实网络 attempt
    expect(networkAttempts).toHaveBeenCalledTimes(3);
    // Observer 事实：只有 1 个 logical call + 1 个 attempt，且诚实标记可见度
    expect(observer.eventsOfType("logical_call_start")).toHaveLength(1);
    expect(observer.eventsOfType("attempt_start")).toHaveLength(1);
    expect(observer.attemptIds()).toHaveLength(1);
    expect(observer.eventsOfType("attempt_start")[0].details).toMatchObject({
      attemptVisibility: "logical_boundary",
    });
    // 终态仍然正确（SDK 折叠后成功 → ok）
    expect(observer.eventsOfType("logical_call_end")[0].status).toBe("ok");
    // 不伪造：没有 attempt_error 事件（内部 429/500 对 observer 不可见）
    expect(observer.eventsOfType("attempt_error")).toHaveLength(0);
  });
});
