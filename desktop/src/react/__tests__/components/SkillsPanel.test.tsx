// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { installWindowTestT } from '../helpers/i18n-test-strings';
import { useStore } from '../../stores';
import { SkillsPanel } from '../../components/SkillsPanel';

const fetchMock = vi.fn();
vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: (...args: unknown[]) => fetchMock(...args),
}));

function jsonResponse(body: unknown): Response {
  return { ok: true, json: async () => body } as unknown as Response;
}

async function flushMicrotasks(ticks = 3) {
  await act(async () => {
    for (let i = 0; i < ticks; i++) await Promise.resolve();
  });
}

describe('SkillsPanel', () => {
  beforeEach(() => {
    fetchMock.mockReset();
    installWindowTestT({
      'settings.skills.installSuccess': 'installed {name}',
    });
    window.platform = {
      getFilePath: vi.fn(() => '/tmp/new-skill.skill'),
      openSkillViewer: vi.fn(),
    } as unknown as typeof window.platform;
    useStore.setState({
      activePanel: 'skills',
      currentAgentId: 'agent-a',
      agentName: 'Hana',
      agentYuan: 'lingxi',
      agents: [
        { id: 'agent-a', name: 'Hana', yuan: 'lingxi', isPrimary: true },
        { id: 'agent-b', name: 'Mao', yuan: 'butter', isPrimary: false },
      ],
    } as never);
  });

  afterEach(() => {
    cleanup();
    delete (window as unknown as { platform?: unknown }).platform;
  });

  it('installs dropped skills for the selected agent, returns to all skills, and highlights the installed row', async () => {
    let installed = false;
    fetchMock.mockImplementation((url: string, opts?: RequestInit) => {
      if (url.includes('/api/skills/install')) {
        expect(url).toContain('agentId=agent-b');
        expect(JSON.parse(String(opts?.body || '{}'))).toMatchObject({ path: '/tmp/new-skill.skill' });
        installed = true;
        return Promise.resolve(jsonResponse({ ok: true, skill: { name: 'new-skill' } }));
      }
      if (url.includes('/api/skills/bundles')) {
        return Promise.resolve(jsonResponse({ bundles: [] }));
      }
      if (url.includes('/api/skills?agentId=')) {
        return Promise.resolve(jsonResponse({
          skills: installed
            ? [
                { name: 'old-skill', enabled: true, source: 'user', description: 'Existing' },
                { name: 'new-skill', enabled: true, source: 'user', description: 'Fresh' },
              ]
            : [
                { name: 'old-skill', enabled: true, source: 'user', description: 'Existing' },
              ],
        }));
      }
      return Promise.resolve(jsonResponse({}));
    });

    render(<SkillsPanel />);
    await flushMicrotasks(4);

    expect(fetchMock.mock.calls.some((call) =>
      typeof call[0] === 'string'
      && call[0].includes('/api/skills?agentId=agent-a')
      && call[0].includes('runtime=1'),
    )).toBe(true);

    fireEvent.click(screen.getByRole('tab', { name: 'Mao' }));

    const file = new File(['skill'], 'new-skill.skill');
    fireEvent.drop(screen.getByTestId('skills-panel-drop-surface'), {
      dataTransfer: { files: [file] },
    });

    await waitFor(() => expect(fetchMock.mock.calls.some((call) =>
      typeof call[0] === 'string' && call[0].includes('/api/skills/install?agentId=agent-b'),
    )).toBe(true));
    await flushMicrotasks(6);

    expect(screen.getByRole('tab', { name: 'skills.panel.allTab' })).toHaveAttribute('aria-selected', 'true');
    expect(document.querySelector('[data-highlighted-skill="new-skill"]')).toBeTruthy();
  });

  it('installs dropped skills for the current agent from all skills by default', async () => {
    fetchMock.mockImplementation((url: string, opts?: RequestInit) => {
      if (url.includes('/api/skills/install')) {
        expect(url).toContain('agentId=agent-a');
        expect(JSON.parse(String(opts?.body || '{}'))).toMatchObject({ path: '/tmp/new-skill.skill' });
        return Promise.resolve(jsonResponse({ ok: true, skill: { name: 'current-agent-skill' } }));
      }
      if (url.includes('/api/skills/bundles')) {
        return Promise.resolve(jsonResponse({ bundles: [] }));
      }
      if (url.includes('/api/skills?agentId=')) {
        return Promise.resolve(jsonResponse({
          skills: [
            { name: 'current-agent-skill', enabled: true, source: 'user', description: 'Fresh' },
          ],
        }));
      }
      return Promise.resolve(jsonResponse({}));
    });

    render(<SkillsPanel />);
    await flushMicrotasks(4);

    const file = new File(['skill'], 'new-skill.skill');
    fireEvent.drop(screen.getByTestId('skills-panel-drop-surface'), {
      dataTransfer: { files: [file] },
    });

    await waitFor(() => expect(fetchMock.mock.calls.some((call) =>
      typeof call[0] === 'string' && call[0].includes('/api/skills/install?agentId=agent-a'),
    )).toBe(true));
    await flushMicrotasks(6);

    expect(screen.getByRole('tab', { name: 'skills.panel.allTab' })).toHaveAttribute('aria-selected', 'true');
    expect(document.querySelector('[data-highlighted-skill="current-agent-skill"]')).toBeTruthy();
  });

  it('creates skill bundles from the all skills page for the current agent view', async () => {
    let created = false;
    fetchMock.mockImplementation((url: string, opts?: RequestInit) => {
      if (url.includes('/api/skills/bundles') && opts?.method === 'POST') {
        expect(url).toContain('agentId=agent-a');
        expect(JSON.parse(String(opts.body || '{}'))).toMatchObject({
          name: 'Research',
          skillNames: [],
        });
        created = true;
        return Promise.resolve(jsonResponse({ ok: true, bundle: { id: 'research', name: 'Research', skillNames: [] } }));
      }
      if (url.includes('/api/skills/bundles')) {
        return Promise.resolve(jsonResponse({ bundles: [] }));
      }
      if (url.includes('/api/skills?agentId=')) {
        return Promise.resolve(jsonResponse({
          skills: [
            { name: 'reader', enabled: true, source: 'user', description: 'Read' },
          ],
        }));
      }
      return Promise.resolve(jsonResponse({}));
    });

    render(<SkillsPanel />);
    await flushMicrotasks(4);

    fireEvent.click(screen.getByRole('button', { name: 'settings.skills.createBundleAriaLabel' }));
    fireEvent.change(screen.getByLabelText('settings.skills.bundleDialog.bundleNameLabel'), {
      target: { value: 'Research' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'settings.skills.bundleDialog.createBtn' }));

    await waitFor(() => expect(created).toBe(true));
  });

  it('toggles a skill from an agent tab with the same agent skills API as settings', async () => {
    let toggled = false;
    fetchMock.mockImplementation((url: string, opts?: RequestInit) => {
      if (url.includes('/api/agents/agent-b/skills/reader')) {
        expect(opts?.method).toBe('PATCH');
        expect(JSON.parse(String(opts?.body || '{}'))).toEqual({ enabled: true });
        toggled = true;
        return Promise.resolve(jsonResponse({ ok: true, enabled: ['reader'], changed: ['reader'] }));
      }
      if (url.includes('/api/skills/bundles')) {
        return Promise.resolve(jsonResponse({ bundles: [] }));
      }
      if (url.includes('/api/skills?agentId=agent-b')) {
        return Promise.resolve(jsonResponse({
          skills: [
            { name: 'reader', enabled: toggled, source: 'user', description: 'Read' },
          ],
        }));
      }
      if (url.includes('/api/skills?agentId=')) {
        return Promise.resolve(jsonResponse({ skills: [] }));
      }
      return Promise.resolve(jsonResponse({}));
    });

    render(<SkillsPanel />);
    await flushMicrotasks(4);

    fireEvent.click(screen.getByRole('tab', { name: 'Mao' }));
    await screen.findByText('reader');

    fireEvent.click(screen.getByRole('button', { name: '启用 reader' }));

    await waitFor(() => expect(toggled).toBe(true));
  });

  it('mixes compat-enabled external skills inline with badges; remove marks them disabled', async () => {
    let removed = false;
    fetchMock.mockImplementation((url: string, opts?: RequestInit) => {
      if (url.includes('/api/agents/agent-a/skills/code-review')) {
        expect(opts?.method).toBe('PATCH');
        expect(JSON.parse(String(opts?.body || '{}'))).toEqual({ enabled: false });
        removed = true;
        return Promise.resolve(jsonResponse({ ok: true, enabled: [], changed: ['code-review'] }));
      }
      if (url.includes('/api/skills/bundles')) {
        return Promise.resolve(jsonResponse({ bundles: [] }));
      }
      if (url.includes('/api/skills?agentId=agent-a')) {
        return Promise.resolve(jsonResponse({
          skills: [
            { name: 'reader', enabled: true, source: 'user', description: 'Read' },
            { name: 'code-review', enabled: !removed, source: 'external', description: 'Review with Codex', externalLabel: 'Codex', externalPath: '/home/u/.codex/skills' },
            { name: 'doc-writer', enabled: false, source: 'external', description: 'Write docs', externalLabel: 'Claude Code', externalPath: '/home/u/.claude/skills' },
          ],
        }));
      }
      return Promise.resolve(jsonResponse({ skills: [] }));
    });

    render(<SkillsPanel />);
    await flushMicrotasks(4);

    // 全部技能视图：仅启用过的外部技能混排显示（doc-writer 未启用 → 不出现），带来源徽标，无分组标题
    expect(screen.getByText('code-review')).toBeInTheDocument();
    expect(screen.getByText('Codex')).toBeInTheDocument();
    expect(screen.queryByText('doc-writer')).toBeNull();
    expect(screen.queryByText('Claude Code')).toBeNull();
    expect(screen.queryByText('skills.panel.externalSection')).toBeNull();
    // 管理视图无启停开关
    expect(screen.queryByRole('button', { name: '启用 code-review' })).toBeNull();

    // 全部技能页的「移除」✕ = 改为未启用（与设置页兼容技能关闭同一状态），不是删除
    fireEvent.click(screen.getByRole('button', { name: 'skills.panel.externalRemove' }));
    await waitFor(() => expect(removed).toBe(true));
    await waitFor(() => expect(screen.queryByText('code-review')).toBeNull());
  });

  it('shows gated external skills on agent tabs with per-agent toggles', async () => {
    let agentBToggled = false;
    fetchMock.mockImplementation((url: string, opts?: RequestInit) => {
      if (url.includes('/api/agents/agent-b/skills/code-review')) {
        expect(opts?.method).toBe('PATCH');
        expect(JSON.parse(String(opts?.body || '{}'))).toEqual({ enabled: true });
        agentBToggled = true;
        return Promise.resolve(jsonResponse({ ok: true, enabled: ['code-review'], changed: ['code-review'] }));
      }
      if (url.includes('/api/skills/bundles')) {
        return Promise.resolve(jsonResponse({ bundles: [] }));
      }
      if (url.includes('/api/skills?agentId=agent-a')) {
        return Promise.resolve(jsonResponse({
          skills: [
            { name: 'reader', enabled: true, source: 'user', description: 'Read' },
            { name: 'code-review', enabled: true, source: 'external', description: 'Review with Codex', externalLabel: 'Codex', externalPath: '/home/u/.codex/skills' },
          ],
        }));
      }
      if (url.includes('/api/skills?agentId=agent-b')) {
        return Promise.resolve(jsonResponse({
          skills: [
            { name: 'reader', enabled: false, source: 'user', description: 'Read' },
            { name: 'code-review', enabled: agentBToggled, source: 'external', description: 'Review with Codex', externalLabel: 'Codex', externalPath: '/home/u/.codex/skills' },
          ],
        }));
      }
      return Promise.resolve(jsonResponse({ skills: [] }));
    });

    render(<SkillsPanel />);
    await flushMicrotasks(4);

    // 助手页：门槛内技能保持可见，开关取该助手自己的状态，可从关闭直接打开
    fireEvent.click(screen.getByRole('tab', { name: 'Mao' }));
    await screen.findByText('code-review');
    expect(screen.getByText('Codex')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '启用 code-review' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: '启用 code-review' }));

    await waitFor(() => expect(agentBToggled).toBe(true));
  });

  it('toggles a skill bundle from an agent tab with the same bundle API as settings', async () => {
    let bundleToggled = false;
    fetchMock.mockImplementation((url: string, opts?: RequestInit) => {
      if (url.includes('/api/agents/agent-b/skill-bundles/writing-bundle')) {
        expect(opts?.method).toBe('PATCH');
        expect(JSON.parse(String(opts?.body || '{}'))).toEqual({ enabled: true });
        bundleToggled = true;
        return Promise.resolve(jsonResponse({ ok: true, enabled: ['reader'], changed: ['reader'] }));
      }
      if (url.includes('/api/skills/bundles?agentId=agent-b')) {
        return Promise.resolve(jsonResponse({
          bundles: [
            { id: 'writing-bundle', name: 'Writing Bundle', skillNames: ['reader'] },
          ],
        }));
      }
      if (url.includes('/api/skills/bundles')) {
        return Promise.resolve(jsonResponse({ bundles: [] }));
      }
      if (url.includes('/api/skills?agentId=agent-b')) {
        return Promise.resolve(jsonResponse({
          skills: [
            { name: 'reader', enabled: bundleToggled, source: 'user', description: 'Read' },
          ],
        }));
      }
      if (url.includes('/api/skills?agentId=')) {
        return Promise.resolve(jsonResponse({ skills: [] }));
      }
      return Promise.resolve(jsonResponse({}));
    });

    render(<SkillsPanel />);
    await flushMicrotasks(4);

    fireEvent.click(screen.getByRole('tab', { name: 'Mao' }));
    await screen.findByText('Writing Bundle');

    fireEvent.click(screen.getByRole('button', { name: 'settings.skills.expandBundleAriaLabel' }));
    expect(screen.getByText('reader')).toBeTruthy();

    fireEvent.click(screen.getByTestId('skill-bundle-toggle-writing-bundle'));

    await waitFor(() => expect(bundleToggled).toBe(true));
    expect(screen.getByText('reader')).toBeTruthy();
    expect(screen.queryByText('status.loading')).toBeNull();
  });
});
