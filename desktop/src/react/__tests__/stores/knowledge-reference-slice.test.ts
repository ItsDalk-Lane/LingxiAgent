import { describe, it, expect, beforeEach } from 'vitest';
import {
  createKnowledgeReferenceSlice,
  selectKnowledgeRefsForSession,
  type KnowledgeReferenceSlice,
} from '../../stores/knowledge-reference-slice';

type SliceState = KnowledgeReferenceSlice & {
  sessions?: Array<{ sessionId?: string | null; path?: string | null }>;
  sessionLocatorsById?: Record<string, { path: string | null }>;
};

function makeSlice(initial?: Partial<SliceState>): SliceState {
  let state: SliceState;
  const set = (partial: Partial<KnowledgeReferenceSlice> | ((s: KnowledgeReferenceSlice) => Partial<KnowledgeReferenceSlice>)) => {
    const patch = typeof partial === 'function' ? partial(state) : partial;
    state = { ...state, ...patch };
  };
  state = { ...createKnowledgeReferenceSlice(set), ...initial };
  return new Proxy({} as SliceState, {
    get: (_, key: string) => (state as unknown as Record<string, unknown>)[key],
  });
}

describe('knowledge-reference-slice', () => {
  let slice: SliceState;
  beforeEach(() => { slice = makeSlice(); });

  it('初始状态无任何引用', () => {
    expect(slice.knowledgeRefsBySession).toEqual({});
    expect(selectKnowledgeRefsForSession(slice as never, '/session/a')).toBeNull();
    expect(selectKnowledgeRefsForSession(slice as never, null)).toBeNull();
  });

  it('toggle 添加引用，默认问答模式，并缓存名称', () => {
    slice.toggleKnowledgeNotebook('/session/a', 'nb-1', '笔记本一');
    expect(slice.knowledgeRefsBySession['/session/a']).toEqual({
      notebookIds: ['nb-1'],
      notebookNames: { 'nb-1': '笔记本一' },
      mode: 'qa',
    });
  });

  it('再次 toggle 同一笔记本即取消引用；清空后整条记录删除', () => {
    slice.toggleKnowledgeNotebook('/session/a', 'nb-1', '笔记本一');
    slice.toggleKnowledgeNotebook('/session/a', 'nb-1');
    expect(slice.knowledgeRefsBySession['/session/a']).toBeUndefined();
  });

  it('支持同时引用多个笔记本，toggle 只影响目标项', () => {
    slice.toggleKnowledgeNotebook('/session/a', 'nb-1', '一');
    slice.toggleKnowledgeNotebook('/session/a', 'nb-2', '二');
    expect(slice.knowledgeRefsBySession['/session/a'].notebookIds).toEqual(['nb-1', 'nb-2']);
    slice.toggleKnowledgeNotebook('/session/a', 'nb-1');
    expect(slice.knowledgeRefsBySession['/session/a'].notebookIds).toEqual(['nb-2']);
    expect(slice.knowledgeRefsBySession['/session/a'].notebookNames).toEqual({ 'nb-2': '二' });
  });

  it('removeKnowledgeNotebook 移除指定引用并清掉名称缓存', () => {
    slice.toggleKnowledgeNotebook('/session/a', 'nb-1', '一');
    slice.toggleKnowledgeNotebook('/session/a', 'nb-2', '二');
    slice.removeKnowledgeNotebook('/session/a', 'nb-1');
    expect(slice.knowledgeRefsBySession['/session/a']).toEqual({
      notebookIds: ['nb-2'],
      notebookNames: { 'nb-2': '二' },
      mode: 'qa',
    });
    slice.removeKnowledgeNotebook('/session/a', 'nb-2');
    expect(slice.knowledgeRefsBySession['/session/a']).toBeUndefined();
  });

  it('setKnowledgeReferenceMode 在问答/辅助之间切换，且保留引用', () => {
    slice.toggleKnowledgeNotebook('/session/a', 'nb-1');
    slice.setKnowledgeReferenceMode('/session/a', 'assist');
    expect(slice.knowledgeRefsBySession['/session/a'].mode).toBe('assist');
    expect(slice.knowledgeRefsBySession['/session/a'].notebookIds).toEqual(['nb-1']);
    slice.setKnowledgeReferenceMode('/session/a', 'qa');
    expect(slice.knowledgeRefsBySession['/session/a'].mode).toBe('qa');
  });

  it('无引用时 setMode / remove 是空操作，不创建记录', () => {
    slice.setKnowledgeReferenceMode('/session/a', 'assist');
    slice.removeKnowledgeNotebook('/session/a', 'nb-x');
    expect(slice.knowledgeRefsBySession).toEqual({});
  });

  it('clearKnowledgeReferences 清空指定会话全部引用', () => {
    slice.toggleKnowledgeNotebook('/session/a', 'nb-1');
    slice.toggleKnowledgeNotebook('/session/a', 'nb-2');
    slice.clearKnowledgeReferences('/session/a');
    expect(slice.knowledgeRefsBySession['/session/a']).toBeUndefined();
  });

  it('按 session 隔离：不同会话的引用互不影响', () => {
    slice.toggleKnowledgeNotebook('/session/a', 'nb-1', '一');
    slice.toggleKnowledgeNotebook('/session/b', 'nb-2', '二');
    slice.setKnowledgeReferenceMode('/session/b', 'assist');
    expect(selectKnowledgeRefsForSession(slice as never, '/session/a')).toEqual({
      notebookIds: ['nb-1'],
      notebookNames: { 'nb-1': '一' },
      mode: 'qa',
    });
    expect(selectKnowledgeRefsForSession(slice as never, '/session/b')).toEqual({
      notebookIds: ['nb-2'],
      notebookNames: { 'nb-2': '二' },
      mode: 'assist',
    });
    slice.clearKnowledgeReferences('/session/a');
    expect(selectKnowledgeRefsForSession(slice as never, '/session/a')).toBeNull();
    expect(selectKnowledgeRefsForSession(slice as never, '/session/b')?.notebookIds).toEqual(['nb-2']);
  });

  it('有 sessionId 定位时按 sessionId 落键，用 path 读写同一条记录', () => {
    const keyed = makeSlice({
      sessions: [{ sessionId: 'sid-a', path: '/session/a' }],
      sessionLocatorsById: { 'sid-a': { path: '/session/a' } },
    });
    keyed.toggleKnowledgeNotebook('/session/a', 'nb-1', '一');
    expect(keyed.knowledgeRefsBySession['sid-a']?.notebookIds).toEqual(['nb-1']);
    expect(keyed.knowledgeRefsBySession['/session/a']).toBeUndefined();
    expect(selectKnowledgeRefsForSession(keyed as never, '/session/a')?.notebookIds).toEqual(['nb-1']);
    keyed.removeKnowledgeNotebook('/session/a', 'nb-1');
    expect(keyed.knowledgeRefsBySession['sid-a']).toBeUndefined();
  });
});
