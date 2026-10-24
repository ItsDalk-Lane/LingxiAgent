import { useState, useEffect, useRef, useCallback } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { useSettingsStore, type Agent } from '../store';
import { lingxiFetch } from '../api';
import { t } from '../helpers';
import { loadSettingsConfig, loadAgents } from '../actions';
import { loadRustAgents } from '../rust-agents';
import { resolveServerConnection } from '../../services/service-client';
import { Overlay } from '../../ui';
import styles from '../Settings.module.css';

// 确认绑定完整连接快照；字段排序不把等价重建当作切换，任何实际身份/路由变化都须重开。
function connectionIdentity(connection: unknown): string {
  return JSON.stringify(connection ?? null, (_key, value: unknown) => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return value;
    const object = value as Record<string, unknown>;
    return Object.fromEntries(Object.keys(object).sort().map(key => [key, object[key]]));
  });
}

type CleanupSkill = { name: string; description?: string };

export function AgentDeleteOverlay() {
  const { agents, currentAgentId, settingsAgentId, source } = useSettingsStore(
    useShallow(s => ({ agents: s.agents, currentAgentId: s.currentAgentId, settingsAgentId: s.settingsAgentId, source: s }))
  );
  // 解析后备连接可能产生新对象；在订阅快照外解析，避免React快照被新引用反复触发。
  const connection = resolveServerConnection(source);
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
  const previewFailedRef = useRef(false);
  const deleteConnectionRef = useRef<string | null>(null);
  const confirmationOpenRef = useRef(false);
  const connectionChangedRef = useRef(false);
  const rustMode = connection?.serverNodeKind === 'lingxi-service';
  const connectionKey = connectionIdentity(connection);
  const previewPromiseRef = useRef<Promise<void> | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const targetId = deleteTargetId || settingsAgentId || currentAgentId;
  const targetSnapshotRef = useRef<Agent | null>(null);
  // 旧确认只保留原目标供显示拒绝；不能因新连接列表清空而失去原来的拒绝按钮。
  const target = connectionChangedRef.current || deleteConnectionRef.current !== connectionKey
    ? targetSnapshotRef.current : agents.find(a => a.id === targetId);

  const fetchCleanupPreview = useCallback(async (agentId: string) => {
    try {
      const res = await lingxiFetch(`/api/agents/${agentId}/skills/cleanup-preview`);
      const data = await res.json().catch(() => null);
      if (rustMode && !Array.isArray(data?.skills)) throw new Error('技能预览返回格式不正确');
      const skills = Array.isArray(data?.skills)
        ? data.skills
            .filter((s: any) => typeof s?.name === 'string')
            .map((s: any) => ({ name: s.name, description: typeof s.description === 'string' ? s.description : '' }))
        : [];
      cleanupSkillsRef.current = skills;
      setCleanupSkills(skills);
      // 默认全选：符合「删除助手时技能一并清掉」的主诉求，想保留的手动取消
      setSelectedSkills(new Set(skills.map((s: CleanupSkill) => s.name)));
    } catch (failure) {
      if (rustMode) {
        // Rust入口不能把预览失败当成没有技能；阻止继续并显示真正原因。
        previewFailedRef.current = true;
        setError(`${t('settings.agent.deleteFailed')}: ${failure instanceof Error ? failure.message : String(failure)}`);
      }
      // 旧Node入口保留既有两步回退行为。
      cleanupSkillsRef.current = [];
      setCleanupSkills([]);
      setSelectedSkills(new Set());
    }
  }, [rustMode]);

  useEffect(() => useSettingsStore.subscribe(state => {
    // 与lingxiFetch使用同一解析入口，登记表变更、后备来源切换和来回切换均锁存失效。
    if (confirmationOpenRef.current && deleteConnectionRef.current !== connectionIdentity(resolveServerConnection(state))) {
      connectionChangedRef.current = true;
    }
  }), []);

  useEffect(() => {
    const handler = (event: Event) => {
      const agentId = event instanceof CustomEvent && typeof event.detail?.agentId === 'string'
        ? event.detail.agentId
        : null;
      const current = useSettingsStore.getState();
      const originalId = agentId || current.settingsAgentId || current.currentAgentId;
      targetSnapshotRef.current = current.agents.find(agent => agent.id === originalId) || null;
      setDeleteTargetId(agentId);
      deleteConnectionRef.current = connectionIdentity(resolveServerConnection(useSettingsStore.getState()));
      confirmationOpenRef.current = true;
      connectionChangedRef.current = false;
      previewFailedRef.current = false;
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
  }, [fetchCleanupPreview, connectionKey]);

  useEffect(() => {
    if (step === 3) requestAnimationFrame(() => inputRef.current?.focus());
  }, [step]);

  const close = useCallback(() => {
    setVisible(false);
    confirmationOpenRef.current = false;
    setDeleteTargetId(null);
    targetSnapshotRef.current = null;
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
    if (previewFailedRef.current) return;
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
      if (connectionChangedRef.current || deleteConnectionRef.current !== connectionIdentity(resolveServerConnection(useSettingsStore.getState()))) {
        throw new Error('连接已改变，请重新打开删除确认');
      }
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
      if (rustMode && data.logicalDeleted === true) {
        window.dispatchEvent(new CustomEvent('hana-agent-deletion-changed'));
        const pending = [data.cleanupPending && '剩余文件清理待完成', data.referencesPending && '助手引用更新待完成，尚未确认切换', (data.journalCheckpointPending || data.journalError) && '删除记录保存待完成'].filter(Boolean);
        if (pending.length) {
          const completed = deletedCount > 0 ? t('settings.agent.deletedWithSkills', { name: target.name, count: String(deletedCount) }) : t('settings.agent.deleted', { name: target.name });
          showToast(`${completed}；${pending.join('；')}`, 'success');
        }
      }
      useSettingsStore.setState({ settingsAgentId: null });
      if (rustMode) {
        // 删除已成功；列表刷新失败单独显示，不能重新宣称删除失败。
        try {
          await loadRustAgents();
          if (data.referencesPending) useSettingsStore.setState({ currentAgentId: null });
          await loadSettingsConfig();
        }
        catch (failure) { showToast(failure instanceof Error ? failure.message : String(failure), 'error'); }
      } else {
        await loadAgents();
        await loadSettingsConfig();
      }
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
            {error && <div className={styles['settings-inline-error']} role="alert">{error}</div>}
            <div className={styles['agent-delete-actions']}>
              <button className={styles['agent-delete-cancel']} onClick={close}>{t('settings.agent.deleteCancel')}</button>
              <button
                className={styles['agent-delete-danger']}
                disabled={waitingPreview || (rustMode && !!error)}
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
