export interface RuntimeTodo { id: string; content: string; activeForm?: string; status: 'pending' | 'in_progress' | 'blocked' | 'cancelled' | 'completed'; blockedReason?: string }
const labels = { pending: '待处理', in_progress: '进行中', blocked: '受阻', cancelled: '已取消', completed: '已完成' };
/** 只展示真实会话的完整清单，不在页面推断完成。 */
export function SessionTodoList({ todos }: { todos: RuntimeTodo[] }) {
  return todos.length ? <section aria-label="任务清单"><h3>任务清单</h3><ol>{todos.map(todo => <li key={todo.id} data-todo-status={todo.status}>
    <strong>{labels[todo.status]}</strong>：{todo.status === 'in_progress' ? todo.activeForm || todo.content : todo.content}
    {todo.blockedReason && <p>原因：{todo.blockedReason}</p>}
  </li>)}</ol></section> : null;
}
