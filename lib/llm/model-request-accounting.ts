/**
 * 把不经过文字调用客户端的模型请求接入统一用量账本。
 *
 * Adapter 只负责协议和网络；这里统一负责开始、成功和失败三种结果，且记录中
 * 只接收调用归属与模型标识，不接收 Key、Header、提示词或媒体内容。
 */
export async function withModelRequestAccounting({
  usageLedger = null,
  model = null,
  usageContext = null,
  metadata = null,
}: any = {}, run: () => any) {
  if (typeof run !== "function") throw new TypeError("model request runner is required");

  const request = usageLedger?.start?.({
    model,
    usageContext,
    metadata,
  }) || null;

  try {
    const result = await run();
    if (request?.requestId && result?.ok === false) {
      const responseStatus = Number.isFinite(Number(result?.status))
        ? String(Number(result.status))
        : "unknown";
      const responseError = new Error(`model request returned non-success status ${responseStatus}`);
      usageLedger?.recordError?.(request.requestId, responseError, "error", {
        usage: result?.usage ?? null,
        model,
      });
      return result;
    }
    usageLedger?.finish?.(request?.requestId, {
      usage: result?.usage ?? null,
      model,
    });
    return result;
  } catch (error: any) {
    if (request?.requestId) {
      const status = error?.name === "AbortError" || error?.type === "aborted"
        ? "aborted"
        : "error";
      usageLedger?.recordError?.(request.requestId, error, status, {
        usage: error?.usage ?? null,
        model,
      });
    }
    throw error;
  }
}
