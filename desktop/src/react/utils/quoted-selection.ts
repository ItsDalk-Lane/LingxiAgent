import type { QuotedSelection } from '../stores/input-slice';

export const QUOTE_ORIGINAL_START = '[引用原文]';
export const QUOTE_ORIGINAL_END = '[/引用原文]';

export function formatQuotedSelectionForPrompt(sel: QuotedSelection): string {
  if (sel.sourceFilePath && sel.lineStart != null && sel.lineEnd != null) {
    return [
      `[引用片段] ${sel.sourceTitle}（第${sel.lineStart}-${sel.lineEnd}行，共${sel.charCount}字）路径: ${sel.sourceFilePath}`,
      QUOTE_ORIGINAL_START,
      sel.text,
      QUOTE_ORIGINAL_END,
    ].join('\n');
  }
  // 聊天引用：标注片段出自谁的发言。目标会话可能没有原对话上下文（典型：
  // 侧边聊天引用主对话内容）；来源会话的引用由发送链路自动附带
  //（composer-send 的 withQuoteOriginSessionRefs），模型需要时可读取原对话补全语境。
  // 无角色信息（旧数据）保持旧单行格式，与历史解析兼容。
  const roleLabel = sel.sourceRole === 'assistant'
    ? '助手'
    : sel.sourceRole === 'user'
      ? '用户'
      : null;
  if (!roleLabel) return `[引用片段] ${sel.text}`;
  return [
    `[引用片段] 来自对话中的${roleLabel}消息`,
    QUOTE_ORIGINAL_START,
    sel.text,
    QUOTE_ORIGINAL_END,
  ].join('\n');
}
