// @vitest-environment jsdom
//
// 「回退时撤销文件改动」聊天侧 UI 契约：
// - 重新生成入口打开两选项小菜单；第二个选项的可用性由服务端预览裁决
// - 开关关闭 / 无检查点 → 置灰并给出原因；有检查点 → 标注影响文件数
// - 选「回退对话+撤销文件改动」先弹文件清单确认（含覆盖警告），确认才带 fileRollback=workspace
// - 完成后报告横幅逐文件展示成功/失败/来源，可关闭

import '@testing-library/jest-dom/vitest';
import React from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { previewWorkspaceRollbackMock, retrySessionTurnMock, forkSessionTurnMock } = vi.hoisted(() => ({
  previewWorkspaceRollbackMock: vi.fn(),
  retrySessionTurnMock: vi.fn(async () => true),
  forkSessionTurnMock: vi.fn(),
}));

vi.mock('../../../stores/message-turn-actions', () => ({
  activateForkedSession: vi.fn(),
  forkSessionTurn: (...args: any[]) => (forkSessionTurnMock as any)(...args),
  previewWorkspaceRollback: (...args: any[]) => (previewWorkspaceRollbackMock as any)(...args),
  retrySessionTurn: (...args: any[]) => (retrySessionTurnMock as any)(...args),
}));

import { useSessionNodeActions } from '../../../components/chat/SessionNodeActions';
import { FileRollbackReportBanner } from '../../../components/chat/FileRollbackReport';
import { useStore } from '../../../stores';

const SESSION = '/chat/rollback.jsonl';

const TRANSLATIONS: Record<string, string> = {
  'common.regenerate': '重新生成',
  'common.forkSession': '分支为新会话',
  'common.cancel': '取消',
  'chat.fileRollback.conversationOnly': '仅回退对话',
  'chat.fileRollback.withFiles': '回退对话+撤销文件改动',
  'chat.fileRollback.withFilesCount': '回退对话+撤销文件改动（{count} 个文件）',
  'chat.fileRollback.withFilesDegraded': '回退对话+撤销文件改动（快照失败，用备份兜底）',
  'chat.fileRollback.disabledOff': '需先在设置里开启',
  'chat.fileRollback.disabledNoCheckpoint': '该轮没有文件检查点',
  'chat.fileRollback.disabledUnavailable': '工作区检查点不可用',
  'chat.fileRollback.confirmTitle': '回退对话并撤销文件改动',
  'chat.fileRollback.confirmBody': '以下 {count} 个文件会被覆盖：',
  'chat.fileRollback.confirmWarning': '期间所有改动（含你手动改的）都会被覆盖',
  'chat.fileRollback.confirmAction': '确认回退',
  'chat.fileRollback.reportTitle': '文件回退报告',
  'chat.fileRollback.reportAllOk': '{count} 个文件已恢复',
  'chat.fileRollback.reportPartial': '{failed} 个失败，{count} 个成功',
  'chat.fileRollback.reportDegraded': '该轮快照不可用，已用改前备份倒推',
  'chat.fileRollback.reportSourceBackup': '来源：改前备份',
  'chat.fileRollback.dismiss': '知道了',
};

function Harness() {
  const { actions, overlay } = useSessionNodeActions({
    sessionPath: SESSION,
    target: { role: 'user', entryId: 'entry-u1' },
  });
  return (
    <div>
      {actions.map((action) => (
        <button key={action.id} data-testid={`action-${action.id}`} onClick={action.onClick as never}>
          {action.title}
        </button>
      ))}
      {overlay}
    </div>
  );
}

function preview(overrides: Record<string, unknown> = {}) {
  return {
    enabled: true,
    available: true,
    degraded: false,
    commit: 'commit-1',
    reason: null,
    turnInputEntryId: 'entry-u1',
    files: [{ path: 'a.txt', status: 'M' }, { path: 'b.txt', status: 'A' }],
    fileCount: 2,
    ...overrides,
  };
}

async function openMenu() {
  render(<Harness />);
  fireEvent.click(screen.getByTestId('action-regenerate'));
  await waitFor(() => expect(screen.getByText('仅回退对话')).toBeInTheDocument());
}

beforeEach(() => {
  previewWorkspaceRollbackMock.mockReset();
  retrySessionTurnMock.mockClear();
  forkSessionTurnMock.mockClear();
  (window as any).t = (key: string, vars?: Record<string, string | number>) => {
    let value = TRANSLATIONS[key] ?? key;
    if (vars) {
      for (const [name, replacement] of Object.entries(vars)) {
        value = value.replaceAll(`{${name}}`, String(replacement));
      }
    }
    return value;
  };
});

afterEach(() => {
  cleanup();
  useStore.getState().clearFileRollbackReport?.(SESSION);
});

