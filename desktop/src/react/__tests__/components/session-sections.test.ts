import { describe, expect, it } from 'vitest';
import type { Session } from '../../types';
import {
  buildSessionSections,
  filterSessionsForWorkspaceScope,
  resolveWorkspaceScope,
  sessionBelongsToWorkspaceScope,
} from '../../components/session-sections';

function makeSession(overrides: Partial<Session>): Session {
  return {
    path: '/sessions/default.jsonl',
    title: null,
    firstMessage: '',
    modified: '2026-04-29T01:00:00.000Z',
    messageCount: 1,
    agentId: 'hana',
    agentName: 'Hana',
    cwd: null,
    ...overrides,
  };
}

describe('buildSessionSections', () => {
  it('places pinned sessions first, sorts them by modified time, and excludes them from date sections', () => {
    const sections = buildSessionSections([
      makeSession({
        path: '/sessions/today.jsonl',
        firstMessage: 'today',
        modified: '2026-04-29T07:00:00.000Z',
      }),
      makeSession({
        path: '/sessions/recent-pin.jsonl',
        firstMessage: 'recent pin',
        modified: '2026-04-29T09:00:00.000Z',
        pinnedAt: '2026-04-28T07:00:00.000Z',
      }),
      makeSession({
        path: '/sessions/freshly-pinned-old-chat.jsonl',
        firstMessage: 'freshly pinned old chat',
        modified: '2026-04-28T07:00:00.000Z',
        pinnedAt: '2026-04-29T08:00:00.000Z',
      }),
    ], {
      mode: 'time',
      now: new Date('2026-04-29T12:00:00.000Z'),
    });

    expect(sections.map(section => section.kind)).toEqual(['pinned', 'date']);
    expect(sections[0]).toMatchObject({
      kind: 'pinned',
      titleKey: 'sidebar.pinned',
    });
    expect(sections[0].items.map(item => item.path)).toEqual([
      '/sessions/recent-pin.jsonl',
      '/sessions/freshly-pinned-old-chat.jsonl',
    ]);
    expect(sections[1]).toMatchObject({
      kind: 'date',
      titleKey: 'time.today',
    });
    expect(sections[1].items.map(item => item.path)).toEqual(['/sessions/today.jsonl']);
  });

  it('keeps the pinned section visible when no sessions are pinned and rolls yesterday into this week', () => {
    const sections = buildSessionSections([
      makeSession({
        path: '/sessions/yesterday.jsonl',
        modified: '2026-04-28T07:00:00.000Z',
      }),
    ], {
      mode: 'time',
      now: new Date('2026-04-29T12:00:00.000Z'),
    });

    expect(sections).toHaveLength(2);
    expect(sections[0]).toMatchObject({
      kind: 'pinned',
      titleKey: 'sidebar.pinned',
      items: [],
    });
    expect(sections[1]).toMatchObject({
      kind: 'date',
      titleKey: 'time.thisWeek',
    });
  });

  it('sorts sessions within a date group by modified descending', () => {
    const sections = buildSessionSections([
      makeSession({
        path: '/sessions/older.jsonl',
        firstMessage: 'older',
        modified: '2026-04-29T02:00:00.000Z',
      }),
      makeSession({
        path: '/sessions/newer.jsonl',
        firstMessage: 'newer',
        modified: '2026-04-29T09:00:00.000Z',
      }),
      makeSession({
        path: '/sessions/middle.jsonl',
        firstMessage: 'middle',
        modified: '2026-04-29T05:00:00.000Z',
      }),
    ], {
      mode: 'time',
      now: new Date('2026-04-29T12:00:00.000Z'),
    });

    const todaySection = sections.find(s => s.kind === 'date' && s.group === 'today');
    expect(todaySection).toBeDefined();
    expect(todaySection!.items.map(i => i.path)).toEqual([
      '/sessions/newer.jsonl',
      '/sessions/middle.jsonl',
      '/sessions/older.jsonl',
    ]);
  });

  it('uses a deterministic path tie-breaker and sinks malformed dates', () => {
    const sections = buildSessionSections([
      makeSession({
        path: '/sessions/z-same-time.jsonl',
        modified: '2026-04-29T09:00:00.000Z',
      }),
      makeSession({
        path: '/sessions/bad-date.jsonl',
        modified: 'not-a-date',
      }),
      makeSession({
        path: '/sessions/a-same-time.jsonl',
        modified: '2026-04-29T09:00:00.000Z',
      }),
    ], {
      mode: 'time',
      now: new Date('2026-04-29T12:00:00.000Z'),
    });

    const todaySection = sections.find(s => s.kind === 'date' && s.group === 'today');
    const earlierSection = sections.find(s => s.kind === 'date' && s.group === 'earlier');
    expect(todaySection!.items.map(i => i.path)).toEqual([
      '/sessions/a-same-time.jsonl',
      '/sessions/z-same-time.jsonl',
    ]);
    expect(earlierSection!.items.map(i => i.path)).toEqual(['/sessions/bad-date.jsonl']);
  });

  it('orders pinned sessions by their manual pin order, smallest first', () => {
    const sections = buildSessionSections([
      makeSession({
        path: '/sessions/second.jsonl',
        modified: '2026-04-29T09:00:00.000Z',
        pinnedAt: '2026-04-28T07:00:00.000Z',
        pinOrder: 2048,
      }),
      makeSession({
        path: '/sessions/first.jsonl',
        modified: '2026-04-28T01:00:00.000Z',
        pinnedAt: '2026-04-28T07:00:00.000Z',
        pinOrder: -1024,
      }),
      makeSession({
        path: '/sessions/third.jsonl',
        modified: '2026-04-29T10:00:00.000Z',
        pinnedAt: '2026-04-28T07:00:00.000Z',
        pinOrder: 4096,
      }),
    ], { mode: 'time', now: new Date('2026-04-29T12:00:00.000Z') });

    expect(sections[0].items.map(item => item.path)).toEqual([
      '/sessions/first.jsonl',
      '/sessions/second.jsonl',
      '/sessions/third.jsonl',
    ]);
  });

  it('keeps sessions without a manual order below the ordered ones, newest first', () => {
    const sections = buildSessionSections([
      makeSession({
        path: '/sessions/unordered-old.jsonl',
        modified: '2026-04-28T01:00:00.000Z',
        pinnedAt: '2026-04-28T07:00:00.000Z',
      }),
      makeSession({
        path: '/sessions/ordered.jsonl',
        modified: '2026-04-20T01:00:00.000Z',
        pinnedAt: '2026-04-28T07:00:00.000Z',
        pinOrder: 9999,
      }),
      makeSession({
        path: '/sessions/unordered-new.jsonl',
        modified: '2026-04-29T01:00:00.000Z',
        pinnedAt: '2026-04-28T07:00:00.000Z',
        pinOrder: null,
      }),
    ], { mode: 'time', now: new Date('2026-04-29T12:00:00.000Z') });

    expect(sections[0].items.map(item => item.path)).toEqual([
      '/sessions/ordered.jsonl',
      '/sessions/unordered-new.jsonl',
      '/sessions/unordered-old.jsonl',
    ]);
  });
});

