/**
 * @vitest-environment jsdom
 */

import React from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import '@testing-library/jest-dom/vitest';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { lingxiFetch } from '../../settings/api';

const mockState: Record<string, any> = {};

vi.mock('../../settings/store', () => ({
  useSettingsStore: Object.assign((selector?: (state: Record<string, any>) => unknown) => (
    selector ? selector(mockState) : mockState
  ), {
    setState: vi.fn((patch: Record<string, unknown>) => Object.assign(mockState, patch)),
  }),
}));

vi.mock('../../settings/api', () => ({
  lingxiFetch: vi.fn(),
}));

vi.mock('../../settings/actions', () => ({
  switchToAgent: vi.fn(),
  loadSettingsConfig: vi.fn(),
  loadAgents: vi.fn(),
}));

vi.mock('../../settings/helpers', () => ({
  t: (key: string, params?: Record<string, string>) => (
    params?.name ? `${key}:${params.name}` : key
  ),
}));

function openDeleteOverlay(agentId = 'hana') {
  act(() => {
    window.dispatchEvent(new CustomEvent('hana-show-agent-delete', { detail: { agentId } }));
  });
}

/** lingxiFetch mock：按 URL 分流（cleanup-preview 预览 / DELETE 删除） */
function mockLingxiFetch(responses: { preview?: any; delete?: any } = {}) {
  (lingxiFetch as any).mockImplementation(async (url: string, opts?: any) => {
    if (url.includes('/skills/cleanup-preview')) {
      return { json: async () => responses.preview ?? { skills: [] } };
    }
    if (opts?.method === 'DELETE' || url.includes('/api/agents/')) {
      return { json: async () => responses.delete ?? { ok: true, replacementAgentId: 'deepseek', skillsDeleted: [] } };
    }
    return { json: async () => ({}) };
  });
  return lingxiFetch;
}

