/**
 * WorkspaceStableBody — 工作台稳定主体（无运行期卡片）
 *
 * 从 RightWorkspacePanel 拆出的可组合件：
 * WorkspaceSwitcher（标题即工作台下切菜单）+「对话文件 / 工作台 / 项目技能」tabs + TabContent。
 * 项目技能为第三个 tab（DeskCwdSkillsPanel 内嵌渲染，不再悬浮）。
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
import { DeskCwdSkillsPanel } from '../desk/DeskCwdSkills';
import { PluginWidgetView } from '../plugin/PluginWidgetView';
import { SessionRegistryFilesPanel } from './SessionRegistryFilesPanel';
import { WorkspaceSwitcher } from './WorkspaceSwitcher';
import styles from './RightWorkspacePanel.module.css';

interface RightWorkspaceTabDef {
  id: RightWorkspaceTab;
  labelKey: string;
}

const BASE_TABS: RightWorkspaceTabDef[] = [
  { id: 'session-files', labelKey: 'rightWorkspace.tabs.sessionFiles' },
  { id: 'workspace', labelKey: 'rightWorkspace.tabs.workspace' },
  { id: 'project-skills', labelKey: 'rightWorkspace.tabs.projectSkills' },
];

function TabContent({ activeTab }: { activeTab: RightWorkspaceTab }) {
  if (activeTab === 'session-files') return <SessionRegistryFilesPanel />;
  if (activeTab === 'project-skills') return <DeskCwdSkillsPanel />;
  return <DeskSection framed={false} showHeader={false} rightWorkspaceLayout />;
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
    '--right-workspace-tab-slider-offset': `calc(${activeTabIndex} * (100% + 2px))`,
  } as CSSProperties;

  return (
    <>
      <div className={styles.workspaceHeader}>
        <WorkspaceSwitcher />
      </div>
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