describe('workspace scope', () => {
  it('matches sessions strictly by workspaceMountId for mount scopes', () => {
    const scope = { mountId: 'mount-abc', basePath: null };

    expect(sessionBelongsToWorkspaceScope({ cwd: null, workspaceMountId: 'mount-abc' }, scope)).toBe(true);
    expect(sessionBelongsToWorkspaceScope({ cwd: '/anywhere', workspaceMountId: 'mount-abc' }, scope)).toBe(true);
    expect(sessionBelongsToWorkspaceScope({ cwd: '/anywhere', workspaceMountId: 'mount-other' }, scope)).toBe(false);
    expect(sessionBelongsToWorkspaceScope({ cwd: '/anywhere', workspaceMountId: null }, scope)).toBe(false);
    expect(sessionBelongsToWorkspaceScope({ cwd: null, workspaceMountId: null }, scope)).toBe(false);
  });

  it('matches local-directory scopes through normalized cwd comparison', () => {
    const scope = { mountId: null, basePath: '/Users/test/Desktop/project-hana' };

    expect(sessionBelongsToWorkspaceScope({ cwd: '/Users/test/Desktop/project-hana', workspaceMountId: null }, scope)).toBe(true);
    // 尾斜杠归一
    expect(sessionBelongsToWorkspaceScope({ cwd: '/Users/test/Desktop/project-hana/', workspaceMountId: null }, scope)).toBe(true);
    // 反斜杠归一（Windows 风格）
    expect(sessionBelongsToWorkspaceScope({ cwd: 'C:\\Work\\Project', workspaceMountId: null }, { mountId: null, basePath: 'C:/Work/Project' })).toBe(true);
    // Windows 盘符大小写不敏感
    expect(sessionBelongsToWorkspaceScope({ cwd: 'c:/work/project', workspaceMountId: null }, { mountId: null, basePath: 'C:/Work/Project' })).toBe(true);
    // 大小写在 macOS/Linux 风格路径上敏感
    expect(sessionBelongsToWorkspaceScope({ cwd: '/Users/Test/Desktop/project-hana', workspaceMountId: null }, scope)).toBe(false);
    // 子目录不算同一工作台
    expect(sessionBelongsToWorkspaceScope({ cwd: '/Users/test/Desktop/project-hana/sub', workspaceMountId: null }, scope)).toBe(false);
  });

  it('keeps mount sessions out of local-directory scopes and vice versa', () => {
    const localScope = { mountId: null, basePath: '/Users/test/Desktop/project-hana' };
    const mountScope = { mountId: 'mount-abc', basePath: null };

    expect(sessionBelongsToWorkspaceScope({ cwd: '/Users/test/Desktop/project-hana', workspaceMountId: 'mount-abc' }, localScope)).toBe(false);
    expect(sessionBelongsToWorkspaceScope({ cwd: null, workspaceMountId: null }, mountScope)).toBe(false);
  });

  // ── 默认工作台双形态合流（Default = Agent 工作台目录的 mount 视图）──

  it('merges legacy cwd-form sessions into the default mount scope when its local root is known', () => {
    const scope = { mountId: 'default', basePath: null, defaultRootPath: '/Users/test/Desktop/Project/lingxidev' };

    // mount 形态（新入口）会话照常严格匹配
    expect(sessionBelongsToWorkspaceScope({ cwd: '/Users/test/Desktop/Project/lingxidev', workspaceMountId: 'default' }, scope)).toBe(true);
    // 历史 cwd 形态（同一目录的旧会话）合流进同一作用域
    expect(sessionBelongsToWorkspaceScope({ cwd: '/Users/test/Desktop/Project/lingxidev', workspaceMountId: null }, scope)).toBe(true);
    expect(sessionBelongsToWorkspaceScope({ cwd: '/Users/test/Desktop/Project/lingxidev/', workspaceMountId: null }, scope)).toBe(true);
    // 其他目录 / 其他 mount / 无身份不合流
    expect(sessionBelongsToWorkspaceScope({ cwd: '/Users/test/Desktop/other', workspaceMountId: null }, scope)).toBe(false);
    expect(sessionBelongsToWorkspaceScope({ cwd: '/Users/test/Desktop/Project/lingxidev', workspaceMountId: 'mount-other' }, scope)).toBe(false);
    expect(sessionBelongsToWorkspaceScope({ cwd: null, workspaceMountId: null }, scope)).toBe(false);
  });

  it('merges default-mount sessions into the matching local-directory scope when its local root is known', () => {
    const scope = { mountId: null, basePath: '/Users/test/Desktop/Project/lingxidev', defaultRootPath: '/Users/test/Desktop/Project/lingxidev' };

    expect(sessionBelongsToWorkspaceScope({ cwd: '/Users/test/Desktop/Project/lingxidev', workspaceMountId: 'default' }, scope)).toBe(true);
    // 其他 mount 不经由此路径混入本地作用域
    expect(sessionBelongsToWorkspaceScope({ cwd: '/Users/test/Desktop/Project/lingxidev', workspaceMountId: 'mount-other' }, scope)).toBe(false);
  });

  it('keeps non-default mounts strictly separated even when the default root is known', () => {
    const mountScope = { mountId: 'local_fs_probe', basePath: null, defaultRootPath: '/Users/test/Desktop/Project/lingxidev' };

    expect(sessionBelongsToWorkspaceScope({ cwd: '/Users/test/Desktop/Project/lingxidev', workspaceMountId: null }, mountScope)).toBe(false);
    expect(sessionBelongsToWorkspaceScope({ cwd: '/Users/test/Desktop/Project/lingxidev', workspaceMountId: 'default' }, mountScope)).toBe(false);
  });

  it('attaches the dual-form key only when the scope resolves to the default workspace', () => {
    const root = '/Users/test/Desktop/Project/lingxidev';
    // desk 在 default mount：携带合流键
    expect(resolveWorkspaceScope({
      currentSessionPath: '/sessions/live.jsonl',
      deskWorkspaceMountId: 'default',
      deskBasePath: 'studio:default',
      selectedWorkspaceMountId: null,
      selectedFolder: null,
      defaultWorkspaceRootPath: root,
    })).toEqual({ mountId: 'default', basePath: null, defaultRootPath: root });
    // desk 在默认工作台的本地路径形态：携带合流键
    expect(resolveWorkspaceScope({
      currentSessionPath: '/sessions/live.jsonl',
      deskWorkspaceMountId: null,
      deskBasePath: root,
      selectedWorkspaceMountId: null,
      selectedFolder: null,
      defaultWorkspaceRootPath: root,
    })).toEqual({ mountId: null, basePath: root, defaultRootPath: root });
    // 其他 mount / 其他目录：不带合流键（保持旧形状与严格语义）
    expect(resolveWorkspaceScope({
      currentSessionPath: '/sessions/live.jsonl',
      deskWorkspaceMountId: 'mount-abc',
      deskBasePath: 'studio:mount-abc',
      selectedWorkspaceMountId: null,
      selectedFolder: null,
      defaultWorkspaceRootPath: root,
    })).toEqual({ mountId: 'mount-abc', basePath: null });
    expect(resolveWorkspaceScope({
      currentSessionPath: '/sessions/live.jsonl',
      deskWorkspaceMountId: null,
      deskBasePath: '/Users/test/Desktop/other',
      selectedWorkspaceMountId: null,
      selectedFolder: null,
      defaultWorkspaceRootPath: root,
    })).toEqual({ mountId: null, basePath: '/Users/test/Desktop/other' });
    // 未提供默认根路径：不改变既有形状
    expect(resolveWorkspaceScope({
      currentSessionPath: null,
      deskWorkspaceMountId: null,
      deskBasePath: null,
      selectedWorkspaceMountId: 'default',
      selectedFolder: null,
    })).toEqual({ mountId: 'default', basePath: null });
  });

  it('never shows sessions without a reliable identity in any scope', () => {
    const noIdentity = { cwd: null, workspaceMountId: null };
    expect(sessionBelongsToWorkspaceScope(noIdentity, { mountId: 'mount-abc', basePath: null })).toBe(false);
    expect(sessionBelongsToWorkspaceScope(noIdentity, { mountId: null, basePath: '/Users/test/Desktop/project-hana' })).toBe(false);
    expect(sessionBelongsToWorkspaceScope(noIdentity, { mountId: null, basePath: null })).toBe(false);
  });

  it('filters a full session list down to the scoped sessions', () => {
    const sessions = [
      makeSession({ path: '/sessions/local.jsonl', cwd: '/Users/test/Desktop/project-hana' }),
      makeSession({ path: '/sessions/local-trailing.jsonl', cwd: '/Users/test/Desktop/project-hana/' }),
      makeSession({ path: '/sessions/other-dir.jsonl', cwd: '/Users/test/Desktop/other' }),
      makeSession({ path: '/sessions/mount.jsonl', cwd: null, workspaceMountId: 'mount-abc' }),
      makeSession({ path: '/sessions/identity-less.jsonl', cwd: null }),
    ];

    expect(filterSessionsForWorkspaceScope(sessions, { mountId: null, basePath: '/Users/test/Desktop/project-hana' }).map(s => s.path))
      .toEqual(['/sessions/local.jsonl', '/sessions/local-trailing.jsonl']);
    expect(filterSessionsForWorkspaceScope(sessions, { mountId: 'mount-abc', basePath: null }).map(s => s.path))
      .toEqual(['/sessions/mount.jsonl']);
  });

  it('resolves the scope from the desk identity, or from the pending new-session target', () => {
    // 有当前会话：以 desk 为准（mount 优先）
    expect(resolveWorkspaceScope({
      currentSessionPath: '/sessions/live.jsonl',
      deskWorkspaceMountId: 'mount-abc',
      deskBasePath: 'studio:mount-abc',
      selectedWorkspaceMountId: null,
      selectedFolder: '/elsewhere',
    })).toEqual({ mountId: 'mount-abc', basePath: null });

    // 有当前会话：本地目录 desk
    expect(resolveWorkspaceScope({
      currentSessionPath: '/sessions/live.jsonl',
      deskWorkspaceMountId: null,
      deskBasePath: '/Users/test/Desktop/project-hana',
      selectedWorkspaceMountId: null,
      selectedFolder: '/elsewhere',
    })).toEqual({ mountId: null, basePath: '/Users/test/Desktop/project-hana' });

    // pending 新会话：优先 selectedWorkspaceMountId / selectedFolder
    expect(resolveWorkspaceScope({
      currentSessionPath: null,
      deskWorkspaceMountId: null,
      deskBasePath: '/old',
      selectedWorkspaceMountId: 'mount-pending',
      selectedFolder: null,
    })).toEqual({ mountId: 'mount-pending', basePath: null });
    expect(resolveWorkspaceScope({
      currentSessionPath: null,
      deskWorkspaceMountId: null,
      deskBasePath: '/old',
      selectedWorkspaceMountId: null,
      selectedFolder: '/Users/test/Desktop/pending',
    })).toEqual({ mountId: null, basePath: '/Users/test/Desktop/pending' });

    // pending 但 selected 未落地：退回 desk 身份
    expect(resolveWorkspaceScope({
      currentSessionPath: null,
      deskWorkspaceMountId: null,
      deskBasePath: '/Users/test/Desktop/project-hana',
      selectedWorkspaceMountId: null,
      selectedFolder: null,
    })).toEqual({ mountId: null, basePath: '/Users/test/Desktop/project-hana' });
  });
});
