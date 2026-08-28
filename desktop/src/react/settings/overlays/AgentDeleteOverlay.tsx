import { useState, useEffect, useRef, useCallback } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { useSettingsStore } from '../store';
import { lingxiFetch } from '../api';
import { t } from '../helpers';
import { loadSettingsConfig, loadAgents } from '../actions';
import { Overlay } from '../../ui';
import styles from '../Settings.module.css';

type CleanupSkill = { name: string; description?: string };

export function AgentDeleteOverlay() {
  const { agents, currentAgentId, settingsAgentId } = useSettingsStore(
    useShallow(s => ({ agents: s.agents, currentAgentId: s.currentAgentId, settingsAgentId: s.settingsAgentId }))
  );
  const showToast = useSettingsStore(s => s.showToast);
  const [visible, setVisible] = useState(false);
  // step 2（技能勾选）只在预览发现可随删技能时出现，否则 1 → 3 直通
  const [step, setStep] = useState<1 | 2 | 3>(1);
  const [nameInput, setNameInput] = useState('');
  const [deleteTargetId, setDeleteTargetId] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState('');
  const [cleanupSkills, setCleanupSkills] = useState<CleanupSkill[]>([]);
  const [selectedSkills, setSelectedSkills] = useState<Set<string>>(new Set());
  const [waitingPreview, setWaitingPreview] = useState(false);
  // await 预览后闭包里的 state 已过期，走 ref 读最新结果
  const cleanupSkillsRef = useRef<CleanupSkill[]>([]);
  const previewPromiseRef = useRef<Promise<void> | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const targetId = deleteTargetId || settingsAgentId || currentAgentId;
  const target = agents.find(a => a.id === targetId);

  const fetchCleanupPreview = useCallback(async (agentId: string) => {
    try {
      const res = await lingxiFetch(`/api/agents/${agentId}/skills/cleanup-preview`);
      const data = await res.json().catch(() => null);
      const skills = Array.isArray(data?.skills)
        ? data.skills
            .filter((s: any) => typeof s?.name === 'string')
            .map((s: any) => ({ name: s.name, description: typeof s.description === 'string' ? s.description : '' }))
        : [];
      cleanupSkillsRef.current = skills;
      setCleanupSkills(skills);
      // 默认全选：符合「删除助手时技能一并清掉」的主诉求，想保留的手动取消
      setSelectedSkills(new Set(skills.map((s: CleanupSkill) => s.name)));
    } catch {
      // 预览失败静默降级为「无随删技能」，流程退回原来的两步
      cleanupSkillsRef.current = [];
      setCleanupSkills([]);
      setSelectedSkills(new Set());
    }
  }, []);

  useEffect(() => {
    const handler = (event: Event) => {
      const agentId = event instanceof CustomEvent && typeof event.detail?.agentId === 'string'
        ? event.detail.agentId
        : null;
      setDeleteTargetId(agentId);
      setStep(1);
      setNameInput('');
      setError('');
      setCleanupSkills([]);
      setSelectedSkills(new Set());
      cleanupSkillsRef.current = [];
      const resolvedId = agentId || null;
      previewPromiseRef.current = resolvedId ? fetchCleanupPreview(resolvedId) : null;
      setVisible(true);
    };
    window.addEventListener('hana-show-agent-delete', handler);
    return () => window.removeEventListener('hana-show-agent-delete', handler);
  }, [fetchCleanupPreview]);

  useEffect(() => {
    if (step === 3) requestAnimationFrame(() => inputRef.current?.focus());
  }, [step]);

  const close = useCallback(() => {
    setVisible(false);
    setDeleteTargetId(null);
    setDeleting(false);
    setError('');
  }, []);

  const goNextFromWarning = async () => {
    if (waitingPreview) return;
    if (previewPromiseRef.current) {
      setWaitingPreview(true);
      try {
        await previewPromiseRef.current;
      } finally {
        setWaitingPreview(false);
      }
    }
    if (cleanupSkillsRef.current.length > 0) {
      setStep(2);
    } else {
      setStep(3);
    }
  };

  const toggleSkill = (name: string) => {
    setSelectedSkills(prev => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  const confirmDelete = async () => {
    if (!target || nameInput.trim() !== target.name || deleting) return;
    setDeleting(true);
    setError('');
    try {
      const selected = [...selectedSkills];
      const res = await lingxiFetch(
        `/api/agents/${targetId}`,
        selected.length > 0
          ? {
              method: 'DELETE',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ deleteSkills: selected }),
            }
          : { method: 'DELETE' },
      );
      const data = await res.json();
      if (data.error) throw new Error(data.error);
      close();
      const deletedCount = Array.isArray(data?.skillsDeleted) ? data.skillsDeleted.length : 0;
      showToast(
        deletedCount > 0
          ? t('settings.agent.deletedWithSkills', { name: target.name, count: String(deletedCount) })
          : t('settings.agent.deleted', { name: target.name }),
        'success',
      );
      useSettingsStore.setState({ settingsAgentId: null });
      await loadAgents();
      await loadSettingsConfig();
    } catch (err: any) {
      const message = t('settings.agent.deleteFailed') + ': ' + err.message;
      setError(message);
      showToast(message, 'error');
    } finally {
      setDeleting(false);
    }
  };

  if (!target) return null;

  const allSelected = cleanupSkills.length > 0 && selectedSkills.size === cleanupSkills.length;

  return (
    <Overlay
      scope="inline"
      open={visible}
      onClose={close}
      backdrop="blur"
      zIndex={110}
      className={styles['agent-delete-card']}
      disableContainerAnimation
    >
        {step === 1 ? (
          <div className={styles['agent-delete-step']}>
            <h3 className={styles['agent-delete-title']}>{t('settings.agent.deleteTitle1', { name: target.name })}</h3>
            <p className={styles['agent-delete-desc']}>{t('settings.agent.deleteDesc1')}</p>
            <div className={styles['agent-delete-actions']}>
              <button className={styles['agent-delete-cancel']} onClick={close}>{t('settings.agent.deleteCancel')}</button>
              <button
                className={styles['agent-delete-danger']}
                disabled={waitingPreview}
                onClick={goNextFromWarning}
              >
                {t('settings.agent.deleteNext')}
              </button>
            </div>
          </div>
        ) : step === 2 ? (
          <div className={styles['agent-delete-step']}>
            <h3 className={styles['agent-delete-title']}>{t('settings.agent.deleteSkillsTitle')}</h3>
            <p className={styles['agent-delete-desc']}>{t('settings.agent.deleteSkillsDesc', { name: target.name })}</p>
            <div className={styles['agent-delete-skill-toolbar']}>
              <button
                type="button"
                className={styles['agent-delete-skill-toggle']}
                disabled={deleting}
                onClick={() => setSelectedSkills(allSelected ? new Set() : new Set(cleanupSkills.map(s => s.name)))}
              >
                {allSelected ? t('settings.agent.deleteSkillsClearAll') : t('settings.agent.deleteSkillsSelectAll')}
              </button>
              <span className={styles['agent-delete-skill-count']}>
                {selectedSkills.size > 0
                  ? t('settings.agent.deleteSkillsCount', { count: String(selectedSkills.size) })
                  : t('settings.agent.deleteSkillsKeepAll')}
              </span>
            </div>
            <div className={styles['agent-delete-skill-list']}>
              {cleanupSkills.map(skill => (
                <label key={skill.name} className={styles['agent-delete-skill-item']}>
                  <input
                    type="checkbox"
                    checked={selectedSkills.has(skill.name)}
                    disabled={deleting}
                    onChange={() => toggleSkill(skill.name)}
                  />
                  <span className={styles['agent-delete-skill-meta']}>
                    <span className={styles['agent-delete-skill-name']}>{skill.name}</span>
                    {skill.description && (
                      <span className={styles['agent-delete-skill-desc']}>{skill.description}</span>
                    )}
                  </span>
                </label>
              ))}
            </div>
            <div className={styles['agent-delete-actions']}>
              <button className={styles['agent-delete-cancel']} onClick={close} disabled={deleting}>{t('settings.agent.deleteCancel')}</button>
              <button
                className={styles['agent-delete-danger']}
                disabled={deleting}
                onClick={() => setStep(3)}
              >
                {selectedSkills.size > 0
                  ? t('settings.agent.deleteSkillsNextWith', { count: String(selectedSkills.size) })
                  : t('settings.agent.deleteSkillsNextWithout')}
              </button>
            </div>
          </div>
        ) : (
          <div className={styles['agent-delete-step']}>
            <h3 className={styles['agent-delete-title']}>{t('settings.agent.deleteTitle2', { name: target.name })}</h3>
            <div className={styles['settings-form-field']}>
              <input
                ref={inputRef}
                className={`${styles['settings-input']} ${styles['agent-delete-input']}`}
                type="text"
                placeholder={t('settings.agent.deletePlaceholder')}
                autoComplete="off"
                value={nameInput}
                disabled={deleting}
                onChange={(e) => {
                  setNameInput(e.target.value);
                  setError('');
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') { e.preventDefault(); confirmDelete(); }
                }}
              />
            </div>
            {error && <div className={styles['settings-inline-error']} role="alert">{error}</div>}
            <div className={styles['agent-delete-actions']}>
              <button className={styles['agent-delete-cancel']} onClick={close} disabled={deleting}>{t('settings.agent.deleteCancel')}</button>
              <button
                className={styles['agent-delete-danger']}
                disabled={deleting || nameInput.trim() !== target.name}
                onClick={confirmDelete}
              >
                {t('settings.agent.deleteConfirm')}
              </button>
            </div>
          </div>
        )}
    </Overlay>
  );
}
