/**
 * AssistantBlockList - 纯 block 渲染层（任务书 §18）
 *
 * 职责只有：接受 blocks、按 block.id 做 key、调用注册表 renderer。
 * AssistantMessage 与 Process Fold 共享这一层；Process Fold 不再为了
 * 显示几个 block 挂载整条伪造的 AssistantMessage（不变量 6）。
 * 重新 mount（Collapse unmount 后）拥有正确数据与正确 blockId。
 */

import { Component, memo, type ErrorInfo, type ReactNode } from 'react';
import {
  renderRegisteredContentBlock,
} from './block-renderers';
import { KnowledgeRetrievalFold } from './KnowledgeRetrievalFold';
import type { ContentBlock } from '../../stores/chat-types';
import type { KnowledgeRetrievalStats } from '../../../../../shared/knowledge-refs.ts';
import styles from './Chat.module.css';

export interface AssistantBlockListProps {
  blocks: readonly ContentBlock[];
  agentName: string;
  agentId?: string | null;
  yuan: string;
  sessionPath: string;
  messageId: string;
  isStreaming: boolean;
  readOnly: boolean;
  skillPrompt?: string | null;
  /** 配对 user 消息的知识检索统计：存在时在 blocks 最前渲染工具条样式的检索步骤。 */
  knowledgeRetrieval?: KnowledgeRetrievalStats | null;
}

function blockKey(block: ContentBlock, index: number): string {
  return block.id || `block-${index}`;
}

class BlockErrorBoundary extends Component<{
  messageId: string;
  blockType: string;
  blockIdx: number;
  children: ReactNode;
}, { hasError: boolean }> {
  state = { hasError: false };

  static getDerivedStateFromError(): { hasError: boolean } {
    return { hasError: true };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('[AssistantBlockList] content block render failed', {
      messageId: this.props.messageId,
      blockType: this.props.blockType,
      blockIdx: this.props.blockIdx,
      componentStack: info.componentStack,
    }, error);
  }

  componentDidUpdate(prevProps: Readonly<{ messageId: string; blockType: string; blockIdx: number; children: ReactNode }>) {
    if (!this.state.hasError) return;
    if (
      prevProps.messageId !== this.props.messageId
      || prevProps.blockIdx !== this.props.blockIdx
      || prevProps.blockType !== this.props.blockType
    ) {
      this.setState({ hasError: false });
    }
  }

  render() {
    if (this.state.hasError) return null;
    return this.props.children;
  }
}

export const AssistantBlockList = memo(function AssistantBlockList({
  blocks,
  agentName,
  agentId,
  yuan,
  sessionPath,
  messageId,
  isStreaming,
  readOnly,
  skillPrompt = null,
  knowledgeRetrieval = null,
}: AssistantBlockListProps) {
  return (
    <div className={`${styles.message} ${styles.messageAssistant} ${styles.processFoldPanelBlocks}`}>
      {knowledgeRetrieval && <KnowledgeRetrievalFold retrieval={knowledgeRetrieval} />}
      {blocks.map((block, index) => (
        <BlockErrorBoundary
          key={blockKey(block, index)}
          messageId={messageId}
          blockType={block.type}
          blockIdx={index}
        >
          {renderRegisteredContentBlock(block, {
            agentName,
            agentId,
            yuan,
            sessionPath,
            messageId,
            blockIdx: index,
            isStreaming,
            readOnly,
            skillPrompt,
            knowledgeResearch: knowledgeRetrieval?.research,
          })}
        </BlockErrorBoundary>
      ))}
    </div>
  );
});
