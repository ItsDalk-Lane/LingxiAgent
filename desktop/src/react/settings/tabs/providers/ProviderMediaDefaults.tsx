import React, { useState, useEffect, useMemo, useCallback } from 'react';
import { t } from '../../helpers';
import { SelectWidget } from '@/ui';
import type { MediaConfig, MediaProvider, MediaConfigUpdater } from '../../hooks/useMediaSettingsData';
import styles from '../../Settings.module.css';

interface Props {
  capability: 'imageGeneration' | 'videoGeneration';
  runtimeProviderId: string;
  provider: MediaProvider;
  config: MediaConfig;
  defaultModel?: { id: string; provider: string };
  onSaveConfig: (updates: MediaConfigUpdater) => Promise<void>;
}

type JsonSchemaProperty = {
  type?: string | string[];
  enum?: Array<string | number | boolean>;
  default?: any;
  minimum?: number;
  maximum?: number;
  description?: string;
  title?: string;
};

type MediaMode = {
  id: string;
  label?: string;
  parameterSchema?: {
    type?: string;
    properties?: Record<string, JsonSchemaProperty>;
  };
  defaults?: Record<string, any>;
};

type DefaultsModel = {
  id: string;
  name?: string;
  displayName?: string;
  protocolId?: string;
  ratios?: string[];
  resolutions?: string[];
  modes?: MediaMode[];
};

function isPlainObject(value: any): value is Record<string, any> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

function modeDefaultsForProvider(defaults: Record<string, any>, modelId: string, modeId: string) {
  return defaults?.models?.[modelId]?.modes?.[modeId] || {};
}

function clearEmptyObject(value: any) {
  if (!isPlainObject(value)) return value;
  for (const key of Object.keys(value)) {
    if (isPlainObject(value[key])) {
      clearEmptyObject(value[key]);
      if (Object.keys(value[key]).length === 0) delete value[key];
    }
  }
  return value;
}

