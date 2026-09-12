/**
 * attach-workbench-item.ts — 把工作台文件树条目附加到聊天输入框
 *
 * 语义与「把工作台条目拖进聊天框」一致：按原始绝对路径就地附加，不走
 * /api/upload 复制，工作台里的文件和对话里的附件仍是同一个文件。
 * 没有 native 路径的工作台（远端 / 虚拟 mount）拿不到可附加的路径，
 * 调用方应据此不渲染入口。
 */

import { useStore } from '../stores';

declare function t(key: string, vars?: Record<string, string | number>): string;

/**
 * 附件只能进聊天输入框。非聊天页签返回 true，并给出与该页签一致的提示；
 * 拖拽附件与工作台 @ 按钮共用这一判据。
 */
export function blockChatAttachmentOutsideChatTab(): boolean {
  const state = useStore.getState();
  if (state.currentTab === 'chat') return false;
  state.addToast(
    state.currentTab === 'knowledge' ? t('knowledge.useImportButton') : t('channel.filesUnsupported'),
    'error',
  );
  return true;
}

export interface WorkbenchChatAttachment {
  /** 工作台条目的 native 绝对路径 */
  path: string;
  name: string;
  isDirectory?: boolean;
}

/**
 * 把一个工作台条目附加到聊天输入框。
 *
 * 返回是否真的加入：页签不是聊天、路径为空或已达 9 个附件上限时返回 false
 * （上限行为与其它附加入口一致，静默不重复提示）。
 */
export function attachWorkbenchItemToInput(item: WorkbenchChatAttachment): boolean {
  if (!item.path) return false;
  if (blockChatAttachmentOutsideChatTab()) return false;
  if (useStore.getState().attachedFiles.length >= 9) return false;
  useStore.getState().addAttachedFile({
    path: item.path,
    name: item.name,
    isDirectory: !!item.isDirectory,
  });
  return true;
}
