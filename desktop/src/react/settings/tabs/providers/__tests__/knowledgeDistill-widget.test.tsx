/**
 * @vitest-environment jsdom
 *
 * 复现「知识提炼模型只能跟随主模型」：用真 ModelWidget（非 mock）+
 * 注入 runtimeModels 目录，验证 knowledgeDistill 行能列出并保存模型。
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { useSettingsStore } from '../../../store';

const mocks = vi.hoisted(() => ({
  autoSaveGlobalModels: vi.fn(),
}));

vi.mock('../../../helpers', () => ({
  t: (key: string) => key,
  lookupModelMeta: vi.fn(),
  formatContext: (n: number) => String(n),
  autoSaveGlobalModels: mocks.autoSaveGlobalModels,
}));

vi.mock('../../../api', () => ({
  lingxiFetch: vi.fn(),
}));

vi.mock('../../../actions', () => ({
  loadSettingsConfig: vi.fn(),
}));

// AnchoredPortal 需要 layout 环境；替换为直接渲染（listbox 内容照常可查）。
vi.mock('@/ui', () => ({
  AnchoredPortal: ({ open, children }: { open: boolean; children: React.ReactNode }) => (
    open ? <>{children}</> : null
  ),
  Toggle: ({ label }: { label?: string }) => <button type="button">{label}</button>,
}));

import { AuxiliaryModelsSection } from '../AuxiliaryModelsSection';

describe('AuxiliaryModelsSection · knowledgeDistill 行为真实现', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSettingsStore.setState({
      globalModelsConfig: {
        models: {
          title: null,
          summarize: null,
          memory: null,
          knowledge: null,
          knowledgeDistill: null,
          vision: null,
          approval: null,
          guard: null,
          vision_enabled: false,
        },
        search: { provider: '', api_key: '' },
        utility_api: {},
        operation_models: [],
      },
      runtimeModels: [
        {
          id: 'glm-4.7',
          provider: 'zhipu',
          name: 'GLM 4.7',
          contextWindow: 128000,
        },
        {
          id: 'qwen3-max',
          provider: 'aliyun',
          name: 'Qwen3 Max',
          contextWindow: 262144,
        },
      ] as any,
    });
  });

  afterEach(() => {
    cleanup();
  });

  it('knowledgeDistill 行的下拉列出 runtimeModels 中的模型（不只"跟随主模型"）', () => {
    render(<AuxiliaryModelsSection providers={{}} />);

    // 打开知识提炼模型行的下拉（该行的触发按钮当前显示"跟随主模型"文案 key）
    const trigger = screen.getAllByText('settings.api.auxFollowMain')[4] // knowledgeDistill 是第 5 行（index 4）
      .closest('button');
    expect(trigger).not.toBeNull();
    fireEvent.click(trigger as HTMLElement);

    // 下拉里能看到两个模型选项（真 ModelWidget 渲染 runtimeModels）
    expect(screen.getByText('GLM 4.7')).toBeInTheDocument();
    expect(screen.getByText('Qwen3 Max')).toBeInTheDocument();
  });

  it('选择模型后以 knowledgeDistill 字段保存', () => {
    render(<AuxiliaryModelsSection providers={{}} />);

    const trigger = screen.getAllByText('settings.api.auxFollowMain')[4]
      .closest('button');
    fireEvent.click(trigger as HTMLElement);

    fireEvent.click(screen.getByText('GLM 4.7'));
    expect(mocks.autoSaveGlobalModels).toHaveBeenCalledWith({
      models: { knowledgeDistill: { id: 'glm-4.7', provider: 'zhipu' } },
    });
  });
});
