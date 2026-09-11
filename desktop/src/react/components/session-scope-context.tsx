import { createContext, useContext } from 'react';
import { useStore } from '../stores';

/**
 * session-scope-context.tsx — 「这个输入区属于哪个会话」的组件级覆盖。
 *
 * 主聊天页与侧边对话面板会同时挂载 InputArea / 聊天记录等会话级组件。它们读的
 * 「当前会话」默认是 store.currentSessionPath；侧边面板用 Provider 把子树内的
 * 默认会话改写成侧边会话 path，从而复用同一套组件而不必到处传 prop。
 *
 * 约定：
 * - 显式传入 sessionPath 的调用方永远优先（例如 input-slice 的 *ForSession
 *   动作）；本 context 只提供「未显式指定时用哪个会话」。
 * - Provider 的 value 是**字符串**而不是对象：字符串相等即语义相等，避免每次
 *   渲染新建对象触发整棵子树重渲染。
 * - 侧边会话尚未创建完成时传 null，此时子树按「无会话」渲染（禁用输入），
 *   绝不回落到主会话——回落到主会话会把侧边的输入写进主对话。
 */
export const SessionScopeContext = createContext<string | null | undefined>(undefined);

export function SessionScopeProvider({
  sessionPath,
  children,
}: {
  sessionPath: string | null | undefined;
  children: React.ReactNode;
}) {
  return (
    <SessionScopeContext.Provider value={sessionPath}>{children}</SessionScopeContext.Provider>
  );
}

/**
 * 当前渲染面的会话 path：Provider 覆盖优先，否则回落主会话。
 * 返回的是「该面正在看的会话」，尚未创建完成的侧边会话返回 null。
 */
export function useScopedSessionPath(): string | null {
  const scoped = useContext(SessionScopeContext);
  const current = useStore(s => s.currentSessionPath);
  if (scoped === undefined) return current;
  return scoped;
}

/** 当前渲染面是否为侧边会话面（而不是主聊天页）。 */
export function useIsSideChatScope(): boolean {
  const scoped = useContext(SessionScopeContext);
  return scoped !== undefined && scoped !== null;
}
