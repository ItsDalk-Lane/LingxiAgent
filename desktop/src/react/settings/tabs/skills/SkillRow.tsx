import React from 'react';
import type { SkillInfo } from '../../store';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';

function truncateDesc(raw: string): string {
  const cnMatch = raw.match(/[\u4e00-\u9fff].*$/s);
  let desc = cnMatch ? cnMatch[0] : raw;
  desc = desc.replace(/\s*MANDATORY TRIGGERS:.*$/si, '').trim();
  if (desc.length > 80) desc = desc.slice(0, 80) + '\u2026';
  return desc;
}

interface SkillRowProps {
  skill: SkillInfo;
  nameHint?: string;
  deletable?: boolean;
  draggable?: boolean;
  highlighted?: boolean;
  className?: string;
  extraActions?: React.ReactNode;
  /** 标题后缀（如外部技能的来源工具徽标），渲染在技能名之后。 */
  titleSuffix?: React.ReactNode;
  /** 传了就渲染 delete 按钮。Section 1 "技能管理" 传；Section 3 "Agent 配置" 不传。 */
  onDelete?: (name: string) => void;
  /** delete 按钮的 title/aria 文案；缺省用"删除"。外部技能的 ✕ 语义是"改为未启用"，传此参覆盖。 */
  deleteLabel?: string;
  /** 传了就渲染 toggle 按钮。Section 3 "Agent 配置" 传；Section 1 "技能管理" 不传。 */
  onToggle?: (name: string, enabled: boolean) => void;
  onDragStart?: (event: React.DragEvent<HTMLDivElement>, name: string) => void;
  onDragOver?: (event: React.DragEvent<HTMLDivElement>) => void;
  onDrop?: (event: React.DragEvent<HTMLDivElement>) => void;
}

export function SkillRow({
  skill,
  nameHint,
  deletable = true,
  draggable = false,
  highlighted = false,
  className = '',
  extraActions,
  titleSuffix,
  onDelete,
  deleteLabel,
  onToggle,
  onDragStart,
  onDragOver,
  onDrop,
}: SkillRowProps) {
  const displayDesc = truncateDesc(skill.description || '');

  return (
    <div
      className={`${styles['skills-list-item']} ${className}`.trim()}
      data-highlighted-skill={highlighted ? skill.name : undefined}
      draggable={draggable}
      onDragStart={(event) => onDragStart?.(event, skill.name)}
      onDragOver={onDragOver}
      onDrop={onDrop}
      onClick={() => {
        if (skill.baseDir) {
          window.platform?.openSkillViewer?.({
            name: skill.name,
            baseDir: skill.baseDir,
            filePath: skill.filePath,
            installed: true,
          });
        }
      }}
    >
      <div className={styles['skills-list-info']}>
        <span className={styles['skills-list-name']}>
          {skill.name}
          {nameHint && <span className={styles['skills-list-name-hint']}>{nameHint}</span>}
          {titleSuffix}
        </span>
        <span className={styles['skills-list-desc']}>{displayDesc}</span>
      </div>
      <div className={styles['skills-list-actions']}>
        {extraActions}
        {deletable && onDelete && (
          <button
            className={styles['skill-card-delete']}
            type="button"
            title={deleteLabel ?? t('settings.skills.delete')}
            aria-label={deleteLabel ?? t('settings.skills.delete')}
            onClick={(e) => { e.stopPropagation(); onDelete(skill.name); }}
          >
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
              <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        )}
        {onToggle && (
          <button
            className={`hana-toggle${skill.enabled ? ' on' : ''}`}
            type="button"
            title={skill.enabled ? t('settings.skills.toggleDisable') : t('settings.skills.toggleEnable')}
            aria-label={skill.enabled
              ? t('settings.skills.toggleDisableNamed', { name: skill.name })
              : t('settings.skills.toggleEnableNamed', { name: skill.name })}
            onClick={(e) => { e.stopPropagation(); onToggle(skill.name, !skill.enabled); }}
          />
        )}
      </div>
    </div>
  );
}
