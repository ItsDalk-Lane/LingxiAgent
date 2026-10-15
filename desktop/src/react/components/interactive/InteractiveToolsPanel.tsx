import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useStore } from '../../stores';
import { captureSessionConnection, ownsSessionConnection, sessionConnectionRevision, sessionSelectionRevision, sessionOwnerFetch, subscribeSessionConnection, type SessionConnectionOwner } from '../../services/session-connection-owner';
import { QuestionCard, type QuestionRequest } from './QuestionCard';
import { SessionTodoList, type RuntimeTodo } from './SessionTodoList';
import { SessionCard, type RuntimeCard } from './SessionCard';
import type { AskUserValues } from '../input/ask-user-schema';
interface View { owner: SessionConnectionOwner; sessionId: string; selection: number; questions: QuestionRequest[]; todos: RuntimeTodo[]; cards: RuntimeCard[] }
function readView(value: unknown): { questions: QuestionRequest[]; cards: RuntimeCard[]; todos: RuntimeTodo[] } {
  const data = value as { questions?: QuestionRequest[]; cards?: RuntimeCard[]; todoPanel?: { todos?: RuntimeTodo[] } };
  if (!data || !Array.isArray(data.questions) || !Array.isArray(data.cards) || !Array.isArray(data.todoPanel?.todos)) throw new Error('会话交互数据无效');
  if (data.questions.some(q => !q || typeof q.id !== 'string' || !Array.isArray(q.questions) || q.questions.some(x => typeof x.key !== 'string' || typeof x.question !== 'string' || !['single', 'multi', 'text'].includes(x.type) || !Array.isArray(x.options) || !Array.isArray(x.recommended)))) throw new Error('提问格式无效');
  if (data.cards.some(c => !c || typeof c.cardId !== 'string' || typeof c.title !== 'string' || typeof c.code !== 'string')) throw new Error('卡片格式无效');
  if (data.todoPanel.todos.some(t => !t || typeof t.id !== 'string' || typeof t.content !== 'string' || !['pending', 'in_progress', 'blocked', 'cancelled', 'completed'].includes(t.status))) throw new Error('任务清单格式无效');
  return { questions: data.questions, cards: data.cards, todos: data.todoPanel.todos };
}
/** 每次请求和展示都绑定当前连接与会话代际，旧请求不能写进新会话。 */
export function InteractiveToolsPanel() {
  const sessionId = useStore(state => state.currentSessionId);
  const revision = useSyncExternalStore(subscribeSessionConnection, sessionConnectionRevision);
  const selection = useSyncExternalStore(subscribeSessionConnection, sessionSelectionRevision);
  const [view, setView] = useState<View | null>(null);
  const [error, setError] = useState<{ owner: SessionConnectionOwner; sessionId: string; selection: number; text: string } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const refreshRef = useRef<(() => Promise<void>) | null>(null);
  const visible = view && view.sessionId === sessionId && view.selection === selection && ownsSessionConnection(view.owner) ? view : null;
  useEffect(() => {
    const owner = captureSessionConnection();
    if (!sessionId || !owner || owner.connection.serverNodeKind !== 'lingxi-service') return;
    const targetSelection = sessionSelectionRevision(); const abort = new AbortController();
    let alive = true; let waiting = false;
    const current = () => alive && ownsSessionConnection(owner) && useStore.getState().currentSessionId === sessionId && sessionSelectionRevision() === targetSelection;
    setBusy(null); setError(null);
    const refresh = async () => {
      if (!current() || waiting) return; waiting = true;
      try {
        const response = await sessionOwnerFetch(owner, `/lingxi/v1/sessions/interactions?sessionId=${encodeURIComponent(sessionId)}`, { signal: abort.signal });
        const raw = await response.json(); if (raw.sessionId !== sessionId) throw new Error('会话身份不一致');
        const data = readView(raw);
        if (current()) { setView(previous => ({ owner: previous?.owner.key === owner.key && previous.owner.revision === owner.revision ? previous.owner : owner, sessionId, selection: targetSelection, ...data })); setError(null); }
      } catch (failure) { if (current()) setError({ owner, sessionId, selection: targetSelection, text: String(failure) }); }
      finally { waiting = false; }
    };
    refreshRef.current = refresh; void refresh(); const timer = setInterval(() => void refresh(), 600);
    return () => { alive = false; abort.abort(); clearInterval(timer); refreshRef.current = null; };
  }, [sessionId, revision, selection]);
  const answer = async (id: string, action: 'confirmed' | 'dismissed', values: AskUserValues) => {
    if (!visible || busy) return;
    const target = visible;
    const current = () => ownsSessionConnection(target.owner) && useStore.getState().currentSessionId === target.sessionId && sessionSelectionRevision() === target.selection;
    if (!current()) return; setBusy(id);
    try {
      await sessionOwnerFetch(target.owner, '/lingxi/v1/sessions/interactions/answer', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ sessionId: target.sessionId, id, action, values }) });
      if (current()) { setView(old => old === target ? { ...old, questions: old.questions.filter(q => q.id !== id) } : old); await refreshRef.current?.(); }
    } catch (failure) { if (current()) setError({ owner: target.owner, sessionId: target.sessionId, selection: target.selection, text: String(failure) }); }
    finally { if (current()) setBusy(null); }
  };
  const visibleError = error && error.sessionId === sessionId && error.selection === selection && ownsSessionConnection(error.owner) ? error.text : '';
  if (!visible && !visibleError) return null;
  return <div data-testid="interactive-tools-panel">
    {visibleError && <p role="alert">{visibleError}</p>}
    {visible?.questions.map(q => <QuestionCard key={q.id} request={q} busy={busy === q.id} answer={(action, values) => answer(q.id, action, values)} />)}
    {visible && <SessionTodoList todos={visible.todos} />}
    {visible?.cards.map(card => <SessionCard key={`${visible.selection}:${card.cardId}`} card={card} owner={visible.owner} />)}
  </div>;
}