export function ProviderMediaDefaults({ capability, runtimeProviderId, provider, config, defaultModel, onSaveConfig }: Props) {
  const models = (provider.models || []) as DefaultsModel[];
  const defaults = config.providerDefaults?.[runtimeProviderId] || {};
  const isDefault = useCallback((modelId: string) =>
    defaultModel?.id === modelId && defaultModel?.provider === runtimeProviderId,
  [defaultModel?.id, defaultModel?.provider, runtimeProviderId]);

  const updateDefault = (key: string, value: any) => {
    const current = config.providerDefaults || {};
    const provDefaults = { ...current[runtimeProviderId], [key]: value };
    onSaveConfig(() => ({ providerDefaults: { ...current, [runtimeProviderId]: provDefaults } }));
  };

  const initialDefaultsModelId = models.find(m => isDefault(m.id))?.id || models[0]?.id || '';
  const [defaultsModelId, setDefaultsModelId] = useState(initialDefaultsModelId);
  const defaultsModel = models.find(m => m.id === defaultsModelId) || models[0] || null;
  const modelModes = useMemo(() => (
    Array.isArray(defaultsModel?.modes) ? defaultsModel.modes.filter(m => m?.id) : []
  ), [defaultsModel]);
  const [defaultsModeId, setDefaultsModeId] = useState(modelModes[0]?.id || '');
  const defaultsMode = modelModes.find(m => m.id === defaultsModeId) || modelModes[0] || null;
  const schemaProperties = defaultsMode?.parameterSchema?.properties || {};
  const schemaEntries = Object.entries(schemaProperties);
  const schemaDrivenDefaults = schemaEntries.length > 0;
  const fallbackRatios = Array.isArray(defaultsModel?.ratios) ? defaultsModel.ratios : [];
  const fallbackResolutions = Array.isArray(defaultsModel?.resolutions) ? defaultsModel.resolutions : [];
  const savedModeDefaults = defaultsModel && defaultsMode
    ? modeDefaultsForProvider(defaults, defaultsModel.id, defaultsMode.id)
    : {};

  useEffect(() => {
    const nextModelId = models.find(m => m.id === defaultsModelId)?.id
      || models.find(m => isDefault(m.id))?.id
      || models[0]?.id
      || '';
    if (nextModelId !== defaultsModelId) setDefaultsModelId(nextModelId);
  }, [models, defaultsModelId, defaultModel?.id, defaultModel?.provider, isDefault]);

  useEffect(() => {
    const nextModeId = modelModes.find(m => m.id === defaultsModeId)?.id || modelModes[0]?.id || '';
    if (nextModeId !== defaultsModeId) setDefaultsModeId(nextModeId);
  }, [modelModes, defaultsModeId]);

  const updateModeDefault = (key: string, value: any) => {
    if (!defaultsModel || !defaultsMode) return;
    const current = config.providerDefaults || {};
    const providerDefaults = { ...(current[runtimeProviderId] || {}) };
    const modelsMap = { ...(providerDefaults.models || {}) };
    const modelDefaults = { ...(modelsMap[defaultsModel.id] || {}) };
    const modes = { ...(modelDefaults.modes || {}) };
    const modeDefaults = { ...(modes[defaultsMode.id] || {}) };
    if (value === undefined || value === null || value === '') delete modeDefaults[key];
    else modeDefaults[key] = value;
    modes[defaultsMode.id] = modeDefaults;
    modelDefaults.modes = modes;
    modelsMap[defaultsModel.id] = modelDefaults;
    providerDefaults.models = modelsMap;
    clearEmptyObject(providerDefaults);
    onSaveConfig(() => ({ providerDefaults: { ...current, [runtimeProviderId]: providerDefaults } }));
  };

  const renderSchemaControl = (key: string, property: JsonSchemaProperty) => {
    const value = savedModeDefaults[key] ?? '';
    const label = property.title || key;
    const description = property.description || label;
    if (Array.isArray(property.enum)) {
      return (
        <div key={key} className={styles['media-config-field']}>
          <span className={styles['media-config-label']} title={description}>
            {label}
          </span>
          <SelectWidget
            value={value === undefined || value === null ? '' : String(value)}
            onChange={(v) => updateModeDefault(key, v || undefined)}
            options={[
              { value: '', label: t('settings.media.defaultOption') },
              ...property.enum.map(item => ({ value: String(item), label: String(item) })),
            ]}
          />
        </div>
      );
    }
    const isNumber = property.type === 'number' || property.type === 'integer'
      || (Array.isArray(property.type) && (property.type.includes('number') || property.type.includes('integer')));
    return (
      <div key={key} className={styles['media-config-field']}>
        <span className={styles['media-config-label']} title={description}>
          {label}
        </span>
        <input
          className={styles['settings-input']}
          type={isNumber ? 'number' : 'text'}
          min={property.minimum}
          max={property.maximum}
          step={property.type === 'integer' ? 1 : undefined}
          value={value === undefined || value === null ? '' : String(value)}
          placeholder={property.default === undefined ? t('settings.media.defaultOption') : String(property.default)}
          onChange={(event) => {
            const raw = event.currentTarget.value;
            if (!raw) {
              updateModeDefault(key, undefined);
              return;
            }
            updateModeDefault(key, isNumber ? Number(raw) : raw);
          }}
        />
      </div>
    );
  };

  const defaultsTitle = capability === 'videoGeneration'
    ? t('settings.media.videoProviderDefaults')
    : t('settings.media.imageProviderDefaults');

  return (
    <div className={styles['media-defaults']}>
      <div className={styles['media-defaults-title']}>
        {defaultsTitle}
      </div>
      {schemaDrivenDefaults ? (
        <div className={styles['media-defaults-stack']}>
          <div className={`${styles['media-config-grid']}${modelModes.length > 1 ? '' : ' ' + styles['media-config-grid-single']}`}>
            <div className={styles['media-config-field']}>
              <span className={styles['media-config-label']}>
                {capability === 'videoGeneration' ? t('settings.media.videoModels') : t('settings.media.models')}
              </span>
              <SelectWidget
                value={defaultsModel?.id || ''}
                onChange={(v) => setDefaultsModelId(v)}
                options={models.map(model => ({
                  value: model.id,
                  label: model.name || model.id,
                }))}
              />
            </div>
            {modelModes.length > 1 && (
              <div className={styles['media-config-field']}>
                <span className={styles['media-config-label']}>
                  Mode
                </span>
                <SelectWidget
                  value={defaultsMode?.id || ''}
                  onChange={(v) => setDefaultsModeId(v)}
                  options={modelModes.map(mode => ({
                    value: mode.id,
                    label: mode.label || mode.id,
                  }))}
                />
              </div>
            )}
          </div>
          <div className={styles['media-config-grid']}>
            {schemaEntries.map(([key, property]) => renderSchemaControl(key, property))}
          </div>
        </div>
      ) : (
        <div className={styles['media-config-grid']}>
          {capability === 'imageGeneration' && fallbackResolutions.length > 0 && (
            <div className={styles['media-config-field']}>
              <span className={styles['media-config-label']}>
                {t('settings.media.size')}
              </span>
              <SelectWidget
                value={defaults.resolution || ''}
                onChange={(v) => updateDefault('resolution', v || undefined)}
                options={[
                  { value: '', label: t('settings.media.defaultOption') },
                  ...fallbackResolutions.map(item => ({ value: String(item), label: String(item) })),
                ]}
              />
            </div>
          )}
          {fallbackRatios.length > 0 && (
            <div className={styles['media-config-field']}>
              <span className={styles['media-config-label']}>
                {t('settings.media.aspectRatio')}
              </span>
              <SelectWidget
                value={defaults.aspect_ratio || ''}
                onChange={(v) => updateDefault('aspect_ratio', v || undefined)}
                options={[
                  { value: '', label: t('settings.media.defaultOption') },
                  ...fallbackRatios.map(item => ({ value: String(item), label: String(item) })),
                ]}
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
}
