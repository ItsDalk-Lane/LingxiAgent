import React, { useState } from 'react';
import { Toggle } from '@/ui';
import { SettingsRow } from '../../components/SettingsRow';
import { ExpandableRow } from '../../components/ExpandableRow';
import { NumberInput } from '../../components/NumberInput';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';

interface DeferSettingsProps {
  deferEnabled: boolean;
  deferThreshold: number;
  builtinDeferEnabled: boolean;
  busy: boolean;
  onChange: (patch: { deferEnabled?: boolean; deferThreshold?: number; builtinDeferEnabled?: boolean }) => void;
}

/**
 * The deferred-loading knobs. They apply to every connector, so they live at
 * the tab's top level next to the master switch rather than inside any single
 * connector's detail page.
 */
export function DeferSettings({ deferEnabled, deferThreshold, builtinDeferEnabled, busy, onChange }: DeferSettingsProps) {
  const [threshold, setThreshold] = useState(deferThreshold);

  return (
    <ExpandableRow label={t('settings.mcp.deferTitle')}>
      <p className={styles['settings-form-hint']}>{t('settings.mcp.deferScopeNote')}</p>
      <SettingsRow
        label={t('settings.mcp.deferEnabled')}
        hint={t('settings.mcp.deferEnabledHint')}
        control={(
          <Toggle
            on={deferEnabled}
            disabled={busy}
            ariaLabel={t('settings.mcp.deferEnabled')}
            onChange={(on) => onChange({ deferEnabled: on })}
          />
        )}
      />
      <SettingsRow
        label={t('settings.mcp.deferBuiltin')}
        hint={t('settings.mcp.deferBuiltinHint')}
        control={(
          <Toggle
            on={builtinDeferEnabled}
            // 独立于 MCP 总闸：内置/插件按需加载与连接器按需加载各管各的
            // 来源，任一开关都不应使另一个失效。
            disabled={busy}
            ariaLabel={t('settings.mcp.deferBuiltin')}
            onChange={(on) => onChange({ builtinDeferEnabled: on })}
          />
        )}
      />
      <SettingsRow
        label={t('settings.mcp.deferThreshold')}
        hint={t('settings.mcp.deferThresholdHint')}
        control={(
          <NumberInput
            value={threshold}
            min={1}
            step={1}
            precision="int"
            disabled={busy || !deferEnabled}
            onChange={(value) => {
              setThreshold(value);
              // A threshold below one is not a stricter setting, it is a
              // meaningless one; refuse rather than quietly substitute.
              if (Number.isSafeInteger(value) && value > 0) onChange({ deferThreshold: value });
            }}
          />
        )}
      />
    </ExpandableRow>
  );
}
