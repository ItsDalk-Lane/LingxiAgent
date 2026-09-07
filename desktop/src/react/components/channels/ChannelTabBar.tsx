/**
 * ChannelTabBar — fixed top tab bar (chat / knowledge / channels)
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { useStore } from '../../stores';
import type { TabType } from '../../types';
import { toggleSidebar } from '../SidebarLayout';
import { hydrateCurrentChannelIfNeeded } from '../../stores/channel-actions';
import styles from './Channels.module.css';

declare function t(key: string, vars?: Record<string, string | number>): string;

// ── Tab switching logic ──

export function switchTab(tab: TabType) {
  const s = useStore.getState();
  if (tab === s.currentTab) return;

  if (tab === 'channels') {
    s.setActivePanel(null);
  }

  s.setCurrentTab(tab);
  if (tab === 'channels') {
    hydrateCurrentChannelIfNeeded().catch((err: unknown) =>
      console.warn('[channels] hydrate current channel failed', err));
  }
  localStorage.setItem('hana-tab', tab);

  const savedLeft = localStorage.getItem(`hana-sidebar-${tab}`);
  const wantLeftOpen = savedLeft !== 'closed';
  if (s.sidebarOpen !== wantLeftOpen) toggleSidebar(wantLeftOpen);
}

// ── Tab list ──

const FIXED_TABS: TabType[] = ['chat', 'knowledge', 'channels'];

function getTabLabel(tab: TabType): string {
  if (tab === 'chat') return t('channel.chatTab');
  if (tab === 'knowledge') return t('knowledge.tab');
  if (tab === 'channels') return t('channel.tab');
  return tab;
}

// ── Component ──

export function ChannelTabBar() {
  const currentTab = useStore(s => s.currentTab);
  const channelTotalUnread = useStore(s => s.channelTotalUnread);
  const locale = useStore(s => s.locale);

  const tabsRef = useRef<HTMLDivElement>(null);
  const sliderRef = useRef<HTMLDivElement>(null);
  const btnRefs = useRef<Map<string, HTMLButtonElement>>(new Map());

  const setBtnRef = useCallback((tab: TabType, el: HTMLButtonElement | null) => {
    if (el) btnRefs.current.set(tab, el);
    else btnRefs.current.delete(tab);
  }, []);

  const moveSlider = useCallback((tab: TabType, animate: boolean) => {
    const container = tabsRef.current;
    const slider = sliderRef.current;
    const target = btnRefs.current.get(tab);
    if (!slider || !container) return;
    if (!target) {
      slider.style.width = '0px';
      slider.style.transform = 'translateX(0px)';
      return;
    }
    const parentRect = container.getBoundingClientRect();
    const targetRect = target.getBoundingClientRect();
    const offsetX = targetRect.left - parentRect.left;
    if (!animate) slider.style.transition = 'none';
    slider.style.width = targetRect.width + 'px';
    slider.style.transform = `translateX(${offsetX - 2}px)`;
    if (!animate) requestAnimationFrame(() => { slider.style.transition = ''; });
  }, []);

  useEffect(() => { moveSlider(currentTab, true); }, [currentTab, moveSlider]);
  useEffect(() => { moveSlider(useStore.getState().currentTab || 'chat', false); }, [locale, moveSlider]);
  // Initial position after mount
  useEffect(() => {
    requestAnimationFrame(() => moveSlider(useStore.getState().currentTab || 'chat', false));
  }, [moveSlider]);

  // Restore saved tab on mount
  useEffect(() => {
    const savedTab = localStorage.getItem('hana-tab');
    if (savedTab && savedTab !== 'chat') switchTab(savedTab as TabType);
  }, []);

  const handleTabClick = useCallback((tab: TabType) => {
    switchTab(tab);
  }, []);

  return (
    <div className={styles.tbTabs} ref={tabsRef}>
      <div className={styles.tbTabsSlider} ref={sliderRef}></div>
      {FIXED_TABS.map(tab => {
        const isActive = currentTab === tab;
        let cls = styles.tbTab;
        if (isActive) cls += ` ${styles.tbTabActive}`;

        return (
          <button
            key={tab}
            ref={(el) => setBtnRef(tab, el)}
            className={cls}
            data-tab={tab}
            onClick={() => handleTabClick(tab)}
          >
            {getTabLabel(tab)}
            {tab === 'channels' && channelTotalUnread > 0 && <span className={styles.tbTabBadge} />}
          </button>
        );
      })}
    </div>
  );
}
