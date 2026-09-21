import { emitSessionShutdown } from "../lib/pi-sdk/index.ts";

/**
 * 统一释放 session 相关资源。
 *
 * 顺序契约：
 *   1. emit session_shutdown
 *   2. 调用 Hanako 层 unsub
 *   3. 调用 session.dispose()
 *
 * 任一步失败都只 warn，不阻断后续清理；同时把清理错误收集在返回值里
 * （P02-A12：清理失败不抹掉原始错误，也不被静默吞掉——调用方可将返回的
 * errors 并入 AggregateError 上报，未回收资源写明）。
 *
 * @param {object} args
 * @param {object|null} args.session
 * @param {(() => void)|null} [args.unsub]
 * @param {string} args.label
 * @param {(msg: string) => void} [args.warn]
 * @returns {Promise<{ errors: unknown[] }>} 三步中实际抛出的清理错误（已 warn）
 */
export async function teardownSessionResources({ session, unsub, label, warn }) {
  const errors = [];
  try {
    if (session) {
      await emitSessionShutdown(session);
    }
  } catch (err) {
    errors.push(err);
    warn?.(`${label}: emitSessionShutdown failed: ${err.message}`);
  }

  try {
    unsub?.();
  } catch (err) {
    errors.push(err);
    warn?.(`${label}: unsub failed: ${err.message}`);
  }

  try {
    session?.dispose?.();
  } catch (err) {
    errors.push(err);
    warn?.(`${label}: session.dispose failed: ${err.message}`);
  }
  return { errors };
}
