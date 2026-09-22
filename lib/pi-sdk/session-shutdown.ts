/** Pi 生命周期真实适配；公共 index 与清理入口共同使用。 */
export interface ShutdownSession {
  extensionRunner?: {
    hasHandlers?(event: 'session_shutdown'): boolean;
    emit(event: { type: 'session_shutdown'; reason: 'quit' }): unknown;
  } | null;
  dispose?(): void;
}

/**
 * Emit `session_shutdown` event to the session's extension runner.
 *
 * 为什么在 adapter 层实现而不从 SDK 导出:
 *   SDK 的 emitSessionShutdownEvent 辅助函数只在 core/extensions/runner.js
 *   内部暴露, 顶级 index.js 未 re-export。直接 import 深层路径会违反
 *   adapter 纪律。实现本身仅 7 行, 自己实现更干净。
 *
 * 契约: AgentSession.dispose() 本身不 emit shutdown, 调用方必须在
 *   dispose 前显式 emit, 否则监听 session_shutdown 的扩展(如
 *   deferred-result-ext) 无法清理自身的 setInterval 和 store 订阅,
 *   导致长期运行进程的内存泄漏。
 *
 * @param {object} session - AgentSession 实例
 * @returns {Promise<boolean>} 事件是否被 emit (false = 无 handler)
 */
export async function emitSessionShutdown(session: ShutdownSession | null | undefined): Promise<boolean> {
  const runner = session?.extensionRunner;
  if (runner?.hasHandlers?.("session_shutdown")) {
    await runner.emit({ type: "session_shutdown", reason: "quit" });
    return true;
  }
  return false;
}

