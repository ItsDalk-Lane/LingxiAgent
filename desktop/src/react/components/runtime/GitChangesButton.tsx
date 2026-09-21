/**
 * GitChangesButton — 工作台首行「变更 +n -m」按钮
 *
 * 源代码管理的入口（原运行信息弹窗·环境信息卡·Git图谱行迁移至此）：
 *   - 非 Git 仓库 / 探测中 / 探测失败：不渲染（探测完成是 git 仓库才出现）
 *   - 标题纯文字无图标：「变更 +增 -删」（千分位，+绿 -红）；无变更时是「变更 +0 -0」
 *   - 点击 → 源代码管理面板（GitGraphPanel，提交或推送与变更文件合并后的单一界面）
 * 数据走 useGitEnv 共享层：与输入框分支下拉同目录只探测一次，面板操作后刷新同一份缓存。
 */
import { useState } from 'react';
import { useStore } from '../../stores';
import { useGitEnv } from '../../hooks/use-git-env';
import { GitGraphPanel } from './GitGraphPanel';
import styles from './GitChangesButton.module.css';

function fmt(n: number): string {
  return n.toLocaleString('en-US');
}

export function GitChangesButton() {
  const t = window.t ?? ((p: string) => p);
  const dir = useStore(s => s.deskWorkspaceNativeRoot || s.deskBasePath);
  const sessionPath = useStore(s => s.currentSessionPath);
  const agentId = useStore(s => s.currentAgentId);
  const { status, branches, refresh } = useGitEnv(dir, agentId);
  const [graphOpen, setGraphOpen] = useState(false);

  // 探测完成且是 Git 仓库才渲染：loading / 非 git / 失败都不占工作台工具行的位置
  if (!status?.isRepo) return null;

  return (
    <>
      <button
        type="button"
        className={styles.button}
        data-testid="desk-git-changes-btn"
        aria-label={`${t('gitEnv.changes')} +${fmt(status.total.additions)} -${fmt(status.total.deletions)}`}
        onClick={() => setGraphOpen(true)}
      >
        <span className={styles.label}>{t('gitEnv.changes')}</span>
        <span className={styles.added}>+{fmt(status.total.additions)}</span>
        <span className={styles.deleted}>-{fmt(status.total.deletions)}</span>
      </button>

      <GitGraphPanel
        open={graphOpen}
        onClose={() => setGraphOpen(false)}
        dir={dir!}
        status={status}
        branches={branches}
        sessionPath={sessionPath}
        agentId={agentId}
        refresh={refresh}
      />
    </>
  );
}
