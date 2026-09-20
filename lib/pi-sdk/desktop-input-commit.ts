import { AsyncLocalStorage } from 'node:async_hooks';

interface CommitScope {
  session: any;
  /** 真正把输入交给底层执行器，后续没有回执也不能推断成未接受。 */
  handedOff?: () => void;
  /** 插入已被底层队列接收；在让出执行权前保存仍需前置于 user 的展示元数据。 */
  queued?: () => void;
  committed: (entryId: string) => void;
  unavailable: () => void;
}
const scopeStorage = new AsyncLocalStorage<CommitScope>();
const installed = new WeakSet<object>();
/** steer 入队的 user 消息对象：message_end 落盘时凭身份识别「插话 commit」，
 *  与 prompt 的初始用户消息区分（后者也可能在 Run 激活后才 message_end）。 */
const steeredUserMessages = new WeakSet<object>();
let runtimeRevision = 0;

/** 只作读取期间的 ABA 检测；跨进程重启由客户端连接代次隔离。 */
export function noteDesktopInputRuntimeChange(): void { runtimeRevision += 1; }
export function desktopInputRuntimeRevision(): number { return runtimeRevision; }
export function hasDesktopInputCommitObserver(session: any): boolean { return !!session && installed.has(session); }
export function withDesktopInputCommitted<T>(scope: CommitScope, action: () => T): T {
  return scopeStorage.run(scope, action);
}

/** 消费「该 user 消息是否来自 steer」的一次性标记。message_end(user) 处调用。 */
export function consumeSteeredUserMessage(message: any): boolean {
  if (!message || (typeof message !== 'object' && typeof message !== 'function')) return false;
  if (!steeredUserMessages.has(message)) return false;
  steeredUserMessages.delete(message);
  return true;
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
    if (!scope) return null;
    // 移交证据先于输入形状及关联校验，不能把无法关联误当成没有执行。
    scope.handedOff?.();
    // 运行实例已更换：不能签发旧实例的关联，也不能否认新实例已经接手输入。
    if (scope.session !== session) { scope.unavailable(); return null; }
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
      // steer 身份标记不依赖关联 scope：即使没有输入关联观察（旧嵌入方），
      // 插话落盘时的 Run 切分仍然需要认出这条 user 消息。
      const steeredInput = message
        ?? (Array.isArray(args[0])
          ? args[0].find((item: any) => item?.role === 'user')
          : (args[0]?.role === 'user' ? args[0] : null));
      if (steeredInput) steeredUserMessages.add(steeredInput);
      let result: any;
      try { result = originalSteer.apply(this, args); }
      catch (error) { if (message) pending.delete(message); throw error; }
      // SDK 的底层 agent.steer 同步入队。展示回调发生在入队成功后、上层 async
      // steer 让出执行权前；回调出错也保留后续 user append 的关联观察。
      if (message) pending.get(message)?.queued?.();
      return result;
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
