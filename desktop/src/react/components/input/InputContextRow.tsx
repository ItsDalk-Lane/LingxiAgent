import { memo } from 'react';
import { AttachedFilesBar } from './AttachedFilesBar';
import { QuotedSelectionCard } from './QuotedSelectionCard';
import { KnowledgeReferenceBar } from './KnowledgeReferenceBar';
import { useStore } from '../../stores';
import { selectKnowledgeRefsForSession } from '../../stores/knowledge-reference-slice';
import type { AttachedFile } from '../../stores/input-slice';
import styles from './InputArea.module.css';

interface Props {
  attachedFiles: AttachedFile[];
  removeAttachedFile: (index: number) => void;
  hasQuotedSelection: boolean;
  /** 知识库引用条的会话键（sessionPath；pending 新会话为 HOME_DRAFT_KEY；null 不渲染引用条） */
  knowledgeRefSessionKey?: string | null;
}

/** 输入框上方的上下文行：附件、引用、知识库引用条 */
export const InputContextRow = memo(function InputContextRow({
  attachedFiles, removeAttachedFile, hasQuotedSelection, knowledgeRefSessionKey = null,
}: Props) {
  // 引用条自身也会按引用为空返回 null；这里先订阅一次避免渲染空行壳。
  const hasKnowledgeRefs = useStore(s => {
    const refs = selectKnowledgeRefsForSession(s, knowledgeRefSessionKey);
    return !!refs && refs.notebookIds.length > 0;
  });
  if (attachedFiles.length === 0 && !hasQuotedSelection && !hasKnowledgeRefs) return null;

  return (
    <div className={styles['input-context-row']}>
      <div className={styles['input-context-left']}>
        {attachedFiles.length > 0 && <AttachedFilesBar files={attachedFiles} onRemove={removeAttachedFile} />}
        <KnowledgeReferenceBar sessionKey={knowledgeRefSessionKey} />
        <QuotedSelectionCard />
      </div>
    </div>
  );
});
