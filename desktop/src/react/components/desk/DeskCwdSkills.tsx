/**
 * DeskCwdSkills — CWD 项目技能内嵌面板
 *
 * 「对话文件 / 工作台 / 项目技能」切换行的第三个 tab 内容：
 * 挂载即随工作台加载，不再有独立按钮与悬浮展开态。
 */

import { useCallback, useEffect, useState } from 'react';
import { useStore } from '../../stores';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import { canUseNativeResourcePath } from '../../services/resource-access';
import { resolveServerConnection } from '../../services/server-connection';
import { isWebRuntime } from '../../utils/platform-runtime';
import type { CwdSkillInfo } from '../../stores/desk-slice';
import css from './Desk.module.css';

// ── 加载 CWD skills ──

function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error || new Error('failed to read skill package'));
    reader.onload = () => {
      const result = String(reader.result || '');
      resolve(result.includes(',') ? result.split(',').pop() || '' : result);
    };
    reader.readAsDataURL(file);
  });
}

function canUseNativeDeskPath() {
  return canUseNativeResourcePath({ connection: resolveServerConnection(useStore.getState()) });
}

async function loadCwdSkills() {
  const s = useStore.getState();
  if (!s.deskBasePath) {
    useStore.setState({
      cwdSkills: [],
      cwdSkillPolicy: {
        discoverProjectSkills: true,
        discoverCompatibleProjectSkills: false,
      },
    });
    return;
  }
  const params = new URLSearchParams();
  if (s.deskWorkspaceMountId) {
    params.set('mountId', s.deskWorkspaceMountId);
  } else {
    params.set('dir', s.deskBasePath);
  }
  if (s.selectedAgentId) params.set('agentId', s.selectedAgentId);
  try {
    const res = await lingxiFetch(
      `/api/desk/skills?${params}`,
    );
    const data = await res.json();
    useStore.setState({
      cwdSkills: data.skills || [],
      ...(data.policy ? { cwdSkillPolicy: data.policy } : {}),
    });
  } catch { /* ignore */ }
}

// ── CWD Skills 面板（内嵌于右下区第三个 tab） ──

