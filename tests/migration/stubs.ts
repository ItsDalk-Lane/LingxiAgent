/**
 * R00-T05 迁移夹具替身（跨语言共享、确定性、零网络）。
 *
 * 边界声明（对应任务书 05_验收与性能协议.md 第 1 层替身表）：
 *  - 模型替身只替代「外部模型流」这一外部协议边界；被测的解析/网关/存储全部为真实旧实现。
 *  - 工具替身只替代「外部工具执行器」边界并记录调用次数与参数；权限准备、digest 校验、
 *    生命周期与取消语义全部走真实 core/tool-invocation-gateway.ts。
 *  - 不使用 vitest mock：替身是纯函数记录器，未来 Rust 侧可按同一契约复刻。
 */

/** 模型替身：固定流片段 + 可控断点。 */
export interface ScriptedModelStub {
  /** 按序返回所有片段（确定性）。 */
  frames(): string[];
  /**
   * 逐帧推送；hangAfterFrame 后的下一帧在 abortSignal abort 前不产生，
   * 用于模拟「流式中途挂起直到取消」的可控断点。
   */
  feed(onFrame: (frame: string) => void, opts?: { hangAfterFrame?: number; abortSignal?: AbortSignal }): Promise<"completed" | "aborted">;
}

export function createScriptedModelStub(frames: string[]): ScriptedModelStub {
  return {
    frames: () => [...frames],
    async feed(onFrame, opts = {}) {
      const hangAfter = opts.hangAfterFrame ?? -1;
      for (let i = 0; i < frames.length; i += 1) {
        if (hangAfter >= 0 && i > hangAfter) {
          const signal = opts.abortSignal;
          if (signal) {
            if (signal.aborted) return "aborted";
            await new Promise<"aborted">((resolve) => {
              signal.addEventListener(
                "abort",
                () => resolve("aborted"),
                { once: true },
              );
            });
            return "aborted";
          }
          await new Promise(() => {});
        }
        onFrame(frames[i]);
      }
      return "completed";
    },
  };
}

/** 工具替身：记录调用次数与参数的执行器，可注入行为。 */
export type RecordingExecutorBehavior =
  | { kind: "ok"; result?: unknown }
  | { kind: "abortError" }
  | { kind: "fail"; message: string }
  | { kind: "abortThenResolve"; controller: AbortController };

export interface RecordingToolStub {
  calls: Array<{ toolCallId: string; args: Record<string, unknown>; ctxInvocationRoute?: unknown; ctxEffectiveTargetId?: unknown }>;
  executeCanonical: (toolCallId: string, args: Record<string, unknown>, signal?: AbortSignal, onUpdate?: unknown, ctx?: any) => Promise<unknown>;
}

export function createRecordingToolStub(behavior: RecordingExecutorBehavior = { kind: "ok" }): RecordingToolStub {
  const stub: RecordingToolStub = {
    calls: [],
    async executeCanonical(toolCallId, args, _signal, _onUpdate, ctx) {
      stub.calls.push({
        toolCallId,
        args,
        ctxInvocationRoute: ctx?.invocationRoute,
        ctxEffectiveTargetId: ctx?.effectiveTargetId,
      });
      if (behavior.kind === "abortError") {
        const error = new Error("The operation was aborted");
        error.name = "AbortError";
        throw error;
      }
      if (behavior.kind === "fail") {
        throw new Error(behavior.message);
      }
      if (behavior.kind === "abortThenResolve") {
        // 模拟「外部成功与本地取消竞争」：AbortSignal 实例本身没有 abort()，
        // 取消必须经由所属 AbortController 发起。
        behavior.controller.abort(new Error("user_abort"));
        return { toolCallId, args, note: "executor finished but signal already aborted" };
      }
      return behavior.result ?? { toolCallId, args, content: [{ type: "text", text: "ok" }] };
    },
  };
  return stub;
}
