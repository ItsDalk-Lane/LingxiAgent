// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ComposerToolbar } from '../../components/input/ComposerToolbar';
import inputStyles from '../../components/input/InputArea.module.css';

vi.mock('../../components/input/PlanModeButton', () => ({
  PlanModeButton: (props: { mode: string; locked: boolean }) => React.createElement(
    'button',
    { type: 'button', 'data-testid': 'plan-mode', 'data-mode': props.mode, 'data-locked': String(props.locked) },
    'plan',
  ),
}));

vi.mock('../../components/input/ContextRing', () => ({
  ContextRing: () => React.createElement('span', { 'data-testid': 'context-ring' }, 'context'),
}));

vi.mock('../../components/input/ThinkingLevelButton', () => ({
  ThinkingLevelButton: () => React.createElement('button', { type: 'button', 'data-testid': 'thinking-level' }, 'thinking'),
}));

vi.mock('../../components/input/ModelSelector', () => ({
  ModelSelector: () => React.createElement('button', { type: 'button', 'data-testid': 'model-selector' }, 'model'),
}));

function buildProps(overrides: Partial<React.ComponentProps<typeof ComposerToolbar>> = {}): React.ComponentProps<typeof ComposerToolbar> {
  return {
    t: (key) => key,
    onNewSession: vi.fn(),
    onAttach: vi.fn(),
    slashBtnRef: { current: null },
    onSlashToggle: vi.fn(),
    permissionMode: 'ask',
    onPermissionModeChange: vi.fn(),
    planModeLocked: false,
    showThinking: false,
    thinkingLevel: 'auto',
    onThinkingChange: vi.fn(),
    availableThinkingLevels: ['off', 'medium', 'high'],
    models: [],
    sessionModel: undefined,
    isStreaming: false,
    showAudioInput: false,
    audioRecordingActive: false,
    audioRecordingBusy: false,
    onAudioToggle: vi.fn(),
    ...overrides,
  };
}

function renderBar(overrides: Partial<React.ComponentProps<typeof ComposerToolbar>> = {}) {
  return render(<ComposerToolbar {...buildProps(overrides)} />);
}

describe('ComposerToolbar', () => {
  afterEach(() => cleanup());

  it('calls the new-session handler from the first toolbar button with the new-chat title', () => {
    const onNewSession = vi.fn();
    renderBar({ onNewSession });

    const button = screen.getByTitle('sidebar.newChat');
    fireEvent.click(button);

    expect(onNewSession).toHaveBeenCalledTimes(1);
    // 新建聊天必须排在附件按钮之前
    const attach = screen.getByTitle('input.attachFiles');
    expect(button.compareDocumentPosition(attach) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('uses a message-semantics icon instead of a plain plus for the new-chat button', () => {
    renderBar();

    const button = screen.getByTitle('sidebar.newChat');
    const paths = Array.from(button.querySelectorAll('path')).map(p => p.getAttribute('d'));
    // message-square 外框 + 内部加号两笔；不再是单纯的「+」
    expect(paths).toContain('M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z');
    expect(paths.length).toBeGreaterThan(1);
  });

  it('keeps the attach behavior with a paperclip icon', () => {
    const onAttach = vi.fn();
    renderBar({ onAttach });

    const button = screen.getByTitle('input.attachFiles');
    expect(button.querySelector('path')?.getAttribute('d'))
      .toBe('m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48');

    fireEvent.click(button);
    expect(onAttach).toHaveBeenCalledTimes(1);
  });

  it('toggles the slash menu and binds the slash button ref', () => {
    const onSlashToggle = vi.fn();
    const slashBtnRef = { current: null as HTMLButtonElement | null };
    renderBar({ onSlashToggle, slashBtnRef });

    const button = screen.getByTitle('input.commandMenu');
    fireEvent.click(button);

    expect(onSlashToggle).toHaveBeenCalledTimes(1);
    expect(slashBtnRef.current).toBe(button);
  });

  it('renders permission/plan-mode and context controls with their props intact', () => {
    renderBar({ permissionMode: 'operate', planModeLocked: true });

    const plan = screen.getByTestId('plan-mode');
    expect(plan.getAttribute('data-mode')).toBe('operate');
    expect(plan.getAttribute('data-locked')).toBe('true');
    expect(screen.getByTestId('context-ring')).toBeTruthy();
  });

  it('shows the thinking/model split control only when thinking is supported', () => {
    const { rerender } = renderBar({ showThinking: true });
    expect(screen.getByTestId('thinking-level')).toBeTruthy();
    expect(screen.getByTestId('model-selector')).toBeTruthy();

    rerender(<ComposerToolbar {...buildProps({ showThinking: false })} />);

    expect(screen.queryByTestId('thinking-level')).toBeNull();
    expect(screen.getByTestId('model-selector')).toBeTruthy();
  });

  it('does not render the send control inside the toolbar', () => {
    const { container } = renderBar();

    expect(container.querySelector(`.${inputStyles['send-btn']}`)).toBeNull();
  });

  it('hides the audio button when audio input is unsupported', () => {
    renderBar({ showAudioInput: false });

    expect(screen.queryByLabelText('input.recordAudio')).toBeNull();
  });

  it('shows the audio button and calls the toggle handler when audio input is supported', () => {
    const onAudioToggle = vi.fn();
    renderBar({ showAudioInput: true, onAudioToggle });

    const button = screen.getByLabelText('input.recordAudio');
    fireEvent.click(button);

    expect(onAudioToggle).toHaveBeenCalledTimes(1);
  });

  it('switches the audio button label while recording', () => {
    renderBar({ showAudioInput: true, audioRecordingActive: true });

    expect(screen.getByLabelText('input.stopRecording')).toBeTruthy();
  });
});
