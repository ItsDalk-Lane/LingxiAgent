import { AsyncLocalStorage } from 'node:async_hooks';

interface CommitScope { session: any; committed: (entryId: string) => void; unavailable: () => void }
const scopeStorage = new AsyncLocalStorage<CommitScope>();
const installed = new WeakSet<object>();
let runtimeRevision = 0;

/** 只作读取期间的 ABA 检测；跨进程重启由客户端连接代次隔离。 */
export function noteDesktopInputRuntimeChange(): void { runtimeRevision += 1; }
export function desktopInputRuntimeRevision(): number { return runtimeRevision; }
export function hasDesktopInputCommitObserver(session: any): boolean { return !!session && installed.has(session); }
export function withDesktopInputCommitted<T>(scope: CommitScope, action: () => T): T {
  return scopeStorage.run(scope, action);
}

/** 本项目 SDK 包装：按输入对象身份关联，不改 SDK 消息或 append 实现。 */
export function installDesktopInputCommitObserver(session: any): void {
  const agent = session?.agent;
  if (!agent || typeof agent.prompt !== 'function' || typeof agent.subscribe !== 'function' || installed.has(session)) return;
  installed.add(session);
  const pending = new WeakMap<object, CommitScope>();
  // SDK 构造时已先订阅；agent 按注册顺序 await，轮到此处时 append 已返回。
  const unsubscribe = agent.subscribe((event: any) => {
    if (event.type === 'agent_start' || event.type === 'agent_end') noteDesktopInputRuntimeChange();
    const message = event?.message;
    if (event.type !== 'message_end' || message?.role !== 'user') return;
    const scope = pending.get(message);
    if (!scope) return;
    pending.delete(message);
    try {
      const matches = session.sessionManager.getBranch().filter((entry: any) => entry.type === 'message' && entry.message === message);
      if (matches.length !== 1 || typeof matches[0].id !== 'string') { scope.unavailable(); return; }
      scope.committed(matches[0].id);
    } catch { scope.unavailable(); }
  });
  const remember = (input: any): object | null => {
    const scope = scopeStorage.getStore();
    if (!scope || scope.session !== session) return null;
    const users = (Array.isArray(input) ? input : [input]).filter(message => message?.role === 'user');
    if (users.length !== 1) { scope.unavailable(); return null; }
    pending.set(users[0], scope);
    return users[0];
  };
  const originalPrompt = agent.prompt;
  agent.prompt = async function (...args: any[]) {
    const message = remember(args[0]);
    try { return await originalPrompt.apply(this, args); }
    finally { if (message) pending.delete(message); }
  };
  if (typeof agent.steer === 'function') {
    const originalSteer = agent.steer;
    agent.steer = function (...args: any[]) {
      const message = remember(args[0]);
      try { return originalSteer.apply(this, args); }
      catch (error) { if (message) pending.delete(message); throw error; }
    };
  }
  const originalSessionPrompt = session.prompt;
  session.prompt = async function (...args: any[]) {
    noteDesktopInputRuntimeChange();
    try { return await originalSessionPrompt.apply(this, args); }
    finally { noteDesktopInputRuntimeChange(); }
  };
  const originalDispose = session.dispose;
  session.dispose = function (...args: any[]) {
    unsubscribe();
    return originalDispose?.apply(this, args);
  };
}
