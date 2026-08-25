import { useCallback } from 'react';
import { useStore } from '../../stores';
import { JianEditor } from '../desk/DeskEditor';
import { PluginWidgetView } from '../plugin/PluginWidgetView';
import { WorkspaceStableBody } from './WorkspaceStableBody';
import { SessionTodoCard } from './SessionTodoCard';
import { TerminalCard } from './TerminalCard';
import { WorkflowCard } from './WorkflowCard';
import { AgentActivityCard } from './AgentActivityCard';
import { SessionStatusCard } from './SessionStatusCard';
import styles from './RightWorkspacePanel.module.css';

function Chevron({ open }: { open: boolean }) {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {open ? <polyline points="6 9 12 15 18 9" /> : <polyline points="18 15 12 9 6 15" />}
    </svg>
  );
}

function JianDrawer() {
  const open = useStore(s => s.jianDrawerOpen);
  const t = window.t ?? ((p: string) => p);
  const label = t('desk.jianLabel');

  return (
    <section className={styles.jianDrawer} data-open={open ? 'true' : 'false'} role="region" aria-label={label}>
      <div className={styles.jianHeader}>
        <span className={styles.jianTitle}>{label}</span>
      </div>
      <div className={styles.jianBody}>
        <JianEditor showHeader={false} />
      </div>
    </section>
  );
}

function JianFloatingToggle() {
  const open = useStore(s => s.jianDrawerOpen);
  const setOpen = useStore(s => s.setJianDrawerOpen);
  const t = window.t ?? ((p: string) => p);
  const actionLabel = open ? t('rightWorkspace.jian.collapse') : t('rightWorkspace.jian.expand');
  const toggle = useCallback(() => setOpen(!open), [open, setOpen]);

  return (
    <button
      className={styles.jianToggle}
      type="button"
      aria-label={actionLabel}
      aria-expanded={open}
      onClick={toggle}
    >
      <Chevron open={open} />
    </button>
  );
}

export function RightWorkspacePanel({ compact = false }: { compact?: boolean }) {
  const jianView = useStore(s => s.jianView);
  const jianDrawerOpen = useStore(s => s.jianDrawerOpen);

  if (jianView.startsWith('widget:')) {
    return (
      <div className={styles.shell}>
        <PluginWidgetView pluginId={jianView.slice(7)} />
      </div>
    );
  }

  return (
    <div className={styles.shell}>
      {!compact && <SessionTodoCard />}
      <div
        className={`universal-card ${styles.workspaceCard}`}
        data-right-workspace-card=""
        data-jian-open={jianDrawerOpen ? 'true' : 'false'}
      >
        <WorkspaceStableBody />
        <JianDrawer />
        <JianFloatingToggle />
      </div>
      {!compact && (
        <>
          <TerminalCard />
          <WorkflowCard />
          <AgentActivityCard />
          <SessionStatusCard />
        </>
      )}
    </div>
  );
}