describe('AgentDeleteOverlay', () => {
  beforeEach(() => {
    Object.keys(mockState).forEach(key => delete mockState[key]);
    Object.assign(mockState, {
      agents: [
        { id: 'hana', name: '小花', yuan: 'lingxi', isPrimary: true },
        { id: 'deepseek', name: 'DeepSeek', yuan: 'deepseek', isPrimary: false },
      ],
      currentAgentId: 'hana',
      settingsAgentId: 'hana',
      showToast: vi.fn(),
    });
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('uses the explicit event target instead of the selected settings agent', async () => {
    mockLingxiFetch();
    const { AgentDeleteOverlay } = await import('../../settings/overlays/AgentDeleteOverlay');
    render(<AgentDeleteOverlay />);
    openDeleteOverlay('deepseek');

    expect(screen.getByRole('heading', { name: 'settings.agent.deleteTitle1:DeepSeek' })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'settings.agent.deleteTitle1:小花' })).not.toBeInTheDocument();
  });

  it('skips the skill step when the preview finds no orphan skills (legacy flow)', async () => {
    const lingxiFetch = mockLingxiFetch({ preview: { skills: [] } });
    const actions = await import('../../settings/actions');
    const { AgentDeleteOverlay } = await import('../../settings/overlays/AgentDeleteOverlay');
    render(<AgentDeleteOverlay />);
    openDeleteOverlay('hana');

    await waitFor(() => expect(lingxiFetch).toHaveBeenCalledWith('/api/agents/hana/skills/cleanup-preview'));
    fireEvent.click(screen.getByText('settings.agent.deleteNext'));

    await waitFor(() => expect(screen.getByPlaceholderText('settings.agent.deletePlaceholder')).toBeInTheDocument());
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText('settings.agent.deletePlaceholder'), {
      target: { value: '小花' },
    });
    fireEvent.click(screen.getByText('settings.agent.deleteConfirm'));

    // 无随删技能时不带 body，与旧版完全一致
    await waitFor(() => expect(lingxiFetch).toHaveBeenCalledWith('/api/agents/hana', { method: 'DELETE' }));
    expect(actions.switchToAgent).not.toHaveBeenCalled();
    expect(actions.loadAgents).toHaveBeenCalled();
    expect(actions.loadSettingsConfig).toHaveBeenCalled();
  });

  it('offers orphan skills pre-selected, sends only the kept-selections in the DELETE body', async () => {
    const lingxiFetch = mockLingxiFetch({
      preview: { skills: [
        { name: 'alpha', description: 'Alpha skill' },
        { name: 'beta', description: 'Beta skill' },
      ] },
      delete: { ok: true, replacementAgentId: 'deepseek', skillsDeleted: ['alpha'] },
    });
    const { AgentDeleteOverlay } = await import('../../settings/overlays/AgentDeleteOverlay');
    render(<AgentDeleteOverlay />);
    openDeleteOverlay('hana');

    await waitFor(() => expect(lingxiFetch).toHaveBeenCalledWith('/api/agents/hana/skills/cleanup-preview'));
    fireEvent.click(screen.getByText('settings.agent.deleteNext'));

    // 技能步骤出现，默认全选
    await waitFor(() => expect(screen.getByText('settings.agent.deleteSkillsTitle')).toBeInTheDocument());
    const boxes = screen.getAllByRole('checkbox');
    expect(boxes).toHaveLength(2);
    expect(boxes[0]).toBeChecked();
    expect(boxes[1]).toBeChecked();

    // 取消 beta，只随删 alpha
    fireEvent.click(boxes[1]);
    expect(boxes[1]).not.toBeChecked();

    fireEvent.click(screen.getByText('settings.agent.deleteSkillsNextWith'));
    await waitFor(() => expect(screen.getByPlaceholderText('settings.agent.deletePlaceholder')).toBeInTheDocument());

    fireEvent.change(screen.getByPlaceholderText('settings.agent.deletePlaceholder'), {
      target: { value: '小花' },
    });
    fireEvent.click(screen.getByText('settings.agent.deleteConfirm'));

    await waitFor(() => expect(lingxiFetch).toHaveBeenCalledWith('/api/agents/hana', {
      method: 'DELETE',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ deleteSkills: ['alpha'] }),
    }));

    // 删除了技能时 toast 用带技能数的文案
    await waitFor(() => expect(mockState.showToast).toHaveBeenCalledWith(
      'settings.agent.deletedWithSkills:小花',
      'success',
    ));
  });

  it('allows keeping all skills and proceeds without a deleteSkills body', async () => {
    const lingxiFetch = mockLingxiFetch({
      preview: { skills: [{ name: 'alpha', description: '' }] },
    });
    const { AgentDeleteOverlay } = await import('../../settings/overlays/AgentDeleteOverlay');
    render(<AgentDeleteOverlay />);
    openDeleteOverlay('hana');

    await waitFor(() => expect(lingxiFetch).toHaveBeenCalledWith('/api/agents/hana/skills/cleanup-preview'));
    fireEvent.click(screen.getByText('settings.agent.deleteNext'));

    await waitFor(() => expect(screen.getByText('settings.agent.deleteSkillsTitle')).toBeInTheDocument());
    // 清空选择 → 按钮变成「保留技能，继续」
    fireEvent.click(screen.getByText('settings.agent.deleteSkillsClearAll'));
    const box = screen.getByRole('checkbox');
    expect(box).not.toBeChecked();
    fireEvent.click(screen.getByText('settings.agent.deleteSkillsNextWithout'));

    await waitFor(() => expect(screen.getByPlaceholderText('settings.agent.deletePlaceholder')).toBeInTheDocument());
    fireEvent.change(screen.getByPlaceholderText('settings.agent.deletePlaceholder'), {
      target: { value: '小花' },
    });
    fireEvent.click(screen.getByText('settings.agent.deleteConfirm'));

    await waitFor(() => expect(lingxiFetch).toHaveBeenCalledWith('/api/agents/hana', { method: 'DELETE' }));
  });

  it('falls back to the legacy flow when the preview request fails', async () => {
    const { lingxiFetch } = await import('../../settings/api');
    (lingxiFetch as any).mockRejectedValue(new Error('server offline'));
    const { AgentDeleteOverlay } = await import('../../settings/overlays/AgentDeleteOverlay');
    render(<AgentDeleteOverlay />);
    openDeleteOverlay('hana');

    fireEvent.click(screen.getByText('settings.agent.deleteNext'));
    await waitFor(() => expect(screen.getByPlaceholderText('settings.agent.deletePlaceholder')).toBeInTheDocument());
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();
  });
});
