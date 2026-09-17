import { Overlay } from '../../ui';
import { KnowledgePage } from './KnowledgePage';
import styles from './KnowledgeModal.module.css';

interface Props {
  open: boolean;
  onClose: () => void;
  zIndex?: number;
}

/**
 * 知识库弹窗：原顶部「知识」页签的整页界面，改为侧栏功能行的弹窗入口
 * （与「已归档的聊天」弹窗同模式）。Escape / 点击遮罩 / 页头 × 均可关闭。
 * 不在弹窗角落放悬浮 ×：右上角与应用的「运行信息」悬浮件重叠，会被盖住点不到。
 */
export function KnowledgeModal({ open, onClose, zIndex = 1000 }: Props) {
  return (
    <Overlay
      scope="inline"
      open={open}
      onClose={onClose}
      backdrop="blur"
      zIndex={zIndex}
      className={styles.modal}
      disableContainerAnimation
      contentProps={{ 'data-testid': 'knowledge-modal' }}
    >
      <KnowledgePage onClose={onClose} />
    </Overlay>
  );
}