describe('SessionNodeActions 回退选项菜单', () => {
  it('开关关闭时界面不出现文件回退选项', async () => {
    previewWorkspaceRollbackMock.mockResolvedValue(preview({
      enabled: false,
      available: false,
      reason: 'file_rollback_disabled',
      files: [],
      fileCount: 0,
    }));

    await openMenu();

    expect(screen.getByText('仅回退对话')).toBeInTheDocument();
    expect(screen.queryByText(/回退对话\+撤销文件改动/)).toBeNull();
  });

  it('无检查点时置灰并说明原因', async () => {
    previewWorkspaceRollbackMock.mockResolvedValue(preview({
      available: false,
      reason: 'no_checkpoint',
      files: [],
      fileCount: 0,
    }));

    await openMenu();

    expect(screen.getByText(/该轮没有文件检查点/)).toBeTruthy();
    expect(screen.getByText(/该轮没有文件检查点/).closest('.context-menu-item')?.className).toContain('disabled');
  });

  it('有检查点时标注影响文件数，确认后带 fileRollback=workspace', async () => {
    previewWorkspaceRollbackMock.mockResolvedValue(preview());

    await openMenu();
    const option = screen.getByText('回退对话+撤销文件改动（2 个文件）');
    expect(option.closest('.context-menu-item')?.className).not.toContain('disabled');

    fireEvent.click(option);
    await waitFor(() => expect(screen.getByText('回退对话并撤销文件改动')).toBeInTheDocument());
    expect(screen.getByTestId('file-rollback-confirm-list').textContent).toContain('a.txt');
    expect(screen.getByTestId('file-rollback-confirm-list').textContent).toContain('b.txt');
    expect(screen.getByTestId('file-rollback-confirm-warning').textContent).toContain('期间所有改动');
    expect(retrySessionTurnMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText('确认回退'));
    await waitFor(() => expect(retrySessionTurnMock).toHaveBeenCalledWith(
      SESSION,
      { role: 'user', entryId: 'entry-u1' },
      { fileRollback: 'workspace' },
    ));
  });

  it('默认选项「仅回退对话」保持现状调用形状（不带 fileRollback）', async () => {
    previewWorkspaceRollbackMock.mockResolvedValue(preview());

    await openMenu();
    fireEvent.click(screen.getByText('仅回退对话'));

    await waitFor(() => expect(retrySessionTurnMock).toHaveBeenCalledWith(
      SESSION,
      { role: 'user', entryId: 'entry-u1' },
    ));
  });

  it('快照降级时选项仍可用，但提示用备份兜底', async () => {
    previewWorkspaceRollbackMock.mockResolvedValue(preview({
      degraded: true,
      commit: null,
      reason: 'snapshot_degraded',
      fileCount: 0,
      files: [],
    }));

    await openMenu();
    const option = screen.getByText('回退对话+撤销文件改动（快照失败，用备份兜底）');
    expect(option.closest('.context-menu-item')?.className).not.toContain('disabled');
  });
});

describe('FileRollbackReportBanner', () => {
  it('逐文件展示成功与失败，并可关闭', async () => {
    useStore.getState().setFileRollbackReport(SESSION, {
      ok: false,
      reason: 'partial_failure',
      degraded: false,
      commit: 'commit-1',
      turnInputEntryId: 'entry-u1',
      files: [
        { path: 'ok.txt', change: 'modified', action: 'restored', source: 'snapshot', ok: true },
        { path: 'bad.txt', change: 'added', action: 'failed', source: 'snapshot', ok: false, reason: 'permission denied' },
      ],
      failures: [
        { path: 'bad.txt', change: 'added', action: 'failed', source: 'snapshot', ok: false, reason: 'permission denied' },
      ],
    });

    render(<FileRollbackReportBanner sessionPath={SESSION} />);

    expect(screen.getByTestId('file-rollback-report')).toBeInTheDocument();
    expect(screen.getByText('1 个失败，1 个成功')).toBeTruthy();
    expect(screen.getByText('ok.txt')).toBeTruthy();
    expect(screen.getByText('permission denied')).toBeTruthy();

    fireEvent.click(screen.getByText('知道了'));
    await waitFor(() => expect(screen.queryByTestId('file-rollback-report')).not.toBeInTheDocument());
  });

  it('降级报告标注备份来源', () => {
    useStore.getState().setFileRollbackReport(SESSION, {
      ok: true,
      reason: null,
      degraded: true,
      commit: null,
      turnInputEntryId: 'entry-u1',
      files: [{ path: 'edit.txt', change: 'modified', action: 'restored', source: 'backup', ok: true }],
      failures: [],
    });

    render(<FileRollbackReportBanner sessionPath={SESSION} />);

    expect(screen.getByTestId('file-rollback-report-degraded').textContent).toContain('改前备份倒推');
    expect(screen.getByText('来源：改前备份')).toBeTruthy();
    expect(screen.getByText('1 个文件已恢复')).toBeTruthy();
  });
});
