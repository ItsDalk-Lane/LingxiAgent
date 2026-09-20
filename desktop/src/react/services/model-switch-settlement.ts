/**
 * 换模型完成后的残留清账（修复：换模型后第一轮思考/工具不进折叠卡）。
 *
 * 为什么在这个时刻清账是安全的：换模型在流式进行中会被服务端拒绝
 * （MODEL_SWITCH_CONFLICT 409），前端模型选择器在 isStreaming 时也不允许
 * 打开，所以「切换成功」必然落在两轮之间的空档——此刻不存在任何合法的
 * 「进行中」轮次，清账零误伤。
 *
 * 清的是四本「进行中」账本。换模型会重签上下文缓存契约、重置思考档位、
 * 在会话文件追加 model_change 记录，轮次身份连续性在此被扰动；若上一轮的
 * 收口信号被身份门禁拒收（identitiesMatch / runKey 去重），残留会让换模型
 * 后的第一轮停留在 live 模式——思考/工具直接暴露在正文，直到下一轮对话
 * 出现新助手消息才因「最新消息锚点」脱离而自愈。在空档把账归零，第一轮
 * 从干净的地基起步，不再依赖迟到信号的补救。
 */
import { useStore } from '../stores';
import { clearLiveTurnStore } from '../stores/live-turn-store';
import { streamBufferManager } from '../hooks/use-stream-buffer';

export function settleModelSwitchResidue(sessionPath: string | null | undefined): void {
  const path = typeof sessionPath === 'string' && sessionPath.trim() ? sessionPath.trim() : null;
  if (!path) return;
  // 账本 1：会话忙碌标志与登记身份（status(false) 被 identitiesMatch 拒收时的残留）
  useStore.getState().forceRemoveStreamingSession(path);
  // 账本 2：live 快照（assistant_run_end 未走到 commit 时的残留，按会话前缀全清）
  clearLiveTurnStore(path);
  // 账本 3：stream buffer 的 Run 运行态（runActive / activeRunKey / lastFinalizedRunKey）
  streamBufferManager.clear(path);
  // 账本 4：已写进消息的投影 streaming 残留（唯一落库的一本，走统一投影路径结算）
  useStore.getState().settleStreamingProjection(path);
}
