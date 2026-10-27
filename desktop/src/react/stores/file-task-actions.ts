/**
 * 文件任务入口的轻量桥：Welcome 页的固定入口不直接依赖 InputArea/MainContent
 *（避免 Welcome → MainContent 的循环 import），由真正持有附件处理函数的
 * InputArea 注册；Welcome 只负责触发。
 */

export type FileTaskHandler = () => Promise<void> | void;

let registeredHandler: FileTaskHandler | null = null;

/** InputArea 挂载时注册；返回卸载函数，避免热重载/切换后指向旧闭包。 */
export function registerFileTaskHandler(handler: FileTaskHandler): () => void {
  registeredHandler = handler;
  return () => {
    if (registeredHandler === handler) registeredHandler = null;
  };
}

/**
 * 固定“文件任务”入口：优先走当前输入区的附件选择流程（与回形针按钮同一路径）。
 * 没有输入区时返回 false，由调用方决定是否提示；绝不静默伪造已打开。
 */
export async function openFileTaskEntry(): Promise<boolean> {
  if (!registeredHandler) return false;
  await registeredHandler();
  return true;
}
