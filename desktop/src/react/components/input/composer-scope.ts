import { useMemo } from 'react';
import { useStore } from '../../stores';
import { sessionScopedValue } from '../../stores/session-slice';
import type { AttachedFile, QuotedSelection } from '../../stores/input-slice';

/**
 * composer-scope.ts — 输入区里「按会话分桶」的读取钩子。
 *
 * 附件、已提交引用、文档上下文开关都是会话级状态。主聊天页与侧边对话面板会
 * 同时挂载，因此必须各读各的；同时保留既有语义：主聊天页（含「新建聊天」草稿
 * 态，此时还没有 sessionPath）读全局兼容字段。
 *
 * 判定规则只在这一处：
 * - sessionPath 有值 → 该会话的桶。主会话的桶与全局镜像由写入方同步维护，
 *   两者等价。
 * - sessionPath 为 null 且 isScoped=false（主聊天页草稿态）→ 全局字段。
 * - sessionPath 为 null 且 isScoped=true（侧边面板会话尚未创建完成）→ 空值：
 *   绝不回落主会话，否则侧边面板会显示、甚至发出主会话的附件/引用。
 * 此外，侧边面板的会话若恰好等于当前会话（用户把侧边会话切成了主会话），
 * 也统一走全局镜像，避免同一份状态出现两种读数。
 */

const EMPTY_ATTACHMENTS: AttachedFile[] = [];
const EMPTY_QUOTES: QuotedSelection[] = [];

function useScopeRead<T>(
  scopeSessionPath: string | null | undefined,
  isScoped: boolean,
  primaryValue: T,
  scopedValue: T,
): T {
  const isPrimarySession = useStore(s => !!scopeSessionPath && s.currentSessionPath === scopeSessionPath);
  return useMemo(() => {
    if (scopeSessionPath) return isPrimarySession ? primaryValue : scopedValue;
    return isScoped ? scopedValue : primaryValue;
  }, [isPrimarySession, isScoped, primaryValue, scopeSessionPath, scopedValue]);
}

export function useScopedAttachedFiles(
  scopeSessionPath: string | null | undefined,
  isScoped = false,
): AttachedFile[] {
  const scopedValue = useStore(s => (scopeSessionPath
    ? sessionScopedValue(s as never, s.attachedFilesBySession, scopeSessionPath) ?? EMPTY_ATTACHMENTS
    : EMPTY_ATTACHMENTS));
  const primaryValue = useStore(s => s.attachedFiles);
  return useScopeRead(scopeSessionPath, isScoped, primaryValue, scopedValue);
}

export function useScopedQuotedSelections(
  scopeSessionPath: string | null | undefined,
  isScoped = false,
): QuotedSelection[] {
  const scopedValue = useStore(s => (scopeSessionPath
    ? sessionScopedValue(s as never, s.quotedSelectionsBySession, scopeSessionPath) ?? EMPTY_QUOTES
    : EMPTY_QUOTES));
  const primaryValue = useStore(s => s.quotedSelections);
  return useScopeRead(scopeSessionPath, isScoped, primaryValue, scopedValue);
}

export function useScopedDocContext(
  scopeSessionPath: string | null | undefined,
  isScoped = false,
): boolean {
  const scopedValue = useStore(s => (scopeSessionPath
    ? sessionScopedValue(s as never, s.docContextAttachedBySession, scopeSessionPath) ?? false
    : false));
  const primaryValue = useStore(s => s.docContextAttached);
  return useScopeRead(scopeSessionPath, isScoped, primaryValue, scopedValue);
}
