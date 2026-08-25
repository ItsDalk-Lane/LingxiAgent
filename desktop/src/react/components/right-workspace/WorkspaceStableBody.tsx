/**
 * WorkspaceStableBody — 工作台稳定主体（无运行期卡片）
 *
 * 从 RightWorkspacePanel 拆出的可组合件：
 * WorkspaceHeader（标题 + 项目技能按钮 + 项目技能面板）+「对话文件 / 工作台」tabs + TabContent。
 * 不含 JianDrawer / JianFloatingToggle（属运行期内容，由 RightWorkspacePanel 在 workspaceCard 内组合），
 * 也不含 SessionTodoCard / TerminalCard 等运行卡。
 *
 * 使用方：
 * - RightWorkspacePanel：作为 .workspaceCard 内的稳定主体（移动端 rail 沿用）。
 * - ChatSidebar 工作台区：桌面端左栏下半区直接内嵌（含 Plugin Widget 短路）。
 */

import type { CSSProperties } from 'react';
import { useStore } from '../../stores';
import type { RightWorkspaceTab } from '../../types';
import { DeskSection } from '../DeskSection';
import { DeskCwdSkillsButton, DeskCwdSkillsPanel } from '../desk/DeskCwdSkills';
import { PluginWidgetView } from '../plugin/PluginWidgetView';
import { SessionRegistryFilesPanel } from './SessionRegistryFilesPanel';
import styles from './RightWorkspacePanel.module.css';
import { workspaceDisplayName } from '../../../../../shared/workspace-history.ts';

interface RightWorkspaceTabDef {
  id: RightWorkspaceTab;
  labelKey: string;
}

const BASE_TABS: RightWorkspaceTabDef[] = [
  { id: 'session-files', labelKey: 'rightWorkspace.tabs.sessionFiles' },
  { id: 'workspace', labelKey: 'rightWorkspace.tabs.workspace' },
];

function TabContent({ activeTab }: { activeTab: RightWorkspaceTab }) {
  if (activeTab === 'session-files') return <SessionRegistryFilesPanel />;
  return <DeskSection framed={false} showHeader={false} rightWorkspaceLayout />;
}

function WorkspaceHeader() {
  const deskBasePath = useStore(s => s.deskBasePath);
  const deskWorkspaceMountId = useStore(s => s.deskWorkspaceMountId);
  const deskWorkspaceLabel = useStore(s => s.deskWorkspaceLabel);
  const selectedFolder = useStore(s => s.selectedFolder);
  const homeFolder = useStore(s => s.homeFolder);
  const t = window.t ?? ((p: string) => p);
  const title = deskWorkspaceMountId
    ? (deskWorkspaceLabel || deskWorkspaceMountId)
    : workspaceDisplayName(deskBasePath || selectedFolder || homeFolder, t('desk.title'));
  const titlePath = deskWorkspaceMountId ? title : (deskBasePath || selectedFolder || homeFolder || undefined);

  return (
    <>
      <div className={styles.workspaceHeader}>
        <div className={styles.workspaceTitle} title={titlePath}>
          {title}
        </div>
        <DeskCwdSkillsButton />
      </div>
      <DeskCwdSkillsPanel />
    </>
  );
}

export function WorkspaceStableBody() {
  const rightWorkspaceTab = useStore(s => s.rightWorkspaceTab);
  const setRightWorkspaceTab = useStore(s => s.setRightWorkspaceTab);
  const jianView = useStore(s => s.jianView);
  const t = window.t ?? ((p: string) => p);

  // Plugin Widget 属于工作台稳定主体：widget 视图时整体短路（与 RightWorkspacePanel 行为一致）
  if (jianView.startsWith('widget:')) {
    return <PluginWidgetView pluginId={jianView.slice(7)} />;
  }

  const activeTab = BASE_TABS.some(tab => tab.id === rightWorkspaceTab)
    ? rightWorkspaceTab
    : 'workspace';
  const activeTabIndex = Math.max(0, BASE_TABS.findIndex(tab => tab.id === activeTab));
  const tabsStyle = {
    '--right-workspace-active-tab-index': `${activeTabIndex}`,
    '--right-workspace-tab-slider-offset': activeTabIndex === 0 ? '0px' : 'calc(100% + 2px)',
  } as CSSProperties;

  return (
    <>
      <WorkspaceHeader />
      <div className={styles.tabs} role="tablist" aria-label={t('rightWorkspace.tabs.label')} style={tabsStyle}>
        <div className={styles.tabSlider} data-right-workspace-tab-slider aria-hidden="true" />
        {BASE_TABS.map(tab => {
          const selected = activeTab === tab.id;
          return (
            <button
              key={tab.id}
              type="button"
              className={`${styles.tab}${selected ? ` ${styles.tabActive}` : ''}`}
              role="tab"
              aria-selected={selected}
              onClick={() => setRightWorkspaceTab(tab.id)}
            >
              {t(tab.labelKey)}
            </button>
          );
        })}
      </div>
      <div className={styles.content} role="tabpanel">
        <TabContent activeTab={activeTab} />
      </div>
    </>
  );
}