export function DeskCwdSkillsPanel() {
  const skills = useStore(s => s.cwdSkills) ?? [];
  const deskBasePath = useStore(s => s.deskBasePath);
  const deskWorkspaceMountId = useStore(s => s.deskWorkspaceMountId);
  const skillCatalogVersion = useStore(s => s.skillCatalogVersion);
  const canUseNativePath = useStore(s => canUseNativeResourcePath({ connection: resolveServerConnection(s) }));
  const t = window.t ?? ((p: string) => p);

  // 挂载（= 切到该 tab）即加载；工作台切换或技能目录变更时刷新
  useEffect(() => {
    void loadCwdSkills();
  }, [deskBasePath, deskWorkspaceMountId, skillCatalogVersion]);

  const [dragging, setDragging] = useState(false);
  const [cmPos, setCmPos] = useState<{ x: number; y: number } | null>(null);
  const [cmSkill, setCmSkill] = useState<CwdSkillInfo | null>(null);

  useEffect(() => {
    if (!cmPos) return;
    const close = () => { setCmPos(null); setCmSkill(null); };
    document.addEventListener('click', close);
    return () => document.removeEventListener('click', close);
  }, [cmPos]);

  const deleteSkill = useCallback(async (skill: CwdSkillInfo) => {
    if (!skill.baseDir) return;
    try {
      await lingxiFetch('/api/desk/delete-skill', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          skillDir: skill.baseDir,
          ...(useStore.getState().deskWorkspaceMountId ? { mountId: useStore.getState().deskWorkspaceMountId } : {}),
        }),
      });
      await loadCwdSkills();
    } catch (err) {
      console.error('[cwd-skills] delete failed:', err);
    }
  }, []);

  const handleDrop = useCallback(async (e: React.DragEvent) => {
    e.preventDefault();
    setDragging(false);
    const files = Array.from(e.dataTransfer.files);
    if (files.length === 0) return;
    const { deskBasePath: dir, deskWorkspaceMountId: mountId } = useStore.getState();
    const canUseNativePathForDrop = canUseNativeDeskPath();
    console.log('[cwd-skills] drop: files=', files.length, 'dir=', dir);
    if (!dir) return;
    let installed = false;
    for (const file of files) {
      const filePath = canUseNativePathForDrop ? window.platform?.getFilePath?.(file) : null;
      console.log('[cwd-skills] filePath=', filePath, 'file.name=', file.name);
      try {
        const s = useStore.getState();
        const contentBase64 = filePath ? null : await fileToBase64(file);
        const res = await lingxiFetch('/api/desk/install-skill', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            ...(filePath ? { filePath } : { file: { filename: file.name || 'skill.skill', contentBase64 } }),
            ...(mountId ? { mountId } : { dir }),
            ...(s.selectedAgentId ? { agentId: s.selectedAgentId } : {}),
          }),
        });
        const data = await res.json();
        if (data.error) {
          console.warn('[cwd-skills] install failed:', data.error);
        } else {
          console.log('[cwd-skills] installed:', data.name);
          installed = true;
        }
      } catch (err) {
        console.error('[cwd-skills] install failed:', err);
      }
    }
    if (installed) await loadCwdSkills();
  }, []);

  const grouped: Record<string, CwdSkillInfo[]> = {};
  for (const s of skills) {
    (grouped[s.source] ??= []).push(s);
  }

  return (
    <div
      className={`${css.cwdPanelInline}${dragging ? ` ${css.dragOver}` : ''}`}
      data-desk-cwd-panel=""
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        setCmPos({ x: e.clientX, y: e.clientY });
      }}
      onDragOver={(e) => { e.preventDefault(); e.stopPropagation(); setDragging(true); }}
      onDragLeave={() => setDragging(false)}
      onDrop={(e) => { handleDrop(e); }}
    >
      <div className={css.cwdDescLine}>
        <span className={css.cwdDescDeco} />
        <span className={css.cwdDescText}>{t('desk.cwdSkillsDesc')}</span>
        <span className={css.cwdDescDeco} />
      </div>

      {skills.length === 0 ? (
        <>
          <p className={css.cwdEmpty}>{t('desk.cwdSkillsEmpty')}</p>
          <p className={css.cwdHint}>{t('desk.cwdSkillsDrop')}</p>
        </>
      ) : (
        <>
          {Object.entries(grouped).map(([source, items]) => (
            <div key={source}>
              <div className={css.cwdGroupLabel}>{source}</div>
              {items.map(s => {
                let desc = s.description || '';
                if (desc.length > 60) desc = desc.slice(0, 60) + '...';
                return (
                  <div
                    className={css.cwdSkillItem}
                    key={s.name}
                    onDoubleClick={() => {
                      if (deskWorkspaceMountId || !canUseNativePath) return;
                      window.platform?.openSkillViewer?.({
                        name: s.name,
                        baseDir: s.baseDir,
                        filePath: s.filePath,
                        installed: false,
                      });
                    }}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      setCmPos({ x: e.clientX, y: e.clientY });
                      setCmSkill(s);
                    }}
                  >
                    <span className={css.cwdSkillName}>{s.name}</span>
                    {s.active && <span className={css.cwdSkillDesc}>{t('desk.cwdSkillActive')}</span>}
                    {!s.active && s.shadowed && (
                      <span className={css.cwdSkillDesc}>
                        {t('desk.cwdSkillShadowed')} · {s.shadowedBy?.source || ''}
                      </span>
                    )}
                    {!s.active && !s.shadowed && (
                      <span className={css.cwdSkillDesc}>{t('desk.cwdSkillInactive')}</span>
                    )}
                    {desc && <span className={css.cwdSkillDesc}>{desc}</span>}
                  </div>
                );
              })}
            </div>
          ))}
          <p className={css.cwdHint}>{t('desk.cwdSkillsDrop')}</p>
        </>
      )}
      {cmPos && (
        <div className={css.cwdCtxMenu} style={{ position: 'fixed', left: cmPos.x, top: cmPos.y, zIndex: 9999 }}>
          {!isWebRuntime() && !deskWorkspaceMountId && canUseNativePath && (
            <button onClick={() => {
              const target = cmSkill?.baseDir || (useStore.getState().deskBasePath + '/.agents/skills');
              window.platform?.showInFinder?.(target);
              setCmPos(null);
            }}>
              {t('desk.openInFinder')}
            </button>
          )}
          {cmSkill && (
            <button className={css.cwdCtxDanger} onClick={() => {
              deleteSkill(cmSkill);
              setCmPos(null);
            }}>
              {t('desk.deleteSkill')}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
