import React, { useEffect, useState } from 'react';
import { t, autoSaveConfig } from '../../helpers';
import { lingxiFetch } from '../../api';
import { Toggle } from '@/ui';
import { loadSettingsConfig } from '../../actions';
import { useSettingsStore } from '../../store';
import { SettingsSection } from '../../components/SettingsSection';
import styles from '../../Settings.module.css';

interface SkillInstallConfig {
  enabled?: boolean;
  allow_github_fetch?: boolean;
  safety_review?: boolean;
}

interface SkillCapabilitiesProps {
  installCfg: SkillInstallConfig | undefined;
}

export function SkillCapabilities({ installCfg }: SkillCapabilitiesProps) {
  const installEnabled = installCfg ? installCfg.enabled === true : undefined;
  const githubEnabled = installCfg ? installCfg.allow_github_fetch === true : undefined;
  const safetyReviewEnabled = installCfg ? installCfg.safety_review !== false : undefined;

  const [showGithubWarning, setShowGithubWarning] = useState(false);
  const [showSafetyWarning, setShowSafetyWarning] = useState(false);

  // 踩坑自动沉淀 / goal 预算是全局 preferences（不分 agent），走各自的
  // /api/preferences/* 路由，与本区 per-agent capabilities 的 autoSaveConfig 通道不是同一条。
  const showToast = useSettingsStore(s => s.showToast);
  const [autolearnEnabled, setAutolearnEnabled] = useState<boolean | undefined>(undefined);
  const [goalEnabled, setGoalEnabled] = useState<boolean | undefined>(undefined);
  useEffect(() => {
    let alive = true;
    lingxiFetch('/api/preferences/goal')
      .then(res => res.json())
      .then((data) => {
        if (alive) setGoalEnabled(data?.goal?.enabled !== false);
      })
      .catch(() => {});
    return () => { alive = false; };
  }, []);
  const handleGoalToggle = async (on: boolean) => {
    const previous = goalEnabled;
    setGoalEnabled(on);
    try {
      const res = await lingxiFetch('/api/preferences/goal', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ goal: { enabled: on } }),
      });
      const data = await res.json();
      if (data?.error) throw new Error(data.error);
      setGoalEnabled(data?.goal?.enabled !== false);
    } catch (err: any) {
      setGoalEnabled(previous);
      showToast(t('settings.saveFailed') + ': ' + (err?.message || String(err)), 'error');
    }
  };
  useEffect(() => {
    let alive = true;
    lingxiFetch('/api/preferences/autolearn')
      .then(res => res.json())
      .then((data) => {
        if (alive) setAutolearnEnabled(data?.autolearn?.enabled !== false);
      })
      .catch(() => { /* 读取失败保持 undefined（Toggle 显示未加载态），不打扰 */ });
    return () => { alive = false; };
  }, []);
  const handleAutolearnToggle = async (on: boolean) => {
    const previous = autolearnEnabled;
    setAutolearnEnabled(on);
    try {
      const res = await lingxiFetch('/api/preferences/autolearn', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ autolearn: { enabled: on } }),
      });
      const data = await res.json();
      if (data?.error) throw new Error(data.error);
      setAutolearnEnabled(data?.autolearn?.enabled !== false);
    } catch (err: any) {
      setAutolearnEnabled(previous);
      showToast(t('settings.saveFailed') + ': ' + (err?.message || String(err)), 'error');
    }
  };

  const handleGithubToggle = async (on: boolean) => {
    if (on) {
      setShowGithubWarning(true);
    } else {
      await autoSaveConfig(
        { capabilities: { learn_skills: { allow_github_fetch: false } } },
        { silent: true },
      );
      await loadSettingsConfig();
    }
  };

  const confirmGithubFetch = async () => {
    setShowGithubWarning(false);
    await autoSaveConfig(
      { capabilities: { learn_skills: { allow_github_fetch: true } } },
      { silent: true },
    );
    await loadSettingsConfig();
  };

  return (
    <>
      <SettingsSection title={t('settings.toolCaps.title')}>
        <div className={styles['capability-row']}>
          <div className={styles['capability-row-label']}>
            <span className={styles['capability-row-name']}>{t('settings.skills.learnCreate')}</span>
            <span className={styles['capability-row-desc']}>{t('settings.skills.learnCreateDesc')}</span>
          </div>
          <Toggle
            on={installEnabled}
            onChange={async (on) => {
              if (!on && githubEnabled) {
                await autoSaveConfig(
                  { capabilities: { learn_skills: { enabled: false, allow_github_fetch: false } } },
                  { silent: true },
                );
              } else {
                await autoSaveConfig(
                  { capabilities: { learn_skills: { enabled: on } } },
                  { silent: true },
                );
              }
              await loadSettingsConfig();
            }}
          />
        </div>
        {installEnabled === true && (
          <div className={`${styles['capability-row']} ${styles['capability-row-nested']}`}>
            <div className={styles['capability-row-label']}>
              <span className={styles['capability-row-name']}>{t('settings.skills.fetchRemote')}</span>
              <span className={`${styles['capability-row-desc']} ${styles['warn']}`}>{t('settings.skills.fetchRemoteDesc')}</span>
            </div>
            <Toggle
              on={githubEnabled}
              onChange={handleGithubToggle}
            />
          </div>
        )}
        {installEnabled === true && (
          <div className={`${styles['capability-row']} ${styles['capability-row-nested']}`}>
            <div className={styles['capability-row-label']}>
              <span className={styles['capability-row-name']}>{t('settings.skills.safetyReview')}</span>
              <span className={styles['capability-row-desc']}>{t('settings.skills.safetyReviewDesc')}</span>
            </div>
            <Toggle
              on={safetyReviewEnabled}
              onChange={async (on) => {
                if (!on) {
                  setShowSafetyWarning(true);
                } else {
                  await autoSaveConfig(
                    { capabilities: { learn_skills: { safety_review: true } } },
                    { silent: true },
                  );
                  await loadSettingsConfig();
                }
              }}
            />
          </div>
        )}
        <div className={styles['capability-row']}>
          <div className={styles['capability-row-label']}>
            <span className={styles['capability-row-name']}>{t('settings.skills.autolearn')}</span>
            <span className={styles['capability-row-desc']}>{t('settings.skills.autolearnDesc')}</span>
          </div>
          <Toggle
            on={autolearnEnabled}
            onChange={handleAutolearnToggle}
          />
        </div>
        <div className={styles['capability-row']}>
          <div className={styles['capability-row-label']}>
            <span className={styles['capability-row-name']}>{t('settings.skills.goalToggle')}</span>
            <span className={styles['capability-row-desc']}>{t('settings.skills.goalToggleDesc')}</span>
          </div>
          <Toggle
            on={goalEnabled}
            onChange={handleGoalToggle}
          />
        </div>
        <p className={styles['settings-inline-note']} style={{ padding: 'var(--space-8) var(--space-16)', margin: 0 }}>{t('settings.skills.learnHint')}</p>
      </SettingsSection>

      {showGithubWarning && (
        <div className="hana-warning-overlay" onClick={() => setShowGithubWarning(false)}>
          <div className="hana-warning-box" onClick={(e) => e.stopPropagation()}>
            <h3 className="hana-warning-title">{t('settings.skills.fetchWarning.title')}</h3>
            <div className="hana-warning-body">
              <p>{t('settings.skills.fetchWarning.body1')}</p>
              <p>{t('settings.skills.fetchWarning.body2')}</p>
              <p>
                1. {t('settings.skills.fetchWarning.risk1')}<br />
                2. {t('settings.skills.fetchWarning.risk2')}<br />
                3. {t('settings.skills.fetchWarning.risk3')}
              </p>
            </div>
            <div className="hana-warning-actions">
              <button className="hana-warning-cancel" onClick={() => setShowGithubWarning(false)}>
                {t('common.cancel')}
              </button>
              <button className="hana-warning-confirm" onClick={confirmGithubFetch}>
                {t('settings.skills.fetchWarning.confirm')}
              </button>
            </div>
          </div>
        </div>
      )}

      {showSafetyWarning && (
        <div className="hana-warning-overlay" onClick={() => setShowSafetyWarning(false)}>
          <div className="hana-warning-box" onClick={(e) => e.stopPropagation()}>
            <h3 className="hana-warning-title">{t('settings.skills.safetyWarning.title')}</h3>
            <div className="hana-warning-body">
              <p>{t('settings.skills.safetyWarning.body1')}</p>
              <p>
                1. {t('settings.skills.safetyWarning.risk1')}<br />
                2. {t('settings.skills.safetyWarning.risk2')}<br />
                3. {t('settings.skills.safetyWarning.risk3')}
              </p>
              <p>{t('settings.skills.safetyWarning.body2')}</p>
            </div>
            <div className="hana-warning-actions">
              <button className="hana-warning-cancel" onClick={() => setShowSafetyWarning(false)}>
                {t('common.cancel')}
              </button>
              <button className="hana-warning-confirm" onClick={async () => {
                setShowSafetyWarning(false);
                await autoSaveConfig(
                  { capabilities: { learn_skills: { safety_review: false } } },
                  { silent: true },
                );
                await loadSettingsConfig();
              }}>
                {t('settings.skills.safetyWarning.confirm')}
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
